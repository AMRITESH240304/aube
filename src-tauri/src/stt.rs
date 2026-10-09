// Transcribes while recording with Apple's on-device SpeechAnalyzer, through the
// `aube-stt` sidecar (stt/main.swift). Each speaker gets their own process: the
// mic is "You" and call audio is "Them", so neither can drown out the other.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::{echo::Echo, err, lid, mixer::OUTPUT_RATE, transcripts::transcript_path, Result};

// Frames arrive as [mic, call], matching this order.
const SPEAKERS: [&str; 2] = ["You", "Them"];

const AUTO: &str = "auto";
const SPEECH_TO_DETECT: usize = 8 * lid::RATE;
const LONGEST_WAIT: usize = 30 * OUTPUT_RATE as usize;
const LEAD_IN: usize = OUTPUT_RATE as usize;
const QUIET: f32 = 0.02;

#[derive(Deserialize)]
#[serde(untagged)]
enum Event {
    Text {
        start: f64,
        end: f64,
        text: String,
        #[serde(rename = "final")]
        is_final: bool,
    },
    Error {
        error: String,
    },
}

#[derive(Default)]
struct Turn {
    speaker: &'static str,
    start: f64,
    end: f64,
    text: String,
}

// Shared with each speaker's reader thread.
struct Live {
    app: AppHandle,
    id: String,
    turns: Mutex<Vec<Turn>>,
    error: Mutex<Option<String>>,
}

impl Live {
    // `pending` is the speaker's phrase still being heard; a final result settles it.
    fn handle(
        &self,
        speaker: &'static str,
        pending: &mut Option<usize>,
        event: Event,
        offset: f64,
    ) {
        let (start, end, text, is_final) = match event {
            Event::Text {
                start,
                end,
                text,
                is_final,
            } => (
                start + offset,
                end + offset,
                text.trim().to_string(),
                is_final,
            ),
            Event::Error { error } => {
                *self.error.lock().unwrap() = Some(error);
                return;
            }
        };
        let mut turns = self.turns.lock().unwrap();
        let index = *pending.get_or_insert_with(|| {
            turns.push(Turn::default());
            turns.len() - 1
        });
        let _ = self.app.emit(
            "live-text",
            json!({ "recordingId": self.id, "index": index, "speaker": speaker, "start": start, "text": text }),
        );
        turns[index] = Turn {
            speaker,
            start,
            end,
            text,
        };
        if is_final {
            *pending = None;
        }
    }

    fn detected(&self, speaker: &str, language: Option<&Language>) {
        let _ = self.app.emit(
            "live-language",
            json!({ "recordingId": self.id, "speaker": speaker, "language": language.map(|l| &l.name) }),
        );
    }
}

struct Stream {
    audio: mpsc::Sender<Vec<f32>>,
    child: Child,
    threads: [JoinHandle<()>; 2],
}

fn sidecar() -> Result<Command> {
    let path = std::env::current_exe()
        .map_err(err)?
        .with_file_name("aube-stt");
    Ok(Command::new(path))
}

// Without a language, the sidecar uses the Mac's.
fn spawn(
    live: &Arc<Live>,
    speaker: &'static str,
    language: Option<&str>,
    offset: f64,
) -> Result<Stream> {
    let mut child = sidecar()?
        .args(language)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start transcription: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("Couldn't start transcription.")?;
    let stdout = child.stdout.take().ok_or("Couldn't start transcription.")?;

    // Writing on its own thread means a slow start (like the first-time model
    // download) never holds up recording. Closing the channel closes stdin,
    // which tells the sidecar to finish.
    let (audio, chunks) = mpsc::channel::<Vec<f32>>();
    let writer = thread::spawn(move || {
        for chunk in chunks {
            let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
            if stdin.write_all(&bytes).is_err() {
                break;
            }
        }
    });
    let live = live.clone();
    let reader = thread::spawn(move || {
        let mut pending = None;
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::io::Result::ok)
        {
            if let Ok(event) = serde_json::from_str(&line) {
                live.handle(speaker, &mut pending, event, offset);
            }
        }
    });
    Ok(Stream {
        audio,
        child,
        threads: [writer, reader],
    })
}

enum Feed {
    Waiting {
        held: Vec<f32>,
        speech: Vec<f32>,
        skipped: usize,
    },
    Streaming(Stream),
    Off,
}

struct Speaker {
    name: &'static str,
    feed: Feed,
}

impl Speaker {
    fn push(&mut self, audio: Vec<f32>, live: &Arc<Live>, languages: &[Language]) {
        match &mut self.feed {
            Feed::Streaming(stream) => {
                let _ = stream.audio.send(audio);
            }
            Feed::Waiting {
                held,
                speech,
                skipped,
            } => {
                if audio.iter().any(|s| s.abs() > QUIET) {
                    speech.extend(lid::downsample(&audio, OUTPUT_RATE as usize));
                }
                held.extend(audio);
                if speech.is_empty() && held.len() > LEAD_IN {
                    let silence = held.len() - LEAD_IN;
                    held.drain(..silence);
                    *skipped += silence;
                }
                if speech.len() >= SPEECH_TO_DETECT || held.len() >= LONGEST_WAIT {
                    self.begin(live, languages);
                }
            }
            Feed::Off => {}
        }
    }

    fn begin(&mut self, live: &Arc<Live>, languages: &[Language]) {
        let Feed::Waiting {
            held,
            speech,
            skipped,
        } = std::mem::replace(&mut self.feed, Feed::Off)
        else {
            return;
        };
        let language = detect(&speech, languages);
        live.detected(self.name, language);
        let offset = skipped as f64 / OUTPUT_RATE as f64;
        match spawn(live, self.name, language.map(|l| l.id.as_str()), offset) {
            Ok(stream) => {
                let _ = stream.audio.send(held);
                self.feed = Feed::Streaming(stream);
            }
            Err(e) => *live.error.lock().unwrap() = Some(e),
        }
    }
}

fn detect<'a>(speech: &[f32], languages: &'a [Language]) -> Option<&'a Language> {
    if speech.len() < lid::RATE {
        return None;
    }
    let codes: Vec<&str> = languages.iter().map(Language::code).collect();
    let code = lid::detector()?.detect(speech, &codes)?;
    languages.iter().find(|l| l.code() == code)
}

pub struct Transcriber {
    live: Arc<Live>,
    speakers: Vec<Speaker>,
    languages: Vec<Language>,
    echo: Echo,
}

impl Transcriber {
    pub fn new(app: AppHandle, id: String, language: Option<String>) -> Result<Self> {
        let live = Arc::new(Live {
            app,
            id,
            turns: Mutex::default(),
            error: Mutex::default(),
        });
        let auto = language.as_deref() == Some(AUTO);
        let languages = if auto {
            thread::spawn(lid::detector);
            languages().unwrap_or_default()
        } else {
            Vec::new()
        };
        let fixed = language.filter(|_| !auto);
        let speakers = SPEAKERS
            .into_iter()
            .map(|name| {
                let feed = if languages.is_empty() {
                    Feed::Streaming(spawn(&live, name, fixed.as_deref(), 0.0)?)
                } else {
                    Feed::Waiting {
                        held: Vec::new(),
                        speech: Vec::new(),
                        skipped: 0,
                    }
                };
                Ok(Speaker { name, feed })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            live,
            speakers,
            languages,
            echo: Echo::new(),
        })
    }

    pub fn push(&mut self, frames: &[[f32; 2]]) {
        if frames.is_empty() {
            return;
        }
        let mic = self.echo.cancel(frames);
        let call = frames.iter().map(|f| f[1]).collect();
        for (speaker, audio) in self.speakers.iter_mut().zip([mic, call]) {
            speaker.push(audio, &self.live, &self.languages);
        }
    }

    pub fn error(&self) -> Option<String> {
        self.live.error.lock().unwrap().clone()
    }

    // Lets each sidecar finish what it heard, then saves `<id>.transcript.json`.
    pub fn finish(mut self, dir: &Path) -> Result<()> {
        for speaker in &mut self.speakers {
            if matches!(&speaker.feed, Feed::Waiting { speech, .. } if !speech.is_empty()) {
                speaker.begin(&self.live, &self.languages);
            }
        }
        let mut children = Vec::new();
        for speaker in self.speakers {
            if let Feed::Streaming(stream) = speaker.feed {
                drop(stream.audio);
                children.push((stream.child, stream.threads));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        for (mut child, threads) in children {
            while child.try_wait().is_ok_and(|status| status.is_none()) {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            for thread in threads {
                let _ = thread.join();
            }
        }

        let mut turns = self.live.turns.lock().unwrap();
        turns.retain(|t| !t.text.is_empty());
        turns.sort_by(|a, b| a.start.total_cmp(&b.start));
        let segments: Vec<_> = turns
            .iter()
            .map(
                |t| json!({ "start": t.start, "end": t.end, "speaker": t.speaker, "text": t.text }),
            )
            .collect();
        let text = turns
            .iter()
            .map(|t| format!("{}: {}", t.speaker, t.text))
            .collect::<Vec<_>>()
            .join("\n\n");
        let transcript = json!({
            "text": text,
            "speech_detected": !segments.is_empty(),
            "segments": segments,
        });
        let bytes = serde_json::to_vec_pretty(&transcript).map_err(err)?;
        std::fs::write(transcript_path(dir, &self.live.id), bytes).map_err(err)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Language {
    id: String,
    name: String,
    installed: bool,
}

impl Language {
    fn code(&self) -> &str {
        self.id.split('-').next().unwrap_or(&self.id)
    }
}

fn languages() -> Result<Vec<Language>> {
    let output = sidecar()?.arg("--languages").output().map_err(err)?;
    serde_json::from_slice(&output.stdout).map_err(err)
}

// Both run off the main thread: downloading a language takes a while.
#[tauri::command(async)]
pub fn transcription_languages() -> Result<Vec<Language>> {
    languages()
}

#[tauri::command(async)]
pub fn install_language(id: String) -> Result<()> {
    let output = sidecar()?.args(["--install", &id]).output().map_err(err)?;
    match serde_json::from_slice(&output.stdout) {
        _ if output.status.success() => Ok(()),
        Ok(Event::Error { error }) => Err(error),
        _ => Err(format!("Couldn't download {id}.")),
    }
}

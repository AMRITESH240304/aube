// Transcribes while recording with Apple's on-device SpeechAnalyzer, through the
// `aube-stt` sidecar (stt/main.swift). Each speaker gets their own process: the
// mic is "You" and call audio is "Them", so neither can drown out the other.

use std::{
    collections::{HashMap, VecDeque},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::{echo::Echo, err, lid, mixer::OUTPUT_RATE, transcripts::transcript_path, Result};

// Frames arrive as [mic, call], matching this order.
const SPEAKERS: [&str; 2] = ["You", "Them"];

const AUTO: &str = "auto";
const CHECK_EVERY: usize = 4 * lid::RATE;
const SPEECH_WINDOW: usize = 8 * lid::RATE;
const REWIND: usize = 8 * OUTPUT_RATE as usize;
const QUIET: f32 = 0.02;
const GUESSES: &str = "detected-languages.json";

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
    stream: usize,
    start: f64,
    end: f64,
    text: String,
}

struct Origin {
    speaker: &'static str,
    stream: usize,
    offset: f64,
    alive: Arc<AtomicBool>,
}

// Shared with each speaker's reader thread.
struct Live {
    app: AppHandle,
    id: String,
    turns: Mutex<Vec<Turn>>,
    error: Mutex<Option<String>>,
    streams: AtomicUsize,
    guesses: Mutex<HashMap<String, String>>,
    guesses_path: Option<PathBuf>,
}

impl Live {
    // `pending` is the speaker's phrase still being heard; a final result settles it.
    fn handle(&self, origin: &Origin, pending: &mut Option<usize>, event: Event) {
        let (start, end, text, is_final) = match event {
            Event::Text {
                start,
                end,
                text,
                is_final,
            } => (
                start + origin.offset,
                end + origin.offset,
                text.trim().to_string(),
                is_final,
            ),
            Event::Error { error } => {
                *self.error.lock().unwrap() = Some(error);
                return;
            }
        };
        let mut turns = self.turns.lock().unwrap();
        if !origin.alive.load(Relaxed) {
            return;
        }
        let index = *pending.get_or_insert_with(|| {
            turns.push(Turn::default());
            turns.len() - 1
        });
        self.emit_text(index, origin.speaker, start, &text);
        turns[index] = Turn {
            speaker: origin.speaker,
            stream: origin.stream,
            start,
            end,
            text,
        };
        if is_final {
            *pending = None;
        }
    }

    fn emit_text(&self, index: usize, speaker: &str, start: f64, text: &str) {
        let _ = self.app.emit(
            "live-text",
            json!({ "recordingId": self.id, "index": index, "speaker": speaker, "start": start, "text": text }),
        );
    }

    fn cut(&self, stream: usize, from: f64) {
        let mut turns = self.turns.lock().unwrap();
        for (index, turn) in turns.iter_mut().enumerate() {
            if turn.stream == stream && turn.start >= from && !turn.text.is_empty() {
                turn.text.clear();
                self.emit_text(index, turn.speaker, turn.start, "");
            }
        }
    }

    fn detected(&self, speaker: &str, language: &Language) {
        let _ = self.app.emit(
            "live-language",
            json!({ "recordingId": self.id, "speaker": speaker, "language": language.name }),
        );
    }

    fn guess(&self, speaker: &str) -> Option<String> {
        self.guesses.lock().unwrap().get(speaker).cloned()
    }

    fn remember(&self, speaker: &str, language: &Language) {
        let mut guesses = self.guesses.lock().unwrap();
        guesses.insert(speaker.to_owned(), language.id.clone());
        if let (Some(path), Ok(bytes)) = (&self.guesses_path, serde_json::to_vec(&*guesses)) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

struct Stream {
    id: usize,
    audio: mpsc::Sender<Vec<f32>>,
    child: Child,
    threads: [JoinHandle<()>; 2],
    alive: Arc<AtomicBool>,
}

impl Stream {
    fn stop(mut self) {
        self.alive.store(false, Relaxed);
        drop(self.audio);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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
    let alive = Arc::new(AtomicBool::new(true));
    let origin = Origin {
        speaker,
        stream: live.streams.fetch_add(1, Relaxed),
        offset,
        alive: alive.clone(),
    };
    let id = origin.stream;
    let live = live.clone();
    let reader = thread::spawn(move || {
        let mut pending = None;
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::io::Result::ok)
        {
            if let Ok(event) = serde_json::from_str(&line) {
                live.handle(&origin, &mut pending, event);
            }
        }
    });
    Ok(Stream {
        id,
        audio,
        child,
        threads: [writer, reader],
        alive,
    })
}

#[derive(Default)]
struct Listening {
    recent: VecDeque<f32>,
    speech: VecDeque<f32>,
    since_check: usize,
    confirmed: bool,
    candidate: Option<String>,
}

impl Listening {
    fn hear(&mut self, audio: &[f32]) {
        self.recent.extend(audio);
        let excess = self.recent.len().saturating_sub(REWIND);
        self.recent.drain(..excess);
        if audio.iter().any(|s| s.abs() > QUIET) {
            let before = self.speech.len();
            self.speech
                .extend(lid::downsample(audio, OUTPUT_RATE as usize));
            self.since_check += self.speech.len() - before;
            let excess = self.speech.len().saturating_sub(SPEECH_WINDOW);
            self.speech.drain(..excess);
        }
    }
}

struct Speaker {
    name: &'static str,
    stream: Option<Stream>,
    language: Option<String>,
    heard: usize,
    listening: Option<Listening>,
}

impl Speaker {
    fn push(&mut self, audio: Vec<f32>, live: &Arc<Live>, languages: &[Language]) {
        self.heard += audio.len();
        let due = self.listening.as_mut().is_some_and(|listening| {
            listening.hear(&audio);
            listening.since_check >= CHECK_EVERY
        });
        if let Some(stream) = &self.stream {
            let _ = stream.audio.send(audio);
        }
        if due {
            self.check(live, languages);
        }
    }

    fn check(&mut self, live: &Arc<Live>, languages: &[Language]) {
        let Some(listening) = &mut self.listening else {
            return;
        };
        listening.since_check = 0;
        let found = detect(listening.speech.make_contiguous(), languages);
        let Some(found) = found else {
            listening.candidate = None;
            return;
        };
        if covers(self.language.as_deref(), found) {
            if !listening.confirmed {
                live.detected(self.name, found);
            }
            listening.confirmed = true;
            listening.candidate = None;
            return;
        }
        if listening.confirmed && listening.candidate.as_deref() != Some(&found.id) {
            listening.candidate = Some(found.id.clone());
            return;
        }
        self.switch(live, found);
    }

    fn switch(&mut self, live: &Arc<Live>, language: &Language) {
        let Some(listening) = &mut self.listening else {
            return;
        };
        let rewind: Vec<f32> = listening.recent.iter().copied().collect();
        let from = (self.heard - rewind.len()) as f64 / OUTPUT_RATE as f64;
        listening.confirmed = true;
        listening.candidate = None;
        if let Some(old) = self.stream.take() {
            let id = old.id;
            old.stop();
            live.cut(id, from);
        }
        live.detected(self.name, language);
        live.remember(self.name, language);
        self.language = Some(language.id.clone());
        match spawn(live, self.name, Some(&language.id), from) {
            Ok(stream) => {
                let _ = stream.audio.send(rewind);
                self.stream = Some(stream);
            }
            Err(e) => *live.error.lock().unwrap() = Some(e),
        }
    }
}

fn covers(current: Option<&str>, found: &Language) -> bool {
    let current = current.map(|id| id.split('-').next().unwrap_or(id));
    current == Some(found.code()) || (current == Some("hi") && found.code() == "en")
}

fn detect<'a>(speech: &[f32], languages: &'a [Language]) -> Option<&'a Language> {
    if speech.len() < lid::RATE {
        return None;
    }
    let codes: Vec<&str> = languages.iter().map(Language::code).collect();
    let code = lid::detector()?.detect(speech, &codes)?;
    languages.iter().find(|l| l.code() == code)
}

fn saved_guesses(path: Option<&Path>) -> HashMap<String, String> {
    path.and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub struct Transcriber {
    live: Arc<Live>,
    speakers: Vec<Speaker>,
    languages: Vec<Language>,
    echo: Echo,
}

impl Transcriber {
    pub fn new(
        app: AppHandle,
        id: String,
        language: Option<String>,
        recorded: usize,
    ) -> Result<Self> {
        let auto = language.as_deref() == Some(AUTO);
        let languages = if auto {
            thread::spawn(lid::detector);
            languages().unwrap_or_default()
        } else {
            Vec::new()
        };
        let listen = !languages.is_empty();
        let guesses_path = app.path().app_data_dir().ok().map(|dir| dir.join(GUESSES));
        let live = Arc::new(Live {
            app,
            id,
            turns: Mutex::default(),
            error: Mutex::default(),
            streams: AtomicUsize::new(0),
            guesses: Mutex::new(saved_guesses(guesses_path.as_deref())),
            guesses_path,
        });
        let speakers = SPEAKERS
            .into_iter()
            .map(|name| {
                let language = if listen {
                    live.guess(name)
                } else {
                    language.clone().filter(|_| !auto)
                };
                let offset = recorded as f64 / OUTPUT_RATE as f64;
                Ok(Speaker {
                    name,
                    stream: Some(spawn(&live, name, language.as_deref(), offset)?),
                    language,
                    heard: recorded,
                    listening: listen.then(Listening::default),
                })
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
    pub fn finish(self, dir: &Path) -> Result<()> {
        let mut children = Vec::new();
        for stream in self.speakers.into_iter().filter_map(|s| s.stream) {
            drop(stream.audio);
            children.push((stream.child, stream.threads));
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

        let path = transcript_path(dir, &self.live.id);
        let mut turns = self.live.turns.lock().unwrap();
        turns.retain(|t| !t.text.is_empty());
        turns.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut segments = saved_segments(&path);
        segments.extend(turns.iter().map(
            |t| json!({ "start": t.start, "end": t.end, "speaker": t.speaker, "text": t.text }),
        ));
        let text = segments
            .iter()
            .filter_map(|segment| {
                let text = segment["text"].as_str()?;
                Some(match segment["speaker"].as_str() {
                    Some(speaker) => format!("{speaker}: {text}"),
                    None => text.to_owned(),
                })
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let transcript = json!({
            "text": text,
            "speech_detected": !segments.is_empty(),
            "segments": segments,
        });
        let bytes = serde_json::to_vec_pretty(&transcript).map_err(err)?;
        std::fs::write(path, bytes).map_err(err)
    }
}

fn saved_segments(path: &Path) -> Vec<Value> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|mut transcript| match transcript["segments"].take() {
            Value::Array(segments) => Some(segments),
            _ => None,
        })
        .unwrap_or_default()
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

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

use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::{echo::Echo, err, transcripts::transcript_path, Result};

// Frames arrive as [mic, call], matching this order.
const SPEAKERS: [&str; 2] = ["You", "Them"];

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
    fn handle(&self, speaker: &'static str, pending: &mut Option<usize>, event: Event) {
        let (start, end, text, is_final) = match event {
            Event::Text {
                start,
                end,
                text,
                is_final,
            } => (start, end, text.trim().to_string(), is_final),
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
}

struct Stream {
    audio: mpsc::Sender<Vec<f32>>,
    child: Child,
    threads: [JoinHandle<()>; 2],
}

fn spawn(live: &Arc<Live>, speaker: &'static str) -> Result<Stream> {
    let sidecar = std::env::current_exe()
        .map_err(err)?
        .with_file_name("aube-stt");
    let mut child = Command::new(sidecar)
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
                live.handle(speaker, &mut pending, event);
            }
        }
    });
    Ok(Stream {
        audio,
        child,
        threads: [writer, reader],
    })
}

pub struct Transcriber {
    live: Arc<Live>,
    streams: Vec<Stream>,
    // Only when there's a call to hear.
    echo: Option<Echo>,
}

impl Transcriber {
    pub fn new(app: AppHandle, id: String, with_call: bool) -> Result<Self> {
        let live = Arc::new(Live {
            app,
            id,
            turns: Mutex::default(),
            error: Mutex::default(),
        });
        let speakers = if with_call {
            &SPEAKERS[..]
        } else {
            &SPEAKERS[..1]
        };
        let streams = speakers
            .iter()
            .map(|speaker| spawn(&live, speaker))
            .collect::<Result<_>>()?;
        Ok(Self {
            live,
            streams,
            echo: with_call.then(Echo::new),
        })
    }

    pub fn push(&mut self, frames: &[[f32; 2]]) {
        if frames.is_empty() {
            return;
        }
        let mic = match &mut self.echo {
            Some(echo) => echo.cancel(frames),
            None => frames.iter().map(|f| f[0]).collect(),
        };
        let call = frames.iter().map(|f| f[1]).collect();
        for (stream, audio) in self.streams.iter().zip([mic, call]) {
            let _ = stream.audio.send(audio);
        }
    }

    pub fn error(&self) -> Option<String> {
        self.live.error.lock().unwrap().clone()
    }

    // Lets each sidecar finish what it heard, then saves `<id>.transcript.json`.
    pub fn finish(self, dir: &Path) -> Result<()> {
        let mut children = Vec::new();
        for stream in self.streams {
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

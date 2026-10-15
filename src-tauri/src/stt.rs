// Transcribes while recording with Apple's on-device SpeechAnalyzer, through the
// `aube-stt` sidecar (stt/main.swift). Each speaker gets their own process: the
// mic is "You" and call audio is "Them", so neither can drown out the other.

use std::{
    collections::{HashMap, VecDeque},
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    echo::Echo,
    err,
    library::{Db, Segment, Word},
    lid,
    mixer::{Decimator, OUTPUT_RATE},
    tape::{Audio, Channel, Tape},
    Result,
};

// Frames arrive as [mic, call], matching this order.
const SPEAKERS: [(&str, Channel); 2] = [("You", Channel::Mic), ("Them", Channel::Call)];

const AUTO: &str = "auto";
const GUESSES: &str = "detected-languages";

const SPEECH_BEFORE_LOCKING: usize = 30 * lid::RATE;
const SPEECH_WORTH_REMEMBERING: usize = 10 * lid::RATE;
const LOCK_CONFIDENCE: f32 = 0.6;
const REWIND: usize = 120 * OUTPUT_RATE as usize;

const VAD_FRAME: usize = lid::RATE * 30 / 1000;
const VAD_MIN_LEVEL: f32 = 0.005;
const VAD_ABOVE_FLOOR: f32 = 3.0;
const VAD_FLOOR_RISE: f32 = 0.001;

#[derive(Deserialize)]
#[serde(untagged)]
enum Event {
    Text {
        start: f64,
        end: f64,
        text: String,
        #[serde(rename = "final")]
        is_final: bool,
        #[serde(default)]
        words: Vec<Word>,
    },
    Error {
        error: String,
    },
}

struct Turn {
    speaker: &'static str,
    channel: Channel,
    stream: usize,
    start: f64,
    end: f64,
    text: String,
    words: Vec<Word>,
}

struct Origin {
    speaker: &'static str,
    channel: Channel,
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
}

impl Live {
    // `pending` is the speaker's phrase still being heard; a final result settles it.
    fn handle(&self, origin: &Origin, pending: &mut Option<usize>, event: Event) {
        let (start, end, text, is_final, words) = match event {
            Event::Text {
                start,
                end,
                text,
                is_final,
                words,
            } => (
                start + origin.offset,
                end + origin.offset,
                text.trim().to_string(),
                is_final,
                words
                    .into_iter()
                    .map(|word| Word {
                        start: word.start.map(|s| s + origin.offset),
                        end: word.end.map(|e| e + origin.offset),
                        text: word.text,
                    })
                    .collect(),
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
        let index = pending.unwrap_or(turns.len());
        self.emit_text(index, origin.speaker, start, &text);
        let turn = Turn {
            speaker: origin.speaker,
            channel: origin.channel,
            stream: origin.stream,
            start,
            end,
            text,
            words,
        };
        if index == turns.len() {
            turns.push(turn);
        } else {
            turns[index] = turn;
        }
        *pending = (!is_final).then_some(index);
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
                turn.words.clear();
                self.emit_text(index, turn.speaker, turn.start, "");
            }
        }
    }

    fn locked(&self, speaker: &str, language: Option<&Language>) {
        let _ = self.app.emit(
            "live-language",
            json!({ "recordingId": self.id, "speaker": speaker, "language": language.map(|l| &l.name) }),
        );
    }

    fn guess(&self, speaker: &str) -> Option<String> {
        self.guesses.lock().unwrap().get(speaker).cloned()
    }

    fn remember(&self, speaker: &str, language: &str) {
        let mut guesses = self.guesses.lock().unwrap();
        guesses.insert(speaker.to_owned(), language.to_owned());
        if let Ok(value) = serde_json::to_string(&*guesses) {
            let _ = self.app.state::<Db>().set_setting(GUESSES, &value);
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
    channel: Channel,
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
        channel,
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

struct Vad {
    floor: f32,
}

impl Default for Vad {
    fn default() -> Self {
        Self {
            floor: VAD_MIN_LEVEL,
        }
    }
}

impl Vad {
    fn voiced(&mut self, frame: &[f32]) -> bool {
        let level = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        self.floor = if level < self.floor {
            level
        } else {
            self.floor + (level - self.floor) * VAD_FLOOR_RISE
        };
        level > (self.floor * VAD_ABOVE_FLOOR).max(VAD_MIN_LEVEL)
    }
}

#[derive(Default)]
struct Detection {
    rewind: VecDeque<f32>,
    decimator: Decimator,
    unframed: Vec<f32>,
    speech: Vec<f32>,
    vad: Vad,
}

impl Detection {
    fn hear(&mut self, audio: &[f32]) {
        self.rewind.extend(audio);
        let excess = self.rewind.len().saturating_sub(REWIND);
        self.rewind.drain(..excess);

        self.unframed.extend(self.decimator.push(audio));
        let whole = self.unframed.len() / VAD_FRAME * VAD_FRAME;
        for frame in self.unframed[..whole].as_chunks::<VAD_FRAME>().0 {
            if self.vad.voiced(frame) {
                self.speech.extend_from_slice(frame);
            }
        }
        self.unframed.drain(..whole);
    }

    fn ready(&self) -> bool {
        self.speech.len() >= SPEECH_BEFORE_LOCKING
    }
}

struct Speaker {
    name: &'static str,
    channel: Channel,
    stream: Option<Stream>,
    tape: Option<Tape>,
    language: Option<String>,
    heard: usize,
    detection: Option<Detection>,
}

impl Speaker {
    fn push(&mut self, audio: Vec<f32>, live: &Arc<Live>, languages: &[Language]) {
        self.heard += audio.len();
        if let Some(tape) = &mut self.tape {
            if tape.write(&audio).is_err() {
                self.tape = None;
            }
        }
        let ready = self.detection.as_mut().is_some_and(|detection| {
            detection.hear(&audio);
            detection.ready()
        });
        if let Some(stream) = &self.stream {
            let _ = stream.audio.send(audio);
        }
        if ready {
            self.lock(live, languages);
        }
    }

    fn lock(&mut self, live: &Arc<Live>, languages: &[Language]) {
        let Some(detection) = self.detection.take() else {
            return;
        };
        if let Some(found) = confident(&detection.speech, languages) {
            if !covers(self.language.as_deref(), found) {
                self.switch(live, found, detection.rewind);
            }
            if let Some(language) = &self.language {
                live.remember(self.name, language);
            }
        }
        let current = self
            .language
            .as_deref()
            .and_then(|id| languages.iter().find(|l| l.id == id));
        live.locked(self.name, current);
    }

    fn switch(&mut self, live: &Arc<Live>, language: &Language, rewind: VecDeque<f32>) {
        let from = (self.heard - rewind.len()) as f64 / OUTPUT_RATE as f64;
        if let Some(old) = self.stream.take() {
            let id = old.id;
            old.stop();
            live.cut(id, from);
        }
        self.language = Some(language.id.clone());
        match spawn(live, self.name, self.channel, Some(&language.id), from) {
            Ok(stream) => {
                let _ = stream.audio.send(rewind.into());
                self.stream = Some(stream);
            }
            Err(e) => *live.error.lock().unwrap() = Some(e),
        }
    }

    fn remember_unlocked(&mut self, live: &Live, languages: &[Language]) {
        let Some(detection) = self.detection.take() else {
            return;
        };
        if detection.speech.len() < SPEECH_WORTH_REMEMBERING {
            return;
        }
        if let Some(found) = confident(&detection.speech, languages) {
            if !covers(self.language.as_deref(), found) {
                live.remember(self.name, &found.id);
            }
        }
    }
}

fn covers(current: Option<&str>, found: &Language) -> bool {
    let current = current.map(|id| id.split('-').next().unwrap_or(id));
    current == Some(found.code()) || (current == Some("hi") && found.code() == "en")
}

fn confident<'a>(speech: &[f32], languages: &'a [Language]) -> Option<&'a Language> {
    let codes: Vec<&str> = languages.iter().map(Language::code).collect();
    let guess = lid::detector()?.detect(speech, &codes)?;
    if guess.confidence < LOCK_CONFIDENCE {
        return None;
    }
    languages.iter().find(|l| l.code() == guess.code)
}

pub fn your_language(app: &AppHandle) -> Option<String> {
    saved_guesses(app).remove(SPEAKERS[0].0)
}

fn saved_guesses(app: &AppHandle) -> HashMap<String, String> {
    app.state::<Db>()
        .setting(GUESSES)
        .and_then(|value| serde_json::from_str(&value).ok())
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
        let detect = !languages.is_empty();
        let live = Arc::new(Live {
            guesses: Mutex::new(saved_guesses(&app)),
            app: app.clone(),
            id: id.clone(),
            turns: Mutex::default(),
            error: Mutex::default(),
            streams: AtomicUsize::new(0),
        });
        let speakers = SPEAKERS
            .into_iter()
            .map(|(name, channel)| {
                let language = if detect {
                    live.guess(name)
                } else {
                    language.clone().filter(|_| !auto)
                };
                let offset = recorded as f64 / OUTPUT_RATE as f64;
                Ok(Speaker {
                    name,
                    channel,
                    stream: Some(spawn(&live, name, channel, language.as_deref(), offset)?),
                    tape: Audio::of(&app)
                        .and_then(|audio| audio.tape(&id, channel, recorded))
                        .inspect_err(|e| eprintln!("not keeping {} audio: {e}", channel.name()))
                        .ok(),
                    language,
                    heard: recorded,
                    detection: detect.then(Detection::default),
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

    pub fn lines(&self, from: f64, to: f64) -> Vec<String> {
        let turns = self.live.turns.lock().unwrap();
        let mut heard: Vec<&Turn> = turns
            .iter()
            .filter(|turn| turn.start >= from && turn.start < to && !turn.text.is_empty())
            .collect();
        heard.sort_by(|a, b| a.start.total_cmp(&b.start));
        heard
            .into_iter()
            .map(|turn| format!("{}: {}", turn.speaker, turn.text))
            .collect()
    }

    pub fn error(&self) -> Option<String> {
        self.live.error.lock().unwrap().clone()
    }

    pub fn finish(mut self) -> Result<()> {
        let mut children = Vec::new();
        for speaker in &mut self.speakers {
            speaker.remember_unlocked(&self.live, &self.languages);
            if let Some(tape) = speaker.tape.take() {
                let _ = tape.close();
            }
            if let Some(stream) = speaker.stream.take() {
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
        turns.sort_by(|a, b| a.start.total_cmp(&b.start));
        let segments: Vec<Segment> = turns
            .drain(..)
            .filter(|turn| !turn.text.is_empty())
            .map(|turn| Segment {
                speaker: Some(turn.speaker.to_owned()),
                start: turn.start,
                end: turn.end,
                text: turn.text,
                source: Some(turn.channel),
                words: turn.words,
            })
            .collect();
        self.live
            .app
            .state::<Db>()
            .add_segments(&self.live.id, &segments)
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

#[cfg(test)]
#[path = "../tests/unit/stt.rs"]
mod tests;

use std::{
    collections::HashSet,
    io::Write,
    process::{Command, Stdio},
    sync::{Mutex, MutexGuard},
    thread::{self, JoinHandle},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    err,
    library::{Db, Segment},
    Result,
};

pub const BATCH_SECS: f64 = 300.0;
const SETTLE_SECS: f64 = 10.0;
const SHORTEST_GAP: f64 = 1.0;
const SAME_POINT: f64 = 0.75;
const LIST_MARKERS: [&str; 3] = ["- ", "1. ", "> "];

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Topic {
    pub heading: String,
    pub points: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Reply<T> {
    Done(T),
    Failed { error: String },
}

#[derive(Deserialize)]
struct Topics {
    topics: Vec<Topic>,
}

#[derive(Deserialize)]
struct Combined {
    overview: String,
    sections: Vec<Placed>,
}

#[derive(Deserialize)]
struct Placed {
    heading: String,
    topics: Vec<usize>,
    notes: Vec<usize>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Human,
    Ai,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub origin: Origin,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Section {
    pub heading: Line,
    pub lines: Vec<Line>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Summary {
    pub overview: String,
    pub sections: Vec<Section>,
}

#[derive(Default, Debug, PartialEq)]
struct Notes {
    lines: Vec<String>,
    headings: Vec<String>,
}

fn one_at_a_time() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Batches {
    app: AppHandle,
    recording: String,
    next: f64,
    running: Vec<JoinHandle<()>>,
}

impl Batches {
    pub fn new(app: AppHandle, recording: String, recorded_secs: f64) -> Self {
        Self {
            app,
            recording,
            next: recorded_secs,
            running: Vec::new(),
        }
    }

    pub fn tick(&mut self, heard_secs: f64, transcript: impl FnOnce(f64, f64) -> Vec<String>) {
        let (start, end) = (self.next, self.next + BATCH_SECS);
        if heard_secs < end + SETTLE_SECS {
            return;
        }
        let lines = transcript(start, end);
        let (app, recording) = (self.app.clone(), self.recording.clone());
        self.running.push(thread::spawn(move || {
            let _one_at_a_time = one_at_a_time();
            if let Err(e) = summarise_batch(&app, &recording, start, end, lines) {
                eprintln!("couldn't summarise {start}-{end}s of {recording}: {e}");
            }
        }));
        self.next = end;
    }

    pub fn finish(self) {
        write(self.app, self.recording, self.running);
    }
}

pub fn write(app: AppHandle, recording: String, running: Vec<JoinHandle<()>>) {
    thread::spawn(move || {
        for batch in running {
            let _ = batch.join();
        }
        let _one_at_a_time = one_at_a_time();
        announce(&app, &recording, true, None);
        let error = summarise(&app, &recording).err();
        announce(&app, &recording, false, error);
    });
}

fn announce(app: &AppHandle, recording: &str, working: bool, error: Option<String>) {
    let _ = app.emit(
        "summary",
        json!({ "recordingId": recording, "working": working, "error": error }),
    );
}

fn ask<T: DeserializeOwned>(mode: &str, request: &Value) -> Result<T> {
    let program = std::env::current_exe()
        .map_err(err)?
        .with_file_name("aube-notes");
    let mut child = Command::new(program)
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start the summary: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("Couldn't start the summary.")?
        .write_all(request.to_string().as_bytes())
        .map_err(err)?;
    let output = child.wait_with_output().map_err(err)?;
    let reply = String::from_utf8_lossy(&output.stdout);
    let last = reply.lines().last().unwrap_or_default();
    match serde_json::from_str(last).map_err(|_| "Couldn't write the summary.".to_string())? {
        Reply::Done(value) => Ok(value),
        Reply::Failed { error } => Err(error),
    }
}

fn summarise_batch(
    app: &AppHandle,
    recording: &str,
    start: f64,
    end: f64,
    transcript: Vec<String>,
) -> Result<()> {
    let db = app.state::<Db>();
    let topics = if transcript.is_empty() {
        Vec::new()
    } else {
        let notes = read_notes(db.notes_between(recording, start, end)?).lines;
        ask::<Topics>(
            "batch",
            &json!({ "transcript": transcript, "notes": notes }),
        )?
        .topics
    };
    let topics = serde_json::to_string(&topics).map_err(err)?;
    db.add_batch(recording, start, end, &topics)
}

fn summarise(app: &AppHandle, recording: &str) -> Result<()> {
    let db = app.state::<Db>();
    let (title, duration) = db.recording(recording)?;
    let segments = db.segments(recording)?;
    let covered: Vec<(f64, f64)> = db
        .batches(recording)?
        .into_iter()
        .map(|(start, end, _)| (start, end))
        .collect();
    for (start, end) in gaps(&covered, duration) {
        summarise_batch(
            app,
            recording,
            start,
            end,
            transcript_between(&segments, start, end),
        )?;
    }
    let topics: Vec<Topic> = db
        .batches(recording)?
        .into_iter()
        .flat_map(|(_, _, topics)| serde_json::from_str::<Vec<Topic>>(&topics).unwrap_or_default())
        .collect();
    if topics.is_empty() {
        return Ok(());
    }
    let notes = read_notes(db.note_lines(recording)?);
    let combined: Combined = ask(
        "combine",
        &json!({ "title": title, "topics": topics, "notes": notes.lines, "headings": notes.headings }),
    )?;
    let summary = assemble(combined, &topics, &notes);
    db.set_summary(recording, &serde_json::to_string(&summary).map_err(err)?)
}

fn gaps(covered: &[(f64, f64)], duration: f64) -> Vec<(f64, f64)> {
    let mut sorted = covered.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut missing = Vec::new();
    let mut reached = 0.0;
    for (start, end) in sorted.into_iter().chain([(duration, duration)]) {
        if start - reached >= SHORTEST_GAP {
            let mut from = reached;
            while from < start {
                let to = (from + BATCH_SECS).min(start);
                missing.push((from, to));
                from = to;
            }
        }
        reached = f64::max(reached, end);
    }
    missing
}

fn transcript_between(segments: &[Segment], start: f64, end: f64) -> Vec<String> {
    segments
        .iter()
        .filter(|segment| segment.start >= start && segment.start < end && !segment.text.is_empty())
        .map(|segment| {
            let speaker = segment.speaker.as_deref().unwrap_or("Someone");
            format!("{speaker}: {}", segment.text)
        })
        .collect()
}

fn read_notes(lines: Vec<String>) -> Notes {
    let mut notes = Notes::default();
    for line in lines {
        let line = line.trim();
        if line.starts_with('#') {
            notes
                .headings
                .push(line.trim_start_matches('#').trim().to_owned());
        } else if !line.is_empty() {
            let text = LIST_MARKERS
                .iter()
                .find_map(|marker| line.strip_prefix(marker))
                .unwrap_or(line);
            notes.lines.push(text.trim().to_owned());
        }
    }
    notes
}

fn claim(indices: Vec<usize>, used: &mut [bool]) -> Vec<usize> {
    indices
        .into_iter()
        .filter(|&i| i < used.len() && !std::mem::replace(&mut used[i], true))
        .collect()
}

fn words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() > 2)
        .map(str::to_lowercase)
        .collect()
}

fn repeats(point: &str, note: &str) -> bool {
    let (point, note) = (words(point), words(note));
    let shorter = point.len().min(note.len());
    shorter > 0 && point.intersection(&note).count() as f64 / shorter as f64 >= SAME_POINT
}

fn assemble(combined: Combined, topics: &[Topic], notes: &Notes) -> Summary {
    let mut topics_used = vec![false; topics.len()];
    let mut notes_used = vec![false; notes.lines.len()];
    let mut sections: Vec<(String, Vec<usize>, Vec<usize>)> = combined
        .sections
        .into_iter()
        .map(|placed| {
            let topic_ids = claim(placed.topics, &mut topics_used);
            let note_ids = claim(placed.notes, &mut notes_used);
            (placed.heading, topic_ids, note_ids)
        })
        .collect();
    for (i, topic) in topics.iter().enumerate() {
        if !topics_used[i] {
            sections.push((topic.heading.clone(), vec![i], Vec::new()));
        }
    }
    let unplaced: Vec<usize> = (0..notes.lines.len()).filter(|&i| !notes_used[i]).collect();
    if !unplaced.is_empty() {
        match sections.first_mut() {
            Some(first) => first.2.extend(unplaced),
            None => sections.push(("Notes".to_owned(), Vec::new(), unplaced)),
        }
    }

    let sections = sections
        .into_iter()
        .filter(|(_, topic_ids, note_ids)| !topic_ids.is_empty() || !note_ids.is_empty())
        .map(|(heading, topic_ids, note_ids)| {
            let mine: Vec<&str> = note_ids.iter().map(|&i| notes.lines[i].as_str()).collect();
            let theirs = topic_ids
                .iter()
                .flat_map(|&i| &topics[i].points)
                .filter(|point| !mine.iter().any(|note| repeats(point, note)));
            let origin = if notes
                .headings
                .iter()
                .any(|h| h.eq_ignore_ascii_case(&heading))
            {
                Origin::Human
            } else {
                Origin::Ai
            };
            Section {
                heading: Line {
                    text: heading,
                    origin,
                },
                lines: mine
                    .iter()
                    .map(|text| Line {
                        text: (*text).to_owned(),
                        origin: Origin::Human,
                    })
                    .chain(theirs.map(|text| Line {
                        text: text.clone(),
                        origin: Origin::Ai,
                    }))
                    .collect(),
            }
        })
        .collect();
    Summary {
        overview: combined.overview,
        sections,
    }
}

#[tauri::command]
pub fn summary(db: State<'_, Db>, id: String) -> Option<Summary> {
    db.summary(&id)
        .and_then(|summary| serde_json::from_str(&summary).ok())
}

#[tauri::command]
pub fn summarize(app: AppHandle, id: String) {
    write(app, id, Vec::new());
}

#[cfg(test)]
#[path = "../tests/unit/summary.rs"]
mod tests;

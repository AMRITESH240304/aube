use std::{collections::HashMap, process::Command, sync::Mutex, thread};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{path::BaseDirectory, AppHandle, Emitter, Manager};

use crate::{
    err,
    library::{Db, Segment, Word},
    recorder::Recorder,
    tape::{Audio, Channel},
    Result,
};

const CALL_SPEECH_FOR_ONLINE: f64 = 15.0;
const MIC_SPEECH_FOR_IN_PERSON: f64 = 60.0;
const MOST_CALL_SPEECH_IN_PERSON: f64 = 5.0;
const LEAST_DIARIZED_SPEECH: f64 = 30.0;
const SHORTEST_TURN: f64 = 0.25;
const NEAREST_TURN: f64 = 1.0;

const VOICEPRINT: &str = "voiceprint";
const VOICEPRINT_MODEL: &str = "fluidaudio-0.17.7-offline";
const ENROLL_LEAST_SPEECH: f64 = 20.0;
const ENROLL_DOMINANT_SHARE: f64 = 0.8;
const ENROLL_ENOUGH_SPEECH: f64 = 600.0;
const SELF_MATCH: f32 = 0.5;
const SAME_VOICE: f32 = 0.5;
const MOST_ATTEMPTS: u32 = 3;

type Voices = [(String, Vec<f32>)];

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
#[serde(untagged)]
enum Reply {
    Done(Diarization),
    Failed { error: String },
}

#[derive(Deserialize)]
struct Diarization {
    segments: Vec<Turn>,
    voices: HashMap<String, Vec<f32>>,
}

#[derive(Deserialize, Clone)]
struct Turn {
    speaker: String,
    start: f64,
    end: f64,
}

#[derive(Serialize, Deserialize)]
struct Voiceprint {
    model: String,
    voice: Vec<f32>,
    seconds: f64,
}

impl Diarization {
    fn speech_by_speaker(&self) -> Vec<(&str, f64)> {
        let mut speech: Vec<(&str, f64)> = Vec::new();
        for turn in &self.segments {
            match speech
                .iter_mut()
                .find(|(speaker, _)| *speaker == turn.speaker)
            {
                Some((_, seconds)) => *seconds += turn.end - turn.start,
                None => speech.push((&turn.speaker, turn.end - turn.start)),
            }
        }
        speech
    }

    fn total_speech(&self) -> f64 {
        self.segments.iter().map(|turn| turn.end - turn.start).sum()
    }

    fn dominant(&self) -> Option<(&str, f64)> {
        self.speech_by_speaker()
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    fn distinct(&self) -> bool {
        self.speech_by_speaker().len() >= 2 && self.total_speech() >= LEAST_DIARIZED_SPEECH
    }

    fn names(&self, you: Option<&str>, known: &Voices) -> HashMap<String, String> {
        let distinct = self.distinct();
        let mut names = HashMap::new();
        if !distinct && known.is_empty() {
            return names;
        }
        if let Some(you) = you {
            names.insert(you.to_owned(), "You".to_owned());
        }
        let mut pairs: Vec<(f32, &str, &str)> = self
            .voices
            .iter()
            .flat_map(|(speaker, voice)| {
                known.iter().map(move |(name, old)| {
                    (similarity(voice, old), speaker.as_str(), name.as_str())
                })
            })
            .filter(|(score, _, _)| *score >= SAME_VOICE)
            .collect();
        pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, speaker, name) in pairs {
            if !names.contains_key(speaker) && !names.values().any(|taken| taken == name) {
                names.insert(speaker.to_owned(), name.to_owned());
            }
        }
        if distinct {
            let mut last = known
                .iter()
                .filter_map(|(name, _)| name.strip_prefix("Speaker ")?.parse::<u32>().ok())
                .max()
                .unwrap_or(0);
            for (speaker, _) in self.speech_by_speaker() {
                if !names.contains_key(speaker) {
                    last += 1;
                    names.insert(speaker.to_owned(), format!("Speaker {last}"));
                }
            }
        }
        names
    }
}

pub fn identify(app: AppHandle, recording: String) {
    thread::spawn(move || {
        let _one_at_a_time = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        announce(&app, &recording, true);
        let finished = match relabel_recording(&app, &recording) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("couldn't tell speakers apart in {recording}: {e}");
                give_up(&app, &recording)
            }
        };
        let recording_now = app.state::<Recorder>().elapsed(&recording).is_some();
        if finished && !recording_now {
            if let Ok(audio) = Audio::of(&app) {
                audio.remove(&recording);
            }
        }
        announce(&app, &recording, false);
    });
}

pub fn identify_waiting(app: &AppHandle) {
    let waiting = Audio::of(app)
        .map(|audio| audio.waiting())
        .unwrap_or_default();
    for recording in waiting {
        identify(app.clone(), recording);
    }
}

fn give_up(app: &AppHandle, recording: &str) -> bool {
    let Ok(audio) = Audio::of(app) else {
        return true;
    };
    let mut pending = audio.pending(recording).unwrap_or_default();
    pending.attempts += 1;
    pending.attempts >= MOST_ATTEMPTS || audio.set_pending(recording, pending).is_err()
}

fn announce(app: &AppHandle, recording: &str, working: bool) {
    let _ = app.emit(
        "speakers",
        json!({ "recordingId": recording, "working": working }),
    );
}

fn relabel_recording(app: &AppHandle, recording: &str) -> Result<()> {
    let from = Audio::of(app)?
        .pending(recording)
        .unwrap_or_default()
        .start_secs();
    let db = app.state::<Db>();
    let segments = db.segments(recording)?;
    let speech = |channel| -> f64 {
        segments
            .iter()
            .filter(|s| s.source == Some(channel) && s.start >= from)
            .map(|s| s.end - s.start)
            .sum()
    };
    let (mic, call) = (speech(Channel::Mic), speech(Channel::Call));
    let (channel, diarization, you) = if call >= CALL_SPEECH_FOR_ONLINE {
        if mic >= ENROLL_LEAST_SPEECH {
            if let Err(e) = enroll(app, recording, from) {
                eprintln!("couldn't learn your voice from {recording}: {e}");
            }
        }
        (
            Channel::Call,
            diarize(app, recording, Channel::Call, from)?,
            None,
        )
    } else if mic >= MIC_SPEECH_FOR_IN_PERSON && call <= MOST_CALL_SPEECH_IN_PERSON {
        let room = diarize(app, recording, Channel::Mic, from)?;
        let you = find_you(&room, voiceprint(app).as_ref());
        (Channel::Mic, room, you)
    } else {
        return Ok(());
    };
    let known = db.voices(recording, channel)?;
    let names = diarization.names(you.as_deref(), &known);
    let relabeled = relabel(&segments, channel, &diarization, &names, from);
    db.replace_segments(recording, segments.len(), &relabeled)?;
    let voices: Vec<(String, Vec<f32>)> = names
        .iter()
        .filter_map(|(speaker, name)| {
            Some((name.clone(), diarization.voices.get(speaker)?.clone()))
        })
        .collect();
    db.add_voices(recording, channel, &voices)
}

fn relabel(
    segments: &[Segment],
    channel: Channel,
    diarization: &Diarization,
    names: &HashMap<String, String>,
    from: f64,
) -> Vec<Segment> {
    let fallback = match channel {
        Channel::Mic => "You",
        Channel::Call => "Them",
    };
    segments
        .iter()
        .flat_map(|segment| {
            if segment.source == Some(channel) && segment.start >= from {
                split(segment, &diarization.segments, names, fallback)
            } else {
                vec![segment.clone()]
            }
        })
        .collect()
}

fn speaker_at(turns: &[Turn], start: f64, end: f64) -> Option<&str> {
    let overlap = |turn: &Turn| end.min(turn.end) - start.max(turn.start);
    let gap = |turn: &Turn| (turn.start - end).max(start - turn.end);
    turns
        .iter()
        .filter(|turn| overlap(turn) > 0.0)
        .max_by(|a, b| overlap(a).total_cmp(&overlap(b)))
        .or_else(|| {
            turns
                .iter()
                .filter(|turn| gap(turn) <= NEAREST_TURN)
                .min_by(|a, b| gap(a).total_cmp(&gap(b)))
        })
        .map(|turn| turn.speaker.as_str())
}

fn split(
    segment: &Segment,
    turns: &[Turn],
    names: &HashMap<String, String>,
    fallback: &str,
) -> Vec<Segment> {
    let name = |speaker: Option<&str>| {
        speaker
            .and_then(|speaker| names.get(speaker))
            .map_or(fallback, String::as_str)
            .to_owned()
    };
    let whole = || {
        vec![Segment {
            speaker: Some(name(speaker_at(turns, segment.start, segment.end))),
            ..segment.clone()
        }]
    };
    if segment.words.is_empty() {
        return whole();
    }
    let mut speakers: Vec<Option<&str>> = segment
        .words
        .iter()
        .map(|word| match (word.start, word.end) {
            (Some(start), Some(end)) => speaker_at(turns, start, end),
            _ => None,
        })
        .collect();
    fill_gaps(&mut speakers);
    smooth_single_words(&mut speakers);
    if speakers.iter().all(Option::is_none) {
        return whole();
    }

    let mut pieces: Vec<(Option<&str>, Vec<Word>)> = Vec::new();
    for (word, speaker) in segment.words.iter().zip(speakers) {
        match pieces.last_mut() {
            Some((last, words)) if *last == speaker => words.push(word.clone()),
            _ => pieces.push((speaker, vec![word.clone()])),
        }
    }
    pieces
        .into_iter()
        .filter_map(|(speaker, words)| {
            let text = words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<String>()
                .trim()
                .to_owned();
            (!text.is_empty()).then(|| Segment {
                speaker: Some(name(speaker)),
                start: words
                    .iter()
                    .find_map(|word| word.start)
                    .unwrap_or(segment.start),
                end: words
                    .iter()
                    .rev()
                    .find_map(|word| word.end)
                    .unwrap_or(segment.end),
                text,
                source: segment.source,
                words,
            })
        })
        .collect()
}

fn fill_gaps(speakers: &mut [Option<&str>]) {
    let mut previous = None;
    for speaker in speakers.iter_mut() {
        match speaker {
            Some(_) => previous = *speaker,
            None => *speaker = previous,
        }
    }
    let first = speakers.iter().find_map(|speaker| *speaker);
    for speaker in speakers.iter_mut().take_while(|speaker| speaker.is_none()) {
        *speaker = first;
    }
}

fn smooth_single_words(speakers: &mut [Option<&str>]) {
    for i in 1..speakers.len().saturating_sub(1) {
        if speakers[i - 1] == speakers[i + 1] {
            speakers[i] = speakers[i - 1];
        }
    }
}

fn diarize(app: &AppHandle, recording: &str, channel: Channel, from: f64) -> Result<Diarization> {
    let audio = Audio::of(app)?.path(recording, channel);
    if !audio.exists() {
        return Err(format!("no {} audio was kept", channel.name()));
    }
    let program = std::env::current_exe()
        .map_err(err)?
        .with_file_name("aube-diarizer");
    let models = app
        .path()
        .resolve("models/diarizer", BaseDirectory::Resource)
        .map_err(err)?;
    let output = Command::new(program)
        .arg(models)
        .arg(audio)
        .output()
        .map_err(err)?;
    let reply = String::from_utf8_lossy(&output.stdout);
    let last = reply.lines().last().unwrap_or_default();
    match serde_json::from_str(last).map_err(err)? {
        Reply::Done(mut diarization) => {
            diarization
                .segments
                .retain(|turn| turn.end - turn.start >= SHORTEST_TURN);
            for turn in &mut diarization.segments {
                turn.start += from;
                turn.end += from;
            }
            diarization
                .segments
                .sort_by(|a, b| a.start.total_cmp(&b.start));
            Ok(diarization)
        }
        Reply::Failed { error } => Err(error),
    }
}

fn normalized(voice: &[f32]) -> Vec<f32> {
    let length = voice.iter().map(|x| x * x).sum::<f32>().sqrt();
    if length == 0.0 {
        return voice.to_vec();
    }
    voice.iter().map(|x| x / length).collect()
}

fn similarity(a: &[f32], b: &[f32]) -> f32 {
    normalized(a)
        .iter()
        .zip(normalized(b))
        .map(|(x, y)| x * y)
        .sum()
}

fn voiceprint(app: &AppHandle) -> Option<Voiceprint> {
    app.state::<Db>()
        .setting(VOICEPRINT)
        .and_then(|value| serde_json::from_str::<Voiceprint>(&value).ok())
        .filter(|print| print.model == VOICEPRINT_MODEL)
}

fn find_you(room: &Diarization, voiceprint: Option<&Voiceprint>) -> Option<String> {
    match voiceprint {
        Some(print) => room
            .voices
            .iter()
            .map(|(speaker, voice)| (speaker, similarity(voice, &print.voice)))
            .filter(|(_, score)| *score >= SELF_MATCH)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(speaker, _)| speaker.clone()),
        None => room.dominant().map(|(speaker, _)| speaker.to_owned()),
    }
}

fn enroll(app: &AppHandle, recording: &str, from: f64) -> Result<()> {
    let known = voiceprint(app);
    if known
        .as_ref()
        .is_some_and(|print| print.seconds >= ENROLL_ENOUGH_SPEECH)
    {
        return Ok(());
    }
    let mic = diarize(app, recording, Channel::Mic, from)?;
    let total = mic.total_speech();
    let Some((speaker, seconds)) = mic.dominant() else {
        return Ok(());
    };
    if seconds < ENROLL_LEAST_SPEECH || seconds / total < ENROLL_DOMINANT_SHARE {
        return Ok(());
    }
    let Some(voice) = mic.voices.get(speaker) else {
        return Ok(());
    };
    let print = learn(known, voice, seconds);
    let value = serde_json::to_string(&print).map_err(err)?;
    app.state::<Db>().set_setting(VOICEPRINT, &value)
}

fn learn(known: Option<Voiceprint>, voice: &[f32], seconds: f64) -> Voiceprint {
    let voice = normalized(voice);
    let Some(known) = known else {
        return Voiceprint {
            model: VOICEPRINT_MODEL.to_owned(),
            voice,
            seconds,
        };
    };
    let total = known.seconds + seconds;
    let blended: Vec<f32> = known
        .voice
        .iter()
        .zip(&voice)
        .map(|(old, new)| (old * known.seconds as f32 + new * seconds as f32) / total as f32)
        .collect();
    Voiceprint {
        model: VOICEPRINT_MODEL.to_owned(),
        voice: normalized(&blended),
        seconds: total,
    }
}

#[cfg(test)]
#[path = "../tests/unit/speakers.rs"]
mod tests;

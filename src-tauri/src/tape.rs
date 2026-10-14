use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{
    err,
    mixer::{Decimator, OUTPUT_RATE, SPEECH_RATE},
    Result,
};

const BYTES_PER_SAMPLE: u64 = 2;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Channel {
    Mic,
    Call,
}

impl Channel {
    pub const ALL: [Channel; 2] = [Channel::Mic, Channel::Call];

    pub fn name(self) -> &'static str {
        match self {
            Channel::Mic => "mic",
            Channel::Call => "call",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|channel| channel.name() == name)
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug)]
pub struct Pending {
    pub start_frame: u64,
    pub attempts: u32,
}

impl Pending {
    pub fn start_secs(&self) -> f64 {
        self.start_frame as f64 / OUTPUT_RATE as f64
    }
}

pub struct Audio {
    dir: PathBuf,
}

impl Audio {
    pub fn of(app: &AppHandle) -> Result<Self> {
        Self::at(app.path().app_data_dir().map_err(err)?.join("audio"))
    }

    fn at(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir).map_err(err)?;
        Ok(Self { dir })
    }

    pub fn path(&self, recording: &str, channel: Channel) -> PathBuf {
        self.dir.join(format!("{recording}.{}.pcm", channel.name()))
    }

    fn marker(&self, recording: &str) -> PathBuf {
        self.dir.join(format!("{recording}.json"))
    }

    pub fn pending(&self, recording: &str) -> Option<Pending> {
        if let Ok(data) = fs::read(self.marker(recording)) {
            return Some(serde_json::from_slice(&data).unwrap_or_default());
        }
        let kept = Channel::ALL
            .into_iter()
            .any(|channel| self.path(recording, channel).exists());
        kept.then(Pending::default)
    }

    pub fn set_pending(&self, recording: &str, pending: Pending) -> Result<()> {
        let data = serde_json::to_vec(&pending).map_err(err)?;
        fs::write(self.marker(recording), data).map_err(err)
    }

    pub fn waiting(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let recordings: BTreeSet<String> = entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter_map(|name| name.split('.').next().map(str::to_owned))
            .filter(|recording| !recording.is_empty())
            .collect();
        recordings.into_iter().collect()
    }

    pub fn remove(&self, recording: &str) {
        for channel in Channel::ALL {
            let _ = fs::remove_file(self.path(recording, channel));
        }
        let _ = fs::remove_file(self.marker(recording));
    }

    pub fn tape(&self, recording: &str, channel: Channel, recorded: usize) -> Result<Tape> {
        let pending = match self.pending(recording) {
            Some(pending) => pending,
            None => {
                let fresh = Pending {
                    start_frame: recorded as u64,
                    attempts: 0,
                };
                self.set_pending(recording, fresh)?;
                fresh
            }
        };
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(recording, channel))
            .map_err(err)?;
        let kept_frames = (recorded as u64).saturating_sub(pending.start_frame);
        let expected = kept_frames * SPEECH_RATE as u64 / OUTPUT_RATE as u64 * BYTES_PER_SAMPLE;
        let length = file.metadata().map_err(err)?.len();
        if length > expected {
            file.set_len(expected).map_err(err)?;
        }
        let mut tape = Tape {
            file: BufWriter::new(file),
            decimator: Decimator::default(),
        };
        let missing = expected.saturating_sub(length) / BYTES_PER_SAMPLE;
        tape.write_samples(std::iter::repeat_n(0.0, missing as usize))?;
        Ok(tape)
    }
}

pub struct Tape {
    file: BufWriter<File>,
    decimator: Decimator,
}

impl Tape {
    pub fn write(&mut self, audio: &[f32]) -> Result<()> {
        let speech = self.decimator.push(audio);
        self.write_samples(speech.into_iter())
    }

    fn write_samples(&mut self, samples: impl Iterator<Item = f32>) -> Result<()> {
        for sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            self.file.write_all(&value.to_le_bytes()).map_err(err)?;
        }
        Ok(())
    }

    pub fn close(mut self) -> Result<()> {
        self.file.flush().map_err(err)
    }
}

#[cfg(test)]
#[path = "../tests/unit/tape.rs"]
mod tests;

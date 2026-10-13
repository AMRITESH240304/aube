use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
};

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

pub fn path(app: &AppHandle, recording: &str, channel: Channel) -> Result<PathBuf> {
    let dir = app.path().app_data_dir().map_err(err)?.join("audio");
    fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir.join(format!("{recording}.{}.pcm", channel.name())))
}

pub fn remove(app: &AppHandle, recording: &str) {
    for channel in Channel::ALL {
        if let Ok(path) = path(app, recording, channel) {
            let _ = fs::remove_file(path);
        }
    }
}

pub struct Tape {
    file: BufWriter<File>,
    decimator: Decimator,
}

impl Tape {
    pub fn open(
        app: &AppHandle,
        recording: &str,
        channel: Channel,
        recorded: usize,
    ) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path(app, recording, channel)?)
            .map_err(err)?;
        let expected = recorded as u64 * SPEECH_RATE as u64 / OUTPUT_RATE as u64 * BYTES_PER_SAMPLE;
        let length = file.metadata().map_err(err)?.len();
        if length > expected {
            file.set_len(expected).map_err(err)?;
        }
        let mut tape = Self {
            file: BufWriter::new(file),
            decimator: Decimator::default(),
        };
        let missing = expected.saturating_sub(length) / BYTES_PER_SAMPLE;
        tape.write_samples(std::iter::repeat_n(0.0, missing as usize))?;
        Ok(tape)
    }

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

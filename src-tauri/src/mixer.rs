use std::{collections::VecDeque, time::Duration};

use cpal::StreamInstant;

pub const OUTPUT_RATE: u32 = 48_000;
pub const SPEECH_RATE: u32 = 16_000;
const DECIMATION: usize = (OUTPUT_RATE / SPEECH_RATE) as usize;

// How far one source may run ahead before the other is treated as silent.
const MAX_LAG: usize = OUTPUT_RATE as usize / 2;
// Audio captured this much later than its track has reached means the source
// started late or stopped delivering for a while.
const GAP: u64 = OUTPUT_RATE as u64 / 20;

#[derive(Clone, Copy)]
pub enum Source {
    Mic,
    System,
}

pub struct Chunk {
    pub source: Source,
    pub samples: Vec<f32>,
    // Both sources stamp capture times from the same system clock.
    pub captured: StreamInstant,
    // First chunk after a pause: the paused time is skipped, not filled.
    pub resumed: bool,
}

struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Resampler {
    fn new(rate: u32) -> Self {
        Self {
            step: rate as f64 / OUTPUT_RATE as f64,
            pos: 0.0,
            prev: 0.0,
        }
    }

    // Linear interpolation; `prev` carries the last sample across chunks.
    fn push(&mut self, input: &[f32], out: &mut VecDeque<f32>) {
        let prev = self.prev;
        let at = |i: f64| if i < 0.0 { prev } else { input[i as usize] };
        while self.pos < input.len() as f64 - 1.0 {
            let i = self.pos.floor();
            let (a, b) = (at(i), at(i + 1.0));
            out.push_back(a + (b - a) * (self.pos - i) as f32);
            self.pos += self.step;
        }
        self.pos -= input.len() as f64;
        self.prev = input.last().copied().unwrap_or(prev);
    }
}

#[derive(Default)]
pub struct Decimator {
    leftover: Vec<f32>,
}

impl Decimator {
    pub fn push(&mut self, audio: &[f32]) -> Vec<f32> {
        self.leftover.extend_from_slice(audio);
        let whole = self.leftover.len() / DECIMATION * DECIMATION;
        let out = self.leftover[..whole]
            .as_chunks::<DECIMATION>()
            .0
            .iter()
            .map(|frame| frame.iter().sum::<f32>() / DECIMATION as f32)
            .collect();
        self.leftover.drain(..whole);
        out
    }
}

fn frames(time: Duration) -> u64 {
    (time.as_secs_f64() * OUTPUT_RATE as f64) as u64
}

struct Track {
    resampler: Resampler,
    queue: VecDeque<f32>,
    // Output samples this track has reached, silence filled into gaps included.
    position: u64,
    // Paused time, which isn't part of the recording.
    skipped: Duration,
}

impl Track {
    fn new(rate: u32) -> Self {
        Self {
            resampler: Resampler::new(rate),
            queue: VecDeque::new(),
            position: 0,
            skipped: Duration::ZERO,
        }
    }

    // Places audio by when it was captured, so both sources stay in step even
    // when one starts late or goes quiet, which would otherwise shift it
    // against the other for the rest of the recording.
    fn push(&mut self, chunk: &Chunk, origin: StreamInstant) {
        let since = chunk.captured.duration_since(origin);
        if chunk.resumed {
            let reached = Duration::from_secs_f64(self.position as f64 / OUTPUT_RATE as f64);
            self.skipped = since.saturating_sub(reached);
        }
        let due = frames(since.saturating_sub(self.skipped));
        if due > self.position + GAP {
            let gap = (due - self.position) as usize;
            self.queue.extend(std::iter::repeat_n(0.0, gap));
            self.position = due;
        }
        let before = self.queue.len();
        self.resampler.push(&chunk.samples, &mut self.queue);
        self.position += (self.queue.len() - before) as u64;
    }

    // A source that has nothing yet counts as silent, and that silence counts
    // toward where it has reached.
    fn pop(&mut self) -> f32 {
        self.queue.pop_front().unwrap_or_else(|| {
            self.position += 1;
            0.0
        })
    }
}

pub struct Mixer {
    origin: Option<StreamInstant>,
    mic: Track,
    system: Option<Track>,
    drained: u64,
}

impl Mixer {
    pub fn new(mic_rate: u32, system_rate: Option<u32>) -> Self {
        Self {
            origin: None,
            mic: Track::new(mic_rate),
            system: system_rate.map(Track::new),
            drained: 0,
        }
    }

    pub fn set_system_rate(&mut self, rate: u32) {
        match &mut self.system {
            Some(track) => track.resampler = Resampler::new(rate),
            None => {
                let mut track = Track::new(rate);
                track.position = self.drained;
                self.system = Some(track);
            }
        }
    }

    pub fn push(&mut self, chunk: Chunk) {
        let origin = *self.origin.get_or_insert(chunk.captured);
        let track = match chunk.source {
            Source::Mic => Some(&mut self.mic),
            Source::System => self.system.as_mut(),
        };
        if let Some(track) = track {
            track.push(&chunk, origin);
        }
    }

    // Pairs up what both sources have delivered as [mic, system] frames;
    // `all` flushes everything left.
    pub fn drain(&mut self, all: bool) -> Vec<[f32; 2]> {
        let mic = self.mic.queue.len();
        let n = match self.system.as_ref().map(|t| t.queue.len()) {
            Some(system) if !all => mic.min(system).max(mic.max(system).saturating_sub(MAX_LAG)),
            Some(system) => mic.max(system),
            None => mic,
        };
        self.drained += n as u64;
        (0..n)
            .map(|_| [self.mic.pop(), self.system.as_mut().map_or(0.0, Track::pop)])
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/mixer.rs"]
mod tests;

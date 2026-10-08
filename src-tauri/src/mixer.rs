use std::{collections::VecDeque, time::Duration};

use cpal::StreamInstant;

pub const OUTPUT_RATE: u32 = 48_000;

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
}

impl Mixer {
    pub fn new(mic_rate: u32, system_rate: Option<u32>) -> Self {
        Self {
            origin: None,
            mic: Track::new(mic_rate),
            system: system_rate.map(Track::new),
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
        (0..n)
            .map(|_| [self.mic.pop(), self.system.as_mut().map_or(0.0, Track::pop)])
            .collect()
    }
}

pub fn mix([mic, system]: [f32; 2]) -> i16 {
    ((mic + system).clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    // 10 ms chunks of silence with a click at `click` seconds, as the device
    // would deliver them between `from` and `to` seconds.
    fn chunks(source: Source, from: f64, to: f64, click: f64) -> Vec<Chunk> {
        let mut out = Vec::new();
        let mut t = from;
        while t < to - 1e-9 {
            let mut samples = vec![0.0; 480];
            let at = ((click - t) * OUTPUT_RATE as f64).round();
            if (0.0..480.0).contains(&at) {
                samples[at as usize] = 1.0;
            }
            let captured = StreamInstant::from_secs_f64(100.0 + t);
            out.push(Chunk {
                source,
                samples,
                captured,
                resumed: false,
            });
            t += 0.01;
        }
        out
    }

    #[test]
    fn late_and_interrupted_sources_stay_in_step() {
        let mut mixer = Mixer::new(OUTPUT_RATE, Some(OUTPUT_RATE));
        let mut mic = chunks(Source::Mic, 0.0, 10.0, 7.0).into_iter().peekable();
        // The call starts half a second late and goes quiet from 3 s to 5 s.
        let mut call = chunks(Source::System, 0.5, 3.0, 7.0)
            .into_iter()
            .chain(chunks(Source::System, 5.0, 10.0, 7.0))
            .peekable();
        let mut out = Vec::new();
        while mic.peek().is_some() || call.peek().is_some() {
            let take_mic = match (mic.peek(), call.peek()) {
                (Some(m), Some(c)) => m.captured <= c.captured,
                (m, _) => m.is_some(),
            };
            mixer.push(if take_mic { mic.next() } else { call.next() }.unwrap());
            out.extend(mixer.drain(false));
        }
        out.extend(mixer.drain(true));
        // Within a sample or two: the resampler holds one back.
        let find = |i: usize| out.iter().position(|f| f[i] > 0.5).unwrap() as i64;
        assert!((find(0) - find(1)).abs() <= 2, "{} vs {}", find(0), find(1));
        assert!((find(0) - 7 * OUTPUT_RATE as i64).abs() <= 2);
    }

    #[test]
    fn pauses_are_cut_not_filled() {
        let mut mixer = Mixer::new(OUTPUT_RATE, None);
        for chunk in chunks(Source::Mic, 0.0, 1.0, 9.0) {
            mixer.push(chunk);
        }
        let mut after = chunks(Source::Mic, 4.0, 5.0, 4.5);
        after[0].resumed = true;
        for chunk in after {
            mixer.push(chunk);
        }
        let out = mixer.drain(true);
        assert!((out.len() as i64 - 2 * OUTPUT_RATE as i64).abs() <= 2);
        let click = out.iter().position(|f| f[0] > 0.5).unwrap() as i64;
        assert!((click - OUTPUT_RATE as i64 * 3 / 2).abs() <= 2);
    }
}

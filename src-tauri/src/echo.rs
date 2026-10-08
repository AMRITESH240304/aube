// On speakers, the mic also hears the call, so the other side would be
// transcribed twice: once as themselves and again as "You". WebRTC's echo
// canceller (AEC3) removes the call from the mic, using the call audio we
// already capture as the reference.

use sonora::{config::EchoCanceller, AudioProcessing, Config, StreamConfig};

use crate::mixer::OUTPUT_RATE;

// The canceller works in 10 ms frames.
const FRAME: usize = OUTPUT_RATE as usize / 100;

pub struct Echo {
    apm: AudioProcessing,
    pending: Vec<[f32; 2]>,
}

impl Echo {
    pub fn new() -> Self {
        let stream = StreamConfig::new(OUTPUT_RATE, 1);
        let apm = AudioProcessing::builder()
            .config(Config {
                echo_canceller: Some(EchoCanceller::default()),
                ..Default::default()
            })
            .capture_config(stream)
            .render_config(stream)
            .build();
        Self {
            apm,
            pending: Vec::new(),
        }
    }

    // Takes [mic, call] frames and returns the mic without the call, in whole
    // 10 ms frames; the remainder waits for the next chunk.
    pub fn cancel(&mut self, frames: &[[f32; 2]]) -> Vec<f32> {
        self.pending.extend_from_slice(frames);
        let whole = self.pending.len() / FRAME * FRAME;
        let mut cleaned = vec![0.0; whole];
        let mut played = [0.0; FRAME];
        for (chunk, out) in self.pending[..whole]
            .chunks(FRAME)
            .zip(cleaned.chunks_mut(FRAME))
        {
            let mic: Vec<f32> = chunk.iter().map(|f| f[0]).collect();
            let call: Vec<f32> = chunk.iter().map(|f| f[1]).collect();
            let _ = self.apm.process_render_f32(&[&call], &mut [&mut played]);
            if self.apm.process_capture_f32(&[&mic], &mut [out]).is_err() {
                out.copy_from_slice(&mic);
            }
        }
        self.pending.drain(..whole);
        cleaned
    }
}

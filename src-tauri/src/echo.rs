// On speakers, the mic also hears the call, so the other side would be
// transcribed twice: once as themselves and again as "You". WebRTC's echo
// canceller (AEC3) removes the call from the mic, using the call audio we
// already capture as the reference.

use sonora_aec3::{block::Block, block_processor::BlockProcessor, config::EchoCanceller3Config};

use crate::mixer::{OUTPUT_RATE, SPEECH_RATE};

const STEP: usize = (OUTPUT_RATE / SPEECH_RATE) as usize;
const BLOCK: usize = 64;
const SCALE: f32 = 32768.0;
const TRANSPARENCY: f32 = 16.0;

pub struct Echo {
    aec: BlockProcessor,
    pending: Vec<[f32; 2]>,
    last: f32,
}

impl Echo {
    pub fn new() -> Self {
        let mut config = EchoCanceller3Config::default();
        let tunings = &mut config.suppressor;
        for tuning in [&mut tunings.normal_tuning, &mut tunings.nearend_tuning] {
            for mask in [&mut tuning.mask_lf, &mut tuning.mask_hf] {
                mask.enr_transparent *= TRANSPARENCY;
                mask.enr_suppress *= TRANSPARENCY;
            }
        }
        Self {
            aec: BlockProcessor::new(&config, SPEECH_RATE as usize, 1, 1),
            pending: Vec::new(),
            last: 0.0,
        }
    }

    // Takes [mic, call] frames and returns the mic without the call, in whole
    // blocks; the remainder waits for the next chunk.
    pub fn cancel(&mut self, frames: &[[f32; 2]]) -> Vec<f32> {
        self.pending.extend_from_slice(frames);
        let whole = self.pending.len() / (BLOCK * STEP) * (BLOCK * STEP);
        let mut cleaned = Vec::with_capacity(whole);
        let mut mic = Block::new(1, 1);
        let mut call = Block::new(1, 1);
        for chunk in self.pending[..whole].chunks(BLOCK * STEP) {
            let down = |side: usize, block: &mut Block| {
                for (out, step) in block.view_mut(0, 0).iter_mut().zip(chunk.chunks(STEP)) {
                    *out = step.iter().map(|f| f[side]).sum::<f32>() * SCALE / STEP as f32;
                }
            };
            down(0, &mut mic);
            down(1, &mut call);
            self.aec.buffer_render(&call);
            self.aec.process_capture(false, false, None, &mut mic);
            for &sample in mic.view(0, 0) {
                let sample = sample / SCALE;
                cleaned.extend(
                    (1..=STEP).map(|i| self.last + (sample - self.last) * i as f32 / STEP as f32),
                );
                self.last = sample;
            }
        }
        self.pending.drain(..whole);
        cleaned
    }
}

#[cfg(test)]
#[path = "../tests/unit/echo.rs"]
mod tests;

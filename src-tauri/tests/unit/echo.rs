use super::*;

const SECOND: usize = OUTPUT_RATE as usize;

fn voice(seconds: usize, seed: u64, syllables: f32) -> Vec<f32> {
    let mut state = seed;
    let mut noise = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as f32 / u64::MAX as f32 * 2.0 - 1.0
    };
    let (mut low, mut pitch, mut phase) = (0.0, 0.0, 0.0);
    (0..seconds * SECOND)
        .map(|i| {
            low = 0.97 * low + 0.03 * noise();
            if i % (SECOND / 10) == 0 {
                pitch = 0.003 + 0.01 * (noise() * 0.5 + 0.5);
            }
            phase += pitch;
            let envelope = (i as f32 / SECOND as f32 * syllables * std::f32::consts::PI)
                .sin()
                .abs();
            (low * 5.0 + phase.sin() + (2.0 * phase).sin() * 0.5) * envelope
        })
        .collect()
}

fn rms(audio: &[f32]) -> f32 {
    (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn keeps_your_voice_while_the_call_talks_and_drops_the_call_alone() {
    let them = voice(20, 7, 1.2);
    let you: Vec<f32> = voice(20, 99, 1.9)
        .iter()
        .enumerate()
        .map(|(i, s)| if i >= 12 * SECOND { s * 0.05 } else { 0.0 })
        .collect();
    let delay = SECOND / 25;
    let speaker: Vec<f32> = them.iter().map(|s| (s * 0.6).tanh() * 0.25).collect();
    let heard = |i: usize, lag: usize, gain: f32| {
        i.checked_sub(delay + lag).map_or(0.0, |j| speaker[j] * gain)
    };
    let mic: Vec<f32> = (0..you.len())
        .map(|i| you[i] + heard(i, 0, 0.5) + heard(i, 111, 0.25) + heard(i, 480, 0.15))
        .collect();
    let call: Vec<f32> = them.iter().map(|s| s * 0.15).collect();

    let mut echo = Echo::new();
    let frames: Vec<[f32; 2]> = mic.iter().zip(&call).map(|(m, c)| [*m, *c]).collect();
    let cleaned: Vec<f32> = frames.chunks(SECOND / 10).flat_map(|c| echo.cancel(c)).collect();

    let alone = 8 * SECOND..12 * SECOND;
    assert!(rms(&cleaned[alone.clone()]) < rms(&mic[alone]) * 0.05);

    let both = 14 * SECOND..19 * SECOND;
    let kept = (0..SECOND / 50)
        .map(|lag| {
            let out = &cleaned[both.start + lag..both.end + lag];
            dot(out, &you[both.clone()]) / dot(&you[both.clone()], &you[both.clone()])
        })
        .fold(f32::MIN, f32::max);
    assert!(kept > 0.7, "kept only {kept:.2} of your voice");
}

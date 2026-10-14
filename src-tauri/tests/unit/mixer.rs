use super::*;

// 10 ms chunks of silence with a click at `click` seconds, as the device
// would deliver them between `from` and `to` seconds.
fn chunks(source: Source, from: f64, to: f64, click: f64) -> Vec<Chunk> {
    chunks_at(OUTPUT_RATE, source, from, to, click)
}

fn chunks_at(rate: u32, source: Source, from: f64, to: f64, click: f64) -> Vec<Chunk> {
    let len = rate as usize / 100;
    let mut out = Vec::new();
    let mut t = from;
    while t < to - 1e-9 {
        let mut samples = vec![0.0; len];
        let at = ((click - t) * rate as f64).round();
        if (0.0..len as f64).contains(&at) {
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

fn play(mixer: &mut Mixer, mic: Vec<Chunk>, call: Vec<Chunk>) -> Vec<[f32; 2]> {
    let mut chunks: Vec<Chunk> = mic.into_iter().chain(call).collect();
    chunks.sort_by_key(|chunk| chunk.captured);
    let mut out = Vec::new();
    for chunk in chunks {
        mixer.push(chunk);
        out.extend(mixer.drain(false));
    }
    out.extend(mixer.drain(true));
    out
}

fn click(out: &[[f32; 2]], source: usize) -> i64 {
    out.iter().position(|f| f[source] > 0.5).unwrap() as i64
}

#[test]
fn call_audio_arriving_mid_recording_at_another_rate_stays_in_step() {
    let mut mixer = Mixer::new(OUTPUT_RATE, None);
    let mut out = play(&mut mixer, chunks(Source::Mic, 0.0, 4.0, 9.0), Vec::new());
    mixer.set_system_rate(44_100);
    out.extend(play(
        &mut mixer,
        chunks(Source::Mic, 4.0, 10.0, 7.0),
        chunks_at(44_100, Source::System, 4.0, 10.0, 7.0),
    ));
    assert!((click(&out, 0) - click(&out, 1)).abs() <= 2);
    assert!((click(&out, 0) - 7 * OUTPUT_RATE as i64).abs() <= 2);
}

#[test]
fn late_and_interrupted_sources_stay_in_step() {
    let mut mixer = Mixer::new(OUTPUT_RATE, Some(OUTPUT_RATE));
    // The call starts half a second late and goes quiet from 3 s to 5 s.
    let call = chunks(Source::System, 0.5, 3.0, 7.0)
        .into_iter()
        .chain(chunks(Source::System, 5.0, 10.0, 7.0))
        .collect();
    let out = play(&mut mixer, chunks(Source::Mic, 0.0, 10.0, 7.0), call);
    assert!((click(&out, 0) - click(&out, 1)).abs() <= 2);
    assert!((click(&out, 0) - 7 * OUTPUT_RATE as i64).abs() <= 2);
}

#[test]
fn decimating_uneven_chunks_loses_no_samples() {
    let mut decimator = Decimator::default();
    let total: usize = [1, 7, 480, 2, 1000]
        .iter()
        .map(|&n| decimator.push(&vec![0.5; n]).len())
        .sum();
    assert_eq!(total, (1 + 7 + 480 + 2 + 1000) / 3);
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

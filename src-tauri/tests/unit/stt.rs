use super::*;

fn tone(level: f32, frames: usize) -> Vec<f32> {
    (0..frames * VAD_FRAME)
        .map(|i| level * (i as f32 * 0.3).sin())
        .collect()
}

#[test]
fn vad_keeps_speech_over_steady_background_noise() {
    let mut vad = Vad::default();
    let noise = tone(0.01, 200);
    let noisy = noise
        .as_chunks::<VAD_FRAME>()
        .0
        .iter()
        .filter(|frame| vad.voiced(&frame[..]))
        .count();
    let speech = tone(0.2, 20);
    let voiced = speech
        .as_chunks::<VAD_FRAME>()
        .0
        .iter()
        .filter(|frame| vad.voiced(&frame[..]))
        .count();
    assert!(noisy < 5, "{noisy} noise frames counted as speech");
    assert_eq!(voiced, 20);
}

#[test]
fn detection_needs_thirty_seconds_of_speech_not_of_time() {
    let mut detection = Detection::default();
    detection.hear(&vec![0.0; 60 * OUTPUT_RATE as usize]);
    assert!(!detection.ready());
    let syllable = OUTPUT_RATE as usize / 5;
    let pause = OUTPUT_RATE as usize / 10;
    let speech: Vec<f32> = (0..50 * OUTPUT_RATE as usize)
        .map(|i| {
            let speaking = i % (syllable + pause) < syllable;
            if speaking {
                0.2 * (i as f32 * 0.1).sin()
            } else {
                0.0
            }
        })
        .collect();
    detection.hear(&speech);
    assert!(detection.ready());
    assert!(detection.rewind.len() <= REWIND);
}

fn language(id: &str) -> Language {
    Language {
        id: id.into(),
        name: id.into(),
        installed: true,
    }
}

#[test]
fn hindi_covers_english_but_not_the_other_way() {
    assert!(covers(Some("hi-IN"), &language("en-US")));
    assert!(covers(Some("en-IN"), &language("en-US")));
    assert!(!covers(Some("en-US"), &language("hi-IN")));
    assert!(!covers(None, &language("en-US")));
}

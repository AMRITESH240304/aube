use super::*;

const SECOND: usize = OUTPUT_RATE as usize;
const SPEECH_SECOND: u64 = SPEECH_RATE as u64 * BYTES_PER_SAMPLE;

fn audio(name: &str) -> Audio {
    let dir = std::env::temp_dir().join(format!("aube-tape-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    Audio::at(dir).unwrap()
}

fn record(audio: &Audio, recorded: usize, seconds: usize) {
    for channel in Channel::ALL {
        let mut tape = audio.tape("r", channel, recorded).unwrap();
        tape.write(&vec![0.1; seconds * SECOND]).unwrap();
        tape.close().unwrap();
    }
}

fn length(audio: &Audio) -> u64 {
    fs::metadata(audio.path("r", Channel::Mic)).unwrap().len()
}

#[test]
fn a_continued_recording_appends_while_its_audio_is_kept() {
    let audio = audio("kept");
    record(&audio, 0, 10);
    record(&audio, 10 * SECOND, 5);
    assert_eq!(length(&audio), 15 * SPEECH_SECOND);
    assert_eq!(audio.pending("r").unwrap().start_frame, 0);
}

#[test]
fn a_continued_recording_starts_fresh_once_earlier_audio_is_removed() {
    let audio = audio("removed");
    record(&audio, 0, 10);
    assert_eq!(audio.waiting(), ["r"]);
    audio.remove("r");
    assert!(audio.pending("r").is_none());
    assert!(audio.waiting().is_empty());

    record(&audio, 600 * SECOND, 5);
    assert_eq!(length(&audio), 5 * SPEECH_SECOND);
    assert_eq!(audio.pending("r").unwrap().start_secs(), 600.0);
}

#[test]
fn a_failed_attempt_is_remembered() {
    let audio = audio("attempts");
    record(&audio, 0, 1);
    let mut pending = audio.pending("r").unwrap();
    pending.attempts += 1;
    audio.set_pending("r", pending).unwrap();
    assert_eq!(audio.pending("r").unwrap().attempts, 1);
}

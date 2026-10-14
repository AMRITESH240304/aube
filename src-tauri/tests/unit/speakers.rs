use super::*;

fn turn(speaker: &str, start: f64, end: f64) -> Turn {
    Turn {
        speaker: speaker.into(),
        start,
        end,
    }
}

fn word(text: &str, start: f64, end: f64) -> Word {
    Word {
        text: text.into(),
        start: Some(start),
        end: Some(end),
    }
}

fn diarization(turns: Vec<Turn>) -> Diarization {
    Diarization {
        segments: turns,
        voices: HashMap::from([("S1".into(), vec![1.0, 0.0]), ("S2".into(), vec![0.0, 1.0])]),
    }
}

fn call_segment(words: Vec<Word>) -> Segment {
    Segment {
        speaker: Some("Them".into()),
        start: words[0].start.unwrap(),
        end: words.last().unwrap().end.unwrap(),
        text: words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<String>()
            .trim()
            .into(),
        source: Some(Channel::Call),
        words,
    }
}

#[test]
fn a_phrase_spanning_two_voices_is_split_between_them() {
    let call = diarization(vec![turn("S1", 0.0, 20.0), turn("S2", 20.0, 40.0)]);
    let segments = [call_segment(vec![
        word("Sounds", 18.0, 18.5),
        word(" good.", 18.5, 19.0),
        word(" Engineering", 20.2, 20.8),
        word(" is", 20.8, 21.0),
        word(" done.", 21.0, 21.5),
    ])];
    let relabeled = relabel(&segments, Channel::Call, &call, &call.names(None, &[]), 0.0);
    let lines: Vec<(&str, &str)> = relabeled
        .iter()
        .map(|s| (s.speaker.as_deref().unwrap(), s.text.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            ("Speaker 1", "Sounds good."),
            ("Speaker 2", "Engineering is done.")
        ]
    );
    assert_eq!(relabeled[1].start, 20.2);
}

#[test]
fn one_stray_word_does_not_split_a_phrase() {
    let call = diarization(vec![
        turn("S1", 0.0, 20.0),
        turn("S2", 20.0, 20.4),
        turn("S1", 20.4, 40.0),
    ]);
    let segments = [call_segment(vec![
        word("We", 19.5, 19.8),
        word(" ship", 20.0, 20.3),
        word(" Friday.", 20.5, 21.0),
    ])];
    let relabeled = relabel(&segments, Channel::Call, &call, &call.names(None, &[]), 0.0);
    assert_eq!(relabeled.len(), 1);
    assert_eq!(relabeled[0].speaker.as_deref(), Some("Speaker 1"));
}

#[test]
fn a_single_voice_keeps_the_channel_name() {
    let call = diarization(vec![turn("S1", 0.0, 60.0)]);
    let segments = [call_segment(vec![word("Hello", 1.0, 1.5)])];
    let relabeled = relabel(&segments, Channel::Call, &call, &call.names(None, &[]), 0.0);
    assert_eq!(relabeled[0].speaker.as_deref(), Some("Them"));
}

#[test]
fn your_voice_is_found_in_the_room_by_voiceprint() {
    let room = diarization(vec![turn("S1", 0.0, 50.0), turn("S2", 50.0, 60.0)]);
    let print = Voiceprint {
        model: VOICEPRINT_MODEL.into(),
        voice: vec![0.1, 0.9],
        seconds: 120.0,
    };
    assert_eq!(find_you(&room, Some(&print)).as_deref(), Some("S2"));
    assert_eq!(find_you(&room, None).as_deref(), Some("S1"));
    let names = room.names(Some("S2"), &[]);
    assert_eq!(names["S2"], "You");
    assert_eq!(names["S1"], "Speaker 1");
}

#[test]
fn a_continued_recording_keeps_earlier_names_and_relabels_only_new_lines() {
    let known = [
        ("Speaker 1".to_owned(), vec![1.0, 0.0]),
        ("Speaker 2".to_owned(), vec![0.0, 1.0]),
    ];
    let later = Diarization {
        segments: vec![turn("S1", 100.0, 120.0), turn("S2", 120.0, 140.0)],
        voices: HashMap::from([
            ("S1".into(), vec![0.1, 0.95]),
            ("S2".into(), vec![-0.7, 0.7]),
        ]),
    };
    let names = later.names(None, &known);
    assert_eq!(names["S1"], "Speaker 2");
    assert_eq!(names["S2"], "Speaker 3");

    let mut earlier = call_segment(vec![word("Hello", 10.0, 10.5)]);
    earlier.speaker = Some("Speaker 1".into());
    let segments = [earlier, call_segment(vec![word("Welcome", 101.0, 101.5)])];
    let relabeled = relabel(&segments, Channel::Call, &later, &names, 100.0);
    let speakers: Vec<&str> = relabeled
        .iter()
        .map(|s| s.speaker.as_deref().unwrap())
        .collect();
    assert_eq!(speakers, ["Speaker 1", "Speaker 2"]);
}

#[test]
fn a_short_continuation_still_matches_a_known_voice() {
    let known = [("Speaker 2".to_owned(), vec![0.0, 1.0])];
    let short = Diarization {
        segments: vec![turn("S1", 0.0, 10.0)],
        voices: HashMap::from([("S1".into(), vec![0.0, 1.0])]),
    };
    assert_eq!(short.names(None, &known)["S1"], "Speaker 2");
    assert!(short.names(None, &[]).is_empty());
}

#[test]
fn learning_blends_voices_by_how_long_each_was_heard() {
    let first = learn(None, &[2.0, 0.0], 30.0);
    assert_eq!(first.voice, [1.0, 0.0]);
    let second = learn(Some(first), &[0.0, 1.0], 30.0);
    assert!((second.voice[0] - second.voice[1]).abs() < 1e-6);
    assert_eq!(second.seconds, 60.0);
}

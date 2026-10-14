use super::*;

fn topic(heading: &str, points: &[&str]) -> Topic {
    Topic {
        heading: heading.into(),
        points: points.iter().map(|&p| p.into()).collect(),
    }
}

fn placed(heading: &str, topics: &[usize], notes: &[usize]) -> Placed {
    Placed {
        heading: heading.into(),
        topics: topics.to_vec(),
        notes: notes.to_vec(),
    }
}

fn lines(section: &Section) -> Vec<(&str, Origin)> {
    section
        .lines
        .iter()
        .map(|line| (line.text.as_str(), line.origin))
        .collect()
}

#[test]
fn notes_keep_their_words_and_headings_are_kept_apart() {
    let notes = read_notes(vec![
        "# Budget".into(),
        "- Ask finance for Q3 numbers".into(),
        "1. Hire two engineers".into(),
        "> quote".into(),
        "   ".into(),
    ]);
    assert_eq!(notes.headings, ["Budget"]);
    assert_eq!(
        notes.lines,
        ["Ask finance for Q3 numbers", "Hire two engineers", "quote"]
    );
}

#[test]
fn your_lines_come_first_and_points_that_repeat_them_are_dropped() {
    let topics = [topic(
        "Hiring",
        &[
            "The team will hire two engineers.",
            "Interviews start next week.",
        ],
    )];
    let notes = Notes {
        lines: vec!["Hire two engineers".into()],
        headings: vec!["hiring".into()],
    };
    let combined = Combined {
        overview: "Hiring plans.".into(),
        sections: vec![placed("Hiring", &[0], &[0])],
    };
    let summary = assemble(combined, &topics, &notes);
    let section = &summary.sections[0];
    assert_eq!(section.heading.origin, Origin::Human);
    assert_eq!(
        lines(section),
        [
            ("Hire two engineers", Origin::Human),
            ("Interviews start next week.", Origin::Ai),
        ]
    );
}

#[test]
fn nothing_is_lost_when_the_model_skips_or_repeats_indices() {
    let topics = [
        topic("Launch", &["Ship Friday."]),
        topic("Budget", &["Spend less."]),
    ];
    let notes = Notes {
        lines: vec!["Call Sam".into(), "Book room".into()],
        headings: Vec::new(),
    };
    let combined = Combined {
        overview: String::new(),
        sections: vec![placed("Launch", &[0, 0, 9], &[1, 1])],
    };
    let summary = assemble(combined, &topics, &notes);
    let headings: Vec<&str> = summary
        .sections
        .iter()
        .map(|s| s.heading.text.as_str())
        .collect();
    assert_eq!(headings, ["Launch", "Budget"]);
    assert_eq!(summary.sections[0].heading.origin, Origin::Ai);
    assert_eq!(
        lines(&summary.sections[0]),
        [
            ("Book room", Origin::Human),
            ("Call Sam", Origin::Human),
            ("Ship Friday.", Origin::Ai),
        ]
    );
}

#[test]
fn only_uncovered_time_is_batched_again() {
    assert_eq!(
        gaps(&[], 650.0),
        [(0.0, 300.0), (300.0, 600.0), (600.0, 650.0)]
    );
    assert_eq!(
        gaps(&[(300.0, 600.0), (0.0, 300.0)], 600.5),
        Vec::<(f64, f64)>::new()
    );
    assert_eq!(gaps(&[(0.0, 300.0)], 420.0), [(300.0, 420.0)]);
    assert_eq!(
        gaps(&[(0.0, 300.0), (500.0, 800.0)], 800.0),
        [(300.0, 500.0)]
    );
}

#[test]
fn a_batch_reads_only_its_own_window() {
    let segment = |speaker: &str, start: f64, text: &str| Segment {
        speaker: Some(speaker.into()),
        start,
        end: start + 1.0,
        text: text.into(),
        source: None,
        words: Vec::new(),
    };
    let segments = [
        segment("You", 10.0, "Before"),
        segment("Speaker 1", 300.0, "Inside"),
        segment("You", 599.0, ""),
        segment("You", 600.0, "After"),
    ];
    assert_eq!(
        transcript_between(&segments, 300.0, 600.0),
        ["Speaker 1: Inside"]
    );
}

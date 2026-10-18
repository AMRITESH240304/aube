use super::*;

fn memory_db() -> Db {
    let connection = Connection::open_in_memory().unwrap();
    migrate(&connection).unwrap();
    Db(Mutex::new(connection))
}

#[test]
fn segments_keep_their_channel_and_words() {
    let db = memory_db();
    db.create_recording("r", "Meeting", 0).unwrap();
    let word = Word {
        text: "hello".into(),
        start: Some(1.0),
        end: Some(1.4),
    };
    let segment = |speaker: &str, source| Segment {
        speaker: Some(speaker.into()),
        start: 1.0,
        end: 1.4,
        text: "hello".into(),
        source,
        words: vec![word.clone()],
    };
    db.add_segments("r", &[segment("Speaker 1", Some(tape::Channel::Call))])
        .unwrap();
    db.connection()
        .execute(
            "INSERT INTO segments (recording_id, speaker, start_secs, end_secs, text)
             VALUES ('r', 'You', 2, 3, 'older')",
            [],
        )
        .unwrap();

    let saved = db.segments("r").unwrap();
    assert_eq!(saved[0].source, Some(tape::Channel::Call));
    assert_eq!(saved[0].words, std::slice::from_ref(&word));
    assert_eq!(saved[1].source, Some(tape::Channel::Mic));
    assert!(saved[1].words.is_empty());

    db.replace_segments("r", 1, &[segment("You", Some(tape::Channel::Mic))])
        .unwrap();
    assert_eq!(db.segments("r").unwrap().len(), 2);
    db.replace_segments("r", 2, &[segment("You", Some(tape::Channel::Mic))])
        .unwrap();
    assert_eq!(db.segments("r").unwrap().len(), 1);
}

#[test]
fn speaker_voices_keep_the_first_print_and_go_with_the_recording() {
    let db = memory_db();
    db.create_recording("r", "Meeting", 0).unwrap();
    let call = tape::Channel::Call;
    db.add_voices("r", call, &[("Speaker 1".into(), vec![1.0, 0.0])])
        .unwrap();
    db.add_voices("r", call, &[("Speaker 1".into(), vec![0.0, 1.0])])
        .unwrap();
    assert_eq!(
        db.voices("r", call).unwrap(),
        [("Speaker 1".to_owned(), vec![1.0, 0.0])]
    );
    assert!(db.voices("r", tape::Channel::Mic).unwrap().is_empty());
    db.connection()
        .execute("DELETE FROM recordings WHERE id = 'r'", [])
        .unwrap();
    assert!(db.voices("r", call).unwrap().is_empty());
}

fn line(id: &str, text: &str) -> Line {
    Line {
        id: id.into(),
        text: text.into(),
    }
}

fn edits(db: &Db) -> Vec<(String, String, String, String)> {
    let connection = db.connection();
    let mut query = connection
        .prepare("SELECT block_id, kind, before, after FROM note_edits ORDER BY id")
        .unwrap();
    query
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn records_line_history() {
    let db = memory_db();
    db.create_recording("r", "Meeting", 0).unwrap();
    let save = |lines: &[Line], secs| db.save_note("r", "{}", "", lines, secs).unwrap();

    save(&[line("a", "hello")], Some(724.0));
    save(&[line("a", "hello world")], Some(726.0));
    save(&[line("a", "hello world"), line("b", "next")], Some(730.0));
    save(&[line("b", "next")], None);

    let created_secs: Option<f64> = db
        .connection()
        .query_row(
            "SELECT created_secs FROM note_blocks WHERE id = 'a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(created_secs, Some(724.0));
    assert_eq!(
        edits(&db),
        [
            ("a", "create", "", "hello world"),
            ("b", "create", "", "next"),
            ("a", "delete", "hello world", ""),
        ]
        .map(|(a, b, c, d)| (a.into(), b.into(), c.into(), d.into()))
    );
}

#[test]
fn summaries_read_notes_by_when_they_were_written() {
    let db = memory_db();
    db.create_recording("r", "Meeting", 0).unwrap();
    db.save_note("r", "{}", "", &[line("a", "- early")], Some(30.0))
        .unwrap();
    db.save_note(
        "r",
        "{}",
        "",
        &[line("a", "- early"), line("b", "late")],
        Some(400.0),
    )
    .unwrap();
    db.save_note(
        "r",
        "{}",
        "",
        &[line("b", "late"), line("c", "after")],
        None,
    )
    .unwrap();
    assert_eq!(
        db.notes_between("r", 0.0, 300.0).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(db.notes_between("r", 300.0, 600.0).unwrap(), ["late"]);
    assert_eq!(db.note_lines("r").unwrap(), ["late", "after"]);

    let batch = |start: f64, topics: &str, declined| Batch {
        start,
        end: start + 300.0,
        topics: topics.into(),
        declined,
        source: "apple".into(),
    };
    db.add_batch("r", &batch(300.0, "[]", true)).unwrap();
    db.add_batch("r", &batch(0.0, "[1]", false)).unwrap();
    db.add_batch("r", &batch(0.0, "[2]", false)).unwrap();
    assert_eq!(
        db.batches("r").unwrap(),
        [batch(0.0, "[2]", false), batch(300.0, "[]", true)]
    );

    assert!(db.summary("r").is_none());
    db.set_summary("r", "{}").unwrap();
    db.set_summary("r", "{\"x\":1}").unwrap();
    assert_eq!(db.summary("r").as_deref(), Some("{\"x\":1}"));
}

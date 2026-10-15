use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::{err, recorder::Recorder, tape, Result};

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS recordings (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    duration_secs REAL NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS segments (
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    speaker TEXT,
    start_secs REAL NOT NULL,
    end_secs REAL NOT NULL,
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS segments_by_recording ON segments(recording_id, start_secs);

CREATE TABLE IF NOT EXISTS notes (
    recording_id TEXT PRIMARY KEY REFERENCES recordings(id) ON DELETE CASCADE,
    doc TEXT NOT NULL,
    markdown TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS note_blocks (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    created_secs REAL,
    updated_at INTEGER NOT NULL,
    updated_secs REAL,
    deleted_at INTEGER
);
CREATE INDEX IF NOT EXISTS note_blocks_by_recording ON note_blocks(recording_id, position);

CREATE TABLE IF NOT EXISTS note_edits (
    id INTEGER PRIMARY KEY,
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    block_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    before TEXT NOT NULL,
    after TEXT NOT NULL,
    at INTEGER NOT NULL,
    secs REAL
);
CREATE INDEX IF NOT EXISTS note_edits_by_block ON note_edits(block_id, at);

CREATE TABLE IF NOT EXISTS speaker_voices (
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    channel TEXT NOT NULL,
    name TEXT NOT NULL,
    voice TEXT NOT NULL,
    PRIMARY KEY (recording_id, channel, name)
);

CREATE TABLE IF NOT EXISTS summary_batches (
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    start_secs REAL NOT NULL,
    end_secs REAL NOT NULL,
    topics TEXT NOT NULL,
    PRIMARY KEY (recording_id, start_secs)
);

CREATE TABLE IF NOT EXISTS summaries (
    recording_id TEXT PRIMARY KEY REFERENCES recordings(id) ON DELETE CASCADE,
    summary TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

const ADDED_COLUMNS: [(&str, &str, &str); 3] = [
    ("segments", "source", "TEXT"),
    ("segments", "words", "TEXT"),
    ("summary_batches", "declined", "INTEGER NOT NULL DEFAULT 0"),
];

const MERGE_EDITS_MS: i64 = 5_000;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

pub struct Db(Mutex<Connection>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    id: String,
    title: String,
    started_at: i64,
    duration_secs: f64,
    transcribed: bool,
}

#[derive(Serialize, Clone)]
pub struct Segment {
    pub speaker: Option<String>,
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(skip)]
    pub source: Option<tape::Channel>,
    #[serde(skip)]
    pub words: Vec<Word>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub start: Option<f64>,
    pub end: Option<f64>,
}

#[derive(Debug, PartialEq)]
pub struct Batch {
    pub start: f64,
    pub end: f64,
    pub topics: String,
    pub declined: bool,
}

#[derive(Serialize)]
pub struct Transcript {
    segments: Vec<Segment>,
}

#[derive(Serialize)]
pub struct Match {
    id: String,
    start: f64,
    text: String,
}

#[derive(Deserialize)]
pub struct Line {
    id: String,
    text: String,
}

impl Db {
    pub fn open(app: &AppHandle) -> Result<Self> {
        let dir = app.path().app_data_dir().map_err(err)?;
        std::fs::create_dir_all(&dir).map_err(err)?;
        let connection = Connection::open(dir.join("aube.db")).map_err(err)?;
        migrate(&connection).map_err(err)?;
        Ok(Self(Mutex::new(connection)))
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap()
    }

    pub fn create_recording(&self, id: &str, title: &str, started_at: i64) -> Result<()> {
        self.connection()
            .execute(
                "INSERT INTO recordings (id, title, started_at) VALUES (?1, ?2, ?3)",
                params![id, title, started_at],
            )
            .map(drop)
            .map_err(err)
    }

    pub fn recording(&self, id: &str) -> Result<(String, f64)> {
        self.connection()
            .query_row(
                "SELECT title, duration_secs FROM recordings WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(err)?
            .ok_or_else(|| "This recording no longer exists.".into())
    }

    pub fn set_duration(&self, id: &str, secs: f64) -> Result<()> {
        self.connection()
            .execute(
                "UPDATE recordings SET duration_secs = ?2 WHERE id = ?1",
                params![id, secs],
            )
            .map(drop)
            .map_err(err)
    }

    pub fn add_segments(&self, id: &str, segments: &[Segment]) -> Result<()> {
        let mut connection = self.connection();
        let tx = connection.transaction().map_err(err)?;
        insert_segments(&tx, id, segments).map_err(err)?;
        tx.commit().map_err(err)
    }

    pub fn replace_segments(&self, id: &str, replacing: usize, segments: &[Segment]) -> Result<()> {
        let mut connection = self.connection();
        let tx = connection.transaction().map_err(err)?;
        let removed = tx
            .execute("DELETE FROM segments WHERE recording_id = ?1", [id])
            .map_err(err)?;
        if removed != replacing {
            return Ok(());
        }
        insert_segments(&tx, id, segments).map_err(err)?;
        tx.commit().map_err(err)
    }

    pub fn segments(&self, id: &str) -> Result<Vec<Segment>> {
        let connection = self.connection();
        let mut query = connection
            .prepare(
                "SELECT speaker, start_secs, end_secs, text,
                    COALESCE(source, CASE speaker WHEN 'You' THEN 'mic' WHEN 'Them' THEN 'call' END),
                    words
                 FROM segments WHERE recording_id = ?1 ORDER BY start_secs",
            )
            .map_err(err)?;
        let segments = query
            .query_map([id], |row| {
                let source: Option<String> = row.get(4)?;
                let words: Option<String> = row.get(5)?;
                Ok(Segment {
                    speaker: row.get(0)?,
                    start: row.get(1)?,
                    end: row.get(2)?,
                    text: row.get(3)?,
                    source: source.as_deref().and_then(tape::Channel::from_name),
                    words: words
                        .and_then(|words| serde_json::from_str(&words).ok())
                        .unwrap_or_default(),
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<_>>()
            .map_err(err);
        segments
    }

    pub fn voices(&self, id: &str, channel: tape::Channel) -> Result<Vec<(String, Vec<f32>)>> {
        let connection = self.connection();
        let mut query = connection
            .prepare(
                "SELECT name, voice FROM speaker_voices WHERE recording_id = ?1 AND channel = ?2",
            )
            .map_err(err)?;
        let voices = query
            .query_map(params![id, channel.name()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(err)?
            .filter_map(|row| {
                let (name, voice) = row.ok()?;
                Some((name, serde_json::from_str(&voice).ok()?))
            })
            .collect();
        Ok(voices)
    }

    pub fn add_voices(
        &self,
        id: &str,
        channel: tape::Channel,
        voices: &[(String, Vec<f32>)],
    ) -> Result<()> {
        let mut connection = self.connection();
        let tx = connection.transaction().map_err(err)?;
        for (name, voice) in voices {
            let voice = serde_json::to_string(voice).map_err(err)?;
            tx.execute(
                "INSERT OR IGNORE INTO speaker_voices (recording_id, channel, name, voice)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, channel.name(), name, voice],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    pub fn note_lines(&self, id: &str) -> Result<Vec<String>> {
        self.texts(
            "SELECT text FROM note_blocks WHERE recording_id = ?1 AND deleted_at IS NULL ORDER BY position",
            params![id],
        )
    }

    pub fn notes_between(&self, id: &str, from: f64, to: f64) -> Result<Vec<String>> {
        self.texts(
            "SELECT text FROM note_blocks
             WHERE recording_id = ?1 AND deleted_at IS NULL AND created_secs >= ?2 AND created_secs < ?3
             ORDER BY position",
            params![id, from, to],
        )
    }

    fn texts(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<String>> {
        let connection = self.connection();
        let mut query = connection.prepare(sql).map_err(err)?;
        let texts = query
            .query_map(params, |row| row.get(0))
            .map_err(err)?
            .collect::<rusqlite::Result<_>>()
            .map_err(err);
        texts
    }

    pub fn batches(&self, id: &str) -> Result<Vec<Batch>> {
        let connection = self.connection();
        let mut query = connection
            .prepare(
                "SELECT start_secs, end_secs, topics, declined FROM summary_batches
                 WHERE recording_id = ?1 ORDER BY start_secs",
            )
            .map_err(err)?;
        let batches = query
            .query_map([id], |row| {
                Ok(Batch {
                    start: row.get(0)?,
                    end: row.get(1)?,
                    topics: row.get(2)?,
                    declined: row.get(3)?,
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<_>>()
            .map_err(err);
        batches
    }

    pub fn add_batch(&self, id: &str, batch: &Batch) -> Result<()> {
        self.connection()
            .execute(
                "INSERT OR REPLACE INTO summary_batches
                    (recording_id, start_secs, end_secs, topics, declined)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, batch.start, batch.end, batch.topics, batch.declined],
            )
            .map(drop)
            .map_err(err)
    }

    pub fn summary(&self, id: &str) -> Option<String> {
        self.connection()
            .query_row(
                "SELECT summary FROM summaries WHERE recording_id = ?1",
                [id],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn set_summary(&self, id: &str, summary: &str) -> Result<()> {
        self.connection()
            .execute(
                "INSERT INTO summaries (recording_id, summary, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(recording_id) DO UPDATE SET
                    summary = excluded.summary, created_at = excluded.created_at",
                params![id, summary, now()],
            )
            .map(drop)
            .map_err(err)
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.connection()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.connection()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(drop)
            .map_err(err)
    }

    fn save_note(
        &self,
        recording: &str,
        doc: &str,
        markdown: &str,
        lines: &[Line],
        secs: Option<f64>,
    ) -> rusqlite::Result<()> {
        let at = now();
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        tx.execute(
            "INSERT INTO notes (recording_id, doc, markdown, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(recording_id) DO UPDATE SET
                doc = excluded.doc, markdown = excluded.markdown, updated_at = excluded.updated_at",
            params![recording, doc, markdown, at],
        )?;
        let mut current: HashMap<String, String> = tx
            .prepare(
                "SELECT id, text FROM note_blocks WHERE recording_id = ?1 AND deleted_at IS NULL",
            )?
            .query_map([recording], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (position, line) in (0_i64..).zip(lines) {
            match current.remove(&line.id) {
                None => {
                    tx.execute(
                        "INSERT INTO note_blocks
                            (id, recording_id, position, text, created_at, created_secs, updated_at, updated_secs)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)
                         ON CONFLICT(id) DO UPDATE SET
                            position = excluded.position, text = excluded.text, deleted_at = NULL,
                            updated_at = excluded.updated_at, updated_secs = excluded.updated_secs",
                        params![line.id, recording, position, line.text, at, secs],
                    )?;
                    record_edit(
                        &tx,
                        recording,
                        &line.id,
                        Edit::Create,
                        "",
                        &line.text,
                        at,
                        secs,
                    )?;
                }
                Some(before) if before != line.text => {
                    tx.execute(
                        "UPDATE note_blocks SET position = ?2, text = ?3, updated_at = ?4, updated_secs = ?5
                         WHERE id = ?1",
                        params![line.id, position, line.text, at, secs],
                    )?;
                    record_edit(
                        &tx,
                        recording,
                        &line.id,
                        Edit::Change,
                        &before,
                        &line.text,
                        at,
                        secs,
                    )?;
                }
                Some(_) => {
                    tx.execute(
                        "UPDATE note_blocks SET position = ?2 WHERE id = ?1",
                        params![line.id, position],
                    )?;
                }
            }
        }
        for (id, before) in current {
            tx.execute(
                "UPDATE note_blocks SET deleted_at = ?2 WHERE id = ?1",
                params![id, at],
            )?;
            record_edit(&tx, recording, &id, Edit::Delete, &before, "", at, secs)?;
        }
        tx.commit()
    }
}

fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(SCHEMA)?;
    for (table, column, kind) in ADDED_COLUMNS {
        let existing: Vec<String> = connection
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if !existing.iter().any(|name| name == column) {
            connection.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?;
        }
    }
    Ok(())
}

fn insert_segments(tx: &Transaction, id: &str, segments: &[Segment]) -> rusqlite::Result<()> {
    for segment in segments {
        let words = (!segment.words.is_empty())
            .then(|| serde_json::to_string(&segment.words).ok())
            .flatten();
        tx.execute(
            "INSERT INTO segments (recording_id, speaker, start_secs, end_secs, text, source, words)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                segment.speaker,
                segment.start,
                segment.end,
                segment.text,
                segment.source.map(tape::Channel::name),
                words
            ],
        )?;
    }
    Ok(())
}

#[derive(PartialEq)]
enum Edit {
    Create,
    Change,
    Delete,
}

impl Edit {
    fn name(&self) -> &'static str {
        match self {
            Edit::Create => "create",
            Edit::Change => "change",
            Edit::Delete => "delete",
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn record_edit(
    tx: &Transaction,
    recording: &str,
    block: &str,
    edit: Edit,
    before: &str,
    after: &str,
    at: i64,
    secs: Option<f64>,
) -> rusqlite::Result<()> {
    let recent: Option<(i64, String)> = tx
        .query_row(
            "SELECT id, kind FROM note_edits WHERE block_id = ?1 AND at > ?2 ORDER BY at DESC LIMIT 1",
            params![block, at - MERGE_EDITS_MS],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match recent {
        Some((id, kind)) if edit == Edit::Change && kind != Edit::Delete.name() => tx.execute(
            "UPDATE note_edits SET after = ?2, at = ?3, secs = ?4 WHERE id = ?1",
            params![id, after, at, secs],
        ),
        _ => tx.execute(
            "INSERT INTO note_edits (recording_id, block_id, kind, before, after, at, secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![recording, block, edit.name(), before, after, at, secs],
        ),
    }
    .map(drop)
}

#[tauri::command]
pub fn list_recordings(db: State<'_, Db>) -> Result<Vec<Recording>> {
    let connection = db.connection();
    let mut query = connection
        .prepare(
            "SELECT id, title, started_at, duration_secs,
                EXISTS (SELECT 1 FROM segments WHERE recording_id = recordings.id)
             FROM recordings ORDER BY started_at DESC",
        )
        .map_err(err)?;
    let recordings = query
        .query_map([], |row| {
            Ok(Recording {
                id: row.get(0)?,
                title: row.get(1)?,
                started_at: row.get(2)?,
                duration_secs: row.get(3)?,
                transcribed: row.get(4)?,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(err);
    recordings
}

#[tauri::command]
pub fn rename_recording(db: State<'_, Db>, id: String, title: String) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Give the recording a name.".into());
    }
    db.connection()
        .execute(
            "UPDATE recordings SET title = ?2 WHERE id = ?1",
            params![id, title],
        )
        .map(drop)
        .map_err(err)
}

#[tauri::command]
pub fn delete_recording(app: AppHandle, db: State<'_, Db>, id: String) -> Result<()> {
    db.connection()
        .execute("DELETE FROM recordings WHERE id = ?1", [&id])
        .map_err(err)?;
    if let Ok(audio) = tape::Audio::of(&app) {
        audio.remove(&id);
    }
    Ok(())
}

#[tauri::command]
pub fn transcript(db: State<'_, Db>, id: String) -> Result<Transcript> {
    Ok(Transcript {
        segments: db.segments(&id)?,
    })
}

#[tauri::command]
pub fn search_transcripts(db: State<'_, Db>, query: String) -> Result<Vec<Match>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let connection = db.connection();
    let mut search = connection
        .prepare(
            "SELECT segments.recording_id, segments.start_secs, segments.text
             FROM segments JOIN recordings ON recordings.id = segments.recording_id
             WHERE segments.text LIKE '%' || ?1 || '%'
             ORDER BY recordings.started_at DESC, segments.start_secs",
        )
        .map_err(err)?;
    let matches = search
        .query_map([query], |row| {
            Ok(Match {
                id: row.get(0)?,
                start: row.get(1)?,
                text: row.get(2)?,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(err);
    matches
}

#[tauri::command]
pub fn note(db: State<'_, Db>, recording_id: String) -> Option<String> {
    db.connection()
        .query_row(
            "SELECT doc FROM notes WHERE recording_id = ?1",
            [recording_id],
            |row| row.get(0),
        )
        .ok()
}

#[tauri::command]
pub fn save_note(
    db: State<'_, Db>,
    recorder: State<'_, Recorder>,
    recording_id: String,
    doc: String,
    markdown: String,
    lines: Vec<Line>,
) -> Result<()> {
    let secs = recorder.elapsed(&recording_id);
    db.save_note(&recording_id, &doc, &markdown, &lines, secs)
        .map_err(err)
}

#[cfg(test)]
#[path = "../tests/unit/library.rs"]
mod tests;

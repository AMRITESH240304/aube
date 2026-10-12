use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::{err, recorder::Recorder, Result};

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

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

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

#[derive(Serialize)]
pub struct Segment {
    pub speaker: Option<String>,
    pub start: f64,
    pub end: f64,
    pub text: String,
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
        connection.execute_batch(SCHEMA).map_err(err)?;
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
        for segment in segments {
            tx.execute(
                "INSERT INTO segments (recording_id, speaker, start_secs, end_secs, text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    segment.speaker,
                    segment.start,
                    segment.end,
                    segment.text
                ],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
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
pub fn delete_recording(db: State<'_, Db>, id: String) -> Result<()> {
    db.connection()
        .execute("DELETE FROM recordings WHERE id = ?1", [id])
        .map(drop)
        .map_err(err)
}

#[tauri::command]
pub fn transcript(db: State<'_, Db>, id: String) -> Result<Transcript> {
    let connection = db.connection();
    let mut query = connection
        .prepare(
            "SELECT speaker, start_secs, end_secs, text FROM segments
             WHERE recording_id = ?1 ORDER BY start_secs",
        )
        .map_err(err)?;
    let segments = query
        .query_map([id], |row| {
            Ok(Segment {
                speaker: row.get(0)?,
                start: row.get(1)?,
                end: row.get(2)?,
                text: row.get(3)?,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(err)?;
    Ok(Transcript { segments })
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
mod tests {
    use super::*;

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
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        let db = Db(Mutex::new(connection));
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
}

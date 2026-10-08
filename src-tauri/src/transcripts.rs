use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::{err, recorder::recordings_dir, Result};

pub(crate) fn transcript_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.transcript.json"))
}

#[tauri::command]
pub fn transcript(app: AppHandle, id: String) -> Result<Value> {
    let path = transcript_path(&recordings_dir(&app)?, &id);
    let bytes = std::fs::read(path).map_err(|_| "This recording doesn't have a transcript yet.")?;
    serde_json::from_slice(&bytes).map_err(err)
}

#[tauri::command]
pub fn delete_recording(app: AppHandle, id: String) -> Result<()> {
    let dir = recordings_dir(&app)?;
    for path in [
        dir.join(format!("{id}.wav")),
        dir.join(format!("{id}.json")),
        transcript_path(&dir, &id),
    ] {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(err(e)),
            _ => {}
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct Stored {
    #[serde(default)]
    segments: Vec<Segment>,
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct Segment {
    pub start: f64,
    pub text: String,
}

#[derive(Serialize)]
pub struct Match {
    id: String,
    start: f64,
    text: String,
}

// Over the transcripts saved on disk: instant and works offline.
#[tauri::command]
pub fn search_transcripts(app: AppHandle, query: String) -> Result<Vec<Match>> {
    let dir = recordings_dir(&app)?;
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(err)?.flatten() {
        let path = entry.path();
        let Some(id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".transcript.json"))
        else {
            continue;
        };
        let Some(stored) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        else {
            continue;
        };
        matches.extend(
            stored
                .segments
                .into_iter()
                .filter(|s| s.text.to_lowercase().contains(&query))
                .map(|s| Match {
                    id: id.to_string(),
                    start: s.start,
                    text: s.text,
                }),
        );
    }
    Ok(matches)
}

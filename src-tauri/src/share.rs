use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tauri::{AppHandle, Manager};

use crate::{err, Result};

const UNTITLED: &str = "Aube note";
const UNSAFE: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

fn file_stem(title: &str) -> String {
    let stem: String = title
        .chars()
        .map(|c| {
            if UNSAFE.contains(&c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim().trim_start_matches('.').trim();
    if stem.is_empty() {
        UNTITLED.to_owned()
    } else {
        stem.to_owned()
    }
}

fn free_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.md"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} {n}.md")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

#[tauri::command]
pub fn save_markdown(app: AppHandle, title: String, markdown: String) -> Result<String> {
    let dir = app.path().download_dir().map_err(err)?;
    let path = free_path(&dir, &file_stem(&title));
    fs::write(&path, markdown).map_err(err)?;
    let _ = Command::new("open").arg("-R").arg(&path).spawn();
    Ok(path.display().to_string())
}

#[cfg(test)]
#[path = "../tests/unit/share.rs"]
mod tests;

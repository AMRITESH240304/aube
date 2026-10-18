use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{err, library::Db, Result};

const SETTINGS: &str = "llm";
const DEFAULT_URL: &str = "http://127.0.0.1:11434";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Apple,
    Ollama,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub provider: Provider,
    pub url: String,
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::Apple,
            url: DEFAULT_URL.to_owned(),
            model: String::new(),
        }
    }
}

impl Settings {
    pub fn source(&self) -> String {
        match self.provider {
            Provider::Apple => "apple".to_owned(),
            Provider::Ollama => format!("ollama:{}", self.model.trim()),
        }
    }
}

pub fn settings(app: &AppHandle) -> Settings {
    app.state::<Db>()
        .setting(SETTINGS)
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

fn clean(mut settings: Settings) -> Result<Settings> {
    settings.url = settings.url.trim().trim_end_matches('/').to_owned();
    settings.model = settings.model.trim().to_owned();
    if settings.url.is_empty() {
        settings.url = DEFAULT_URL.to_owned();
    }
    if settings.provider == Provider::Ollama
        && !(settings.url.starts_with("http://") || settings.url.starts_with("https://"))
    {
        return Err("Ollama's address must start with http:// or https://.".to_owned());
    }
    Ok(settings)
}

#[tauri::command]
pub fn llm(app: AppHandle) -> Settings {
    settings(&app)
}

#[tauri::command]
pub fn set_llm(app: AppHandle, settings: Settings) -> Result<()> {
    let settings = clean(settings)?;
    app.state::<Db>()
        .set_setting(SETTINGS, &serde_json::to_string(&settings).map_err(err)?)?;
    crate::dictation::start(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn ollama_models(url: String) -> Result<Vec<String>> {
    let settings = clean(Settings {
        provider: Provider::Ollama,
        url,
        model: String::new(),
    })?;
    let program = std::env::current_exe()
        .map_err(err)?
        .with_file_name("aube-notes");
    let output = std::process::Command::new(program)
        .args(["--models", &settings.url])
        .output()
        .map_err(|e| format!("Couldn't reach Ollama: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.lines().last().unwrap_or_default();
    if !output.status.success() {
        if let Some(error) = serde_json::from_str::<serde_json::Value>(last)
            .ok()
            .and_then(|value| value.get("error")?.as_str().map(str::to_owned))
        {
            return Err(error);
        }
        return Err("Couldn't reach Ollama.".to_owned());
    }
    serde_json::from_str(last).map_err(|_| "Ollama returned an unexpected model list.".to_owned())
}

#[cfg(test)]
#[path = "../tests/unit/llm.rs"]
mod tests;

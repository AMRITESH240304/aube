use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        Mutex,
    },
    thread,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{err, library::Db, llm, stt, Result};

const SETTINGS: &str = "dictation";
const OFF: &str = "off";
const KEYS: [&str; 6] = [
    OFF,
    "fn",
    "right-option",
    "right-command",
    "right-control",
    "right-shift",
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub key: String,
    pub cleanup: bool,
    pub spoken_edits: bool,
    pub voice_fixes: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            key: OFF.to_owned(),
            cleanup: true,
            spoken_edits: true,
            voice_fixes: true,
        }
    }
}

impl Settings {
    pub fn arguments(
        &self,
        language: Option<String>,
        ask_for_access: bool,
        llm: &llm::Settings,
    ) -> Option<Vec<String>> {
        if self.key == OFF || !KEYS.contains(&self.key.as_str()) {
            return None;
        }
        let mut arguments = vec!["--key".to_owned(), self.key.clone()];
        if let Some(language) = language {
            arguments.extend(["--language".to_owned(), language]);
        }
        if !self.cleanup {
            arguments.push("--no-cleanup".to_owned());
        } else {
            match llm.provider {
                llm::Provider::Apple => arguments.extend(["--llm".to_owned(), "apple".to_owned()]),
                llm::Provider::Ollama => arguments.extend([
                    "--llm".to_owned(),
                    "ollama".to_owned(),
                    "--ollama-url".to_owned(),
                    llm.url.clone(),
                    "--ollama-model".to_owned(),
                    llm.model.clone(),
                ]),
            }
        }
        if !self.spoken_edits {
            arguments.push("--no-spoken-edits".to_owned());
        }
        if !self.voice_fixes {
            arguments.push("--no-voice-fixes".to_owned());
        }
        if ask_for_access {
            arguments.push("--ask-for-access".to_owned());
        }
        Some(arguments)
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub ready: bool,
    pub accessibility: bool,
    pub fn_conflict: bool,
    pub cleanup: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Dictation {
    helper: Mutex<Option<Child>>,
    status: Mutex<Option<Status>>,
    generation: AtomicUsize,
    asked_for_access: AtomicBool,
}

#[derive(Serialize)]
pub struct Overview {
    settings: Settings,
    status: Option<Status>,
}

fn settings(app: &AppHandle) -> Settings {
    app.state::<Db>()
        .setting(SETTINGS)
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

fn report(app: &AppHandle, status: Option<Status>) {
    *app.state::<Dictation>().status.lock().unwrap() = status.clone();
    let _ = app.emit("dictation-status", status);
}

pub fn stop(app: &AppHandle) {
    let dictation = app.state::<Dictation>();
    dictation.generation.fetch_add(1, SeqCst);
    let helper = dictation.helper.lock().unwrap().take();
    if let Some(mut helper) = helper {
        let _ = helper.kill();
        let _ = helper.wait();
    }
}

pub fn start(app: &AppHandle) {
    stop(app);
    let settings = settings(app);
    let ask_for_access =
        settings.key != OFF && !app.state::<Dictation>().asked_for_access.swap(true, SeqCst);
    let Some(arguments) =
        settings.arguments(stt::your_language(app), ask_for_access, &llm::settings(app))
    else {
        return report(app, None);
    };
    report(app, Some(Status::default()));
    let program = match std::env::current_exe() {
        Ok(exe) => exe.with_file_name("aube-dictate"),
        Err(e) => return failed(app, e.to_string()),
    };
    let mut helper = match Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(helper) => helper,
        Err(e) => return failed(app, format!("Couldn't start push to talk: {e}")),
    };
    let Some(stdout) = helper.stdout.take() else {
        return failed(app, "Couldn't start push to talk.".to_owned());
    };
    let dictation = app.state::<Dictation>();
    let generation = dictation.generation.load(SeqCst);
    *dictation.helper.lock().unwrap() = Some(helper);

    let app = app.clone();
    thread::spawn(move || {
        let current = || app.state::<Dictation>().generation.load(SeqCst) == generation;
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::io::Result::ok)
        {
            if let Ok(status) = serde_json::from_str::<Status>(&line) {
                if current() {
                    report(&app, Some(status));
                }
            }
        }
        if current() {
            let stopped = app.state::<Dictation>().status.lock().unwrap().clone();
            if stopped.as_ref().is_none_or(|status| status.error.is_none()) {
                failed(&app, "Push to talk stopped unexpectedly.".to_owned());
            }
        }
    });
}

fn failed(app: &AppHandle, error: String) {
    report(
        app,
        Some(Status {
            error: Some(error),
            ..Status::default()
        }),
    );
}

#[tauri::command]
pub fn dictation(app: AppHandle, state: State<'_, Dictation>) -> Overview {
    Overview {
        settings: settings(&app),
        status: state.status.lock().unwrap().clone(),
    }
}

#[tauri::command]
pub fn set_dictation(app: AppHandle, settings: Settings) -> Result<()> {
    if !KEYS.contains(&settings.key.as_str()) {
        return Err(format!("Unknown key: {}", settings.key));
    }
    let value = serde_json::to_string(&settings).map_err(err)?;
    app.state::<Db>().set_setting(SETTINGS, &value)?;
    start(&app);
    Ok(())
}

#[tauri::command]
pub fn open_system_settings(pane: String) -> Result<()> {
    let url = match pane.as_str() {
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        "keyboard" => "x-apple.systempreferences:com.apple.Keyboard-Settings.extension",
        _ => return Err(format!("Unknown settings pane: {pane}")),
    };
    Command::new("open").arg(url).spawn().map(drop).map_err(err)
}

#[cfg(test)]
#[path = "../tests/unit/dictation.rs"]
mod tests;

use tauri::{Manager, RunEvent};

mod echo;
mod lid;
mod mixer;
#[cfg(target_os = "macos")]
mod permission;
mod recorder;
mod stt;
mod transcripts;

type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(recorder::Recorder::default())
        .setup(|app| {
            if let Err(e) = recorder::recover_interrupted(app.handle()) {
                eprintln!("recovery failed: {e}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            recorder::start_recording,
            recorder::stop_recording,
            recorder::set_paused,
            recorder::recording_status,
            recorder::list_recordings,
            recorder::rename_recording,
            transcripts::transcript,
            transcripts::delete_recording,
            transcripts::search_transcripts,
            stt::transcription_languages,
            stt::install_language,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Quitting mid-call saves the recording instead of losing it.
            if let RunEvent::Exit = event {
                let _ = recorder::finish(&app.state::<recorder::Recorder>());
            }
        });
}

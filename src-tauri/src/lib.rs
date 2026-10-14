use tauri::{Manager, RunEvent};

mod echo;
mod library;
mod lid;
mod mixer;
#[cfg(target_os = "macos")]
mod permission;
mod recorder;
mod speakers;
mod stt;
mod summary;
mod tape;

type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(recorder::Recorder::default())
        .setup(|app| {
            app.manage(library::Db::open(app.handle())?);
            speakers::identify_waiting(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            recorder::start_recording,
            recorder::stop_recording,
            recorder::set_paused,
            recorder::recording_status,
            library::list_recordings,
            library::rename_recording,
            library::delete_recording,
            library::transcript,
            library::search_transcripts,
            library::note,
            library::save_note,
            summary::summary,
            summary::summarize,
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

use super::*;
use crate::llm;

fn settings(key: &str, cleanup: bool) -> Settings {
    Settings {
        key: key.into(),
        cleanup,
    }
}

fn apple() -> llm::Settings {
    llm::Settings::default()
}

#[test]
fn off_and_unknown_keys_start_nothing() {
    assert_eq!(Settings::default().arguments(None, true, &apple()), None);
    assert_eq!(
        settings("caps-lock", true).arguments(None, true, &apple()),
        None
    );
}

#[test]
fn the_helper_gets_the_key_language_and_cleanup_choice() {
    assert_eq!(
        settings("fn", true)
            .arguments(Some("hi-IN".into()), false, &apple())
            .unwrap(),
        ["--key", "fn", "--language", "hi-IN", "--llm", "apple"]
    );
    assert_eq!(
        settings("right-option", false)
            .arguments(None, true, &apple())
            .unwrap(),
        ["--key", "right-option", "--no-cleanup", "--ask-for-access"]
    );
}

#[test]
fn ollama_gets_its_address_and_model() {
    let ollama = llm::Settings {
        provider: llm::Provider::Ollama,
        url: "http://127.0.0.1:11434".into(),
        model: "llama3.2".into(),
    };
    assert_eq!(
        settings("fn", true)
            .arguments(None, false, &ollama)
            .unwrap(),
        [
            "--key",
            "fn",
            "--llm",
            "ollama",
            "--ollama-url",
            "http://127.0.0.1:11434",
            "--ollama-model",
            "llama3.2"
        ]
    );
}

#[test]
fn the_helpers_status_lines_are_read_as_status() {
    let status: Status = serde_json::from_str(
        r#"{"ready":true,"accessibility":false,"fnConflict":true,"cleanup":"Turn on Apple Intelligence."}"#,
    )
    .unwrap();
    assert!(status.ready && status.fn_conflict && !status.accessibility);
    assert_eq!(
        status.cleanup.as_deref(),
        Some("Turn on Apple Intelligence.")
    );
    assert_eq!(status.error, None);
}

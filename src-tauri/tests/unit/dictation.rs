use super::*;

fn settings(key: &str, cleanup: bool) -> Settings {
    Settings {
        key: key.into(),
        cleanup,
    }
}

#[test]
fn off_and_unknown_keys_start_nothing() {
    assert_eq!(Settings::default().arguments(None, true), None);
    assert_eq!(settings("caps-lock", true).arguments(None, true), None);
}

#[test]
fn the_helper_gets_the_key_language_and_cleanup_choice() {
    assert_eq!(
        settings("fn", true)
            .arguments(Some("hi-IN".into()), false)
            .unwrap(),
        ["--key", "fn", "--language", "hi-IN"]
    );
    assert_eq!(
        settings("right-option", false)
            .arguments(None, true)
            .unwrap(),
        ["--key", "right-option", "--no-cleanup", "--ask-for-access"]
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

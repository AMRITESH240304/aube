use super::*;

#[test]
fn titles_become_safe_file_names() {
    assert_eq!(file_stem("Standup: Q3 / plans?"), "Standup- Q3 - plans-");
    assert_eq!(file_stem("  .hidden "), "hidden");
    assert_eq!(file_stem("   "), UNTITLED);
}

#[test]
fn saving_twice_never_overwrites() {
    let dir = std::env::temp_dir().join(format!("aube-share-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let first = free_path(&dir, "Meeting");
    assert_eq!(first.file_name().unwrap(), "Meeting.md");
    fs::write(&first, "").unwrap();
    assert_eq!(
        free_path(&dir, "Meeting").file_name().unwrap(),
        "Meeting 2.md"
    );
    fs::remove_dir_all(&dir).unwrap();
}

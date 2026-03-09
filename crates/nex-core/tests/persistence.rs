use nex_core::{atomic_write_json, backup_path, load_bytes_with_backup, load_json_with_backup};

#[test]
fn atomic_write_json_overwrites_existing_file_and_cleans_backup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("state.json");

    atomic_write_json(&path, &vec!["alpha"]).expect("write initial state");
    atomic_write_json(&path, &vec!["beta"]).expect("overwrite state");

    let loaded: Vec<String> = load_json_with_backup(&path)
        .expect("load state")
        .expect("state exists");
    assert_eq!(loaded, vec!["beta".to_string()]);
    assert!(
        !backup_path(&path).exists(),
        "successful writes should not leave stale backup files"
    );
}

#[test]
fn load_json_with_backup_recovers_from_invalid_primary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("state.json");
    let backup = backup_path(&path);

    std::fs::write(&path, "{not valid json").expect("write corrupt primary");
    std::fs::write(&backup, "[1,2,3]").expect("write backup");

    let loaded: Vec<u32> = load_json_with_backup(&path)
        .expect("load backup")
        .expect("backup exists");
    assert_eq!(loaded, vec![1, 2, 3]);
}

#[test]
fn load_bytes_with_backup_recovers_when_primary_is_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("state.bin");
    let backup = backup_path(&path);

    std::fs::write(&backup, b"replica bytes").expect("write backup bytes");

    let loaded = load_bytes_with_backup(&path)
        .expect("load backup bytes")
        .expect("backup exists");
    assert_eq!(loaded, b"replica bytes");
}

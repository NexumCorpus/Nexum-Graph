use nex_coord::{CoordinationDocument, CrdtLockEntry};
use nex_core::{IntentKind, backup_path};

#[test]
fn load_from_path_recovers_from_backup_when_primary_bytes_are_invalid() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("coordination.loro");
    let backup = backup_path(&path);

    let document = CoordinationDocument::new(1).expect("create document");
    document
        .replace_lock_entries(&[CrdtLockEntry {
            agent_name: "alice".to_string(),
            agent_id: [1u8; 16],
            target_name: "processRequest".to_string(),
            target: [2u8; 32],
            kind: IntentKind::Write,
        }])
        .expect("seed lock entries");
    document.save_to_path(&backup).expect("save backup");

    std::fs::write(&path, b"not-loro").expect("write corrupt primary");

    let loaded = CoordinationDocument::load_from_path(&path, 2).expect("load document");
    let entries = loaded.lock_entries().expect("read locks");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].agent_name, "alice");
    assert_eq!(entries[0].target_name, "processRequest");
    assert_eq!(entries[0].kind, IntentKind::Write);
}

use cc_switch_lib::bridges::session as session_bridge;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

fn seed_codex_session() -> std::path::PathBuf {
    let home = ensure_test_home().to_path_buf();
    let session_id = "11111111-2222-3333-4444-555555555555";
    let session_path = home
        .join(".codex")
        .join("sessions")
        .join("demo-project")
        .join("2026")
        .join("03")
        .join(format!("{session_id}.jsonl"));
    if let Some(parent) = session_path.parent() {
        std::fs::create_dir_all(parent).expect("create codex session dir");
    }

    let lines = [
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-03-09T10:00:00Z",
            "payload": {
                "id": session_id,
                "cwd": "/tmp/demo-project"
            }
        })
        .to_string(),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-03-09T10:01:00Z",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "session summary line" }]
            }
        })
        .to_string(),
    ];

    std::fs::write(&session_path, format!("{}\n{}\n", lines[0], lines[1]))
        .expect("write codex session");
    session_path
}

#[test]
fn session_baseline_legacy_codex_scan_is_stable() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let path = seed_codex_session();

    let sessions = session_bridge::legacy_list_sessions();
    let session = sessions
        .iter()
        .find(|item| item.source_path.as_deref() == Some(path.to_string_lossy().as_ref()))
        .expect("seeded codex session");

    assert_eq!(session.session_id, "11111111-2222-3333-4444-555555555555");
    assert_eq!(session.resume_command.as_deref(), Some("codex resume 11111111-2222-3333-4444-555555555555"));
}

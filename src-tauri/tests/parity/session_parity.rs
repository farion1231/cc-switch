use cc_switch_lib::bridges::session as session_bridge;

use super::support::{ensure_test_home, reset_test_fs, test_mutex};

fn seed_codex_session() -> String {
    let home = ensure_test_home().to_path_buf();
    let session_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
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
            "timestamp": "2026-03-09T12:00:00Z",
            "payload": {
                "id": session_id,
                "cwd": "/tmp/parity-project"
            }
        })
        .to_string(),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-03-09T12:01:00Z",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            }
        })
        .to_string(),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-03-09T12:02:00Z",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "world" }]
            }
        })
        .to_string(),
    ];

    std::fs::write(
        &session_path,
        format!("{}\n{}\n{}\n", lines[0], lines[1], lines[2]),
    )
    .expect("write codex session");
    session_path.to_string_lossy().to_string()
}

#[test]
fn session_parity_codex_scan_and_messages_match_legacy() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());
    reset_test_fs();
    let source_path = seed_codex_session();

    let legacy_sessions = session_bridge::legacy_list_sessions()
        .into_iter()
        .filter(|item| item.source_path.as_deref() == Some(source_path.as_str()))
        .collect::<Vec<_>>();
    let core_sessions = session_bridge::list_sessions()
        .expect("core list sessions")
        .into_iter()
        .filter(|item| item.source_path.as_deref() == Some(source_path.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(core_sessions).expect("core sessions json"),
        serde_json::to_value(legacy_sessions).expect("legacy sessions json")
    );

    let legacy_messages = session_bridge::legacy_get_session_messages("codex", &source_path)
        .expect("legacy messages");
    let core_messages =
        session_bridge::get_session_messages("codex", &source_path).expect("core messages");
    assert_eq!(
        serde_json::to_value(core_messages).expect("core messages json"),
        serde_json::to_value(legacy_messages).expect("legacy messages json")
    );
}

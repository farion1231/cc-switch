use std::fs;

use cc_switch_lib::{CodexTargetContext, SessionCatalog};
use tempfile::tempdir;

#[test]
fn session_catalog_lists_sessions_for_an_explicit_codex_target_home() {
    let fixture = tempdir().expect("fixture");
    let config_dir = fixture.path().join(".codex");
    let sessions = config_dir.join("sessions/2026/07/23");
    fs::create_dir_all(&sessions).expect("sessions");
    fs::write(
        sessions.join("rollout-2026-07-23T00-00-00-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl"),
        concat!(
            r#"{"timestamp":"2026-07-23T00:00:00Z","type":"session_meta","payload":{"id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"/tmp/demo"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-23T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello target"}]}}"#,
            "\n",
        ),
    )
    .expect("session");

    let context = CodexTargetContext::new(&config_dir);
    let listed = SessionCatalog::scan_target(&context);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
}

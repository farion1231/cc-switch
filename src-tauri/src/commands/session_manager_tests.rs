use super::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[test]
#[serial_test::serial]
fn borrowed_session_responses_preserve_the_json_and_channel_contract() {
    let _home = crate::commands::startup_tests::TestHome::new();
    let root = crate::config::get_claude_settings_path()
        .parent()
        .unwrap()
        .join("projects/runtime");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("runtime.jsonl");
    let record = json!({"type":"user", "uuid":"u1", "sessionId":"runtime",
        "message":{"role":"user", "content":"你好\n\"quoted\" 😀"}});
    std::fs::write(&path, format!("{record}\n")).unwrap();
    let source = path.to_string_lossy().into_owned();
    let expected =
        serde_json::to_value(session_manager::load_messages("claude", &source).unwrap()).unwrap();
    assert_eq!(expected.as_array().unwrap().len(), 1);

    let response =
        tauri::async_runtime::block_on(get_session_messages("claude".into(), source.clone()))
            .unwrap();
    assert_eq!(response.deserialize::<Value>().unwrap(), expected);

    let chunks = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received = Arc::clone(&chunks);
    let channel = Channel::new(move |body| {
        received
            .lock()
            .unwrap()
            .push(body.deserialize::<Value>().unwrap());
        Ok(())
    });
    tauri::async_runtime::block_on(stream_session_messages("claude".into(), source, channel))
        .unwrap();
    let chunks = chunks.lock().unwrap();
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0]["type"], "header");
    assert_eq!(chunks[0]["total"], 1);
    assert!(chunks[0]["parseMs"].is_number());
    assert_eq!(
        chunks[1],
        json!({"type":"messages", "start":0, "messages":expected})
    );
    assert_eq!(chunks[2]["type"], "done");
    assert!(chunks[2]["payloadBytes"].as_u64().unwrap() > 0);
}

#[test]
fn failed_session_stream_still_sends_an_error_packet() {
    let chunks = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received = Arc::clone(&chunks);
    let channel = Channel::new(move |body| {
        received
            .lock()
            .unwrap()
            .push(body.deserialize::<Value>().unwrap());
        Ok(())
    });
    let error = tauri::async_runtime::block_on(stream_session_messages(
        "unknown".into(),
        "missing".into(),
        channel,
    ))
    .unwrap_err();
    assert_eq!(
        *chunks.lock().unwrap(),
        [json!({"type":"error", "message":error})]
    );
}

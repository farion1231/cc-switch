use cc_switch_lib::remote::client::{RemoteClientError, RemoteSession};
use cc_switch_lib::remote::protocol::{encode_frame, Frame, FrameKind, HelloAck, RpcResponse};
use serde_json::json;
use std::io::Cursor;

fn server_frames(response_id: &str) -> Vec<u8> {
    let hello_ack = Frame::from_json(
        FrameKind::HelloAck,
        &HelloAck {
            agent_version: "3.18.0".to_string(),
            protocol_minor: 0,
            schema_version: 16,
            platform: "linux".to_string(),
            architecture: "x86_64".to_string(),
            capabilities: vec!["provider.list".to_string()],
        },
    )
    .expect("hello ack");
    let response = Frame::from_json(
        FrameKind::Response,
        &RpcResponse {
            id: response_id.to_string(),
            result: Some(json!({ "remote-a": { "id": "remote-a" } })),
            error: None,
        },
    )
    .expect("response");

    [
        encode_frame(&hello_ack).expect("encode hello ack"),
        encode_frame(&response).expect("encode response"),
    ]
    .concat()
}

#[test]
fn remote_session_handshakes_and_correlates_response() {
    let reader = Cursor::new(server_frames("req-1"));
    let writer = Vec::new();
    let mut session = RemoteSession::connect(reader, writer, "3.18.0").expect("handshake");

    let result = session
        .invoke_with_id("req-1", "provider.list", json!({ "app": "claude" }), 30_000)
        .expect("invoke provider list");

    assert_eq!(result["remote-a"]["id"], "remote-a");
    assert_eq!(session.hello_ack().platform, "linux");

    let (_reader, written) = session.into_parts();
    let mut written = written.as_slice();
    assert_eq!(
        cc_switch_lib::remote::protocol::decode_frame(&mut written)
            .expect("client hello")
            .kind,
        FrameKind::Hello
    );
    assert_eq!(
        cc_switch_lib::remote::protocol::decode_frame(&mut written)
            .expect("client request")
            .kind,
        FrameKind::Request
    );
}

#[test]
fn remote_session_rejects_mismatched_response_id() {
    let reader = Cursor::new(server_frames("wrong-id"));
    let writer = Vec::new();
    let mut session = RemoteSession::connect(reader, writer, "3.18.0").expect("handshake");

    let error = session
        .invoke_with_id("req-1", "provider.list", json!({ "app": "claude" }), 30_000)
        .expect_err("mismatched response must fail");

    assert!(matches!(
        error,
        RemoteClientError::ResponseIdMismatch { expected, actual }
            if expected == "req-1" && actual == "wrong-id"
    ));
}

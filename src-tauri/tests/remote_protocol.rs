use cc_switch_lib::remote::protocol::{
    decode_frame, encode_frame, Frame, FrameKind, ProtocolError, RpcRequest, MAX_FRAME_BYTES,
    PROTOCOL_MAJOR,
};
use serde_json::json;

#[test]
fn protocol_round_trips_request_frame() {
    let request = RpcRequest {
        id: "req-1".to_string(),
        command: "provider.list".to_string(),
        args: json!({ "app": "codex" }),
        timeout_ms: 30_000,
        operation_id: None,
    };
    let frame = Frame::from_json(FrameKind::Request, &request).expect("encode request payload");

    let bytes = encode_frame(&frame).expect("encode frame");
    let decoded = decode_frame(&mut bytes.as_slice()).expect("decode frame");
    let decoded_request: RpcRequest = decoded.json().expect("decode request payload");

    assert_eq!(decoded.kind, FrameKind::Request);
    assert_eq!(decoded.major, PROTOCOL_MAJOR);
    assert_eq!(decoded_request, request);
}

#[test]
fn protocol_rejects_invalid_magic() {
    let mut bytes = vec![0_u8; 11];
    bytes[..4].copy_from_slice(b"BAD!");

    let error = decode_frame(&mut bytes.as_slice()).expect_err("invalid magic must fail");

    assert!(matches!(error, ProtocolError::InvalidMagic));
}

#[test]
fn protocol_rejects_payload_above_limit_before_reading_body() {
    let mut bytes = Vec::from(*b"CCS1");
    bytes.push(FrameKind::Request as u8);
    bytes.extend_from_slice(&PROTOCOL_MAJOR.to_be_bytes());
    bytes.extend_from_slice(&((MAX_FRAME_BYTES as u32) + 1).to_be_bytes());

    let error = decode_frame(&mut bytes.as_slice()).expect_err("oversized frame must fail");

    assert!(matches!(
        error,
        ProtocolError::PayloadTooLarge { actual } if actual == MAX_FRAME_BYTES + 1
    ));
}

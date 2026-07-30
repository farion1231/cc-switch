use cc_switch_lib::remote::client::{RemoteClientError, RemoteSession};
use cc_switch_lib::remote::protocol::{
    decode_frame, encode_frame, write_frame, CancelRequest, Frame, FrameKind, HelloAck, RpcRequest,
    RpcResponse,
};
use serde_json::json;
use std::io::Cursor;
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc};
use std::time::Duration;

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
    let session = RemoteSession::connect(reader, writer, "3.18.0").expect("handshake");

    let result = session
        .invoke_with_id(
            "req-1",
            "op-1",
            "provider.list",
            json!({ "app": "claude" }),
            30_000,
        )
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
fn remote_session_discards_unknown_response_id_until_disconnect() {
    let reader = Cursor::new(server_frames("wrong-id"));
    let writer = Vec::new();
    let session = RemoteSession::connect(reader, writer, "3.18.0").expect("handshake");

    let error = session
        .invoke_with_id(
            "req-1",
            "op-1",
            "provider.list",
            json!({ "app": "claude" }),
            30_000,
        )
        .expect_err("未知响应后连接关闭必须报告离线");

    assert!(matches!(error, RemoteClientError::Offline(_)));
}

fn tcp_session(
    server: impl FnOnce(TcpStream) + Send + 'static,
) -> RemoteSession<TcpStream, TcpStream> {
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定测试端口");
    let address = listener.local_addr().expect("读取测试地址");
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("接受测试连接");
        server(stream);
    });
    let writer = TcpStream::connect(address).expect("连接测试服务端");
    let reader = writer.try_clone().expect("复制读取流");
    RemoteSession::connect(reader, writer, "3.18.0").expect("完成测试握手")
}

fn acknowledge(stream: &mut TcpStream) {
    assert_eq!(
        decode_frame(stream).expect("读取 hello").kind,
        FrameKind::Hello
    );
    write_frame(
        stream,
        &Frame::from_json(
            FrameKind::HelloAck,
            &HelloAck {
                agent_version: "3.18.0".to_string(),
                protocol_minor: 0,
                schema_version: 16,
                platform: "linux".to_string(),
                architecture: "x86_64".to_string(),
                capabilities: vec!["usage.summary".to_string()],
            },
        )
        .expect("编码 hello ack"),
    )
    .expect("写入 hello ack");
}

#[test]
fn concurrent_requests_receive_their_own_out_of_order_responses() {
    let session = Arc::new(tcp_session(|mut stream| {
        acknowledge(&mut stream);
        let first: RpcRequest = decode_frame(&mut stream)
            .expect("读取第一条请求")
            .json()
            .expect("解析第一条请求");
        let second: RpcRequest = decode_frame(&mut stream)
            .expect("读取第二条请求")
            .json()
            .expect("解析第二条请求");
        for request in [second, first] {
            write_frame(
                &mut stream,
                &Frame::from_json(
                    FrameKind::Response,
                    &RpcResponse {
                        id: request.id.clone(),
                        result: Some(json!({ "requestId": request.id })),
                        error: None,
                    },
                )
                .expect("编码倒序响应"),
            )
            .expect("写入倒序响应");
        }
    }));

    let first_session = Arc::clone(&session);
    let first = std::thread::spawn(move || {
        first_session.invoke_with_id(
            "request-1",
            "operation-1",
            "usage.summary",
            json!({}),
            1_000,
        )
    });
    let second_session = Arc::clone(&session);
    let second = std::thread::spawn(move || {
        second_session.invoke_with_id(
            "request-2",
            "operation-2",
            "usage.summary",
            json!({}),
            1_000,
        )
    });

    assert_eq!(
        first.join().expect("等待第一请求").expect("第一请求成功")["requestId"],
        "request-1"
    );
    assert_eq!(
        second.join().expect("等待第二请求").expect("第二请求成功")["requestId"],
        "request-2"
    );
}

#[test]
fn timed_out_request_sends_cancel_for_the_same_operation() {
    let (cancel_sender, cancel_receiver) = mpsc::channel();
    let session = tcp_session(move |mut stream| {
        acknowledge(&mut stream);
        let request: RpcRequest = decode_frame(&mut stream)
            .expect("读取超时请求")
            .json()
            .expect("解析超时请求");
        let cancel_frame = decode_frame(&mut stream).expect("读取取消帧");
        assert_eq!(cancel_frame.kind, FrameKind::Cancel);
        let cancel: CancelRequest = cancel_frame.json().expect("解析取消帧");
        cancel_sender.send((request, cancel)).expect("回传取消帧");
    });

    let error = session
        .invoke_with_id(
            "request-timeout",
            "operation-timeout",
            "usage.session_sync",
            json!({}),
            20,
        )
        .expect_err("无响应请求必须超时");
    assert_eq!(error.code(), "REMOTE_OPERATION_TIMEOUT");
    let (request, cancel) = cancel_receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("服务端收到 Cancel");
    assert_eq!(request.id, "request-timeout");
    assert_eq!(request.operation_id.as_deref(), Some("operation-timeout"));
    assert_eq!(cancel.request_id, "request-timeout");
    assert_eq!(cancel.operation_id, "operation-timeout");
}

#[test]
fn duplicate_request_id_does_not_replace_the_original_waiter() {
    let (seen_sender, seen_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let session = Arc::new(tcp_session(move |mut stream| {
        acknowledge(&mut stream);
        let request: RpcRequest = decode_frame(&mut stream)
            .expect("读取原始请求")
            .json()
            .expect("解析原始请求");
        seen_sender.send(()).expect("通知原始请求已到达");
        release_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("等待释放响应");
        write_frame(
            &mut stream,
            &Frame::from_json(
                FrameKind::Response,
                &RpcResponse {
                    id: request.id,
                    result: Some(json!({ "ok": true })),
                    error: None,
                },
            )
            .expect("编码原始响应"),
        )
        .expect("写入原始响应");
    }));
    let first_session = Arc::clone(&session);
    let first = std::thread::spawn(move || {
        first_session.invoke_with_id(
            "duplicate-id",
            "original-operation",
            "usage.summary",
            json!({}),
            1_000,
        )
    });
    seen_receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("服务端收到原始请求");

    let duplicate = session
        .invoke_with_id(
            "duplicate-id",
            "duplicate-operation",
            "usage.summary",
            json!({}),
            1_000,
        )
        .expect_err("重复 request ID 必须被拒绝");
    assert!(matches!(
        duplicate,
        RemoteClientError::DuplicateRequestId(id) if id == "duplicate-id"
    ));
    release_sender.send(()).expect("释放原始响应");
    assert_eq!(
        first.join().expect("等待原始调用").expect("原始调用成功")["ok"],
        true
    );
}

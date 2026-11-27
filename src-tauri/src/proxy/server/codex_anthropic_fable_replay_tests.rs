use super::{
    assert_output_call, message, receive_messages, stream_events, AppType, Database, MockUpstream,
    Provider, ProviderMeta, ProxyConfig, ProxyServer, ARGUMENT_ENVELOPE,
};
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, StatusCode},
    response::Response,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};

const FABLE: &str = "claude-fable-5-2";
const PRIMARY_PROVIDER: &str = "fable-test-primary";
const CALL_ID: &str = "toolu_test";
const TOOL_NAME: &str = "automation_update";

struct TestProxy {
    proxy: ProxyServer,
    local: String,
    upstream: String,
    mock: MockUpstream,
    upstream_task: JoinHandle<()>,
    db: Arc<Database>,
    client: reqwest::Client,
}

impl TestProxy {
    async fn start() -> Self {
        let mock = MockUpstream::default();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/v1/messages", post(receive_fable_messages))
            .with_state(mock.clone());
        let upstream_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Arc::new(Database::memory().unwrap());
        save_provider(&db, &upstream, PRIMARY_PROVIDER, FABLE);
        let (proxy, local) = start_proxy(db.clone()).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        Self {
            proxy,
            local,
            upstream,
            mock,
            upstream_task,
            db,
            client,
        }
    }

    async fn send(&self, request: &Value, session: Option<&str>) -> (StatusCode, String) {
        let mut post = self
            .client
            .post(format!("{}/v1/responses", self.local))
            .json(request);
        if let Some(session) = session {
            post = post.header("session_id", format!("fable-replay-fixture-{session}"));
        }
        let response = post.send().await.unwrap();
        (response.status(), response.text().await.unwrap())
    }

    async fn queue(&self, reply: (&'static str, String)) {
        self.mock.replies.lock().await.push_back(reply);
    }

    async fn last_request(&self) -> Value {
        self.mock.requests.lock().await.last().unwrap().1.clone()
    }

    async fn complete_tool_turn(
        &self,
        transport: Transport,
        request: &Value,
        session: Option<&str>,
        model: &str,
        blocks: &[Value],
    ) -> Vec<Value> {
        self.queue(transport.reply(model, blocks, "tool_use")).await;
        let (status, wire) = self.send(request, session).await;
        assert_eq!(status, StatusCode::OK, "{transport:?}: {wire}");
        let output = transport.response(&wire)["output"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(self.last_request().await["model"], model);
        output
    }

    async fn finish_replay(&self, request: &Value, session: Option<&str>, model: &str) -> Value {
        self.queue(Transport::Json.reply(
            model,
            &[json!({"type":"text","text":"done"})],
            "end_turn",
        ))
        .await;
        let (status, wire) = self.send(request, session).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "proxy rejected the tool replay: {wire}"
        );
        self.last_request().await
    }

    async fn assert_replay_rejected(&self, request: &Value, session: Option<&str>, case: &str) {
        let count = self.mock.requests.lock().await.len();
        let (status, wire) = self.send(request, session).await;
        assert!(
            !status.is_success() && wire.contains("thinking"),
            "{case}: expected a thinking-history error, got {status}: {wire}"
        );
        assert_eq!(
            self.mock.requests.lock().await.len(),
            count,
            "{case}: replay reached the upstream"
        );
    }

    async fn restart(&mut self) {
        self.proxy.stop().await.unwrap();
        (self.proxy, self.local) = start_proxy(self.db.clone()).await;
    }

    async fn stop(&self) {
        assert!(self.mock.replies.lock().await.is_empty());
        self.proxy.stop().await.unwrap();
        self.upstream_task.abort();
    }
}

async fn receive_fable_messages(state: State<MockUpstream>, body: Json<Value>) -> Response {
    let (status, [(_, content_type)], wire) = receive_messages(state, body).await;
    let body = if content_type == "text/event-stream" {
        let chunks: Vec<_> = wire
            .split_inclusive("\n\n")
            .map(|frame| Ok::<_, Infallible>(Bytes::copy_from_slice(frame.as_bytes())))
            .collect();
        Body::from_stream(futures::stream::iter(chunks))
    } else {
        Body::from(wire)
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap()
}

async fn start_proxy(db: Arc<Database>) -> (ProxyServer, String) {
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..ProxyConfig::default()
        },
        db,
        None,
    );
    let local = format!("http://127.0.0.1:{}", proxy.start().await.unwrap().port);
    (proxy, local)
}

fn save_provider(db: &Database, upstream: &str, id: &str, model: &str) {
    let mut provider = Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "base_url": upstream,
            "auth": {"OPENAI_API_KEY":"mock-upstream-key"},
            "config": format!("model = \"{model}\"")
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some("anthropic".into()),
        ..ProviderMeta::default()
    });
    db.save_provider(&AppType::Codex, &provider).unwrap();
    db.set_current_provider(&AppType::Codex, id).unwrap();
}

#[derive(Clone, Copy, Debug)]
enum Transport {
    Json,
    Sse,
    UnmarkedSse,
    JsonForSse,
}

impl Transport {
    fn request_stream(self) -> bool {
        matches!(self, Self::Sse | Self::JsonForSse)
    }

    fn reply(self, model: &str, blocks: &[Value], stop_reason: &str) -> (&'static str, String) {
        match self {
            Self::Json | Self::JsonForSse => {
                let mut value = message(json!(blocks), stop_reason);
                value["model"] = json!(model);
                ("application/json", value.to_string())
            }
            Self::Sse => ("text/event-stream", sse_message(model, blocks, stop_reason)),
            Self::UnmarkedSse => (
                "application/octet-stream",
                sse_message(model, blocks, stop_reason),
            ),
        }
    }

    fn response(self, wire: &str) -> Value {
        if self.request_stream() {
            let events = stream_events(wire);
            return events
                .into_iter()
                .find(|event| event["type"] == "response.completed")
                .unwrap_or_else(|| panic!("missing response.completed in {wire}"))["response"]
                .clone();
        }
        serde_json::from_str(wire).unwrap()
    }

    fn assert_no_completed_turn(self, status: StatusCode, wire: &str) {
        if !status.is_success() {
            return;
        }
        if self.request_stream() {
            assert!(
                !stream_events(wire).iter().any(|event| {
                    event["type"] == "response.completed"
                        && event["response"]["status"] == "completed"
                }),
                "{wire}"
            );
        } else {
            assert_ne!(self.response(wire)["status"], "completed", "{wire}");
        }
    }
}

fn sse_message(model: &str, blocks: &[Value], stop_reason: &str) -> String {
    let mut start_message = message(json!([]), stop_reason);
    start_message["model"] = json!(model);
    start_message["stop_reason"] = Value::Null;
    let mut frames = vec![json!({"type":"message_start","message":start_message})];
    for (index, block) in blocks.iter().enumerate() {
        let mut start = block.clone();
        let mut deltas = Vec::new();
        match block["type"].as_str().unwrap() {
            "tool_use" => {
                start["input"] = json!({});
                let input = block["input"].to_string();
                let (first, second) = input.split_at(input.len() / 2);
                deltas.extend([first, second].map(
                    |partial_json| json!({"type":"input_json_delta","partial_json":partial_json}),
                ));
            }
            "thinking" => {
                start = json!({"type":"thinking","thinking":"","signature":""});
                if let Some(thinking) = block["thinking"].as_str().filter(|text| !text.is_empty()) {
                    deltas.push(json!({"type":"thinking_delta","thinking":thinking}));
                }
                if let Some(signature) = block["signature"].as_str() {
                    deltas.push(json!({"type":"signature_delta","signature":signature}));
                }
            }
            _ => {}
        }
        frames.push(json!({"type":"content_block_start","index":index,"content_block":start}));
        frames.extend(
            deltas
                .into_iter()
                .map(|delta| json!({"type":"content_block_delta","index":index,"delta":delta})),
        );
        frames.push(json!({"type":"content_block_stop","index":index}));
    }
    frames.extend([
        json!({"type":"message_delta","delta":{"stop_reason":stop_reason},"usage":{"output_tokens":10}}),
        json!({"type":"message_stop"}),
    ]);
    frames
        .iter()
        .map(|frame| {
            format!(
                "event: {}\ndata: {frame}\n\n",
                frame["type"].as_str().unwrap()
            )
        })
        .collect()
}

fn request(model: &str, stream: bool, wrapped: bool) -> Value {
    let mut parameters = json!({
        "type":"object",
        "properties":{"mode":{"type":"string"}},
        "required":["mode"],
        "additionalProperties":false
    });
    if wrapped {
        parameters["oneOf"] = json!([
            {"properties":{"mode":{"const":"rename"}}},
            {"properties":{"mode":{"const":"archive"}}}
        ]);
    }
    json!({
        "model":model,
        "stream":stream,
        "max_output_tokens":8192,
        "reasoning":{"effort":"high"},
        "include":["reasoning.encrypted_content"],
        "tools":[{"type":"function","name":TOOL_NAME,"parameters":parameters}],
        "input":[{"role":"user","content":"update the automation"}]
    })
}

fn tool_block(wrapped: bool, id: &str) -> Value {
    let mut input = json!({"mode":"rename"});
    if wrapped {
        input = json!({ARGUMENT_ENVELOPE:input});
    }
    json!({"type":"tool_use","id":id,"name":TOOL_NAME,"input":input})
}

fn client_call(id: &str) -> Value {
    json!({"type":"function_call","call_id":id,"name":TOOL_NAME,"arguments":"{\"mode\":\"rename\"}"})
}

fn replay(request: &Value, output: &[Value], call_id: &str) -> Value {
    let mut request = request.clone();
    request["input"]
        .as_array_mut()
        .unwrap()
        .extend_from_slice(output);
    request["input"].as_array_mut().unwrap().push(json!({
        "type":"function_call_output","call_id":call_id,"output":"applied"
    }));
    request
}

fn assert_adaptive_high(wire: &Value) {
    assert_eq!(wire["thinking"], json!({"type":"adaptive"}), "{wire}");
    assert_eq!(wire["output_config"]["effort"], "high", "{wire}");
}

fn assert_replayed_tool(wire: &Value, expected_assistant: &[Value]) {
    assert_adaptive_high(wire);
    let messages = wire["messages"].as_array().unwrap();
    let assistant = &messages[messages.len() - 2];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["content"], json!(expected_assistant));
    let user = messages.last().unwrap();
    assert_eq!(user["role"], "user");
    let results = user["content"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["type"], "tool_result");
    assert_eq!(results[0]["tool_use_id"], CALL_ID);
    assert_eq!(results[0]["content"], "applied");
}

async fn zero_thinking_round_trip(transport: Transport, wrapped: bool) {
    let proxy = TestProxy::start().await;
    let session = Some("fable-zero-thinking-tool-turn");
    let request = request("client-facing-alias", transport.request_stream(), wrapped);
    let tool = tool_block(wrapped, CALL_ID);
    proxy
        .queue(transport.reply(FABLE, std::slice::from_ref(&tool), "tool_use"))
        .await;
    let (status, text) = proxy.send(&request, session).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{transport:?} wrapped={wrapped}: {text}"
    );
    let response = transport.response(&text);
    assert_output_call(&response, &json!({"mode":"rename"}));
    assert_eq!(response["output"].as_array().unwrap().len(), 1);
    let wire = proxy.last_request().await;
    assert_adaptive_high(&wire);
    assert_eq!(wire["model"], FABLE);
    assert_eq!(wire["stream"], transport.request_stream());
    let schema = &wire["tools"][0]["input_schema"];
    assert!(schema.get("oneOf").is_none());
    assert_eq!(
        schema["properties"][ARGUMENT_ENVELOPE]
            .get("oneOf")
            .is_some(),
        wrapped
    );

    let mut next = replay(&request, response["output"].as_array().unwrap(), CALL_ID);
    next["model"] = json!("another-client-alias");
    let wire = proxy.finish_replay(&next, session, FABLE).await;
    assert_eq!(wire["model"], FABLE);
    assert_replayed_tool(&wire, &[tool]);
    proxy.stop().await;
}

#[tokio::test]
async fn fable_52_zero_thinking_json_tool_replay() {
    zero_thinking_round_trip(Transport::Json, false).await;
    zero_thinking_round_trip(Transport::Json, true).await;
}

#[tokio::test]
async fn fable_52_zero_thinking_sse_tool_replay() {
    for transport in [
        Transport::Sse,
        Transport::UnmarkedSse,
        Transport::JsonForSse,
    ] {
        for wrapped in [false, true] {
            zero_thinking_round_trip(transport, wrapped).await;
        }
    }
}

#[tokio::test]
async fn fable_52_signed_thinking_and_tool_order_survive_replay() {
    let proxy = TestProxy::start().await;
    let blocks = [
        json!({"type":"thinking","thinking":"verify the conditions","signature":"mock-sig-visible"}),
        json!({"type":"text","text":"I will update the automation."}),
        json!({"type":"thinking","thinking":"","signature":"mock-sig-empty"}),
        json!({"type":"redacted_thinking","data":"mock-opaque-redaction"}),
        tool_block(true, CALL_ID),
    ];
    for (index, transport) in [
        Transport::Json,
        Transport::Sse,
        Transport::UnmarkedSse,
        Transport::JsonForSse,
    ]
    .into_iter()
    .enumerate()
    {
        let session_id = format!("fable-signed-{index}");
        let session = Some(session_id.as_str());
        let request = request(FABLE, transport.request_stream(), true);
        let output = proxy
            .complete_tool_turn(transport, &request, session, FABLE, &blocks)
            .await;
        let types: Vec<_> = output
            .iter()
            .map(|item| item["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            types,
            [
                "reasoning",
                "message",
                "reasoning",
                "reasoning",
                "function_call"
            ]
        );
        assert_eq!(output[0]["summary"][0]["text"], "verify the conditions");
        assert_eq!(output[2]["summary"], json!([]));
        assert_eq!(output[3]["summary"], json!([]));
        for item in [&output[0], &output[2], &output[3]] {
            assert!(item["encrypted_content"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
        }
        let next = replay(&request, &output, CALL_ID);
        let wire = proxy.finish_replay(&next, session, FABLE).await;
        assert_replayed_tool(&wire, &blocks);
        let omitted_reasoning = replay(&request, &[output[1].clone(), output[4].clone()], CALL_ID);
        proxy
            .assert_replay_rejected(&omitted_reasoning, session, "signed reasoning omitted")
            .await;
        let mut reordered = output;
        reordered.swap(0, 2);
        let next = replay(&request, &reordered, CALL_ID);
        proxy
            .assert_replay_rejected(&next, session, "signed reasoning reordered")
            .await;
    }
    proxy.stop().await;
}

#[tokio::test]
async fn fable_52_unsigned_replay_is_scoped_to_observed_tool_turn() {
    let mut proxy = TestProxy::start().await;
    let session = Some("fable-origin-session");
    let request = request(FABLE, false, false);
    let fabricated = replay(&request, &[client_call(CALL_ID)], CALL_ID);
    proxy
        .assert_replay_rejected(&fabricated, session, "before observation")
        .await;

    let tool = tool_block(false, CALL_ID);
    let output = proxy
        .complete_tool_turn(
            Transport::Json,
            &request,
            session,
            FABLE,
            std::slice::from_ref(&tool),
        )
        .await;
    let next = replay(&request, &output, CALL_ID);
    proxy
        .assert_replay_rejected(&next, Some("fable-other-session"), "different session")
        .await;
    proxy
        .assert_replay_rejected(&next, None, "missing session")
        .await;

    let other_id = "toolu_fabricated";
    let unobserved = replay(&request, &[client_call(other_id)], other_id);
    proxy
        .assert_replay_rejected(&unobserved, session, "different tool call ID")
        .await;
    let mut edited_call = output[0].clone();
    edited_call["arguments"] = json!("{\"mode\":\"archive\"}");
    let edited = replay(&request, &[edited_call], CALL_ID);
    proxy
        .assert_replay_rejected(&edited, session, "same tool ID with edited arguments")
        .await;

    save_provider(&proxy.db, &proxy.upstream, "fable-test-secondary", FABLE);
    proxy
        .assert_replay_rejected(&next, session, "different provider")
        .await;
    save_provider(
        &proxy.db,
        &proxy.upstream,
        PRIMARY_PROVIDER,
        "claude-fable-5.2",
    );
    proxy
        .assert_replay_rejected(&next, session, "different outbound Fable 5.2 model ID")
        .await;
    save_provider(
        &proxy.db,
        &proxy.upstream,
        PRIMARY_PROVIDER,
        "claude-fable-5-1",
    );
    proxy
        .assert_replay_rejected(&next, session, "different actual model")
        .await;
    save_provider(&proxy.db, &proxy.upstream, PRIMARY_PROVIDER, FABLE);
    let wire = proxy.finish_replay(&next, session, FABLE).await;
    assert_replayed_tool(&wire, &[tool]);
    proxy.restart().await;
    proxy
        .assert_replay_rejected(&next, session, "proxy restarted")
        .await;
    proxy.stop().await;
}

#[tokio::test]
async fn fable_52_parallel_replay_requires_every_result_and_allows_retry() {
    let proxy = TestProxy::start().await;
    let session = Some("parallel-tools");
    let request = request(FABLE, false, false);
    let second_id = "toolu_second";
    let tools = [tool_block(false, CALL_ID), tool_block(false, second_id)];
    let output = proxy
        .complete_tool_turn(Transport::Json, &request, session, FABLE, &tools)
        .await;

    let missing = replay(&request, &output, CALL_ID);
    proxy
        .assert_replay_rejected(&missing, session, "missing parallel result")
        .await;
    for id in [CALL_ID, "toolu_unknown"] {
        let mut wrong = missing.clone();
        wrong["input"].as_array_mut().unwrap().push(json!({
            "type":"function_call_output","call_id":id,"output":"second"
        }));
        proxy
            .assert_replay_rejected(&wrong, session, "duplicate or mismatched result")
            .await;
    }

    let mut fresh = missing.clone();
    fresh["input"].as_array_mut().unwrap().push(json!({
        "type":"message","role":"user","content":"Start a new task"
    }));
    let wire = proxy.finish_replay(&fresh, session, FABLE).await;
    let messages = wire["messages"].as_array().unwrap();
    let latest = messages.last().unwrap()["content"].as_array().unwrap();
    assert!(latest
        .iter()
        .any(|block| block["text"] == "Start a new task"));
    assert!(latest.iter().all(|block| block["type"] != "tool_result"));

    let mut complete = missing;
    complete["input"].as_array_mut().unwrap().push(json!({
        "type":"function_call_output","call_id":second_id,"output":"second"
    }));
    for _ in 0..2 {
        let wire = proxy.finish_replay(&complete, session, FABLE).await;
        assert_adaptive_high(&wire);
        let messages = wire["messages"].as_array().unwrap();
        assert_eq!(messages[messages.len() - 2]["content"], json!(tools));
        let results = messages.last().unwrap()["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["tool_use_id"], CALL_ID);
        assert_eq!(results[1]["tool_use_id"], second_id);
    }
    proxy.stop().await;
}

#[tokio::test]
async fn fable_52_incomplete_or_unsigned_thinking_does_not_authorize_replay() {
    let proxy = TestProxy::start().await;
    for transport in [
        Transport::Json,
        Transport::Sse,
        Transport::UnmarkedSse,
        Transport::JsonForSse,
    ] {
        let session_id = format!("fable-unsigned-thinking-{transport:?}");
        let session = Some(session_id.as_str());
        let request = request(FABLE, transport.request_stream(), false);
        proxy
            .queue(transport.reply(
                FABLE,
                &[
                    json!({"type":"thinking","thinking":"unsigned"}),
                    tool_block(false, CALL_ID),
                ],
                "tool_use",
            ))
            .await;
        let (status, wire) = proxy.send(&request, session).await;
        transport.assert_no_completed_turn(status, &wire);
        let next = replay(&request, &[client_call(CALL_ID)], CALL_ID);
        proxy
            .assert_replay_rejected(&next, session, "unsigned thinking block")
            .await;
    }

    for transport in [Transport::Sse, Transport::UnmarkedSse] {
        for wrapped in [false, true] {
            let session_id = format!("fable-truncated-{transport:?}-{wrapped}");
            let session = Some(session_id.as_str());
            let request = request(FABLE, transport.request_stream(), wrapped);
            let tool = tool_block(wrapped, CALL_ID);
            let (media_type, wire) = transport.reply(FABLE, &[tool], "tool_use");
            // End the connection after the tool block closes and before the
            // upstream sends the message's terminal frames.
            let (partial, _) = wire.split_once("event: message_delta\n").unwrap();
            proxy.queue((media_type, partial.into())).await;
            let (status, wire) = proxy.send(&request, session).await;
            transport.assert_no_completed_turn(status, &wire);
            let next = replay(&request, &[client_call(CALL_ID)], CALL_ID);
            proxy
                .assert_replay_rejected(&next, session, "truncated response")
                .await;
        }
    }

    let session = Some("fable-max-tokens");
    let request = request(FABLE, false, false);
    proxy
        .queue(Transport::Json.reply(FABLE, &[tool_block(false, CALL_ID)], "max_tokens"))
        .await;
    let (status, wire) = proxy.send(&request, session).await;
    Transport::Json.assert_no_completed_turn(status, &wire);
    let next = replay(&request, &[client_call(CALL_ID)], CALL_ID);
    proxy
        .assert_replay_rejected(&next, session, "max_tokens")
        .await;
    proxy.stop().await;
}

#[tokio::test]
async fn fable_52_error_json_cannot_authorize_tool_replay() {
    let proxy = TestProxy::start().await;
    for transport in [Transport::Json, Transport::JsonForSse] {
        let session_id = format!("fable-json-error-{transport:?}");
        let session = Some(session_id.as_str());
        let request = request(FABLE, transport.request_stream(), false);
        let mut body = message(json!([tool_block(false, CALL_ID)]), "tool_use");
        body["model"] = json!(FABLE);
        body["error"] = json!({"type":"overloaded_error","message":"fixture error"});
        proxy.queue(("application/json", body.to_string())).await;
        let (status, wire) = proxy.send(&request, session).await;
        transport.assert_no_completed_turn(status, &wire);
        let next = replay(&request, &[client_call(CALL_ID)], CALL_ID);
        proxy
            .assert_replay_rejected(&next, session, "JSON tool turn containing error")
            .await;
    }
    proxy.stop().await;
}

#[tokio::test]
async fn fable_52_read_replay_uses_sanitized_arguments() {
    let proxy = TestProxy::start().await;
    for transport in [
        Transport::Json,
        Transport::Sse,
        Transport::UnmarkedSse,
        Transport::JsonForSse,
    ] {
        for wrapped in [false, true] {
            let session_id = format!("fable-read-{transport:?}-{wrapped}");
            let session = Some(session_id.as_str());
            let mut request = request(FABLE, transport.request_stream(), false);
            let mut schema = json!({
                "type":"object",
                "properties":{"file_path":{"type":"string"},"pages":{"type":"string"}},
                "required":["file_path"],
                "additionalProperties":false
            });
            if wrapped {
                schema["oneOf"] = json!([
                    {"properties":{"file_path":{"const":"fixture.txt"}}},
                    {"properties":{"file_path":{"const":"other.txt"}}}
                ]);
            }
            request["tools"] = json!([{"type":"function","name":"Read","parameters":schema}]);
            let raw = json!({"file_path":"fixture.txt","pages":""});
            let sanitized = json!({"file_path":"fixture.txt"});
            let (raw, expected) = if wrapped {
                (
                    json!({ARGUMENT_ENVELOPE:raw}),
                    json!({ARGUMENT_ENVELOPE:sanitized}),
                )
            } else {
                (raw, sanitized.clone())
            };
            let block = json!({"type":"tool_use","name":"Read","id":CALL_ID,"input":raw});
            let output = proxy
                .complete_tool_turn(transport, &request, session, FABLE, &[block])
                .await;
            assert_eq!(output.len(), 1);
            assert_eq!(output[0]["name"], "Read");
            assert_eq!(output[0]["call_id"], CALL_ID);
            let arguments =
                serde_json::from_str::<Value>(output[0]["arguments"].as_str().unwrap()).unwrap();
            assert_eq!(arguments, sanitized);
            let next = replay(&request, &output, CALL_ID);
            let wire = proxy.finish_replay(&next, session, FABLE).await;
            assert_replayed_tool(
                &wire,
                &[json!({"type":"tool_use","name":"Read","id":CALL_ID,"input":expected})],
            );
        }
    }
    proxy.stop().await;
}

#[tokio::test]
async fn fable_52_sse_replay_stops_at_message_stop() {
    let proxy = TestProxy::start().await;
    let tool = tool_block(false, CALL_ID);
    let mut upstream = sse_message(FABLE, std::slice::from_ref(&tool), "tool_use");
    upstream.push_str(concat!(
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"late fixture error\"}}\n\n"
    ));
    for (index, (media_type, transport)) in [
        ("text/event-stream", Transport::Sse),
        ("text/event-stream; charset=utf-8", Transport::Sse),
        ("application/octet-stream", Transport::UnmarkedSse),
    ]
    .into_iter()
    .enumerate()
    {
        let session_id = format!("fable-sse-terminal-{index}");
        let session = Some(session_id.as_str());
        let request = request(FABLE, transport.request_stream(), false);
        proxy.queue((media_type, upstream.clone())).await;
        let (status, wire) = proxy.send(&request, session).await;
        assert_eq!(status, StatusCode::OK, "{media_type}: {wire}");
        let response = transport.response(&wire);
        assert_eq!(response["status"], "completed");
        assert_output_call(&response, &json!({"mode":"rename"}));
        let next = replay(&request, response["output"].as_array().unwrap(), CALL_ID);
        let wire = proxy.finish_replay(&next, session, FABLE).await;
        assert_replayed_tool(&wire, std::slice::from_ref(&tool));
    }
    proxy.stop().await;
}

#[tokio::test]
async fn fable_52_unsigned_replay_keeps_other_model_behavior() {
    let proxy = TestProxy::start().await;
    let request = request("client-facing-alias", false, false);
    let next = replay(&request, &[client_call(CALL_ID)], CALL_ID);
    let session = Some("fable-control-session");
    for model in ["claude-opus-5-5", "claude-sonnet-5-5"] {
        save_provider(&proxy.db, &proxy.upstream, PRIMARY_PROVIDER, model);
        let wire = proxy.finish_replay(&next, session, model).await;
        assert_eq!(wire["model"], model);
        assert_replayed_tool(&wire, &[tool_block(false, CALL_ID)]);
    }
    save_provider(
        &proxy.db,
        &proxy.upstream,
        PRIMARY_PROVIDER,
        "claude-opus-4-8",
    );
    let wire = proxy.finish_replay(&next, session, "claude-opus-4-8").await;
    assert_eq!(wire["thinking"], json!({"type":"disabled"}));
    proxy.stop().await;
}

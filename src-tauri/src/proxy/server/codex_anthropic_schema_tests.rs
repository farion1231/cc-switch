use super::{ProxyConfig, ProxyServer};
use crate::{AppType, Database, Provider, ProviderMeta};
use axum::{
    extract::State,
    http::{header, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Mutex};

const ARGUMENT_ENVELOPE: &str = "cc_switch_args";

#[path = "codex_anthropic_fable_replay_tests.rs"]
mod codex_anthropic_fable_replay_tests;

type MockReply = (StatusCode, [(header::HeaderName, &'static str); 1], String);

#[derive(Clone, Default)]
struct MockUpstream {
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    replies: Arc<Mutex<VecDeque<(&'static str, String)>>>,
}

async fn receive_messages(State(mock): State<MockUpstream>, Json(body): Json<Value>) -> MockReply {
    let rejects_root_composition = body["tools"].as_array().into_iter().flatten().any(|tool| {
        let schema = &tool["input_schema"];
        ["oneOf", "anyOf", "allOf"]
            .iter()
            .any(|key| schema.get(key).is_some())
    });
    mock.requests
        .lock()
        .await
        .push(("/v1/messages".into(), body));
    if rejects_root_composition {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            json!({"type":"error", "error":{"type":"invalid_request_error",
                "message":"tools.0.custom.input_schema: root composition is not supported"}})
            .to_string(),
        );
    }
    receive_reply(&mock).await
}

async fn receive_responses(State(mock): State<MockUpstream>, Json(body): Json<Value>) -> MockReply {
    mock.requests
        .lock()
        .await
        .push(("/v1/responses".into(), body));
    receive_reply(&mock).await
}

async fn receive_chat(State(mock): State<MockUpstream>, Json(body): Json<Value>) -> MockReply {
    mock.requests
        .lock()
        .await
        .push(("/v1/chat/completions".into(), body));
    receive_reply(&mock).await
}

async fn receive_reply(mock: &MockUpstream) -> MockReply {
    let (content_type, body) = mock
        .replies
        .lock()
        .await
        .pop_front()
        .expect("queued mock reply");
    (StatusCode::OK, [(header::CONTENT_TYPE, content_type)], body)
}

fn register_provider(db: &Database, app: AppType, format: &str, base_url: &str) {
    let settings = if app == AppType::Claude {
        json!({"env":{"ANTHROPIC_BASE_URL":base_url,"ANTHROPIC_API_KEY":"mock-upstream-key"}})
    } else {
        json!({"base_url":base_url,"auth":{"OPENAI_API_KEY":"mock-upstream-key"}})
    };
    let mut provider = Provider::with_id(format.into(), format.into(), settings, None);
    provider.meta = Some(ProviderMeta {
        api_format: Some(format.into()),
        ..ProviderMeta::default()
    });
    db.save_provider(&app, &provider)
        .expect("save mock provider");
    db.set_current_provider(&app, &provider.id)
        .expect("select mock provider");
}

fn message(content: Value, stop_reason: &str) -> Value {
    json!({"type":"message","id":"msg_test","role":"assistant","model":"claude-test",
        "content":content,"stop_reason":stop_reason,"usage":{"input_tokens":10,"output_tokens":10}})
}

fn tool_stream(argument: &Value, complete: bool) -> String {
    let argument = argument.to_string();
    let (first, second) = argument.split_at(argument.len() / 2);
    let mut frames = vec![
        json!({"type":"message_start","message":message(json!([]), "tool_use")}),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"toolu_test","name":"automation_update","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":first}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":second}}),
    ];
    if complete {
        frames.extend([
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":10}}),
            json!({"type":"message_stop"}),
        ]);
    }
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

fn assert_output_call(value: &Value, arguments: &Value) {
    let call = &value["output"][0];
    assert_eq!(call["type"], "function_call", "{value}");
    assert_eq!(call["name"], "automation_update");
    assert_eq!(call["call_id"], "toolu_test");
    assert_eq!(
        serde_json::from_str::<Value>(call["arguments"].as_str().unwrap()).unwrap(),
        *arguments
    );
}

fn stream_events(wire: &str) -> Vec<Value> {
    wire.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

#[tokio::test]
async fn codex_anthropic_composed_tool_schema_real_routes() {
    let mock = MockUpstream::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let upstream = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/v1/messages", post(receive_messages))
        .route("/v1/responses", post(receive_responses))
        .route("/v1/chat/completions", post(receive_chat))
        .with_state(mock.clone());
    let mock_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let db = Arc::new(Database::memory().unwrap());
    register_provider(&db, AppType::Codex, "anthropic", &upstream);
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..ProxyConfig::default()
        },
        db.clone(),
        None,
    );
    let local = format!("http://127.0.0.1:{}", proxy.start().await.unwrap().port);
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let send =
        |path: &str, request: &Value| client.post(format!("{local}{path}")).json(request).send();

    let opaque = json!({"_id":"neo","nested":[{"__typename":"Marker"}]});
    let arguments = json!({"_payload":opaque,"mode":"rename"});
    let envelope = json!({ARGUMENT_ENVELOPE:arguments});
    let schema = json!({
        "type":"object",
        "properties":{"_payload":{"$ref":"#/$defs/_payload"},"mode":{"type":"string"}},
        "$defs":{"_payload":{"type":"object","const":opaque,"enum":[opaque],
            "examples":[opaque],"default":opaque,"_schema_note":"private"}},
        "required":["_payload","mode"],"additionalProperties":false,
        "oneOf":[{"properties":{"mode":{"const":"rename"}}},
                 {"properties":{"mode":{"const":"archive"}}}]
    });
    let response_tool =
        json!({"type":"function","name":"automation_update","parameters":schema,"strict":true});
    let deferred_tool = json!({"type":"function","name":"deferred_update","parameters":{
        "type":"object","allOf":[{"required":["id"]},{"properties":{"id":{"type":"string"}}}]
    }});
    let ordinary_tool = json!({"type":"function","name":"ordinary","parameters":{
        "type":"object","properties":{ARGUMENT_ENVELOPE:{"type":"object"}}
    }});
    let request = json!({"model":"claude-test","stream":false,"max_output_tokens":100,
        "tools":[response_tool,ordinary_tool], "input":[{"role":"user","content":"hello"},
          {"type":"tool_search_output","call_id":"lookup_test","tools":[deferred_tool]}]});
    let response_message = message(
        json!([
            {"type":"tool_use","name":"automation_update","id":"toolu_test","input":envelope},
            {"type":"tool_use","name":"ordinary","id":"toolu_plain","input":envelope}
        ]),
        "tool_use",
    );
    mock.replies
        .lock()
        .await
        .push_back(("application/json", response_message.to_string()));
    let result = send("/v1/responses", &request).await.unwrap();
    let status = result.status();
    let text = result.text().await.unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "upstream rejected the tool schema: {text}"
    );
    let output: Value = serde_json::from_str(&text).unwrap();
    assert_output_call(&output, &arguments);
    assert_eq!(output["output"][1]["name"], "ordinary");
    assert_eq!(
        serde_json::from_str::<Value>(output["output"][1]["arguments"].as_str().unwrap()).unwrap(),
        envelope
    );

    {
        let requests = mock.requests.lock().await;
        let tools = requests[0].1["tools"].as_array().unwrap();
        let wire_tool = tools
            .iter()
            .find(|tool| tool["name"] == "automation_update")
            .unwrap();
        let wire_schema = &wire_tool["input_schema"];
        let mut expected = schema.clone();
        expected["properties"]["_payload"]["$ref"] =
            json!("#/properties/cc_switch_args/$defs/_payload");
        expected["$defs"]["_payload"]
            .as_object_mut()
            .unwrap()
            .remove("_schema_note");
        assert_eq!(
            *wire_schema,
            json!({"type":"object","required":[ARGUMENT_ENVELOPE],
            "additionalProperties":false,"properties":{ARGUMENT_ENVELOPE:expected}})
        );
        assert!(wire_tool.get("strict").is_none());
        let deferred = tools
            .iter()
            .find(|tool| tool["name"] == "deferred_update")
            .unwrap();
        assert_eq!(
            deferred["input_schema"]["properties"][ARGUMENT_ENVELOPE],
            deferred_tool["parameters"]
        );
    }

    let mut replay = request.clone();
    replay["input"].as_array_mut().unwrap().extend([
        output["output"][0].clone(),
        json!({"type":"function_call_output","call_id":"toolu_test","output":"applied"}),
    ]);
    mock.replies.lock().await.push_back((
        "application/json",
        message(
            json!([
                {"type":"text","text":"done"}
            ]),
            "end_turn",
        )
        .to_string(),
    ));
    assert_eq!(
        send("/v1/responses", &replay).await.unwrap().status(),
        StatusCode::OK
    );
    {
        let requests = mock.requests.lock().await;
        let blocks: Vec<_> = requests[1].1["messages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|message| message["content"].as_array().into_iter().flatten())
            .collect();
        let call = blocks
            .iter()
            .find(|block| block["type"] == "tool_use")
            .unwrap();
        let result = blocks
            .iter()
            .find(|block| block["type"] == "tool_result")
            .unwrap();
        assert_eq!(call["input"], envelope);
        assert_eq!(call["id"], result["tool_use_id"]);
        assert_eq!(result["content"], "applied");
    }

    let mut streamed = request.clone();
    streamed["stream"] = json!(true);
    for (content_type, wire) in [
        ("text/event-stream", tool_stream(&envelope, true)),
        ("application/json", response_message.to_string()),
    ] {
        mock.replies.lock().await.push_back((content_type, wire));
        let response = send("/v1/responses", &streamed).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let events = stream_events(&response.text().await.unwrap());
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        assert_output_call(&completed["response"], &arguments);
        let deltas: String = events
            .iter()
            .filter(|event| {
                event["type"] == "response.function_call_arguments.delta"
                    && event["output_index"] == 0
            })
            .map(|event| event["delta"].as_str().unwrap())
            .collect();
        assert_eq!(serde_json::from_str::<Value>(&deltas).unwrap(), arguments);
    }

    mock.replies
        .lock()
        .await
        .push_back(("application/octet-stream", tool_stream(&envelope, true)));
    let response = send("/v1/responses", &request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_output_call(&response.json::<Value>().await.unwrap(), &arguments);

    mock.replies
        .lock()
        .await
        .push_back(("text/event-stream", tool_stream(&envelope, false)));
    let events = stream_events(
        &send("/v1/responses", &streamed)
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
    );
    assert!(events
        .iter()
        .any(|event| matches!(event["type"].as_str(), Some("error" | "response.failed"))));
    assert!(!events.iter().any(|event| matches!(
        event["type"].as_str(),
        Some(
            "response.completed"
                | "response.output_item.done"
                | "response.function_call_arguments.done"
                | "response.function_call_arguments.delta"
        )
    )));

    mock.replies
        .lock()
        .await
        .push_back(("application/octet-stream", tool_stream(&envelope, false)));
    let response = send("/v1/responses", &request).await.unwrap();
    assert!(!response.status().is_success());

    register_provider(&db, AppType::Claude, "anthropic", &upstream);
    let native_messages = json!({"model":"claude-test","max_tokens":100,"_private":"drop me",
        "tools":[{"name":"ordinary","input_schema":{"type":"object","properties":{"_payload":{"type":"object"}}}}],
        "messages":[{"role":"user","content":"go"},{"role":"assistant","content":[
           {"type":"tool_use","id":"toolu_native","name":"ordinary","input":arguments}
        ]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_native","content":"done"}]}]});
    mock.replies
        .lock()
        .await
        .push_back(("application/json", response_message.to_string()));
    let response = send("/v1/messages", &native_messages).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap(), response_message);
    {
        let requests = mock.requests.lock().await;
        let (_, last) = requests.last().unwrap();
        assert_eq!(last["messages"], native_messages["messages"]);
        assert_eq!(
            last["tools"][0]["input_schema"],
            native_messages["tools"][0]["input_schema"]
        );
        assert!(last.get("_private").is_none());
    }

    for (format, endpoint, reply) in [
        (
            "openai_responses",
            "/v1/responses",
            json!({"id":"resp_test","object":"response","output":[]}),
        ),
        (
            "openai_chat",
            "/v1/chat/completions",
            json!({"id":"chat_test","object":"chat.completion",
            "created":0,"model":"gpt-test","choices":[{"index":0,"finish_reason":"stop",
                "message":{"role":"assistant","content":"done"}}]}),
        ),
    ] {
        register_provider(&db, AppType::Codex, format, &upstream);
        mock.replies
            .lock()
            .await
            .push_back(("application/json", reply.to_string()));
        let mut control = request.clone();
        control["model"] = json!("gpt-test");
        control["input"].as_array_mut().unwrap().truncate(1);
        let response = send("/v1/responses", &control).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{format}");
        let requests = mock.requests.lock().await;
        let (path, body) = requests.last().unwrap();
        assert_eq!(path, endpoint);
        let parameters = if format == "openai_chat" {
            &body["tools"][0]["function"]["parameters"]
        } else {
            &body["tools"][0]["parameters"]
        };
        assert_eq!(parameters["oneOf"], schema["oneOf"]);
        assert!(parameters["properties"].get(ARGUMENT_ENVELOPE).is_none());
    }

    assert!(mock.replies.lock().await.is_empty());
    proxy.stop().await.unwrap();
    mock_task.abort();
}

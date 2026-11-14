//! WorkBuddy's Chat Completions client, backed by a managed xAI account.
//! Credentials stay in the existing OAuth manager. Requests use the Responses
//! pipeline (privacy filtering, token refresh, cancellation and usage logging).
use super::{handlers, server::ProxyState, ProxyError};
use crate::{app_config::AppType, mode::stack::StackTarget};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, Request},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use subtle::ConstantTimeEq;

fn invalid(message: &str) -> ProxyError {
    ProxyError::InvalidRequest(message.into())
}

pub(super) async fn handle(
    State(state): State<ProxyState>,
    Path(id): Path<String>,
    request: Request<Body>,
) -> Result<Response, ProxyError> {
    let provider = state
        .db
        .get_provider_by_id(&id, "workbuddy")
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?
        .filter(|p| p.is_xai_oauth())
        .ok_or_else(|| invalid("Unknown WorkBuddy OAuth provider"))?;
    let expected = provider
        .settings_config
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if expected.len() < 32 || !bool::from(expected.as_bytes().ct_eq(supplied.as_bytes())) {
        return Err(ProxyError::AuthError(
            "Invalid WorkBuddy gateway key".into(),
        ));
    }
    // Removing the provider from WorkBuddy must revoke access without deleting the account.
    let installed = crate::workbuddy_config::get_providers()
        .map_err(|e| ProxyError::ConfigError(e.to_string()))?;
    if !installed.contains_key(&id) {
        return Err(invalid("WorkBuddy provider is not enabled"));
    }
    let (mut parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|_| invalid("WorkBuddy request is too large"))?;
    let chat: Value =
        serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid Chat Completions JSON"))?;
    let model = chat
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("model is required"))?;
    let allowed = provider
        .settings_config
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|m| m.get("id").and_then(Value::as_str) == Some(model))
        });
    if !allowed || !model.starts_with("grok-") {
        return Err(invalid(
            "Model is not configured for this WorkBuddy provider",
        ));
    }
    let streaming = chat.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let include_usage = chat
        .pointer("/stream_options/include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let converted = to_responses(&chat)?;
    let target = StackTarget {
        provider,
        upstream_model: model.into(),
        original_model: model.into(),
    };
    // Never forward the local gateway credential or the original body length.
    parts.headers.remove(header::AUTHORIZATION);
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.remove(header::CONTENT_ENCODING);
    parts.uri = "/v1/responses".parse().expect("static URI");
    let request = Request::from_parts(parts, Body::from(converted.to_string()));
    let response = handlers::handle_responses_for_target(
        state,
        request,
        AppType::Codex,
        "WorkBuddy",
        "workbuddy",
        Some(target),
    )
    .await?;
    if !response.status().is_success() {
        return Ok(response);
    }
    if streaming {
        return Ok(convert_stream(response, model.to_string(), include_usage));
    }
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .map_err(|_| invalid("Could not read Grok response"))?;
    let body: Value =
        serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid Grok response"))?;
    Ok(Json(to_chat(&body, model)?).into_response())
}

fn to_responses(chat: &Value) -> Result<Value, ProxyError> {
    if chat
        .get("n")
        .and_then(Value::as_u64)
        .is_some_and(|n| n != 1)
    {
        return Err(invalid("Only n=1 is supported"));
    }
    let effort = chat
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .unwrap_or("xhigh");
    if !matches!(effort, "low" | "medium" | "high" | "xhigh") {
        return Err(invalid("Grok effort must be low, medium, high or xhigh"));
    }
    let mut input = Vec::new();
    for m in chat
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("messages is required"))?
    {
        let role = m
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("Message role is required"))?;
        if role == "tool" {
            let call_id = m
                .get("tool_call_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid("tool_call_id is required"))?;
            let output = match m.get("content") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            input.push(json!({"type":"function_call_output", "call_id":call_id, "output":output}));
            continue;
        }
        if !matches!(role, "user" | "assistant" | "system" | "developer") {
            return Err(invalid("Unsupported message role"));
        }
        let mut content = Vec::new();
        match m.get("content") {
            Some(Value::String(s)) if !s.is_empty() => content.push(
                json!({"type":if role == "assistant" {"output_text"} else {"input_text"},"text":s}),
            ),
            Some(Value::Array(parts)) => {
                for part in parts {
                    match part.get("type").and_then(Value::as_str) {
                    Some("text") => content.push(json!({"type":if role == "assistant" {"output_text"} else {"input_text"},"text":part["text"]})),
                    Some("image_url") if role == "user" => {
                        let url = part.pointer("/image_url/url").and_then(Value::as_str).ok_or_else(|| invalid("Image URL is required"))?;
                        content.push(json!({"type":"input_image","image_url":url,"detail":part.pointer("/image_url/detail").and_then(Value::as_str).unwrap_or("auto")}));
                    },
                    _ => return Err(invalid("Unsupported message content type")),
                }
                }
            }
            None | Some(Value::Null) | Some(Value::String(_)) => {}
            _ => return Err(invalid("Unsupported message content")),
        }
        if !content.is_empty() {
            input.push(json!({"role":role,"content":content}));
        }
        if let Some(calls) = m.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("Tool call ID is required"))?;
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("Function name is required"))?;
                let args = call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("Function arguments must be a string"))?;
                input.push(
                    json!({"type":"function_call","call_id":id,"name":name,"arguments":args}),
                );
            }
        }
    }
    let mut out = json!({"model":chat["model"],"input":input,"stream":chat.get("stream").and_then(Value::as_bool).unwrap_or(false),"reasoning":{"effort":effort}});
    if let Some(tools) = chat.get("tools").and_then(Value::as_array) {
        let mut converted = Vec::new();
        for tool in tools {
            if tool["type"] != "function" {
                return Err(invalid("Only function tools are supported"));
            }
            let mut function = tool
                .get("function")
                .and_then(Value::as_object)
                .cloned()
                .ok_or_else(|| invalid("Invalid function tool"))?;
            function.insert("type".into(), json!("function"));
            converted.push(Value::Object(function));
        }
        out["tools"] = json!(converted);
    }
    if let Some(choice) = chat.get("tool_choice") {
        out["tool_choice"] = if choice.is_string() {
            choice.clone()
        } else {
            let name = choice
                .pointer("/function/name")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("Invalid tool_choice"))?;
            json!({"type":"function","name":name})
        };
    }
    for key in ["parallel_tool_calls", "temperature", "top_p"] {
        if let Some(v) = chat.get(key) {
            out[key] = v.clone();
        }
    }
    if let Some(v) = chat
        .get("max_completion_tokens")
        .or_else(|| chat.get("max_tokens"))
    {
        out["max_output_tokens"] = v.clone();
    }
    if let Some(format) = chat.get("response_format") {
        out["text"] = json!({"format": if format["type"] == "json_schema" {
            let mut schema = format["json_schema"].clone(); schema["type"] = json!("json_schema"); schema
        } else { format.clone() }});
    }
    Ok(out)
}

fn usage(r: &Value) -> Value {
    let u = &r["usage"];
    let input = u["input_tokens"].as_u64().unwrap_or(0);
    let output = u["output_tokens"].as_u64().unwrap_or(0);
    json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output,
        "prompt_tokens_details":u["input_tokens_details"],"completion_tokens_details":u["output_tokens_details"]})
}
fn finish(r: &Value, tools: bool) -> &'static str {
    if r["status"] == "incomplete" {
        "length"
    } else if tools {
        "tool_calls"
    } else {
        "stop"
    }
}
fn to_chat(r: &Value, model: &str) -> Result<Value, ProxyError> {
    if r["status"] == "failed" || r.get("error").is_some_and(|e| !e.is_null()) {
        return Err(ProxyError::UpstreamError {
            status: 502,
            body: Some("Grok response failed".into()),
        });
    }
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for item in r["output"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("message") => for p in item["content"].as_array().into_iter().flatten() { text.push_str(p["text"].as_str().or_else(||p["refusal"].as_str()).unwrap_or("")); },
            Some("reasoning") => for p in item["summary"].as_array().into_iter().flatten() { reasoning.push_str(p["text"].as_str().unwrap_or("")); },
            Some("function_call") => calls.push(json!({"id":item["call_id"],"type":"function","function":{"name":item["name"],"arguments":item["arguments"]}})),
            _ => {},
        }
    }
    let mut message = json!({"role":"assistant","content":text});
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    Ok(
        json!({"id":r["id"],"object":"chat.completion","created":r["created_at"],"model":r.get("model").unwrap_or(&json!(model)),
        "choices":[{"index":0,"message":message,"finish_reason":finish(r,!calls.is_empty())}],"usage":usage(r)}),
    )
}

struct ChatStream {
    id: String,
    model: String,
    calls: HashMap<u64, usize>,
    done: bool,
    include_usage: bool,
}
impl ChatStream {
    fn chunk(&self, delta: Value, finish: Option<&str>) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":self.model,
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    }
    fn event(&mut self, e: Value) -> Result<Vec<Value>, ProxyError> {
        let mut out = Vec::new();
        match e["type"].as_str().unwrap_or("") {
            "response.created" => {
                if let Some(id) = e["response"]["id"].as_str() {
                    self.id = id.into();
                }
                if let Some(model) = e["response"]["model"].as_str() {
                    self.model = model.into();
                }
                out.push(self.chunk(json!({"role":"assistant","content":""}), None));
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                out.push(self.chunk(json!({"content":e["delta"]}), None))
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                out.push(self.chunk(json!({"reasoning_content":e["delta"]}), None))
            }
            "response.output_item.added" if e["item"]["type"] == "function_call" => {
                let index = self.calls.len();
                self.calls
                    .insert(e["output_index"].as_u64().unwrap_or(0), index);
                out.push(self.chunk(json!({"tool_calls":[{"index":index,"id":e["item"]["call_id"],"type":"function","function":{"name":e["item"]["name"],"arguments":""}}]}),None));
            }
            "response.function_call_arguments.delta" => {
                let index = self
                    .calls
                    .get(&e["output_index"].as_u64().unwrap_or(0))
                    .ok_or_else(|| invalid("Tool delta without a tool call"))?;
                out.push(self.chunk(
                    json!({"tool_calls":[{"index":index,"function":{"arguments":e["delta"]}}]}),
                    None,
                ));
            }
            "response.completed" | "response.incomplete" => {
                out.push(self.chunk(
                    json!({}),
                    Some(finish(&e["response"], !self.calls.is_empty())),
                ));
                if self.include_usage {
                    let mut chunk = self.chunk(json!({}), None);
                    chunk["choices"] = json!([]);
                    chunk["usage"] = usage(&e["response"]);
                    out.push(chunk);
                }
                self.done = true;
            }
            "response.failed" | "error" => return Err(invalid("Grok stream failed")),
            _ => {}
        }
        Ok(out)
    }
}

fn convert_stream(response: Response, model: String, include_usage: bool) -> Response {
    let mut upstream = response.into_body().into_data_stream();
    let stream = async_stream::try_stream! {
        let mut state=ChatStream{id:format!("chatcmpl-{}",uuid::Uuid::new_v4()),model,calls:HashMap::new(),done:false,include_usage};
        let mut pending=Vec::<u8>::new();
        while let Some(chunk)=upstream.next().await {
            pending.extend_from_slice(&chunk.map_err(std::io::Error::other)?);
            if pending.len()>8*1024*1024 { Err(std::io::Error::other("Grok SSE event too large"))?; }
            loop {
                let boundary=pending.windows(2).position(|s|s==b"\n\n").map(|n|(n,2))
                    .into_iter().chain(pending.windows(4).position(|s|s==b"\r\n\r\n").map(|n|(n,4))).min_by_key(|p|p.0);
                let Some((end,delimiter))=boundary else {break};
                let block=String::from_utf8(pending.drain(..end+delimiter).collect()).map_err(std::io::Error::other)?;
                let data=block.lines().filter_map(|line|super::sse::strip_sse_field(line,"data")).collect::<Vec<_>>().join("\n");
                if data.is_empty() || data=="[DONE]" {continue;}
                let event=serde_json::from_str(&data).map_err(std::io::Error::other)?;
                for chunk in state.event(event).map_err(|e|std::io::Error::other(e.to_string()))? {
                    yield Bytes::from(format!("data: {chunk}\n\n"));
                }
                if state.done {break;}
            }
            if state.done {break;}
        }
        if !state.done { Err(std::io::Error::other("Grok stream ended before completion"))?; }
        yield Bytes::from_static(b"data: [DONE]\n\n");
    };
    let stream: std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<Bytes, std::io::Error>> + Send>,
    > = Box::pin(stream);
    (
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt-in network smoke test. Uses an existing account store only when the
    /// caller explicitly provides it; all app/model configuration stays isolated.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires CC_SWITCH_LIVE_XAI_DIR and an existing Grok subscription"]
    fn live_workbuddy_grok_oauth() {
        use crate::{
            commands::XaiOAuthState,
            database::Database,
            provider::{Provider, ProviderMeta},
        };
        use std::sync::Arc;
        let auth_dir =
            std::env::var("CC_SWITCH_LIVE_XAI_DIR").expect("explicit OAuth store required");
        let home = std::env::var("CC_SWITCH_TEST_HOME").expect("isolated test home required");
        assert!(!std::path::Path::new(&auth_dir).starts_with(&home));
        super::super::http_client::init(std::env::var("HTTPS_PROXY").ok().as_deref()).unwrap();
        let manager =
            super::super::providers::xai_oauth_auth::XaiOAuthManager::new(auth_dir.into());
        let mut context = tauri::generate_context!();
        context.config_mut().app.windows.clear();
        let app = tauri::Builder::default()
            .any_thread()
            .manage(XaiOAuthState(Arc::new(tokio::sync::RwLock::new(manager))))
            .build(context)
            .expect("headless Tauri app");
        tauri::async_runtime::block_on(async {
            let db = Arc::new(Database::memory().unwrap());
            let key = uuid::Uuid::new_v4().to_string();
            let model = "grok-4.7-build-fast";
            let mut provider = Provider::with_id(
                "workbuddy-live-test".into(),
                "Grok smoke test".into(),
                json!({"apiKey":key,"models":[{"id":model}]}),
                None,
            );
            provider.meta = Some(ProviderMeta {
                provider_type: Some("xai_oauth".into()),
                ..Default::default()
            });
            db.save_provider("workbuddy", &provider).unwrap();
            let server = super::super::server::ProxyServer::new(
                super::super::ProxyConfig {
                    listen_port: 0,
                    ..Default::default()
                },
                db.clone(),
                Some(app.handle().clone()),
            );
            let info = server.start().await.unwrap();
            let origin = format!("http://127.0.0.1:{}", info.port);
            let config = crate::workbuddy_config::oauth_live_config(&provider, &origin).unwrap();
            crate::workbuddy_config::set_provider(&provider.id, &config).unwrap();
            let url = format!("{origin}/workbuddy/{}/v1/chat/completions", provider.id);
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .unwrap();
            let request = json!({"model":model,"reasoning_effort":"xhigh","messages":[{"role":"user","content":"Use the multiply tool to compute 17 times 23."}],
                "tools":[{"type":"function","function":{"name":"multiply","description":"Multiply two integers","parameters":{"type":"object","properties":{"a":{"type":"integer"},"b":{"type":"integer"}},"required":["a","b"]}}}],
                "tool_choice":{"type":"function","function":{"name":"multiply"}}});
            let denied = client
                .post(&url)
                .bearer_auth("wrong")
                .json(&request)
                .send()
                .await
                .unwrap();
            assert!(!denied.status().is_success());
            let response = client
                .post(&url)
                .bearer_auth(&key)
                .json(&request)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let text = response.text().await.unwrap();
            assert!(status.is_success(), "Gateway HTTP {status}: {text}");
            let answer: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(answer["model"], model);
            assert_eq!(answer["choices"][0]["finish_reason"], "tool_calls");
            let message = answer["choices"][0]["message"].clone();
            let call = &message["tool_calls"][0];
            assert_eq!(call["function"]["name"], "multiply");
            let args: Value =
                serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
            assert_eq!(
                args["a"].as_f64().unwrap() * args["b"].as_f64().unwrap(),
                391.0
            );
            println!("PASS: OAuth, actual model {model}, xhigh request, non-streaming tool call, gateway auth");
            let next = json!({"model":model,"reasoning_effort":"xhigh","stream":true,"stream_options":{"include_usage":true},
                "messages":[request["messages"][0].clone(),message.clone(),{"role":"tool","tool_call_id":call["id"],"content":"391"}]});
            let response = client
                .post(&url)
                .bearer_auth(&key)
                .json(&next)
                .send()
                .await
                .unwrap();
            assert!(response.status().is_success());
            let text = response.text().await.unwrap();
            assert!(text.contains("391"));
            assert!(text.contains("[DONE]"));
            assert!(text.contains("completion_tokens"));
            println!("PASS: tool-result round trip, streamed answer 391, completion and usage");
            crate::workbuddy_config::remove_provider(&provider.id).unwrap();
            let revoked = client
                .post(&url)
                .bearer_auth(&key)
                .json(&request)
                .send()
                .await
                .unwrap();
            assert!(!revoked.status().is_success());
            server.stop().await.unwrap();
        });
    }

    #[test]
    fn converts_image_and_parallel_tool_round_trip() {
        let request = json!({"model":"grok-4.7-build-fast","reasoning_effort":"xhigh",
            "messages":[
                {"role":"system","content":"Be precise"},
                {"role":"user","content":[{"type":"text","text":"Compare"},{"type":"image_url","image_url":{"url":"data:image/png;base64,YQ=="}}]},
                {"role":"assistant","content":null,"tool_calls":[
                    {"id":"call_a","type":"function","function":{"name":"lookup","arguments":"{\"id\":1}"}},
                    {"id":"call_b","type":"function","function":{"name":"lookup","arguments":"{\"id\":2}"}}]},
                {"role":"tool","tool_call_id":"call_a","content":"A"},
                {"role":"tool","tool_call_id":"call_b","content":"B"}],
            "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object","properties":{"id":{"type":"integer"}}}}}],
            "tool_choice":{"type":"function","function":{"name":"lookup"}},"parallel_tool_calls":true});
        let r = to_responses(&request).unwrap();
        assert_eq!(r["reasoning"]["effort"], "xhigh");
        assert_eq!(r["input"][1]["content"][1]["type"], "input_image");
        assert_eq!(r["input"][2]["call_id"], "call_a");
        assert_eq!(r["input"][3]["call_id"], "call_b");
        assert_eq!(
            r["input"][4],
            json!({"type":"function_call_output","call_id":"call_a","output":"A"})
        );
        assert_eq!(r["tools"][0]["type"], "function");
        assert_eq!(r["tool_choice"], json!({"type":"function","name":"lookup"}));
    }

    #[test]
    fn rejects_unsupported_effort_and_content_instead_of_silent_loss() {
        assert!(
            to_responses(&json!({"model":"grok-4.7","messages":[],"reasoning_effort":"max"}))
                .is_err()
        );
        assert!(to_responses(&json!({"model":"grok-4.7","messages":[{"role":"user","content":[{"type":"input_audio"}]}]})).is_err());
        let r = to_responses(&json!({"model":"grok-4.7","messages":[]})).unwrap();
        assert_eq!(r["reasoning"]["effort"], "xhigh");
    }

    #[test]
    fn non_streaming_tools_usage_and_truncation() {
        let r=to_chat(&json!({"id":"resp_1","model":"grok-4.7","status":"completed","created_at":1,
            "output":[{"type":"function_call","call_id":"call_1","name":"read","arguments":"{}"}],
            "usage":{"input_tokens":10,"output_tokens":20,"output_tokens_details":{"reasoning_tokens":5}}}),"fallback").unwrap();
        assert_eq!(r["model"], "grok-4.7");
        assert_eq!(r["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(r["choices"][0]["message"]["tool_calls"][0]["id"], "call_1");
        assert_eq!(r["usage"]["total_tokens"], 30);
        assert_eq!(
            r["usage"]["completion_tokens_details"]["reasoning_tokens"],
            5
        );
        assert_eq!(finish(&json!({"status":"incomplete"}), true), "length");
        assert!(to_chat(
            &json!({"status":"failed","error":{"message":"failed"}}),
            "grok"
        )
        .is_err());
    }

    #[tokio::test]
    async fn streaming_handles_utf8_boundaries_parallel_calls_and_usage() {
        let events = [
            json!({"type":"response.created","response":{"id":"resp_1","model":"grok-4.7"}}),
            json!({"type":"response.reasoning_summary_text.delta","delta":"检查"}),
            json!({"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","call_id":"a","name":"read"}}),
            json!({"type":"response.output_item.added","output_index":4,"item":{"type":"function_call","call_id":"b","name":"read"}}),
            json!({"type":"response.function_call_arguments.delta","output_index":4,"delta":"{\"n\":2}"}),
            json!({"type":"response.function_call_arguments.delta","output_index":2,"delta":"{\"n\":1}"}),
            json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":7}}}),
        ];
        let wire = events
            .iter()
            .map(|e| format!("data: {e}\r\n\r\n"))
            .collect::<String>();
        let chunks = wire
            .into_bytes()
            .into_iter()
            .map(|b| Ok::<_, std::io::Error>(Bytes::from(vec![b])))
            .collect::<Vec<_>>();
        let response = Body::from_stream(futures::stream::iter(chunks)).into_response();
        let result = convert_stream(response, "fallback".into(), true);
        let bytes = axum::body::to_bytes(result.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let output = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(output.contains("检查"));
        assert!(output.ends_with("data: [DONE]\n\n"));
        let chunks = output
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .filter(|l| *l != "[DONE]")
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            chunks[4]["choices"][0]["delta"]["tool_calls"][0]["index"],
            1
        );
        assert_eq!(
            chunks[5]["choices"][0]["delta"]["tool_calls"][0]["index"],
            0
        );
        assert_eq!(chunks[6]["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(chunks[7]["usage"]["total_tokens"], 10);
    }

    #[tokio::test]
    async fn disconnected_stream_is_an_error_not_a_successful_completion() {
        let response =
            Body::from("data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n")
                .into_response();
        let result = convert_stream(response, "grok".into(), false);
        assert!(axum::body::to_bytes(result.into_body(), 1024 * 1024)
            .await
            .is_err());
    }
}

use super::*;
use serde_json::json;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    sync::{Arc, Mutex},
};

pub fn run_stdio() -> Result<(), String> {
    let encoded =
        std::env::var(CONFIG_ENV).map_err(|_| "Missing API profile environment variable")?;
    let profile = decode_profile(&encoded)?;
    let header_text = std::env::var(HEADERS_ENV).unwrap_or_else(|_| "{}".into());
    if header_text.len() > 16 * 1024 {
        return Err("Static headers exceed 16 KiB".into());
    }
    if header_text.len() + encoded.len() > MAX_ENCODED {
        return Err("Combined API definition and headers exceed 24 KiB".into());
    }
    let headers = serde_json::from_str(&header_text).map_err(|_| "Invalid API headers JSON")?;
    let runtime = Arc::new(ApiRuntime::new(profile, headers)?);
    let executor = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| "Cannot initialize API runtime")?;
    let mut session = Session::new(runtime.clone());
    let calls: Arc<Mutex<HashMap<String, (u64, tokio::task::AbortHandle)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut generation = 0u64;
    let permits = Arc::new(tokio::sync::Semaphore::new(8));
    let mut reader = BufReader::new(std::io::stdin());
    let result = (|| -> Result<(), String> {
        loop {
            let mut line = Vec::new();
            let length = reader
                .by_ref()
                .take(1024 * 1024 + 1)
                .read_until(b'\n', &mut line)
                .map_err(|_| "Cannot read MCP stdin")?;
            if length == 0 {
                break;
            }
            if length > 1024 * 1024 {
                return Err("MCP message exceeds 1 MiB".into());
            }
            let message = match serde_json::from_slice(&line) {
                Ok(value) => value,
                Err(_) => {
                    write_message(&rpc_error(Value::Null, -32700, "Invalid JSON"))?;
                    continue;
                }
            };
            match session.dispatch(message) {
                Dispatch::None => {}
                Dispatch::Reply(reply) => write_message(&reply)?,
                Dispatch::Cancel(id) => {
                    let handle = calls
                        .lock()
                        .map_err(|_| "MCP call registry unavailable")?
                        .remove(&id.to_string());
                    if let Some((_, handle)) = handle {
                        handle.abort();
                        write_message(&rpc_error(id, -32800, "Request cancelled"))?;
                    }
                }
                Dispatch::Call {
                    id,
                    name,
                    arguments,
                } => {
                    let key = id.to_string();
                    let mut pending = calls.lock().map_err(|_| "MCP call registry unavailable")?;
                    if pending.contains_key(&key) {
                        write_message(&rpc_error(id, -32600, "Duplicate active request ID"))?;
                        continue;
                    }
                    let permit = match permits.clone().try_acquire_owned() {
                        Ok(permit) => permit,
                        Err(_) => {
                            write_message(&rpc_error(
                                id,
                                -32000,
                                "At most 8 API calls may run concurrently",
                            ))?;
                            continue;
                        }
                    };
                    let runtime = runtime.clone();
                    let registry = calls.clone();
                    let task_key = key.clone();
                    generation = generation.wrapping_add(1);
                    let token = generation;
                    let task = executor.spawn(async move {
                        let _permit = permit;
                        let reply = call_response(&runtime, id, &name, arguments).await;
                        // A cancelled task is removed first and can never emit success.
                        let completed = registry.lock().is_ok_and(|mut map| {
                            if map.get(&task_key).is_some_and(|(saved, _)| *saved == token) {
                                map.remove(&task_key);
                                true
                            } else {
                                false
                            }
                        });
                        if completed && write_message(&reply).is_err() {
                            std::process::exit(1);
                        }
                    });
                    pending.insert(key, (token, task.abort_handle()));
                }
            }
        }
        Ok(())
    })();
    if let Ok(mut pending) = calls.lock() {
        for (_, (_, handle)) in pending.drain() {
            handle.abort();
        }
    }
    executor.shutdown_timeout(std::time::Duration::from_secs(1));
    result
}

fn write_message(message: &Value) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, message).map_err(|_| "Cannot write MCP stdout")?;
    out.write_all(b"\n")
        .and_then(|_| out.flush())
        .map_err(|_| "Cannot write MCP stdout".into())
}
fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
async fn call_response(runtime: &ApiRuntime, id: Value, name: &str, arguments: Value) -> Value {
    match runtime.call(name, arguments).await {
        Ok(result) => rpc_result(id, result),
        Err(message) => rpc_result(
            id,
            json!({"content":[{"type":"text","text":message}],"isError":true}),
        ),
    }
}
enum Dispatch {
    None,
    Reply(Value),
    Call {
        id: Value,
        name: String,
        arguments: Value,
    },
    Cancel(Value),
}

pub(super) struct Session {
    runtime: Arc<ApiRuntime>,
    state: u8,
}
impl Session {
    pub fn new(runtime: Arc<ApiRuntime>) -> Self {
        Self { runtime, state: 0 }
    }
    fn dispatch(&mut self, message: Value) -> Dispatch {
        if !message.is_object() || message["jsonrpc"] != "2.0" || !message["method"].is_string() {
            return Dispatch::Reply(rpc_error(
                message.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "Invalid JSON-RPC request",
            ));
        }
        let method = message["method"].as_str().unwrap();
        let Some(id) = message.get("id").cloned() else {
            if method == "notifications/initialized" && self.state == 1 {
                self.state = 2;
            }
            if method == "notifications/cancelled" && self.state == 2 {
                if let Some(id) = message["params"].get("requestId") {
                    return Dispatch::Cancel(id.clone());
                }
            }
            return Dispatch::None;
        };
        if !(id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()) {
            return Dispatch::Reply(rpc_error(Value::Null, -32600, "Invalid request ID"));
        }
        let params = &message["params"];
        let reply = match method {
            "initialize" => {
                if self.state != 0 {
                    return Dispatch::Reply(rpc_error(id, -32600, "Already initialized"));
                }
                let Some(version) = params["protocolVersion"].as_str() else {
                    return Dispatch::Reply(rpc_error(id, -32602, "Missing protocol version"));
                };
                if !params["capabilities"].is_object()
                    || !params["clientInfo"]["name"].is_string()
                    || !params["clientInfo"]["version"].is_string()
                {
                    return Dispatch::Reply(rpc_error(
                        id,
                        -32602,
                        "Invalid initialization parameters",
                    ));
                }
                let version = if matches!(
                    version,
                    "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25"
                ) {
                    version
                } else {
                    "2025-11-25"
                };
                self.state = 1;
                rpc_result(
                    id,
                    json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"cc-switch-api","version":env!("CARGO_PKG_VERSION")}}),
                )
            }
            "ping" => rpc_result(id, json!({})),
            _ if self.state != 2 => rpc_error(id, -32002, "MCP session is not initialized"),
            "tools/list" => {
                if params.get("cursor").is_some() {
                    rpc_error(id, -32602, "This server does not use pagination cursors")
                } else {
                    let tools:Vec<_>=self.runtime.profile.tools.iter().map(|tool|json!({"name":tool.name,"description":tool.description,"inputSchema":tool.input_schema,"annotations":{"readOnlyHint":tool.is_read_only(),"destructiveHint":!tool.is_read_only(),"openWorldHint":true}})).collect();
                    rpc_result(id, json!({"tools":tools}))
                }
            }
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return Dispatch::Reply(rpc_error(id, -32602, "Tool name is required"));
                };
                if !self
                    .runtime
                    .profile
                    .tools
                    .iter()
                    .any(|tool| tool.name == name)
                {
                    return Dispatch::Reply(rpc_error(id, -32602, "Unknown API tool"));
                }
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !arguments.is_object() {
                    return Dispatch::Reply(rpc_error(
                        id,
                        -32602,
                        "Tool arguments must be an object",
                    ));
                }
                return Dispatch::Call {
                    id,
                    name: name.into(),
                    arguments,
                };
            }
            _ => rpc_error(id, -32601, "Method not found"),
        };
        Dispatch::Reply(reply)
    }
    #[cfg(test)]
    pub async fn handle(&mut self, message: Value) -> Option<Value> {
        match self.dispatch(message) {
            Dispatch::Reply(reply) => Some(reply),
            Dispatch::Call {
                id,
                name,
                arguments,
            } => Some(call_response(&self.runtime, id, &name, arguments).await),
            _ => None,
        }
    }
}

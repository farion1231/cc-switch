use std::collections::HashMap;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

pub const BRIDGE_BINDING_NAME: &str = "codexSessionDeleteV2";
const CDP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CDP_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

pub type BridgeHandler = Arc<
    dyn Fn(String, Value) -> Pin<Box<dyn Future<Output = anyhow::Result<Value>> + Send>>
        + Send
        + Sync,
>;

static NEXT_MESSAGE_ID: AtomicU64 = AtomicU64::new(100);

pub fn build_bridge_script(binding_name: &str) -> String {
    format!(
        r#"
(() => {{
  window.__codexSessionDeleteCallbacks = new Map();
  window.__codexSessionDeleteSeq = 0;
  window.__codexSessionDeleteResolve = (id, result) => {{
    const callback = window.__codexSessionDeleteCallbacks.get(id);
    if (!callback) return;
    window.__codexSessionDeleteCallbacks.delete(id);
    callback.resolve(result);
  }};
  window.__codexSessionDeleteReject = (id, message) => {{
    const callback = window.__codexSessionDeleteCallbacks.get(id);
    if (!callback) return;
    window.__codexSessionDeleteCallbacks.delete(id);
    callback.resolve({{ status: "failed", message }});
  }};
  window.__codexSessionDeleteBridge = (path, payload) => new Promise((resolve) => {{
    const id = String(++window.__codexSessionDeleteSeq);
    window.__codexSessionDeleteCallbacks.set(id, {{ resolve }});
    window.{binding_name}(JSON.stringify({{ id, path, payload }}));
  }});
}})();
"#
    )
}

pub fn bridge_health_check_script() -> &'static str {
    r#"
(() => {
  const bridge = window.__codexSessionDeleteBridge;
  if (typeof bridge !== "function") return false;
  try {
    return Promise.race([
      Promise.resolve(bridge("/backend/status", {})).then((result) => !!result && result.status === "ok"),
      new Promise((resolve) => setTimeout(() => resolve(false), 2000)),
    ]);
  } catch (error) {
    return false;
  }
})()
"#
}

pub async fn evaluate_script(websocket_url: &str, script: &str) -> anyhow::Result<Value> {
    evaluate_script_with_await_promise(websocket_url, script, false).await
}

pub async fn evaluate_script_with_await_promise(
    websocket_url: &str,
    script: &str,
    await_promise: bool,
) -> anyhow::Result<Value> {
    let socket = connect_cdp_websocket(websocket_url).await?;
    let mut session = CdpSession::new(socket);
    let response = session
        .send_command(
            1,
            "Runtime.evaluate",
            runtime_evaluate_params_with_await_promise(script, await_promise),
        )
        .await?;
    ensure_runtime_evaluate_succeeded(response)
}

pub async fn run_periodic_evaluations<F>(
    websocket_url: &str,
    period: Duration,
    mut next_expression: F,
) -> anyhow::Result<()>
where
    F: FnMut() -> anyhow::Result<Option<String>>,
{
    let socket = connect_cdp_websocket(websocket_url).await?;
    let mut session = CdpSession::new(socket);
    let mut interval = tokio::time::interval(period);
    loop {
        interval.tick().await;
        let Some(expression) = next_expression()? else {
            return Ok(());
        };
        let response = session
            .send_command(
                next_message_id(),
                "Runtime.evaluate",
                runtime_evaluate_params(&expression),
            )
            .await?;
        let response = ensure_runtime_evaluate_succeeded(response)?;
        if runtime_evaluate_result_is_false(&response) {
            bail!("periodic Runtime.evaluate reported unavailable capability");
        }
    }
}

pub async fn add_script_to_new_documents(
    websocket_url: &str,
    script: &str,
) -> anyhow::Result<Value> {
    let socket = connect_cdp_websocket(websocket_url).await?;
    let mut session = CdpSession::new(socket);
    session
        .send_command(
            1,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": script }),
        )
        .await
}

pub async fn install_bridge(
    websocket_url: &str,
    binding_name: &str,
    handler: BridgeHandler,
    new_document_scripts: &[String],
) -> anyhow::Result<()> {
    let socket = connect_cdp_websocket(websocket_url).await?;
    let mut session = CdpSession::new(socket).with_handler(handler);

    session.send_command(1, "Runtime.enable", json!({})).await?;
    session
        .send_command(2, "Runtime.removeBinding", json!({ "name": binding_name }))
        .await?;
    session
        .send_command(3, "Runtime.addBinding", json!({ "name": binding_name }))
        .await?;

    let bridge_script = build_bridge_script(binding_name);
    session
        .send_command(
            4,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": bridge_script }),
        )
        .await?;
    session
        .send_command(
            5,
            "Runtime.evaluate",
            runtime_evaluate_params(&bridge_script),
        )
        .await?;

    for script in new_document_scripts {
        let message_id = next_message_id();
        session
            .send_command(
                message_id,
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": script }),
            )
            .await?;
        let message_id = next_message_id();
        session
            .send_command(
                message_id,
                "Runtime.evaluate",
                runtime_evaluate_params(script),
            )
            .await?;
    }

    session.drain_binding_queue().await?;
    tokio::spawn(async move {
        loop {
            if session.drain_binding_queue().await.is_err() {
                break;
            }
            match session.next_message().await {
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
    });

    Ok(())
}

pub fn runtime_evaluate_params(script: &str) -> Value {
    runtime_evaluate_params_with_await_promise(script, false)
}

pub fn runtime_evaluate_params_with_await_promise(script: &str, await_promise: bool) -> Value {
    json!({
        "expression": script,
        "awaitPromise": await_promise,
        "allowUnsafeEvalBlockedByCSP": true,
    })
}

pub fn resolve_bridge_expression(request_id: &str, result: &Value) -> anyhow::Result<String> {
    Ok(format!(
        "window.__codexSessionDeleteResolve({}, {})",
        serde_json::to_string(request_id)?,
        serde_json::to_string(result)?,
    ))
}

pub fn reject_bridge_expression(request_id: &str, message: &str) -> anyhow::Result<String> {
    Ok(format!(
        "window.__codexSessionDeleteReject({}, {})",
        serde_json::to_string(request_id)?,
        serde_json::to_string(message)?,
    ))
}

async fn connect_cdp_websocket(
    websocket_url: &str,
) -> anyhow::Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let (socket, _) = tokio::time::timeout(CDP_CONNECT_TIMEOUT, connect_async(websocket_url))
        .await
        .with_context(|| {
            format!(
                "timed out connecting CDP websocket after {}s",
                CDP_CONNECT_TIMEOUT.as_secs()
            )
        })?
        .context("failed to connect CDP websocket")?;

    Ok(socket)
}

struct CdpSession<S> {
    socket: S,
    responses: HashMap<u64, Value>,
    binding_calls: VecDeque<Value>,
    handler: Option<BridgeHandler>,
}

impl<S> CdpSession<S>
where
    S: SinkExt<Message>
        + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin
        + Send,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    fn new(socket: S) -> Self {
        Self {
            socket,
            responses: HashMap::new(),
            binding_calls: VecDeque::new(),
            handler: None,
        }
    }

    fn with_handler(mut self, handler: BridgeHandler) -> Self {
        self.handler = Some(handler);
        self
    }

    async fn send_command(
        &mut self,
        message_id: u64,
        method: &str,
        params: Value,
    ) -> anyhow::Result<Value> {
        self.socket
            .send(Message::Text(
                json!({
                    "id": message_id,
                    "method": method,
                    "params": params,
                })
                .to_string()
                .into(),
            ))
            .await
            .with_context(|| format!("failed to send CDP command {method} id {message_id}"))?;

        tokio::time::timeout(
            CDP_COMMAND_TIMEOUT,
            self.wait_for_id(message_id, method.to_string()),
        )
        .await
        .with_context(|| {
            format!(
                "timed out waiting for CDP command {method} id {message_id} response after {}s",
                CDP_COMMAND_TIMEOUT.as_secs()
            )
        })?
    }

    async fn send_command_without_wait(
        &mut self,
        message_id: u64,
        method: &str,
        params: Value,
    ) -> anyhow::Result<()> {
        self.socket
            .send(Message::Text(
                json!({
                    "id": message_id,
                    "method": method,
                    "params": params,
                })
                .to_string()
                .into(),
            ))
            .await
            .with_context(|| format!("failed to send CDP command {method} id {message_id}"))?;
        Ok(())
    }

    async fn wait_for_id(&mut self, message_id: u64, method: String) -> anyhow::Result<Value> {
        loop {
            if let Some(response) = self.responses.remove(&message_id) {
                return command_result(response, &method, message_id);
            }

            let Some(message) = self.next_message().await? else {
                bail!("CDP websocket closed before response for {method} id {message_id}");
            };

            if let Some(response_id) = message.get("id").and_then(Value::as_u64) {
                if response_id == message_id {
                    return command_result(message, &method, message_id);
                }
                self.responses.insert(response_id, message);
            }
        }
    }

    async fn next_message(&mut self) -> anyhow::Result<Option<Value>> {
        let Some(message) = self.socket.next().await else {
            return Ok(None);
        };
        let message = message.context("failed to read CDP websocket message")?;
        let Message::Text(text) = message else {
            return Ok(Some(json!({})));
        };
        let value: Value = serde_json::from_str(&text).context("failed to parse CDP message")?;

        if value.get("method").and_then(Value::as_str) == Some("Runtime.bindingCalled") {
            self.binding_calls.push_back(value.clone());
        }

        Ok(Some(value))
    }

    async fn drain_binding_queue(&mut self) -> anyhow::Result<()> {
        while let Some(message) = self.binding_calls.pop_front() {
            self.route_binding_call(message).await?;
        }
        Ok(())
    }

    fn route_binding_call(
        &mut self,
        message: Value,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async move {
            let Some(handler) = self.handler.clone() else {
                return Ok(());
            };

            let Some(payload_text) = message
                .get("params")
                .and_then(|params| params.get("payload"))
                .and_then(Value::as_str)
            else {
                return Ok(());
            };

            let parsed: Value = match serde_json::from_str(payload_text) {
                Ok(parsed) => parsed,
                Err(error) => {
                    if let Some(request_id) = extract_string_field(payload_text, "id") {
                        self.reject_bridge_request(
                            &request_id,
                            &format!("failed to parse bridge payload: {error}"),
                        )
                        .await?;
                    }
                    return Ok(());
                }
            };
            self.route_parsed_binding_call(&handler, parsed).await
        })
    }

    async fn route_parsed_binding_call(
        &mut self,
        handler: &BridgeHandler,
        parsed: Value,
    ) -> anyhow::Result<()> {
        let Some(request_id) = parsed.get("id").and_then(Value::as_str) else {
            return Ok(());
        };
        let path = parsed
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let payload = parsed.get("payload").cloned().unwrap_or_else(|| json!({}));

        match handler(path, payload).await {
            Ok(result) => {
                self.resolve_bridge_request(request_id, &result).await?;
            }
            Err(error) => {
                self.reject_bridge_request(request_id, &error.to_string())
                    .await?;
            }
        }

        Ok(())
    }

    async fn resolve_bridge_request(
        &mut self,
        request_id: &str,
        result: &Value,
    ) -> anyhow::Result<()> {
        let expression = resolve_bridge_expression(request_id, result)?;
        let message_id = next_message_id();
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "bridge.resolve_start",
            json!({
                "request_id": request_id,
                "message_id": message_id,
                "result_status": result.get("status").and_then(Value::as_str).unwrap_or("")
            }),
        );
        let sent = self
            .send_command_without_wait(
                message_id,
                "Runtime.evaluate",
                runtime_evaluate_params(&expression),
            )
            .await;
        match &sent {
            Ok(_) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "bridge.resolve_ok",
                    json!({
                        "request_id": request_id,
                        "message_id": message_id
                    }),
                );
            }
            Err(error) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "bridge.resolve_failed",
                    json!({
                        "request_id": request_id,
                        "message_id": message_id,
                        "message": error.to_string()
                    }),
                );
            }
        }
        sent.map(|_| ())
    }

    async fn reject_bridge_request(
        &mut self,
        request_id: &str,
        message: &str,
    ) -> anyhow::Result<()> {
        let expression = reject_bridge_expression(request_id, message)?;
        let message_id = next_message_id();
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "bridge.reject_start",
            json!({
                "request_id": request_id,
                "message_id": message_id,
                "message": message
            }),
        );
        let sent = self
            .send_command_without_wait(
                message_id,
                "Runtime.evaluate",
                runtime_evaluate_params(&expression),
            )
            .await;
        match &sent {
            Ok(_) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "bridge.reject_ok",
                    json!({
                        "request_id": request_id,
                        "message_id": message_id
                    }),
                );
            }
            Err(error) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "bridge.reject_failed",
                    json!({
                        "request_id": request_id,
                        "message_id": message_id,
                        "error": error.to_string()
                    }),
                );
            }
        }
        sent.map(|_| ())
    }
}

fn command_result(response: Value, method: &str, message_id: u64) -> anyhow::Result<Value> {
    if let Some(error) = response.get("error") {
        bail!("CDP command {method} id {message_id} failed: {error}");
    }
    Ok(response)
}

fn ensure_runtime_evaluate_succeeded(response: Value) -> anyhow::Result<Value> {
    if let Some(exception) = response
        .get("result")
        .and_then(|result| result.get("exceptionDetails"))
    {
        bail!("Runtime.evaluate raised an exception: {exception}");
    }
    Ok(response)
}

fn runtime_evaluate_result_is_false(response: &Value) -> bool {
    response
        .get("result")
        .and_then(|result| result.get("result"))
        .and_then(|result| result.get("value"))
        .is_some_and(|value| value == false)
}

fn extract_string_field(input: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\"");
    let mut index = input.find(&needle)? + needle.len();
    let bytes = input.as_bytes();

    while matches!(bytes.get(index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        index += 1;
    }
    if bytes.get(index) != Some(&b':') {
        return None;
    }
    index += 1;
    while matches!(bytes.get(index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        index += 1;
    }
    if bytes.get(index) != Some(&b'"') {
        return None;
    }
    index += 1;

    let mut output = String::new();
    let mut escaped = false;
    for ch in input[index..].chars() {
        if escaped {
            output.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return Some(output),
            _ => output.push(ch),
        }
    }

    None
}

fn next_message_id() -> u64 {
    NEXT_MESSAGE_ID.fetch_add(1, Ordering::Relaxed) + 1
}

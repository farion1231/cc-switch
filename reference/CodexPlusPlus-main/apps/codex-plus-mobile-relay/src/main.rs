use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::str;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Host,
    Client,
}

impl Role {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "host" => Some(Self::Host),
            "client" => Some(Self::Client),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Client => "client",
        }
    }
}

#[derive(Debug, Clone)]
struct Registration {
    role: Role,
    room: String,
    token: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct RegisterMessage {
    #[serde(rename = "type")]
    message_type: String,
    role: String,
    room: String,
    token: String,
}

#[derive(Default)]
struct RelayState {
    rooms: HashMap<String, RoomState>,
    started_at: Option<Instant>,
    total_connections: u64,
    active_connections: u64,
    forwarded_messages: u64,
    forwarded_bytes: u64,
}

struct RoomState {
    token: String,
    host: Option<mpsc::UnboundedSender<Message>>,
    client: Option<mpsc::UnboundedSender<Message>>,
    connected_at: Instant,
    forwarded_messages: u64,
    forwarded_bytes: u64,
}

impl RoomState {
    fn new(token: String) -> Self {
        Self {
            token,
            host: None,
            client: None,
            connected_at: Instant::now(),
            forwarded_messages: 0,
            forwarded_bytes: 0,
        }
    }

    fn sender_for(&self, role: Role) -> Option<mpsc::UnboundedSender<Message>> {
        match role {
            Role::Host => self.host.clone(),
            Role::Client => self.client.clone(),
        }
    }

    fn set_sender(&mut self, role: Role, sender: mpsc::UnboundedSender<Message>) {
        let slot = match role {
            Role::Host => &mut self.host,
            Role::Client => &mut self.client,
        };
        if let Some(previous) = slot.replace(sender) {
            let _ = previous.send(Message::Close(None));
        }
    }

    fn clear_sender(&mut self, role: Role) {
        match role {
            Role::Host => self.host = None,
            Role::Client => self.client = None,
        }
    }

    fn is_empty(&self) -> bool {
        self.host.is_none() && self.client.is_none()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayStatus {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    uptime_seconds: u64,
    rooms: usize,
    active_connections: u64,
    total_connections: u64,
    forwarded_messages: u64,
    forwarded_bytes: u64,
    room_details: Vec<RoomStatus>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RoomStatus {
    room: String,
    host_online: bool,
    client_online: bool,
    connections: u8,
    age_seconds: u64,
    forwarded_messages: u64,
    forwarded_bytes: u64,
}

#[derive(Clone)]
struct RegisteredPeer {
    room: String,
    role: Role,
    sender: mpsc::UnboundedSender<Message>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bind = env::var("CODEX_PLUS_MOBILE_RELAY_BIND")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "0.0.0.0:57323".to_string());
    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("failed to bind mobile relay server on {bind}"))?;
    let local_addr = listener.local_addr()?;
    println!("Codex++ mobile relay listening on ws://{local_addr}");
    println!(
        "Clients must send first message: {{\"type\":\"register\",\"role\":\"host|client\",\"room\":\"...\",\"token\":\"...\"}}"
    );

    let state = Arc::new(Mutex::new(RelayState {
        started_at: Some(Instant::now()),
        ..RelayState::default()
    }));
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, addr) = accepted?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(error) = handle_tcp_connection(stream, addr, state).await {
                        eprintln!("relay connection {addr} closed: {error:#}");
                    }
                });
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("failed to wait for Ctrl+C")?;
                break;
            }
        }
    }
    Ok(())
}

async fn handle_tcp_connection(
    stream: TcpStream,
    addr: SocketAddr,
    state: Arc<Mutex<RelayState>>,
) -> anyhow::Result<()> {
    if !looks_like_websocket(&stream).await? {
        return handle_http_connection(stream, state).await;
    }
    handle_websocket_connection(stream, addr, state).await
}

async fn handle_websocket_connection(
    stream: TcpStream,
    addr: SocketAddr,
    state: Arc<Mutex<RelayState>>,
) -> anyhow::Result<()> {
    let url_registration = Arc::new(StdMutex::new(None::<Registration>));
    let callback_registration = Arc::clone(&url_registration);
    let websocket = accept_hdr_async(
        stream,
        move |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
            if let Some(registration) =
                registration_from_uri(request.uri().path(), request.uri().query())
            {
                if let Ok(mut slot) = callback_registration.lock() {
                    *slot = Some(registration);
                }
            }
            Ok(response)
        },
    )
    .await
    .context("failed to accept websocket")?;
    let (mut outgoing, mut incoming) = websocket.split();

    let registration = match url_registration.lock().ok().and_then(|slot| slot.clone()) {
        Some(registration) => registration,
        None => {
            let first = tokio::time::timeout(Duration::from_secs(10), incoming.next())
                .await
                .context("registration timed out")?
                .transpose()
                .context("failed to read registration")?
                .context("connection closed before registration")?;
            parse_registration(first)?
        }
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let peer = register_peer(&state, registration, tx).await?;
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if outgoing.send(message).await.is_err() {
                break;
            }
        }
    });

    println!(
        "relay registered {} room={} addr={}",
        peer.role.as_str(),
        peer.room,
        addr
    );

    while let Some(message) = incoming.next().await {
        let message = message.context("failed to read websocket message")?;
        if message.is_close() {
            break;
        }
        forward_message(&state, &peer, message).await;
    }

    unregister_peer(&state, &peer).await;
    writer.abort();
    println!(
        "relay disconnected {} room={} addr={}",
        peer.role.as_str(),
        peer.room,
        addr
    );
    Ok(())
}

async fn looks_like_websocket(stream: &TcpStream) -> anyhow::Result<bool> {
    let mut buffer = [0_u8; 2048];
    let read = stream.peek(&mut buffer).await?;
    let head = String::from_utf8_lossy(&buffer[..read]).to_ascii_lowercase();
    Ok(head.contains("\r\nupgrade: websocket") || head.contains("\r\nsec-websocket-key:"))
}

async fn handle_http_connection(
    mut stream: TcpStream,
    state: Arc<Mutex<RelayState>>,
) -> anyhow::Result<()> {
    let mut buffer = vec![0_u8; 8192];
    let read = stream.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let request_line = request.lines().next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");
    let (status, content_type, body) = match path {
        "/" | "/index.html" => (
            "200 OK",
            "text/html; charset=utf-8",
            relay_test_page().into_bytes(),
        ),
        "/mobile" => (
            "200 OK",
            "text/html; charset=utf-8",
            mobile_relay_page().into_bytes(),
        ),
        "/health" => (
            "200 OK",
            "application/json; charset=utf-8",
            serde_json::json!({
                "status": "ok",
                "service": "codex-plus-mobile-relay",
                "version": env!("CARGO_PKG_VERSION")
            })
            .to_string()
            .into_bytes(),
        ),
        "/status" => (
            "200 OK",
            "application/json; charset=utf-8",
            serde_json::to_string(&relay_status(&state).await)?.into_bytes(),
        ),
        _ => (
            "404 Not Found",
            "application/json; charset=utf-8",
            serde_json::json!({
                "status": "failed",
                "message": "not found"
            })
            .to_string()
            .into_bytes(),
        ),
    };
    let response = format!(
        concat!(
            "HTTP/1.1 {}\r\n",
            "Content-Type: {}\r\n",
            "Cache-Control: no-store, no-cache, must-revalidate, max-age=0\r\n",
            "Pragma: no-cache\r\n",
            "Expires: 0\r\n",
            "Content-Length: {}\r\n",
            "Connection: close\r\n\r\n"
        ),
        status,
        content_type,
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await?;
    Ok(())
}

fn parse_registration(message: Message) -> anyhow::Result<Registration> {
    let text = match message {
        Message::Text(text) => text.to_string(),
        Message::Binary(bytes) => {
            String::from_utf8(bytes.to_vec()).context("binary registration must be utf-8 json")?
        }
        _ => bail!("first message must be registration json"),
    };
    let registration: RegisterMessage =
        serde_json::from_str(&text).context("registration is not valid json")?;
    if registration.message_type != "register" {
        bail!("registration type must be register");
    }
    if registration.room.trim().is_empty() {
        bail!("room is required");
    }
    if registration.token.trim().is_empty() {
        bail!("token is required");
    }
    let role = Role::from_str(&registration.role).context("role must be host or client")?;
    Ok(Registration {
        role,
        room: registration.room,
        token: registration.token,
    })
}

fn registration_from_uri(path: &str, query: Option<&str>) -> Option<Registration> {
    let query = query?;
    let role = match path {
        "/host" => Some(Role::Host),
        "/client" => Some(Role::Client),
        "/ws" => query_value(query, "role").and_then(|role| Role::from_str(&role)),
        _ => None,
    }?;
    let room = query_value(query, "room")?;
    let token = query_value(query, "token")?;
    if room.trim().is_empty() || token.trim().is_empty() {
        return None;
    }
    Some(Registration { role, room, token })
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    output.push(byte);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

async fn register_peer(
    state: &Arc<Mutex<RelayState>>,
    registration: Registration,
    sender: mpsc::UnboundedSender<Message>,
) -> anyhow::Result<RegisteredPeer> {
    let mut state = state.lock().await;
    state.total_connections = state.total_connections.saturating_add(1);
    state.active_connections = state.active_connections.saturating_add(1);
    let room = state
        .rooms
        .entry(registration.room.clone())
        .or_insert_with(|| RoomState::new(registration.token.clone()));
    if room.token != registration.token {
        bail!("room token mismatch");
    }
    room.set_sender(registration.role, sender.clone());
    let _ = sender.send(Message::Text(
        serde_json::json!({
            "type": "registered",
            "role": registration.role.as_str(),
            "room": registration.room
        })
        .to_string()
        .into(),
    ));
    Ok(RegisteredPeer {
        room: registration.room,
        role: registration.role,
        sender,
    })
}

async fn forward_message(state: &Arc<Mutex<RelayState>>, peer: &RegisteredPeer, message: Message) {
    let message_bytes = message_len(&message);
    let target = {
        let mut state = state.lock().await;
        state.forwarded_messages = state.forwarded_messages.saturating_add(1);
        state.forwarded_bytes = state.forwarded_bytes.saturating_add(message_bytes);
        let Some(room) = state.rooms.get_mut(&peer.room) else {
            return;
        };
        room.forwarded_messages = room.forwarded_messages.saturating_add(1);
        room.forwarded_bytes = room.forwarded_bytes.saturating_add(message_bytes);
        let target_role = match peer.role {
            Role::Host => Role::Client,
            Role::Client => Role::Host,
        };
        room.sender_for(target_role)
    };
    if let Some(target) = target {
        let _ = target.send(message);
    }
}

async fn unregister_peer(state: &Arc<Mutex<RelayState>>, peer: &RegisteredPeer) {
    let mut state = state.lock().await;
    state.active_connections = state.active_connections.saturating_sub(1);
    let Some(room) = state.rooms.get_mut(&peer.room) else {
        return;
    };
    let still_same_sender = room
        .sender_for(peer.role)
        .as_ref()
        .map(|sender| sender.same_channel(&peer.sender))
        .unwrap_or(false);
    if still_same_sender {
        room.clear_sender(peer.role);
    }
    if room.is_empty() {
        state.rooms.remove(&peer.room);
    }
}

fn message_len(message: &Message) -> u64 {
    match message {
        Message::Text(text) => text.len() as u64,
        Message::Binary(bytes) => bytes.len() as u64,
        Message::Ping(bytes) | Message::Pong(bytes) => bytes.len() as u64,
        Message::Close(_) | Message::Frame(_) => 0,
    }
}

async fn relay_status(state: &Arc<Mutex<RelayState>>) -> RelayStatus {
    let state = state.lock().await;
    let now = Instant::now();
    let mut room_details = state
        .rooms
        .iter()
        .map(|(room, detail)| {
            let host_online = detail.host.is_some();
            let client_online = detail.client.is_some();
            RoomStatus {
                room: room.clone(),
                host_online,
                client_online,
                connections: u8::from(host_online) + u8::from(client_online),
                age_seconds: now.saturating_duration_since(detail.connected_at).as_secs(),
                forwarded_messages: detail.forwarded_messages,
                forwarded_bytes: detail.forwarded_bytes,
            }
        })
        .collect::<Vec<_>>();
    room_details.sort_by(|left, right| left.room.cmp(&right.room));
    RelayStatus {
        status: "ok",
        service: "codex-plus-mobile-relay",
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state
            .started_at
            .map(|started| now.saturating_duration_since(started).as_secs())
            .unwrap_or_default(),
        rooms: state.rooms.len(),
        active_connections: state.active_connections,
        total_connections: state.total_connections,
        forwarded_messages: state.forwarded_messages,
        forwarded_bytes: state.forwarded_bytes,
        room_details,
    }
}

fn relay_test_page() -> String {
    r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Codex++ Mobile Relay</title>
  <style>
    * { box-sizing: border-box; }
    body { margin: 0; font: 14px/1.45 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #f6f7f9; color: #15171a; }
    main { max-width: 860px; margin: 0 auto; padding: 24px; }
    h1 { margin: 0 0 16px; font-size: 22px; }
    section { background: #fff; border: 1px solid #d9dee7; border-radius: 8px; padding: 16px; margin: 12px 0; }
    label { display: grid; gap: 6px; margin: 10px 0; font-weight: 600; }
    input, textarea, select, button { font: inherit; }
    input, textarea, select { width: 100%; border: 1px solid #c8ced8; border-radius: 6px; padding: 9px 10px; background: #fff; }
    textarea { min-height: 96px; resize: vertical; }
    button { border: 1px solid #1d4ed8; background: #2563eb; color: #fff; border-radius: 6px; padding: 9px 12px; cursor: pointer; }
    button.secondary { border-color: #c8ced8; background: #fff; color: #15171a; }
    .row { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 12px; }
    .actions { display: flex; gap: 10px; flex-wrap: wrap; margin-top: 12px; }
    pre { min-height: 180px; overflow: auto; background: #101418; color: #d8e2f0; border-radius: 8px; padding: 12px; white-space: pre-wrap; }
    @media (max-width: 720px) { main { padding: 14px; } .row { grid-template-columns: 1fr; } }
  </style>
</head>
<body>
<main>
  <h1>Codex++ Mobile Relay</h1>
  <section>
    <div class="row">
      <label>角色
        <select id="role">
          <option value="client">client</option>
          <option value="host">host</option>
        </select>
      </label>
      <label>房间
        <input id="room" value="test">
      </label>
      <label>令牌
        <input id="token" value="123456">
      </label>
    </div>
    <div class="actions">
      <button id="connect">连接</button>
      <button id="disconnect" class="secondary">断开</button>
    </div>
  </section>
  <section>
    <label>发送内容
      <textarea id="message">hello</textarea>
    </label>
    <div class="actions">
      <button id="send">发送</button>
      <button id="status" class="secondary">请求 /backend/status</button>
      <button id="clear" class="secondary">清空日志</button>
    </div>
  </section>
  <section>
    <pre id="log"></pre>
  </section>
</main>
<script>
let socket = null;
const $ = (id) => document.getElementById(id);
function log(line) {
  const now = new Date().toLocaleTimeString();
  $("log").textContent += `[${now}] ${line}\n`;
  $("log").scrollTop = $("log").scrollHeight;
}
function wsBase() {
  return `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}`;
}
$("connect").onclick = () => {
  if (socket && socket.readyState === WebSocket.OPEN) return;
  const role = encodeURIComponent($("role").value);
  const room = encodeURIComponent($("room").value);
  const token = encodeURIComponent($("token").value);
  const path = role === "host" ? "host" : "client";
  socket = new WebSocket(`${wsBase()}/${path}?room=${room}&token=${token}`);
  socket.onopen = () => log("已连接");
  socket.onclose = () => log("已断开");
  socket.onerror = () => log("连接错误");
  socket.onmessage = (event) => log(`收到: ${event.data}`);
};
$("disconnect").onclick = () => {
  if (socket) socket.close();
};
$("send").onclick = () => {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    log("未连接");
    return;
  }
  socket.send($("message").value);
  log(`已发送: ${$("message").value}`);
};
$("status").onclick = () => {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    log("未连接");
    return;
  }
  const request = {
    type: "httpRequest",
    id: String(Date.now()),
    method: "GET",
    path: "/backend/status",
    headers: {},
    body: ""
  };
  socket.send(JSON.stringify(request));
  log(`已请求: ${request.path}`);
};
$("clear").onclick = () => {
  $("log").textContent = "";
};
</script>
</body>
</html>"#
        .to_string()
}

fn mobile_relay_page() -> String {
    r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
  <title>Codex++ 手机控制</title>
  <style>
    * { box-sizing: border-box; }
    :root { --bg: #f6f7f8; --panel: #fff; --line: #d8dde3; --text: #101418; --muted: #69727d; --accent: #0f766e; --danger: #b42318; --bubble-user: #e7f4f1; --bubble-agent: #fff; }
    @media (prefers-color-scheme: dark) { :root { --bg: #111315; --panel: #191c20; --line: #2e343b; --text: #f2f4f7; --muted: #a4adb8; --accent: #2dd4bf; --danger: #ff8a80; --bubble-user: #123c38; --bubble-agent: #20242a; } }
    html, body { margin: 0; height: 100%; background: var(--bg); color: var(--text); font: 14px/1.45 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
    body { overflow: hidden; }
    button, input, textarea { font: inherit; }
    button { border: 1px solid var(--line); border-radius: 8px; background: var(--panel); color: var(--text); padding: 8px 10px; }
    button.primary { border-color: var(--accent); background: var(--accent); color: #fff; }
    input, textarea { border: 1px solid var(--line); border-radius: 8px; background: var(--bg); color: var(--text); outline: none; padding: 10px; }
    .app { height: 100dvh; display: grid; grid-template-rows: auto 1fr; }
    .topbar { display: flex; align-items: center; gap: 10px; padding: calc(env(safe-area-inset-top) + 10px) 12px 10px; border-bottom: 1px solid var(--line); background: var(--panel); }
    .title { font-weight: 700; white-space: nowrap; }
    .status { flex: 1; min-width: 0; color: var(--muted); font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .layout { min-height: 0; display: grid; grid-template-columns: 340px 1fr; }
    .sessions { min-height: 0; border-right: 1px solid var(--line); background: var(--panel); display: grid; grid-template-rows: auto 1fr; }
    .connect { display: grid; gap: 8px; padding: 10px; border-bottom: 1px solid var(--line); }
    .connect-row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto; gap: 8px; }
    .search { padding: 10px; border-bottom: 1px solid var(--line); }
    .search input { width: 100%; }
    .list { overflow: auto; }
    .group { border-bottom: 1px solid var(--line); }
    .group-title { width: 100%; position: sticky; top: 0; z-index: 1; padding: 8px 10px; background: var(--panel); color: var(--muted); font-size: 12px; font-weight: 700; border: 0; border-bottom: 1px solid var(--line); display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; gap: 8px; align-items: center; cursor: pointer; }
    .group-name, .meta { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .group-new { width: 28px; height: 28px; padding: 0; border-radius: 7px; }
    .group.collapsed .item { display: none; }
    .item { width: 100%; display: block; text-align: left; border: 0; border-bottom: 1px solid var(--line); border-radius: 0; padding: 12px; background: transparent; }
    .item.active { background: color-mix(in srgb, var(--accent) 12%, transparent); }
    .preview { font-size: 14px; line-height: 1.38; max-height: 39px; overflow: hidden; }
    .meta { margin-top: 6px; color: var(--muted); font-size: 12px; }
    .detail { min-height: 0; display: grid; grid-template-rows: auto 1fr auto; }
    .thread-head { padding: 12px; border-bottom: 1px solid var(--line); background: var(--panel); display: grid; gap: 6px; }
    .thread-title { font-weight: 700; }
    .thread-meta { color: var(--muted); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .messages { overflow: auto; padding: 12px; }
    .composer { display: grid; grid-template-columns: 1fr auto; gap: 8px; padding: 10px; border-top: 1px solid var(--line); background: var(--panel); }
    .composer textarea { width: 100%; min-height: 42px; max-height: 130px; resize: vertical; }
    .composer button { align-self: end; min-height: 42px; }
    .empty { color: var(--muted); padding: 18px; text-align: center; }
    .turn { margin: 0 0 12px; }
    .bubble { border: 1px solid var(--line); background: var(--bubble-agent); border-radius: 8px; padding: 10px 12px; white-space: pre-wrap; overflow-wrap: anywhere; }
    .bubble.user { background: var(--bubble-user); }
    .role { margin: 0 0 4px; color: var(--muted); font-size: 12px; }
    .error { color: var(--danger); }
    .mobile-back { display: none; }
    @media (max-width: 760px) {
      .layout { grid-template-columns: 1fr; }
      .sessions.hidden, .detail.hidden { display: none; }
      .sessions { border-right: 0; }
      .connect-row { grid-template-columns: 1fr; }
      .mobile-back { display: inline-block; width: fit-content; }
    }
  </style>
</head>
<body>
<div class="app">
  <header class="topbar"><strong class="title">Codex++</strong><span id="status" class="status">待连接</span></header>
  <main class="layout">
    <section id="sessionsPane" class="sessions">
      <div class="connect">
        <div class="connect-row">
          <input id="room" placeholder="房间 ID" autocomplete="off">
          <input id="key" placeholder="Key" type="password" autocomplete="off">
          <button id="connect" class="primary">连接</button>
        </div>
      </div>
      <div class="search"><input id="filter" placeholder="搜索会话" autocomplete="off"></div>
      <div id="sessions" class="list"><div class="empty">连接后读取会话</div></div>
    </section>
    <section id="detailPane" class="detail hidden">
      <div class="thread-head">
        <button id="back" class="mobile-back">返回</button>
        <div id="threadTitle" class="thread-title">选择一个会话</div>
        <div id="threadMeta" class="thread-meta"></div>
      </div>
      <div id="messages" class="messages"><div class="empty">从左侧选择会话，或在项目目录里点 + 新建</div></div>
      <form id="composer" class="composer">
        <textarea id="messageInput" placeholder="输入消息" rows="1"></textarea>
        <button id="send" class="primary" type="submit">发送</button>
      </form>
    </section>
  </main>
</div>
<script>
let socket = null;
let pending = new Map();
let pendingRpc = new Map();
let rpcId = 1;
let requestId = 1;
let appServerSessionId = "";
let appServerConnected = false;
const state = { sessions: [], selectedId: null, selectedCwd: "", filter: "", expandedProjects: new Set(), pendingMessages: new Map(), pollTimers: new Map(), streaming: null, thinking: null };
const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
const statusEl = $("status");
const sessionsEl = $("sessions");
const messagesEl = $("messages");
const titleEl = $("threadTitle");
const metaEl = $("threadMeta");
const sessionsPane = $("sessionsPane");
const detailPane = $("detailPane");
function setStatus(text, error = false) { statusEl.textContent = text; statusEl.classList.toggle("error", error); }
function b64url(bytes) {
  let text = btoa(String.fromCharCode(...new Uint8Array(bytes)));
  return text.replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/g, "");
}
function b64urlDecode(text) {
  const padded = text.replaceAll("-", "+").replaceAll("_", "/") + "===".slice((text.length + 3) % 4);
  return Uint8Array.from(atob(padded), ch => ch.charCodeAt(0));
}
async function cryptoKey() {
  if (!crypto?.subtle) throw new Error("当前页面不是安全上下文，WebCrypto 不可用");
  const raw = new TextEncoder().encode($("key").value);
  const digest = await crypto.subtle.digest("SHA-256", raw);
  return crypto.subtle.importKey("raw", digest, "AES-GCM", false, ["encrypt", "decrypt"]);
}
async function encrypt(payload) {
  if (!crypto?.subtle) {
    setStatus("当前浏览器禁用 WebCrypto，已使用兼容模式", true);
    return { type: "plaintext", payload };
  }
  const key = await cryptoKey();
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const plain = new TextEncoder().encode(JSON.stringify(payload));
  const encrypted = await crypto.subtle.encrypt({ name: "AES-GCM", iv: nonce }, key, plain);
  return { type: "encrypted", nonce: b64url(nonce), payload: b64url(encrypted) };
}
async function decrypt(envelope) {
  if (envelope?.type === "plaintext") return envelope.payload;
  if (!envelope || envelope.type !== "encrypted") throw new Error("收到未加密数据包");
  const key = await cryptoKey();
  const nonce = b64urlDecode(envelope.nonce);
  const data = b64urlDecode(envelope.payload);
  const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: nonce }, key, data);
  return JSON.parse(new TextDecoder().decode(plain));
}
async function connect() {
  const room = encodeURIComponent($("room").value.trim());
  if (!room || !$("key").value) { setStatus("需要房间 ID 和 Key", true); return; }
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  if (socket) try { socket.close(); } catch {}
  socket = new WebSocket(`${scheme}://${location.host}/client?room=${room}&token=${room}`);
  socket.onopen = async () => { setStatus("已连接 relay，正在读取会话..."); try { await loadSessions(); } catch (e) { setStatus(e.message, true); } };
  socket.onclose = () => { appServerConnected = false; setStatus("已断开"); };
  socket.onerror = () => setStatus("连接错误", true);
  socket.onmessage = async (event) => {
    try {
      const message = JSON.parse(event.data);
      if (message.type === "registered") return;
      const response = await decrypt(message);
      if (response.type === "appServerConnected") {
        appServerConnected = true;
        const id = String(response.id ?? "");
        const resolver = pending.get(id);
        if (resolver) { pending.delete(id); resolver.resolve(response); }
        return;
      }
      if (response.type === "appServerMessage") {
        handleAppServerMessage(response.message);
        return;
      }
      if (response.type === "appServerClosed") {
        appServerConnected = false;
        for (const item of pendingRpc.values()) item.reject(new Error(response.error || "app-server 连接已关闭"));
        pendingRpc.clear();
        setStatus(response.error || "app-server 连接已关闭", !!response.error);
        return;
      }
      const id = String(response.id ?? "");
      const resolver = pending.get(id);
      if (resolver) { pending.delete(id); resolver.resolve(response); return; }
    } catch (error) {
      setStatus(`解密/解析失败：${error.message}`, true);
    }
  };
}
async function request(path, body = "", timeoutMs = 30000) {
  if (!socket || socket.readyState !== WebSocket.OPEN) throw new Error("未连接");
  const id = `${Date.now()}-${requestId++}`;
  const packet = await encrypt({ type: "httpRequest", id, method: body ? "POST" : "GET", path, headers: {}, body });
  socket.send(JSON.stringify(packet));
  return await new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    setTimeout(() => { if (pending.delete(id)) reject(new Error("请求超时")); }, timeoutMs);
  });
}
async function ensureAppServer() {
  if (appServerConnected && appServerSessionId) return;
  appServerSessionId = appServerSessionId || `mobile-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const id = `${Date.now()}-${requestId++}`;
  const packet = await encrypt({ type: "appServerConnect", id, sessionId: appServerSessionId });
  socket.send(JSON.stringify(packet));
  await new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    setTimeout(() => { if (pending.delete(id)) reject(new Error("app-server 连接超时")); }, 30000);
  });
  await rpcRaw("initialize", { clientInfo: { name: "Codex++ Mobile Relay", version: "1.0.0" }, capabilities: { experimentalApi: true } });
}
async function rpc(method, params = {}) {
  await ensureAppServer();
  return await rpcRaw(method, params);
}
async function rpcRaw(method, params = {}) {
  if (!socket || socket.readyState !== WebSocket.OPEN) throw new Error("未连接");
  if (!appServerSessionId) throw new Error("app-server 未连接");
  const id = rpcId++;
  const payload = { jsonrpc: "2.0", id, method, params };
  const packet = await encrypt({ type: "appServerMessage", sessionId: appServerSessionId, message: JSON.stringify(payload) });
  const promise = new Promise((resolve, reject) => {
    pendingRpc.set(String(id), { resolve, reject });
    setTimeout(() => { if (pendingRpc.delete(String(id))) reject(new Error(`${method} 超时`)); }, method === "turn/start" ? 60000 : 30000);
  });
  socket.send(JSON.stringify(packet));
  return promise;
}
function handleAppServerMessage(text) {
  let message;
  try { message = JSON.parse(text); } catch { return; }
  if (message.id != null) {
    const resolver = pendingRpc.get(String(message.id));
    if (!resolver) return;
    pendingRpc.delete(String(message.id));
    if (message.error) resolver.reject(new Error(message.error.message || "请求失败"));
    else resolver.resolve(message.result);
    return;
  }
  if (!message.method || !state.selectedId) return;
  const params = message.params || {};
  const threadId = eventThreadId(params);
  if (threadId && threadId !== state.selectedId) return;
  if (message.method === "item/agentMessage/delta") {
    const delta = extractDeltaText(params);
    if (delta) { appendAgentDelta(params, delta); setStatus("正在接收回复..."); }
    return;
  }
  if (message.method === "turn/started") {
    state.streaming = null;
    appendThinkingNode();
    setStatus("正在思考...");
    return;
  }
  if (message.method === "item/completed") {
    handleCompletedItem(params);
    return;
  }
  if (message.method === "turn/completed" || message.method === "thread/status/changed") {
    clearThinkingNode();
    setStatus("回复完成，正在同步...");
    refreshThread(state.selectedId).catch((error) => setStatus(`同步失败：${error.message}`, true));
  }
}
function eventThreadId(params) { return params.threadId || params.thread_id || params.thread?.id || params.turn?.threadId || params.turn?.thread_id || params.item?.threadId || params.item?.thread_id || ""; }
function extractDeltaText(params) {
  const value = params.delta ?? params.text ?? params.chunk ?? params.content ?? params.item?.text ?? "";
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map((part) => part?.text || part?.content || part?.delta || "").filter(Boolean).join("");
  if (value && typeof value === "object") return value.text || value.content || value.delta || value.value || "";
  return "";
}
function appendAgentDelta(params, delta) {
  if (messagesEl.querySelector(".empty")) messagesEl.innerHTML = "";
  clearThinkingNode();
  const turnId = params.turnId || params.turn_id || params.turn?.id || "";
  const itemId = params.itemId || params.item_id || params.item?.id || "";
  const key = `${turnId}:${itemId}`;
  if (!state.streaming || state.streaming.key !== key) {
    const node = appendMessageNode("Codex", "", false);
    state.streaming = { key, node, bubble: node.querySelector(".bubble"), text: "" };
  }
  state.streaming.text += delta;
  state.streaming.bubble.textContent = state.streaming.text;
  messagesEl.scrollTop = messagesEl.scrollHeight;
}
function handleCompletedItem(params) {
  const item = params.item || {};
  const text = itemText(item);
  if (!text) return;
  const role = itemRole(item);
  if (role === "用户") {
    reconcilePendingMessages(state.selectedId, new Map([[text, 1]]));
    confirmPendingMessageNode(text);
    return;
  }
  if (role !== "Codex") return;
  clearThinkingNode();
  if (state.streaming?.bubble) {
    state.streaming.text = text;
    state.streaming.bubble.textContent = text;
  } else {
    appendMessageNode("Codex", text);
  }
  messagesEl.scrollTop = messagesEl.scrollHeight;
}
async function loadSessions() {
  setStatus("正在读取会话...");
  const result = await rpc("thread/list", {});
  state.sessions = Array.isArray(result?.data) ? result.data : [];
  renderSessions();
  setStatus(`已加载 ${state.sessions.length} 个会话`);
}
function visibleSessions() {
  const filter = state.filter.trim().toLowerCase();
  if (!filter) return state.sessions;
  return state.sessions.filter((item) => [item.preview, item.cwd, item.id, item.modelProvider].filter(Boolean).join("\n").toLowerCase().includes(filter));
}
function renderSessions() {
  const items = visibleSessions();
  if (!items.length) { sessionsEl.innerHTML = `<div class="empty">没有会话</div>`; return; }
  sessionsEl.innerHTML = "";
  for (const group of groupSessionsByProject(items)) {
    const section = document.createElement("div");
    const collapsed = !state.expandedProjects.has(group.key);
    section.className = "group" + (collapsed ? " collapsed" : "");
    const title = document.createElement("div");
    title.className = "group-title";
    title.innerHTML = `<span class="chevron"></span><span class="group-name"></span><span></span><button class="group-new" type="button">+</button>`;
    title.querySelector(".chevron").textContent = collapsed ? ">" : "v";
    title.querySelector(".group-name").textContent = group.label;
    title.addEventListener("click", () => { collapsed ? state.expandedProjects.add(group.key) : state.expandedProjects.delete(group.key); renderSessions(); });
    title.querySelector(".group-new").addEventListener("click", (event) => { event.stopPropagation(); newThreadInProject(group.cwd); });
    section.appendChild(title);
    for (const item of group.items) {
      const button = document.createElement("button");
      button.className = "item" + (item.id === state.selectedId ? " active" : "");
      button.type = "button";
      button.innerHTML = `<div class="preview"></div><div class="meta"></div>`;
      button.querySelector(".preview").textContent = item.preview || item.name || item.id;
      button.querySelector(".meta").textContent = `${formatTime(item.updatedAt || item.createdAt)} · ${item.modelProvider || "provider 未记录"}`;
      button.onclick = () => selectThread(item.id);
      section.appendChild(button);
    }
    sessionsEl.appendChild(section);
  }
}
function groupSessionsByProject(items) {
  const groups = [], seen = new Map();
  for (const item of items) {
    const key = String(item.cwd || "").trim().toLowerCase() || "__unknown__";
    let group = seen.get(key);
    if (!group) { group = { key, label: projectLabel(item.cwd), cwd: item.cwd || "", items: [] }; seen.set(key, group); groups.push(group); }
    group.items.push(item);
  }
  return groups;
}
function projectLabel(cwd) {
  const value = String(cwd || "").trim();
  if (!value) return "未知目录";
  return value.split(/[\\/]/).filter(Boolean).pop() || value;
}
function newThreadInProject(cwd) {
  state.selectedId = null; state.selectedCwd = String(cwd || "").trim(); renderSessions();
  titleEl.textContent = "新建会话"; metaEl.textContent = state.selectedCwd || "未知目录"; messagesEl.innerHTML = `<div class="empty">输入第一条消息后发送</div>`;
  if (matchMedia("(max-width: 760px)").matches) { sessionsPane.classList.add("hidden"); detailPane.classList.remove("hidden"); }
  $("messageInput").focus();
}
async function selectThread(threadId) {
  state.selectedId = threadId; state.selectedCwd = ""; renderSessions();
  if (matchMedia("(max-width: 760px)").matches) { sessionsPane.classList.add("hidden"); detailPane.classList.remove("hidden"); }
  const item = state.sessions.find((entry) => entry.id === threadId);
  titleEl.textContent = item?.preview || item?.name || threadId; metaEl.textContent = `${item?.modelProvider || ""} · ${item?.cwd || ""}`;
  messagesEl.innerHTML = `<div class="empty">正在同步会话...</div>`;
  await refreshThread(threadId, item);
}
async function refreshThread(threadId, fallbackItem = null) {
  const item = fallbackItem || state.sessions.find((entry) => entry.id === threadId);
  const resumePromise = rpc("thread/resume", { threadId });
  const turnsPromise = rpc("thread/turns/list", { threadId });
  try {
    const turnsValue = await turnsPromise;
    const threadFromTurns = extractThread(turnsValue);
    const turns = extractTurns(turnsValue) || extractTurns(threadFromTurns);
    renderThread(threadFromTurns || item, normalizeTurns(turns), threadId);
    setStatus("会话内容已加载");
  } catch (error) {
    messagesEl.innerHTML = `<div class="empty error"></div>`;
    messagesEl.querySelector(".error").textContent = `消息列表不可用：${error.message}`;
    setStatus(`消息列表不可用：${error.message}`, true);
  }
  try {
    const resumeValue = await resumePromise;
    const thread = extractThread(resumeValue);
    if (thread && threadId === state.selectedId) {
      titleEl.textContent = thread.preview || thread.name || thread.id || "会话";
      metaEl.textContent = `${formatTime(thread.updatedAt || thread.createdAt)} · ${thread.cwd || ""}`;
    }
    setStatus("会话已打开");
  } catch (error) {
    setStatus(`会话内容已加载，打开状态同步失败：${error.message}`, true);
  }
}
function renderThread(thread, turns, threadId = state.selectedId) {
  if (thread) { titleEl.textContent = thread.preview || thread.name || thread.id || "会话"; metaEl.textContent = `${formatTime(thread.updatedAt || thread.createdAt)} · ${thread.cwd || ""}`; }
  messagesEl.innerHTML = "";
  const confirmedUserTexts = new Map();
  for (const turn of turns) {
    const items = turnItems(turn);
    for (const item of items) {
      const text = itemText(item);
      if (!text) continue;
      const role = itemRole(item);
      if (role === "用户") confirmedUserTexts.set(text, (confirmedUserTexts.get(text) || 0) + 1);
      appendMessageNode(role, text);
    }
  }
  reconcilePendingMessages(threadId, confirmedUserTexts);
  for (const message of pendingMessagesFor(threadId)) appendMessageNode("用户", message.text, true);
  if (!messagesEl.children.length) messagesEl.innerHTML = `<div class="empty">没有文本消息</div>`;
  messagesEl.scrollTop = messagesEl.scrollHeight;
}
function turnItems(turn) {
  if (!turn || typeof turn !== "object") return [turn];
  if (Array.isArray(turn.items)) return turn.items;
  if (Array.isArray(turn.messages)) return turn.messages;
  const items = [];
  if (turn.input != null) items.push({ type: "userMessage", content: turn.input });
  if (turn.output != null) items.push({ type: "agentMessage", content: turn.output });
  if (turn.request != null) items.push({ type: "userMessage", content: turn.request });
  if (turn.response != null) items.push({ type: "agentMessage", content: turn.response });
  return items.length ? items : [turn];
}
function extractThread(value) {
  if (!value || typeof value !== "object") return null;
  return value.thread || value.data?.thread || value.result?.thread || value.conversation || null;
}
function extractTurns(value) {
  if (!value) return null;
  if (Array.isArray(value)) return value;
  if (Array.isArray(value.data)) return value.data;
  if (Array.isArray(value.turns)) return value.turns;
  if (Array.isArray(value.items)) return [{ items: value.items, createdAt: value.createdAt, updatedAt: value.updatedAt }];
  if (Array.isArray(value.messages)) return value.messages;
  if (Array.isArray(value.thread?.turns)) return value.thread.turns;
  if (Array.isArray(value.thread?.items)) return [{ items: value.thread.items, createdAt: value.thread.createdAt, updatedAt: value.thread.updatedAt }];
  if (Array.isArray(value.data?.turns)) return value.data.turns;
  if (Array.isArray(value.data?.items)) return [{ items: value.data.items, createdAt: value.data.createdAt, updatedAt: value.data.updatedAt }];
  if (Array.isArray(value.conversation?.turns)) return value.conversation.turns;
  if (Array.isArray(value.conversation?.items)) return [{ items: value.conversation.items, createdAt: value.conversation.createdAt, updatedAt: value.conversation.updatedAt }];
  return null;
}
function appendMessageNode(role, text, pending = false) {
  if (messagesEl.querySelector(".empty")) messagesEl.innerHTML = "";
  const wrap = document.createElement("div");
  wrap.className = "turn";
  wrap.innerHTML = `<div class="role"></div><div class="bubble"></div>`;
  wrap.querySelector(".role").textContent = pending ? `${role} · 待同步` : role;
  wrap.querySelector(".bubble").classList.toggle("user", role === "用户");
  wrap.querySelector(".bubble").textContent = text;
  messagesEl.appendChild(wrap);
  return wrap;
}
function confirmPendingMessageNode(text) {
  for (const node of messagesEl.querySelectorAll(".turn")) {
    const role = node.querySelector(".role");
    const bubble = node.querySelector(".bubble");
    if (role?.textContent === "用户 · 待同步" && bubble?.textContent === text) {
      role.textContent = "用户";
      return;
    }
  }
}
function appendThinkingNode() {
  if (state.thinking?.isConnected) return state.thinking;
  const node = appendMessageNode("Codex", "正在思考...");
  node.dataset.thinking = "true";
  state.thinking = node;
  messagesEl.scrollTop = messagesEl.scrollHeight;
  return node;
}
function clearThinkingNode() {
  if (state.thinking?.isConnected) state.thinking.remove();
  state.thinking = null;
}
function rememberPendingMessage(threadId, text) {
  if (!threadId) return;
  const list = pendingMessagesFor(threadId);
  list.push({ text, createdAt: Date.now() });
  state.pendingMessages.set(threadId, list);
}
function pendingMessagesFor(threadId) {
  if (!threadId) return [];
  return state.pendingMessages.get(threadId) || [];
}
function forgetPendingMessage(threadId, text) {
  const remaining = pendingMessagesFor(threadId).filter((message) => message.text !== text);
  if (remaining.length) state.pendingMessages.set(threadId, remaining);
  else state.pendingMessages.delete(threadId);
}
function reconcilePendingMessages(threadId, confirmedUserTexts = new Map()) {
  const pending = pendingMessagesFor(threadId);
  if (!pending.length) return;
  const remaining = pending.filter((message) => {
    const expired = Date.now() - message.createdAt > 120000;
    const confirmedCount = confirmedUserTexts.get(message.text) || 0;
    if (confirmedCount > 0) {
      confirmedUserTexts.set(message.text, confirmedCount - 1);
      return false;
    }
    return !expired;
  });
  if (remaining.length) state.pendingMessages.set(threadId, remaining);
  else state.pendingMessages.delete(threadId);
}
function pollThread(threadId, attempt = 0) {
  if (!threadId) return;
  if (attempt === 0 && state.pollTimers.has(threadId)) {
    clearTimeout(state.pollTimers.get(threadId));
    state.pollTimers.delete(threadId);
  }
  if (attempt > 60) {
    state.pollTimers.delete(threadId);
    setStatus("已发送，回复可能仍在生成，稍后可重新打开会话同步");
    return;
  }
  const delay = attempt < 8 ? 1200 : 3000;
  const timer = window.setTimeout(async () => {
    try {
      await refreshThread(threadId);
      if (pendingMessagesFor(threadId).length) {
        pollThread(threadId, attempt + 1);
      } else {
        state.pollTimers.delete(threadId);
      }
    } catch (error) {
      setStatus(`同步失败：${error.message}`, true);
      pollThread(threadId, attempt + 1);
    }
  }, delay);
  state.pollTimers.set(threadId, timer);
}
function normalizeTurns(turns) { return Array.isArray(turns) ? [...turns].sort((a, b) => turnTimestamp(a) - turnTimestamp(b)) : []; }
function turnTimestamp(turn) { const value = turn?.startedAt || turn?.createdAt || turn?.completedAt || 0; return value < 100000000000 ? value * 1000 : value; }
function itemRole(item) {
  const raw = String(item?.role || item?.author?.role || item?.message?.role || item?.item?.role || item?.type || "").toLowerCase();
  if (raw === "user" || raw === "usermessage" || raw === "input_text" || raw === "input") return "用户";
  if (raw === "assistant" || raw === "agent" || raw === "codex" || raw === "agentmessage" || raw === "assistantmessage" || raw === "output_text" || raw === "output") return "Codex";
  if (raw === "toolcall" || raw === "tool_call" || raw === "function_call") return "工具";
  if (raw === "toolresult" || raw === "tool_result" || raw === "function_call_output") return "工具结果";
  return item?.type || item?.role || "消息";
}
function itemText(item) {
  const text = extractText(item, 0);
  return typeof text === "string" ? text.trim() : "";
}
function extractText(value, depth) {
  if (value == null || depth > 6) return "";
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map((part) => extractText(part, depth + 1)).filter(Boolean).join("\n");
  if (typeof value !== "object") return "";
  for (const key of ["text", "output_text", "input_text", "markdown", "value", "delta", "content", "message", "output", "input", "parts", "payload", "item", "data"]) {
    if (value[key] == null) continue;
    const text = extractText(value[key], depth + 1);
    if (text) return text;
  }
  return "";
}
function formatTime(value) { if (!value) return "未知时间"; const ms = value < 100000000000 ? value * 1000 : value; return new Date(ms).toLocaleString(); }
async function sendMessage(threadId, text, skipResume = false) {
  if (!skipResume) await rpc("thread/resume", { threadId });
  return await rpc("turn/start", { threadId, clientUserMessageId: `codex-plus-mobile-${Date.now()}`, input: [{ type: "text", text }] });
}
$("connect").onclick = connect;
$("filter").oninput = (event) => { state.filter = event.target.value; renderSessions(); };
$("back").onclick = () => { detailPane.classList.add("hidden"); sessionsPane.classList.remove("hidden"); };
$("composer").onsubmit = async (event) => {
  event.preventDefault();
  const input = $("messageInput");
  const text = input.value.trim();
  if (!text) return;
  input.value = "";
  let threadId = state.selectedId;
  let isNew = false;
  try {
    if (!threadId) {
      const result = await rpc("thread/start", state.selectedCwd ? { cwd: state.selectedCwd } : {});
      const thread = result?.thread || result?.data || result;
      threadId = thread?.id || result?.threadId || result?.id;
      if (!threadId) throw new Error("新建会话失败：app-server 未返回 thread id");
      state.selectedId = threadId;
      isNew = true;
    }
    rememberPendingMessage(threadId, text);
    appendMessageNode("用户", text, true);
    setStatus("正在思考...");
    pollThread(threadId);
    sendMessage(threadId, text, isNew)
      .then(() => setStatus("已发送，正在同步回复..."))
      .catch((error) => {
        forgetPendingMessage(threadId, text);
        setStatus(error.message, true);
      });
  } catch (error) {
    forgetPendingMessage(threadId, text);
    setStatus(error.message, true);
  }
};
if (params.get("room")) $("room").value = params.get("room");
if (params.get("key")) $("key").value = params.get("key");
if (params.get("auto") === "1" && $("room").value && $("key").value) connect();
</script>
</body>
</html>"#
        .to_string()
}

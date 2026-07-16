//! Minimal CDP helpers: list targets and Runtime.evaluate injection.

use crate::error::AppError;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct CdpTarget {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub web_socket_debugger_url: Option<String>,
}

/// True when a page target is a Codex shell we may inject into.
///
/// Live Windows Codex Desktop exposes Electron pages as `app://-/index.html`
/// (title "Codex"), not only https://chatgpt.com / openai.com URLs.
fn is_codex_page_url(url: &str, title: &str) -> bool {
    let url_l = url.to_ascii_lowercase();
    let title_l = title.to_ascii_lowercase();
    url_l.contains("chatgpt.com")
        || url_l.contains("openai.com")
        || url_l.contains("codex")
        || url_l.starts_with("app://")
        || title_l.contains("codex")
}

/// Prefer the main Codex shell over avatar/overlay helper windows.
fn codex_page_rank(url: &str) -> u8 {
    let u = url.to_ascii_lowercase();
    if u.contains("overlay") || u.contains("avatar") {
        return 2;
    }
    if u.starts_with("app://") || u.contains("chatgpt.com") || u.contains("codex") {
        return 0;
    }
    1
}

/// Select the best injectable Codex page target (never service_worker / iframe-only).
pub fn pick_codex_target(targets: &[CdpTarget]) -> Option<&CdpTarget> {
    targets
        .iter()
        .filter(|t| {
            t.kind == "page"
                && t.web_socket_debugger_url.is_some()
                && is_codex_page_url(&t.url, &t.title)
        })
        .min_by_key(|t| codex_page_rank(&t.url))
}

pub async fn list_targets(cdp_port: u16) -> Result<Vec<CdpTarget>, AppError> {
    let url = format!("http://127.0.0.1:{cdp_port}/json/list");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| AppError::Config(format!("cdp client: {e}")))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Config(format!("cdp list failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Config(format!(
            "cdp list status {}",
            resp.status()
        )));
    }
    resp.json::<Vec<CdpTarget>>()
        .await
        .map_err(|e| AppError::Config(format!("cdp list parse: {e}")))
}

pub async fn inject_script(cdp_port: u16, script: &str) -> Result<(), AppError> {
    let targets = list_targets(cdp_port).await?;
    let target = pick_codex_target(&targets).ok_or_else(|| {
        AppError::Config("未找到可注入的 Codex page target（仅 page 类型）".into())
    })?;
    let ws_url = target
        .web_socket_debugger_url
        .clone()
        .ok_or_else(|| AppError::Config("target missing webSocketDebuggerUrl".into()))?;

    inject_via_websocket(&ws_url, script).await
}

async fn inject_via_websocket(ws_url: &str, script: &str) -> Result<(), AppError> {
    use futures_util::SinkExt;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let (mut ws, _) = connect_async(ws_url)
        .await
        .map_err(|e| AppError::Config(format!("cdp ws connect: {e}")))?;

    // Store Codex CSP connect-src omits loopback. Live probe proved:
    // setBypassCSP alone does NOT unlock the current document; bypass + reload does.
    let setup_cmds = [
        json!({"id": 1, "method": "Page.enable", "params": {}}),
        json!({"id": 2, "method": "Page.setBypassCSP", "params": {"enabled": true}}),
        json!({"id": 3, "method": "Page.reload", "params": {"ignoreCache": false}}),
    ];

    for cmd in &setup_cmds {
        let id = cmd["id"].as_i64().unwrap_or(0);
        ws.send(Message::Text(cmd.to_string().into()))
            .await
            .map_err(|e| AppError::Config(format!("cdp send id {id}: {e}")))?;
        let soft = id == 1; // Page.enable may be optional
        let res = wait_for_cdp_id(&mut ws, id, 5).await;
        if let Err(e) = res {
            if !soft {
                return Err(e);
            }
        }
    }

    // Poll document.readyState after reload before injecting.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut poll_id: i64 = 100;
    while std::time::Instant::now() < deadline {
        poll_id += 1;
        let probe = json!({
            "id": poll_id,
            "method": "Runtime.evaluate",
            "params": {
                "expression": "document.readyState",
                "returnByValue": true
            }
        });
        ws.send(Message::Text(probe.to_string().into()))
            .await
            .map_err(|e| AppError::Config(format!("cdp ready send: {e}")))?;
        if let Ok(state) = wait_for_cdp_string(&mut ws, poll_id, 2).await {
            if state == "complete" || state == "interactive" {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    let eval = json!({
        "id": 10,
        "method": "Runtime.evaluate",
        "params": {
            "expression": script,
            "awaitPromise": false,
            "returnByValue": true
        }
    });
    ws.send(Message::Text(eval.to_string().into()))
        .await
        .map_err(|e| AppError::Config(format!("cdp send evaluate: {e}")))?;
    wait_for_cdp_id(&mut ws, 10, 5).await
}

async fn wait_for_cdp_id(
    ws: &mut (
        impl StreamExt<Item = Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>>
            + Unpin
    ),
    expect_id: i64,
    secs: u64,
) -> Result<(), AppError> {
    use tokio_tungstenite::tungstenite::Message;

    tokio::time::timeout(std::time::Duration::from_secs(secs), async {
        while let Some(msg) = ws.next().await {
            let msg = msg.map_err(|e| AppError::Config(format!("cdp recv: {e}")))?;
            if let Message::Text(t) = msg {
                let v: Value = serde_json::from_str(&t)
                    .map_err(|e| AppError::Config(format!("cdp json: {e}")))?;
                if v.get("id").and_then(|x| x.as_i64()) == Some(expect_id) {
                    if v.get("error").is_some() {
                        return Err(AppError::Config(format!(
                            "cdp id {expect_id} error: {v}"
                        )));
                    }
                    return Ok(());
                }
            }
        }
        Err(AppError::Config(format!(
            "cdp id {expect_id}: connection closed"
        )))
    })
    .await
    .map_err(|_| AppError::Config(format!("cdp id {expect_id} timeout")))?
}

async fn wait_for_cdp_string(
    ws: &mut (
        impl StreamExt<Item = Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>>
            + Unpin
    ),
    expect_id: i64,
    secs: u64,
) -> Result<String, AppError> {
    use tokio_tungstenite::tungstenite::Message;

    tokio::time::timeout(std::time::Duration::from_secs(secs), async {
        while let Some(msg) = ws.next().await {
            let msg = msg.map_err(|e| AppError::Config(format!("cdp recv: {e}")))?;
            if let Message::Text(t) = msg {
                let v: Value = serde_json::from_str(&t)
                    .map_err(|e| AppError::Config(format!("cdp json: {e}")))?;
                if v.get("id").and_then(|x| x.as_i64()) == Some(expect_id) {
                    if v.get("error").is_some() {
                        return Ok(String::new());
                    }
                    let s = v
                        .pointer("/result/result/value")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    return Ok(s);
                }
            }
        }
        Err(AppError::Config(format!(
            "cdp id {expect_id}: connection closed"
        )))
    })
    .await
    .map_err(|_| AppError::Config(format!("cdp id {expect_id} timeout")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(kind: &str, url: &str) -> CdpTarget {
        target_with_title(kind, url, "")
    }

    fn target_with_title(kind: &str, url: &str, title: &str) -> CdpTarget {
        CdpTarget {
            id: "1".into(),
            kind: kind.into(),
            url: url.into(),
            title: title.into(),
            web_socket_debugger_url: Some("ws://127.0.0.1:9/devtools/page/1".into()),
        }
    }

    #[test]
    fn picks_only_injectable_codex_page_target() {
        let targets = [
            target("service_worker", "https://chatgpt.com/sw"),
            target("page", "https://chatgpt.com/codex/tasks/1"),
        ];
        let t = pick_codex_target(&targets).unwrap();
        assert_eq!(t.kind, "page");
        assert!(t.url.contains("codex"));
    }

    #[test]
    fn ignores_non_page_targets() {
        let targets = [target("service_worker", "https://chatgpt.com/codex")];
        assert!(pick_codex_target(&targets).is_none());
    }

    #[test]
    fn picks_live_electron_app_shell_over_overlay() {
        let targets = [
            target_with_title(
                "page",
                "app://-/index.html?initialRoute=%2Favatar-overlay",
                "Codex",
            ),
            target_with_title("page", "app://-/index.html", "Codex"),
            target("worker", ""),
        ];
        let t = pick_codex_target(&targets).unwrap();
        assert_eq!(t.url, "app://-/index.html");
    }

    #[test]
    fn picks_app_shell_by_title_when_url_has_no_codex_token() {
        let targets = [target_with_title("page", "app://-/index.html", "Codex")];
        assert!(pick_codex_target(&targets).is_some());
    }
}

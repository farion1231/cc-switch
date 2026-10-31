use tauri::{command, AppHandle};

async fn apply_remote_config(
    app: &AppHandle,
    port: u16,
    tailscale_enabled: bool,
    action: &str,
) -> Result<String, String> {
    log::info!(
        "[Remote] {action}_remote_server called with port: {port}, tailscale: {tailscale_enabled}"
    );

    let config = crate::remote::RemoteConfig {
        enabled: true,
        port,
        tailscale_enabled,
        // 暴露边界（绑定范围 / CORS / 白名单）不从这里进：start / restart 只管
        // 端口与 Tailscale，那三项一律从 gate 读当前生效值 —— 否则「改了端口
        // 重启一下」会把先前确认过的绑定范围悄悄退回回环。
        ..exposure_of(app).await
    };

    let urls = crate::remote::start_remote(app, config).await?;
    log::info!("[Remote] Server {action}ed on URLs: {:?}", urls);
    Ok(format!("Remote server {action}ed on: {}", urls.join(", ")))
}

#[command]
pub async fn start_remote_server(
    app: AppHandle,
    port: u16,
    tailscale_enabled: bool,
) -> Result<String, String> {
    apply_remote_config(&app, port, tailscale_enabled, "start").await
}

#[command]
pub async fn stop_remote_server(app: AppHandle) -> Result<String, String> {
    crate::remote::stop_remote(&app).await?;
    Ok("Remote server stopped".to_string())
}

/// Restart the remote server without broadcasting SSE shutdown.
/// Used for Tailscale toggle and port changes to avoid "closed" page on browsers.
#[command]
pub async fn restart_remote_server(
    app: AppHandle,
    port: u16,
    tailscale_enabled: bool,
) -> Result<String, String> {
    apply_remote_config(&app, port, tailscale_enabled, "restart").await
}

#[command]
pub fn check_tailscale_available() -> bool {
    crate::remote::is_tailscale_available()
}

#[command]
pub fn get_tailscale_ip() -> Option<String> {
    crate::remote::get_tailscale_ip()
}

/// 本机所有非回环 IPv4 地址（网卡名 + 地址）。
///
/// 只读，不参与绑定决策：设置页在「全部网卡」档位下用它列出「你到底会在
/// 哪些地址上暴露」，真正绑到哪仍由 `BindScope` 决定（那一档绑 `0.0.0.0`）。
#[command]
pub fn list_network_addresses() -> Vec<NetworkAddress> {
    crate::remote::local_ipv4_addresses()
        .into_iter()
        .map(|(interface, address)| NetworkAddress {
            interface,
            address: address.to_string(),
        })
        .collect()
}

/// 一条「本机在哪张网卡上有什么地址」。`interface` 是 OS 给的网卡名
/// （`en0` / `utun4` …），仅用于让用户认出这条地址属于哪张网卡。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAddress {
    pub interface: String,
    pub address: String,
}

// --- 对外暴露边界（绑定范围 / CORS / Origin 白名单）---
//
// 一条不可协商的分界：**收紧**随时可做，**放宽**只有一条路。
//
// 收紧 = 改 CORS 模式、往白名单里加一条具体 origin、把绑定范围往窄的档位拨
// （`AllInterfaces → Tailscale → Loopback`）。
// 放宽 = 让别的机器能连上（往 `AllInterfaces` 方向拨）或让别的源能发请求
// （`ExplicitWhitelist`）。
//
// 渲染层能直接做的事因此只有两件：调 `setRemoteConfig` 收紧 CORS，或者调
// `requestBindScopeChange` **申请**档位变更。收紧由 gate 立即兑现；放宽返回的
// 是待确认状态，真正的落盘发生在**原生弹窗**确认之后 —— 弹窗由 Rust 侧弹、
// Rust 侧收结果，渲染层没有「我批准了」这条通道。
//
// 如果「二次确认」只是渲染层一个 `confirm()` 再调一次 `set_remote_config`，
// 那么任何能执行渲染层代码的路径都能跳过它 —— 确认就成了装饰品。
//
// 安全边界：
// - 远程 REST 的同一组端点（见 `remote/handlers.rs`）是 `Protected`：非回环来源
//   仍然必须持有配对凭证，本功能不改既有鉴权模型。
// - 对已被完全攻陷且已配对的客户端，两段式是提高门槛 + 留痕，不是硬边界。

use crate::remote::auth::SessionIdentity;
use crate::remote::config::{BindScopeGate, BindScopeRequest};
use crate::remote::{BindScope, RemoteConfig};
use std::time::Instant;
use tauri::Runtime;

/// 两段式申请的结果（桌面端与浏览器共用同一形状）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindScopeChangeResponse {
    /// 这次调用结束时目标档位是否已经生效。
    pub applied: bool,
    /// 调用结束时**生效中**的档位（待确认 / 用户取消时仍是旧值）。
    pub bind_scope: BindScope,
    /// 仅浏览器侧的放宽申请返回：第二段的凭据。桌面端走原生弹窗，
    /// 不会把 id 交给渲染层。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmation_id: Option<String>,
    /// 仅浏览器侧返回：Unix 秒。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

impl BindScopeChangeResponse {
    fn applied(bind_scope: BindScope) -> Self {
        Self {
            applied: true,
            bind_scope,
            confirmation_id: None,
            expires_at: None,
        }
    }

    fn not_applied(bind_scope: BindScope) -> Self {
        Self {
            applied: false,
            bind_scope,
            confirmation_id: None,
            expires_at: None,
        }
    }
}

/// 取当前生效的暴露边界。
///
/// 服务没起时没有「运行中的配置」可言：端口与 enabled 取真实设置，暴露边界
/// 取落盘值（与启动路径读的是同一份文件），所以不会因为这里返回默认而丢配置、
/// 也不会把端口报成默认的 4000。
async fn exposure_of<R: Runtime>(app: &AppHandle<R>) -> RemoteConfig {
    use tauri::Manager;
    let managed = app.try_state::<ManagedRemoteServer>();
    if let Some(managed) = managed {
        let guard = managed.lock().read().await;
        if let Some(server) = guard.as_ref() {
            let base = server.state().config_snapshot().await;
            return server.state().exposure.overlay(&base);
        }
    }
    let settings = crate::settings::get_settings();
    let base = RemoteConfig {
        enabled: settings.remote_enabled,
        port: settings.remote_port,
        tailscale_enabled: settings.remote_tailscale_enabled,
        ..RemoteConfig::default()
    };
    BindScopeGate::load(auth_config_dir()).overlay(&base)
}

/// 取运行中的闸门；服务没起时读同一份落盘文件。
///
/// 两条路读的是同一个 `exposure.json`，所以「没起服务时改档位」与「起服务后
/// 改档位」落在同一处。
async fn gate_of<R: Runtime>(app: &AppHandle<R>) -> BindScopeGate {
    use tauri::Manager;
    let gate = {
        let managed = app.try_state::<ManagedRemoteServer>();
        match managed {
            Some(managed) => {
                let guard = managed.lock().read().await;
                guard.as_ref().map(|server| server.state().exposure.clone())
            }
            None => None,
        }
    };
    gate.unwrap_or_else(|| BindScopeGate::load(auth_config_dir()))
}

/// 让运行中的 listener 跟上已经落盘的新档位。服务没起时是 no-op。
async fn rebind_bind_scope<R: Runtime>(app: &AppHandle<R>, scope: BindScope) -> Result<(), String> {
    use tauri::Manager;
    let state = {
        let Some(managed) = app.try_state::<ManagedRemoteServer>() else {
            return Ok(());
        };
        let guard = managed.lock().read().await;
        guard.as_ref().map(|server| server.state())
    };
    match state {
        Some(state) => state.rebind_bind_scope(scope).await,
        None => Ok(()),
    }
}

/// 读当前暴露边界。
///
/// 泛型化的理由与安全无关，纯粹是可测性：`AppHandle<Wry>` 会把这族命令钉死在
/// 生产那一个 runtime 上，而行为级闸门要用 `tauri::test::mock_builder` 的
/// runtime 无关 mock 夹具把它们真调一遍（见 `lib.rs::remote_invoke_handler`）。
#[command]
pub async fn get_remote_config<R: Runtime>(app: AppHandle<R>) -> Result<RemoteConfig, String> {
    Ok(exposure_of(&app).await)
}

/// 暴露边界写入的判定内核：收窄实参、校验白名单、落盘，全在这一行。
///
/// 拆出来的唯一理由是可测性。`#[tauri::command]` 那一层要 `AppHandle<R>`，而
/// 「传进来的 `bind_scope` 一律不认」这条判据必须能在没有 Tauri runtime 的情况
/// 下被真调一遍 —— 判据的强度不该取决于本轮能不能新增一条 crate。
///
/// 签名里也没有「绑定范围」这个参数：整份 `config` 进来，被取走的只有
/// `cors_mode` 与 `origin_whitelist`。[`BindScopeGate::set_cors`] 里没有
/// `bind_scope` 那一行 —— 那不是「读出来再丢回去」，是压根不读。
pub fn apply_exposure_config(gate: &BindScopeGate, config: &RemoteConfig) -> Result<(), String> {
    // 白名单校验在这里发生，`Origin: null` / `*` 到不了落盘。
    gate.set_cors(config.cors_mode, &config.origin_whitelist)
}

/// 写 CORS 模式与 Origin 白名单。
///
/// **绑定范围不在这里**：本命令只接受 `cors_mode` 与 `origin_whitelist`，传进来
/// 的 `bind_scope` 一律忽略。要改绑定范围只能走
/// [`request_bind_scope_change`] 的两段式。
#[command]
pub async fn set_remote_config<R: Runtime>(
    app: AppHandle<R>,
    config: RemoteConfig,
) -> Result<(), String> {
    let gate = gate_of(&app).await;
    apply_exposure_config(&gate, &config)
}

/// 申请把绑定范围改成 `scope`。
///
/// - **收紧**：gate 立即落盘，本命令随即重绑 listener，返回 `applied: true`。
/// - **放宽**：gate 只登记一条待确认请求；本命令**在这里弹原生对话框**，
///   用户点「确认」之后才走 gate 的确认入口并重绑。取消则撤销这条申请。
///   渲染层拿到的不是一个可以自己兑现的 id —— 它根本没有第二段的通道。
///
/// 为什么不放一条 `confirm_bind_scope_change` 命令给渲染层调用：那样「确认」
/// 又变回渲染层的一行代码，这条闸门就只剩形式。浏览器侧存在 REST 确认端点，
/// 因为它的确认方是**另一个已认证会话**，而不是同一个 webview。
#[command]
pub async fn request_bind_scope_change<R: Runtime>(
    app: AppHandle<R>,
    scope: BindScope,
) -> Result<BindScopeChangeResponse, String> {
    let gate = gate_of(&app).await;
    let request = gate.request_bind_scope_change(scope, SessionIdentity::LocalWindow)?;

    match request {
        BindScopeRequest::Applied(scope) => {
            rebind_bind_scope(&app, scope).await?;
            Ok(BindScopeChangeResponse::applied(scope))
        }
        BindScopeRequest::Pending {
            confirmation_id, ..
        } => {
            let current = gate.read_remote_config().bind_scope;
            let port = exposure_of(&app).await.port;
            let approved = show_native_bind_scope_confirmation(&app, current, scope, port).await;

            if !approved {
                let _ = gate.reject_bind_scope_change(
                    &confirmation_id,
                    SessionIdentity::LocalWindow,
                    Instant::now(),
                );
                log::info!("[Remote] bind scope widening declined in native dialog");
                return Ok(BindScopeChangeResponse::not_applied(current));
            }

            let applied = gate.confirm_bind_scope_change(
                &confirmation_id,
                SessionIdentity::LocalWindow,
                Instant::now(),
            )?;
            rebind_bind_scope(&app, applied).await?;
            Ok(BindScopeChangeResponse::applied(applied))
        }
    }
}

/// 原生确认弹窗：列出「当前 → 目标」以及新增的可达地址。
///
/// `show` 是非阻塞 API + oneshot 回传，而不是 `blocking_show`：命令跑在 tokio
/// worker 上，阻塞式弹窗会占住一个 worker 直到用户点按钮。
async fn show_native_bind_scope_confirmation<R: Runtime>(
    app: &AppHandle<R>,
    current: BindScope,
    target: BindScope,
    port: u16,
) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let chinese = crate::is_chinese_locale();
    let tier = |scope: BindScope| match (chinese, scope) {
        (true, BindScope::Loopback) => "仅本机 (127.0.0.1)",
        (true, BindScope::Tailscale) => "仅 Tailscale",
        (true, BindScope::AllInterfaces) => "全部网卡 (0.0.0.0)",
        (false, BindScope::Loopback) => "This machine only (127.0.0.1)",
        (false, BindScope::Tailscale) => "Tailscale only",
        (false, BindScope::AllInterfaces) => "All interfaces (0.0.0.0)",
    };
    let addresses = crate::remote::reachable_addresses(target, port);
    let address_lines = if addresses.is_empty() {
        String::new()
    } else {
        addresses
            .iter()
            .map(|address| format!("  • {address}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let message = if chinese {
        format!(
            "远程管理服务的绑定范围将从「{}」放宽到「{}」。\n\n\
             这会新增以下可达地址：\n{}\n\n\
             非回环来源的请求仍然必须持有配对凭证。\n\n\
             确认放宽绑定范围？",
            tier(current),
            tier(target),
            address_lines
        )
    } else {
        format!(
            "The remote management server's binding scope will be widened from \
             \"{}\" to \"{}\".\n\nThis adds the following reachable addresses:\n{}\n\n\
             Requests from non-loopback sources still require a pairing credential.\n\n\
             Widen the binding scope?",
            tier(current),
            tier(target),
            address_lines
        )
    };

    let (confirm, cancel) = if chinese {
        ("确认放宽", "取消")
    } else {
        ("Widen", "Cancel")
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title(if chinese {
            "确认放宽绑定范围"
        } else {
            "Confirm Binding Scope"
        })
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            confirm.to_string(),
            cancel.to_string(),
        ))
        .show(move |approved| {
            let _ = tx.send(approved);
        });

    rx.await.unwrap_or(false)
}

// --- 配对审批与设备管理（桌面端侧）---
//
// 这四条与 HTTP 那四条（`/pairing/approve`、`/pairing/revoke` …）作用在同一份状态
// 上，但入口是 Tauri 命令而不是网络路由：**审批是 human-in-the-loop 的那一环**，
// 把它留在 GUI 进程内比再开一条 loopback HTTP 路由少一整层可能被误配的鉴权。
//
// 三条边界贯穿下面每一条：
//
// 1. 返回值只有元数据（客户端名、创建时间、最近使用）。凭据库里存的摘要**不**
//    跟着发到前端 —— 它是一份「可复现 token」的判定材料。
// 2. 失败一律是同一类错误字符串，不区分「配对码不存在 / 已过期 / 已用掉」。
//    分开报等于给出一个 oracle，让调用方能反推某个配对码的存活状态。
// 3. 证据缺失（`whois` 超时、找不到 tailscale 二进制）**不会**让审批失败，见
//    [`crate::remote::pairing::approve`]。证据由后端**重算**，不用前端送来的快照 ——
//    快照是谁都能伪造的，见 [`approve_bound`]。

use crate::remote::pairing::{
    approve, list_client_meta, revoke_client_by_name, ClientMeta, PairingApiState,
};
use crate::remote::ManagedRemoteServer;

/// 凭据目录。与 `RemoteServer` 用的是同一个 `get_app_config_dir()` —— 中间件读
/// 的就是这里的 `remote-auth/clients.json`。
fn auth_config_dir() -> std::path::PathBuf {
    crate::config::get_app_config_dir()
}

/// 取运行中的配对状态。
///
/// 远程服务没起就没有待批请求可言，因此这里报「未运行」而不是自己另建一个
/// store —— 另建的那个 CLI 永远看不到，只能让人以为批了。
async fn pairing_state(app: &AppHandle) -> Result<PairingApiState, String> {
    use tauri::Manager;
    let managed = app.state::<ManagedRemoteServer>();
    let guard = managed.lock().read().await;
    guard
        .as_ref()
        .map(|server| server.state().pairing.clone())
        .ok_or_else(|| "remote server is not running".to_string())
}

/// 所有失败共用的一句话。
///
/// 「配对码不存在 / 已过期 / 已被用掉 / 不属于这条请求」必须折叠成同一个出口：
/// 分开报就等于给出一个 oracle，让调用方能反推某个配对码的存活状态。
const DENIED: &str = "unknown or no longer pending pairing request";

/// 后端重算出的来源核不出来、而界面没交回确认时说的话。
///
/// 刻意**不**并进 [`DENIED`]：那条说的是「这条请求已经不在了」，把「你没勾那个
/// 确认」说成「请求不见了」，用户会去重开一次配对，而不是把勾补上。这条命令只
/// 在 GUI 进程内可达，不构成对配对请求发起方的 oracle。
const UNCONFIRMED: &str =
    "source of this pairing request could not be verified; explicit confirmation required";

/// 批准一条待审请求。
///
/// `request_id` 与 `user_code` 都收：配对码是状态转移的键，`request_id` 用来核对
/// 这条码确实属于用户刚看过的那条请求（界面上两者来自同一条审批事件）。
///
/// 证据**在后端重算**，不用前端送来的快照：快照是谁都能伪造的，而重算要的是
/// `tailscale whois <发起方 IP>` 与 `tailscale status --json` 的一次现场比对
/// （见 [`crate::remote::pairing::evidence_for`]）。`whois` 与 `status` 各自 2s
/// 上限，最坏 4s —— 这一条命令本来就跑在 `async fn` 上，不占 GUI 线程。
///
/// `whois` 超时导致的证据缺失**不会**让本命令变成一条死路，但它也不再默认放行：
/// 重算出来的来源核不出来时，必须由界面勾过一次「我已知悉来源无法核实」
/// （`confirmed_unverified_source`）才放行，否则报 [`UNCONFIRMED`] 且请求仍待审。
///
/// 这个标志位**只能由界面勾选结果决定**：后端重算看到的来源可能与用户在界面上
/// 看到的那一份不同，命令层若替用户默认同意，就等于给一条从未被确认过的授权开了
/// 后门。
///
/// `confirmed_foreign_account` 是**第二道**独立的门：对外来账号（`tailscale whois`
/// 的 `Node.User.ID` 与 `Self.UserID` 不等）的默认拒绝由状态机自己执行，见
/// [`crate::remote::pairing::approve`]。界面上那个勾是提示，不是防线 —— 绕开渲染
/// 层的调用同样必须被这道闸门拦下。缺省成 `true` 则是给所有请求开后门，故前端
/// 必须如实传值。
#[command]
pub async fn approve_pairing(
    app: AppHandle,
    request_id: String,
    user_code: String,
    confirmed_unverified_source: bool,
    confirmed_foreign_account: bool,
) -> Result<(), String> {
    let pairing = pairing_state(&app).await?;
    approve_bound(
        &pairing.store,
        &request_id,
        &user_code,
        confirmed_unverified_source,
        confirmed_foreign_account,
    )
    .await?;
    Ok(())
}

/// 核对 `request_id` ↔ `user_code` 的绑定，后端重算一次证据，然后批准。
///
/// 拆出来是为了能直接测：命令层要 `AppHandle` + 正在运行的远程服务，而这里只要
/// 一个 [`PairingStore`]。返回重算出来的决策供调用方记审计。
async fn approve_bound(
    store: &crate::remote::pairing::PairingStore,
    request_id: &str,
    user_code: &str,
    confirmed_unverified_source: bool,
    confirmed_foreign_account: bool,
) -> Result<Option<crate::remote::pairing::ApprovalDecision>, String> {
    let Some(bound_code) = store.user_code_for_request(request_id) else {
        return Err(DENIED.to_string());
    };
    if bound_code != user_code {
        return Err(DENIED.to_string());
    }

    // 重算要用「当初是谁发起的」—— 批准命令的 peer 是桌面端自己，那个地址查不出
    // 任何东西。来源 IP 在请求落库时就记下了。
    let peer = store
        .peer_for_request(request_id)
        .ok_or_else(|| DENIED.to_string())?;
    let evidence = crate::remote::pairing::evidence_for(std::net::SocketAddr::new(peer, 0)).await;

    let decision = approve(
        store,
        user_code,
        Some(&evidence),
        confirmed_unverified_source,
        confirmed_foreign_account,
        Instant::now(),
    )
    .map_err(|e| match e {
        crate::remote::pairing::PairingError::Unconfirmed => UNCONFIRMED.to_string(),
        _ => DENIED.to_string(),
    })?;

    log::info!(
        "[Remote] pairing: 已批准请求 {request_id}，证据来源 {:?} / 外来账号 {} / 决策 {:?}",
        evidence.source_kind,
        evidence.is_foreign_account,
        decision
    );
    Ok(decision)
}

/// 拒绝一条待审请求。`Rejected` 是终态，不可逆。
#[command]
pub async fn reject_pairing(app: AppHandle, request_id: String) -> Result<(), String> {
    let pairing = pairing_state(&app).await?;

    let Some(user_code) = pairing.store.user_code_for_request(&request_id) else {
        return Err(DENIED.to_string());
    };
    pairing
        .store
        .reject(&user_code, Instant::now())
        .map_err(|_| DENIED.to_string())
}

/// 列出已配对客户端。**只回元数据**，没有摘要、没有明文。
#[command]
pub fn list_paired_clients() -> Result<Vec<ClientMeta>, String> {
    list_client_meta(&auth_config_dir())
}

/// 按客户端名撤销其全部凭据。返回是否摘到了东西（撤销是幂等的）。
#[command]
pub fn revoke_client(client_name: String) -> Result<bool, String> {
    if client_name.is_empty() {
        return Err("client name must not be empty".to_string());
    }
    revoke_client_by_name(&auth_config_dir(), &client_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    /// 远程服务这一族命令（生命周期 5 条 + 地址枚举 1 条 + 审批 4 条 + 配置 3 条）。
    pub(crate) const REMOTE_COMMANDS: [&str; 13] = [
        "start_remote_server",
        "stop_remote_server",
        "restart_remote_server",
        "check_tailscale_available",
        "get_tailscale_ip",
        "list_network_addresses",
        "approve_pairing",
        "reject_pairing",
        "list_paired_clients",
        "revoke_client",
        "get_remote_config",
        "set_remote_config",
        "request_bind_scope_change",
    ];

    /// 读 `lib.rs` 源码，逐名收集 `generate_handler![...]` 内的标识符。
    ///
    /// 不走编译期反射：这里要验证的恰恰是「源码里有没有登记」，读源码才是
    /// 真正的判据。任何一处旁路（写了命令但忘了进 handler）都会在这里露出来。
    pub(crate) fn registered_command_names() -> BTreeSet<String> {
        let lib_rs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let source = std::fs::read_to_string(&lib_rs)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", lib_rs.display()));

        let start = source
            .find("generate_handler![")
            .unwrap_or_else(|| panic!("no generate_handler! macro found in {}", lib_rs.display()));
        let body_start = start + "generate_handler![".len();
        let end = source[body_start..]
            .find(']')
            .map(|i| body_start + i)
            .unwrap_or_else(|| panic!("unterminated generate_handler! in {}", lib_rs.display()));

        source[body_start..end]
            .lines()
            // 先逐行去掉行注释（注释里也有逗号，不能按逗号盲切）。
            .map(|line| line.split("//").next().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(" ")
            .split(',')
            .filter_map(|entry| {
                // 登记项写成 `commands::foo`，只取最后一段路径分量作为命令名。
                let ident = entry.trim().rsplit("::").next().unwrap_or_default().trim();
                if ident.is_empty() {
                    None
                } else {
                    Some(ident.to_string())
                }
            })
            .collect()
    }

    #[test]
    fn generate_handler_registers_every_remote_command() {
        let expected: BTreeSet<String> = REMOTE_COMMANDS.iter().map(|s| s.to_string()).collect();
        let registered = registered_command_names();

        // 集合相等（逐名），不接受「至少 N 条」这种弱断言。
        let missing: Vec<&String> = expected.difference(&registered).collect();
        assert!(
            missing.is_empty(),
            "generate_handler! is missing remote commands: {missing:?}"
        );

        // 反向：收集到的每一项都必须是合法标识符，否则上面的比较建立在垃圾数据上。
        for name in &registered {
            assert!(
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "unexpected non-identifier entry collected from generate_handler!: {name:?}"
            );
        }

        assert_eq!(
            expected.intersection(&registered).count(),
            REMOTE_COMMANDS.len()
        );
    }

    #[test]
    fn existing_five_commands_are_registered() {
        let existing_five = [
            "start_remote_server",
            "stop_remote_server",
            "restart_remote_server",
            "check_tailscale_available",
            "get_tailscale_ip",
        ];
        let registered = registered_command_names();

        for name in existing_five {
            assert!(
                registered.contains(name),
                "pre-existing command {name} must remain registered in generate_handler!"
            );
        }
    }

    /// 批准命令必须在**后端重算**证据，不能把 `None` 传给 `approve`。
    ///
    /// 前端送来的证据是快照，谁都能伪造；重算一次（`tailscale whois` + `status`）
    /// 才拿得到没人能代填的结论。传 `None` 等于让这条链路上最该有判据的地方
    /// 恰好是空的。
    #[tokio::test]
    async fn approve_pairing_recomputes_evidence_on_the_backend() {
        use crate::remote::pairing::PairingStore;

        let store = PairingStore::new();
        let ticket = store
            .request_from_peer(
                "100.64.0.9".parse().expect("valid peer ip"),
                "cli",
                &["read"],
                Instant::now(),
            )
            .expect("配对请求必须放行");

        let decision = approve_bound(&store, &ticket.request_id, &ticket.user_code, true, false)
            .await
            .expect("后端重算证据后必须能批准");

        assert!(
            decision.is_some(),
            "批准必须带回后端重算出来的决策，不能是「没有证据」的空洞 None"
        );

        // 配对码必须与 request_id 成对：错配一律折叠成同一个出口。
        let other = store.request("other-cli", &[], Instant::now());
        assert_eq!(
            approve_bound(&store, &other.request_id, &ticket.user_code, true, false).await,
            Err(DENIED.to_string()),
            "配对码不属于这条请求时必须拒绝"
        );
        assert_eq!(
            approve_bound(&store, "no-such-request", &ticket.user_code, true, false).await,
            Err(DENIED.to_string())
        );
    }

    /// 命令层不得替用户「默认同意」：重算出的证据核不出来时，没勾确认就不能批。
    ///
    /// 这条钉的是**接线**：后端自己重算证据，它看到的可能与界面上看到的那一份
    /// 不同（用户在界面上看到的是 `Tailnet`，重算时 `whois` 超时了）。这种时候
    /// 命令层若自己把确认当成「已经有了」，就等于给了一条从未被确认过的授权开
    /// 后门。失败之后请求必须仍待审，用户重勾一次即可。
    #[tokio::test]
    async fn approve_bound_refuses_to_skip_the_degraded_confirmation() {
        use crate::remote::pairing::{PairingStore, PollOutcome};

        let store = PairingStore::new();
        let now = Instant::now();
        let ticket = store
            .request_from_peer(
                "198.51.100.7".parse().expect("valid peer ip"),
                "cli",
                &["read"],
                now,
            )
            .expect("配对请求必须放行");

        assert_eq!(
            approve_bound(&store, &ticket.request_id, &ticket.user_code, false, false).await,
            Err(UNCONFIRMED.to_string()),
            "重算出「来源核不出来」时，未确认的批准必须失败"
        );
        assert!(
            matches!(store.poll(&ticket.poll_token, now), PollOutcome::Pending),
            "这次失败不得把待审请求消费掉"
        );

        assert!(
            approve_bound(&store, &ticket.request_id, &ticket.user_code, true, false)
                .await
                .expect("勾了确认就必须放行")
                .is_some(),
            "确认之后必须带回决策，而不是空洞的 None"
        );
    }

    /// `approve_bound(` 的实参表，只取调用点上的那几个字。
    ///
    /// 刻意把**签名**排除在外：`confirmed_foreign_account` 作为形参出现在签名里，
    /// 于是一句「这个标识符在函数体里出现过吗」的朴素检查，在调用点被换成字面量
    /// `true` 时照样为真 —— 那正是这道锁要抓的改动。
    fn approve_pairing_bound_args(src: &str) -> String {
        let body = src
            .split("pub async fn approve_pairing(")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn reject_pairing(").next())
            .expect("approve_pairing body must be delimited by reject_pairing");

        body.split("approve_bound(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("approve_pairing must call approve_bound(")
            .to_string()
    }

    /// 结构锁：`approve_pairing` 必须把外来账号确认**原样递下去**。
    ///
    /// 外来账号的默认拒绝有两道门：界面上的勾，和状态机里 `record.foreign_account`
    /// 对 `ack_foreign` 的核对（[`crate::remote::pairing::PairingStore::approve`]）。
    /// 第二道才是防线 —— 第一道只管渲染层，任何绕开它的调用都摸不到。
    ///
    /// 所以这条钉的是**接线**：`approve_pairing` 若把这个标志位换成字面量（无论
    /// `true` 还是 `false`），状态机那道门就再也收不到真实意图了。写成 `true` 时
    /// 更糟：外来账号从此默认放行 —— 本命令要守住的正是「外来账号默认拒绝」。
    ///
    /// 用结构锁而不是行为锁，是因为命令层要 `AppHandle` + 在跑的远程服务；而且判定
    /// 「是不是外来账号」要真去跑 `tailscale whois` 与 `status` 比对，测试里给不出
    /// 一条外来账号。这与紧挨着的 `approve_pairing_does_not_pass_a_none_evidence`
    /// 是同一类锁，量的是同一件事的两头：那边管证据，这边管确认。
    #[test]
    fn approve_pairing_forwards_the_foreign_acknowledgement() {
        let args = approve_pairing_bound_args(include_str!("remote.rs"));

        assert!(
            args.contains("confirmed_foreign_account"),
            "approve_pairing 必须把外来账号确认原样传给 approve_bound，不能换成一个 \
             字面量：调用点写死会让状态机那道默认拒绝失效（写死 true 等于外来账号 \
             默认放行）。实际实参：\n{args}"
        );
    }

    /// 反向夹具：探测器必须量的是**调用点**，不是签名。
    ///
    /// 没有这条，上面那条锁就可能因为形参同名而恒绿 —— 一个永远抓不到东西的锁，
    /// 比没有锁更糟。
    #[test]
    fn forwarded_ack_detector_ignores_the_signature_parameter() {
        let fixture = "pub async fn approve_pairing(\n    \
             confirmed_foreign_account: bool,\n) {\n    \
             approve_bound(&store, true);\n}\n\
             pub async fn reject_pairing(";

        assert!(
            !approve_pairing_bound_args(fixture).contains("confirmed_foreign_account"),
            "探测器把签名里的形参当成了接线：调用点写死字面量时它仍报已转发"
        );
        assert!(
            approve_pairing_bound_args(
                "pub async fn approve_pairing() {
                    approve_bound(&store, confirmed_foreign_account);
                 }
                 pub async fn reject_pairing(",
            )
            .contains("confirmed_foreign_account"),
            "探测器连真正的转发都认不出来了"
        );
    }

    /// 结构锁：`approve_pairing` 不得把证据形参写成 `None`。
    ///
    /// `approve_bound` 的行为锁管住了「重算」这件事，这条管住**接线**：有人日后
    /// 把命令层改回直接调 `approve(.., None, ..)` 时，这里先红。
    #[test]
    fn approve_pairing_does_not_pass_a_none_evidence() {
        let source = include_str!("remote.rs");
        let body = source
            .split("pub async fn approve_pairing(")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn reject_pairing(").next())
            .expect("approve_pairing body must be delimited by reject_pairing");

        assert!(
            !body.contains(", None,"),
            "approve_pairing 不得再把证据形参传成 None：\n{body}"
        );
        assert!(
            body.contains("approve_bound("),
            "approve_pairing 必须走 approve_bound（内部重算证据）：\n{body}"
        );
    }

    // --- 暴露边界的行为级闸门 ---------------------------------------------
    //
    // 这一族原先挂在 Tauri IPC 上验：app 由 `tauri::test::mock_builder` 构建，载荷
    // 走 `get_ipc_response` 的真实反序列化。那条路要求 `tauri` 的 `test` feature，
    // 也就是 `[dev-dependencies]` 里多出一条 crate —— 而本仓的判据是依赖树
    // 零新增。
    //
    // 于是判定下沉到 [`apply_exposure_config`]：命令层退化成「找到闸门 + 转发」的
    // 薄壳，实参收窄的判定、校验、落盘全在这一层之内，不需要任何 Tauri runtime。
    // 保留的是本族真正要验的那条负向断言 —— `{bind_scope:"allInterfaces"}` 的载荷既改不了
    // 生效中的绑定范围，也不会把越权夹带的字段写进 `exposure.json`。
    //
    // 让出去的是两样：「原生弹窗事件确实经 IPC 回到了 Rust 侧」那段只剩结构锁
    // （下面的 `bind_scope_command_wires_the_native_confirmation_event`），以及
    // 「测试 handler 登记齐全」那一条（由本模块既有的
    // `generate_handler_registers_every_remote_command` 覆盖）。

    /// 落盘文件名。生产侧是 `remote::config` 里的私有常量，这里照抄一份：断言要能
    /// 独立指出「磁盘上那个文件」，而不是跟着生产常量一起变。
    const EXPOSURE_FILE: &str = "exposure.json";

    /// 一份隔离的凭据目录，外加「远程服务没起时」命令层拿到的那扇门。
    ///
    /// 生产上 `set_remote_config` 在服务未启动时会 `BindScopeGate::load(auth_config_dir())`
    /// 重新读盘，因此这里每次都重新 `load`：读回值来自磁盘，不是内存里那份残留。
    struct Exposure {
        dir: tempfile::TempDir,
    }

    impl Exposure {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().expect("create isolated credential dir"),
            }
        }

        fn gate(&self) -> BindScopeGate {
            BindScopeGate::load(self.dir.path())
        }

        /// 线上格式的载荷 → `RemoteConfig`，走 serde 反序列化。IPC 层把
        /// `InvokeBody::Json` 变成实参走的也是这一步，不是另一条路。
        fn payload(&self, raw: serde_json::Value) -> RemoteConfig {
            serde_json::from_value(raw)
                .expect("payload must deserialize into RemoteConfig（线上格式）")
        }

        fn bytes(&self) -> Option<Vec<u8>> {
            std::fs::read(self.dir.path().join(EXPOSURE_FILE)).ok()
        }
    }

    /// 对抗载荷：一次调用里同时夹带「放宽」意图的每一个字段。
    ///
    /// `bindScope` / `bindAddress` / `corsOrigins` 三项都属于越权覆盖：
    /// 前者是本命令明令不接的绑定范围，后两者在这份契约里根本不存在。
    fn escalation_payload() -> serde_json::Value {
        json!({
            "enabled": true,
            "port": 4000,
            "tailscaleEnabled": true,
            "bindScope": "allInterfaces",
            "corsMode": "explicitWhitelist",
            "originWhitelist": ["*"],
            "cors_origins": ["*"],
            "bind_address": "0.0.0.0"
        })
    }

    /// 渲染层直接调 `set_remote_config({bindScope:"allInterfaces"})` 之后，配置必须没变、
    /// 也没落盘。
    #[test]
    fn set_remote_config_cannot_widen_the_exposure_boundary() {
        let e = Exposure::new();
        assert_eq!(
            e.gate().read_remote_config().bind_scope,
            BindScope::Loopback,
            "起点必须是默认的仅回环"
        );

        // 先走一条**合法**的收紧调用把落盘文件建起来。判据③要比较的是「落盘前后
        // 逐字节一致」，基线必须是磁盘上一个真实存在、有真实内容的文件 —— 拿
        // `None` 当基线比出来的 `None == None` 是空断言，删掉落盘逻辑它照样绿。
        apply_exposure_config(
            &e.gate(),
            &e.payload(json!({
                "enabled": true,
                "port": 4000,
                "tailscaleEnabled": false,
                // `bind_scope` 是 `RemoteConfig` 的必填字段，反序列化要求它在场；
                // 「在场」不等于「被采纳」—— 这正是本闸门要验的那件事。
                "bindScope": "loopback",
                "corsMode": "explicitWhitelist",
                "originWhitelist": ["http://127.0.0.1:4000"]
            })),
        )
        .expect("收紧白名单必须被接受");
        let baseline = e
            .bytes()
            .expect("收紧之后必须落盘，否则后面的逐字节比较是空断言");

        // 载荷里的 `origin_whitelist: ["*"]` 是通配 origin，判据本身就该拒。
        let err = apply_exposure_config(&e.gate(), &e.payload(escalation_payload()))
            .expect_err("通配 origin 的白名单必须被拒");
        assert!(
            err.contains("origin whitelist entry rejected"),
            "拒绝理由必须来自白名单判据，而不是某条别的兜底：{err}"
        );

        // 判据①：读回来的绑定范围仍是默认值（仅回环）。
        assert_eq!(
            e.gate().read_remote_config().bind_scope,
            BindScope::Loopback,
            "set_remote_config 不得改绑定范围"
        );

        // 判据③：落盘文件一个字节都没动。
        assert_eq!(
            e.bytes().as_deref(),
            Some(baseline.as_slice()),
            "被拒的调用不得落盘：落盘前后的文件内容必须逐字节一致"
        );

        // 再走一条**会成功**的、同样夹带越权字段的调用：绑定范围照样不动。
        apply_exposure_config(
            &e.gate(),
            &e.payload(json!({
                "enabled": true,
                "port": 4000,
                "tailscaleEnabled": false,
                "corsMode": "explicitWhitelist",
                "originWhitelist": ["http://127.0.0.1:4000"],
                "bindScope": "allInterfaces",
                "bind_address": "0.0.0.0",
                "cors_origins": ["*"]
            })),
        )
        .expect("收紧白名单必须被接受");

        assert_eq!(
            e.gate().read_remote_config().bind_scope,
            BindScope::Loopback,
            "一次成功的收紧调用同样不得顺带把绑定范围放宽"
        );

        // 收紧写的是同一份配置。`bindScope` 必须**根本不出现在文件里**：
        // 字段在场（哪怕是 "loopback"）就意味着 `set_remote_config` 采纳了
        // 调用方夹带的档位，而显式档位还会让旧设置迁移不再生效。
        let raw = e.bytes().expect("收紧之后必须落盘，否则收紧也成了空操作");
        let persisted: serde_json::Value =
            serde_json::from_slice(&raw).expect("exposure.json must be JSON");
        assert!(
            persisted.get("bindScope").is_none(),
            "set_remote_config 不得把任何 bindScope 写进文件：{persisted}"
        );
        assert_eq!(
            e.gate().read_remote_config().bind_scope,
            BindScope::Loopback,
            "没显式选过档位时生效值仍然是收紧的默认值"
        );
        assert!(
            !String::from_utf8_lossy(&raw).contains("0.0.0.0"),
            "bind_address 不是这份契约里的字段，不得被写进落盘文件：{raw:?}"
        );
    }

    /// `set_remote_config` 只落 CORS 两项，绑定范围一个字节都不跟着走。
    ///
    /// 判据要能分辨两件事，而只看「`bind_scope` 没变」分辨不了：
    ///
    /// * 这条命令**真的执行了**（白名单确实被写进磁盘）；
    /// * 它**只**写了白名单，越权夹带的 `bind_scope: "allInterfaces"` 被原地丢弃。
    ///
    /// 所以基线与越权调用送的是**两份不同的白名单**：若落盘内容等于越权调用送的那份，
    /// 判定确实跑过；若等于基线那份，越权调用就是一次提前 return 的假绿。两者都不
    /// 成立时 `bindScope` 才真的是「被刻意忽略了」，而不是「整条调用没发生」。
    #[test]
    fn set_remote_config_persists_cors_but_drops_the_bind_scope() {
        let e = Exposure::new();

        // 起点：没有落盘文件，也没有内存态的越权。
        assert!(
            e.bytes().is_none(),
            "起点必须没有落盘文件，否则「未落盘」这条断言无从判断"
        );
        assert_eq!(
            e.gate().read_remote_config().bind_scope,
            BindScope::Loopback,
            "起点必须是默认的仅回环"
        );

        // --- 第一段：被白名单判据拒掉的越权调用，整份配置一个字节都不许落盘 ---
        let err = apply_exposure_config(&e.gate(), &e.payload(escalation_payload()))
            .expect_err("通配 origin 的白名单必须被拒");
        assert!(
            err.contains("origin whitelist entry rejected"),
            "拒绝理由必须来自白名单判据，而不是某条别的兜底：{err}"
        );
        assert!(e.bytes().is_none(), "被拒的调用不得留下任何落盘痕迹");
        assert_eq!(
            e.gate().read_remote_config().bind_scope,
            BindScope::Loopback,
            "被拒的调用不得改绑定范围"
        );

        // --- 第二段：一条**会成功**的调用，夹带的越权字段只多不少 ---
        //
        // 白名单故意与上面那份不同：落盘内容等于它，才能证明判定真的执行到了
        // `set_cors`，而不是在看到 `bind_scope` 之后就提前 return 了。
        let whitelist = ["http://127.0.0.1:4000", "https://desk.example.invalid"];
        apply_exposure_config(
            &e.gate(),
            &e.payload(json!({
                "enabled": true,
                "port": 4000,
                "tailscaleEnabled": false,
                "bindScope": "allInterfaces",
                "bind_address": "0.0.0.0",
                "cors_origins": ["*"],
                "corsMode": "explicitWhitelist",
                "originWhitelist": whitelist,
            })),
        )
        .expect("合法的显式白名单必须被接受");

        let raw = e
            .bytes()
            .expect("合法的收紧调用必须落盘，否则这条用例什么都没验到");
        let persisted: serde_json::Value =
            serde_json::from_slice(&raw).expect("exposure.json must be JSON");

        // 判据(c)：判定确实执行了 —— 送进去的白名单原样落盘。
        assert_eq!(
            persisted
                .get("corsMode")
                .and_then(serde_json::Value::as_str),
            Some("explicitWhitelist"),
            "set_remote_config 确实执行到了落盘这一步（否则下面几条都是空断言）：{persisted}"
        );
        let on_disk: Vec<String> = persisted
            .get("originWhitelist")
            .and_then(serde_json::Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| e.as_str().map(str::to_string))
                    .collect()
            })
            .expect("originWhitelist must be an array");
        assert_eq!(
            on_disk,
            whitelist.to_vec(),
            "送进去的白名单必须原样落盘：{persisted}"
        );

        // 判据(a)：越权夹带的绑定范围**连字段都不该出现**在磁盘上；
        // 读回接口给出的仍然是收紧的默认值（回环）。
        assert!(
            persisted.get("bindScope").is_none(),
            "set_remote_config 一律不得落盘 bind_scope：{persisted}"
        );
        assert_eq!(
            e.gate().read_remote_config().bind_scope,
            BindScope::Loopback,
            "set_remote_config 一律不得改生效中的绑定范围"
        );

        // 越权夹带的其余字段在这份契约里根本不存在，序列化时必须被静默丢弃。
        assert!(
            !String::from_utf8_lossy(&raw).contains("0.0.0.0"),
            "bind_address 不是这份契约里的字段，不得被写进落盘文件：{raw:?}"
        );
        assert!(
            !String::from_utf8_lossy(&raw).contains("cors_origins"),
            "cors_origins 不是这份契约里的字段，不得被写进落盘文件：{raw:?}"
        );
    }

    /// `BindScope` 恰好三档，且书写顺序就是「宽窄」顺序。
    ///
    /// 线上格式是反序列化的唯一入口：多一个变体 / 少一个变体 / 名字变了，
    /// 这里都先红。
    #[test]
    fn bind_scope_has_three_ordered_tiers() {
        assert_eq!(
            serde_json::from_value::<BindScope>(json!("loopback")).unwrap(),
            BindScope::Loopback
        );
        assert_eq!(
            serde_json::from_value::<BindScope>(json!("tailscale")).unwrap(),
            BindScope::Tailscale
        );
        assert_eq!(
            serde_json::from_value::<BindScope>(json!("allInterfaces")).unwrap(),
            BindScope::AllInterfaces
        );

        // 线上值必须与规格逐字一致（camelCase）。
        assert_eq!(
            serde_json::to_value(BindScope::AllInterfaces).unwrap(),
            json!("allInterfaces")
        );
        assert_eq!(
            serde_json::to_value(BindScope::Tailscale).unwrap(),
            json!("tailscale")
        );

        for loose in [
            "lan",
            "all",
            "any",
            "public",
            "0.0.0.0",
            "*",
            "AllInterfaces",
        ] {
            assert!(
                serde_json::from_value::<BindScope>(json!(loose)).is_err(),
                "BindScope 反序列化不得接受 {loose:?}"
            );
        }

        // 顺序即宽窄：变宽判定直接由枚举顺序给出。
        assert!(BindScope::Tailscale.widens_from(BindScope::Loopback));
        assert!(BindScope::AllInterfaces.widens_from(BindScope::Tailscale));
        assert!(!BindScope::Loopback.widens_from(BindScope::AllInterfaces));

        assert_eq!(BindScope::default(), BindScope::Loopback);
        assert_eq!(RemoteConfig::default().bind_scope, BindScope::Loopback);
    }

    /// 取 `request_bind_scope_change` 命令体本身（不含后面的弹窗辅助函数）。
    fn bind_scope_command_body(src: &str) -> String {
        src.split("pub async fn request_bind_scope_change")
            .nth(1)
            .and_then(|rest| {
                rest.split("async fn show_native_bind_scope_confirmation")
                    .next()
            })
            .expect("request_bind_scope_change must be delimited by its native-dialog helper")
            .to_string()
    }

    /// 申请 → 原生弹窗 → 确认；命令层不得自己把申请兑现掉。
    ///
    /// 两段式的**行为**在 `remote::config` 里有逐条断言（过期、猜错 id、重复兑现、
    /// 跨会话）。这里钉的是命令层的接线：
    ///
    /// - 渲染层能碰到的只有「申请」，第二段是 Rust 侧弹的原生对话框；
    /// - 弹窗返回 true 之前不得调用 gate 的确认入口；
    /// - 用户取消时必须撤销 pending，而不是让它悬着。
    fn bind_scope_only_registers(src: &str) -> Result<(), String> {
        let body = bind_scope_command_body(src);

        if !body.contains("gate.request_bind_scope_change(scope, SessionIdentity::LocalWindow)") {
            return Err(format!(
                "申请命令必须把范围登记到闸门上，且身份是本机窗口：\n{body}"
            ));
        }
        if body.contains("set_cors(") {
            return Err(format!("申请命令不得写 CORS：\n{body}"));
        }
        if body.contains("listen(") {
            return Err(format!(
                "不得再依赖渲染层可发的确认事件：第二段必须是原生对话框：\n{body}"
            ));
        }

        // 兑现只能发生在原生弹窗返回之后。
        let Some((before_dialog, after_dialog)) =
            body.split_once("show_native_bind_scope_confirmation(&app")
        else {
            return Err(format!(
                "request_bind_scope_change 必须弹原生确认框：\n{body}"
            ));
        };
        if before_dialog.contains("confirm_bind_scope_change") {
            return Err(format!(
                "申请命令在弹窗之前不得兑现这次申请：\n{before_dialog}"
            ));
        }
        if !after_dialog.contains("gate.confirm_bind_scope_change(") {
            return Err(format!("兑现必须发生在原生弹窗确认之后：\n{after_dialog}"));
        }
        if !after_dialog.contains("reject_bind_scope_change(") {
            return Err(format!(
                "弹窗被取消同样要撤销 pending，而不是让记录悬着：\n{after_dialog}"
            ));
        }
        if !after_dialog.contains("SessionIdentity::LocalWindow") {
            return Err(format!("确认 / 撤销必须用本机窗口身份：\n{after_dialog}"));
        }
        Ok(())
    }

    /// 第二段必须是**真的**原生对话框，而且不能阻塞 tokio worker。
    fn bind_scope_uses_the_native_dialog(src: &str) -> Result<(), String> {
        let body = bind_scope_command_body(src);
        if !body.contains("show_native_bind_scope_confirmation(&app, current, scope, port)") {
            return Err(format!("必须调用原生确认辅助函数：\n{body}"));
        }

        let helper = src
            .split("async fn show_native_bind_scope_confirmation")
            .nth(1)
            .and_then(|rest| rest.split("rx.await").next())
            .expect("native dialog helper must exist");
        if !helper.contains("tauri_plugin_dialog") {
            return Err("原生对话框必须走 tauri-plugin-dialog".to_string());
        }
        if !helper.contains(".dialog()") || !helper.contains(".show(") {
            return Err(format!("必须调用 app.dialog() 的非阻塞 show()：\n{helper}"));
        }
        if helper.contains("blocking_show") {
            return Err(
                "不得在 async 命令里用 blocking_show：它会占住 tokio worker 直到用户点按钮"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// `set_remote_config` 只能是转发。
    ///
    /// 这一族真正的判定住在 [`apply_exposure_config`] 里，命令层负责找闸门。命令层
    /// 若自己再写一份「取哪两个字段、怎么校验」，那就已经有两条判定路径了 —— 日后
    /// 改其中一条，另一条悄悄放行，而两边看上去都还对。
    fn set_remote_config_only_forwards(src: &str) -> Result<(), String> {
        let body = command_body(src, "set_remote_config", "request_bind_scope_change");

        if !body.contains("apply_exposure_config(&gate, &config)") {
            return Err(format!(
                "set_remote_config 必须把整份 config 原样转给 apply_exposure_config：\n{body}"
            ));
        }
        if body.contains("set_cors(") {
            return Err(format!("set_remote_config 不得自己再写一份判定：\n{body}"));
        }
        Ok(())
    }

    #[test]
    fn bind_scope_command_only_registers_and_never_cashes_in() {
        if let Err(reason) = bind_scope_only_registers(include_str!("remote.rs")) {
            panic!("{reason}");
        }
    }

    #[test]
    fn bind_scope_command_shows_a_native_dialog() {
        if let Err(reason) = bind_scope_uses_the_native_dialog(include_str!("remote.rs")) {
            panic!("{reason}");
        }
    }

    #[test]
    fn set_remote_config_forwards_to_apply_exposure_config() {
        if let Err(reason) = set_remote_config_only_forwards(include_str!("remote.rs")) {
            panic!("{reason}");
        }
    }

    /// 反向夹具：上面三条结构锁必须真的量得到东西。
    ///
    /// 读源码的锁天生有个致命失效模式 —— 切错地方就恒绿，而恒绿的锁比没有锁更
    /// 糟（它让人以为这一族钉住了）。所以每条锁都配一份「改成它该抓的样子」的
    /// 夹具：夹具必须报出失败，否则探测器已经失效。
    #[test]
    fn the_exposure_locks_catch_the_wiring_they_claim_to() {
        let src = include_str!("remote.rs");

        // 反向基线：没改动的源码，三条锁都通过。
        assert!(bind_scope_only_registers(src).is_ok());
        assert!(bind_scope_uses_the_native_dialog(src).is_ok());
        assert!(set_remote_config_only_forwards(src).is_ok());

        // ① 判定被内联回命令层 —— 出现了第二条判定路径。
        let inlined = src.replace(
            "apply_exposure_config(&gate, &config)",
            "gate.set_cors(config.cors_mode, &config.origin_whitelist)",
        );
        assert!(
            set_remote_config_only_forwards(&inlined).is_err(),
            "判定被内联回 set_remote_config 时，转发锁必须先红"
        );

        // ② 申请即兑现 —— 弹窗之前就把 pending 确认了，两段式塌成一段式。
        let eager = src.replace(
            "let request = gate.request_bind_scope_change(scope, SessionIdentity::LocalWindow)?;",
            "let request = gate.request_bind_scope_change(scope, SessionIdentity::LocalWindow)?;\n    \
             let _ = gate.confirm_bind_scope_change(\"x\", SessionIdentity::LocalWindow, Instant::now());",
        );
        assert!(
            bind_scope_only_registers(&eager).is_err(),
            "申请命令当场兑现时，两段式锁必须先红"
        );

        // ③ 取消路径被删掉 —— pending 会悬着。
        let no_reject = src.replace("gate.reject_bind_scope_change(", "gate.ignore_pending(");
        assert!(
            bind_scope_only_registers(&no_reject).is_err(),
            "取消不撤销 pending 时，锁必须先红"
        );

        // ④ 退回渲染层可发的确认事件。
        let listener = src.replace(
            "show_native_bind_scope_confirmation(&app, current, scope, port).await;",
            "app.listen(\"x\", |_| {});",
        );
        assert!(
            bind_scope_only_registers(&listener).is_err(),
            "退回事件通道时，锁必须先红"
        );

        // ⑤ 原生弹窗换成阻塞式 —— 会占住 tokio worker。
        let blocking = src.replace(".show(move |approved|", ".blocking_show(move |approved|");
        assert!(
            bind_scope_uses_the_native_dialog(&blocking).is_err(),
            "blocking_show 混进来时，弹窗锁必须先红"
        );
    }

    /// 读 `remote.rs` 源码，取两个命令之间的那截函数体。
    ///
    /// 切分点只认 `pub async fn <name>`、不认后面的参数表：这族命令多带一个
    /// `<R: Runtime>`，按带括号的整串切会在泛型化之后静默找不到（`nth(1)` 返回
    /// `None`，直接变成一条读不懂的失败信息）。
    fn command_body(src: &str, name: &str, next: &str) -> String {
        src.split(&format!("pub async fn {name}"))
            .nth(1)
            .and_then(|rest| rest.split(&format!("pub async fn {next}")).next())
            .unwrap_or_else(|| panic!("{name} body must be delimited by {next}"))
            .to_string()
    }
}

//! Remote Management HTTP Server
//!
//! 轻量 Web 服务，通过手机/远程浏览器切换 provider。
//! 直接调用 ProviderService::switch()，确保 backfill 和状态一致性。
//! 彻底解决独立 Python remote 绕过桌面端状态管理导致的竞态覆盖问题。

pub(crate) mod auth;
pub(crate) mod config;
mod events;
mod handlers;
mod html;
pub(crate) mod redact;
// 配对子系统要向桌面端暴露命令（`approve` / `reject` / 设备管理），所以可见性
// 从 crate 内放宽到 `pub(crate)`；仍然是 crate 私有，不对外成 API。
pub(crate) mod pairing;
mod whois;

// 下面是整个 remote 子系统唯一的 E2E 夹具挂载点（不是 `lib.rs`、不是
// `src-tauri/tests/`）：`mod e2e;` 只在 test 配置下编译。四层授权规则的 12 格
// 逐格用例就住在 `src-tauri/src/remote/e2e.rs`，其模块文档里也逐字写回了本挂载点。
#[cfg(test)]
mod e2e;

use auth::{auth_layer, AuthScope, REMOTE_LISTENER_IS_TLS};
use axum::http::Method;
use axum::routing::{MethodFilter, MethodRouter};
use axum::Router;
use events::{AppHandleSink, EventSink};
use pairing::{PairingApiState, PairingStore};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio::task::JoinHandle;
use tower_http::cors::{Any, CorsLayer};

/// 服务绑在哪个范围上。
///
/// 三档，按 **可达面从窄到宽** 排列；`Ord` 的顺序就是「变宽 / 变窄」的判据
/// （见 [`Self::widens_from`]），所以变体的书写顺序本身是契约的一部分。
///
/// 放宽（往更宽的档位走）必须走两段式确认（见 [`config::BindScopeGate`]）；
/// 收紧随时可做。
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum BindScope {
    /// 只绑 `127.0.0.1`（本机可达）
    #[default]
    Loopback,
    /// `127.0.0.1` + Tailscale IP（tailnet 内可达，取不到 Tailscale IP 时退回回环）
    Tailscale,
    /// `0.0.0.0`（覆盖全部网卡，局域网与 tailnet 都可直接连）
    AllInterfaces,
}

impl BindScope {
    /// `self` 是否比 `current` 更宽。变宽需要第二段确认，变窄立即生效。
    ///
    /// 用枚举的 `Ord` 而不是另写一张 `match` 表：多一个变体时 `Ord` 自然把它
    /// 排进顺序里，而手写的表会静默漏掉新变体。
    pub fn widens_from(self, current: Self) -> bool {
        self > current
    }
}

/// 跨域策略。取值集合恰为两个，**不存在** `Any` / 通配变体 —— 放宽到任意
/// origin 这件事在本类型上不可构造，而不是靠调用方自觉不填。
///
/// [`CorsMode::ALL`] 是这个集合的唯一来源：枚举多一个变体，断言它长度的
/// 那个测试就先红。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CorsMode {
    /// 只接受与被访问服务同源的浏览器请求
    #[default]
    SameOrigin,
    /// 只接受 [`crate::remote::config`] 白名单里逐条列出的 origin
    ExplicitWhitelist,
}

impl CorsMode {
    /// 全部取值的集合。新增变体时这里和
    /// `config::tests::cors_mode_has_no_wildcard_variant` 会一起挡下来。
    pub const ALL: [CorsMode; 2] = [CorsMode::SameOrigin, CorsMode::ExplicitWhitelist];
}

/// Remote server 配置
///
/// `rename_all = "camelCase"` 是 **IPC 契约**，不是风格：前端 TS 接口按
/// camelCase 声明（`bindScope` / `tailscaleEnabled` / …）。漏掉它的时候字段会
/// 序列化成 `bind_scope`，前端读到 `undefined` —— 绑定范围单选框一个都不选中，
/// 点「仅本机」还会被判成「放宽」。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConfig {
    /// 是否启用远程管理服务
    pub enabled: bool,
    /// 监听端口
    pub port: u16,
    /// **遗留字段**，不再是绑定的权威源：绑定范围只看 [`Self::bind_scope`]。
    ///
    /// 保留它是为了 `start_remote_server` / `restart_remote_server` 的既有实参
    /// 形状与老 settings.json 的兼容；新代码不得据此决定监听地址。
    pub tailscale_enabled: bool,
    /// 绑定范围，默认回环。变宽只能经两段式确认变更。
    pub bind_scope: BindScope,
    /// 跨域策略，默认同源。
    pub cors_mode: CorsMode,
    /// 显式 Origin 白名单，仅 [`CorsMode::ExplicitWhitelist`] 下生效。
    pub origin_whitelist: Vec<String>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 4000,
            tailscale_enabled: false,
            bind_scope: BindScope::Loopback,
            cors_mode: CorsMode::SameOrigin,
            origin_whitelist: Vec::new(),
        }
    }
}

/// Tauri webview 的 origin：两个平台固有值，**不是**配置项。
///
/// 为什么它们必须显式在列：webview 里跑的正是那个要读响应的页面，而
/// `auth::policy` 的第②层判据对不在名单里的 Origin 一律 403 —— 漏掉这两条，
/// 桌面端自己的 UI 会落进第 11 格，表现是「打开就是白屏」。
///
/// 它们是常量而不是 [`RemoteConfig`] 的字段：它们描述的是「谁在读」，不是
/// 「用户允许了什么」，让用户去配它等于把一个非选择题伪装成选择题。
pub const TAURI_WEBVIEW_ORIGINS: [&str; 2] = ["tauri://localhost", "https://tauri.localhost"];

/// debug 构建下 Vite dev server 的 origin。
///
/// release 构建里不存在这个来源，因此用 `#[cfg]` 去掉而不是留着当一条永不
/// 命中的死条目 —— 后者会让人误以为「开发环境的白名单」在生产上也一样。
#[cfg(debug_assertions)]
pub const DEV_SERVER_ORIGIN: &str = "http://localhost:3000";

/// debug 构建下 Vite dev server 的 origin 集合。
///
/// 常量 [`DEV_SERVER_ORIGIN`] 写死 3000，但 Vite 的 `port` 没有配 `strictPort`：
/// 3000 被占用时它会自己换一个端口，webview 的 origin 随之改变。前端靠
/// `fetch` 打自己的 `/api/health` 判断服务起没起来（`RemoteSettings.tsx` 的
/// `checkServerStatus`），origin 不在名单里就拿到 403 —— 表现成「点了启用
/// 远程管理，报启动失败」，而服务其实已经绑上了。
///
/// `CC_SWITCH_DEV_ORIGIN` 让开发者显式声明实际端口；条目同样过
/// [`config::validate_whitelist`]（拒通配 / `null` / 非 http(s)）。
/// release 构建里整段不存在。
#[cfg(debug_assertions)]
fn dev_server_origins(extra: Option<&str>) -> Vec<String> {
    let mut origins = vec![DEV_SERVER_ORIGIN.to_string()];
    match extra {
        Some(extra)
            if config::validate_whitelist(std::slice::from_ref(&extra.to_string())).is_ok() =>
        {
            origins.push(extra.to_string());
        }
        Some(extra) => {
            log::warn!("[Remote] CC_SWITCH_DEV_ORIGIN 不是合法的 origin，已忽略: {extra:?}");
        }
        None => {}
    }
    origins
}

/// 本服务自身的 origin：`(scheme, 127.0.0.1, port)`。
///
/// 端口取 [`RemoteConfig::port`] 而不是实际绑定值：origin 是**浏览器**看到的
/// 串，`127.0.0.1:0` 起起来的临时端口上没有任何浏览器会来请求，把它写进
/// 白名单只会在文档里留一个假 origin。
fn self_origin(cfg: &RemoteConfig, tls: bool) -> String {
    format!("{}://127.0.0.1:{}", scheme_of(tls), cfg.port)
}

/// 当前 listener 的 scheme。
///
/// 与 [`auth::cookie::REMOTE_LISTENER_IS_TLS`] 同源：cookie 上的 `Secure` 与
/// CORS 白名单里的 scheme 说的是同一件事实（这个 listener 走不走 TLS），
/// 两处分岔就会得到「cookie 带了 Secure 但 origin 是 http」这种浏览器直接
/// 静默拒收的组合。
fn scheme_of(tls: bool) -> &'static str {
    if tls {
        "https"
    } else {
        "http"
    }
}

/// CORS `Access-Control-Allow-Origin` 的白名单。
///
/// **这是装配层唯一的 origin 来源**：[`build_router`] 只经它取
/// CORS 名单，测试与将来的其它装配点也必须经它 —— 两处各写一份清单，
/// 漏一条的后果是「内嵌 webview 被浏览器按跨源掐掉」，而这类故障只在
/// 生产构建里表现为白屏。
///
/// 名单由三部分拼成，来源各自独立、互不覆盖：
///
/// 1. **本服务自身**（`self_origin`）：服务自己发出去的页面的 origin。
/// 2. **内嵌 webview**（[`TAURI_WEBVIEW_ORIGINS`] + [`DEV_SERVER_ORIGIN`]）：
///    谁在读响应，不受用户配置影响。
/// 3. **Tailscale origin**：仅当 [`RemoteConfig::bind_scope`] 比 [`BindScope::Loopback`]
///    宽 —— 那两个档位下浏览器可以从 Tailscale 地址打开页面。
/// 4. **用户白名单**：仅当 [`CorsMode::ExplicitWhitelist`]，逐条过
///    [`config::validate_whitelist`] —— 与 `set_cors` 那道闸门**同一份判据**。
///
/// 三条硬约束：
///
/// - **没有通配**。`CorsLayer` 的 `allow_origin(Any)` 与白名单里的 `*` 是同一句
///   「任意 origin」，而 Origin 是攻击者完全可控的字段：放宽到任意 origin，
///   任意网页都读得到响应内容，等于把鉴权结果交给攻击者页面。
/// - **没有 `Origin: null`**。它不归属任何站点（sandbox iframe / `file:` /
///   `data:` 都发它），列进去等于放行整个 sandbox 面。
/// - **`RemoteConfig` 可以被直接构造**。`set_cors` 会拒毒条目，但命令层、测试
///   夹具、将来的迁移代码都能绕过那道闸门，所以本函数自己再过一遍。
pub fn allowed_origins(cfg: &RemoteConfig, tls: bool) -> Vec<axum::http::HeaderValue> {
    use axum::http::HeaderValue;

    let scheme = scheme_of(tls);
    let mut origins: Vec<String> = vec![self_origin(cfg, tls)];
    origins.extend(TAURI_WEBVIEW_ORIGINS.iter().map(|o| o.to_string()));
    #[cfg(debug_assertions)]
    {
        let extra = std::env::var("CC_SWITCH_DEV_ORIGIN").ok();
        origins.extend(dev_server_origins(extra.as_deref()));
    }

    if cfg.bind_scope != BindScope::Loopback {
        if let Some(ip) = get_tailscale_ip() {
            origins.push(format!("{scheme}://{ip}:{}", cfg.port));
        }
    }

    if cfg.cors_mode == CorsMode::ExplicitWhitelist {
        origins.extend(
            cfg.origin_whitelist
                .iter()
                .filter(|o| config::validate_whitelist(std::slice::from_ref(o)).is_ok())
                .cloned(),
        );
    }

    origins
        .into_iter()
        // 装不进 header 的串（内嵌 CR/LF 等）直接丢：一份读不出内容的名单
        // 塞给 `CorsLayer` 只会 panic，而 panic 发生在起服务的路径上。
        .filter_map(|o| HeaderValue::from_str(o.trim()).ok())
        .collect()
}

/// Tauri 运行时句柄，可能缺席（headless）。
///
/// 为什么包一层而不是把字段改成 `Option<AppHandle>`：`handlers.rs` 有 12 处
/// 直接以 `state.app_handle.state::<AppState>()` / `&state.app_handle` 的形式
/// 使用它。靠 `Deref` 转发，这些调用点一行都不用改。
///
/// 生产构建下 `inner` 恒为 `Some`：`headless()` 带 `#[cfg(test)]`，非测试构建
/// 根本构造不出空句柄，所以「GUI 专属路由在 headless 下取句柄」这条路径是
/// 测试构建独有的。
pub struct RemoteApp {
    inner: Option<tauri::AppHandle>,
}

impl RemoteApp {
    /// 生产路径：总是持有真实运行时
    pub fn gui(app: tauri::AppHandle) -> Self {
        Self { inner: Some(app) }
    }

    /// 无 GUI 运行时的构造入口，仅测试构建可用（E2E 基座用它起服务）
    #[cfg(test)]
    pub fn headless() -> Self {
        Self { inner: None }
    }
}

impl std::ops::Deref for RemoteApp {
    type Target = tauri::AppHandle;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().expect(
            "headless remote server: this route needs the Tauri runtime \
             (managed AppState / tray); it is unavailable without an AppHandle",
        )
    }
}

impl Clone for RemoteApp {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Remote server 共享状态
///
/// 监听地址表与生效配置挂在 **state** 上而不是 [`RemoteServer`] 上：两段式
/// 确认的 REST handler 只拿得到 `Arc<RemoteState>`，而「确认之后立刻重绑」
/// 需要读写这两份东西。server 因此退化成一层很薄的包装。
pub struct RemoteState {
    /// Tauri AppHandle，用于访问 managed AppState 和刷新托盘。
    /// headless 构造下缺席，此时依赖 GUI 的路由不可用。
    pub app_handle: RemoteApp,
    /// 事件出口：把审批事件/前端通知发出去，可在测试中替换
    pub sink: Arc<dyn EventSink>,
    /// SSE 广播通道发送端
    pub sse_tx: broadcast::Sender<String>,
    /// 凭据落盘目录：配对签发与 `auth_layer` 验凭据读的是同一个。
    ///
    /// 显式随 server 走而不是各自去读全局：测试夹具因此能指向自己的临时目录，
    /// 生产端则是 `~/.cc-switch`。
    pub config_dir: PathBuf,
    /// 运行状态标志：stop() 后设为 false，拒绝新请求
    pub running: std::sync::atomic::AtomicBool,
    /// 配对子系统的独立状态：不持有 AppHandle，因此能与 GUI 生命周期脱钩。
    ///
    /// 桌面端的审批命令要读写同一个 store（见 `commands::remote`），故对 crate 内
    /// 可见 —— 与 server 另起一个 store 会让「批了」落不到 HTTP 路由那边。
    pub(crate) pairing: PairingApiState,
    /// 对外暴露边界（绑定范围 / CORS / 白名单）。同样对 crate 内可见 —— 桌面端的
    /// `get/set_remote_config` 与两段式确认读写的是同一个闸门。
    pub(crate) exposure: config::BindScopeGate,
    /// 按地址独立管理的 listener：支持动态增删，绑定范围变化时只动变的地址。
    listeners: RwLock<HashMap<SocketAddr, ListenerEntry>>,
    /// 当前生效配置。**只有** [`RemoteState::update`] 会写它，且写之前所有
    /// 新 listener 都已经用新配置装配好了 router。
    config: RwLock<RemoteConfig>,
}

struct ListenerEntry {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

/// Remote 管理服务器
///
/// 薄包装：所有状态都在 [`RemoteState`] 里，见那里的文档。
pub struct RemoteServer {
    state: Arc<RemoteState>,
}

/// 根据配置计算目标监听地址列表。
///
/// 档位与地址的对应关系是产品契约（不是实现细节）：
///
/// | `BindScope` | 监听地址 |
/// |---|---|
/// | `Loopback` | `127.0.0.1:{port}` |
/// | `Tailscale` | `127.0.0.1:{port}` + `{tailscale_ip}:{port}` |
/// | `AllInterfaces` | `0.0.0.0:{port}` |
///
/// Tailscale 档位拿不到 Tailscale IP 时退回 loopback 并**记日志**：静默退回
/// 会让用户以为「已经对 tailnet 开放」，而实际只有一个回环 listener。
pub(crate) fn compute_target_addrs(config: &RemoteConfig) -> Vec<SocketAddr> {
    compute_target_addrs_with(config, get_tailscale_ip().as_deref())
}

/// [`compute_target_addrs`] 的可注入版本：Tailscale IP 由调用方给。
///
/// 拆出来是为了让「拿得到 / 拿不到 Tailscale IP」两条分支都能被单元测试覆盖，
/// 而不必依赖这台机器上 `tailscale ip -4` 的当前输出。
pub(crate) fn compute_target_addrs_with(
    config: &RemoteConfig,
    tailscale_ip: Option<&str>,
) -> Vec<SocketAddr> {
    let loopback = SocketAddr::from(([127, 0, 0, 1], config.port));

    match config.bind_scope {
        BindScope::Loopback => vec![loopback],
        BindScope::Tailscale => match tailscale_ip {
            Some(ip) => match format!("{ip}:{}", config.port).parse() {
                Ok(addr) => vec![loopback, addr],
                Err(e) => {
                    log::warn!(
                        "[Remote] bind scope is Tailscale but {ip:?} is not a usable address \
                         ({e}); falling back to loopback only"
                    );
                    vec![loopback]
                }
            },
            None => {
                log::warn!(
                    "[Remote] bind scope is Tailscale but no Tailscale IP is available; \
                     falling back to loopback only"
                );
                vec![loopback]
            }
        },
        BindScope::AllInterfaces => vec![SocketAddr::from(([0, 0, 0, 0], config.port))],
    }
}

/// 某个档位会新增的可达地址（供原生弹窗列出「将要对哪些地址开放」）。
///
/// 回环地址是每一档都有的，这里省略；`AllInterfaces` 追加一条通配说明 ——
/// 逐个枚举本机网卡既不完整（网卡随时增减），也会把「全部网卡」说小。
pub fn reachable_addresses(scope: BindScope, port: u16) -> Vec<String> {
    match scope {
        BindScope::Loopback => Vec::new(),
        BindScope::Tailscale => get_tailscale_ip()
            .map(|ip| vec![format!("http://{ip}:{port}")])
            .unwrap_or_else(|| vec!["Tailscale IP unavailable".to_string()]),
        BindScope::AllInterfaces => {
            let mut out = vec![format!("0.0.0.0:{port} (all interfaces)")];
            if let Some(ip) = get_tailscale_ip() {
                out.push(format!("http://{ip}:{port}"));
            }
            out
        }
    }
}

/// 把一个已经摘出 map 的 listener 交给后台收尾。
///
/// 发送 shutdown 之后最多等 3 秒；到点直接 abort。调用方（[`RemoteState::update`]）
/// 必须**不**等待这个任务：更新请求本身可能就跑在这个 listener 上。
fn retire_listener(addr: SocketAddr, entry: ListenerEntry) {
    let _ = entry.shutdown_tx.send(());
    tokio::spawn(async move {
        let abort_handle = entry.handle.abort_handle();
        match tokio::time::timeout(std::time::Duration::from_secs(3), entry.handle).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("[Remote] Listener task on {addr} panicked: {e}"),
            Err(_) => {
                log::warn!("[Remote] Listener on {addr} stop timeout, aborting");
                abort_handle.abort();
            }
        }
        log::info!("[Remote] Stopped listening on {addr}");
    });
}

impl RemoteState {
    /// 当前生效配置的快照。
    pub(crate) async fn config_snapshot(&self) -> RemoteConfig {
        self.config.read().await.clone()
    }

    /// 启动所有配置中的地址（首次启动用）
    async fn start(self: &Arc<Self>) -> Result<Vec<String>, String> {
        let config = self.config_snapshot().await;
        let addrs = compute_target_addrs(&config);
        let urls = self.start_addrs(addrs, &config).await;

        if urls.is_empty() {
            return Err("Failed to bind any address".to_string());
        }

        Ok(urls)
    }

    /// 增量更新：根据新配置增删 listener，不中断保留的地址。
    ///
    /// **顺序是承重的**：新 listener 的 router 必须用 `new_config` 装配（显式传参，
    /// 见 [`build_router`]），不能让它去读 `self.config` —— 旧顺序先 `start_addrs`
    /// 再写 `self.config`，导致新地址上的 router 仍然是旧配置（CORS 名单、Origin
    /// 判据全部滞后一次变更）。这里先停旧地址、再用新配置起新地址、最后提交配置。
    async fn update(self: &Arc<Self>, new_config: RemoteConfig) -> Result<Vec<String>, String> {
        let old_config = self.config_snapshot().await;
        let current_addrs: Vec<SocketAddr> = self.listeners.read().await.keys().cloned().collect();
        let target_addrs = compute_target_addrs(&new_config);

        let to_stop: Vec<SocketAddr> = current_addrs
            .iter()
            .filter(|a| !target_addrs.contains(a))
            .cloned()
            .collect();
        let to_start: Vec<SocketAddr> = target_addrs
            .iter()
            .filter(|a| !current_addrs.contains(a))
            .cloned()
            .collect();

        // 先摘掉旧 listener，但**不在这里等它们收尾**。
        //
        // 这一条是承重的：重绑请求本身就跑在其中一个 listener 上。优雅关闭要等
        // 现有连接结束，而那发连接正在等本函数返回 —— 互相等待，最后只能靠 3s
        // 超时 abort 收场（表现为「改一次档位卡三秒」，甚至把响应连根掐掉）。
        // 所以这里只把 map 摘干净、把 shutdown 发出去，等待与终止交给后台任务。
        let mut retired: Vec<(SocketAddr, ListenerEntry)> = Vec::new();
        {
            let mut listeners = self.listeners.write().await;
            for addr in &to_stop {
                if let Some(entry) = listeners.remove(addr) {
                    retired.push((*addr, entry));
                }
            }
        }
        for (addr, entry) in retired {
            retire_listener(addr, entry);
        }

        // 保留的地址：listener 与 router 都不动。
        let mut urls: Vec<String> = target_addrs
            .iter()
            .filter(|a| current_addrs.contains(a))
            .map(|a| format!("http://{a}"))
            .collect();

        // 启动新地址，router 显式用新配置装配。
        if !to_start.is_empty() {
            urls.extend(self.start_addrs(to_start.clone(), &new_config).await);
        }

        // 每个目标地址都必须真的绑上了才提交配置。否则「配置说 0.0.0.0，实际
        // 一个 listener 都没有」会让下一次重启前的整个窗口内服务静默失联。
        let missing: Vec<SocketAddr> = {
            let listeners = self.listeners.read().await;
            target_addrs
                .iter()
                .filter(|addr| !listeners.contains_key(addr))
                .copied()
                .collect()
        };
        if !missing.is_empty() {
            log::error!(
                "[Remote] could not bind target addresses {missing:?}; rolling back to {old_config:?}"
            );
            // 撤掉这次新起的 listener（不等收尾，同上）。
            let mut rolled_back: Vec<(SocketAddr, ListenerEntry)> = Vec::new();
            {
                let mut listeners = self.listeners.write().await;
                for addr in &to_start {
                    if let Some(entry) = listeners.remove(addr) {
                        rolled_back.push((*addr, entry));
                    }
                }
            }
            for (addr, entry) in rolled_back {
                retire_listener(addr, entry);
            }
            // 把旧地址按旧配置起回来。失败也无可奈何，但至少配置没有前进。
            let restored = self.start_addrs(to_stop, &old_config).await;
            return Err(format!(
                "bind scope update could not bind {missing:?}; restored old listeners {restored:?}"
            ));
        }

        *self.config.write().await = new_config;

        Ok(urls)
    }

    /// 把生效的绑定范围改到 `scope` 并**立刻重绑 listener**。
    ///
    /// 只应由两段式确认的确认 / 收紧路径调用：闸门负责持久化与鉴权，这里只负责
    /// 「已经在跑的 listener 跟上」。
    pub(crate) async fn rebind_bind_scope(
        self: &Arc<Self>,
        scope: BindScope,
    ) -> Result<(), String> {
        let mut config = self.config_snapshot().await;
        if config.bind_scope == scope {
            return Ok(());
        }
        config.bind_scope = scope;
        self.update(config).await.map(|_| ())
    }

    /// 启动指定地址的 listener（如果已在运行则跳过）。
    ///
    /// `cfg` 必须与这批 listener 应生效的配置一致；调用方显式传入而不是在这里
    /// 读 `self.config`，见 [`RemoteState::update`] 的顺序说明。
    async fn start_addrs(
        self: &Arc<Self>,
        addrs: Vec<SocketAddr>,
        cfg: &RemoteConfig,
    ) -> Vec<String> {
        let app = build_router(self, cfg);
        let mut started_urls = Vec::new();
        let mut listeners = self.listeners.write().await;

        for addr in addrs {
            if listeners.contains_key(&addr) {
                started_urls.push(format!("http://{addr}"));
                continue;
            }

            // 旧 listener 刚被摘掉时端口可能还没释放（收尾在后台）。这里对
            // `AddrInUse` 做有界重试（10 × 50ms），而不是把一次成功的档位切换
            // 记成「绑不上」。
            let bound = {
                let mut attempt = 0u8;
                loop {
                    match tokio::net::TcpListener::bind(addr).await {
                        Ok(listener) => break Some(listener),
                        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && attempt < 10 => {
                            attempt += 1;
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                        Err(e) => {
                            log::warn!("[Remote] Failed to bind {addr}: {e}");
                            break None;
                        }
                    }
                }
            };

            if let Some(listener) = bound {
                // 端口 0 时内核会分配真实端口，必须以 listener 实际绑定的
                // 地址对外报 URL，否则会把 ":0" 泄漏给调用方。
                let bound = listener.local_addr().unwrap_or(addr);
                let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

                let app_clone = app.clone();
                let handle = tokio::spawn(async move {
                    // `into_make_service_with_connect_info`：配对限流按来源
                    // peer IP 计，缺了它 handler 拿不到 `ConnectInfo`。
                    let serve = axum::serve(
                        listener,
                        app_clone.into_make_service_with_connect_info::<SocketAddr>(),
                    );
                    let graceful = serve.with_graceful_shutdown(async move {
                        let _ = shutdown_rx.await;
                    });
                    if let Err(e) = graceful.await {
                        log::warn!("[Remote] Server error on {addr}: {e}");
                    }
                });

                listeners.insert(
                    addr,
                    ListenerEntry {
                        shutdown_tx,
                        handle,
                    },
                );
                started_urls.push(format!("http://{bound}"));
                log::info!("[Remote] Listening on {bound}");
            }
        }

        started_urls
    }

    /// 停止指定地址的 listener
    async fn stop_addrs(&self, addrs: Vec<SocketAddr>) {
        let mut listeners = self.listeners.write().await;

        for addr in addrs {
            if let Some(entry) = listeners.remove(&addr) {
                let _ = entry.shutdown_tx.send(());
                let abort_handle = entry.handle.abort_handle();

                match tokio::time::timeout(std::time::Duration::from_secs(3), entry.handle).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => log::warn!("[Remote] Listener task on {addr} panicked: {e}"),
                    Err(_) => {
                        log::warn!("[Remote] Listener on {addr} stop timeout, aborting");
                        abort_handle.abort();
                    }
                }

                log::info!("[Remote] Stopped listening on {addr}");
            }
        }
    }

    async fn stop(&self) -> Result<(), String> {
        // 先标记为 stopped，拒绝已建立连接上的新请求
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        log::info!("[Remote] Running flag set to false");

        let addrs: Vec<SocketAddr> = self.listeners.read().await.keys().cloned().collect();
        self.stop_addrs(addrs).await;

        log::info!("[Remote] Server stopped");
        Ok(())
    }
}

impl RemoteServer {
    /// 构造 server。
    ///
    /// `config_dir` 是凭据落盘目录，必须与 `auth_layer` 读的是同一个：配对在
    /// 这里签发的摘要，中间件要能在那里读到，反之亦然。传错目录的后果不是
    /// 「配对不生效」，而是**签发出去一把中间件永远不认的死 token**。
    ///
    /// 顺带在这里建 `remote-auth/secret`（0600）：签发摘要要用它，吊销时反查
    /// 也要用。凭据文件过宽（0644）时这里就报错，远程服务拒绝带着一个别人
    /// 也读得到的 secret 起来。
    fn new(
        app_handle: RemoteApp,
        sink: Arc<dyn EventSink>,
        config: RemoteConfig,
        config_dir: PathBuf,
    ) -> Result<Self, String> {
        Self::new_with_exposure(app_handle, sink, config, config_dir, None)
    }

    /// [`RemoteServer::new`] 的可注入版本：TTL 可缩短（仅测试构建使用）。
    ///
    /// 过期验证不能真的 sleep 五分钟 —— 可注入的 TTL 让 E2E 用几百毫秒就能
    /// 验到同一段判据。生产入口不暴露这个参数。
    fn new_with_exposure(
        app_handle: RemoteApp,
        sink: Arc<dyn EventSink>,
        config: RemoteConfig,
        config_dir: PathBuf,
        ttl: Option<std::time::Duration>,
    ) -> Result<Self, String> {
        let (sse_tx, _) = broadcast::channel(16);
        let secret = auth::load_or_create(&config_dir).map_err(|e| e.to_string())?;
        let exposure = match ttl {
            Some(ttl) => config::BindScopeGate::load_with_ttl(config_dir.clone(), ttl),
            None => config::BindScopeGate::load(config_dir.clone()),
        };

        let state = Arc::new(RemoteState {
            sink: sink.clone(),
            app_handle,
            sse_tx,
            config_dir: config_dir.clone(),
            running: std::sync::atomic::AtomicBool::new(true),
            // 审批事件与其它前端事件共用同一个出口：桌面端只需订阅一处。
            pairing: PairingApiState {
                // 必须与下面 `secret` 是同一把：状态机交给撤销的那份摘要要拿去
                // `clients.json` 里逐条 `ct_eq` 比对，而那份库正是用这把 secret
                // 写的。状态机自己另生成一把的话，撤销会受理成功却一条没摘。
                store: PairingStore::with_secret(secret),
                sink,
                config_dir: config_dir.clone(),
                secret,
            },
            exposure,
            listeners: RwLock::new(HashMap::new()),
            config: RwLock::new(config),
        });

        Ok(Self { state })
    }

    /// 基于给定 TTL 构造 server（仅测试构建）。见 [`Self::new_with_exposure`]。
    #[cfg(test)]
    pub(crate) fn new_with_ttl(
        app_handle: RemoteApp,
        sink: Arc<dyn EventSink>,
        config: RemoteConfig,
        config_dir: PathBuf,
        ttl: std::time::Duration,
    ) -> Result<Self, String> {
        Self::new_with_exposure(app_handle, sink, config, config_dir, Some(ttl))
    }

    /// 启动所有配置中的地址（首次启动用）
    async fn start(&self) -> Result<Vec<String>, String> {
        self.state.start().await
    }

    /// 增量更新：根据新配置增删 listener，不中断保留的地址
    async fn update(&self, new_config: RemoteConfig) -> Result<Vec<String>, String> {
        self.state.update(new_config).await
    }

    /// 启动指定地址的 listener（如果已在运行则跳过），配置取当前生效值。
    ///
    /// 只被 E2E 基座使用：生产启动走 [`RemoteState::start`]，那里地址由
    /// `compute_target_addrs` 算出。
    #[cfg(test)]
    async fn start_addrs(&self, addrs: Vec<SocketAddr>) -> Vec<String> {
        let cfg = self.state.config_snapshot().await;
        self.state.start_addrs(addrs, &cfg).await
    }

    async fn stop(&self) -> Result<(), String> {
        self.state.stop().await
    }

    pub(crate) fn state(&self) -> Arc<RemoteState> {
        self.state.clone()
    }

    /// 当前生效配置的快照。
    #[cfg(test)]
    pub(crate) async fn config_snapshot(&self) -> RemoteConfig {
        self.state.config_snapshot().await
    }
}

/// 业务路由的鉴权分区表：`(路径, 该路径的凭据通道)`。
///
/// 分区依据两条**互相独立**的事实，都不是「这条路由要不要保护」：
///
/// 1. 无凭据时是否必须可达 —— 只有配对端点与根页面是 `Public`。
/// 2. 载体在协议层有没有地方放 token —— `<img src>` / `EventSource` 放不了，
///    只能走会话 cookie，因此 `/api/icon`、`/api/provider-icons/:name`、
///    `/api/events` 是 `BrowserSafe`（`html.rs` 里这三处调用点为证）。
///    其余都是 `Protected`，接受 `Authorization: Bearer` **与会话 cookie 两条
///    通道**（顺序与语义见 `auth::middleware` 里的 `AuthScope::credential_channels`；
///    某个载体一旦在请求里出现，它就必须验过，不靠另一条通道回落过关）。
///
/// 三类 scope **全部**经过同一个 [`auth_layer`]；`BrowserSafe` 绝不等于跳过
/// 中间件，它只是换了一条凭据通道。
///
/// 配对端点（`/pairing/*`）是「拿到凭据之前」那一方的入口，属于配对子系统
/// 自带的一次性凭据，不在本表内、也不计入这 17 条。
pub const ROUTE_SCOPE_TABLE: &[(&str, AuthScope)] = &[
    ("/", AuthScope::Public),
    ("/api/providers", AuthScope::Protected),
    ("/api/usage", AuthScope::Protected),
    ("/api/switch", AuthScope::Protected),
    ("/api/reorder", AuthScope::Protected),
    ("/api/current", AuthScope::Protected),
    ("/api/events", AuthScope::BrowserSafe),
    ("/api/health", AuthScope::Protected),
    ("/api/icon", AuthScope::BrowserSafe),
    ("/api/provider-icons/:name", AuthScope::BrowserSafe),
    ("/api/proxy/control", AuthScope::Protected),
    ("/api/proxy/takeover", AuthScope::Protected),
    ("/api/proxy/failover", AuthScope::Protected),
    ("/api/provider/test", AuthScope::Protected),
    // 两段式绑定范围：申请 / 确认 / 拒绝。三条**全部**是 `Protected` ——
    // 放开绑定范围的决定权在 Rust 侧，而能申请这件事本身也要求先证明
    // 「你是谁」（非回环来源必须持有配对凭证）。
    (
        "/api/remote/config/bind-scope/request",
        AuthScope::Protected,
    ),
    (
        "/api/remote/config/bind-scope/confirm",
        AuthScope::Protected,
    ),
    ("/api/remote/config/bind-scope/reject", AuthScope::Protected),
];

/// Tauri managed state wrapper
pub struct ManagedRemoteServer(Arc<RwLock<Option<RemoteServer>>>);

/// 业务路由进入 Router 的**唯一**入口。
///
/// 裸 `.route("/…")` 一旦允许，某条新路由就能绕开 [`ROUTE_SCOPE_TABLE`]，
/// 也就绕开了鉴权分区 —— 所以路径是 `&'static str` 而不是
/// `impl AsRef<str>`：动态拼出来的路径在编译期就走不进这个入口。
///
/// `cfg` 与 CORS 装配读的是**同一个值**（[`build_router`] 里那一份），这样中间件
/// 的 Origin 判据与 `Access-Control-Allow-Origin` 永远同源。少传一个参数就没法
/// 从这一层保证「同一份配置来源」。
#[allow(clippy::too_many_arguments)]
fn register<S>(
    router: Router<S>,
    path: &'static str,
    method: Method,
    scope: AuthScope,
    handler: MethodRouter<S>,
    config_dir: &Path,
    cfg: &RemoteConfig,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    assert!(
        ROUTE_SCOPE_TABLE.iter().any(|(p, _)| *p == path),
        "路由 {path} 没有登记在 ROUTE_SCOPE_TABLE 里：新增路由必须先在表中声明它的凭据通道"
    );
    // method 必须能映射成一个 MethodFilter。映射不出来说明装配处写了非标准动词，
    // 装配期就红，不留到某个请求打过来时才发现这条路由的方法是空的。
    MethodFilter::try_from(method.clone())
        .unwrap_or_else(|_| panic!("路由 {path} 声明了无法映射到 MethodFilter 的方法 {method}"));

    router.route(
        path,
        handler.layer(auth_layer(scope, config_dir.to_path_buf(), cfg)),
    )
}

impl ManagedRemoteServer {
    pub fn new(inner: Arc<RwLock<Option<RemoteServer>>>) -> Self {
        Self(inner)
    }

    pub fn lock(&self) -> &Arc<RwLock<Option<RemoteServer>>> {
        &self.0
    }

    /// 获取 sse_tx 广播通道（仅当 server 已启动时返回 Some）
    pub async fn sse_tx(&self) -> Option<broadcast::Sender<String>> {
        let guard = self.0.read().await;
        guard.as_ref().map(|s| s.state().sse_tx.clone())
    }
}

/// 获取 Tailscale IPv4 地址
///
/// 走 `whois::resolve_bin()` 的三级解析阶梯（覆盖变量 → 绝对候选 → PATH），
/// 而不是裸 `Command::new("tailscale")`：GUI 启动的进程常常不带 `/usr/local/bin`，
/// 裸命令名会在这台机器上「每次都查不到」。
pub fn get_tailscale_ip() -> Option<String> {
    let bin = whois::resolve_bin()?;
    std::process::Command::new(bin)
        .args(["ip", "-4"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|ip| !ip.is_empty())
}

/// Check if Tailscale CLI is available and returns an IP
pub fn is_tailscale_available() -> bool {
    get_tailscale_ip().is_some()
}

/// 本机所有非回环 IPv4 地址（网卡名, 地址）。
///
/// 设置页在「全部网卡」档位下用它回答「我到底会在哪些地址上暴露」。绑定本身
/// 不看这张表 —— 那一档绑的是 `0.0.0.0`，这张表只描述「这个 0.0.0.0 当前
/// 长着哪些脸」，用于让用户在放宽之前看见暴露面。
///
/// 走 `getifaddrs(3)` 而不是新增依赖：依赖树零新增是本仓库的既有约束，
/// `libc` 已经在列。Windows 走同目录下的 `GetAdaptersAddresses` 实现，
/// 过滤规则与这里一致。
///
/// 过滤掉的地址：回环（不是「对外暴露」）、链路本地 `169.254/16`（APIPA
/// 只说明没拿到 DHCP）、未指定 / 组播 / 广播。
#[cfg(not(target_os = "windows"))]
pub fn local_ipv4_addresses() -> Vec<(String, std::net::Ipv4Addr)> {
    use std::ffi::CStr;
    use std::net::Ipv4Addr;

    let mut out = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `getifaddrs` 成功时给出一个由 `freeifaddrs` 释放的链表；
    // 下面只读该链表，并在返回前释放一次。
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return out;
    }

    let mut cursor = list;
    while !cursor.is_null() {
        // SAFETY: 游标来自 `getifaddrs` 返回的链表，非空即有效；`ifa_next`
        // 在循环体开头就取走，后面不再依赖游标仍指向这块内存。
        let (next, name, family, addr_ptr) = unsafe {
            let ifa = &*cursor;
            let addr_ptr = ifa.ifa_addr;
            let family = if addr_ptr.is_null() {
                0
            } else {
                (*addr_ptr).sa_family
            };
            (ifa.ifa_next, ifa.ifa_name, family, addr_ptr)
        };
        cursor = next;

        if addr_ptr.is_null() || family as i32 != libc::AF_INET {
            continue;
        }

        // SAFETY: 上一步已确认它是 `AF_INET` 的 sockaddr。
        let sin = unsafe { &*(addr_ptr as *const libc::sockaddr_in) };
        // `s_addr` 是网络字节序，先转成主机序的数值再交给 `Ipv4Addr::from`
        // （后者按大端解释这个 u32）。
        let addr = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
        if addr.is_loopback()
            || addr.is_link_local()
            || addr.is_unspecified()
            || addr.is_multicast()
            || addr.is_broadcast()
        {
            continue;
        }

        let name = if name.is_null() {
            String::new()
        } else {
            // SAFETY: `ifa_name` 是 `getifaddrs` 填好的 NUL 结尾字符串。
            unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned()
        };
        out.push((name, addr));
    }

    // SAFETY: `list` 由 `getifaddrs` 分配，这里只释放一次。
    unsafe { libc::freeifaddrs(list) };

    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    out.dedup();
    out
}

/// Windows 版：`GetAdaptersAddresses` 枚举本机所有非回环 IPv4 地址，
/// 过滤规则与 Unix 版一致，输出也按（网卡名, 地址）排序去重。
#[cfg(target_os = "windows")]
pub fn local_ipv4_addresses() -> Vec<(String, std::net::Ipv4Addr)> {
    use std::net::Ipv4Addr;
    use windows_sys::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN};

    fn wide_to_string(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        // SAFETY: `GetAdaptersAddresses` 返回的 PWSTR 以 NUL 结尾。
        while unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: 上面已量出 NUL 之前的长度，区间有效。
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
    }

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;

    // 第一次调用只问所需缓冲区大小；ERROR_BUFFER_OVERFLOW 是预期结果。
    let mut size: u32 = 0;
    let rc = unsafe {
        GetAdaptersAddresses(
            AF_UNSPEC as u32,
            flags,
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if rc != ERROR_BUFFER_OVERFLOW || size == 0 {
        return Vec::new();
    }

    // 用 u64 承载缓冲区，保证满足 `IP_ADAPTER_ADDRESSES_LH` 的对齐要求。
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    let rc = unsafe {
        GetAdaptersAddresses(
            AF_UNSPEC as u32,
            flags,
            std::ptr::null(),
            buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
            &mut size,
        )
    };
    if rc != 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        // SAFETY: `adapter` 指向 GetAdaptersAddresses 填好的链表节点。
        let (name, mut unicast, next) = unsafe {
            let a = &*adapter;
            (a.FriendlyName, a.FirstUnicastAddress, a.Next)
        };

        while !unicast.is_null() {
            // SAFETY: 同一链表，节点由系统分配。
            let (addr_ptr, addr_len, next_unicast) = unsafe {
                let u = &*unicast;
                (u.Address.lpSockaddr, u.Address.iSockaddrLength, u.Next)
            };
            unicast = next_unicast;

            if addr_ptr.is_null() || addr_len < std::mem::size_of::<SOCKADDR_IN>() as i32 {
                continue;
            }
            // SAFETY: 长度已覆盖 SOCKADDR_IN，且下面先核对 family。
            let (family, addr) = unsafe {
                let sin = &*(addr_ptr as *const SOCKADDR_IN);
                // `sin_addr` 是网络字节序的 4 个字节，直接按字节构造最省事。
                let octets = *(std::ptr::addr_of!(sin.sin_addr) as *const [u8; 4]);
                (sin.sin_family, Ipv4Addr::from(octets))
            };
            if family != AF_INET
                || addr.is_loopback()
                || addr.is_link_local()
                || addr.is_unspecified()
                || addr.is_multicast()
                || addr.is_broadcast()
            {
                continue;
            }
            out.push((wide_to_string(name), addr));
        }

        adapter = next;
    }

    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    out.dedup();
    out
}

/// Start or update the remote server.
/// If a server already exists, performs an incremental update:
/// - keeps existing listeners (e.g. localhost) that are still needed
/// - only starts/stops the changed addresses (e.g. Tailscale IP)
pub async fn start_remote(
    app: &tauri::AppHandle,
    config: RemoteConfig,
) -> Result<Vec<String>, String> {
    use tauri::Manager;

    let managed = app.state::<ManagedRemoteServer>();
    let mut guard = managed.lock().write().await;

    if let Some(server) = guard.as_ref() {
        // 已有 server：增量更新，不中断保留的地址
        log::info!("[Remote] Updating remote server configuration...");
        let urls = server.update(config).await?;
        log::info!("[Remote] Server updated on: {}", urls.join(", "));
        Ok(urls)
    } else {
        // 首次启动
        let new_server = RemoteServer::new(
            RemoteApp::gui(app.clone()),
            Arc::new(AppHandleSink::new(app.clone())),
            config,
            crate::config::get_app_config_dir(),
        )?;
        let urls = new_server.start().await?;
        *guard = Some(new_server);
        log::info!("[Remote] Server started on: {}", urls.join(", "));
        Ok(urls)
    }
}

/// Stop the remote server
pub async fn stop_remote(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let managed = app.state::<ManagedRemoteServer>();
    let mut guard = managed.lock().write().await;

    if let Some(server) = guard.take() {
        // 用户显式关闭：先广播 shutdown 让浏览器主动销毁页面
        match server
            .state()
            .sse_tx
            .send(r#"{"type":"shutdown"}"#.to_string())
        {
            Ok(n) => log::info!("[Remote] Broadcasted shutdown to {} SSE clients", n),
            Err(e) => log::warn!("[Remote] No SSE clients to broadcast shutdown: {e}"),
        }
        // 给 SSE 消息一点时间到达客户端
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        server.stop().await?;
    }

    log::info!("[Remote] Server stopped via command");
    Ok(())
}

/// 配对路由。状态是 [`PairingApiState`] 而不是 [`RemoteState`] —— 配对是
/// 「还没拿到凭据的人」走的路，不该经过持有 AppHandle 的那套状态。
///
/// 但**闸门照挂不误**：这里与 [`build_router`] 里 17 条业务路由走的是同一个
/// [`auth_layer`]，scope 取 [`AuthScope::Public`]。scope 决定的是「凭据闸门
/// 要不要」，不是「Origin 否决要不要」—— [`auth::middleware`] 的判定链把
/// Origin 否决排在最前，`Public` 只豁免第三段。于是配对端的语义与
/// `GET /`（同样是 `Public`）逐条对齐：
///
/// - `Origin` 在场且不在白名单 → 403（凭据再对也不救）；
/// - `Origin` 缺失或合法 → 放行，绝不因为「没带凭据」而 401。
///
/// 少了这一层，四条 `/pairing/*` 就成了 Origin 白名单上唯一的缺口：任意网页
/// 能让浏览器带着攻击者的 `Origin` 打 `POST /pairing/request`，而那一条
/// 路由的作用正是**签发长期凭据**。`cfg` 必须由 [`build_router`] 传进来，
/// 好让中间件读到的 Origin 名录与 CORS 的 `Access-Control-Allow-Origin`
/// 仍然同源（同一个 `allowed_origins(&cfg, ..)`）。
///
/// 刻意不进 [`register`]：那条入口断言路径已登记在 [`ROUTE_SCOPE_TABLE`]，
/// 而配对路由不计入那 17 条 —— 这里挂的是同一段中间件，不是同一张分区表。
fn pairing_router(state: &Arc<RemoteState>, cfg: &RemoteConfig) -> Router<Arc<RemoteState>> {
    pairing::router(state.pairing.clone())
        .with_state(state.pairing.clone())
        .layer(auth_layer(AuthScope::Public, state.config_dir.clone(), cfg))
}

/// 业务路由的唯一装配点。
///
/// `cfg` 是**显式参数**，不读 `state.config`：增量更新时新 listener 必须用
/// 新配置装配 router，而 `state.config` 在启动成功之前仍是旧值。读旧值的后果
/// 是 CORS 白名单与 Origin 判据永远滞后一次变更。
fn build_router(state: &Arc<RemoteState>, cfg: &RemoteConfig) -> Router {
    // Origin 白名单只有一份来源：`allowed_origins` 按 `RemoteConfig` 算。
    // 这里若自己另列一份，中间件那边就永远漏掉其中几条 —— 两份清单各自
    // 漂移，而症状是「内嵌 webview 被浏览器按跨源掐掉」，只在生产构建里
    // 表现为白屏。
    let origins = allowed_origins(cfg, REMOTE_LISTENER_IS_TLS);

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(Any)
        .allow_headers(Any);

    // 业务路由只能经受控的 register() 进入 Router：路径与 scope 一并
    // 声明在本函数里，鉴权中间件由 register 挂在每条路由上。
    let mut router: Router<Arc<RemoteState>> = Router::new();
    router = register(
        router,
        "/",
        Method::GET,
        AuthScope::Public,
        axum::routing::get(handlers::index),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/providers",
        Method::GET,
        AuthScope::Protected,
        axum::routing::get(handlers::get_providers),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/usage",
        Method::GET,
        AuthScope::Protected,
        axum::routing::get(handlers::get_usage),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/switch",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::switch_provider),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/reorder",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::reorder_providers),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/current",
        Method::GET,
        AuthScope::Protected,
        axum::routing::get(handlers::get_current),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/events",
        Method::GET,
        AuthScope::BrowserSafe,
        axum::routing::get(handlers::sse_events),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/health",
        Method::GET,
        AuthScope::Protected,
        axum::routing::get(handlers::health_check),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/icon",
        Method::GET,
        AuthScope::BrowserSafe,
        axum::routing::get(handlers::get_icon),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/provider-icons/:name",
        Method::GET,
        AuthScope::BrowserSafe,
        axum::routing::get(handlers::get_provider_icon),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/proxy/control",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::proxy_control),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/proxy/takeover",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::proxy_takeover),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/proxy/failover",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::proxy_failover),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/provider/test",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::test_provider),
        state.config_dir.as_path(),
        cfg,
    );
    // 两段式绑定范围的三条路由。全部 `Protected`：非回环来源必须持有配对
    // 凭证才摸得到，而它们只负责「申请 / 确认 / 拒绝」，没有任何一条能直接
    // 接受一个档位并落盘。
    router = register(
        router,
        "/api/remote/config/bind-scope/request",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::request_bind_scope),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/remote/config/bind-scope/confirm",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::confirm_bind_scope),
        state.config_dir.as_path(),
        cfg,
    );
    router = register(
        router,
        "/api/remote/config/bind-scope/reject",
        Method::POST,
        AuthScope::Protected,
        axum::routing::post(handlers::reject_bind_scope),
        state.config_dir.as_path(),
        cfg,
    );

    // 配对是「还没拿到凭据」一方唯一的入口，走配对子系统自带的一次性凭据，
    // 不在本表内、不计入 17 条业务路由 —— 但 Origin 白名单与业务路由共用同一段
    // 中间件（见 `pairing_router`），白名单外的 Origin 一律 403。
    router
        .merge(pairing_router(state, cfg))
        .layer(cors)
        .with_state(state.clone())
}

/// App 内切换 provider 后，直接广播 SSE 给所有已连接的远程浏览器（App → Web 方向）。
/// 在 tray.rs / failover.rs 等切换点调用此函数，替代无效的 app_handle.listen() 方案
/// （app_handle.emit() 只发给 JS 前端，不触发 Rust listen 回调）。
///
/// 函数内部会从 AppState 查询 provider 名称，调用方只需传入 provider_id 和 app_type。
pub async fn broadcast_provider_switch(
    app: &tauri::AppHandle,
    app_type_str: &str,
    provider_id: &str,
) {
    use tauri::Manager;

    let managed = match app.try_state::<ManagedRemoteServer>() {
        Some(m) => m,
        None => {
            log::warn!("[Remote] broadcast_provider_switch: ManagedRemoteServer not found");
            return;
        }
    };

    let guard = managed.lock().read().await;
    let server = match guard.as_ref() {
        Some(s) => s,
        None => {
            log::warn!("[Remote] broadcast_provider_switch: RemoteServer not started");
            return;
        }
    };

    let name: String = app
        .try_state::<crate::store::AppState>()
        .and_then(|s| {
            s.db.get_all_providers(app_type_str)
                .ok()
                .and_then(|map| map.get(provider_id).map(|p| p.name.clone()))
        })
        .unwrap_or_else(|| provider_id.to_string());

    let msg = serde_json::json!({
        "type": "switch",
        "provider_id": provider_id,
        "name": name,
    })
    .to_string();

    match server.state().sse_tx.send(msg) {
        Ok(_) => {
            log::info!(
                "[Remote] Broadcasted provider switch: {} (id={})",
                name,
                provider_id
            );
        }
        Err(e) => {
            log::error!("[Remote] Failed to broadcast provider switch: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::auth::origin_is_allowed;
    use super::{allowed_origins, CorsMode, RemoteConfig};
    #[cfg(debug_assertions)]
    use super::{dev_server_origins, DEV_SERVER_ORIGIN};

    /// 三档位到监听地址的映射（规格 B），含 Tailscale 拿不到 IP 的兜底。
    #[test]
    fn compute_target_addrs_follows_the_tier_and_falls_back_without_tailscale() {
        let loopback = std::net::SocketAddr::from(([127, 0, 0, 1], 4567));
        let tailscale = std::net::SocketAddr::from(([203, 0, 113, 9], 4567));
        let all = std::net::SocketAddr::from(([0, 0, 0, 0], 4567));

        let mut cfg = super::RemoteConfig {
            port: 4567,
            ..Default::default()
        };
        assert_eq!(
            super::compute_target_addrs_with(&cfg, Some("203.0.113.9")),
            vec![loopback],
            "Loopback 只绑回环"
        );

        cfg.bind_scope = super::BindScope::Tailscale;
        assert_eq!(
            super::compute_target_addrs_with(&cfg, Some("203.0.113.9")),
            vec![loopback, tailscale],
            "Tailscale = 回环 + Tailscale IP"
        );
        assert_eq!(
            super::compute_target_addrs_with(&cfg, None),
            vec![loopback],
            "拿不到 Tailscale IP 时退回回环，而不是什么都不绑"
        );
        assert_eq!(
            super::compute_target_addrs_with(&cfg, Some("not-an-ip")),
            vec![loopback],
            "Tailscale IP 解析失败同样退回回环"
        );

        cfg.bind_scope = super::BindScope::AllInterfaces;
        assert_eq!(
            super::compute_target_addrs_with(&cfg, Some("203.0.113.9")),
            vec![all],
            "AllInterfaces 只绑 0.0.0.0（它覆盖回环）"
        );
    }

    /// 剥掉行尾注释（区分字符串字面量），让断言读的是代码而不是说明。
    fn strip_comments(src: &str) -> String {
        let mut out = String::new();
        for line in src.lines() {
            let bytes = line.as_bytes();
            let mut in_str = false;
            let mut cut = bytes.len();
            let mut i = 0;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' if in_str => i += 1,
                    b'"' => in_str = !in_str,
                    b'/' if !in_str && bytes.get(i + 1) == Some(&b'/') => {
                        cut = i;
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }
            let code = line[..cut].trim();
            if code.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(code);
        }
        out
    }

    /// 本文件的生产代码部分：`mod tests {` 之前的全部内容，且已剥掉行尾注释。
    ///
    /// 两处收窄缺一不可：
    ///
    /// - 剥注释：装配点上方那段注释里也写着 `into_make_service_with_connect_info`，
    ///   直接 `contains` 会被纯说明文字骗过去。
    /// - 截到测试模块之前：断言用的字面量本身就是源文件里的一行，自指的
    ///   needle 会把自己数进去，让断言恒真。
    fn production_code(src: &str) -> String {
        let head = src
            .split("mod tests {")
            .next()
            .expect("源码里总有 mod tests {");
        strip_comments(head)
    }

    /// serve 必须经 `into_make_service_with_connect_info::<SocketAddr>()` 装配。
    ///
    /// 缺了它，axum 不会往 request 的 extensions 里塞对端地址，中间件的
    /// `peer_of` 永远读到 `None` → 一律按 NonLoopback 算，四层规则就退化成
    /// 「碰运气」：服务将来绑到 `0.0.0.0` 时，外部请求的 peer 应当非 loopback
    /// 并自动落到要 token 的分支，而这个前提只有在装配了这一层时才成立。
    #[test]
    fn serve_uses_connect_info_service() {
        let code = production_code(include_str!("mod.rs"));
        assert!(
            code.contains("into_make_service_with_connect_info::<SocketAddr>()"),
            "axum::serve 必须走 into_make_service_with_connect_info::<SocketAddr>()，\
             否则中间件拿不到真实 TCP peer"
        );
        // 两个 listener 共用同一个 Router，共享的 serve 装配点就是
        // `for addr in addrs` 循环里的那一次调用；出现两次说明有人按地址各写了一份。
        assert_eq!(
            code.matches("axum::serve(").count(),
            1,
            "两个 listener 必须共用同一处 axum::serve 装配：ConnectInfo 的装配不能按地址分叉"
        );
    }

    /// CORS 白名单里既没有通配，也没有 `Origin: null`。
    ///
    /// 这两条各对应一类真实来源：`null` 是 sandbox iframe / `file:` / `data:`
    /// 页面发出的请求，`*` 则**就是**「任意 origin」的另一种拼写。放进白名单
    /// 等于把「有页面在跨源读这个服务」这件事从「需要逐条批准」降级成「默认允许」，
    /// 而 Origin 是浏览器给的、攻击者完全可控的字段。
    ///
    /// 反证输入是「最毒」的白名单：`validate_whitelist` 在 `set_cors` 那道闸门上
    /// 已经会拒它们，但 [`RemoteConfig`] 是个可以直接构造的普通结构体 ——
    /// 命令层、测试夹具、未来的迁移代码都能绕过那道闸门。`allowed_origins`
    /// 因此自己再过一遍同一份判据（[`config::validate_whitelist`]），
    /// 而不是靠「入口已经校验过」。
    #[test]
    fn cors_origins_exclude_wildcard_and_null() {
        use axum::http::HeaderValue;

        /// 白名单里「看起来像能放行」的毒条目，以及一条合法对照。
        const POISON: [&str; 5] = ["*", "null", "NULL", "", "   "];
        const OK_ENTRY: &str = "http://ok.example";

        let cfg = RemoteConfig {
            cors_mode: CorsMode::ExplicitWhitelist,
            origin_whitelist: POISON
                .iter()
                .map(|s| s.to_string())
                .chain(std::iter::once(OK_ENTRY.to_string()))
                .collect(),
            ..Default::default()
        };

        for tls in [false, true] {
            let origins = allowed_origins(&cfg, tls);
            assert!(
                !origins.is_empty(),
                "白名单不得为空：内嵌 webview 的 origin 不在列就会落第 11 格 403"
            );

            for origin in &origins {
                let s = origin.to_str().expect("origin 必须是可见 ASCII");
                let trimmed = s.trim();
                assert_ne!(trimmed, "*", "白名单里不得有通配 origin：{s:?}");
                assert_ne!(trimmed, "null", "白名单里不得有 Origin: null：{s:?}");
                assert!(!s.contains('*'), "白名单里不得有带通配的 origin：{s:?}");
                assert!(!trimmed.is_empty(), "白名单里不得有空白 origin");
                // 与中间件第②层判据同源：CORS 面放行的每一条，中间件都必须放行。
                // 清单是同一个函数算出来的（`allowed_origins`），两层不可能分叉 ——
                // 这条断言盯的是「有人又在本模块外另列了一份」。
                assert!(
                    origin_is_allowed(trimmed, &cfg, tls),
                    "CORS 面放行的 {trimmed:?} 必须被中间件第②层判据接受，\
                     否则 CORS 放行、403 否决，两层各说各话"
                );
            }

            // 结构性的内嵌 webview origin 必须显式在列 —— 不在列就是 403，
            // 而 Tauri webview 是桌面端唯一的 UI 通道。
            assert!(
                origins.contains(&HeaderValue::from_static("tauri://localhost")),
                "tauri://localhost（macOS/Linux webview）必须在白名单里，实际 {origins:?}"
            );
            assert!(
                origins.contains(&HeaderValue::from_static("https://tauri.localhost")),
                "https://tauri.localhost（Windows webview）必须在白名单里，实际 {origins:?}"
            );
            // TLS listener 上 scheme 跟着换，否则 https 页面发来的请求
            // 匹配不上任何一条，CORS 面直接失效。
            let own = if tls {
                "https://127.0.0.1:4000"
            } else {
                "http://127.0.0.1:4000"
            };
            assert!(
                origins.contains(&HeaderValue::from_str(own).unwrap()),
                "TLS={tls} 时本服务自身的 origin 应当是 {own}，实际 {origins:?}"
            );
        }

        // 合法条目进得来，毒的一个都进不来（否则上面的否定断言是空的）。
        let origins = allowed_origins(&cfg, false);
        assert!(
            origins.contains(&HeaderValue::from_static(OK_ENTRY)),
            "合法白名单条目必须真的放行，否则上面的否定断言验不到东西：{origins:?}"
        );
        for poison in POISON {
            assert!(
                !origins.iter().any(|o| o == poison),
                "{poison:?} 混进了 CORS 白名单：{origins:?}"
            );
        }
    }

    /// `RemoteConfig` 跨 IPC 的字段名是 camelCase —— 前端 TS 接口按这个写。
    ///
    /// 这条钉的是一次真实故障：结构体漏了 `rename_all`，字段序列化成
    /// `bind_scope`，前端读 `config.bindScope` 拿到 `undefined` —— 绑定范围
    /// 三个单选框一个都不选中；而 `indexOf(undefined) === -1`，点「仅本机」
    /// 又被判成「放宽」，平白弹一个确认框。
    #[test]
    fn remote_config_speaks_camel_case_over_ipc() {
        let value = serde_json::to_value(RemoteConfig::default()).expect("serialize");
        for key in [
            "enabled",
            "port",
            "tailscaleEnabled",
            "bindScope",
            "corsMode",
            "originWhitelist",
        ] {
            assert!(value.get(key).is_some(), "缺字段 {key}：{value}");
        }
        assert!(
            value.get("bind_scope").is_none(),
            "不得出现 snake_case 字段名：{value}"
        );

        let back: RemoteConfig = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back.bind_scope, super::BindScope::Loopback);
    }

    /// `CC_SWITCH_DEV_ORIGIN` 只加合法 origin，非法条目不得混进来。
    ///
    /// 这条防的是一次真实踩坑：Vite 的 `port: 3000` 没配 `strictPort`，3000 被
    /// 占用时它自己换端口，webview 的 origin 随之改变，前端 `fetch` 自己的
    /// `/api/health` 拿到 403 —— 界面报「启动失败」，而服务其实已经绑上了。
    #[cfg(debug_assertions)]
    #[test]
    fn dev_origin_override_is_validated_before_joining_the_allowlist() {
        let accepted = dev_server_origins(Some("http://localhost:3400"));
        assert!(
            accepted.iter().any(|o| o == "http://localhost:3400"),
            "合法的 dev origin 必须进名单：{accepted:?}"
        );
        assert!(
            accepted.iter().any(|o| o == DEV_SERVER_ORIGIN),
            "写死的 3000 仍然在列：{accepted:?}"
        );

        for poison in ["*", "null", "file:///tmp/x", ""] {
            assert_eq!(
                dev_server_origins(Some(poison)),
                vec![DEV_SERVER_ORIGIN.to_string()],
                "{poison:?} 不得进白名单"
            );
        }
    }

    /// `local_ipv4_addresses` 只返回「对外可达」那一类地址。
    ///
    /// 不断言具体网卡（CI 机器未必有 `en0`），只钉过滤规则：回环与链路本地
    /// 不得出现在暴露面里 —— 那是「本机自己」和「没拿到 DHCP」，都不是用户
    /// 想看见的暴露地址。
    #[test]
    fn local_ipv4_addresses_excludes_loopback_and_link_local() {
        for (iface, addr) in super::local_ipv4_addresses() {
            assert!(!addr.is_loopback(), "{iface} 回环地址混进了暴露面: {addr}");
            assert!(
                !addr.is_link_local(),
                "{iface} 链路本地地址混进了暴露面: {addr}"
            );
            assert!(!addr.is_unspecified(), "{iface} 0.0.0.0 混进了暴露面");
        }
    }

    /// 中间件第②层判据与 CORS 装配读的是**同一份来源**，不是各写一份。
    ///
    /// 这条锁的是**配置来源唯一**。只做源码包含检查的那一版
    /// 是空转的：`allowed_origins(` 出现在文件里，与「中间件读的是不是这份名单」
    /// 毫无关系 —— 硬编码一份 `["127.0.0.1", "localhost"]` 的中间件同样能通过。
    /// 所以这里用三条**负向**自测把「同源」钉死：每一条在两层各写一份时都会红，
    /// 而只有当两层真的算自同一份 `(RemoteConfig, tls)` 时才全绿。
    #[test]
    fn cors_assembly_goes_through_allowed_origins() {
        let code = production_code(include_str!("mod.rs"));
        assert!(
            code.contains("allowed_origins("),
            "build_router 必须经 allowed_origins(&cfg, ...) 装配 CORS"
        );
        assert!(
            !code.contains("allow_origin(Any)"),
            "allow_origin(Any) 就是通配 origin：本服务不得表达「任意来源」"
        );

        // 负向一：用户白名单里的 origin 必须**同时**过 CORS 面与中间件。
        // 这是本条要挡的真实故障：显式白名单模式下 origin 被 CORS 层放行、却被
        // 中间件 403，用户的 CORS 配置静默失效。
        let user_origin = "https://desk.example.invalid";
        let whitelist_cfg = RemoteConfig {
            cors_mode: CorsMode::ExplicitWhitelist,
            origin_whitelist: vec![user_origin.to_string()],
            ..Default::default()
        };
        assert!(
            allowed_origins(&whitelist_cfg, false)
                .iter()
                .any(|o| o == user_origin),
            "CORS 面必须放行用户白名单里的 {user_origin}"
        );
        assert!(
            origin_is_allowed(user_origin, &whitelist_cfg, false),
            "中间件必须放行同一条 {user_origin}：两层算自同一份来源"
        );

        // 负向二：端口一改，两层一起换名单。硬编码 `127.0.0.1:4000` 的那份
        // 中间件白名单过不了这条 —— 它对配置无感。
        let moved = RemoteConfig {
            port: 4001,
            ..Default::default()
        };
        assert!(
            !allowed_origins(&moved, false)
                .iter()
                .any(|o| o == "http://127.0.0.1:4000"),
            "端口改成 4001 之后 CORS 面不得再放行 http://127.0.0.1:4000"
        );
        assert!(
            !origin_is_allowed("http://127.0.0.1:4000", &moved, false),
            "端口改成 4001 之后中间件也不得再放行 http://127.0.0.1:4000：\
             判定必须由配置算出，不能另有一份写死的 host 表"
        );

        // 负向三：中间件判据的实现里不得再留一份独立的 host 白名单常量。
        let policy = production_code(include_str!("auth/policy.rs"));
        assert!(
            policy.contains("allowed_origins("),
            "中间件的 Origin 判定必须经 allowed_origins(&cfg, tls) 取名单"
        );
        assert!(
            !policy.contains("ALLOWED_HOSTS") && !policy.contains("ALLOWED_ORIGINS"),
            "policy.rs 里不得另立一份 origin 白名单：那是第二份来源"
        );
    }

    /// 中间件的 peer 只来自连接属性，不读监听地址、不读配置开关。
    ///
    /// 读监听地址是最容易写出的错法：服务一旦绑到 `0.0.0.0`，listener 的
    /// local_addr 就是 `0.0.0.0`，据此判 loopback 会把整台机器的外部请求
    /// 全放行。
    #[test]
    fn peer_is_taken_from_connect_info_only() {
        let code = production_code(include_str!("auth/middleware.rs"));
        let body = code
            .split("fn peer_of(")
            .nth(1)
            .and_then(|rest| rest.split("\n}").next())
            .expect("middleware.rs 里必须有 peer_of");
        assert!(
            body.contains("ConnectInfo<SocketAddr>"),
            "peer_of 必须从 ConnectInfo<SocketAddr> 取对端地址"
        );
        for forbidden in ["local_addr", "config", "tailscale"] {
            assert!(
                !body.contains(forbidden),
                "peer_of 里出现 {forbidden}：peer 只能来自连接属性，\
                 读监听地址/配置会让 0.0.0.0 绑定下的外部请求被误判成 loopback"
            );
        }
    }
}

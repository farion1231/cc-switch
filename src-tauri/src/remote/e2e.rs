//! E2E 基座：headless 起 remote server，供后续全部 E2E 用例复用。
//!
//! 为什么放在 crate 内而不是 `src-tauri/tests/`：后者是独立 crate，看不到私有的
//! `mod remote;`（`lib.rs`），而 `RemoteServer::new` / `build_router` / `start_addrs`
//! 全是私有固有方法 —— 放外面根本建不出夹具。挂载方式与 `handlers.rs` 的
//! `#[cfg(test)] mod tests` 同款。
//!
//! # 挂载方式（逐字可查）
//!
//! 本文件的挂载点**不在本文件里**，而在 `src-tauri/src/remote/mod.rs` 的模块声明
//! 段。该文件逐字写着：
//!
//! ```text
//! #[cfg(test)]
//! mod e2e;
//! ```
//!
//! 即：
//!
//! - 声明所在的文件是 `src-tauri/src/remote/mod.rs`（不是本文件，也不是
//!   `lib.rs`）；
//! - 声明逐字为 `mod e2e;`（`#[cfg(test)]` 是它上面那一行的独立属性）；
//! - `#[cfg(test)]` 条件意味着本文件只在 `cargo test` 的 test 配置下编译，
//!   正式构建里不参与，因而可以放心用 `pub` 夹具 API。
//!
//! 所以要核对挂载点，请看 `src-tauri/src/remote/mod.rs`；本文件顶部的这段文档
//! 只是把同一件事写一遍，免得引用时两边对不上。
//!
//! # 12 格判定表
//!
//! 四层授权规则的每一条出口都由下表逐格钉死。表里出现的判据与本文件的断言
//! 用的是同一套字面量：`peer` 来自 `super::auth` 的 `peer_of(ConnectInfo)`（真实
//! TCP 对端，不是配置开关），`Origin` 取 `ORIGIN_WHITELISTED` / `ORIGIN_FOREIGN`
//! 两个常量，代理信号头是 `PROXY_SIGNAL`。**12 格一律不带凭据**，所以
//! 「凭据档位」一列恒为「无」，这一列是为了让读者看出：出口翻转只由前两列驱动，
//! 与是否出示凭据无关。
//!
//! | 格号 | 用例函数 | peer | Origin | 代理信号头 | 凭据 | 期望出口 | 对应规则 |
//! |------|----------|------|--------|------------|------|----------|----------|
//! | 第 1 格 | `cell_01_remote_no_origin_no_proxy_is_401` | 非 loopback | 无 | 无 | 无 | 401 | 非 loopback 无 token → 401 |
//! | 第 2 格 | `cell_02_remote_no_origin_with_proxy_signal_is_401` | 非 loopback | 无 | 有 | 无 | 401 | 非 loopback 无 token → 401（经代理转发不降低要求） |
//! | 第 3 格 | `cell_03_remote_whitelisted_origin_no_proxy_is_401` | 非 loopback | 白名单 | 无 | 无 | 401 | Origin 只影响第①层，闸门之后仍须凭据 |
//! | 第 4 格 | `cell_04_remote_whitelisted_origin_with_proxy_signal_is_401` | 非 loopback | 白名单 | 有 | 无 | 401 | 同上 |
//! | 第 5 格 | `cell_05_remote_foreign_origin_is_403` | 非 loopback | 非白名单 | 无 | 无 | 403 | 非白名单 Origin → 403 |
//! | 第 6 格 | `cell_06_remote_foreign_origin_with_proxy_signal_is_403` | 非 loopback | 非白名单 | 有 | 无 | 403 | 代理信号不是凭据，不能给 Origin 翻案 |
//! | 第 7 格 | `cell_07_loopback_no_origin_no_proxy_allows_real_tcp` | loopback | 无 | 无 | 无 | 放行（2xx） | loopback 无 Origin 无代理信号 → 放行 |
//! | 第 8 格 | `cell_08_loopback_no_origin_with_proxy_signal_is_401` | loopback | 无 | 有 | 无 | 401 | loopback + 代理信号 → 按远程处理，无 token → 401 |
//! | 第 9 格 | `cell_09_loopback_whitelisted_origin_no_proxy_allows_real_tcp` | loopback | 白名单 | 无 | 无 | 放行（2xx） | 白名单 Origin 不改变通道 |
//! | 第 10 格 | `cell_10_loopback_whitelisted_origin_with_proxy_signal_is_401` | loopback | 白名单 | 有 | 无 | 401 | 白名单 Origin 抹不掉代理信号带来的要求 |
//! | 第 11 格 | `cell_11_loopback_foreign_origin_is_403` | loopback | 非白名单 | 无 | 无 | 403 | Origin 否决在最前，本机也不例外 |
//! | 第 12 格 | `cell_12_loopback_foreign_origin_with_proxy_signal_is_403` | loopback | 非白名单 | 有 | 无 | 403 | 凭据 / 代理信号都救不了 Origin 否决 |
//!
//! 第 1~6 格走 `remote_cell`，来源地址钉在本机真实非 loopback 地址上，本机没有
//! 第二块可用地址时按 SKIP 约定退出（第 7~12 格走 `loopback_cell`，在任何 runner
//! 上都真跑，不依赖网卡）。参见本文件里 `connect_info_is_actually_used` 的变异
//! 矩阵：`peer` 那一列若被替换成常量，上面会有一批格子转红。

use super::auth::{
    generate_token, load_clients, load_or_create, parse_session_cookie, save_clients, token_hash,
    AuthScope, ClientToken, SecretError, AUTH_SUBDIR, REMOTE_LISTENER_IS_TLS, SECRET_FILE,
};
use super::config::startup_remote_config;
use super::events::{EventSink, RecordingSink};
use super::{
    allowed_origins, redact, BindScope, RemoteApp, RemoteConfig, RemoteServer, ROUTE_SCOPE_TABLE,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// 一次「远端来源」构造的结果：本机那块非 loopback 地址。
///
/// 客户端把本地来源钉死在 `ip` 上、目的地址仍是 loopback listener，于是服务端
/// [`super::auth`] 的 `peer_of` 读到的一定是 `NonLoopback` —— 这一侧是真实
/// TCP 连接属性，不是测试往 extensions 里手塞的。
#[derive(Debug, Clone, Copy)]
pub struct SourceAddr {
    /// 本机可作来源的非 loopback 地址
    pub ip: IpAddr,
}

/// 探测本机私有网段的目标地址。
///
/// 按私网三段各铺开一遍：`connect` 一个 UDP socket 到某个目标，内核只做路由
/// 选择、不发任何包，`local_addr()` 就是「去那儿会从哪个地址出发」。铺开是为了
/// 不假设网段形态（192.168.x.x / 10.x.x.x / 172.16-31.x.x 都可能）。
fn probe_targets() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    for a in 1..=40u8 {
        out.push(Ipv4Addr::new(192, 168, a, 1));
    }
    for a in 0..=2u8 {
        for b in 0..=1u8 {
            out.push(Ipv4Addr::new(10, a, b, 1));
        }
    }
    for a in 16..=31u8 {
        for b in 0..=1u8 {
            out.push(Ipv4Addr::new(172, a, b, 1));
        }
    }
    out
}

/// 这块地址能不能当**来源**发包到一个 loopback listener。
///
/// 实测这一步是必需的，不能靠网段猜测替代：VPN / tun 网卡（Clash 之类把默认
/// 路由劫走的）会给出 198.18.x.x、100.64.x.x 这种「看起来像局域网、其实不是」
/// 的候选地址 —— 它们能 bind 当来源，却发不出包。
///
/// 目的地址固定用 127.0.0.1：非 loopback 目的会让包真的上局域网，一来一回要
/// 穿过 macOS 的「本地网络」隐私权限（未授权的进程连 SYN 都发不出去）与 CI
/// 沙箱的出网策略 —— 而这里要验的从来不是「能不能上网」，只是「服务端看到的
/// 对端是不是非 loopback」。目的留在 loopback，这条路就完全不碰局域网。
async fn sources_to_loopback(ip: IpAddr) -> Result<(), String> {
    const STEP: Duration = Duration::from_millis(400);

    let Ok(Ok(listener)) = tokio::time::timeout(
        STEP,
        tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)),
    )
    .await
    else {
        return Err("loopback listener 起不来".to_string());
    };
    let Ok(local) = listener.local_addr() else {
        return Err("取不到 listener 地址".to_string());
    };
    let Ok(socket) = tokio::net::TcpSocket::new_v4() else {
        return Err("建 socket 失败".to_string());
    };
    if let Err(e) = socket.bind(SocketAddr::new(ip, 0)) {
        return Err(format!("来源 bind 失败：{e}"));
    }
    if !matches!(
        tokio::time::timeout(STEP, socket.connect(local)).await,
        Ok(Ok(_))
    ) {
        return Err("发不出到 loopback 的包（多半是 tun / VPN 网卡）".to_string());
    }
    match tokio::time::timeout(STEP, listener.accept()).await {
        // 对端必须就是这块地址本身：否则「来源」是假的
        Ok(Ok((_, peer))) if peer.ip() == ip => Ok(()),
        Ok(Ok((_, peer))) => Err(format!("对端不是本地址（实际是 {peer}）")),
        Ok(Err(e)) => Err(format!("accept 失败：{e}")),
        Err(_) => Err("连接发起了但 listener 收不到".to_string()),
    }
}

/// 造一个真实的非 loopback 来源；本机没有可用地址时返回 `Err` 并附上
/// 每个候选被否的原因。
///
/// 为什么不用 `oneshot` + 手塞 `ConnectInfo`：那样验的是「我们希望中间件看到
/// 的 peer」，中间件完全可以在别的装配点上拿不到 ConnectInfo 而测试仍绿 ——
/// 上一个任务补的正是这条装配门禁，这里必须让真实连接属性说了算。
pub async fn nonloopback_source() -> Result<SourceAddr, String> {
    let mut seen: Vec<IpAddr> = Vec::new();
    let mut rejected: Vec<String> = Vec::new();
    for target in probe_targets() {
        let Ok(sock) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
            continue;
        };
        // UDP 的 connect 只做路由选择，一个包都不发
        if sock.connect(SocketAddr::from((target, 9))).is_err() {
            continue;
        }
        let Ok(local) = sock.local_addr() else {
            continue;
        };
        let ip = local.ip();
        if ip.is_loopback() || ip.is_unspecified() || seen.contains(&ip) {
            continue;
        }
        seen.push(ip);
        match sources_to_loopback(ip).await {
            Ok(()) => return Ok(SourceAddr { ip }),
            Err(why) => rejected.push(format!("{ip}：{why}")),
        }
    }
    Err(if rejected.is_empty() {
        "探测不到任何非 loopback 地址（可能只有 lo0 / 127.0.0.1）".to_string()
    } else {
        rejected.join("；")
    })
}

/// 跑起来的 remote server + 隔离环境。
///
/// `cfg_dir` 是独立的 tempfile 目录：夹具绝不能读到用户真实的 `~/.cc-switch`。
pub struct Harness {
    /// 实际监听地址（含内核分配的临时端口）
    pub base_url: String,
    /// 凭据落盘目录。
    ///
    /// 这里存**路径**而不是 `TempDir` 本身：重启序列要求服务 A 与服务 B 共用
    /// 同一个目录（[`start_headless_in`]），而 `TempDir` 一 drop 就删目录 ——
    /// 所有权若挂在服务 A 上，`teardown(A)` 一执行，B 起来时 secret 已经被删了，
    /// 「重启」就变成了「换了个空目录」。删除责任因此挪到 `cfg_owner`。
    pub cfg_dir: PathBuf,
    /// 目录的删除权。自建 tempdir 时是 `Some`（随 `Drop` 删除）；用调用方给的
    /// 目录时是 `None`，由调用方负责清理。
    cfg_owner: Option<tempfile::TempDir>,
    /// 注入的事件出口，用例结束后可读出事件。
    /// 后续 auth E2E 用例据此断言审批事件，本任务的用例还没读它。
    #[allow(dead_code)]
    pub recording: RecordingSink,
    /// 持有 server 与 listener 任务
    server: Arc<RemoteServer>,
    /// listener 任务的 AbortHandle，供 `Drop` 在断言失败时同步兜底
    abort_handles: Vec<tokio::task::AbortHandle>,
}

impl Harness {
    /// 拼出可用的 base URL（去掉结尾斜杠）
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl Drop for Harness {
    /// 断言失败也要收尾。
    ///
    /// `Drop` 不能 await，所以这里不能调 `stop()`，只能同步 abort listener 任务 ——
    /// abort 会 drop 掉 task 内的 `axum::serve`，进而 drop 掉 `TcpListener`，
    /// 端口随之释放。正常路径由 [`harness::teardown`] 走完整的优雅关闭。
    fn drop(&mut self) {
        for h in &self.abort_handles {
            h.abort();
        }
        // 目录删除权就挂在这个字段上。显式 drop 既是让意图可读，也是免得它看着
        // 像「没人读的死字段」被顺手删掉 —— 删了这个字段，tempdir 就再也不会被
        // 删除（`start_headless` 那条路径会一路漏到 /tmp）。
        drop(self.cfg_owner.take());
    }
}

/// 无 AppHandle 的 headless 启动入口。
///
/// 走的是生产装配路径本身：`RemoteServer::new` → `build_router` → `start_addrs`
/// （后两步由 `start_addrs` 内部调用），本文件不自己建 `Router`。
///
/// 端口传 0 交给内核分配，绝不写死 4000。
pub async fn start_headless(recording: RecordingSink) -> Harness {
    let cfg_dir = tempfile::tempdir().expect("create isolated temp config dir");
    let config_dir = cfg_dir.path().to_path_buf();
    // 删除权留给 Harness：目录必须活到最后一个用例收尾那一刻。
    start_headless_inner(config_dir, Some(cfg_dir), recording).await
}

/// 在**给定**凭据目录上起一个 headless server，目录的生命周期归调用方。
///
/// 这是「重启」夹具的那一半：把服务停掉、对同一个 `config_dir` 再调一次本函数，
/// 就是一次等价的凭据重载 —— `secret.rs` 的 `load_or_create` 全部路径都由这个
/// 入参拼出（`config_dir.join(AUTH_SUBDIR)`），没有任何全局状态或进程内缓存，
/// 所以「换一个 server 实例」在凭据这一侧与「换进程」是同一件事。
///
/// 不自己建 tempdir 是刻意的：两个服务必须共用同一个目录，各自建各自的等于
/// 「拿一把新钥匙去开旧锁」，那样断言会绿而产品是坏的。
pub async fn start_headless_in(config_dir: PathBuf, recording: RecordingSink) -> Harness {
    start_headless_inner(config_dir, None, recording).await
}

/// 两个入口共用的建服逻辑。`cfg_owner` 只决定「谁删这个目录」。
///
/// 绑定固定是回环：`127.0.0.1:0` 交给内核分配临时端口。需要别的绑定面时走
/// [`start_headless_on`] —— 那一格在回环绑定下**不可能**为真，原因见它的文档。
async fn start_headless_inner(
    config_dir: PathBuf,
    cfg_owner: Option<tempfile::TempDir>,
    recording: RecordingSink,
) -> Harness {
    start_headless_bound(
        config_dir,
        cfg_owner,
        recording,
        vec![SocketAddr::from(([127, 0, 0, 1], 0))],
    )
    .await
}

/// 按**给定地址**建服，凭据目录自建临时目录、删权归返回的 [`Harness`]。
///
/// 与 [`start_headless`] 的唯一差别是绑定地址成了入参：夹具的绑定面是它的一项
/// 属性，不该藏在建服函数体里的一行常量。用例要表达「这一格跑在一台**另起**的
/// 服务上、连的是回环 listener」时，就把它写出来。
///
/// 注意「非回环对端」有两种造法，只有一种在本机能跑通：
///
/// - **来源钉在非回环地址、目的留在回环 listener**（[`raw_of`]）—— 可行。
/// - **listener 绑在非回环地址上、客户端连那个地址** —— 不可行。实测在本机，
///   连裸 `tokio::net::TcpListener` 都收不到回程：TCP 三次握手能完成（连接进
///   了 backlog），数据却发不出去，macOS 的「本地网络」隐私权限与 CI 沙箱的
///   出网策略都掐在这里 —— [`sources_to_loopback`] 的注释记的是同一件事。
///   而 [`super::auth::policy`] 的判定只读 `ConnectInfo` 里的对端地址，与目的
///   地址无关，所以第一种造法给出的判据与第二种完全等价。
pub async fn start_headless_on(addrs: Vec<SocketAddr>, recording: RecordingSink) -> Harness {
    let cfg_dir = tempfile::tempdir().expect("create isolated temp config dir");
    let config_dir = cfg_dir.path().to_path_buf();
    start_headless_bound(config_dir, Some(cfg_dir), recording, addrs).await
}

/// 真正建服的那一步：地址由调用方给，端口照样传 0 交给内核分配。
async fn start_headless_bound(
    config_dir: PathBuf,
    cfg_owner: Option<tempfile::TempDir>,
    recording: RecordingSink,
    addrs: Vec<SocketAddr>,
) -> Harness {
    assert!(
        !addrs.is_empty(),
        "start_headless_bound 必须至少给一个绑定地址，否则 base_url 无从谈起"
    );

    // 事件出口注入：事件投递在这里被抽成 [`EventSink`] 接缝，headless 下没有 GUI 运行时
    let sink: Arc<dyn EventSink> = Arc::new(recording.clone());

    // headless = 没有 Tauri 运行时；依赖 GUI 的路由取句柄时会 panic，
    // 与 auth 路径无关，后者只走注入的 sink。
    let config = RemoteConfig {
        enabled: true,
        port: 0,
        tailscale_enabled: false,
        ..Default::default()
    };
    // 凭据目录显式指到本夹具的临时目录：配对签发的摘要与 `auth_layer` 验凭据
    // 读的是同一个 `cfg_dir`，所以这条链路从头到尾不碰真实的 `~/.cc-switch`。
    let server = Arc::new(
        RemoteServer::new(RemoteApp::headless(), sink, config, config_dir.clone())
            .expect("headless server must be able to prepare its isolated auth dir"),
    );

    // 端口传 0 → 内核分配临时端口
    let urls = server.start_addrs(addrs).await;
    assert_eq!(
        urls.len(),
        1,
        "headless 启动必须绑定到恰好一个地址，实际: {urls:?}"
    );

    let base_url = urls[0].trim_end_matches('/').to_string();

    // 收集 abort handle 供 Drop 兜底（私有字段，本模块是 remote 的子模块，够得着）
    let abort_handles: Vec<tokio::task::AbortHandle> = {
        let listeners = server.state.listeners.read().await;
        listeners
            .values()
            .map(|e| e.handle.abort_handle())
            .collect()
    };

    Harness {
        base_url,
        cfg_dir: config_dir,
        cfg_owner,
        recording,
        server,
        abort_handles,
    }
}

/// 抢一个当前空闲的本地端口（先绑后放）。
///
/// 只给 E2E 用：`port: 0` 在 Tailscale 档位上会让 loopback 与 tailscale 两个
/// listener 各拿一个**不同**的临时端口，而生产配置里它们是同一个端口。
/// 因此按档位起服务时必须显式给一个真实端口。
pub fn free_port() -> u16 {
    let probe = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("probe listener for a free port");
    let port = probe.local_addr().expect("probe local addr").port();
    drop(probe);
    port
}

/// 按**生产绑定路径**（`compute_target_addrs`）起一个 headless server。
///
/// 与 [`start_headless_on`] 的差别：那里绑定地址由用例直接给；这里给的是
/// `RemoteConfig`，地址从 `bind_scope` 算出来 —— 这正是本文件要验的那段逻辑。
///
/// `ttl` 只对两段式确认的 pending 记录生效；给 `Some` 是为了在几百毫秒内验到
/// 「过期后确认被拒」，而不是真的 sleep 五分钟。
pub async fn start_headless_configured(
    config: RemoteConfig,
    ttl: Option<Duration>,
    recording: RecordingSink,
) -> Harness {
    let cfg_dir = tempfile::tempdir().expect("create isolated temp config dir");
    start_headless_configured_in(
        cfg_dir.path().to_path_buf(),
        Some(cfg_dir),
        config,
        ttl,
        recording,
    )
    .await
}

/// [`start_headless_configured`] 的「目录归调用方」版本：重启用例要复用同一个
/// 凭据目录，不能被 Harness 的 Drop 删掉。
pub async fn start_headless_configured_in(
    config_dir: PathBuf,
    cfg_owner: Option<tempfile::TempDir>,
    config: RemoteConfig,
    ttl: Option<Duration>,
    recording: RecordingSink,
) -> Harness {
    let port = config.port;
    let sink: Arc<dyn EventSink> = Arc::new(recording.clone());
    let server = match ttl {
        Some(ttl) => {
            RemoteServer::new_with_ttl(RemoteApp::headless(), sink, config, config_dir.clone(), ttl)
        }
        None => RemoteServer::new(RemoteApp::headless(), sink, config, config_dir.clone()),
    }
    .expect("headless server must prepare its isolated auth dir");

    let server = Arc::new(server);
    let urls = server
        .start()
        .await
        .expect("server must bind at least one address");

    // `0.0.0.0` 不是客户端能直接用的目的地址：AllInterfaces 档位用回环验证
    // 「服务确实起来了」，非回环目的地址由对应用例单独探测。
    let base_url = urls
        .iter()
        .find(|url| url.contains("127.0.0.1"))
        .cloned()
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));

    let abort_handles: Vec<tokio::task::AbortHandle> = {
        let listeners = server.state.listeners.read().await;
        listeners
            .values()
            .map(|entry| entry.handle.abort_handle())
            .collect()
    };

    Harness {
        base_url,
        cfg_dir: config_dir,
        cfg_owner,
        recording,
        server,
        abort_handles,
    }
}

/// 往夹具的凭据目录里写若干条真实凭据，返回明文 token。
///
/// 走生产同一套 `ClientToken::new` + `save_clients`：两个会话的区分靠的是真实
/// 摘要，而不是测试里编出来的身份字符串。
fn issue_tokens(cfg_dir: &Path, names: &[&str]) -> Vec<String> {
    let secret = load_or_create(cfg_dir).expect("secret in the isolated dir");
    let mut clients = Vec::new();
    let mut tokens = Vec::new();
    for name in names {
        let token = generate_token();
        clients.push(ClientToken::new(name, &secret, &token));
        tokens.push(token);
    }
    save_clients(cfg_dir, &clients).expect("persist client tokens");
    tokens
}

/// 带着 `Authorization: Bearer` 发一个 JSON POST。
async fn post_json_with_token(
    base: &str,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST {path} must reach the live listener: {e}"))
}

/// 断言某个目的地址**确实没人监听**：连接层必须是 `ConnectionRefused`，
/// 而不是「碰巧超时」或「连上后 401」。后两者都不能证明 listener 没绑。
async fn assert_connection_refused(source: IpAddr, destination: SocketAddr) {
    match tcp_connect_from(Some(source), destination).await {
        Err(error) => assert_eq!(
            error.kind(),
            std::io::ErrorKind::ConnectionRefused,
            "{destination} 必须被拒绝连接（listener 不存在），实际错误：{error}"
        ),
        Ok(_) => panic!("{destination} 不应有人监听，实际连上了"),
    }
}

/// 从指定来源地址（`None` = 系统选择）做一次真实 TCP 连接。
///
/// 「连不上」必须是连接层的失败，而不是 HTTP 层 401 —— 本功能的价值全在
/// listener 到底绑没绑；用状态码代替连接结果会把「没绑」验成「绑了但鉴权拒绝」。
async fn tcp_connect_from(
    source: Option<IpAddr>,
    destination: SocketAddr,
) -> std::io::Result<tokio::net::TcpStream> {
    let socket = tokio::net::TcpSocket::new_v4()?;
    if let Some(ip) = source {
        socket.bind(SocketAddr::new(ip, 0))?;
    }
    match tokio::time::timeout(Duration::from_secs(2), socket.connect(destination)).await {
        Ok(result) => result,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "connect timed out",
        )),
    }
}

/// 从指定来源地址发一发 GET，返回 HTTP 状态码。
async fn get_status_from_source(
    source: IpAddr,
    destination: SocketAddr,
    path: &str,
    bearer: Option<&str>,
) -> reqwest::Result<u16> {
    let client = reqwest::Client::builder()
        .local_address(source)
        .timeout(Duration::from_secs(3))
        .build()
        .expect("reqwest client");
    let mut request = client.get(format!("http://{destination}{path}"));
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    request
        .send()
        .await
        .map(|response| response.status().as_u16())
}

/// 与 [`get_status_from_source`] 相同，但对**连接层**失败做有界重试。
///
/// 为什么需要：macOS 的「本地网络」隐私权限 / 路由预热会让进程里**第一条**到
/// 本机非回环地址的连接偶发超时（TCP 已进 backlog，数据发不出去）。这是
/// 操作系统的一次性成本，不是被测代码的行为。重试只发生在 `reqwest::Error`
/// 上，HTTP 状态码本身仍然原样返回 —— 断言没有被放宽。
async fn get_status_from_source_retrying(
    source: IpAddr,
    destination: SocketAddr,
    path: &str,
    bearer: Option<&str>,
) -> u16 {
    let mut last_error = None;
    for attempt in 0..5u64 {
        match get_status_from_source(source, destination, path, bearer).await {
            Ok(status) => return status,
            Err(error) => {
                last_error = Some(error.to_string());
                tokio::time::sleep(Duration::from_millis(200 * (attempt + 1))).await;
            }
        }
    }
    panic!(
        "GET http://{destination}{path} never reached the listener after 5 attempts: {:?}",
        last_error
    );
}

/// 与 [`tcp_connect_from`] 相同，但用于「必须连上」的断言（同一段 OS 预热成本）。
async fn tcp_connect_from_retrying(
    source: Option<IpAddr>,
    destination: SocketAddr,
) -> tokio::net::TcpStream {
    let mut last_error = None;
    for attempt in 0..5u64 {
        match tcp_connect_from(source, destination).await {
            Ok(stream) => return stream,
            Err(error) => {
                last_error = Some(error.to_string());
                tokio::time::sleep(Duration::from_millis(200 * (attempt + 1))).await;
            }
        }
    }
    panic!("{destination} was never reachable after 5 attempts: {last_error:?}");
}

/// 正常收尾：关 listener、join 任务。
pub async fn teardown(h: Harness) {
    h.server
        .stop()
        .await
        .expect("remote server must stop cleanly");
}

// ===========================================================================
// 端到端全链路：配对 → 领 token → 带 token 调用 → SSE → 撤销
// ===========================================================================
//
// 非 mock 清单（这一层存在的全部理由就是「集成之后是不是真的能跑」）：
//
// - 网络：真实 TCP listener + 内核分配的临时端口，客户端是真实 `reqwest`
// - 存储：真实临时目录下的 `remote-auth/secret` 与 `clients.json`
// - 密码学：真实 HMAC-SHA256（[`super::auth`] 的 token 层）
// - 状态机：真实 `Pending → Approved → Consumed`
// - 客户端：真实 HTTP 往返，401/200 是真实状态码
//
// **唯一**替身是 GUI 事件投递：headless 下没有 Tauri 运行时，事件走
// [`EventSink`] 接缝，用记录器接收。事件的内容与生产完全一致 ——
// 走的是同一个 `ApprovalEvent` 构造点，断言的字段就是桌面端会收到的字段。
//
// 端点契约（测试即契约：任一条不符，红的位置就是契约被改动的位置）：
//
// | 方法 | 路径               | 凭据 / 请求体                             | 成功响应 |
// |------|--------------------|-------------------------------------------|----------|
// | POST | `/pairing/request` | `{clientName, requestedScopes}`           | `{requestId, userCode, pollToken}` |
// | POST | `/pairing/poll`    | 头 `Authorization: Pairing <pollToken>`，体空 | `{status, clientId, clientToken}` |
// | POST | `/pairing/approve` | 体 `{userCode}`，**只接 loopback**          | `{approved}` |
// | POST | `/pairing/revoke`  | 体 `{clientId}`，**只接 loopback**          | `{revoked}` |
// | GET  | `/api/*`           | `Authorization: Bearer <token>`，或会话 cookie | 200 |
// | GET  | `/api/events`      | 会话 cookie / 本机                        | SSE 帧流 |
//
// `/api/*` 那行列的是 `Protected` 的两条**任选其一**通道：Bearer 优先，没带
// Bearer 时回落到会话 cookie（远端浏览器打开内嵌 Web UI 之后，页面里的 `fetch`
// 只能靠浏览器自动带 cookie）。两条通道验出来的是同一档 `Cred::Session`，12 格
// 判定表一个出口都没动。**但通道之间不回落**：带了 `Authorization` 就必须它
// 验过，带着一个无效的 Bearer 再挂一条有效 cookie 仍是 401。
//
// `pollToken` **只**从 `Authorization: Pairing …` 进来，请求体是空的且
// `deny_unknown_fields` —— 一次性凭据出现在 URL 里就已经泄了。

/// 把「第几步、做什么、错在哪」绑进失败信息。
///
/// 十步串成一条链，红一条时没有步骤号基本等于让人重跑一遍读代码。
fn step<T, E: std::fmt::Display>(n: u8, what: &str, r: Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("E2E 第 {n} 步（{what}）失败：{e}"),
    }
}

/// `POST` 一个 JSON 文档。
async fn post(base: &str, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}{path}"))
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST {path} must reach the live listener: {e}"))
}

/// 带 `Authorization: Bearer` 打一发请求。
async fn call(base: &str, token: &str, path: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {path} must reach the live listener: {e}"))
}

/// 在 JSON 里找任意层级的、名字像 token 的字段。
///
/// 名字匹配而不是值匹配：值可以随便变形（`access_token` / `clientToken` /
/// `token`），但凭据字段**一定**带 token 这个词。命中就返回该字段名。
fn find_token_shaped_field(v: &serde_json::Value) -> Option<String> {
    // 显式循环而不是 `find_map(self)` / `find_map(闭包)`：两者都会让编译器
    // 把本函数的返回类型和「迭代器项类型」互相代入，撞上 E0275 类型求值溢出。
    let entries: Vec<(&String, &serde_json::Value)> = match v {
        serde_json::Value::Object(m) => m.iter().collect(),
        serde_json::Value::Array(a) => {
            for item in a {
                if let Some(found) = find_token_shaped_field(item) {
                    return Some(found);
                }
            }
            return None;
        }
        _ => return None,
    };

    for (k, val) in entries {
        let lower = k.to_ascii_lowercase();
        if lower.contains("token") && !val.is_null() {
            return Some(k.clone());
        }
        if let Some(found) = find_token_shaped_field(val) {
            return Some(found);
        }
    }
    None
}

/// 轮询一次：凭 `Authorization: Pairing <pollToken>` 打 `POST /pairing/poll`。
///
/// 一次性凭据**只**从这一个头进来，body 是空的 `PairingPollBody`。写成函数是
/// 为了让「第二次 poll 用的确实是同一个 token」在代码上看得见。
async fn poll(base: &str, poll_token: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}/pairing/poll"))
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Pairing {poll_token}"),
        )
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST /pairing/poll must reach the live listener: {e}"))
}

/// 从指定来源地址打一发批准请求，返回状态码。
///
/// 用真实 socket 把**来源**钉在 `source` 上，与 [`status_of`] 同一套做法：
/// 判定的是服务端 `ConnectInfo` 真的读到的对端，不是一个构造出来的 peer。
async fn approve_from(base: &str, source: IpAddr, user_code: &str) -> Option<u16> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr: SocketAddr = base
        .trim_start_matches("http://")
        .parse()
        .unwrap_or_else(|e| panic!("base URL {base} 必须解析成 host:port：{e}"));
    let socket = if source.is_ipv4() {
        tokio::net::TcpSocket::new_v4()
    } else {
        tokio::net::TcpSocket::new_v6()
    }
    .unwrap_or_else(|e| panic!("建 socket 失败（来源 {source}）：{e}"));
    socket
        .bind(SocketAddr::new(source, 0))
        .unwrap_or_else(|e| panic!("来源 {source} bind 失败：{e}"));
    let mut stream = socket
        .connect(addr)
        .await
        .unwrap_or_else(|e| panic!("POST /pairing/approve 必须打到 {addr}（来源 {source}）：{e}"));

    let body = serde_json::json!({ "userCode": user_code }).to_string();
    let request = format!(
        "POST /pairing/approve HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("写批准请求失败：{e}"));

    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut raw)).await;
    let head = String::from_utf8_lossy(&raw);
    head.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
}

/// 带外批准一条待批准的配对请求。
///
/// 「带外」= 走的是桌面端那条批准路径（`POST /pairing/approve`，只接受 loopback
/// 来源），不复用发起方那条会话，因此它天然拒绝「客户端自己给自己批」。
/// 浏览器 E2E 要复用同一路径，故此函数 `pub`：两处 E2E 走同一个
/// 批准入口，才谈得上「同一条路径」。
pub async fn approve_out_of_band(h: &Harness, user_code: &str) {
    let resp = post(
        &h.base_url,
        "/pairing/approve",
        serde_json::json!({ "userCode": user_code }),
    )
    .await;
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    assert_eq!(
        status, 200,
        "第 5 步：approve_out_of_band 必须接受配对码 {user_code}，实际 {status}，body {body}"
    );
}

/// 在 SSE 响应里等第一帧 `data:`，带超时。
///
/// 真读真流：不是断言「有个 handler 存在」，而是真的连上去、真的收帧。
async fn first_sse_frame(resp: reqwest::Response) -> String {
    use futures::StreamExt;

    assert_eq!(
        resp.status().as_u16(),
        200,
        "第 7 步：SSE 订阅必须先鉴权通过，未鉴权时不应拿到事件流"
    );

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("SSE chunk must be readable");
            buf.push_str(&String::from_utf8_lossy(&chunk));
            if let Some((_, rest)) = buf.split_once("data: ") {
                if let Some((frame, _)) = rest.split_once('\n') {
                    return frame.trim().to_string();
                }
            }
            if buf.len() > 8192 {
                break;
            }
        }
        String::new()
    })
    .await;

    match read {
        Ok(frame) if !frame.is_empty() => frame,
        Ok(_) => panic!("第 7 步：SSE 流在 5s 内没有产出任何 data: 帧（收到 {buf:?}）"),
        Err(_) => panic!("第 7 步：SSE 流在 5s 内没有产出任何 data: 帧（超时）"),
    }
}

/// 递归找出临时目录下的某个文件名，返回其权限位。
fn find_file_mode(dir: &std::path::Path, file_name: &str) -> Option<u32> {
    let entries = std::fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(m) = find_file_mode(&p, file_name) {
                return Some(m);
            }
        } else if p.file_name().and_then(|n| n.to_str()) == Some(file_name) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                return p.metadata().ok().map(|m| m.permissions().mode() & 0o777);
            }
            #[cfg(not(unix))]
            return Some(0o600);
        }
    }
    None
}

/// 读出 secret 文件的原始字节，供「secret 不出现在任何响应体」断言用。
async fn secret_bytes(h: &Harness) -> Vec<u8> {
    let dir = h.cfg_dir.clone();
    let mut found: Option<Vec<u8>> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()) == Some("secret") {
                found = std::fs::read(&p).ok();
            }
        }
    }
    found.expect("配对链路必须在临时目录下落出 secret 文件，实际目录没有找到")
}

/// 十步全链路：配对 → 批准 → 领 token → 带 token 调用 → SSE → 撤销。
///
/// 十格与验收清单一一对应，**编号即格号**：红在第几格 == 清单第几项没验成。
///
/// | 格 | 验什么 |
/// |----|--------|
/// | 1 | 非回环来源、无凭据 → protected 业务路由 401 |
/// | 2 | `POST /pairing/request` 拿到 userCode 与 pollToken |
/// | 3 | EventSink 记录器收到**带 peer 地址**的待批事件 |
/// | 4 | 批准前 poll 为 `pending`，且不带任何 token 形态字段 |
/// | 5 | 带外批准后一次性取回明文 token；同一 pollToken 再 poll 失效 |
/// | 6 | 带 token 调业务路由 200，错 token 401 |
/// | 7 | 带凭据打开 `/api/events`，**真实触发**一次切换后收到真实帧 |
/// | 8 | 撤销后同一 token 立即 401（不重启） |
/// | 9 | secret / clients.json 权限 0600，secret 字节不出现在任何响应体 |
/// | 10 | 收尾关 listener、join 任务、端口可重新绑定 |
///
/// 撤销后同一个 token **立刻** 401，且不重启进程 —— 重启能掩盖「撤销只是
/// 让内存缓存失效、落盘没生效」这类只在重启后才暴露的问题。
#[serial_test::serial]
#[tokio::test]
async fn e2e_pair_then_call_then_revoke() {
    // --- 第 1 步：非回环 + 无凭据 → protected 业务路由必须 401 ---
    //
    // 这一格必须自己起一份独立夹具、独立收尾：主 harness 是为「配对 → 领凭据 →
    // 撤销」那串事准备的，而判定表第 7 格对 `Peer::Loopback` 无条件放行（内嵌
    // Web UI 在配对之前就要能打开）—— 于是在同一个回环 base_url 上「无凭据必须
    // 401」根本取不到，补多少遍断言都只是绿。借用别处用例的证更糟：那条用例
    // 不跑时这一格就静默失去覆盖。
    //
    // 让服务端 `ConnectInfo` 真的读成非 loopback 的做法，是把客户端 socket 的
    // **来源**钉在一块真实非回环地址上、目的仍留在回环 listener（[`raw_of`]）。
    // 目的地也换成非回环地址行不通：那一侧要真的上局域网，macOS 的「本地网络」
    // 隐私权限与 CI 沙箱都会把连接掐在半路（[`sources_to_loopback`] 的注释
    // 记的就是这件事，实测裸 tokio listener 在非回环地址上同样收不到回程）。
    // 而判定的输入只有 `ConnectInfo` 里的对端地址，与目的地址无关。
    let nonloop = require_nonloopback().await;
    let probe = start_headless_on(
        vec![SocketAddr::from(([127, 0, 0, 1], 0))],
        RecordingSink::new(),
    )
    .await;
    let (unauth_status, unauth_body) =
        raw_of(&probe.base_url, nonloop.ip, "/api/providers", &[]).await;
    assert_eq!(
        unauth_status, 401,
        "第 1 步：非回环来源（{}）且不带任何凭据时，protected 路由 /api/providers \
         必须 401，实际 {unauth_status}，body {unauth_body}",
        nonloop.ip
    );
    // 独立夹具正常收尾：第 10 格只对主 harness 负责，这条 listener 不许漏。
    teardown(probe).await;

    let h = start_headless(RecordingSink::new()).await;
    let base = h.base_url.clone();

    // 第 6 格与第 8 格都走这条通道：先定下来，后面两格直接用，省得中间换通道
    // 导致两次比较的不是同一件事。
    let gated = gated_channel().await;

    // --- 第 2 步：发起配对 ---
    let r1 = post(
        &base,
        "/pairing/request",
        serde_json::json!({"clientName": "cli", "requestedScopes": ["read"]}),
    )
    .await;
    let s1 = r1.status().as_u16();
    let b1: serde_json::Value = step(2, "POST /pairing/request", r1.json().await);
    assert_eq!(s1, 200, "第 2 步：配对请求必须受理，实际 {s1}，body {b1}");

    let user_code = step(2, "取 userCode", field(&b1, "userCode"));
    let poll_token = step(2, "取 pollToken", field(&b1, "pollToken"));
    let request_id = step(2, "取 requestId", field(&b1, "requestId"));
    assert_eq!(
        user_code.len(),
        6,
        "第 2 步：userCode 是给人敲的配对码，必须 6 位，实际 {user_code:?}"
    );

    // --- 第 3 步：事件出口收到审批事件（唯一替身，但内容与生产一致）---
    let recorded = h.recording.recorded();
    assert_eq!(
        recorded.len(),
        1,
        "第 3 步：配对请求必须经 EventSink 发出且只发出一条审批事件，实际 {}",
        recorded.len()
    );
    assert_eq!(
        recorded[0].user_code, user_code,
        "第 3 步：事件里的 userCode 必须与响应里的一致（桌面端靠它显示配对码）"
    );
    assert_eq!(
        recorded[0].request_id, request_id,
        "第 3 步：事件的 requestId 必须与响应里的一致 —— 桌面端正是靠它把用户敲的 \
         配对码对回这条待批请求，对不上就永远批不中"
    );
    assert_eq!(
        recorded[0].client_name, "cli",
        "第 3 步：事件必须带客户端名"
    );
    // peer 地址：桌面端审批界面上「谁在请求」那一栏读的就是它。空串意味着
    // ConnectInfo 那一层没装配上，而那恰好是最容易在装配改动里悄悄丢掉的接线 ——
    // 事件照样发得出，只是再也说不清是谁在批。
    assert!(
        !recorded[0].evidence.peer_addr.trim().is_empty(),
        "第 3 步：审批事件必须带来源 peer 地址，实际为空串 —— 桌面端没有它就无从判断\
         「这条待批请求是谁发起的」"
    );

    // --- 第 4 步：批准之前 poll 必须是 pending，且不得带任何 token 形态字段 ---
    //
    // 「未批准」必须是**看得见的**状态，而不是一个错误码或一个空响应：客户端
    // 正是靠这个字段决定「继续等」还是「告诉用户去桌面端批一下」。
    let r4 = poll(&base, &poll_token).await;
    let s4 = r4.status().as_u16();
    let b4: serde_json::Value = step(4, "POST /pairing/poll（批准前）", r4.json().await);
    assert_eq!(
        s4, 200,
        "第 4 步：批准前 poll 必须是一个可判定的 200（而不是错误），实际 {s4}，body {b4}"
    );
    assert_eq!(
        step(4, "取 status", field(&b4, "status")),
        "pending",
        "第 4 步：批准前 poll 的 status 必须是 pending，实际 {b4}"
    );
    assert_eq!(
        find_token_shaped_field(&b4),
        None,
        "第 4 步：pending 响应体里不得出现任何 token 形态字段 —— 「还没批」不能被\
         读成「已经有一把凭据了」：{b4}"
    );

    // --- 第 5 步：带外批准 → 一次性取回明文 token ---
    //
    // 批准是 human-in-the-loop 的那一环，因此只有本机能批：局域网里任何一台
    // 机器都能给自己批掉，配对就只剩个形式。下面那条 403 是这一格的前置，
    // 不占格号。本机只有 lo0 时没有第二块真实网卡，可见地 SKIP；批准本身照走
    // 本机通道，十格链路不因此少一格。
    match nonloopback_source().await {
        Ok(src) => assert_eq!(
            approve_from(&base, src.ip, &user_code).await,
            Some(403),
            "第 5 步前置：远端来源批准配对必须 403 —— 批准是给人看的动作，\
             对局域网开放等于任何机器都能给自己批"
        ),
        Err(why) => require_or_skip(&format!(
            "缺少第二个非 loopback 源地址，「远端不得批准」这一前置未运行（{why}）；\
             批准本身仍走本机通道验证"
        )),
    }
    approve_out_of_band(&h, &user_code).await;

    // 状态机 Pending → Approved → Consumed
    let r5 = poll(&base, &poll_token).await;
    let s5 = r5.status().as_u16();
    // 会话 cookie 只在交付凭据的那一发下发，且 body 被读掉就取不到了 —— 先读头。
    let session_cookie = r5
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(super::auth::parse_session_cookie)
        .unwrap_or_else(|| {
            panic!(
                "第 5 步：交付凭据的那一发 poll 必须下发会话 cookie，\
                 否则第 7 格打不开 /api/events 的凭据通道"
            )
        });
    let b5: serde_json::Value = step(5, "POST /pairing/poll（批准后）", r5.json().await);
    assert_eq!(
        s5, 200,
        "第 5 步：批准后 poll 必须换到 token，实际 {s5}，body {b5}"
    );
    assert_eq!(
        step(5, "取 status", field(&b5, "status")),
        "approved",
        "第 5 步：批准后 poll 的 status 必须是 approved，实际 {b5}"
    );

    let client_token = step(5, "取 clientToken", field(&b5, "clientToken"));
    let client_id = step(5, "取 clientId", field(&b5, "clientId"));
    assert!(
        !client_token.is_empty(),
        "第 5 步：批准后必须换到一把真的明文凭据，实际是空串"
    );

    // 同一个 pollToken 第二次 poll 必须失效，且不带 token 形态字段
    let reused = poll(&base, &poll_token).await;
    let s_reused = reused.status().as_u16();
    let b_reused: serde_json::Value = reused.json().await.unwrap_or(serde_json::Value::Null);
    assert!(
        s_reused >= 400,
        "第 5 步：同一 pollToken 第二次 poll 必须失效，实际却返回 {s_reused}，body {b_reused}"
    );
    assert_eq!(
        find_token_shaped_field(&b_reused),
        None,
        "第 5 步：失效响应体里仍带 token 形态字段：{b_reused}"
    );

    // --- 第 6 步：带 token 调业务路由 200；错 token 401 ---
    let (s6, body6) = gated
        .call_with_bearer(&base, &client_token, "/api/providers")
        .await;
    assert_eq!(
        s6, 200,
        "第 6 步：带有效 token 调 /api/providers 必须 200（通道：{}），实际 {s6}，body {body6}",
        gated.label
    );

    // 反向腿：401 必须由「凭据不对」造成，而不是「这条路对谁都不通」。
    let (wrong6, body_wrong6) = gated
        .call_with_bearer(&base, "never-issued-token", "/api/providers")
        .await;
    assert_eq!(
        wrong6, 401,
        "第 6 步：从未签发过的 token 在同一条通道上必须 401（通道：{}），实际 {wrong6}，body {body_wrong6}",
        gated.label
    );

    // --- 第 7 步：带凭据打开 SSE，真实触发一次切换，收到真实帧 ---
    //
    // 订阅先建立、触发后到：`send()` 返回就意味着 handler 已经把 rx 挂上了，
    // 反过来就会撞上「广播早于订阅」的竞态。
    let sse = reqwest::Client::new()
        .get(format!("{base}/api/events"))
        .header(
            axum::http::header::COOKIE,
            format!("cc_switch_session={session_cookie}"),
        )
        .send()
        .await
        .unwrap_or_else(|e| {
            panic!("第 7 步：带会话 cookie 打开 /api/events 必须到达 listener：{e}")
        });

    // 切换需要一个真实存在的目标。种进 headless 那个进程内 `AppState` ——
    // `/api/switch` 的 handler 取的正是同一个实例（同一个 `OnceLock`）。
    let switch_target = seed_headless_provider();

    // live 配置写到隔离 home：切换会把 provider 配置写进 `~/.claude/…`，
    // 测试绝不能拿开发者真实的 CLI 配置开刀。
    let _home = TestHomeGuard::new();

    // 真实触发。不再往 sse_tx 里塞手写 JSON —— 那验的是「广播通道能广播」，
    // 不是「切换会广播」：把 `sse_tx` 换成别的通道、或者 handler 忘了调
    // `state.sse_tx.send`，手写那条一样绿。
    let switched = reqwest::Client::new()
        .post(format!("{base}/api/switch"))
        .bearer_auth(&client_token)
        .json(&serde_json::json!({ "provider_id": switch_target }))
        .send()
        .await;
    // 这一发的**响应**在 headless 下不可信，不是因为切换失败，而是因为
    // handler 的最后一步「刷新托盘」要走 `RemoteApp::Deref` 取 AppHandle
    // ——没有 AppHandle，于是那一行 panic，连接以 502 收场。广播发生在这之前，
    // 所以帧已经发出去了。
    //
    // 因此这里只把响应记成一句话，供下面两条断言的失败信息用（免得有人对着
    // 502 猜「切换是不是失败了」）；真正被钉住的是 SSE 帧里的 provider_id：
    // 那个值只可能来自 handler 读到的请求体 —— 手写的 `sse_tx` 消息给不出它，
    // 切换失败（供应商不存在、落盘出错）也不会广播，同样给不出它。
    let switch_outcome = match switched {
        Ok(resp) => format!("HTTP {}", resp.status()),
        Err(e) => format!("连接在响应前断开（{e}）"),
    };

    let frame = first_sse_frame(sse).await;
    let parsed: serde_json::Value = serde_json::from_str(&frame)
        .unwrap_or_else(|e| panic!("第 7 步：SSE 帧不是合法 JSON（{frame:?}）：{e}"));
    assert_eq!(
        parsed["type"], "switch",
        "第 7 步：必须收到真实 switch 事件帧，实际 {parsed}（触发结果：{switch_outcome}）"
    );
    assert_eq!(
        parsed["provider_id"],
        switch_target.as_str(),
        "第 7 步：帧里的 provider_id 必须就是这次请求切过去的那个 —— 手写 sse_tx 消息\
         给不出这一条（触发结果：{switch_outcome}，实际 {parsed}）"
    );

    // --- 第 8 步：撤销后同 token 立即 401，不重启 ---
    let s8 = post(
        &base,
        "/pairing/revoke",
        serde_json::json!({ "clientId": client_id }),
    )
    .await
    .status()
    .as_u16();
    assert_eq!(s8, 200, "第 8 步：撤销请求必须受理，实际 {s8}");

    let (after, body_after) = gated
        .call_with_bearer(&base, &client_token, "/api/providers")
        .await;
    assert_eq!(
        after, 401,
        "第 8 步：撤销后同一 token 必须**立即** 401（不重启进程），通道：{}，实际 {after}，body {body_after}",
        gated.label
    );

    // 反证：401 只可能来自「凭据被摘掉」。换一个从未签发过的 token，同一条通道
    // 必须得到同一个码 —— 否则 401 也可能是「这条路由对谁都不通」造成的。
    let (never_issued, _) = gated
        .call_with_bearer(&base, "never-issued-token", "/api/providers")
        .await;
    assert_eq!(
        never_issued, 401,
        "第 8 步：从没签发过的 token 在同一条通道上必须同样 401，实际 {never_issued}"
    );

    // 撤销同样摘掉会话 cookie —— 否则「撤销」对 EventSource 型的浏览器调用方
    // 根本不生效，事件流还能接着收。
    let (after_cookie, _) = gated
        .call_with_cookie(&base, &session_cookie, "/api/icon")
        .await;
    assert_eq!(
        after_cookie, 401,
        "第 8 步：撤销后会话 cookie 通道也必须立刻 401 —— 只摘 Bearer 的话，\
         浏览器里的 EventSource 仍然畅通无阻。实际 {after_cookie}"
    );

    // --- 第 9 步：落盘权限 0600，且 secret 字节不出现在任何响应体 ---
    assert_eq!(
        find_file_mode(&h.cfg_dir, "secret"),
        Some(0o600),
        "第 9 步：remote-auth/secret 权限必须是 0600"
    );
    assert_eq!(
        find_file_mode(&h.cfg_dir, "clients.json"),
        Some(0o600),
        "第 9 步：clients.json 权限必须是 0600"
    );
    let secret = secret_bytes(&h).await;
    let secret_str = String::from_utf8_lossy(&secret);
    assert!(
        !secret_str.trim().is_empty(),
        "第 9 步：secret 文件不得为空，否则「不泄露」是空的"
    );
    // 响应体一律按**原始文本**比，而不是解析成 JSON 再比：secret 是原始字节，
    // 解析过程有可能把它悄悄吃掉（例如非法 UTF-8 被替换字符顶掉），那样断言
    // 会在最该红的时候绿。
    let responses: [(&str, String); 7] = [
        ("配对响应", b1.to_string()),
        ("pending 响应", b4.to_string()),
        ("领 token 响应", b5.to_string()),
        ("失效响应", b_reused.to_string()),
        ("第 6 步响应", body6),
        ("错 token 响应", body_wrong6),
        ("撤销后响应", body_after),
    ];
    for (name, body) in &responses {
        assert!(
            !body.contains(secret_str.trim()),
            "第 9 步：{name} 泄露了 secret 字节"
        );
    }

    // --- 第 10 步：正常收尾，端口可重新绑定 ---
    let port: u16 = base
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .expect("base URL 必须以端口结尾，否则第 10 步的「端口可重新绑定」无从验证");
    drop(_home);
    teardown(h).await;
    let rebound = std::net::TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("第 10 步：收尾后端口 {port} 必须可重新绑定：{e}"));
    drop(rebound);
}

/// 在 headless 的进程内 [`crate::store::AppState`] 里种一条可供切换的 provider。
///
/// 走 `handlers::headless_app_state()` 而不是新开一个库：handler 取的正是那个
/// `OnceLock` 里的实例，种到别处去这条记录对它是不可见的 —— 那样的「种好了」
/// 只会让切换以「供应商不存在」失败，而失败路径不广播，于是断言会指向 SSE
/// 而不是「数据没种到」。
fn seed_headless_provider() -> String {
    use crate::provider::Provider;

    let id = "e2e-switch-target".to_string();
    let provider = Provider::with_id(
        id.clone(),
        "E2E 切换目标".to_string(),
        serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://e2e.invalid",
                "ANTHROPIC_AUTH_TOKEN": "e2e-placeholder-token"
            }
        }),
        None,
    );
    super::handlers::headless_app_state()
        .db
        .save_provider("claude", &provider)
        .unwrap_or_else(|e| {
            panic!("第 7 步前置：必须能在 headless AppState 里种下一条可切换的 provider：{e}")
        });
    id
}

/// 边界：同一个 pollToken 第二次 poll 必须失效，且响应体里不带任何 token 形态字段。
#[serial_test::serial]
#[tokio::test]
async fn e2e_poll_token_is_single_use() {
    let h = start_headless(RecordingSink::new()).await;
    let base = h.base_url.clone();

    let r1 = post(
        &base,
        "/pairing/request",
        serde_json::json!({"clientName": "cli", "requestedScopes": ["read"]}),
    )
    .await;
    let b1: serde_json::Value = step(1, "POST /pairing/request", r1.json().await);
    let user_code = step(1, "取 userCode", field(&b1, "userCode"));
    let poll_token = step(1, "取 pollToken", field(&b1, "pollToken"));

    approve_out_of_band(&h, &user_code).await;

    let first = poll(&base, &poll_token).await;
    let s_first = first.status().as_u16();
    let b_first: serde_json::Value = step(2, "首次 poll", first.json().await);
    assert_eq!(
        s_first, 200,
        "首次 poll 必须换到 token，实际 {s_first}，body {b_first}"
    );
    step(2, "取 clientToken", field(&b_first, "clientToken"));

    // 第二次：同一 pollToken 必须失效
    let second = poll(&base, &poll_token).await;
    let s_second = second.status().as_u16();
    let b_second: serde_json::Value = second.json().await.unwrap_or(serde_json::Value::Null);
    assert!(
        s_second >= 400,
        "同一 pollToken 第二次 poll 必须失效，实际却返回 {s_second}，body {b_second}"
    );

    // 且响应体不得含任何 token 形态字段 —— 「失效」不是「换个字段名再发一次」
    assert_eq!(
        find_token_shaped_field(&b_second),
        None,
        "失效响应体里仍带 token 形态字段：{b_second}"
    );

    teardown(h).await;
}

// ===========================================================================
// 重启：secret 逐字节不变，且重启前签发的凭据在新服务里仍然被接受
// ===========================================================================

/// 目录的所有权归夹具，不归任何一个 [`Harness`]。
///
/// `start_headless_in` 刻意不持有删除权（两个服务共用一个目录，而服务 A 会被
/// `teardown` 消费掉），删除责任因此落在这里；用 `TempDir` 而不是裸 `PathBuf`
/// 是为了让断言 panic 展开时目录照样被清掉。
struct OwnedCfgDir(tempfile::TempDir);

impl OwnedCfgDir {
    fn new() -> Self {
        Self(tempfile::tempdir().expect("create isolated temp config dir"))
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

/// 重启前后：secret 文件逐字节相同，且**重启前签发**的 clientToken 仍能打通受保护路由。
///
/// 「重启」为什么不用子进程夹具：`RemoteServer::new` 是 `mod remote` 的私有固有
/// 方法，`src-tauri/tests/` 那个独立 crate 建不出服务。而 `load_or_create` 的
/// 全部路径都由 `config_dir` 入参拼出、没有任何全局状态或进程内缓存，所以
/// 「停掉 A、对同一个目录再起一个 B」在凭据这一侧与「换进程」是同一件事 ——
/// B 是全新构造的 server，`PairingApiState` 与中间件读的都是磁盘上那一份。
///
/// 为什么旧的 `restart_reuses_same_secret_bytes`（secret.rs）不够：它只对同一
/// tempdir 连调两次 `load_or_create` 并比对返回值，证明的是「函数没重写文件」，
/// 完全没有证明**任何一张已签发的凭证**在服务重建后仍被接受 —— 把
/// `load_or_create` 改成每次无条件重新生成，那条单测照样绿，而所有已配对设备
/// 会同时掉线。本条锁的就是后半句。
#[serial_test::serial]
#[tokio::test]
async fn restart_keeps_secret_bytes_and_still_accepts_pre_restart_token() {
    let gated = gated_channel().await;
    let cfg_dir = OwnedCfgDir::new();

    // --- 服务 A：走完配对，拿到一把 clientToken ---
    let a = start_headless_in(cfg_dir.path().to_path_buf(), RecordingSink::new()).await;
    let base_a = a.base_url.clone();

    let requested = post(
        &base_a,
        "/pairing/request",
        serde_json::json!({"clientName": "cli", "requestedScopes": ["read"]}),
    )
    .await;
    let req_body: serde_json::Value = step(1, "POST /pairing/request", requested.json().await);
    let user_code = step(1, "取 userCode", field(&req_body, "userCode"));
    let poll_token = step(1, "取 pollToken", field(&req_body, "pollToken"));

    approve_out_of_band(&a, &user_code).await;

    let polled = poll(&base_a, &poll_token).await;
    let poll_body: serde_json::Value = step(2, "POST /pairing/poll", polled.json().await);
    let client_token = step(2, "取 clientToken", field(&poll_body, "clientToken"));

    let secret_before = secret_bytes(&a).await;
    assert!(
        !secret_before.is_empty(),
        "restart 前置：secret 文件不得为空，否则「逐字节相同」是空的"
    );
    assert_eq!(
        find_file_mode(&a.cfg_dir, "secret"),
        Some(0o600),
        "restart 前置：服务 A 落下的 secret 权限必须是 0600"
    );
    assert_eq!(
        find_file_mode(&a.cfg_dir, "clients.json"),
        Some(0o600),
        "restart 前置：服务 A 落下的凭据库权限必须是 0600"
    );

    // 反证基线：这把 token 在**重启前**确实是通的。没有这一条，后面那个 200 可能
    // 只是因为这条路由对谁都放行（loopback 判定表第 7 格）。
    let (before, body_before) = gated
        .call_with_bearer(&base_a, &client_token, "/api/providers")
        .await;
    assert_eq!(
        before, 200,
        "restart 前置：刚签发的 token 必须先在服务 A 上打通（通道：{}），实际 {before}，body {body_before}",
        gated.label
    );

    // --- 重启：停掉 A，对同一个目录起 B ---
    teardown(a).await;

    let b = start_headless_in(cfg_dir.path().to_path_buf(), RecordingSink::new()).await;

    let secret_after = secret_bytes(&b).await;
    assert_eq!(
        secret_before, secret_after,
        "重启必须逐字节复用同一个 secret：重新生成等于无条件撤销全部已签发凭据"
    );
    assert_eq!(
        find_file_mode(&b.cfg_dir, "secret"),
        Some(0o600),
        "重启后 secret 权限必须仍是 0600"
    );
    assert_eq!(
        find_file_mode(&b.cfg_dir, "clients.json"),
        Some(0o600),
        "重启后凭据库权限必须仍是 0600"
    );

    // 主断言：重启前签发的 token，在**重建后的服务**上仍打得开受保护路由。
    let (after, body_after) = gated
        .call_with_bearer(&b.base_url, &client_token, "/api/providers")
        .await;
    assert_eq!(
        after, 200,
        "重启前签发的 clientToken 在重启后必须仍然可用（通道：{}），实际 {after}，body {body_after}",
        gated.label
    );

    // 反证：这次的 200 来自「凭据被认」，不是「这条路对谁都通」。换一把从未
    // 签发过的 token，同一条通道、同一个路由必须得到 401。
    let (never_issued, _) = gated
        .call_with_bearer(&b.base_url, "never-issued-token", "/api/providers")
        .await;
    assert_eq!(
        never_issued, 401,
        "从未签发过的 token 在同一条通道上必须同样 401，实际 {never_issued}"
    );

    teardown(b).await;
}

// ===========================================================================
// 鉴权分区 E2E：按 scope 遍历真实路由表，逐一断言各自的凭据期望
// ===========================================================================
//
// 三条断言各锁一个方向，缺任何一条都有**错误实现能保持全绿**：
//
// - 没有 Protected 断言 → 把 `Protected` 也挂成 `Public`，远端可匿名调用
//   `/api/switch`、`/api/proxy/control`，所有既有测试仍绿。
// - 没有 BrowserSafe 断言 → 把 `BrowserSafe` 读成「绕过中间件」，`/api/events`
//   在非 loopback 上无凭据可读，构成真实读泄露（订阅到切换事件流），而所有
//   测试仍绿；单元层只验了「手搓的 Router」，验不到真实装配树。
// - 没有 Public 反向断言 → 把配对端点、根页面也挂上鉴权中间件要求凭据，
//   客户端永远拿不到凭据：拿到凭据的唯一入口自己被锁在门外，形成死锁，
//   而「全部 401」看起来反而像更安全。
//
// 三条都经**真实 TCP + 真实中间件**：不用 `oneshot`，因为 `oneshot` 手工塞
// `ConnectInfo` 验的是「我们想让它看到的 peer」，不是「它真的看到的 peer」。

/// 把路由表里某一条路径换成可直接发请求的字面路径。
///
/// `:name` 这类参数段在表里是路由模式，在真实 URL 里必须是个具体值。
fn concretize(path: &str) -> String {
    const SAMPLE: &str = "e2e-probe";
    path.split('/')
        .map(|seg| if seg.starts_with(':') { SAMPLE } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

/// 按 scope 从真实路由表里取出路径。
///
/// 单一数据源：手抄一份路径清单，新增路由时锁会静默放过它 —— 上一轮
/// `ROUTE_SCOPE_TABLE` 门禁已经证明这个失效是真实发生过的。
fn paths_in_scope(scope: AuthScope) -> Vec<String> {
    ROUTE_SCOPE_TABLE
        .iter()
        .filter(|(_, s)| *s == scope)
        .map(|(p, _)| concretize(p))
        .collect()
}

/// 业务路由表里 `Protected` 分区的全部路径。
pub fn protected_paths() -> Vec<String> {
    paths_in_scope(AuthScope::Protected)
}

/// 业务路由表里 `BrowserSafe` 分区的全部路径。
pub fn browser_safe_paths() -> Vec<String> {
    paths_in_scope(AuthScope::BrowserSafe)
}

/// 业务路由表里 `Public` 分区的全部路径。
pub fn public_paths() -> Vec<String> {
    paths_in_scope(AuthScope::Public)
}

/// 不带任何凭据地打一发请求，回状态码。
///
/// 客户端把本地**来源**地址钉死在 `source` 上，目的地址仍是 loopback
/// listener：这样服务端 `ConnectInfo` 读到的对端就是本机那块**非 loopback**
/// 地址，而不是内核随手挑的 127.0.0.1，而整条路径一个包都不出局域网。
///
/// 为什么自己拼 HTTP/1.1 而不用 `reqwest`：reqwest 0.12 的
/// `ClientBuilder::local_address` 在本机被**静默忽略** —— 实测服务端看到的
/// 对端仍是 127.0.0.1（一个监听 loopback 的裸 socket 打点验证过），于是请求
/// 落在 `Peer::Loopback` 那一侧、中间件按内嵌页面放行，断言会「全绿但什么都没
/// 验」。这里宁可手写十行请求，也要让来源地址真的落到 socket 上。
async fn status_without_credentials(base: &str, source: IpAddr, path: &str) -> u16 {
    status_of(base, source, path, &[]).await
}

/// [`status_without_credentials`] 的可带 header 版本：12 格用例要在同一条连接
/// 属性上只改 header（Origin / 代理信号），所以请求行得能被拼装。
///
/// header 逐条原样附在请求行之后，因此名字或值里混进 CR/LF 会把请求拆成两条 ——
/// 那时状态码读回来仍然「合法」，断言却验的是另一发请求。这里显式拒绝。
async fn status_of(base: &str, source: IpAddr, path: &str, headers: &[(&str, &str)]) -> u16 {
    raw_of(base, source, path, headers).await.0
}

/// [`status_of`] 的带响应体版本。
///
/// 第 9 步要断言「secret 字节不出现在**任何**响应体」，只回状态码的话那条
/// 断言就没有可读的东西可比 —— 于是会退化成一句没人验证的空话。
async fn raw_of(base: &str, source: IpAddr, path: &str, headers: &[(&str, &str)]) -> (u16, String) {
    let addr = addr_of(base);
    let mut request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nAccept: */*\r\n");
    append_headers(&mut request, headers);
    request.push_str("Connection: close\r\n\r\n");
    exchange(base, source, &request).await
}

/// [`raw_of`] 的 POST 版本：带请求体与 `Content-Length`。
///
/// 与 GET 版共用同一个 socket 实现（[`exchange`]）—— 两处各长一份的后果是
/// 「GET 那条路径覆盖到了 ConnectInfo 装配、POST 那条没有」，于是所有 POST 级
/// 断言其实跑在另一套装配上，而报告里看不出来。
async fn post_raw_of(
    base: &str,
    source: IpAddr,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let addr = addr_of(base);
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nAccept: */*\r\nContent-Length: {}\r\n",
        body.len()
    );
    append_headers(&mut request, headers);
    request.push_str("Connection: close\r\n\r\n");
    request.push_str(body);
    exchange(base, source, &request).await
}

/// `base_url` → `host:port`。
fn addr_of(base: &str) -> SocketAddr {
    base.trim_start_matches("http://")
        .parse()
        .unwrap_or_else(|e| panic!("base URL {base} 必须解析成 host:port：{e}"))
}

/// 逐条原样附在请求行之后。
///
/// header 名或值里混进 CR/LF 会把请求拆成两条 —— 那时状态码读回来仍然「合法」，
/// 断言却验的是另一发请求。这里显式拒绝。
fn append_headers(request: &mut String, headers: &[(&str, &str)]) {
    for (name, value) in headers {
        assert!(
            !name.contains(['\r', '\n', ':', ' ']) && !value.contains(['\r', '\n']),
            "header {name:?}: {value:?} 不是合法的单行 header，会把一发请求拆成两发"
        );
        request.push_str(&format!("{name}: {value}\r\n"));
    }
}

/// 在钉死的来源上发一串**已经拼好的** HTTP/1.1 请求，返回 `(状态码, 响应全文)`。
///
/// 来源 bind、`Connection: close`、读超时、状态行解析都收在这里，让 GET 与 POST
/// 走同一份实现。
async fn exchange(base: &str, source: IpAddr, request: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = addr_of(base);
    let socket = if source.is_ipv4() {
        tokio::net::TcpSocket::new_v4()
    } else {
        tokio::net::TcpSocket::new_v6()
    }
    .unwrap_or_else(|e| panic!("建 socket 失败（来源 {source}）：{e}"));
    socket
        .bind(SocketAddr::new(source, 0))
        .unwrap_or_else(|e| panic!("来源 {source} bind 失败：{e}"));
    let mut stream = socket
        .connect(addr)
        .await
        .unwrap_or_else(|e| panic!("请求必须打到活着的 listener {addr}（来源 {source}）：{e}"));

    stream
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("写请求失败（来源 {source}）：{e}"));

    // `Connection: close` 让服务端回完就关；真超时也要把已收到的状态行读出来，
    // 否则「handler 挂住」这种失败会伪装成「没收到响应」。
    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut raw)).await;

    let head = String::from_utf8_lossy(&raw);
    let status_line = head.lines().next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| {
            panic!("响应不是合法 HTTP 状态行：{status_line:?}（来源 {source}，原始 {head:?}）")
        });
    (status, head.into_owned())
}

/// 一条**必须持凭据才能通过**的通道。
///
/// 第 6 步与第 8 步（带 token 200 / 撤销后 401）不能用 loopback 打：判定表
/// 第 7~9 格对 loopback 无条件放行，那样的 401 断言验不到任何东西 —— 无论
/// 撤销有没有生效，它都会绿。
///
/// 优先用真实非 loopback 来源（判定表第 1~4 格，那才是「远端客户端」的形态）；
/// 本机只挂 lo0 时退到第 8 格（loopback + 本机代理信号），这一格同样要求
/// `Cred::Token`，因此锁的仍是同一件事：凭据进没进库、撤销有没有生效。
struct GatedChannel {
    source: IpAddr,
    /// 额外请求头（代理信号），非 loopback 侧为空。
    extra: Vec<(&'static str, &'static str)>,
    /// 人话描述，进断言消息。
    label: &'static str,
}

async fn gated_channel() -> GatedChannel {
    match nonloopback_source().await {
        Ok(src) => GatedChannel {
            source: src.ip,
            extra: Vec::new(),
            label: "真实非 loopback 来源",
        },
        Err(_) => GatedChannel {
            source: IpAddr::V4(Ipv4Addr::LOCALHOST),
            extra: vec![(PROXY_SIGNAL, "1")],
            label: "loopback + 本机代理信号（判定表第 8 格）",
        },
    }
}

impl GatedChannel {
    /// 带 Bearer 凭据打一发，返回 (状态码, 响应全文)。
    async fn call_with_bearer(&self, base: &str, token: &str, path: &str) -> (u16, String) {
        let bearer = format!("Bearer {token}");
        let mut headers: Vec<(&str, &str)> = self.extra.iter().map(|(n, v)| (*n, *v)).collect();
        headers.push(("Authorization", bearer.as_str()));
        raw_of(base, self.source, path, &headers).await
    }
}

/// 把「这台机器能不能造出一个非 loopback 来源」变成一条明确的失败。
///
/// 拿不到就红，不静默跳过：跳过等于把这三条锁摘掉还不留痕迹，而「本机没有
/// 第二块网卡」在 CI 上是可以修的（加一张网卡/换一个 runner），不该由一条
/// 悄悄跳过的用例蒙过去。
async fn require_nonloopback() -> SourceAddr {
    nonloopback_source().await.unwrap_or_else(|why| {
        panic!(
            "本机找不到可自连的非 loopback 地址：无法验证「远端无凭据必须 401」。\n\
             这条用例必须走真实 TCP —— 客户端把来源地址钉在一块非 loopback 上，\
             服务端的 ConnectInfo 才会真的落在 NonLoopback 那一侧。\n\
             逐个候选被否的原因：\n  - {why}"
        )
    })
}

/// Protected 分区：非 loopback 无凭据必须逐条 401。
///
/// 方法一律用 GET：本用例断言的是**鉴权层**的出口，而 `register()` 把中间件
/// 挂在整个 `MethodRouter` 上（含 405 的 fallback），所以 POST-only 的路由
/// 用错动词照样先过鉴权层。错动词时若返回 405，说明中间件没盖住 fallback ——
/// 那才是真问题，断言会当场红。
#[serial_test::serial]
#[tokio::test]
async fn protected_routes_all_reject_missing_token() {
    let src = require_nonloopback().await;
    let h = start_headless(RecordingSink::new()).await;

    let paths = protected_paths();
    assert!(
        !paths.is_empty(),
        "ROUTE_SCOPE_TABLE 里必须有 Protected 路由，否则本用例什么都没验"
    );
    for path in paths {
        let status = status_without_credentials(&h.base_url, src.ip, &path).await;
        assert_eq!(
            status, 401,
            "Protected 路由 {path}：非 loopback 且无凭据必须 401（来源 {}），实际 {status}",
            src.ip
        );
    }

    teardown(h).await;
}

/// BrowserSafe 分区：非 loopback 无凭据同样必须 401。
///
/// `BrowserSafe` 的含义是「载体放不了 `Authorization` 头，改走会话 cookie」，
/// 不是「这条路由免鉴权」。
///
/// 没有这条断言时，把它挂成免鉴权是**全绿**的错误实现：`<img src="/api/icon">`
/// 与 `EventSource("/api/events")` 会变成匿名可读口子，后者直接泄露 provider
/// 切换事件流。单元层那条 `browser_safe_is_not_a_bypass` 用的是手搓 Router，
/// 验不到真实装配树上到底挂了什么，所以这里必须再走一遍真实 HTTP。
#[serial_test::serial]
#[tokio::test]
async fn browser_safe_routes_reject_missing_credentials_on_nonloopback() {
    let src = require_nonloopback().await;
    let h = start_headless(RecordingSink::new()).await;

    let paths = browser_safe_paths();
    assert!(
        !paths.is_empty(),
        "ROUTE_SCOPE_TABLE 里必须有 BrowserSafe 路由，否则本用例什么都没验"
    );
    for path in paths {
        let status = status_without_credentials(&h.base_url, src.ip, &path).await;
        assert_eq!(
            status, 401,
            "BrowserSafe 路由 {path}：它只换凭据通道（cookie），不豁免鉴权；\
             非 loopback 且无凭据必须 401（来源 {}），实际 {status}",
            src.ip
        );
    }

    teardown(h).await;
}

/// Public 分区：无凭据必须**不因凭据而**被拒。
///
/// 这里断言的是 `assert_ne!(401)` 而不是 200：Public 只豁免凭据闸门，第①层
/// Origin 否决对它照样生效，所以发一个坏 Origin 仍然是 403。把期望写成 200
/// 会把这个用例变成「顺手锁死了 Origin 语义」的耦合断言。
#[serial_test::serial]
#[tokio::test]
async fn public_routes_do_not_401_for_missing_credentials() {
    let src = require_nonloopback().await;
    let h = start_headless(RecordingSink::new()).await;

    let paths = public_paths();
    assert!(
        !paths.is_empty(),
        "ROUTE_SCOPE_TABLE 里必须有 Public 路由，否则本用例什么都没验"
    );
    for path in paths {
        let status = status_without_credentials(&h.base_url, src.ip, &path).await;
        assert_ne!(
            status, 401,
            "Public 路由 {path}：无凭据时不得 401（来源 {}），否则配对入口把自己锁在门外，\
             客户端永远拿不到凭据",
            src.ip
        );
    }

    teardown(h).await;
}

// ===========================================================================
// 四层规则 E2E：12 格穷尽判定表逐格走真实 TCP
// ===========================================================================
//
// 为什么纯函数表（`policy::tests::case_count_is_twelve`）之外还要再来一遍：
// 那张表验的是「给定 `(peer, origin, proxy_signal, cred)`，`decide` 给出什么
// 出口」。这里验的是另一件事 —— **peer 那一格的值必须来自真实连接属性**。
//
// 用构造的 peer IP 测，「豁免来自实际连接属性」就退化成对 mock 的自证：
// 中间件在某个装配点上拿不到 `ConnectInfo`，`peer_of` 落进 `None => NonLoopback`
// 的最严分支（或者被写死成 `Peer::Loopback`），而测试手里那份「构造出来的远端
// peer」照样喂进判定表，全绿。要排除的正是这种全绿。
//
// 因此本节的每一个 peer 侧都由 socket 决定：
//
// - loopback 侧：内核自己挑来源，连到 127.0.0.1 的 listener；
// - 远端侧：客户端把来源地址 bind 死在 [`second_nonloopback_addr`] 上，目的
//   地址仍是 loopback listener —— 服务端 `ConnectInfo` 读到的一定是那块非
//   loopback 地址，而整条路径一个包都不出本机。
//
// 凭据列（`Cred::Token` / `Cred::Session`）本层不验：E2E 不走配对就发不出真
// 凭据，硬造一个只会验到「我们自己造的东西我们自己认」。凭据三态由纯函数层
// 逐格锁死；本层锁的是**接线** —— 也就是那条「表是对的，但 peer 根本没喂进去」
// 的缝。

/// 探针路由：`Protected` scope，且 handler 不取 `AppState`（headless 下也返回
/// 200），所以每一格的红都只可能红在鉴权层，不会红在 handler 里。
const PROBE_PATH: &str = "/api/health";

/// 白名单 Origin —— **本夹具这份 `RemoteConfig` 算出来的那一条**。
///
/// 端口是 `0`（端口由内核分配，见 `start_headless_bound`），因此 `self_origin`
/// 落在 `http://127.0.0.1:0`；中间件的 Origin 名录与 CORS 的 ACAO 都从
/// `allowed_origins(&cfg, tls)` 算，中间件那份硬编码 host 表已经没有了。
///
/// 刻意不抄一份「看起来对」的 `127.0.0.1:4000`：那等于把本节要验的「两层同源」
/// 换成「测试拿自己抄的字面量跟自己对答案」。字面量与夹具配置之间由
/// `whitelist_origin_matches_the_fixture_config` 钉住。
const ORIGIN_WHITELISTED: &str = "http://127.0.0.1:0";

/// 抄出来的白名单 Origin 字面量不得与夹具配置漂移。
///
/// 值不一致时，红的是这一条（读的人一眼能看出是哪一格漂了），而不是后面
/// 十几格 12 格表集体变成 403。
#[serial_test::serial]
#[test]
fn whitelist_origin_matches_the_fixture_config() {
    let cfg = RemoteConfig {
        port: 0,
        ..Default::default()
    };
    assert!(
        allowed_origins(&cfg, REMOTE_LISTENER_IS_TLS)
            .iter()
            .any(|o| o == ORIGIN_WHITELISTED),
        "{ORIGIN_WHITELISTED:?} 必须在本夹具配置的 allowed_origins 里"
    );
}

/// 非白名单 Origin：host 里含 `127.0.0.1` 但逐字节不等，前缀匹配与包含匹配都
/// 会被骗过 —— 正是判定表要拒的那一格。
const ORIGIN_FOREIGN: &str = "https://evil-127.0.0.1.example";

/// 本机代理的转发信号头 —— 三个标准转发头之一（全部三个见
/// `cell_x_forwarding_headers_are_proxy_signal_but_never_judge_peer` 的
/// `FORWARDING_HEADERS`）。用例里只需一个，取最常被反代自动注入的那个。
///
/// 与 `middleware::LOCAL_PROXY_SIGNALS` 同值（那里是模块私有常量，未 re-export）。
/// 这里刻意抄成字面量而不是共用一个常量：信号头一旦被改名，这份 E2E 必须当场
/// 变红提醒复核，而不是跟着一起改名、静默放过「信号头换了语义」。
const PROXY_SIGNAL: &str = "X-Forwarded-For";

/// 一格的期望出口，与 `policy::Decision` 的两个出口一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// 放行（handler 真的跑了，2xx）
    Allow,
    /// 凭据闸门拒绝
    Unauthorized,
    /// Origin 否决
    ForbiddenOrigin,
}

impl Expect {
    /// 把三态出口描述成断言失败信息里可读的一段。
    fn describe(self) -> &'static str {
        match self {
            Expect::Allow => "放行（2xx）",
            Expect::Unauthorized => "凭据闸门拒绝（401）",
            Expect::ForbiddenOrigin => "Origin 否决（403）",
        }
    }
}

/// 本机 loopback 来源地址。
fn loopback() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

/// 本机第二块（非 loopback）地址，能 bind 当来源且真能把包发到 loopback
/// listener。
///
/// 探测必须走异步：判据是「这块地址能不能真的连上一次」，那要建 socket。
/// 返回 `Option` 而不是 `Result` —— 「本机没有第二块地址」是正常形态（只有
/// lo0 的 CI runner），调用方要的是「这一格要不要跳过」而不是错误详情；需要
/// 详情时用 [`nonloopback_source`]。
///
/// 非 loopback 侧的用例复用这一份探测，别再各写一套。
#[allow(dead_code)] // 暂无生产调用方；先把这份探测固化成契约
pub async fn second_nonloopback_addr() -> Option<IpAddr> {
    nonloopback_source().await.ok().map(|s| s.ip)
}

/// 把「这台机器造不出某个前置条件」变成一次**可见的跳过**而不是一条红。
///
/// 为什么默认不是红：非 loopback 那六格验的是「远端无凭据必被拒」，而它只能靠
/// 一块真实网卡；tailnet 身份那几格则只能靠真装了 Tailscale 的机器。只挂 lo0、
/// 没装 Tailscale 的 CI runner 上硬红，会让整条锁永远挂着，看起来像代码坏了，
/// 而它其实什么都没验。
///
/// 为什么**必须**是可见的一行：一次「四分之三判据没跑」的运行，与一次「全跑全绿」
/// 在 CI 界面上看起来一模一样。静默 `return` 等于把「没跑」伪装成「过了」——
/// 而这份报告正是本任务要消灭的那种东西。
///
/// [`REQUIRE_TAILNET_ENV`] 存在的理由：光靠「打印一行」仍然允许一个**专门要跑
/// 这一侧**的 job 被静默摘掉一半锁而无人察觉。设了它的 job 里，跳过一律变成红：
/// 那个 job 的全部意义就是验这一侧，它没有跳过的余地。
///
/// 可见性：`cargo test` 默认吞掉通过用例的 stdout，CI 要看到这行得加
/// `--nocapture` / `--show-output`。所以 stdout 与 stderr 各打一份。
fn require_or_skip(msg: &str) {
    let line = format!("SKIP: {msg}");
    match skip_line(msg, required_tailnet()) {
        SkipAction::Print(printed) => {
            // stdout 与 stderr 各打一份：通过用例的 stdout 默认被吞，只有 stderr
            // 一定进得了 CI 日志。
            println!("{printed}");
            eprintln!("{printed}");
        }
        SkipAction::Fail(fatal) => {
            // 先打再炸：即使这条用例红了，日志里也留着「本来要 SKIP 的是什么」。
            eprintln!("{line}");
            panic!("{fatal}");
        }
    }
}

/// 这条跳过该怎么落地。单独拆出来是为了能**纯函数地**断言两个分支，而不必去抓
/// stdout —— 「跳过必须可见」判的是消息的形状，不是它有没有真的出现在某个 fd 上。
#[derive(Debug, Clone, PartialEq, Eq)]
enum SkipAction {
    /// 打印这一行，让调用方跳过。
    Print(String),
    /// 环境要求这一侧必须跑到，跳过即失败。
    Fail(String),
}

/// 把「缺什么」渲染成**唯一**那行 `SKIP: …`。所有跳过出口都必须经过它，
/// 否则迟早有人手写一句 `println!` 少掉前缀，报告里就再也 grep 不到了。
fn skip_line(msg: &str, require: bool) -> SkipAction {
    let line = format!("SKIP: {msg}");
    if require {
        SkipAction::Fail(format!(
            "{line} —— 但本 job 设了 {REQUIRE_TAILNET_ENV}=1，跳过不计为通过"
        ))
    } else {
        SkipAction::Print(line)
    }
}

/// 专用 job 用它把跳过升级成失败。
pub const REQUIRE_TAILNET_ENV: &str = "CC_SWITCH_REQUIRE_TAILNET";

/// 是否处在「这一侧必须真跑」的专用 job 里。
fn required_tailnet() -> bool {
    required_by(std::env::var(REQUIRE_TAILNET_ENV).ok().as_deref())
}

/// [`required_tailnet`] 的可注入版本，单独拆出来是为了能纯函数地验取值规则。
///
/// **只认 `1`**：别的取值（CI 里很常见的 `true`、以及 `1 ` 这种带空白的拼写错误）
/// 一律当作没设。宽一点看着更友好，实际后果是「有人把值拼错 → 专用 job 静默退回
/// 成跳过」，而这正是本节要消灭的那件事。
fn required_by(raw: Option<&str>) -> bool {
    matches!(raw, Some("1"))
}

/// 能力探测结果：这份机器上哪些前置条件**真的**在。
///
/// 为什么要有这么一个汇总：每条用例各自去 `tailscale_bin(...)` 或
/// `second_nonloopback_addr()` 试一遍，等于每次探测各自解释「缺了算不算红」，
/// 于是有的出口跳、有的出口静默 return、有的出口硬红 —— 判断散在调用点就等于
/// 没有判断。集中到这里之后，**出口只有 [`require_or_skip`] 一个**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// 本机能不能解析出可执行的 `tailscale`。
    ///
    /// 只判「二进制在不在」，**不**跑 `tailscale status`、不读任何 tailnet
    /// 身份字段 —— 一条「这台机器有没有节点」的断言本身就会诱导有人把真实
    /// `StableID` 抄进夹具（门禁见 `tests/privacy_gate.rs`）。
    pub tailscale_available: bool,
    /// 本机第二块非 loopback 地址；只有 lo0 的机器是 `None`。
    pub second_nonloopback: Option<IpAddr>,
}

impl Capabilities {
    /// 缺了什么，写成一行能直接进 `SKIP:` 的话；什么都不缺时是 `None`。
    pub fn missing(&self) -> Option<String> {
        let mut miss = Vec::new();
        if !self.tailscale_available {
            miss.push("解析不到可执行的 tailscale 二进制（tailnet 身份那几格需要它）");
        }
        if self.second_nonloopback.is_none() {
            miss.push("缺少第二个非 loopback 源地址（远端无凭据必被 401 那六格需要它）");
        }
        (!miss.is_empty()).then(|| miss.join("；"))
    }
}

/// 统一的能力探测：两条前置条件各探一次。
///
/// **同步**、且刻意是粗粒度的：它只用来决定「这一格要不要跳过」，判定式是
/// 「本机到底有没有第二块非 loopback 路由」—— 那只要一个 UDP `connect`（内核做
/// 路由选择，一个包都不发）加一次 `local_addr()`，不需要建 TCP 连接，所以不碰
/// async。
///
/// 真正的判据仍然是 [`nonloopback_source`]：它额外验「这块地址能真的连上一次
/// loopback listener」（tun/VPN 网卡能 bind 却发不出包）。所以这里的 `Some` 只是
/// 「值得一试」，不是「一定可用」—— 两者的分工写在这里，免得有人拿 `capabilities()`
/// 去断言某一格真的验过了。
pub fn capabilities() -> Capabilities {
    Capabilities {
        tailscale_available: super::whois::resolve_bin().is_some(),
        second_nonloopback: probe_nonloopback_addrs().into_iter().next(),
    }
}

/// 所有非 loopback 的本机地址，按探测顺序。
///
/// 与 [`probe_targets`] 配套：UDP 的 `connect` 只做路由选择，一个包都不发，
/// 于是 `local_addr()` 就是「去那儿会从哪个地址出发」。
fn probe_nonloopback_addrs() -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = Vec::new();
    for target in probe_targets() {
        let Ok(sock) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
            continue;
        };
        if sock.connect(SocketAddr::from((target, 9))).is_err() {
            continue;
        }
        let Ok(local) = sock.local_addr() else {
            continue;
        };
        let ip = local.ip();
        if ip.is_loopback() || ip.is_unspecified() || out.contains(&ip) {
            continue;
        }
        out.push(ip);
    }
    out
}

/// 取一块可用的非 loopback 来源；本机没有就经唯一出口打印 SKIP 并返回 `None`。
async fn remote_source_or_skip() -> Option<SourceAddr> {
    match nonloopback_source().await {
        Ok(src) => Some(src),
        Err(why) => {
            require_or_skip(&format!(
                "缺少第二个非 loopback 源地址（探测器逐个否掉了所有候选：{why}）"
            ));
            None
        }
    }
}

/// 缺环境必须被**报出来**，不能静默 `return`。
///
/// 这条用例钉的是本节唯一的出口契约：缺什么、要跳哪几格、CI 报告里长什么样。
/// 它不依赖本机有没有第二块网卡 —— 判据跑在纯函数上，所以任何一台机器（哪怕
/// 只有 lo0）都能验「跳过是可见的」。
#[serial_test::serial]
#[test]
fn skip_is_reported_not_silent() {
    const MISSING: &str = "缺少第二个非 loopback 源地址";

    // 默认 job：可见地打印，然后真的跳过。
    assert_eq!(
        skip_line(MISSING, false),
        SkipAction::Print(format!("SKIP: {MISSING}")),
        "不要求 tailnet 的 job 里，跳过必须渲染成一行 `SKIP: <缺什么>`"
    );

    // 报告里能 grep 的东西是那一行本身，所以前缀与正文都不得为空。
    let SkipAction::Print(line) = skip_line(MISSING, false) else {
        unreachable!("上面已断言这是 Print 分支")
    };
    assert!(line.starts_with("SKIP: "), "行首必须是 `SKIP: `：{line}");
    assert!(
        line.trim().len() > "SKIP:".len(),
        "SKIP 行必须说明缺了什么，空正文等于没说：{line}"
    );
    // 空消息也不许渲染成光秃秃的 `SKIP:` —— 那正是「看不出跳了什么」的形态。
    assert_ne!(
        skip_line("", false),
        SkipAction::Print("SKIP:".to_string()),
        "空缺项不得退化成没有正文的 SKIP 行"
    );

    // 专用 job：跳过一律变红，且红消息里仍留着 SKIP 那一行。
    let SkipAction::Fail(fatal) = skip_line(MISSING, true) else {
        panic!("设了 {REQUIRE_TAILNET_ENV}=1 的 job 里，跳过必须计为失败")
    };
    assert!(
        fatal.contains("SKIP: "),
        "失败消息里必须仍带着 SKIP 那一行，否则日志看不出本来要跳过什么：{fatal}"
    );
    assert!(
        fatal.contains(REQUIRE_TAILNET_ENV),
        "失败消息必须点名是哪个开关把跳过升级成失败的：{fatal}"
    );

    // 开关只认 `=1`：别的取值（例如 CI 里常见的 `true`）不改变处置，否则「有人
    // 把值拼错」会让专用 job 静默退回成跳过 —— 而那与一开始不设这个开关没区别。
    assert!(required_by(Some("1")), "只有 `1` 才要求 tailnet");
    for raw in [
        None,
        Some(""),
        Some("0"),
        Some("true"),
        Some("TRUE"),
        Some("1 "),
        Some("01"),
    ] {
        assert!(
            !required_by(raw),
            "{raw:?} 不该被读成「要求 tailnet」—— 拼错的开关值会让专用 job 静默退回成跳过"
        );
    }

    // `require_or_skip` 本身走一遍：默认路径上它必须跑完并返回，而不是把消息吞掉。
    // 专用 job 里它**应该** panic —— 那正是它的职责，所以这里不把它当成失败：
    // 一条在专用 job 里恒红的自检会把「这条用例坏了」和「tailnet 侧真的缺东西」
    // 两种完全不同的信号混在一起。
    if !required_tailnet() {
        require_or_skip(MISSING);
    }

    // 能力探测只报告「有没有」，不泄任何 tailnet 身份值：字段里只有 bool 与地址。
    let caps = capabilities();
    assert_eq!(
        caps.tailscale_available,
        super::whois::resolve_bin().is_some(),
        "capabilities() 必须与真正的解析结果一致，否则「跳过了哪几格」就是猜的"
    );
    if let Some(ip) = caps.second_nonloopback {
        assert!(
            !ip.is_loopback() && !ip.is_unspecified(),
            "报出来的第二地址 {ip} 不是非 loopback —— 这会让「远端那六格」的跳过理由失真"
        );
    }
    // 缺失清单与实测字段严格一致：两样都不缺时必须是 `None`。
    assert_eq!(
        caps.missing().is_none(),
        caps.tailscale_available && caps.second_nonloopback.is_some(),
        "missing() 与字段不自洽：报告的跳过理由会跟实际缺的东西对不上"
    );
}

/// 把一格的三元组渲染成断言消息里的人话。
fn describe_cell(no: u8, source: IpAddr, origin: Option<&str>, proxy: bool) -> String {
    let side = if source.is_loopback() {
        "loopback"
    } else {
        "非 loopback"
    };
    format!(
        "第 {no} 格（peer={source}={side}，Origin={}，代理信号={proxy}）",
        origin.unwrap_or("无")
    )
}

/// 打一格：只改 header，peer 侧由 socket 决定。
async fn assert_cell(
    h: &Harness,
    no: u8,
    source: IpAddr,
    origin: Option<&'static str>,
    proxy: bool,
    expect: Expect,
) {
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(o) = origin {
        headers.push(("Origin", o));
    }
    if proxy {
        headers.push((PROXY_SIGNAL, "1"));
    }
    let cell = describe_cell(no, source, origin, proxy);
    let status = status_of(&h.base_url, source, PROBE_PATH, &headers).await;
    match expect {
        // 放行这一侧只断言 2xx：锁的是「闸门没拦住」，不是 handler 的响应体形状
        // （那是 handler 自己的测试范围）。
        Expect::Allow => assert!(
            (200..300).contains(&status),
            "{cell}：{}，实际 {status}。这条路由是 Protected 的内嵌页面形态，\
             被拦下就说明本机的内嵌 Web UI 打不开了",
            expect.describe()
        ),
        Expect::Unauthorized => {
            assert_eq!(status, 401, "{cell}：{}，实际 {status}", expect.describe())
        }
        Expect::ForbiddenOrigin => assert_eq!(
            status,
            403,
            "{cell}：{}，实际 {status}。Origin 否决必须在凭据闸门之前，且三类 \
             channel 一视同仁",
            expect.describe()
        ),
    }
}

/// 起一份夹具、验一格、收尾：loopback 侧（CI 必跑，不依赖任何网卡）。
///
/// 这里的 peer 不是「我们声称它是 loopback」，而是内核给一个连到 loopback
/// listener 的连接挑的来源地址 —— 也就是 `ConnectInfo` 真正能读到的那个值。
async fn loopback_cell(no: u8, origin: Option<&'static str>, proxy: bool, expect: Expect) {
    let h = start_headless(RecordingSink::new()).await;
    assert_cell(&h, no, loopback(), origin, proxy, expect).await;
    teardown(h).await;
}

/// 同上，远端侧：来源地址钉在真实非 loopback 地址上。
///
/// 本机没有可用地址时打印 SKIP 后直接返回 —— 绝不退化成构造的 peer IP。
async fn remote_cell(no: u8, origin: Option<&'static str>, proxy: bool, expect: Expect) {
    let Some(src) = remote_source_or_skip().await else {
        return;
    };
    let h = start_headless(RecordingSink::new()).await;
    assert_cell(&h, no, src.ip, origin, proxy, expect).await;
    teardown(h).await;
}

// ---------------------------------------------------------------------------
// 第 1~6 格：非 loopback。本机第二块地址不在就跳过。
// ---------------------------------------------------------------------------

/// 第 1 格：远端 + 无 Origin + 无代理信号 + 无凭据 → 401。
///
/// 这是整条链路最要紧的一格：局域网里的任何一台机器都不能匿名调用受保护
/// 路由。403 也不行 —— 403 说的是「Origin 不对」，401 说的是「你没证明你是谁」，
/// 后者才是这里的事实。
#[serial_test::serial]
#[tokio::test]
async fn cell_01_remote_no_origin_no_proxy_is_401() {
    remote_cell(1, None, false, Expect::Unauthorized).await;
}

/// 第 2 格：远端 + 无 Origin + 有代理信号 → 401。
///
/// 经本机代理转发不降低要求：代理替谁转发，它自己也不知道。
#[serial_test::serial]
#[tokio::test]
async fn cell_02_remote_no_origin_with_proxy_signal_is_401() {
    remote_cell(2, None, true, Expect::Unauthorized).await;
}

/// 第 3 格：远端 + 白名单 Origin + 无代理信号 → 401。
///
/// Origin 只影响第①层否决，通过之后仍然要过凭据闸门。
#[serial_test::serial]
#[tokio::test]
async fn cell_03_remote_whitelisted_origin_no_proxy_is_401() {
    remote_cell(3, Some(ORIGIN_WHITELISTED), false, Expect::Unauthorized).await;
}

/// 第 4 格：远端 + 白名单 Origin + 有代理信号 → 401。
#[serial_test::serial]
#[tokio::test]
async fn cell_04_remote_whitelisted_origin_with_proxy_signal_is_401() {
    remote_cell(4, Some(ORIGIN_WHITELISTED), true, Expect::Unauthorized).await;
}

/// 第 5 格：远端 + 非白名单 Origin + 无代理信号 → 403。
#[serial_test::serial]
#[tokio::test]
async fn cell_05_remote_foreign_origin_is_403() {
    remote_cell(5, Some(ORIGIN_FOREIGN), false, Expect::ForbiddenOrigin).await;
}

/// 第 6 格：远端 + 非白名单 Origin + 有代理信号 → 403。
///
/// 代理信号不是凭据，不能给非白名单 Origin 翻案。
#[serial_test::serial]
#[tokio::test]
async fn cell_06_remote_foreign_origin_with_proxy_signal_is_403() {
    remote_cell(6, Some(ORIGIN_FOREIGN), true, Expect::ForbiddenOrigin).await;
}

// ---------------------------------------------------------------------------
// 第 7~12 格：loopback。这六格在任何 runner 上都必须真跑，不依赖网卡。
// ---------------------------------------------------------------------------

/// 第 7 格：loopback + 无 Origin + 无代理信号 → 放行（真实 TCP）。
///
/// 这正是内嵌 Web UI / Tauri webview 的形态，也是原四层写法漏掉的空档。
#[serial_test::serial]
#[tokio::test]
async fn cell_07_loopback_no_origin_no_proxy_allows_real_tcp() {
    loopback_cell(7, None, false, Expect::Allow).await;
}

/// 第 8 格：loopback + 无 Origin + **有**代理信号 → 401。
///
/// 本机代理在替别人转发，它不能裸奔：loopback 被代理信号抬升为远程通道。
#[serial_test::serial]
#[tokio::test]
async fn cell_08_loopback_no_origin_with_proxy_signal_is_401() {
    loopback_cell(8, None, true, Expect::Unauthorized).await;
}

/// 第 9 格：loopback + 白名单 Origin + 无代理信号 → 放行（真实 TCP）。
///
/// 与第 7 格同出口：白名单 Origin 不改变通道，只是不触发第①层否决。
#[serial_test::serial]
#[tokio::test]
async fn cell_09_loopback_whitelisted_origin_no_proxy_allows_real_tcp() {
    loopback_cell(9, Some(ORIGIN_WHITELISTED), false, Expect::Allow).await;
}

/// 第 10 格：loopback + 白名单 Origin + 有代理信号 → 401。
///
/// 与第 8 格同出口：白名单 Origin 不能把代理信号带来的要求抹掉。
#[serial_test::serial]
#[tokio::test]
async fn cell_10_loopback_whitelisted_origin_with_proxy_signal_is_401() {
    loopback_cell(10, Some(ORIGIN_WHITELISTED), true, Expect::Unauthorized).await;
}

/// 第 11 格：loopback + 非白名单 Origin → 403。
///
/// Origin 否决在最前：来源是本机也不例外，且凭据 / 代理信号都救不了。
#[serial_test::serial]
#[tokio::test]
async fn cell_11_loopback_foreign_origin_is_403() {
    loopback_cell(11, Some(ORIGIN_FOREIGN), false, Expect::ForbiddenOrigin).await;
}

/// 第 12 格：loopback + 非白名单 Origin + 有代理信号 → 403。
#[serial_test::serial]
#[tokio::test]
async fn cell_12_loopback_foreign_origin_with_proxy_signal_is_403() {
    loopback_cell(12, Some(ORIGIN_FOREIGN), true, Expect::ForbiddenOrigin).await;
}

// ---------------------------------------------------------------------------
// 反证：peer 判据真的来自连接属性
// ---------------------------------------------------------------------------

/// 反证：peer 判据真的来自连接属性，而不是被某处写死或掐断。
///
/// 反证对象是「peer 那一格的值与真实对端无关」这一类错误实现。逐条变异都实测
/// 过（改完跑一遍、看哪些用例转红、改回去），覆盖矩阵如下：
///
/// | 变异                                        | 转红的用例 |
/// |---------------------------------------------|------------|
/// | `peer_of` ≡ `Peer::Loopback`                | 第 1/3 格、`cell_x_*`、本用例方向三 |
/// | `peer_of` ≡ `Peer::NonLoopback`             | 第 7/9 格、`cell_07b`、`cell_x_*`、本用例方向一 |
/// | `peer_of` 改读 `X-Forwarded-For`            | 同上，另加 `cell_x_*` 的伪造侧 |
/// | 去掉 `into_make_service_with_connect_info`   | 同上（`None` 分支落到最严的 NonLoopback） |
///
/// **覆盖上有一处诚实的边界**：常量 `Peer::Loopback` 只能被**非 loopback 那一侧**
/// 抓住（第 1~6 格 / 本用例方向三）。原因不是漏写，而是它本来就抓不住：只看
/// loopback 上的流量时，「peer 真的是 loopback」与「peer 被写死成 loopback」
/// 产生**完全相同**的观测，任何断言都无法区分。所以这一格必须靠第二块网卡上的
/// 真实连接去分辨 —— 而本机没有第二块地址时按约定 SKIP，不拿 mock 顶替。
/// 其余三种变异只需要 loopback 流量就能抓住，因此第 7~12 格在任何 runner 上
/// 都真跑，不依赖网卡。
#[serial_test::serial]
#[tokio::test]
async fn connect_info_is_actually_used() {
    let h = start_headless(RecordingSink::new()).await;

    // --- 方向一：不装 ConnectInfo / 写死成 NonLoopback，会在这里红 ---
    let embedded = status_of(&h.base_url, loopback(), PROBE_PATH, &[]).await;
    assert!(
        (200..300).contains(&embedded),
        "loopback + 无代理信号必须放行，实际 {embedded}。这条是本机内嵌 Web UI / \
         Tauri webview 的真实形态：拿不到 ConnectInfo 时 peer 落到最严分支，桌面端 \
         自己的页面会先于任何远端请求打不开"
    );
    let via_proxy = status_of(&h.base_url, loopback(), PROBE_PATH, &[(PROXY_SIGNAL, "1")]).await;
    assert_eq!(
        via_proxy, 401,
        "loopback + 代理信号必须 401，实际 {via_proxy}"
    );
    let via_proxy_whitelisted = status_of(
        &h.base_url,
        loopback(),
        PROBE_PATH,
        &[(PROXY_SIGNAL, "1"), ("Origin", ORIGIN_WHITELISTED)],
    )
    .await;
    assert_eq!(
        via_proxy_whitelisted, 401,
        "loopback + 代理信号 + 白名单 Origin 必须 401，实际 {via_proxy_whitelisted}"
    );

    // --- 方向三：常量 Peer::Loopback 只能在这里红（需要第二块真实网卡）---
    let Some(src) = remote_source_or_skip().await else {
        teardown(h).await;
        return;
    };
    let remote = status_of(&h.base_url, src.ip, PROBE_PATH, &[]).await;
    assert_eq!(
        remote, 401,
        "同一路由、同一个 header，来源从 loopback 换成真实非 loopback（{}）后出口必须 \
         从 2xx 翻成 401，实际 {remote}。不变就说明 peer 判据没有来自真实连接属性",
        src.ip
    );

    teardown(h).await;
}

/// 第 7 格的 GET 无副作用：连打 N 次，状态快照与 N=0（第一次）一致。
///
/// 12 格用例全靠重复 GET 取状态码。若某一格悄悄改了状态（比如一次之后闸门就
/// 记住了什么），「第一次通过」和「第二十次通过」会指向不同的东西，而单次断言
/// 永远看不到。
#[serial_test::serial]
#[tokio::test]
async fn cell_07b_loopback_repeated_gets_keep_status_stable() {
    const REPEATS: usize = 5;

    let h = start_headless(RecordingSink::new()).await;
    let baseline = status_of(&h.base_url, loopback(), PROBE_PATH, &[]).await;
    assert!(
        (200..300).contains(&baseline),
        "第 0 次 GET {PROBE_PATH} 必须放行，实际 {baseline}"
    );
    for n in 1..=REPEATS {
        let status = status_of(&h.base_url, loopback(), PROBE_PATH, &[]).await;
        assert_eq!(
            status, baseline,
            "GET {PROBE_PATH} 是只读的：第 {n} 次的状态必须与第 0 次一致，\
             实际 {status} vs {baseline}"
        );
    }

    teardown(h).await;
}

/// 标准转发头**是**代理信号，但**不是** peer 的来源。
///
/// `X-Forwarded-For` / `Forwarded` / `Tailscale-User-Login` 都是**请求方完全
/// 可控**的字段，三方各自真发一次，两条边界都要钉住：
///
/// - **是代理信号**：头在场即把 loopback 请求从第 7 格（免凭据）抬到第 8 格
///   （要凭据）→ 出口必须翻成 **401**。这是判定表本身的要求，也是本用例与
///   `connect_info_is_actually_used` 的分工所在：那里的 `PROXY_SIGNAL` 只发一个，
///   这里三个各发一次，证明三者在判定表里等价。
/// - **不是 peer 的来源**：`peer_of` 只读 `ConnectInfo`，任何请求头都不参与。
///   远端侧带 `X-Forwarded-For: 127.0.0.1` 必须**仍是 401** —— 一旦有人改成读
///   这些头，远端就能靠一行伪造把自己降级成本机页面，那才是真正的洞。
///
/// 三条断言里的取值一律是编的：判据只看头在不在场，与取值无关。
#[serial_test::serial]
#[tokio::test]
async fn cell_x_forwarding_headers_are_proxy_signal_but_never_judge_peer() {
    /// 三个头各配一个「冒充远端」的值 —— 值本身不参与判定，只是为了让
    /// 「取值不影响结论」这句话在失败信息里看得见。
    const FORWARDING_HEADERS: [(&str, &str); 3] = [
        ("X-Forwarded-For", "203.0.113.7"),
        (
            "Forwarded",
            "for=203.0.113.7;proto=https;host=cc-switch.example",
        ),
        ("Tailscale-User-Login", "attacker@evil.example"),
    ];

    let h = start_headless(RecordingSink::new()).await;

    // loopback 侧：带任一转发头即落第 8 格（本机代理通道），免凭据的豁免收回
    let baseline = status_of(&h.base_url, loopback(), PROBE_PATH, &[]).await;
    assert!(
        (200..300).contains(&baseline),
        "前置：loopback 无代理信号必须放行（第 7 格），实际 {baseline}"
    );
    for (name, value) in FORWARDING_HEADERS {
        let status = status_of(&h.base_url, loopback(), PROBE_PATH, &[(name, value)]).await;
        assert_eq!(
            status, 401,
            "loopback 请求带上 {name}: {value} 后必须落第 8 格（本机代理通道，无凭据 \
             → 401），实际 {status}。判据是头在场；取值不影响结论"
        );
    }

    // 远端侧：冒充 loopback 的值不得压低通道
    let Some(src) = remote_source_or_skip().await else {
        require_or_skip(
            "缺少第二个非 loopback 源地址，远端侧的伪造验证未运行（loopback 侧已验完）",
        );
        teardown(h).await;
        return;
    };
    let spoofed_local = status_of(
        &h.base_url,
        src.ip,
        PROBE_PATH,
        &[
            ("X-Forwarded-For", "127.0.0.1"),
            ("Forwarded", "for=127.0.0.1"),
            ("Tailscale-User-Login", "127.0.0.1"),
        ],
    )
    .await;
    assert_eq!(
        spoofed_local, 401,
        "真实非 loopback 来源（{}）伪造 loopback 转发头后必须仍是 401，实际 \
         {spoofed_local}",
        src.ip
    );

    teardown(h).await;
}

/// 从 JSON 里取一个字符串字段，缺失时报出字段名。
fn field(v: &serde_json::Value, name: &str) -> Result<String, String> {
    v.get(name)
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("响应体缺少字符串字段 {name}：{v}"))
}

/// headless（无 `AppHandle`）下 `/api/providers` 必须真的被调用成功。
///
/// 这条锁的是「无头取状态接缝」：`app_state()` 经 `RemoteApp` 的 `Deref`
/// 取 `AppState` 在 headless 下会 panic，请求根本走不完 handler —— panic 发生在
/// 连接里，客户端只看到连接被断开或 5xx。所以这里断言的是**状态码 200**，
/// 而不是「某个函数存在」。
///
/// 请求不带 `Origin`、也不带凭据，走的是 loopback + 无 Origin 那一格：
/// `/api/providers` 的 scope 仍是 `Protected`，放行靠的是 peer 是 loopback，
/// 不是「因为是测试所以放宽」。
#[serial_test::serial]
#[tokio::test]
async fn headless_app_state_serves_providers() {
    let h = start_headless(RecordingSink::new()).await;

    let resp = reqwest::Client::new()
        .get(h.url("/api/providers"))
        .send()
        .await
        .expect("headless server must answer /api/providers");

    assert_eq!(
        resp.status().as_u16(),
        200,
        "无 AppHandle 时 GET /api/providers 必须 200：取状态的路径不能靠 \
         RemoteApp 的 Deref（headless 下它会 panic）"
    );
    let body: serde_json::Value = resp.json().await.expect("providers returns JSON");
    assert!(
        body.get("providers").is_some(),
        "响应必须是 provider 列表信封，实际: {body}"
    );

    teardown(h).await;
}

// ===========================================================================
// 边界 E2E ①：凭据文件权限 fail-closed —— 「端口从未被绑定」由真实占用给出
// ===========================================================================
//
// 「凭据文件权限过宽就不许起服务」这件事，`secret.rs` 的单元测试验的是
// `load_or_create` 返回 `BadPermissions`。但真正要锁住的是**装配层的行为**：
// 远程服务在绑定任何端口**之前**就拒绝启动。两者之间的缝正是这一类：
// `RemoteServer::new` 之后、`start` 之前插进一次「先绑端口、起来再检查权限」
// 的重排，上面所有单元测试照样全绿，而服务已经对外开了口子。
//
// 因此本用例**不**用错误消息当作「端口没被碰过」的证据 —— 那是自证：错误消息
// 恰恰来自被测代码自己。证据必须是内核的：`TcpListener::bind` 抢同一个端口。

/// 在临时配置目录里预置一份给定权限的 `remote-auth/secret`。
///
/// 走生产路径的同一个文件名与子目录（[`AUTH_SUBDIR`] / [`SECRET_FILE`]），
/// 否则测的是一个 `RemoteServer` 根本不读的文件。
#[cfg(unix)]
fn seed_secret_with_mode(cfg_dir: &Path, mode: u32) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let dir = cfg_dir.join(AUTH_SUBDIR);
    std::fs::create_dir_all(&dir).expect("创建 remote-auth 子目录");
    let path = dir.join(SECRET_FILE);
    // 32 字节：与 [`super::auth::load_or_create`] 的长度校验一致，
    // 让「唯一可能的失败理由」真的是权限而不是长度。
    std::fs::write(&path, [0x5Au8; 32]).expect("写入 secret");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("设置权限位");
    path
}

/// 预置 0644 的 `remote-auth/secret` → 拒绝启动，且**端口从未被绑定**。
///
/// 证据分两半，缺一不可：
///
/// 1. 错误是 `SecretError::BadPermissions` 的原文（逐字相等，不是「消息里出现过
///    0644」）。这条锁的是「失败理由没被换掉」。
/// 2. 同一个端口随后仍能 `bind` 上来。这一条才是「从未被绑定」的正面证据 ——
///    错误消息说什么不算数，它恰恰来自被测代码自己；内核说这个端口空着才算数。
///
/// 第 2 条挡住的是「先绑端口、起来之后才验权限」这类重排：那种实现同样会返回
/// 一个错误消息，但端口已经被占住，重绑必然失败。
#[serial_test::serial]
#[tokio::test]
#[cfg(unix)]
async fn e2e_secret_file_permissions_fail_closed() {
    use std::os::unix::fs::PermissionsExt;

    let cfg_dir = tempfile::tempdir().expect("isolated config dir");
    let secret_path = seed_secret_with_mode(cfg_dir.path(), 0o644);

    // 先抢一个端口把号占住，读出它的编号，然后**放掉** —— 于是这个号先前是
    // 空闲的、现在也是空闲的，中间只有被测代码有机会去 bind 它。
    // 一直占着不放是错的：那样端口压根抢不到，任何 bind 实现都会失败，
    // 下面那条「端口从未被绑定」的证据就永远没有机会发挥作用。
    let port = {
        let probe = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("抢一个空闲端口");
        let p = probe.local_addr().expect("probe 的本地地址").port();
        assert_ne!(p, 0, "内核必须分配了一个真实端口");
        drop(probe);
        p
    };

    // 真正的启动路径：构造 → 启动。权限不过就断在构造这一步，端口一步都碰不到。
    // 若构造居然放行，就让它把 `start` 跑完，让下面「端口从未被绑定」那条
    // 断言去拆穿它 —— 少跑这一步，这条错误实现会靠 panic 前的日志蒙过去。
    let attempted = RemoteServer::new(
        RemoteApp::headless(),
        Arc::new(RecordingSink::new()),
        RemoteConfig {
            enabled: true,
            port,
            tailscale_enabled: false,
            ..Default::default()
        },
        cfg_dir.path().to_path_buf(),
    );

    let err = match attempted {
        Err(e) => e,
        Ok(server) => {
            let _ = server.start().await;
            panic!(
                "0644 的 secret 必须让 RemoteServer::new 失败；实际构造成功，\
                 并且已尝试在端口 {port} 上启动 —— 服务可能已经对外开了口子"
            );
        }
    };

    let expected = SecretError::BadPermissions {
        path: secret_path.clone(),
        mode: 0o644,
    };
    assert_eq!(
        err,
        expected.to_string(),
        "失败理由必须逐字是 BadPermissions，实际 {err}"
    );

    // 「端口从未被绑定」的正面证据：同一个端口现在仍能抢回来。
    let rebound = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap_or_else(|e| {
        panic!(
            "端口 {port} 必须从未被绑定过 —— 拒绝启动之后它应当仍是空闲的，实际抢不回来：{e}。\
             说明启动流程在权限校验之前就绑了端口"
        )
    });
    drop(rebound);

    // 不自动 chmod 修复：0644 原样保留，「谁把凭据文件改宽了」才追得出来。
    assert_eq!(
        std::fs::metadata(&secret_path)
            .expect("secret 仍在")
            .permissions()
            .mode()
            & 0o777,
        0o644,
        "拒绝启动不得顺带把权限改回 0600 —— 偷偷修好会让「谁改宽的」永远查不出来"
    );
}

// ===========================================================================
// 边界 E2E ②：Web UI 在非 loopback 上真的能开 —— 以及首次访问的已知代价
// ===========================================================================
//
// 依赖：本机第二块可自连的非 loopback 地址（`nonloopback_source` 的真实 TCP
// 判据）。拿不到时可见地 SKIP；`CC_SWITCH_REQUIRE_TAILNET=1` 的 job 里 skip 计为
// 失败 —— 那个 job 的存在意义就是跑这一侧，静默跳过等于把整条锁摘掉。
//
// 本用例锁的是一条**有意接受的代价**：配对之前，`<img src="/api/icon">` 与
// `EventSource("/api/events")` 这三条 browser-safe 路由在非 loopback 上
// 各自 401，页面会先缺图标、收不到事件流。这不是缺陷，是「浏览器载体放不了
// Authorization 头」这个事实的直接后果。用例名与断言消息都写明这一点，
// 免得有人把它当成 bug「顺手修掉」—— 修的方向只有两个，都更糟：把
// BrowserSafe 挂成免鉴权（远端匿名可读事件流），或者去掉那条 401 断言。

/// 本机拿不出第二块非 loopback 地址时的处置。
///
/// 现在只是一层薄壳：判断「跳过还是变红」只在 [`require_or_skip`] 一处发生，
/// 这里只负责把「为什么没有」这段详情接上去。以前这里是第二个独立的跳过出口，
/// 意味着判断散在两处 —— 改一处忘了另一处，就会出现同一个 job 里两种处置。
fn skip_or_fail_on_missing_nonloopback(why: &str) {
    require_or_skip(&format!(
        "缺少第二个非 loopback 源地址（探测器逐个否掉了所有候选：{why}）"
    ));
}

/// Web UI 在非 loopback 来源上：`GET /` 200，配对前三条 browser-safe 各 401，
/// 会话 cookie 落地后恢复。
///
/// 前半段是**已知且有意接受**的首次访问代价（用例名即证据）；后半段锁的是
/// 「cookie 一落地就恢复」—— 只有后半段成立，前半段的 401 才读得出「凭据
/// 通道还没通」而不是「这条路由坏了」。
///
/// 恢复腿走**真实存储**：配对经生产路径签发真 token，请求带真实的 `Cookie`
/// 头经真实 TCP 从真实非 loopback 来源发出。
///
/// **这段里有一句曾经是事实、现在不是了，留着是因为它解释了本用例当初的形状。**
/// 早先的版本写着「没有任何端点下发 `Set-Cookie`（`ClientToken::cookie_hash`
/// 恒为 `None`）」，于是用 [`super::auth::save_clients`] 手工落一条会话记录。
/// 那句在当时是对的，如今已经错了：`pairing::handlers::issue_credential` 会把
/// `cookie_hash` 真的写进凭据记录（赋值就在那个函数里，紧挨着
/// `ClientToken::new(name, secret, token)`），交付凭据的那次 poll 响应也真的
/// 带 `Set-Cookie`。手工落库这条路因此被拆掉了，现在 cookie 明文直接从响应头里
/// 解出来（见下面 `parse_session_cookie` 那一段的注释）——「我们自己造的东西
/// 我们自己认」这层嫌疑也随之消失。
///
/// 本用例验的仍是中间件的 cookie 通道 + 存储 + 判定表这一整条真实链路。
///#[serial_test::serial]
#[tokio::test]
async fn e2e_web_ui_loads_over_nonloopback() {
    let src = match nonloopback_source().await {
        Ok(src) => src,
        Err(why) => {
            skip_or_fail_on_missing_nonloopback(&why);
            return;
        }
    };
    let h = start_headless(RecordingSink::new()).await;

    // --- 第 1 格：public 外壳。非 loopback 也要能打开页面，否则没法配对 ---
    let (shell_status, shell_body) = raw_of(&h.base_url, src.ip, "/", &[]).await;
    assert_eq!(
        shell_status, 200,
        "GET / 是 Public：远端浏览器必须先能拿到页面，否则没有配对入口（来源 {}）",
        src.ip
    );
    assert!(
        shell_body.contains("CC Switch Remote"),
        "200 的必须是真页面外壳（标题在），不是一个空的 200"
    );
    // 页面必须自带配对入口，否则「能打开页面」和「能配上对」之间就没有桥：
    // 远端浏览器拿到 200 之后，唯一能拿到 cookie 的路就是这两个端点。
    //
    // 断言的是**调用点**而不是端点字符串 —— `/pairing/request` 这几个字在本文件
    // 顶部的流程注释里也出现，早先只比字符串的版本被注释养着，负控把页面里的
    // `fetch` 改掉它照样绿。注释不驱动流程，调用点才驱动。
    for needle in [
        "fetch('/pairing/request'",
        "fetch('/pairing/poll'",
        "'Authorization': 'Pairing '",
        "id=\"pairingCode\"",
    ] {
        assert!(
            shell_body.contains(needle),
            "外壳页里必须有配对入口 `{needle}`：远端浏览器没有别的办法拿到会话 cookie，\
             GET / 是它唯一的非回环入口"
        );
    }

    // --- 第 2~4 格：配对前，三条 browser-safe 在非 loopback 上各 401（已知代价）---
    let browser_safe = browser_safe_paths();
    assert!(
        !browser_safe.is_empty(),
        "ROUTE_SCOPE_TABLE 里必须有 BrowserSafe 路由，否则本用例什么都没验"
    );
    for path in &browser_safe {
        let status = raw_of(&h.base_url, src.ip, path, &[]).await.0;
        assert_eq!(
            status, 401,
            "BrowserSafe 路由 {path}：配对完成前在非 loopback 上必须 401（来源 {}）。\
             这是**已知且有意接受**的首次访问代价 —— <img>/EventSource 放不了 \
             Authorization 头，cookie 还没落地。不要为了「页面好看」把 BrowserSafe \
             挂成免鉴权：那会让远端匿名读到 provider 切换事件流",
            src.ip
        );
    }

    // --- 第 5~7 步：真实配对，拿到真 token ---
    let r1 = post(
        &h.base_url,
        "/pairing/request",
        serde_json::json!({"clientName": "web-ui", "requestedScopes": ["read"]}),
    )
    .await;
    let b1: serde_json::Value = step(1, "POST /pairing/request", r1.json().await);
    let user_code = step(1, "取 userCode", field(&b1, "userCode"));
    let poll_token = step(1, "取 pollToken", field(&b1, "pollToken"));
    approve_out_of_band(&h, &user_code).await;
    let pr = poll(&h.base_url, &poll_token).await;
    // Set-Cookie 必须在读 body 之前取走：`json()` 会把响应消费掉。
    let set_cookie = pr
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .expect("交付凭据的那次 poll 响应必须带 Set-Cookie，否则浏览器通道根本不存在")
        .to_str()
        .expect("Set-Cookie 是可见 ASCII")
        .to_string();
    let b2: serde_json::Value = step(2, "POST /pairing/poll", pr.json().await);
    let client_token = step(2, "取 clientToken", field(&b2, "clientToken"));

    // cookie 的明文**从响应头里取**，不再手工往凭据库里注入摘要。
    //
    // 这条接缝正是本用例以前漏掉的：`session_cookie_attributes` 验「交付时下发了
    // 正确的 Set-Cookie」，本用例验「cookie 能过鉴权」，两半之间靠一句
    // `entry.cookie_hash = Some(token_hash(..))` 搭桥。签发方后来真的落地了
    // （`pairing::handlers::issue_credential`），那座桥却没拆 —— 于是
    // 「真实下发的那条 Set-Cookie 到底能不能用」始终没有断言。
    let session_value =
        parse_session_cookie(&set_cookie).expect("Set-Cookie 里必须能解出 cc_switch_session 的值");

    // --- 第 8~10 格：cookie 落地后恢复 ---
    for path in &browser_safe {
        let status = raw_of(
            &h.base_url,
            src.ip,
            path,
            &[("Cookie", &format!("cc_switch_session={session_value}"))],
        )
        .await
        .0;
        assert_eq!(
            status, 200,
            "BrowserSafe 路由 {path}：会话 cookie 落地后必须恢复 200（来源 {}）—— \
             前面那几条 401 必须读得出「凭据通道还没通」，而不是「这条路由坏了」。\
             用的是 poll 真正下发的那条 Set-Cookie，不是手工注入的摘要",
            src.ip
        );
    }

    // 反证一：cookie 值错的仍 401。否则上面的 200 只说明「带 Cookie 头就行」。
    let (wrong_cookie, _) = raw_of(
        &h.base_url,
        src.ip,
        "/api/icon",
        &[("Cookie", "cc_switch_session=not-the-session-value")],
    )
    .await;
    assert_eq!(
        wrong_cookie, 401,
        "错误的会话 cookie 必须仍 401，实际 {wrong_cookie}"
    );

    // 反证二：Bearer 顶不上 cookie 通道。browser-safe 之所以是 BrowserSafe，
    // 就是因为它的载体放不了 Authorization 头；一旦这条也放行，401 就只对
    // 「什么都不带」成立 —— 那正是把 BrowserSafe 误当豁免的形态。
    let (bearer, _) = raw_of(
        &h.base_url,
        src.ip,
        "/api/icon",
        &[("Authorization", &format!("Bearer {client_token}"))],
    )
    .await;
    assert_eq!(
        bearer, 401,
        "BrowserSafe 只认 cookie 通道：拿 Bearer token 打 /api/icon 必须仍 401，实际 {bearer}"
    );

    teardown(h).await;
}

/// CORS 面与凭据闸门的先后：非白名单 Origin + **有效**凭据 → 403，而不是 2xx。
///
/// 为什么这条值得单独一条用例：12 格表里验的是「无凭据」那一列，而
/// `Cred::Token` / `Cred::Session` 两列只在纯函数层验过。一旦实现把 Origin
/// 否决挪到凭据闸门之后（或者干脆挪进「够不够凭据」的那个布尔里），12 格表在
/// 纯函数层仍然全绿 —— 攻击者拿一把**真的**配对凭据，从任意网页发起跨源请求，
/// 响应内容就被读走了，而所有既有断言都验不到这一点。
///
/// 因此本用例先立一条**反证**：同一份凭据、不带 Origin，必须真的 200。
/// 没有这条腿，上面的 403 只说明「这条路由对谁都不通」。
///
/// 走的是真实链路：真实配对签发的 token、真实 `clients.json` 里的会话摘要、
/// 真实 TCP、真实装配树上的 `CorsLayer` 与 `auth_layer`。
#[serial_test::serial]
#[tokio::test]
async fn cors_origin_denial_precedes_credential_gate() {
    const FOREIGN: &str = "https://evil-127.0.0.1.example";
    const NULL_ORIGIN: &str = "null";

    let h = start_headless(RecordingSink::new()).await;
    let base = h.base_url.clone();
    let src = loopback();

    // --- 真实配对，拿一把**确实有效**的 token ---
    let r1 = post(
        &base,
        "/pairing/request",
        serde_json::json!({"clientName": "cors-probe", "requestedScopes": ["read"]}),
    )
    .await;
    let b1: serde_json::Value = step(1, "POST /pairing/request", r1.json().await);
    let user_code = step(1, "取 userCode", field(&b1, "userCode"));
    let poll_token = step(1, "取 pollToken", field(&b1, "pollToken"));
    approve_out_of_band(&h, &user_code).await;
    let pr = poll(&base, &poll_token).await;
    let set_cookie = pr
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .expect("交付凭据的 poll 必须下发 Set-Cookie")
        .to_str()
        .expect("Set-Cookie 是可见 ASCII")
        .to_string();
    let b2: serde_json::Value = step(2, "POST /pairing/poll", pr.json().await);
    let client_token = step(2, "取 clientToken", field(&b2, "clientToken"));

    // cookie 明文同样取真实下发的那一条（见 `e2e_web_ui_loads_over_nonloopback`
    // 里对这条接缝的说明）。
    let session_value =
        parse_session_cookie(&set_cookie).expect("Set-Cookie 里必须能解出 cc_switch_session 的值");

    let bearer = format!("Bearer {client_token}");
    let cookie = format!("cc_switch_session={session_value}");

    // --- 反证：两条凭据通道在「无 Origin」时都真的放行 ---
    let (bearer_ok, _) = raw_of(&base, src, PROBE_PATH, &[("Authorization", &bearer)]).await;
    assert_eq!(
        bearer_ok, 200,
        "反证：Bearer 通道 + 无 Origin 必须 200，否则下面的 403 验不到「凭据有效」"
    );
    let (cookie_ok, _) = raw_of(&base, src, "/api/icon", &[("Cookie", &cookie)]).await;
    assert_eq!(
        cookie_ok, 200,
        "反证：会话 cookie 通道 + 无 Origin 必须 200，否则下面的 403 验不到「凭据有效」"
    );

    // --- 非白名单 Origin：两条通道都必须 403 ---
    for (label, path, credential) in [
        ("Bearer", PROBE_PATH, ("Authorization", bearer.as_str())),
        ("会话 cookie", "/api/icon", ("Cookie", cookie.as_str())),
    ] {
        let (status, body) = raw_of(&base, src, path, &[credential, ("Origin", FOREIGN)]).await;
        assert_eq!(
            status, 403,
            "{label}：非白名单 Origin {FOREIGN} 必须 403 而非 2xx —— Origin 否决必须\
             先于凭据闸门，否则任意网页拿着真凭据就能读走响应。实际 {status}，body {body}"
        );
        assert!(
            !body
                .to_ascii_lowercase()
                .contains("access-control-allow-origin"),
            "{label}：非白名单 Origin 的响应不得下发 ACAO，实际 {body}"
        );

        // `Origin: null` 同理：sandbox iframe / file: / data: 页面都发它，
        // 它不归属任何站点，列进白名单等于放行整个 sandbox 面。
        let (null_status, _) =
            raw_of(&base, src, path, &[credential, ("Origin", NULL_ORIGIN)]).await;
        assert_eq!(
            null_status, 403,
            "{label}：Origin: null 必须 403 而非 2xx，实际 {null_status}"
        );
    }

    // --- 白名单 Origin 照常放行，且 CORS 面确实下发了 ACAO ---
    // 这一腿验的是装配接线：白名单 origin 必须在 CorsLayer 里，
    // 否则 Tauri webview 会被浏览器按「跨源失败」掐掉。
    let (ok, body) = raw_of(
        &base,
        src,
        PROBE_PATH,
        &[("Authorization", &bearer), ("Origin", "tauri://localhost")],
    )
    .await;
    assert_eq!(
        ok, 200,
        "白名单 Origin + 有效凭据必须 200，实际 {ok}，body {body}"
    );
    assert!(
        body.to_ascii_lowercase()
            .contains("access-control-allow-origin"),
        "白名单 Origin 的响应必须带 access-control-allow-origin，否则内嵌 webview \
         读不到响应：{body}"
    );

    teardown(h).await;
}

// ===========================================================================
// 安全形态 E2E：六类越权 / 泄露形态的真实请求级反证
// ===========================================================================
//
// 与前面 12 格表分工明确：判定表是纯函数、输入是构造出来的三元组；这里验的是
// 构造输入验不到的三件事。
//
// 1. **浏览器形态**。CSRF 必须按真实跨站请求发：外站 `Origin` + CORS 简单请求
//    语义。`Option<Json<T>>` 那道洞只在简单请求形态下可利用 —— 而简单请求
//    不过预检，所以浏览器**真的会**把它发出去。反过来，用带 JSON `Content-Type`
//    的普通 POST 去测，浏览器侧就先被预检挡掉了，洞在服务端是否存在根本测不到。
// 2. **载体形态**。凭据只从 `Authorization` / `Cookie` 进来。放进 URL 就等于
//    同时把它复制进访问日志、浏览器历史与 `Referer` 三个持久载体 —— 那是「认证」
//    没做到的事，却会把凭据扩散到三处去。
// 3. **出口形态**。密钥不得出现在响应体、错误详情与日志三处中的任何一处；
//    而**不含密钥**的正常错误文本必须仍然可读。全量抹成同一个占位符不叫堵洞，
//    叫把远程排障能力一并删掉 —— 那等于把洞换了个位置。

/// 六条写操作路由。
///
/// 顺序与 [`protected_paths`] 扣掉只读项后的顺序一致，这样下面那条「由路由表
/// 反推」的断言可以直接整表比较。
const WRITE_ROUTES: [&str; 9] = [
    "/api/switch",
    "/api/reorder",
    "/api/proxy/control",
    "/api/proxy/takeover",
    "/api/proxy/failover",
    "/api/provider/test",
    "/api/remote/config/bind-scope/request",
    "/api/remote/config/bind-scope/confirm",
    "/api/remote/config/bind-scope/reject",
];

/// `Protected` 分区里唯一的几条**只读**路由。
///
/// 写操作清单由路由表反推（见用例里那条断言）而不是手抄：`Protected` 分区
/// 减去这几条，剩下的就是「会改状态的那几条」。手抄一份清单的话，新增一条写
/// 路由时本用例会静默放过它 —— 而漏掉一条写路由的 CSRF 断言，症状正是下一次
/// 有人加写路由时它默认继承了上一条的结论。
const READ_ONLY_PROTECTED: [&str; 4] = [
    "/api/providers",
    "/api/usage",
    "/api/current",
    "/api/health",
];

/// 跨站表单提交：`Origin` 是外站，`Content-Type` 是 `x-www-form-urlencoded`。
///
/// 简单请求不过预检，所以这发请求在真实浏览器里**会被发出去**，服务端必须自己
/// 挡住 —— 不能指望预检。
async fn cross_site_post(base: &str, path: &str, form_body: &str) -> reqwest::Response {
    cross_site_post_from_origin(base, path, form_body, ORIGIN_FOREIGN).await
}

/// [`cross_site_post`] 的可指定 `Origin` 版本：白名单 Origin 那条「不误伤」的
/// 腿要在同一份代码上只换一个 origin。
async fn cross_site_post_from_origin(
    base: &str,
    path: &str,
    form_body: &str,
    origin: &str,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}{path}"))
        .header(axum::http::header::ORIGIN, origin)
        .header(
            axum::http::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form_body.to_string())
        .send()
        .await
        .unwrap_or_else(|e| panic!("跨站表单 POST {path} 必须打到活的 listener：{e}"))
}

/// 不带 `Content-Type` 的跨站 `fetch` —— 同样不过预检的另一种简单请求形态。
///
/// body 用 `Vec<u8>` 而不是 `&str`：两者都**不**带 Content-Type，但用 `&str` 时
/// 一旦将来某个版本补上默认值，这一格就悄悄变成普通 JSON 请求，验到的就不再是
/// 「没有 Content-Type」这个真正要验的形态了。
async fn cross_site_fetch_without_content_type(base: &str, path: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}{path}"))
        .header(axum::http::header::ORIGIN, ORIGIN_FOREIGN)
        .body(Vec::from(r#"{"providerId":"evil.example"}"#))
        .send()
        .await
        .unwrap_or_else(|e| panic!("无 Content-Type 的跨站 POST {path} 必须打到活的 listener：{e}"))
}

/// 九条写路由的跨站形态一律 403。
///
/// 每一格发的是**浏览器真会发出去**的那一发：外站 `Origin` + CORS 简单请求
/// 语义（表单提交 / 不带 `Content-Type` 的 fetch）。普通 JSON POST 测不到这条：
/// 那种请求在浏览器侧先过预检，服务端有没有这道闸门根本不影响它能不能发出去。
///
/// 期望是 403（Origin 否决）而不是 415（extractor 拒收）也不是 401（凭据闸门）：
/// Origin 否决在判定表第①段，先于凭据闸门、也先于 handler 的 extractor。后两者
/// 都是「别想通过这一层」，只有 403 说的是「跨站这件事本身就不成立」。
///
/// 末尾还有两条反向腿：白名单 Origin 的同形态请求**不得**被 403（否则这条闸门
/// 就是「把跨站一律打死」，桌面端自己的内嵌 Web UI 也会跟着打不开），以及
/// `Origin: null` 同样必须 403 —— sandbox iframe / `file:` / `data:` 页面发的就是
/// 它，它不归属任何站点，列进白名单等于放行整个 sandbox 面。
#[serial_test::serial]
#[tokio::test]
async fn csrf_cross_site_post_on_all_write_routes_is_403() {
    let h = start_headless(RecordingSink::new()).await;
    let base = h.base_url.clone();

    // 完整性：由 ROUTE_SCOPE_TABLE 反推写操作清单，与手抄的那份整表比对。
    // 新增一条写路由而没加进 WRITE_ROUTES 时，红的是这一条 —— 缺口当场可见，
    // 而不是等下一次有人加写路由时它默认继承了上一条的结论。
    let derived: Vec<String> = protected_paths()
        .into_iter()
        .filter(|p| !READ_ONLY_PROTECTED.contains(&p.as_str()))
        .collect();
    assert_eq!(
        derived,
        WRITE_ROUTES.to_vec(),
        "Protected 分区扣掉只读项之后必须**恰好**是这九条写路由。\
         对不上说明：新增了写路由却没加进 WRITE_ROUTES（CSRF 断言会静默放过它），\
         或者有路由被挪出了 Protected 分区。实测路由表反推得 {derived:?}"
    );

    // 表里必须有非 loopback 才能跑的那一格？—— 不需要：Origin 否决与 peer 无关，
    // loopback + 外站 Origin 就是判定表第 11 格，403。因此本用例在任何 runner
    // （含只有 lo0 的 CI）上都真跑，不依赖第二块网卡。
    let form_body = "providerId=evil.example&appType=claude&enabled=true";

    for path in WRITE_ROUTES {
        // 形态一：跨站表单（application/x-www-form-urlencoded）
        let form = cross_site_post(&base, path, form_body).await;
        let status = form.status().as_u16();
        let body = form.text().await.unwrap_or_default();
        assert_eq!(
            status, 403,
            "{path}：跨站表单提交（Origin={ORIGIN_FOREIGN}，Content-Type=\
             application/x-www-form-urlencoded）必须 403 —— 简单请求不过预检，\
             这发请求在真实浏览器里真的会被发出去。实际 {status}，body {body}"
        );
        assert!(
            !body
                .to_ascii_lowercase()
                .contains("access-control-allow-origin"),
            "{path}：外站 Origin 的响应不得下发 ACAO，否则跨站页面读得到响应内容：{body}"
        );

        // 形态二：不带 Content-Type 的 fetch
        let no_ct = cross_site_fetch_without_content_type(&base, path).await;
        let status = no_ct.status().as_u16();
        let body = no_ct.text().await.unwrap_or_default();
        assert_eq!(
            status, 403,
            "{path}：不带 Content-Type 的跨站 fetch 同样是简单请求，必须 403 —— \
             `Option<Json<T>>` 那道洞正是沿这个形态进来的（extractor 把 415 吞成 \
             None，handler 照常执行）。实际 {status}，body {body}"
        );

        // 反向腿：白名单 Origin 的同形态请求不得被 403。
        //
        // 这一格现在会停在 extractor 的 415 上（简单请求不是 JSON），而那正是
        // 「鉴权层放行了」的证据：闸门若把白名单 Origin 也打死，桌面端自己的
        // 内嵌 Web UI / Tauri webview 会一起打不开。这里只断言「不是 403」，
        // 刻意不钉死具体码 —— 那是 handler 层 extractor 的契约，由
        // `handlers::tests` 的 415 断言负责，两边各管一段。
        let whitelisted =
            cross_site_post_from_origin(&base, path, form_body, ORIGIN_WHITELISTED).await;
        let status = whitelisted.status().as_u16();
        let body = whitelisted.text().await.unwrap_or_default();
        assert_ne!(
            status, 403,
            "{path}：白名单 Origin（{ORIGIN_WHITELISTED}）的同形态请求不得被 Origin \
             否决挡下 —— 这条闸门挡的是跨站，不是本机内嵌页面。实际 {status}，body {body}"
        );

        // `Origin: null`：sandbox iframe / file: / data: 页面发的那一种。
        let null_origin = cross_site_post_from_origin(&base, path, form_body, "null").await;
        let status = null_origin.status().as_u16();
        assert_eq!(
            status, 403,
            "{path}：Origin: null 必须 403。它不归属任何站点，判据上等同于外站 —— \
             列进白名单等于放行整个 sandbox 面。实际 {status}"
        );
    }

    teardown(h).await;
}

impl GatedChannel {
    /// 只带通道自身的头（远端侧为空；loopback 回退侧是代理信号），**不带任何凭据**。
    ///
    /// 「URL 里带着凭据」这一格必须走真实 socket：判定的是服务端 `ConnectInfo`
    /// 真的读到的对端，而不是我们声称的来源。
    async fn call_bare(&self, base: &str, path: &str) -> (u16, String) {
        raw_of(base, self.source, path, &self.extra).await
    }

    /// [`GatedChannel::call_bare`] 的 POST 版本。
    async fn post_bare(&self, base: &str, path: &str, body: &str) -> (u16, String) {
        post_raw_of(base, self.source, path, &self.extra, body).await
    }

    /// 带会话 cookie 通道打一发，返回 `(状态码, 响应全文)`。
    ///
    /// 与 [`GatedChannel::call_with_bearer`] 同一套来源绑定，只是把凭据换成
    /// cookie。`Protected` 现在两条通道都收、`BrowserSafe` 仍只收 cookie，
    /// 所以这两条路必须各自成对打 —— 只测其中一条时，另一条的判据没有任何断言。
    async fn call_with_cookie(&self, base: &str, value: &str, path: &str) -> (u16, String) {
        let cookie = format!("cc_switch_session={value}");
        let mut headers: Vec<(&str, &str)> = self.extra.iter().map(|(n, v)| (*n, *v)).collect();
        headers.push(("Cookie", cookie.as_str()));
        raw_of(base, self.source, path, &headers).await
    }

    /// **同时**带 Bearer 与 cookie 打一发 —— 「通道之间不回落」那一格的专属载体。
    ///
    /// 单独成方法而不是在用例里就地拼 header：cookie 头的拼法必须与
    /// [`GatedChannel::call_with_cookie`] 逐字一致，两处各写一份的话，
    /// 「带了无效 Bearer 仍 401」也可能只是 cookie 头拼错造成的 —— 那样这一格
    /// 就变成了在验拼写，而不是在验回落语义。
    async fn call_with_bearer_and_cookie(
        &self,
        base: &str,
        token: &str,
        value: &str,
        path: &str,
    ) -> (u16, String) {
        let bearer = format!("Bearer {token}");
        let cookie = format!("cc_switch_session={value}");
        let mut headers: Vec<(&str, &str)> = self.extra.iter().map(|(n, v)| (*n, *v)).collect();
        headers.push(("Authorization", bearer.as_str()));
        headers.push(("Cookie", cookie.as_str()));
        raw_of(base, self.source, path, &headers).await
    }
}

/// 凭据放进 URL 一律不作数：查询串里的 `?token=` / `?pollToken=` 仍 401。
///
/// 这一格盯的是**载体**而不是校验逻辑：`?token=` 后面挂着的可以是刚签发出来的、
/// 完全有效的凭据，服务端照样必须当它不存在。
///
/// 为什么这条重要：URL 会同时落进三处额外持久载体 —— 访问日志、浏览器历史、
/// 以及从该页面发出的每一个请求的 `Referer`。凭据一旦进了 URL，「通过了鉴权」
/// 这件事就不再等于「只有服务端的凭据库见过它」。
///
/// 每条 401 都配了一条**正向对照**：同一通道上、同一份凭据、只把它从 URL 挪到
/// `Authorization` 头，必须真的放行。没有对照的话，「全部 401」也可能只是
/// 「这个夹具谁也过不去」。
#[serial_test::serial]
#[tokio::test]
async fn credential_in_query_string_is_rejected() {
    let gated = gated_channel().await;
    let h = start_headless(RecordingSink::new()).await;
    let base = h.base_url.clone();

    // --- 第 1 轮配对：把真凭据换出来（正向对照要用）---
    let r1 = post(
        &base,
        "/pairing/request",
        serde_json::json!({"clientName": "query-probe", "requestedScopes": ["read"]}),
    )
    .await;
    let b1: serde_json::Value = step(1, "POST /pairing/request", r1.json().await);
    let user_code = step(1, "取 userCode", field(&b1, "userCode"));
    let poll_token = step(1, "取 pollToken", field(&b1, "pollToken"));
    approve_out_of_band(&h, &user_code).await;
    let pr = poll(&base, &poll_token).await;
    // poll 那一发同时下发了会话 cookie，先读头再读体 ——
    // body 被消费掉之后 `Set-Cookie` 就取不到了。
    let session_cookie = pr
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(super::auth::parse_session_cookie)
        .expect("配对交付的那一发必须下发会话 cookie，否则 cookie 通道的对照无从谈起");
    let b2: serde_json::Value = step(2, "POST /pairing/poll", pr.json().await);
    let client_token = step(2, "取 clientToken", field(&b2, "clientToken"));

    assert!(
        client_token.len() > 16,
        "正向对照需要一把真的凭据，实际 {client_token:?}"
    );

    // --- 查询串里带真 token：一律 401 ---
    let queries: [(&str, String); 6] = [
        (
            "/api/providers",
            format!("/api/providers?token={client_token}"),
        ),
        ("/api/current", format!("/api/current?token={client_token}")),
        ("/api/events", format!("/api/events?token={client_token}")),
        ("/api/icon", format!("/api/icon?token={client_token}")),
        (
            "/api/provider-icons/claude",
            format!("/api/provider-icons/claude?token={client_token}"),
        ),
        ("/api/switch", format!("/api/switch?token={client_token}")),
    ];
    for (label, path) in &queries {
        let method = if *label == "/api/switch" {
            gated.post_bare(&base, path, "providerId=").await
        } else {
            gated.call_bare(&base, path).await
        };
        let (status, body) = method;
        assert_eq!(
            status, 401,
            "{label}：URL 里挂着刚签发的真 token 也必须 401 —— 查询串不作数。\
             凭据只从 Authorization / Cookie 两个头进来。实际 {status}，body {body}"
        );
    }

    // 写路由同样如此：`?token=` 不能替代 Authorization 头把 POST 打通。
    for path in [
        "/api/proxy/control",
        "/api/proxy/takeover",
        "/api/proxy/failover",
        "/api/reorder",
        "/api/provider/test",
    ] {
        let (status, body) = gated
            .post_bare(&base, &format!("{path}?token={client_token}"), "x=1")
            .await;
        assert_eq!(
            status, 401,
            "{path}?token=：写路由也必须 401（且此时还没到 extractor 的 415），\
             实际 {status}，body {body}"
        );
    }

    // --- 正向对照一：同一通道、同一份凭据，只把它从 URL 挪到头里 ---
    let (ok, body) = gated
        .call_with_bearer(&base, &client_token, "/api/providers")
        .await;
    assert_eq!(
        ok, 200,
        "正向对照：Bearer 通道（{}）+ 无凭据查询串必须 200，否则上面那些 401 \
         验不到「凭据本身是有效的」",
        gated.label
    );
    assert!(
        !body.contains(&client_token),
        "凭据明文不得出现在自己的响应体里：{body}"
    );

    // --- 正向对照二：cookie 通道同理 ---
    let (cookie_ok, _) = gated
        .call_with_cookie(&base, &session_cookie, "/api/icon")
        .await;
    assert_eq!(
        cookie_ok, 200,
        "正向对照：会话 cookie 通道（{}）+ 无凭据查询串必须 200，\
         否则 /api/icon?token= 的 401 只说明「这条路由对 cookie 也不认」",
        gated.label
    );

    // --- 第 2 轮配对：验一次性凭据也不从查询串进 ---
    let r2 = post(
        &base,
        "/pairing/request",
        serde_json::json!({"clientName": "query-probe-2", "requestedScopes": ["read"]}),
    )
    .await;
    let b3: serde_json::Value = step(3, "POST /pairing/request", r2.json().await);
    let user_code2 = step(3, "取 userCode", field(&b3, "userCode"));
    let poll_token2 = step(3, "取 pollToken", field(&b3, "pollToken"));
    approve_out_of_band(&h, &user_code2).await;

    let (poll_qs, body) = post_raw_of(
        &base,
        loopback(),
        &format!("/pairing/poll?pollToken={poll_token2}"),
        &[(
            axum::http::header::CONTENT_TYPE.as_str(),
            "application/json",
        )],
        "{}",
    )
    .await;
    assert_eq!(
        poll_qs, 401,
        "/pairing/poll?pollToken=：一次性凭据放进 URL 必须仍 401 —— \
         pollToken 只认 Authorization: Pairing 一个来源。实际 {poll_qs}，body {body}"
    );

    // 正向对照：同一份 pollToken 改回头里，必须真的换到凭据（否则上面那条 401
    // 也可能只是「这个 pollToken 本来就是废的」）。
    let pr2 = poll(&base, &poll_token2).await;
    let status = pr2.status().as_u16();
    let b4: serde_json::Value = step(4, "带 header 的 poll", pr2.json().await);
    assert_eq!(
        status, 200,
        "正向对照：pollToken 走 Authorization: Pairing 必须 200 —— \
         上面那条 401 验的是「查询串不作数」，不是「这个 token 是废的」"
    );
    step(4, "取 clientToken", field(&b4, "clientToken"));

    teardown(h).await;
}

/// 进程级日志收集器：把测试窗口内的 `log!` 记进一个全局缓冲。
///
/// 为什么必须真装一个 logger 而不去读 stdout：断言的对象是「服务端在**请求处理
/// 过程中**写出去的日志」，而 `cargo test` 默认吞掉通过用例的 stdout。
/// `log::set_boxed_logger` 全进程只能成功一次，所以装不上时**硬红**，而不是悄悄
/// 退化成「没抓到日志，所以没泄漏」—— 那正是让这条断言恒真的写法。
struct LogRecorder;

static LOG_BUFFER: OnceLock<Mutex<String>> = OnceLock::new();
static LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);

impl log::Log for LogRecorder {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if let Some(buf) = LOG_BUFFER.get() {
            if let Ok(mut s) = buf.lock() {
                s.push_str(&record.args().to_string());
                s.push('\n');
            }
        }
    }

    fn flush(&self) {}
}

/// 拿到日志缓冲，并把 logger 装上。
///
/// 装不上就是红：这个用例的全部价值都在「日志里搜不到」，抓不到日志还绿等于
/// 什么都没验。`e2e.rs` 是本 crate 里唯一的 logger 安装者，所以第一次调用一定
/// 成功；第二次（同一个用例不会发生）靠 `LOGGER_INSTALLED` 短路而不是重复安装。
fn log_capture() -> &'static Mutex<String> {
    let buf = LOG_BUFFER.get_or_init(|| Mutex::new(String::new()));
    if !LOGGER_INSTALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        log::set_boxed_logger(Box::new(LogRecorder))
            .expect("本进程里不该有第二个 logger（e2e.rs 是唯一安装者）");
        // `log` 的默认上限是 `Off`：不显式抬高的话缓冲永远是空的，下面所有
        // 「日志里搜不到」都会因为什么都没抓到而恒真。
        log::set_max_level(log::LevelFilter::Trace);
    }
    buf
}

/// 从任意一条响应里递归找出所有字符串值（错误详情字段就是其中之一）。
///
/// 收集的是**字符串值**而不是叫 `error` 的键：那条「错误详情」的断言要能对
/// 任何位置上的文本生效，只认固定键名等于把「换个字段名再泄漏一次」放过去。
fn all_string_values(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(a) => a.iter().for_each(|x| all_string_values(x, out)),
        serde_json::Value::Object(m) => m.values().for_each(|x| all_string_values(x, out)),
        _ => {}
    }
}

/// canary 假密钥的三段形态，与 `redact` 的三条正则一一对应。
///
/// 三段拼成**一条**文本使用：分三次验的话，「只覆盖了其中一种形态」很容易被
/// 读成「脱敏有效」—— 而上游回显里三种形态是同一条错误串里混着出现的。
const CANARY_SK: &str = "sk-canarydeadbeef00112233445566778899";
const CANARY_HEX: &str = "cafebabe0123456789abcdef0123456789";
const CANARY_BEARER: &str = "canary-bearer-0123456789abcdef";

/// canary 的三段形态各自的可搜片段。
///
/// 按**片段**而不是整条文本搜：整条拼串一旦被截断或部分替换，整串比对就会漏判，
/// 而「`sk-` 那段还在」「Bearer 后面那段还在」才是真正要判的事。
const CANARY_FRAGMENTS: [&str; 3] = [
    "canarydeadbeef00112233445566778899",
    "cafebabe0123456789abcdef0123456789",
    "canary-bearer-0123456789abcdef",
];

/// 逐条搜索一个可搜面，命中就报出是哪一处。
fn assert_no_canary(where_: &str, haystack: &str) {
    for fragment in CANARY_FRAGMENTS {
        assert!(
            !haystack.contains(fragment),
            "{where_} 泄露了 canary 片段 {fragment:?}。远程面一旦绑到 LAN，\
             这条通道就是把凭据摊给局域网里的每台机器 —— 可搜面：{haystack}"
        );
    }
}

/// 把 `CC_SWITCH_TEST_HOME` 指向一个临时主目录，退出时还原。
///
/// 设置层（`~/.cc-switch/settings.json`）与 db（`~/.cc-switch/cc-switch.db`）都
/// 是按 home 解析的，两个函数都不接受「传个路径进去」——唯一的隔离手段就是这个
/// 环境变量，而它是**进程级**的：同进程里别的用例也在改它。
///
/// 所以构造时当场复核解析结果确实落在临时目录内。万一这个变量被并跑的别的用例
/// 抢走，这一行会在碰任何文件之前就把用例打红；没有它，下一行
/// `Database::init()` 就会拿用户真实的 `~/.cc-switch/cc-switch.db` 去跑迁移 ——
/// 「测试写坏了用户数据」比「测试红了」严重得多。
///
/// 上面那道复核只能保护**自己**：被抢走的是别人。本文件里所有用例因此都挂了
/// `#[serial_test::serial]` —— 它拿的是 `serial_test` 的进程级全局锁，和
/// `services::provider::tests` 里那些用例用的是同一把。那些用例断言的是
/// `get_codex_auth_path()` 这类**按 home 现解析**的路径，而 `get_home_dir()`
/// 每次调用都重新读这个环境变量（`src/config.rs`）。没有这把锁，本文件的
/// `TestHomeGuard` 在别人的断言跑到一半时把变量换掉，对方就会在临时目录之外
/// 找文件，于是「本该被拦下的写」没被拦下 —— 表现为 `services::provider::tests`
/// 里随机换一条用例变红。删掉这些 `#[serial]` 会把它放回来。
struct TestHomeGuard {
    previous: Option<std::ffi::OsString>,
    /// 临时 home 本体：持有它就等于持有删除权。
    dir: Option<tempfile::TempDir>,
}

impl TestHomeGuard {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create isolated test home");
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());

        let resolved = crate::config::get_app_config_dir();
        assert!(
            resolved.starts_with(dir.path()),
            "隔离 home 解析到了临时目录之外：{}（CC_SWITCH_TEST_HOME 可能被并跑的\
             别的用例抢走了）",
            resolved.display()
        );

        Self {
            previous,
            dir: Some(dir),
        }
    }
}

impl Drop for TestHomeGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(previous) => std::env::set_var("CC_SWITCH_TEST_HOME", previous),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        // 目录删除权就挂在这个字段上。显式 drop 兼作自证：删掉它，临时 home 就
        // 再也不会被删除，隔离出来的产物会一路漏在磁盘上。
        drop(self.dir.take());
    }
}

/// canary 假密钥在五个可搜面上都搜不到：响应体、错误详情、日志、落盘设置文件、
/// 导出的 db 文本。
///
/// **注入点**：凭据库。`clients.json` 里那条记录的 `client_name` 换成 canary、
/// 明文 token 换成 canary 的第三段 —— 于是 canary 变成一把**服务端真的持有、
/// 真的认**的凭据，而不只是「调用方发过来的字符串」。这比拿请求体当注入点更
/// 贴近威胁：断言要挡的是「系统持有的东西被下发出去」，不是「调用方自己的输入
/// 被回显」。
///
/// **前三个出口是「发出去的东西」，后两个是「写下去的东西」**。只扫响应体等于
/// 把落盘面整个漏在外面：canary 哪天被顺手抄进 `settings.json` 或某张 db 表，
/// 响应体断言一条都不会红。后两个出口用生产函数真的保存一次设置、真的导出一次
/// db，再对两份落盘文本扫同一把尺子。
///
/// **为什么可达范围是这些路由**：`/api/usage` 与 `/api/provider/test`（也就是
/// `redact` 闸门的两条真实调用点）在 headless 下取 `AppHandle` 会 panic，
/// `/api/proxy/failover` 同理；`/api/events` 是 SSE，连接不终结。这里逐个跳过
/// 并写明理由，而不是悄悄少测几条。
///
/// **闸门本身**：canary 经生产函数 `redact_error` 走一遍，断言三段形态全部消失、
/// 而不含密钥的正常错误文本逐字保留。只证明「会脱敏」的测试，把全部文本替换成
/// 一个占位符也能通过，所以可读性那半条不能省。
#[serial_test::serial]
#[tokio::test]
async fn canary_secret_never_appears_in_any_response() {
    let logs = log_capture();

    let h = start_headless(RecordingSink::new()).await;
    let base = h.base_url.clone();

    // --- 注入：凭据库里放一条元数据与明文都带 canary 的真实记录 ---
    let cfg_dir = h.cfg_dir.clone();
    let secret = load_or_create(&cfg_dir).expect("读取服务端真实 secret");
    let mut canary_client = ClientToken::new(
        &format!("client-{CANARY_SK}"),
        &secret,
        &format!("client-token-{CANARY_BEARER}"),
    );
    canary_client.cookie_hash = Some(token_hash(&secret, CANARY_HEX));
    let mut clients = load_clients(&cfg_dir).expect("读取真实凭据库");
    clients.push(canary_client.clone());
    save_clients(&cfg_dir, &clients).expect("canary 凭据记录落盘");
    assert!(
        canary_client.verify(&secret, &format!("client-token-{CANARY_BEARER}")),
        "前置：canary 凭据必须真的被中间件认，否则下面的 200 对照验不到「凭据有效」"
    );

    // --- 出口一：响应体 ---
    //
    // `/api/providers` 是 `redact.rs` 里点名的「窄白名单」路由：它返回逐字段挑
    // 过的 DTO，**不经过**脱敏闸门 —— 也就是说这条路由的「不泄露」靠的是 DTO
    // 本身足够窄。把它放在这里，等于把那份窄 DTO 也钉住。
    let sweep: [(&str, Option<&str>); 7] = [
        ("/", None),
        ("/api/providers", Some("Authorization")),
        ("/api/current", Some("Authorization")),
        ("/api/health", Some("Authorization")),
        ("/api/icon", Some("Cookie")),
        ("/api/provider-icons/claude", Some("Cookie")),
        ("/api/reorder", Some("Authorization")),
    ];

    let bearer = format!("client-token-{CANARY_BEARER}");
    let cookie = format!("cc_switch_session={CANARY_HEX}");
    let mut bodies: Vec<(String, String)> = Vec::new();

    for (path, channel) in sweep {
        let (method, target) = if path == "/api/reorder" {
            (
                "POST",
                r#"{"movedId":"no-such-provider","beforeId":""}"#.to_string(),
            )
        } else {
            ("GET", String::new())
        };
        let host = addr_of(&base);
        let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\n");
        match channel {
            Some("Authorization") => {
                request.push_str(&format!("Authorization: Bearer {bearer}\r\n"))
            }
            Some("Cookie") => request.push_str(&format!("Cookie: {cookie}\r\n")),
            // "/" 是 Public：不带凭据也必须可达 —— 它正是「配对入口进得来」那一格。
            _ => {}
        }
        if method == "POST" {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", target.len()));
        }
        request.push_str("Connection: close\r\n\r\n");
        request.push_str(&target);

        let (status, raw) = raw_exchange_text(&base, &request).await;
        assert!(
            (200..300).contains(&status) || status == 401,
            "{path}：canary 凭据必须真的打通这条通道，否则下面的「搜不到」是空的。\
             实际 {status}，raw {raw}"
        );
        assert_no_canary(&format!("GET/POST {path} 的响应全文"), &raw);
        bodies.push((path.to_string(), raw));
    }

    // --- 出口二：错误详情 ---
    //
    // `/api/reorder` 用一个不存在的 provider 走真实错误分支，产出一个真的
    // `error` 字段。断言的是「任何位置上的任何字符串值」都不含 canary，而不仅
    // 是叫 `error` 的那个键 —— 换个字段名再泄漏一次是同一类问题。
    let reorder_body: serde_json::Value = bodies
        .iter()
        .find(|(p, _)| p == "/api/reorder")
        .map(|(_, b)| serde_json::from_str(body_part(b)).unwrap_or(serde_json::Value::Null))
        .unwrap_or(serde_json::Value::Null);
    let mut values = Vec::new();
    all_string_values(&reorder_body, &mut values);
    assert!(
        values.iter().any(|v| v.contains("not found")),
        "正向对照：/api/reorder 应当真的产出一条不含密钥的错误详情，实际 {reorder_body}"
    );
    for value in &values {
        assert_no_canary("/api/reorder 的错误详情", value);
    }

    // --- 出口三：日志 ---
    let captured = logs.lock().expect("日志缓冲锁").clone();
    assert!(
        captured.contains("Listening on"),
        "正向对照：日志缓冲必须真的抓到了服务端写出来的日志，否则「日志里搜不到」\
         是空的（log 的默认上限是 Off，不显式抬高等于什么都没抓）。实际抓到 {} 字节",
        captured.len()
    );
    assert_no_canary("请求处理过程中的服务端日志", &captured);

    // --- 出口四、五：真实落盘面（设置层保存 + db 导出），与上面合并扫描 ---
    //
    // 前面三个出口都是「发出去的东西」：HTTP 响应体、错误详情、日志。canary
    // 还会流经另外两处 —— **设置层保存**与 **db 导出** —— 它们是
    // 「写下去的东西」，只扫响应体等于把这两处整个漏在外面：canary 哪天被顺手
    // 抄进 `settings.json` 或某张表，现有断言一条都不会红。
    //
    // 隔离手段只有 `CC_SWITCH_TEST_HOME` 这一个：设置层与 db 的路径都是按 home
    // 解析的，没有「传个路径进去」的入口（`AppSettings::settings_path` /
    // `Database::init` 各自 `get_home_dir()` / `get_app_config_dir()`）。
    let home = TestHomeGuard::new();

    // 设置层保存：走生产函数 `update_settings`，并真的改一个字段 ——
    // 原样重写一遍证明不了「保存时会不会把内存里的东西一并写出去」。
    let previous_settings = crate::settings::get_settings();
    let mut next_settings = previous_settings.clone();
    next_settings.enable_local_proxy = !next_settings.enable_local_proxy;
    crate::settings::update_settings(next_settings).expect("设置层保存必须成功");
    let settings_path = crate::config::get_home_dir()
        .join(".cc-switch")
        .join("settings.json");
    let settings_text = std::fs::read_to_string(&settings_path)
        .unwrap_or_else(|e| panic!("设置层必须真的落出 settings.json（{settings_path:?}）：{e}"));
    serde_json::from_str::<serde_json::Value>(&settings_text).unwrap_or_else(|e| {
        panic!("落盘的 settings.json 必须是合法 JSON，否则下面扫的是噪声：{e}")
    });
    assert!(
        settings_text.contains("enableLocalProxy"),
        "正向对照：settings.json 里必须真的有刚保存的那个字段，否则「搜不到 canary」\
         可能只是因为写了个空壳：{settings_text}"
    );

    // db 导出：走生产函数 `export_sql_string`，产物就是用户导出/同步时拿到的
    // 那份 SQL 文本。
    let db = crate::database::Database::init().expect("db 必须在隔离 home 下初始化成功");
    // 正向对照用的**哨兵行**：证明 `settings` 表的数据真的流进了导出文本。
    //
    // 这里刻意用一根不含 canary 的探针，而不是把 canary 自己塞进去 ——
    // `export_sql_string` 走的是 `snapshot_to_memory` + `dump_sql`，即一份忠实
    // 的 `sqlite3 .dump`，中间**没有任何脱敏层**。往库里写一行 canary 再断言
    // 导出里搜不到它，等于断言「忠实拷贝会删掉内容」，那条断言只可能红，不可能绿。
    // 这条腿真正要验的是另一件事：canary 凭据存放在 `remote-auth/clients.json`
    // （上面 `save_clients` 已落盘），它**没有**被顺手镜像进 SQLite —— 所以库里
    // 干净、导出里自然干净。
    const EXPORT_PROBE_KEY: &str = "export-probe-key";
    const EXPORT_PROBE_VALUE: &str = "export-probe-value";
    {
        let c = db.conn.lock().unwrap();
        c.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
            [EXPORT_PROBE_KEY, EXPORT_PROBE_VALUE],
        )
        .unwrap();
    }
    let db_dump = db.export_sql_string().expect("db 导出必须成功");
    assert!(
        db_dump.contains("CC Switch SQLite"),
        "正向对照：导出文本必须带 CC Switch 的导出头注释，否则扫的不是真正的导出\
         产物：{}",
        &db_dump[..db_dump.len().min(80)]
    );
    assert!(
        db_dump.contains(EXPORT_PROBE_KEY) && db_dump.contains(EXPORT_PROBE_VALUE),
        "正向对照：导出的 db 文本必须真的带上 settings 表的行，否则下面「搜不到 \
         canary」可能只是因为这张表的数据压根没进导出：共 {} 字节",
        db_dump.len()
    );
    assert!(
        db_dump.contains("CREATE TABLE"),
        "正向对照：导出文本必须真的含建表语句，否则「搜不到 canary」可能只是因为\
         导出为空：共 {} 字节",
        db_dump.len()
    );

    // 合并扫描：响应体文本 + 落盘设置文件 + 导出的 db 文本，同一把尺子过一遍。
    let responses_scanned = bodies.len();
    let mut artifacts = bodies;
    artifacts.push(("落盘设置文件 settings.json".to_string(), settings_text));
    artifacts.push(("导出的 db 文本".to_string(), db_dump));
    assert_eq!(
        artifacts.len(),
        responses_scanned + 2,
        "合并扫描必须真的在 {responses_scanned} 份响应体之外再多出落盘设置文件与 \
         db 导出这两件产物"
    );
    for (label, text) in &artifacts {
        assert_no_canary(label, text);
    }

    // 设置层是进程级单例：把内存里的那份还原，免得同进程里并跑的别的用例读到
    // 这条用例翻过的开关。随后删掉临时 home，等于什么都没发生过。
    crate::settings::update_settings(previous_settings).expect("还原设置层内存态");
    drop(home);

    // --- 脱敏闸门本身：canary 进 → 三段形态全出不来 ---
    //
    // 走的是生产函数 `redact_error`，与 `/api/usage` / `/api/provider/test` 上
    // 那两处调用同一个函数。
    let poisoned = format!(
        "upstream rejected: key={CANARY_SK} digest={CANARY_HEX} \
         sent Authorization: Bearer {CANARY_BEARER}"
    );
    let masked = redact::redact_error(&poisoned);
    assert_no_canary("redact_error 的输出", &masked);
    assert!(
        masked.contains("upstream rejected"),
        "脱敏不得把可读的前后文一起抹掉：{masked}"
    );
    assert!(
        masked.contains("Authorization"),
        "Authorization 这个结构信息必须留着（401 且我们确实发了头，与 401 但没带头，\
         排查动作完全不同）：{masked}"
    );

    // --- 反向约束：不含密钥的正常错误文本逐字保留 ---
    //
    // 和上面那几条放在一起才有意义：只证明「会脱敏」的测试，「把全部文本替换成
    // 一个占位符」也能通过 —— 而那不是堵洞，是把远程排障能力一并删掉。
    for plain in [
        "request timed out after 30s",
        "供应商 anthropic 不存在",
        "HTTP 502 Bad Gateway from https://api.example.com/v1/messages",
        "connection refused",
    ] {
        assert_eq!(
            redact::redact_error(plain),
            plain,
            "不含密钥的正常错误文本被改写了：{plain}"
        );
    }
    // 同一条约束在**请求级**上的体现：错误详情必须仍然读得出发生了什么。
    for value in &values {
        assert!(
            !value.contains(redact::REDACTED),
            "不含密钥的错误文本里不该冒出脱敏占位符：{value}"
        );
    }

    teardown(h).await;
}

/// 从 HTTP 响应全文里截出 body 部分（`\r\n\r\n` 之后）。
fn body_part(raw: &str) -> &str {
    raw.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or(raw)
}

/// 在 loopback 上发一串拼好的请求行，返回 `(状态码, 响应全文)`。
///
/// 与 [`exchange`] 的区别只在于来源：这一族用例不需要把来源钉在别的地址上，
/// 判据完全落在响应内容上。
async fn raw_exchange_text(base: &str, request: &str) -> (u16, String) {
    exchange(base, loopback(), request).await
}

/// 会话 cookie 的四个属性齐全，`Secure` 只在 TLS listener 上出现。
///
/// 这条走**真实请求**：配对 → 批准 → poll，读生产代码在**那一次交付**里下发的
/// `Set-Cookie` 响应头。因此它验的不是「某个字符串拼装函数拼对了」，而是「配对
/// 交付的那一刻真的带上了这四个属性」。
///
/// `Secure` 那条特别要紧：非 TLS 的 listener 上带了它，浏览器会**静默拒收整个
/// cookie** —— 不报错、不提示，用户看到的只是「远程打开就是白屏」。
#[serial_test::serial]
#[tokio::test]
async fn session_cookie_attributes() {
    let h = start_headless(RecordingSink::new()).await;

    let r1 = post(
        &h.base_url,
        "/pairing/request",
        serde_json::json!({"clientName": "cookie-attrs", "requestedScopes": ["read"]}),
    )
    .await;
    let b1: serde_json::Value = step(1, "POST /pairing/request", r1.json().await);
    let user_code = step(1, "取 userCode", field(&b1, "userCode"));
    let poll_token = step(1, "取 pollToken", field(&b1, "pollToken"));
    approve_out_of_band(&h, &user_code).await;

    let resp = poll(&h.base_url, &poll_token).await;
    let status = resp.status().as_u16();
    let set_cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .expect("配对交付凭据的那一次响应必须下发 Set-Cookie，否则浏览器通道根本不存在")
        .to_str()
        .expect("Set-Cookie 必须是可见 ASCII")
        .to_string();
    let body: serde_json::Value = resp.json().await.expect("poll returns JSON");
    assert_eq!(
        status, 200,
        "批准后 poll 必须换到凭据，实际 {status}，body {body}"
    );
    let client_token = step(2, "取 clientToken", field(&body, "clientToken"));

    // 正向对照：这次响应确实交付了凭据（否则下面那些断言验的是一个空壳头）。
    assert!(
        !client_token.is_empty(),
        "正向对照：这一发必须真的换到长期凭据，实际 {body}"
    );

    assert!(
        set_cookie.starts_with("cc_switch_session="),
        "会话 cookie 的名字必须是 cc_switch_session，实际 {set_cookie}"
    );
    for attr in ["HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(
            set_cookie.contains(&format!("; {attr}")),
            "Set-Cookie 缺少 `{attr}`：{set_cookie}"
        );
    }
    assert!(
        !set_cookie.contains("Secure"),
        "当前所有 listener 都是明文 http://（见 auth::REMOTE_LISTENER_IS_TLS），\
         带 Secure 会被浏览器**静默拒收整个 cookie** —— 不报错、不提示，表现正是\
         「远程打开就是白屏」。实际 {set_cookie}"
    );

    // cookie 值与 header token 同签发但取值不同：两个摘要挂在同一条记录上，
    // 复用同一个值等于一次泄漏同时废掉两条通道。
    let value = set_cookie
        .split(';')
        .next()
        .and_then(|pair| pair.split_once('='))
        .map(|(_, v)| v.to_string())
        .expect("Set-Cookie 必须是 name=value; … 的形态");
    assert_ne!(
        value, client_token,
        "会话 cookie 不得复用 header token 的明文：{set_cookie}"
    );

    // `Path=/` 不可收窄：顶层导航 `GET /` 才是内嵌 Web UI 的入口，收窄到 `/api`
    // 等于让浏览器拿着 cookie 却发不出那一次导航，表现仍是白屏。
    assert!(
        !set_cookie.contains("Path=/api"),
        "Path 必须正好是 `/`（根页面是 Web UI 入口）：{set_cookie}"
    );

    teardown(h).await;
}

// ===========================================================================
// 并发与扇出 E2E：正确性级别的并发性质
// ===========================================================================
//
// 这一节**不设吞吐或延迟门禁**：仓里没有 criterion / k6 / wrk，也没有一条已
// 知-good 的基准基线，在这种状态下把耗时写成断言只会得到一条永远红或永远绿的
// 假锁。测出来的数字只写进日志，并注明不构成验收判据。
//
// 要证的是三件**正确性**层面的事：
//
// 1. 限流器并发下不丢更新 —— 50 条同时到达，上限必须**恰好**是 20。
//    单线程顺序发 21 条早就锁过了（`state::tests`），那种跑法即使把
//    `allow_hit` 的「读-改-写」三步拆开也照样绿：不丢更新只有在真的并发
//    争同一把锁时才看得见。
// 2. SSE 扇出不错串 —— 3 条真实在线连接收到的是同一帧；断开一条，其余两条
//    继续收。一个订阅者把消息吞掉、或广播只到第一个订阅者，都只有多连接才
//    抓得住。
// 3. 收尾真的收干净 —— listener 任务已 join、端口可重绑；断言 panic 展开时
//    走的是同一条回收路径。
//
// 三节的同步一律用**显式 ready 信号**（`Barrier` / 广播通道的
// `receiver_count`），不用 `sleep`：定时等待只是在猜「应该到了」，而猜错时
// 表现为偶发红 —— 那是最难查的一种红。

/// 并发打进来的配对请求条数：远大于 [`RATE_LIMIT_MAX`]，让上限成为唯一变量。
const CONCURRENT_PAIRING_REQUESTS: usize = 50;

/// 限流上限。取自生产常量而不是写死 20：常量一改，测试跟着走；若在这里写死，
/// 限流真被放宽时这条锁反而还绿着，那它就白设了。
use super::pairing::RATE_LIMIT_MAX;

/// 从 `base_url` 里取出内核分配的端口。
fn port_of(base: &str) -> u16 {
    base.trim_start_matches("http://")
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("base URL 必须以端口结尾，实际 {base}"))
}

/// 反复尝试绑定，直到成功或超时。
///
/// 用 `yield_now` 而不是 `sleep`：这里等的不是「一段时间过去」，而是「abort
/// 已被 runtime 轮询到、TcpListener 已被 drop」这个**状态**。`sleep` 要选一个
/// 猜出来的时长，选短了偶发红、选长了白等。
async fn rebind_when_free(port: u16, label: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match std::net::TcpListener::bind(("127.0.0.1", port)) {
                Ok(l) => {
                    drop(l);
                    return;
                }
                Err(e) => {
                    eprintln!("{label}：端口 {port} 暂时还绑不上（{e}），继续等");
                    tokio::task::yield_now().await;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{label}：端口 {port} 在 10s 内始终绑不上，listener 没有真正退出"));
}

/// 50 条并发配对请求 → 恰好 [`RATE_LIMIT_MAX`] 条放行，其余全部限流。
///
/// 「恰好」是这条用例的全部价值：少了说明并发下丢了更新（有人被重复放行），
/// 多了说明限流判据读到了陈旧状态。两种错误实现都能在顺序跑的旧用例下保持全绿。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rate_limit_concurrent_50_yields_exactly_20() {
    let h = start_headless(RecordingSink::new()).await;
    let base = h.base_url.clone();

    // 一个共享 client：每条自建 client 会各自建连接池，50 个池子既慢又把
    // 「并发」悄悄降级成「内核排队」。
    let client = reqwest::Client::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(CONCURRENT_PAIRING_REQUESTS));

    let started = std::time::Instant::now();
    let mut tasks = Vec::with_capacity(CONCURRENT_PAIRING_REQUESTS);
    for _ in 0..CONCURRENT_PAIRING_REQUESTS {
        let client = client.clone();
        let base = base.clone();
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            // 显式 ready 信号：50 条全部就位后才一起冲出去，于是服务端面对的
            // 是真正的并发到达，而不是「恰好在窗口内的一条条来」。
            barrier.wait().await;
            client
                .post(format!("{base}/pairing/request"))
                .json(&serde_json::json!({
                    "clientName": "cli",
                    "requestedScopes": ["read"],
                }))
                .send()
                .await
                .map(|r| r.status().as_u16())
        }));
    }

    let mut ok_count = 0usize;
    let mut limited_count = 0usize;
    for t in tasks {
        let status = t
            .await
            .expect("并发任务不得 panic")
            .expect("并发 POST /pairing/request 必须到达真实 listener");
        match status {
            200 => ok_count += 1,
            429 => limited_count += 1,
            other => panic!(
                "并发 POST /pairing/request 只应得到 200（放行）或 429（限流），\
                 实际 {other} —— 第三种状态说明并发路径上有别的分支漏进来了"
            ),
        }
    }
    let elapsed = started.elapsed();

    assert_eq!(
        ok_count, RATE_LIMIT_MAX as usize,
        "50 条并发里必须**恰好** {RATE_LIMIT_MAX} 条放行。少了 = 并发下丢了更新\
         （同一次窗口被重复放行），多了 = 限流判据读到了陈旧状态"
    );
    assert_eq!(
        limited_count,
        CONCURRENT_PAIRING_REQUESTS - RATE_LIMIT_MAX as usize,
        "限流条数必须与放行条数互补"
    );

    // 性能基线只记录不设门禁。
    let note = format!(
        "[perf-baseline，非验收判据] {CONCURRENT_PAIRING_REQUESTS} 条并发配对请求耗时 {elapsed:?}\
         （放行 {ok_count} / 限流 {limited_count}）"
    );
    println!("{note}");
    eprintln!("{note}");

    teardown(h).await;
}

/// 从 SSE 响应里逐帧取出 `data:` 载荷，取完即从缓冲移除。
///
/// 按帧切而不是一次性 `data_stream()`：后者只验「流里有东西」，验不出「三条
/// 连接收到的是**同一帧**」—— 那正是本节要锁的东西。
fn take_sse_frame(buf: &mut String) -> Option<String> {
    const MARK: &str = "data: ";
    let pos = buf.find(MARK)?;
    let start = pos + MARK.len();
    let end = start + buf[start..].find('\n')?;
    let frame = buf[start..end].trim().to_string();
    let consumed = end + 1;
    buf.drain(..consumed);
    if frame.is_empty() {
        None
    } else {
        Some(frame)
    }
}

/// 读到 N 帧为止，逐帧带上连接编号。
///
/// 收集 N 帧而不是「读若干秒看看收到什么」：后者在漏发一帧时会静默变绿。
async fn collect_frames(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<(usize, String)>,
    want: usize,
    label: &str,
) -> Vec<(usize, String)> {
    let mut got = Vec::with_capacity(want);
    for i in 0..want {
        let (id, frame) = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "{label}：第 {} 帧在 10s 内没到（已收到 {got:?}）—— 扇出不完整，\
                     或有连接根本没订阅上",
                    i + 1
                )
            })
            .unwrap_or_else(|| panic!("{label}：所有读帧任务都已退出，SSE 流提前结束"));
        got.push((id, frame));
    }
    got
}

/// 等到广播通道上有至少 `want` 个订阅者。
///
/// `receiver_count` 是**服务端订阅状态本身**，不是「过了一段时间应该连上了」。
/// 没有它就只能在 `send()` 返回与 handler 真正执行 `subscribe()` 之间赌一个
/// 时序窗口。
async fn wait_for_receivers(tx: &tokio::sync::broadcast::Sender<String>, want: usize) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while tx.receiver_count() < want {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "10s 内广播通道上的订阅者没到 {want} 个（实际 {}）—— 连接没真正订阅上，\
             后面任何「收到了帧」的断言都会是空的",
            tx.receiver_count()
        )
    });
}

/// 走一遍真实配对链路换到**会话 cookie** 的明文值。
///
/// SSE（`/api/events`）是 `BrowserSafe`：它的载体正是 `EventSource`，协议层放不了
/// `Authorization` 头，所以只有会话 cookie 这条通道能让它拿到已鉴权档位
/// （`e2e_web_ui_loads_over_nonloopback` 的「反证二」已钉死 Bearer 顶不上这条路）。
/// 值来自生产签发路径下发的 `Set-Cookie`，不是测试自己造的。
async fn paired_session_cookie(h: &Harness) -> String {
    use super::auth::{parse_session_cookie, SESSION_COOKIE_NAME};

    let r1 = post(
        &h.base_url,
        "/pairing/request",
        serde_json::json!({"clientName": "web-ui", "requestedScopes": ["read"]}),
    )
    .await;
    let b1: serde_json::Value = step(1, "POST /pairing/request", r1.json().await);
    let user_code = step(1, "取 userCode", field(&b1, "userCode"));
    let poll_token = step(1, "取 pollToken", field(&b1, "pollToken"));

    approve_out_of_band(h, &user_code).await;

    let r = poll(&h.base_url, &poll_token).await;
    let set_cookie = r
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| {
            panic!("配对交付凭据时必须下发会话 cookie，否则浏览器端 EventSource 永远匿名")
        })
        .to_string();
    parse_session_cookie(&set_cookie)
        .unwrap_or_else(|| panic!("Set-Cookie 里必须有 {SESSION_COOKIE_NAME}：{set_cookie}"))
}

/// 3 条真实 SSE 连接同时在线：一次切换三者收到**同一帧**；断开一条，另两条照收。
///
/// 单连接用例（`e2e_pair_then_call_then_revoke` 第 7 步）锁的是「订阅了就能收到
/// 一帧」。这条锁的是单连接验不到的两件事：消息没有在扇出途中被某个订阅者吞掉，
/// 以及某个订阅者的断开不会牵连其余订阅者。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sse_fanout_reaches_all_live_connections() {
    const CONNECTIONS: usize = 3;

    let h = start_headless(RecordingSink::new()).await;
    let cookie = paired_session_cookie(&h).await;
    let sse_tx = h.server.state().sse_tx.clone();

    let client = reqwest::Client::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(usize, String)>();
    let mut readers = Vec::with_capacity(CONNECTIONS);
    for id in 0..CONNECTIONS {
        let client = client.clone();
        let base = h.base_url.clone();
        let cookie = cookie.clone();
        let tx = tx.clone();
        readers.push(tokio::spawn(async move {
            let resp = client
                .get(format!("{base}/api/events"))
                .header(
                    axum::http::header::COOKIE,
                    format!("cc_switch_session={cookie}"),
                )
                .send()
                .await
                .expect("SSE 订阅必须能连上真实 listener");
            assert_eq!(
                resp.status().as_u16(),
                200,
                "带有效会话 cookie 订阅 /api/events 必须 200，实际 {}",
                resp.status().as_u16()
            );
            use futures::StreamExt;
            let mut stream = resp.bytes_stream();
            let mut buf = String::new();
            while let Some(chunk) = stream.next().await {
                let Ok(chunk) = chunk else { break };
                buf.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(frame) = take_sse_frame(&mut buf) {
                    if tx.send((id, frame)).is_err() {
                        return;
                    }
                }
            }
        }));
    }

    // 显式 ready：三条都在服务端 subscribe 之后才广播，否则「收到帧」可能只是
    // 恰好赶上了一个还没订阅完的连接。
    wait_for_receivers(&sse_tx, CONNECTIONS).await;

    // --- 第一次切换：三者同帧 ---
    let frame_a = serde_json::json!({"type":"switch","provider_id":"p1","name":"one"}).to_string();
    assert!(
        sse_tx.send(frame_a.clone()).is_ok(),
        "首次广播必须有订阅者 —— 上面已等齐 {CONNECTIONS} 个"
    );
    let got_a = collect_frames(&mut rx, CONNECTIONS, "首次扇出").await;
    for (id, frame) in &got_a {
        assert_eq!(
            frame, &frame_a,
            "连接 {id} 收到的帧与广播内容不一致：扇出途中被改写或串了别的订阅者的数据"
        );
    }
    let mut ids_a: Vec<usize> = got_a.iter().map(|(id, _)| *id).collect();
    ids_a.sort_unstable();
    assert_eq!(
        ids_a,
        vec![0, 1, 2],
        "首次切换必须三条连接都收到一帧，实际来自 {ids_a:?}"
    );

    // --- 断开中间那条 ---
    let dropped_reader = readers.remove(1);
    dropped_reader.abort();
    let _ = dropped_reader.await;
    wait_for_receivers(&sse_tx, CONNECTIONS - 1).await;

    // --- 第二次切换：另两条照收，且被断开的那个不再有帧 ---
    let frame_b = serde_json::json!({"type":"switch","provider_id":"p2","name":"two"}).to_string();
    assert!(sse_tx.send(frame_b.clone()).is_ok(), "二次广播必须有订阅者");
    let got_b = collect_frames(&mut rx, CONNECTIONS - 1, "断开一条之后的扇出").await;
    for (id, frame) in &got_b {
        assert!(
            *id != 1,
            "已断开的连接 1 不该再收到帧（收到 {frame}）—— 说明它其实没断开，\
             「断开不影响其余」这一格就等于没验"
        );
        assert_eq!(
            frame, &frame_b,
            "连接 {id} 在一条订阅者断开后收到的帧与广播内容不一致"
        );
    }
    let mut ids_b: Vec<usize> = got_b.iter().map(|(id, _)| *id).collect();
    ids_b.sort_unstable();
    assert_eq!(ids_b, vec![0, 2], "断开后必须恰好剩下 0/2 两条在收");

    // 断开的那条不该有残留帧混进来：反证「收到了 2 帧」不是因为收重了自己的。
    assert!(
        rx.try_recv().is_err(),
        "断开连接 1 后又冒出多余帧 —— 断开没有真正生效"
    );

    // 收尾：本用例启动的读帧任务一个都不留。
    for r in readers {
        r.abort();
        let _ = r.await;
    }
    drop(tx);
    teardown(h).await;
}

/// lagged 必须被显式断言：`RecvError::Lagged(n)` 里的 `n` 与实际丢弃条数一致。
///
/// SSE 路由对 lagged 是 `continue`（跳过丢掉的帧继续收），于是**表面上永远看不到
/// lagged** —— 一条「广播溢出时事件会悄悄少掉」的实现能在这个路由下保持全绿。
/// 所以这条用例绕过路由直接面对广播通道本身：接收者刻意不读，把容量灌溢出，
/// 再断言报出来的 `n` 与随后**实际能收到的条数**互补。
///
/// 判据写成「`n + 实际收到 = 发出总数`」而不是写死容量：容量是
/// `RemoteServer::new` 里的实现细节，把它抄进断言只会在有人调容量时把一条
/// 与本节无关的改动变成红。
#[serial_test::serial]
#[tokio::test]
async fn broadcast_lagged_reports_exact_dropped_count() {
    const SENT: usize = 40;

    let h = start_headless(RecordingSink::new()).await;
    let sse_tx = h.server.state().sse_tx.clone();

    // 订阅之后**一次都不收**：这就是「刻意变慢的接收者」，制造溢出靠的是不读，
    // 不是靠 sleep 拖时间。
    let mut slow = sse_tx.subscribe();

    for i in 0..SENT {
        sse_tx
            .send(serde_json::json!({"type":"switch","seq": i}).to_string())
            .expect("至少有一个订阅者，发送不该失败");
    }

    let lagged = match slow.recv().await {
        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => n as usize,
        Err(other) => panic!("接收者被灌了 {SENT} 条却收到 {other:?}，没有发生 lagged"),
        Ok(frame) => panic!("接收者一次都没读却先收到了帧 {frame}，溢出没发生，判据是空的"),
    };
    assert!(
        lagged > 0 && lagged < SENT,
        "Lagged({lagged}) 落在 (0, {SENT}) 之外 —— 报出来的条数与实际丢弃量对不上"
    );

    // lagged 之后通道里剩下的，恰好是 lagged 之后的那些。
    let mut received_after = 0usize;
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), slow.recv()).await {
            Ok(Ok(_)) => received_after += 1,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                panic!("一次 lagged 之后不该再有第二次 lagged —— 接收者已经把剩下的读空了")
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                panic!("发送端还活着时通道不应关闭")
            }
            Err(_) => break,
        }
    }

    assert_eq!(
        lagged + received_after,
        SENT,
        "Lagged 报的 {lagged} 条 + 随后实收 {received_after} 条必须正好等于发出的 {SENT} 条"
    );

    teardown(h).await;
}

/// 收尾之后 listener 任务确实已 join、端口可重绑；断言 panic 展开时同一条回收路径。
///
/// 端口能重绑只是**辅助**证据：它能成立也可能是因为 listener 被 abort 了（abort
/// 同样会 drop 掉 `TcpListener`），而 abort 路径不保证任务跑到收尾。主断言是
/// `stop()` 之后 listener 表被清空、运行标志已翻转 —— 那才说明每个 listener 的
/// `JoinHandle` 都被 await 过。
#[serial_test::serial]
#[tokio::test]
async fn teardown_leaves_no_running_task() {
    let h = start_headless(RecordingSink::new()).await;
    let port = port_of(&h.base_url);
    let server = Arc::clone(&h.server);

    assert_eq!(
        server.state.listeners.read().await.len(),
        1,
        "headless 只应起一个 loopback listener"
    );

    teardown(h).await;

    assert!(
        server.state.listeners.read().await.is_empty(),
        "stop() 之后 listener 表必须被清空 —— 还有条目就意味着某个 listener 任务\
         仍在跑（或没被 join）"
    );
    assert!(
        !server
            .state()
            .running
            .load(std::sync::atomic::Ordering::SeqCst),
        "stop() 之后运行标志必须翻转为 false，否则已建立连接上的请求仍会被受理"
    );

    // 辅助断言：端口可重绑（不设门禁意义之外的含义，仅作旁证）
    rebind_when_free(port, "正常收尾").await;

    // panic 路径：断言展开时 `Drop` 走的是同一条回收路径。
    //
    // 这里不用 `catch_unwind` 真造一次 panic（那会打印噪声且让这条用例自己也
    // 变得难读），而是直接把 `Harness` 丢掉 —— `Drop` 里做的正是断言失败时
    // 展开会走的那段：abort listener 任务。于是「panic 时也收得干净」被单独验到。
    let dropped = start_headless(RecordingSink::new()).await;
    let dropped_port = port_of(&dropped.base_url);
    assert_ne!(
        dropped_port, port,
        "两个夹具必须拿到不同的内核端口，否则两次断言量的不是同一个端口"
    );
    drop(dropped);
    rebind_when_free(dropped_port, "Drop 回收路径").await;
}

/// SSE 会话档位 E2E：匿名本机连接拿到**降级流**（不是 401），带凭据的拿到全量流。
///
/// 判定格第 7 格（Loopback / 无 Origin / 无代理信号）对 `Cred::None` 判 `Allow` ——
/// 那是内嵌 Web UI 存在的理由，不能改成 401。但「放行」不等于「放行全部细节」：
/// 全量切换事件流里的 `provider_id` / `name` 是供应商拓扑，本机无凭据调用方
/// （同机的任何进程、任何被诱导去连这个端口的东西）没有理由读到它。
///
/// 两条腿缺任何一条都有**错误实现能保持全绿**：
/// - 只验匿名腿 → 把 `/api/events` 改成永远 200 空流，测试照样绿；
/// - 只验凭据腿 → 把 handler 写死成降级流，远程浏览器永远看不到切换，测试照样绿。
#[serial_test::serial]
#[tokio::test]
async fn loopback_sse_without_credentials_gets_a_degraded_stream() {
    let h = start_headless(RecordingSink::new()).await;

    // 本机、无凭据：判定格第 7 格放行 → 必须是 200 的事件流。
    let sse = reqwest::Client::new()
        .get(h.url("/api/events"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET /api/events 必须打到真实 listener：{e}"));
    assert_eq!(
        sse.status().as_u16(),
        200,
        "loopback 无凭据打 /api/events 必须是 200 降级流，不能是 401 —— \
         401 会与判定格第 7 格（内嵌页面无凭据可达）冲突"
    );

    // 走与 tray / failover_switch / provider 切换命令同一条生产广播通道发帧。
    h.server
        .state()
        .sse_tx
        .send(
            serde_json::json!({"type":"switch","provider_id":"p-secret","name":"Secret Provider"})
                .to_string(),
        )
        .expect("SSE 通道必须可广播");
    let frame = first_sse_frame(sse).await;
    let parsed: serde_json::Value = serde_json::from_str(&frame)
        .unwrap_or_else(|e| panic!("SSE 帧必须是合法 JSON（{frame:?}）：{e}"));

    assert_eq!(
        parsed["type"], "switch",
        "降级流仍须让客户端知道「发生了切换」，否则前端不会刷新：{parsed}"
    );
    assert_eq!(
        parsed["degraded"], true,
        "降级流必须带显式降级标记，否则调用方无法区分「降级」与「无事件」：{parsed}"
    );
    assert!(
        !frame.contains("provider_id"),
        "匿名连接不得收到 provider_id —— 供应商拓扑不该外泄给同机无凭据调用方：{frame}"
    );
    assert!(
        !frame.contains("Secret Provider"),
        "匿名连接不得收到 provider 名：{frame}"
    );

    teardown(h).await;
}

/// 同一条路由、带**真实签发**的会话 cookie：必须拿到全量帧。
///
/// 凭据通道取 cookie 而不是 Bearer：`/api/events` 是 `BrowserSafe`，载体正是
/// `EventSource`，协议层放不了 header；e2e_web_ui_loads_over_nonloopback 的
/// 「反证二」已经把「Bearer 顶不上 cookie 通道」钉死了。
#[serial_test::serial]
#[tokio::test]
async fn loopback_sse_with_session_cookie_receives_full_frames() {
    use super::auth::SESSION_COOKIE_NAME;

    let h = start_headless(RecordingSink::new()).await;

    // 走真实配对拿真会话 cookie（`Set-Cookie` 由生产签发路径下发）。
    let session_value = paired_session_cookie(&h).await;

    let sse = reqwest::Client::new()
        .get(h.url("/api/events"))
        .header(
            axum::http::header::COOKIE,
            format!("{SESSION_COOKIE_NAME}={session_value}"),
        )
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET /api/events 必须打到真实 listener：{e}"));
    assert_eq!(
        sse.status().as_u16(),
        200,
        "带会话 cookie 打 /api/events 必须 200"
    );

    h.server
        .state()
        .sse_tx
        .send(
            serde_json::json!({"type":"switch","provider_id":"p-auth","name":"Authed Provider"})
                .to_string(),
        )
        .expect("SSE 通道必须可广播");
    let frame = first_sse_frame(sse).await;
    let parsed: serde_json::Value = serde_json::from_str(&frame)
        .unwrap_or_else(|e| panic!("SSE 帧必须是合法 JSON（{frame:?}）：{e}"));

    assert_eq!(parsed["type"], "switch", "{parsed}");
    assert_eq!(
        parsed["provider_id"], "p-auth",
        "持有效凭据的连接必须拿到完整 provider_id：{parsed}"
    );
    assert_eq!(
        parsed["name"], "Authed Provider",
        "持有效凭据的连接必须拿到完整 provider 名：{parsed}"
    );
    assert!(
        parsed.get("degraded").is_none(),
        "已鉴权连接不得被标成降级：{parsed}"
    );

    teardown(h).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[serial_test::serial]
    #[tokio::test]
    async fn remote_server_starts_without_app_handle() {
        let h = start_headless(RecordingSink::new()).await;

        // 真的能用：请求生产 router 上的 /api/health
        let resp = reqwest::get(h.url("/api/health"))
            .await
            .expect("headless server must answer /api/health");
        assert_eq!(resp.status().as_u16(), 200, "health must be ok");
        let body: serde_json::Value = resp.json().await.expect("health returns JSON");
        assert_eq!(body["status"], "ok");

        teardown(h).await;
    }

    // =======================================================================
    // 脱敏闸门：/api/usage 与 /api/provider/test 的错误响应不得含密钥哨兵
    // =======================================================================
    //
    // 静态门禁（`tests/redact_gate.rs`）锁的是「源码里每个 `"error":` 取值都
    // 过了脱敏出口」。它锁不到的那一环是：出口本身接上去之后，真实 HTTP 响应
    // 里确实看不到密钥。两条都要 —— 只有静态门禁时，一个「脱敏了但拼错了字段」
    // 的实现能全绿。
    //
    // 哨兵刻意做成**会被 redact_error 命中**的三种形态（`sk-` 前缀键、
    // `Authorization: Bearer` 令牌、64 位长十六进制），且真的经由生产注入点
    // 进入 handler：
    //
    // - `/api/provider/test`：`providerId` 未命中时被原样嵌进错误文本，
    //   所以把哨兵放进 providerId 就是一条真实的「调用方可控文本 → 响应」通路。
    // - `/api/usage`：夹具 provider 的用量脚本在 `{{apiKey}}` 处拿到明文 key
    //   （这正是 `redact.rs` 模块文档描述的透传源），脚本把它连同另两种哨兵
    //   一起抛出来，错误文本逐字经过 `ProviderUsageDto.error`。

    /// `sk-` 前缀的 Anthropic 形态密钥哨兵。
    const SK_SENTINEL: &str = "sk-ant-api03-REDACTSENTINEL0123456789abcdefXYZ";
    /// `Authorization: Bearer` 形态的令牌哨兵（正则要求头名同现，孤立一个
    /// `Bearer …` 不会被命中 —— 那样这条断言就只是在验一个没接上的东西）。
    const BEARER_SENTINEL: &str = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.REDACTSENTINELsig";
    /// 64 位长十六进制哨兵（无前缀裸密钥形态）。
    const HEX_SENTINEL: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// 全部哨兵片段。响应体里出现任何一个都算泄漏。
    fn sentinels() -> [&'static str; 3] {
        [SK_SENTINEL, "REDACTSENTINELsig", HEX_SENTINEL]
    }

    /// 断言响应体不含任何哨兵，且**含脱敏占位符**。
    ///
    /// 后半个断言是这条用例的非空转保证：只断言「搜不到哨兵」的话，一个把
    /// 整段错误文本抹成空串、或干脆没走到错误分支的实现同样能绿。占位符证明
    /// 哨兵真的到了脱敏出口、并且是被识别后替换掉的。
    fn assert_no_sentinel(label: &str, body: &str) {
        for sentinel in sentinels() {
            assert!(
                !body.contains(sentinel),
                "{label} 的响应体里仍带密钥哨兵片段 {sentinel:?}\nbody: {body}"
            );
        }
        assert!(
            body.contains(redact::REDACTED),
            "{label} 的响应体里没有脱敏占位符 —— 哨兵可能压根没走到错误分支，\
             这条断言就是空的\nbody: {body}"
        );
    }

    /// 走完整配对流程换一个可调用受保护路由的 token。
    async fn paired_client_token(h: &Harness) -> String {
        let base = h.base_url.clone();
        let requested = post(
            &base,
            "/pairing/request",
            serde_json::json!({"clientName": "cli", "requestedScopes": ["read"]}),
        )
        .await;
        let body: serde_json::Value = step(1, "POST /pairing/request", requested.json().await);
        let user_code = step(1, "取 userCode", field(&body, "userCode"));
        let poll_token = step(1, "取 pollToken", field(&body, "pollToken"));

        approve_out_of_band(h, &user_code).await;

        let polled = poll(&base, &poll_token).await;
        let body: serde_json::Value = step(2, "POST /pairing/poll", polled.json().await);
        step(2, "取 clientToken", field(&body, "clientToken"))
    }

    /// 带 `Authorization: Bearer` 发一个 JSON POST。
    async fn post_auth(
        base: &str,
        token: &str,
        path: &str,
        body: serde_json::Value,
    ) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("{base}{path}"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .unwrap_or_else(|e| panic!("POST {path} must reach the live listener: {e}"))
    }

    /// 往 headless 的进程内 `AppState` 塞一个「用量脚本会把密钥回显出来」的
    /// provider。
    ///
    /// 走 [`super::handlers::headless_app_state`] 而不是自建 `AppState`：handler
    /// 读的是那个进程内单例，自建一个的话用例验的就是另一个对象。
    fn seed_leaky_usage_provider() {
        use crate::provider::{Provider, ProviderMeta, UsageScript};

        let app_state = crate::remote::handlers::headless_app_state();
        let provider = Provider {
            id: "redact-gate-fixture".to_string(),
            name: "redact gate fixture".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                usage_script: Some(UsageScript {
                    enabled: true,
                    language: "javascript".to_string(),
                    // 用量脚本的错误文本会被逐字折进 `UsageResult.error`：非法
                    // HTTP 方法这条分支把方法名原样带进 `不支持的 HTTP 方法: {}`，
                    // 于是明文 key（`{{apiKey}}` 处拿到的就是明文，见
                    // services/provider/usage.rs 的变量替换）与另两种哨兵一起
                    // 走到了错误文本里 —— 与真实脚本回显变量是同一条通路。
                    code: format!(
                        r#"(({{
                            request: {{
                                url: "https://usage.invalid/v1/usage",
                                method: "GET {{{{apiKey}}}} {BEARER_SENTINEL} {HEX_SENTINEL}",
                                headers: {{}},
                                body: null
                            }},
                            extractor: function (response) {{ return response; }}
                        }}))"#
                    ),
                    timeout: Some(5),
                    api_key: Some(SK_SENTINEL.to_string()),
                    base_url: None,
                    access_token: None,
                    user_id: None,
                    template_type: None,
                    auto_query_interval: None,
                    coding_plan_provider: None,
                    access_key_id: None,
                    secret_access_key: None,
                    team_organization_id: None,
                    team_project_id: None,
                }),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        app_state
            .db
            .save_provider("claude", &provider)
            .expect("headless 夹具 provider 必须能落进内存库");
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn error_bodies_from_usage_and_provider_test_carry_no_key_material() {
        seed_leaky_usage_provider();

        let h = start_headless(RecordingSink::new()).await;
        let base = h.base_url.clone();
        let token = paired_client_token(&h).await;

        // ── /api/provider/test：调用方可控文本被嵌进错误消息 ──
        let resp = post_auth(
            &base,
            &token,
            "/api/provider/test",
            serde_json::json!({
                "providerId": format!("{SK_SENTINEL} {BEARER_SENTINEL} {HEX_SENTINEL}"),
                "appType": "claude",
            }),
        )
        .await;
        let status = resp.status().as_u16();
        let body = resp.text().await.expect("/api/provider/test 返回文本体");
        assert_eq!(
            status, 200,
            "供应商不存在这条分支历史上就是 200 + success:false，\
             改成别的状态码会让远程调用方的判据失效\nbody: {body}"
        );
        assert!(
            body.contains("不存在"),
            "错误分支根本没走到（找不到「不存在」），下面的脱敏断言就是空的\nbody: {body}"
        );
        assert_no_sentinel("/api/provider/test", &body);

        // ── /api/usage：用量脚本异常里的明文 key ──
        let resp = call(&base, &token, "/api/usage").await;
        let status = resp.status().as_u16();
        let body = resp.text().await.expect("/api/usage 返回文本体");
        assert_eq!(status, 200, "/api/usage 必须可达\nbody: {body}");
        assert!(
            body.contains("redact gate fixture"),
            "夹具 provider 没出现在用量结果里，脚本根本没跑，脱敏断言就是空的\nbody: {body}"
        );
        assert_no_sentinel("/api/usage", &body);

        teardown(h).await;
    }

    #[serial_test::serial]
    #[test]
    fn harness_uses_production_construction() {
        // 自建 Router 会让 E2E 绕开生产装配路径（路由表、鉴权、事件出口全都能漂移）
        //
        // 匹配串在本行拼出来而不是写成字面量：这个检测器扫描的正是它所在的源文件，
        // 写成字面量会把自己判成违规，永远红。
        let needle = ["Router", "::new("].concat();
        let src = include_str!("e2e.rs");
        let offenders: Vec<&str> = src
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && l.contains(&needle))
            .collect();
        assert!(
            offenders.is_empty(),
            "e2e.rs 不得自建 Router，必须走生产 RemoteServer::new + build_router + \
             start_addrs；发现: {offenders:?}"
        );
    }

    // =======================================================================
    // Protected 的浏览器通道：会话 cookie 必须能过凭据闸门
    // =======================================================================
    //
    // `AuthScope::Protected` 从「只认 `Authorization: Bearer`」改成
    // 「Bearer 优先、没带 Bearer 时回落到会话 cookie」。本节钉住新通道，以及
    // 它**没有**放宽的四件事。
    //
    // 为什么必须单独立一节：此前整个 E2E 套件里**没有任何一条断言**打的是
    // 「Protected 路由 + 会话 cookie」。所有 `Cookie` 用法都落在 `/api/icon` 与
    // `/api/provider-icons/:name`，而这两条是 `BrowserSafe`。于是
    // `Protected` 的通道表少列一条 Cookie 时，全仓没有一条断言会红 —— 症状恰好是
    // 「远端浏览器打开内嵌 Web UI 之后页面能开、页面里每一个 `fetch('/api/…')`
    // 都 401，什么都点不动」。

    /// 真实配对一次，交出**同一条凭据记录**上的三样东西：cookie 明文、`clientId`、
    /// header token 明文。
    ///
    /// [`paired_session_cookie`] 只给 cookie，[`paired_client_token`] 只给 token，
    /// 而「撤销后 cookie 立刻失效」那一格要求 cookie 与 `clientId` 出自**同一次**
    /// poll：分两次配对拿来的两样东西分属两条记录，撤销一条碰不到另一条，
    /// 那一格就会绿在一个与它无关的事实上。
    async fn paired_credential(h: &Harness) -> (String, String, String) {
        let base = h.base_url.clone();
        let requested = post(
            &base,
            "/pairing/request",
            serde_json::json!({"clientName": "browser-channel-probe", "requestedScopes": ["read"]}),
        )
        .await;
        let body: serde_json::Value = step(1, "POST /pairing/request", requested.json().await);
        let user_code = step(1, "取 userCode", field(&body, "userCode"));
        let poll_token = step(1, "取 pollToken", field(&body, "pollToken"));

        approve_out_of_band(h, &user_code).await;

        let polled = poll(&base, &poll_token).await;
        // Set-Cookie 必须在读 body 之前取走：`json()` 会把响应消费掉。
        let set_cookie = polled
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| {
                panic!("交付凭据的那次 poll 必须下发 Set-Cookie，否则浏览器通道根本不存在")
            })
            .to_string();
        let body: serde_json::Value = step(2, "POST /pairing/poll", polled.json().await);
        let session = parse_session_cookie(&set_cookie).unwrap_or_else(|| {
            panic!("Set-Cookie 里必须能解出 cc_switch_session 的值：{set_cookie}")
        });

        (
            session,
            step(2, "取 clientId", field(&body, "clientId")),
            step(2, "取 clientToken", field(&body, "clientToken")),
        )
    }

    /// 逐格覆盖 `Protected` 上的会话 cookie 通道。
    ///
    /// 每发请求都过 [`gated_channel()`]，不是直接打 `h.base_url`：判定表第 7/9 格
    /// 对**纯** loopback 来源无条件放行（内嵌 Web UI 在配对之前就要能打开），
    /// 于是在 loopback 上「cookie 认不认」根本测不出东西 —— 实现怎么写都 200。
    ///
    /// 除 A′ 外每一格都打 `/api/providers`（Protected 里最典型的只读路由）；
    /// A′ 才遍历整张 Protected 分区。
    #[serial_test::serial]
    #[tokio::test]
    async fn protected_routes_accept_session_cookie_from_browser() {
        let gated = gated_channel().await;
        let h = start_headless(RecordingSink::new()).await;
        let base = h.base_url.clone();
        let (cookie, client_id, client_token) = paired_credential(&h).await;

        // ── A：改动本体的正例 ──
        let (a, body_a) = gated
            .call_with_cookie(&base, &cookie, "/api/providers")
            .await;
        assert_eq!(
            a, 200,
            "A：会话 cookie 打 Protected 路由 /api/providers 必须 200（通道：{}）—— \
             这正是本次改动的本体：远端浏览器打开内嵌 Web UI 后，页面里的 \
             `fetch('/api/providers')` 只能靠浏览器自动带 cookie。实际 {a}，body {body_a}",
            gated.label
        );

        // ── A′：全部 Protected 路由（含写路由，方法口径见下）──
        //
        // 方法一律用 GET。六条写路由只注册了 POST，于是它们的成功响应是 405
        // 而不是 200 —— 本格验的是**鉴权层**的出口，而 `register()` 把中间件挂在
        // 整个 `MethodRouter` 上（含 405 的 fallback），所以错动词照样先过鉴权层。
        // 因此这一格对写路由断言的是「不是 401」而不是「是 200」：
        // `405` 只可能出现在鉴权层已经放行之后，而同一路径用无效 cookie 打（B 格）
        // 会得到 401 —— 两条一起才说明这条路由认得这个 cookie。
        // 把期望放宽成 `!= 401` 的只有写路由这半边，只读的那半边仍钉死 200。
        let paths = protected_paths();
        assert!(
            !paths.is_empty(),
            "ROUTE_SCOPE_TABLE 里必须有 Protected 路由，否则本用例什么都没验"
        );
        for path in &paths {
            let (status, body) = gated.call_with_cookie(&base, &cookie, path).await;
            assert_ne!(
                status, 401,
                "A′：Protected 路由 {path} 带有效会话 cookie 不得 401（通道：{}）—— \
                 cookie 通道没接上时，远端浏览器打开的内嵌页面每一个 fetch 都会死在这里。\
                 实际 {status}，body {body}",
                gated.label
            );
            if READ_ONLY_PROTECTED.contains(&path.as_str()) {
                assert_eq!(
                    status, 200,
                    "A′：只读的 Protected 路由 {path} 带有效会话 cookie 必须 200（通道：{}），\
                     实际 {status}，body {body}",
                    gated.label
                );
            }
        }

        // ── B：无效 cookie 仍 401 ──
        // 没有这一格，A/A′ 的 200 只说明「带 Cookie 头就行」。
        let (b, body_b) = gated
            .call_with_cookie(&base, "not-the-session-value", "/api/providers")
            .await;
        assert_eq!(
            b, 401,
            "B：错误的会话 cookie 必须仍 401（通道：{}），否则 cookie 通道等于没验过。\
             实际 {b}，body {body_b}",
            gated.label
        );

        // ── C：Bearer 回归 ──
        // 这次改动不能把本来就通的那条路弄坏。
        let (c, body_c) = gated
            .call_with_bearer(&base, &client_token, "/api/providers")
            .await;
        assert_eq!(
            c, 200,
            "C：Bearer 通道必须不受本次改动影响，仍然 200（通道：{}），实际 {c}，body {body_c}",
            gated.label
        );

        // ── D：不回落 ──
        // 某个载体一旦在请求里**出现**了，它就必须验过：带了错的 Bearer 再挂一条
        // 有效 cookie 仍是 401。理由不是「更严更好」，而是让「你拿来证明身份的
        // 东西」和「实际让你进来的东西」是同一件 —— 否则一条被投毒的
        // `Authorization` 头会让调用方以为自己用的是另一把凭据。
        let (d, body_d) = gated
            .call_with_bearer_and_cookie(&base, "never-issued-token", &cookie, "/api/providers")
            .await;
        assert_eq!(
            d, 401,
            "D：无效 Bearer + 有效 cookie 必须 401，不许回落到 cookie 过关（通道：{}）。\
             实际 {d}，body {body_d}",
            gated.label
        );

        // ── E：撤销后 cookie 失效 ──
        // 撤销走的是与 `e2e_pair_then_call_then_revoke` 第 8 步同一条路：
        // `POST /pairing/revoke` 只接 loopback，而 `base_url` 正是 loopback。
        let revoked = post(
            &base,
            "/pairing/revoke",
            serde_json::json!({ "clientId": client_id }),
        )
        .await
        .status()
        .as_u16();
        assert_eq!(revoked, 200, "E：撤销请求必须受理，实际 {revoked}");

        let (e, body_e) = gated
            .call_with_cookie(&base, &cookie, "/api/providers")
            .await;
        assert_eq!(
            e, 401,
            "E：撤销后同一条 cookie 必须**立即** 401（不重启进程，通道：{}）。\
             只摘 Bearer 的话，远端浏览器里的会话会一直活着。实际 {e}，body {body_e}",
            gated.label
        );

        // E 的反证：撤销摘的是那一条记录，所以同一记录上的 Bearer 也一起没了 ——
        // 这一条同时说明 E 的 401 不是「cookie 通道本来就坏」。
        let (e_bearer, _) = gated
            .call_with_bearer(&base, &client_token, "/api/providers")
            .await;
        assert_eq!(
            e_bearer, 401,
            "E：撤销后同一条记录上的 Bearer 也必须 401 —— 两个摘要在同一条记录上，\
             撤销摘掉的就是它。实际 {e_bearer}"
        );

        teardown(h).await;
    }

    // =======================================================================
    // CSRF 交叉格：写路由 × 真会话 cookie × 外站 Origin
    // =======================================================================
    //
    // cookie 一旦能过 `Protected`，CSRF 就成了必须回答的问题。答案是两层：
    //
    // 1. cookie 是 `SameSite=Strict`，跨站请求浏览器根本不会带上它；
    // 2. `policy::decide` 里 **Origin 否决在最前**，凭据再对也翻不了案。
    //
    // 第 2 条此前只有 `cors_origin_denial_precedes_credential_gate` 用 **Bearer**
    // 凭据在 `/api/health` 上钉着，而 `csrf_cross_site_post_on_all_write_routes_is_403`
    // 走的是**无凭据**路径。两者之间缺的正是本用例这一格：**写路由 × 真 cookie ×
    // 外站 Origin**。少了它，把 Origin 否决挪到凭据闸门之后、或把 cookie 通道
    // 单独挂在 Origin 检查之前时，前者手里是 Bearer、后者手里根本没有 cookie，
    // 六条写路由上的跨站 cookie 请求会带着 200 悄悄通过而全仓全绿。
    //
    // 形态用**真实跨站 POST**（表单 `Content-Type`，CORS 简单请求，不过预检）：
    // 预检是浏览器侧的事，用带 JSON `Content-Type` 的普通 POST 去测，浏览器先被
    // 预检挡掉，服务端这道闸门存不存在根本测不到。
    //
    // 每条写路由都配一条**反向腿**：同一份 cookie、只把 Origin 换成白名单内的
    // 那一个，断言「不是 403」。没有这条腿，403 也可能来自「这条路由对谁都不通」。
    // 反向腿刻意不钉死具体码 —— 那停在 handler 层 extractor 的 415 上，属于
    // `handlers::tests` 的契约，两边各管一段（与
    // `csrf_cross_site_post_on_all_write_routes_is_403` 同一口径）。
    #[serial_test::serial]
    #[tokio::test]
    async fn cross_site_origin_with_valid_session_cookie_on_write_routes_is_403() {
        let h = start_headless(RecordingSink::new()).await;
        let base = h.base_url.clone();
        let src = loopback();
        let cookie = paired_session_cookie(&h).await;
        let cookie_header = format!("cc_switch_session={cookie}");
        let form_body = "providerId=evil.example&appType=claude&enabled=true";

        // ── 反证：同一份 cookie、不带 Origin，写路由必须**不是** 403 ──
        // 这一腿立起「cookie 真的有效」。缺了它，下面每一条 403 都可能只是
        // 「这发请求压根没通过凭据层」，而 Origin 否决到底有没有生效无从判断。
        for path in WRITE_ROUTES {
            let (status, body) = post_raw_of(
                &base,
                src,
                path,
                &[("Cookie", cookie_header.as_str())],
                form_body,
            )
            .await;
            assert_ne!(
                status, 403,
                "反证：{path} 带有效会话 cookie 且**不带 Origin** 时不得被 Origin 否决挡下，\
                 否则下面那条 403 验的不是 Origin。实际 {status}，body {body}"
            );
        }

        for path in WRITE_ROUTES {
            let (status, body) = post_raw_of(
                &base,
                src,
                path,
                &[
                    ("Origin", ORIGIN_FOREIGN),
                    ("Cookie", cookie_header.as_str()),
                ],
                form_body,
            )
            .await;
            assert_eq!(
                status, 403,
                "{path}：外站 Origin（{ORIGIN_FOREIGN}）+ **有效**会话 cookie 的跨站 POST \
                 必须 403 —— cookie 能过 Protected 之后，Origin 否决是拦跨站的第二层，\
                 而且它在判定表最前，凭据再对也翻不了案。实际 {status}，body {body}"
            );
            assert!(
                !body
                    .to_ascii_lowercase()
                    .contains("access-control-allow-origin"),
                "{path}：外站 Origin 的响应不得下发 ACAO，否则跨站页面读得到响应内容：{body}"
            );

            // ── 反向腿：白名单 Origin + 同一份 cookie 不得被 403 ──
            let (whitelisted, body_w) = post_raw_of(
                &base,
                src,
                path,
                &[
                    ("Origin", ORIGIN_WHITELISTED),
                    ("Cookie", cookie_header.as_str()),
                ],
                form_body,
            )
            .await;
            assert_ne!(
                whitelisted, 403,
                "{path}：白名单 Origin（{ORIGIN_WHITELISTED}）+ 同一份会话 cookie 不得被 \
                 Origin 否决挡下 —— 这道闸门挡的是跨站，不是内嵌 Web UI 自己。\
                 实际 {whitelisted}，body {body_w}"
            );
        }

        teardown(h).await;
    }
}

// ===========================================================================
// 绑定范围三档位：真实 socket、真实绑定、真实连接
// ===========================================================================
//
// 这一族不用 mock 替换绑定行为 —— 本功能的价值全在「真的绑了」。每条用例都
// 从生产路径起服务（`RemoteConfig` → `compute_target_addrs` → `start`），再用
// 真实 TCP / HTTP 从回环与非回环来源各探一次：
//
// - 「连不上」= 连接层失败（RST / timeout），不是 401；
// - 「连上了但没凭据」= 401（非回环来源仍然必须持有配对凭证）。
//
// 非回环来源由 [`nonloopback_source`] 探测，目的地址用同一块地址：本机连本机
// 的 LAN IP 不会离开机器，也不依赖对外网络。

fn loopback_only(port: u16) -> RemoteConfig {
    RemoteConfig {
        enabled: true,
        port,
        bind_scope: BindScope::Loopback,
        ..RemoteConfig::default()
    }
}

fn with_scope(port: u16, bind_scope: BindScope) -> RemoteConfig {
    RemoteConfig {
        enabled: true,
        port,
        bind_scope,
        ..RemoteConfig::default()
    }
}

async fn request_widening(h: &Harness, token: &str, target: &str) -> String {
    let response = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/request",
        token,
        serde_json::json!({ "scope": target }),
    )
    .await;
    assert_eq!(response.status(), 200, "widening request must be accepted");
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["applied"], serde_json::json!(false));
    assert_eq!(
        body["bindScope"],
        serde_json::json!("loopback"),
        "待确认期间生效值必须还是旧档位：{body}"
    );
    body["confirmationId"]
        .as_str()
        .expect("widening must return a confirmation id")
        .to_string()
}

/// 第 1 条：`Loopback` 档位下，LAN 目的地址根本没有人监听。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_loopback_is_not_reachable_on_lan() {
    let src = match nonloopback_source().await {
        Ok(src) => src,
        Err(why) => {
            eprintln!("SKIP e2e_bind_scope_loopback_is_not_reachable_on_lan: {why}");
            return;
        }
    };

    let port = free_port();
    let h = start_headless_configured(loopback_only(port), None, RecordingSink::new()).await;

    // 回环仍然可用。
    let status = reqwest::get(format!("{}/api/health", h.base_url))
        .await
        .expect("loopback request")
        .status()
        .as_u16();
    assert_eq!(status, 200, "Loopback 档位下 127.0.0.1 必须可用");

    // LAN 目的地址：连接层必须失败。
    assert_connection_refused(src.ip, SocketAddr::new(src.ip, port)).await;

    teardown(h).await;
}

/// 第 2 条：`AllInterfaces` 档位下，非回环目的地址可达，但**仍然要凭据**。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_all_interfaces_reaches_lan_but_requires_credential() {
    let src = match nonloopback_source().await {
        Ok(src) => src,
        Err(why) => {
            eprintln!(
                "SKIP e2e_bind_scope_all_interfaces_reaches_lan_but_requires_credential: {why}"
            );
            return;
        }
    };

    let port = free_port();
    let h = start_headless_configured(
        with_scope(port, BindScope::AllInterfaces),
        None,
        RecordingSink::new(),
    )
    .await;
    let token = issue_tokens(&h.cfg_dir, &["lan-client"]).remove(0);
    let destination = SocketAddr::new(src.ip, port);

    // 真实 TCP：0.0.0.0 绑定让这块网卡上的端口真的开了。
    tcp_connect_from_retrying(Some(src.ip), destination).await;

    // 但可达 ≠ 可用：非回环来源没有凭据必须 401，不是 200。
    let unauthenticated =
        get_status_from_source_retrying(src.ip, destination, "/api/health", None).await;
    assert_eq!(
        unauthenticated, 401,
        "非回环来源未带凭据必须是 401，而不是 {unauthenticated}"
    );

    let authenticated =
        get_status_from_source_retrying(src.ip, destination, "/api/health", Some(&token)).await;
    assert_eq!(authenticated, 200, "带配对凭证的非回环请求必须放行");

    teardown(h).await;
}

/// 第 3 条：`Tailscale` 档位同时绑 Tailscale IP 与回环。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_tailscale_binds_tailnet_and_loopback() {
    let port = free_port();
    let h = start_headless_configured(
        with_scope(port, BindScope::Tailscale),
        None,
        RecordingSink::new(),
    )
    .await;

    let loopback_status = reqwest::get(format!("{}/api/health", h.base_url))
        .await
        .expect("loopback request")
        .status()
        .as_u16();
    assert_eq!(
        loopback_status, 200,
        "Tailscale 档位下 127.0.0.1 必须仍然可用"
    );

    match super::get_tailscale_ip() {
        Some(ip) => {
            let destination: SocketAddr = format!("{ip}:{port}")
                .parse()
                .expect("tailscale ip must form a socket address");
            assert!(
                h.server
                    .state
                    .listeners
                    .read()
                    .await
                    .keys()
                    .any(|addr| addr.ip().to_string() == ip),
                "Tailscale 档位必须真的绑上 {ip}，实际监听表：{:?}",
                h.server
                    .state
                    .listeners
                    .read()
                    .await
                    .keys()
                    .collect::<Vec<_>>()
            );
            tcp_connect_from(None, destination)
                .await
                .unwrap_or_else(|e| panic!("Tailscale listener {destination} 必须可达：{e}"));

            // 本机连 tailnet 地址时，服务端看到的 peer 是 100.x（非回环），
            // 所以没有凭据必须 401。
            let status = reqwest::Client::new()
                .get(format!("http://{destination}/api/health"))
                .send()
                .await
                .expect("tailnet request")
                .status()
                .as_u16();
            assert_eq!(status, 401, "tailnet 来源未带凭据必须是 401");

            let token = issue_tokens(&h.cfg_dir, &["tailnet-client"]).remove(0);
            let status = reqwest::Client::new()
                .get(format!("http://{destination}/api/health"))
                .bearer_auth(&token)
                .send()
                .await
                .expect("authenticated tailnet request")
                .status()
                .as_u16();
            assert_eq!(status, 200, "带凭据的 tailnet 请求必须放行");
        }
        None => {
            // 规格 B 的兜底路径：拿不到 Tailscale IP 时只绑回环并记日志。
            eprintln!(
                "NOTE e2e_bind_scope_tailscale_binds_tailnet_and_loopback: \
                 no Tailscale IP on this machine; verified the documented fallback only"
            );
            let listeners = h.server.state.listeners.read().await;
            assert_eq!(
                listeners.len(),
                1,
                "拿不到 Tailscale IP 时必须退回只有回环一个 listener"
            );
            assert!(
                listeners.keys().all(|addr| addr.ip().is_loopback()),
                "兜底监听表里不得出现非回环地址"
            );
        }
    }

    teardown(h).await;
}

/// 第 4 条：放宽必须两段式；确认之后监听地址真的变了。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_widening_requires_confirmation_and_rebinds() {
    let src = match nonloopback_source().await {
        Ok(src) => src,
        Err(why) => {
            eprintln!("SKIP e2e_bind_scope_widening_requires_confirmation_and_rebinds: {why}");
            return;
        }
    };

    let port = free_port();
    let h = start_headless_configured(loopback_only(port), None, RecordingSink::new()).await;
    let tokens = issue_tokens(&h.cfg_dir, &["alice", "bob"]);
    let (alice, bob) = (&tokens[0], &tokens[1]);
    let destination = SocketAddr::new(src.ip, port);

    // 基线：LAN 目的地址不可达。
    assert_connection_refused(src.ip, destination).await;

    // 第一段：申请不生效、不落盘、不改监听。
    let confirmation_id = request_widening(&h, alice, "allInterfaces").await;
    assert_eq!(
        h.server.config_snapshot().await.bind_scope,
        BindScope::Loopback,
        "申请不得改变生效档位"
    );
    assert_connection_refused(src.ip, destination).await;
    assert!(!h.cfg_dir.join("exposure.json").exists(), "申请不得落盘");

    // 跨会话确认必须被拒（且不消费申请）。
    let response = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/confirm",
        bob,
        serde_json::json!({ "confirmationId": confirmation_id }),
    )
    .await;
    assert_eq!(response.status(), 400, "B 会话不得确认 A 的申请");

    // 第二段：同一会话确认之后才生效。
    let response = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/confirm",
        alice,
        serde_json::json!({ "confirmationId": confirmation_id }),
    )
    .await;
    assert_eq!(response.status(), 200, "同一会话确认必须成功");
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["applied"], serde_json::json!(true));
    assert_eq!(body["bindScope"], serde_json::json!("allInterfaces"));

    // 配置变了，**监听地址也真的变了**。
    assert_eq!(
        h.server.config_snapshot().await.bind_scope,
        BindScope::AllInterfaces
    );
    tcp_connect_from_retrying(Some(src.ip), destination).await;
    assert_eq!(
        get_status_from_source_retrying(src.ip, destination, "/api/health", None).await,
        401,
        "可达之后仍然要求配对凭证"
    );
    assert_eq!(
        get_status_from_source_retrying(src.ip, destination, "/api/health", Some(alice)).await,
        200
    );

    // 落盘的是新档位。
    let persisted: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(h.cfg_dir.join("exposure.json")).expect("exposure.json"),
    )
    .expect("exposure.json must be JSON");
    assert_eq!(persisted["bindScope"], serde_json::json!("allInterfaces"));

    teardown(h).await;
}

/// 第 5 条：收紧不需要确认，立即生效且监听地址真的变窄。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_narrowing_applies_immediately_and_rebinds() {
    let src = match nonloopback_source().await {
        Ok(src) => src,
        Err(why) => {
            eprintln!("SKIP e2e_bind_scope_narrowing_applies_immediately_and_rebinds: {why}");
            return;
        }
    };

    let port = free_port();
    let h = start_headless_configured(
        with_scope(port, BindScope::AllInterfaces),
        None,
        RecordingSink::new(),
    )
    .await;
    let alice = issue_tokens(&h.cfg_dir, &["alice"]).remove(0);
    let destination = SocketAddr::new(src.ip, port);

    // 起点：LAN 可达。
    tcp_connect_from_retrying(Some(src.ip), destination).await;

    // 收紧：一次 request 就生效，没有 confirmation id。
    let response = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/request",
        &alice,
        serde_json::json!({ "scope": "loopback" }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["applied"], serde_json::json!(true), "收紧必须立即生效");
    assert!(
        body.get("confirmationId").is_none() || body["confirmationId"].is_null(),
        "收紧不应返回 confirmation id：{body}"
    );

    assert_eq!(
        h.server.config_snapshot().await.bind_scope,
        BindScope::Loopback
    );
    assert_connection_refused(src.ip, destination).await;
    assert_eq!(
        reqwest::get(format!("{}/api/health", h.base_url))
            .await
            .expect("loopback request")
            .status()
            .as_u16(),
        200,
        "收紧不得弄坏回环"
    );

    teardown(h).await;
}

/// 第 6 条：同一个 confirmationId 确认两次，第二次必须拒绝。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_confirmation_id_is_single_use() {
    let port = free_port();
    let h = start_headless_configured(loopback_only(port), None, RecordingSink::new()).await;
    let alice = issue_tokens(&h.cfg_dir, &["alice"]).remove(0);
    let id = request_widening(&h, &alice, "tailscale").await;

    let first = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/confirm",
        &alice,
        serde_json::json!({ "confirmationId": id }),
    )
    .await;
    assert_eq!(first.status(), 200, "第一次确认必须成功");

    let second = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/confirm",
        &alice,
        serde_json::json!({ "confirmationId": id }),
    )
    .await;
    assert_eq!(second.status(), 400, "同一个 id 第二次确认必须被拒");

    teardown(h).await;
}

/// 第 7 条：过期后确认被拒。TTL 由测试注入（150ms），不真的 sleep 五分钟。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_expired_confirmation_is_rejected() {
    let port = free_port();
    let h = start_headless_configured(
        loopback_only(port),
        Some(Duration::from_millis(150)),
        RecordingSink::new(),
    )
    .await;
    let alice = issue_tokens(&h.cfg_dir, &["alice"]).remove(0);
    let id = request_widening(&h, &alice, "tailscale").await;

    tokio::time::sleep(Duration::from_millis(400)).await;

    let response = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/confirm",
        &alice,
        serde_json::json!({ "confirmationId": id }),
    )
    .await;
    assert_eq!(response.status(), 400, "过期后确认必须被拒");
    assert_eq!(
        h.server.config_snapshot().await.bind_scope,
        BindScope::Loopback,
        "过期申请不得改配置"
    );

    teardown(h).await;
}

/// 第 8 条：A 会话申请、B 会话确认 → 拒绝；A 自己仍能确认。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_cross_session_confirmation_is_rejected() {
    let port = free_port();
    let h = start_headless_configured(loopback_only(port), None, RecordingSink::new()).await;
    let tokens = issue_tokens(&h.cfg_dir, &["alice", "bob"]);
    let (alice, bob) = (&tokens[0], &tokens[1]);
    let id = request_widening(&h, alice, "tailscale").await;

    let by_bob = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/confirm",
        bob,
        serde_json::json!({ "confirmationId": id }),
    )
    .await;
    assert_eq!(by_bob.status(), 400, "B 会话确认必须被拒");
    assert_eq!(
        h.server.config_snapshot().await.bind_scope,
        BindScope::Loopback
    );

    // B 的失败不得把 A 的申请消费掉。
    let by_alice = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/confirm",
        alice,
        serde_json::json!({ "confirmationId": id }),
    )
    .await;
    assert_eq!(by_alice.status(), 200, "A 自己仍然可以确认");
    assert_eq!(
        h.server.config_snapshot().await.bind_scope,
        BindScope::Tailscale
    );

    teardown(h).await;
}

/// 第 9 条：落盘后重新构造 server，档位仍是刚才设的（启动路径 C.2）。
#[serial_test::serial]
#[tokio::test]
async fn e2e_bind_scope_survives_restart() {
    let dir = tempfile::tempdir().expect("config dir owned by the test");
    let port = free_port();
    let h = start_headless_configured_in(
        dir.path().to_path_buf(),
        None,
        loopback_only(port),
        None,
        RecordingSink::new(),
    )
    .await;
    let alice = issue_tokens(&h.cfg_dir, &["alice"]).remove(0);
    let id = request_widening(&h, &alice, "allInterfaces").await;
    let response = post_json_with_token(
        &h.base_url,
        "/api/remote/config/bind-scope/confirm",
        &alice,
        serde_json::json!({ "confirmationId": id }),
    )
    .await;
    assert_eq!(response.status(), 200);
    teardown(h).await;

    // 生产启动路径：`lib.rs` 调的就是这个函数。
    let startup = startup_remote_config(dir.path(), true, port, false);
    assert_eq!(
        startup.bind_scope,
        BindScope::AllInterfaces,
        "启动时必须从 exposure.json 读回档位，而不是回默认值"
    );

    // 重新构造 server：生效配置里仍是刚才的档位。
    let restarted = RemoteServer::new(
        RemoteApp::headless(),
        Arc::new(RecordingSink::new()),
        startup,
        dir.path().to_path_buf(),
    )
    .expect("restart with the same config dir");
    assert_eq!(
        restarted.config_snapshot().await.bind_scope,
        BindScope::AllInterfaces
    );
}

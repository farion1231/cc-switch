//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

use hyper_util::client::proxy::matcher::Matcher;
use once_cell::sync::OnceCell;
use reqwest::Client;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::Duration;

/// 全局 HTTP 客户端实例
static GLOBAL_CLIENT: OnceCell<RwLock<Client>> = OnceCell::new();

/// 当前代理 URL（用于日志和状态查询）
static CURRENT_PROXY_URL: OnceCell<RwLock<Option<String>>> = OnceCell::new();

/// CC Switch 代理服务器当前监听的端口
static CC_SWITCH_PROXY_PORT: OnceCell<RwLock<u16>> = OnceCell::new();

/// 未配置显式代理时是否跟随 env / 系统代理；false 则 `no_proxy()` 直连。默认 true 保持历史行为。
static FOLLOW_SYSTEM_PROXY: AtomicBool = AtomicBool::new(true);

/// 探测「对外部 API 请求实际生效的代理」用的目标地址。
/// 系统代理是否命中取决于目标主机（NO_PROXY 等绕过规则按主机名匹配），必须带上目标才问得出答案。
/// 取一个代表性的上游主机即可：状态只有一行，逐个上游分别展示反而读不出结论。
const HTTPS_PROBE_TARGET: &str = "https://api.anthropic.com/";
/// hyper-util 按目标 scheme 选 http / https 槽，只探 https 看不见 HTTP-only 代理的变化，
/// 而走 http 供应商的转发用的正是那一槽，所以变化比较时两槽各探一次。
const HTTP_PROBE_TARGET: &str = "http://api.anthropic.com/";

/// 当前全局客户端构建时烘焙进去的系统代理。reqwest 构建后不再重读系统配置，
/// 所以它可能落后于此刻的解析结果，两者不一致即「系统代理已变化」，见 `system_proxy_status()`。
static BAKED_SYSTEM_PROXY: RwLock<Option<BakedSystemProxy>> = RwLock::new(None);

/// 来源与快照必须一起记：事后再问一次来源问到的是此刻的系统状态，
/// 系统代理若已改动，就会给一份旧快照配上新来源的标签。
struct BakedSystemProxy {
    snapshot: SystemProxySnapshot,
    /// 展示槽的来源；展示槽直连时没有来源可言
    source: Option<SystemProxySource>,
}

/// 构建客户端时会烘焙进去的系统代理，http 与 https 两槽分别解析。
#[derive(PartialEq, Eq)]
struct SystemProxySnapshot {
    https: Option<ResolvedSystemProxy>,
    http: Option<ResolvedSystemProxy>,
}

impl SystemProxySnapshot {
    fn direct() -> Self {
        Self {
            https: None,
            http: None,
        }
    }

    /// 展示只取 https 槽：供应商几乎都是 https，拿 http 槽顶上会把「https 直连」报成走代理，
    /// 可达性探测也会探到一个对 https 请求不生效的端口。http 槽只参与变化比较。
    fn display(&self) -> Option<&ResolvedSystemProxy> {
        self.https.as_ref()
    }
}

/// 探测目标命中的代理：展示用的去凭据地址，加上只用于比较的凭据指纹。
#[derive(PartialEq, Eq)]
struct ResolvedSystemProxy {
    url: String,
    auth_fingerprint: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemProxySource {
    Env,
    System,
}

pub fn follow_system_proxy() -> bool {
    FOLLOW_SYSTEM_PROXY.load(Ordering::Relaxed)
}

/// 用给定 matcher 解析探测目标会命中的代理，None 表示该目标直连。
fn resolve_probe_proxy(matcher: &Matcher, target: &str) -> Option<ResolvedSystemProxy> {
    let target: hyper::Uri = target.parse().ok()?;
    let intercept = matcher.intercept(&target)?;
    let uri = intercept.uri();
    // 手工拼 scheme://authority 而不用 Uri 的 Display：后者会带上 hyper-util 强制补的 "/" 路径，
    // 与用户在设置里填的代理地址形态对不上。认证信息已被 hyper-util 从该 uri 里剥离。
    let url = format!(
        "{}://{}",
        uri.scheme_str().unwrap_or("http"),
        uri.authority().map(|a| a.as_str()).unwrap_or_default()
    );
    // 只换用户名或密码时地址不变，没有凭据参与比较就察觉不到客户端已经拿着旧凭据；
    // 存不可逆的哈希而不是凭据本身，比较够用，也不会把凭据带进状态展示或日志。
    // http(s) 代理的凭据在 Basic 头里，SOCKS 代理的凭据是原始用户名/密码，两种都要盖进去
    let auth_fingerprint = intercept
        .basic_auth()
        .map(|value| credential_fingerprint(value.as_bytes()))
        .or_else(|| {
            intercept
                .raw_auth()
                .map(|(user, pass)| credential_fingerprint(format!("{user}\0{pass}").as_bytes()))
        });
    Some(ResolvedSystemProxy {
        url,
        auth_fingerprint,
    })
}

fn credential_fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// 用给定 matcher 把两槽各解析一次。
fn snapshot_from(matcher: &Matcher) -> SystemProxySnapshot {
    SystemProxySnapshot {
        https: resolve_probe_proxy(matcher, HTTPS_PROBE_TARGET),
        http: resolve_probe_proxy(matcher, HTTP_PROBE_TARGET),
    }
}

/// 此刻重建客户端、且走跟随分支时会烘焙进去的快照。
/// 走 hyper-util 的 `Matcher::from_system()` 解析——reqwest 自动跟随时用的正是同一套解析。
///
/// 与 `build_client` 的跟随分支同一套取舍：系统代理指向本应用自己的代理端口时必须直连，
/// 否则请求绕回自身。拿原始检测值比会把这个恒定的取舍误报成「系统代理已变化」。
fn current_snapshot() -> SystemProxySnapshot {
    if system_proxy_points_to_loopback() {
        return SystemProxySnapshot::direct();
    }
    snapshot_from(&Matcher::from_system())
}

/// 状态查询用：此刻重建会烘焙进去的代理（去凭据地址，None 表示直连）与烘焙快照是否已变化，
/// 两者出自同一次解析，展示出来的「当前值」不会和变化判定对不上。
/// 变化按 http / https 两槽的（地址，凭据指纹）比较，展示仍只用 https 槽的去凭据地址。
pub fn system_proxy_status() -> (Option<String>, bool) {
    let current = current_snapshot();
    let changed = BAKED_SYSTEM_PROXY
        .read()
        .ok()
        .and_then(|slot| slot.as_ref().map(|baked| baked.snapshot != current))
        .unwrap_or(false);
    (current.display().map(|proxy| proxy.url.clone()), changed)
}

/// 探测目标按 env+系统解析出的代理 `resolved` 是环境变量给的，还是操作系统设置给的。
///
/// 不照抄 hyper-util 的槽位优先级去自己判：它按 http / https 分槽，各槽只在 env 留空时才回填
/// 系统设置（CGI 环境还会整个短路成空），逐个变量看非空必然误判。改为拿同一个探测目标仅按
/// env 再解析一遍，与 `resolved` 一致才说明这个值确实出自环境变量。
fn proxy_source(env: &Matcher, target: &str, resolved: &ResolvedSystemProxy) -> SystemProxySource {
    if resolve_probe_proxy(env, target).as_ref() == Some(resolved) {
        SystemProxySource::Env
    } else {
        SystemProxySource::System
    }
}

/// 当前客户端烘焙的系统代理（https 槽的去凭据地址）与烘焙那一刻记下的来源。
/// 地址和来源只能一起取：分两次读锁，中间夹一次重建就会配出互不相干的一对。
pub fn baked_system_proxy() -> Option<(String, SystemProxySource)> {
    BAKED_SYSTEM_PROXY.read().ok().and_then(|slot| {
        slot.as_ref().and_then(|baked| {
            let proxy = baked.snapshot.display()?;
            Some((proxy.url.clone(), baked.source?))
        })
    })
}

/// 这次重建会烘焙什么；只有真正走跟随分支才有值，其余分支（显式代理 / 关闭跟随）
/// 客户端不吃系统代理。必须在构建客户端之前调用，见 `build_client_with_snapshot`。
fn plan_baked_system_proxy(
    effective_url: Option<&str>,
    follow_system_proxy: bool,
) -> Option<BakedSystemProxy> {
    if effective_url.is_some() || !follow_system_proxy {
        return None;
    }
    let snapshot = current_snapshot();
    // 来源只按 env 再解析一遍，不再读第二次系统配置：分两次读，中间一变就会给旧快照配上新来源的标签
    let source = snapshot
        .display()
        .map(|proxy| proxy_source(&Matcher::from_env(), HTTPS_PROBE_TARGET, proxy));
    Some(BakedSystemProxy { snapshot, source })
}

/// 先读快照再建客户端，合成一步让调用方拿不到反过来的顺序：reqwest 在 build 时读系统配置，
/// 快照若晚于 build 才读，中间配置一变就会「客户端拿着旧值、记录写着新值」，之后新值对新值
/// 永远报不出变化；先读快照最坏只是多报一次「已变化」，重新应用即可自愈。
fn build_client_with_snapshot(
    effective_url: Option<&str>,
    follow_system_proxy: bool,
) -> Result<(Client, Option<BakedSystemProxy>), String> {
    let planned = plan_baked_system_proxy(effective_url, follow_system_proxy);
    let client = build_client(effective_url, follow_system_proxy)?;
    Ok((client, planned))
}

/// 把这次重建对应的快照记下来，与 `build_client_with_snapshot` 成对使用。
fn store_baked_system_proxy(baked: Option<BakedSystemProxy>) {
    if let Ok(mut slot) = BAKED_SYSTEM_PROXY.write() {
        *slot = baked;
    }
}

/// 更新开关；客户端已初始化时按当前显式代理重建，使之立即生效。
/// 启动阶段应先于 `init` 调用，此时只记录标志。
pub fn set_follow_system_proxy(follow: bool) -> Result<(), String> {
    FOLLOW_SYSTEM_PROXY.store(follow, Ordering::Relaxed);
    if GLOBAL_CLIENT.get().is_none() {
        return Ok(());
    }
    apply_proxy(get_current_proxy_url().as_deref())
}

/// 设置 CC Switch 代理服务器的监听端口
///
/// 应在代理服务器启动时调用，以便系统代理检测能正确识别自己的端口
pub fn set_proxy_port(port: u16) {
    if let Some(lock) = CC_SWITCH_PROXY_PORT.get() {
        if let Ok(mut current_port) = lock.write() {
            *current_port = port;
            log::debug!("[GlobalProxy] Updated CC Switch proxy port to {port}");
        }
    } else {
        let _ = CC_SWITCH_PROXY_PORT.set(RwLock::new(port));
        log::debug!("[GlobalProxy] Initialized CC Switch proxy port to {port}");
    }
}

/// 获取 CC Switch 代理服务器的监听端口
fn get_proxy_port() -> u16 {
    CC_SWITCH_PROXY_PORT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|port| *port)
        .unwrap_or(15721) // 默认端口作为回退
}

/// 初始化全局 HTTP 客户端
///
/// 应在应用启动时调用一次。
///
/// # Arguments
/// * `proxy_url` - 代理 URL，如 `http://127.0.0.1:7890` 或 `socks5://127.0.0.1:1080`
///   传入 None 或空字符串表示直连
pub fn init(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let (client, planned) = build_client_with_snapshot(effective_url, follow_system_proxy())?;

    // 尝试初始化全局客户端，如果已存在则记录警告并使用 apply_proxy 更新
    if GLOBAL_CLIENT.set(RwLock::new(client.clone())).is_err() {
        log::warn!(
            "[GlobalProxy] [GP-003] Already initialized, updating instead: {}",
            effective_url
                .map(mask_url)
                .unwrap_or_else(|| "direct connection".to_string())
        );
        // 已初始化，改用 apply_proxy 更新
        return apply_proxy(proxy_url);
    }

    // 初始化代理 URL 记录
    let _ = CURRENT_PROXY_URL.set(RwLock::new(effective_url.map(|s| s.to_string())));

    // 启动早期、命令尚不可调用，没有并发重建，快照不必与上面的发布同锁
    store_baked_system_proxy(planned);

    log::info!(
        "[GlobalProxy] Initialized: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// 验证代理配置（不应用）
///
/// 只验证代理 URL 是否有效，不实际更新全局客户端。
/// 用于在持久化之前验证配置的有效性。
///
/// # Arguments
/// * `proxy_url` - 代理 URL，None 或空字符串表示直连
///
/// # Returns
/// 验证成功返回 Ok(())，失败返回错误信息
pub fn validate_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    // 只调用 build_client 来验证，但不应用
    build_client(effective_url, follow_system_proxy())?;
    Ok(())
}

/// 应用代理配置（假设已验证）
///
/// 直接应用代理配置到全局客户端，不做额外验证。
/// 应在 validate_proxy 成功后调用。
///
/// # Arguments
/// * `proxy_url` - 代理 URL，None 或空字符串表示直连
pub fn apply_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let (new_client, planned) = build_client_with_snapshot(effective_url, follow_system_proxy())?;

    // 更新客户端
    if let Some(lock) = GLOBAL_CLIENT.get() {
        let mut client = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        *client = new_client;
        // 客户端与快照在同一把写锁内提交：分开提交时并发重建会交错出「客户端是 A、记录是 B」
        store_baked_system_proxy(planned);
    } else {
        // 如果还没初始化，则初始化
        return init(proxy_url);
    }

    // 更新代理 URL 记录
    if let Some(lock) = CURRENT_PROXY_URL.get() {
        let mut url = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-002] Failed to acquire URL write lock: {e}");
            "Failed to update proxy URL record: lock poisoned".to_string()
        })?;
        *url = effective_url.map(|s| s.to_string());
    }

    log::info!(
        "[GlobalProxy] Applied: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// 更新代理配置（热更新）
///
/// 可在运行时调用以更改代理设置，无需重启应用。
/// 注意：此函数同时验证和应用，如果需要先验证后持久化再应用，
/// 请使用 validate_proxy + apply_proxy 组合。
///
/// # Arguments
/// * `proxy_url` - 新的代理 URL，None 或空字符串表示直连
#[allow(dead_code)]
pub fn update_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let (new_client, planned) = build_client_with_snapshot(effective_url, follow_system_proxy())?;

    // 更新客户端
    if let Some(lock) = GLOBAL_CLIENT.get() {
        let mut client = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        *client = new_client;
        // 客户端与快照在同一把写锁内提交：分开提交时并发重建会交错出「客户端是 A、记录是 B」
        store_baked_system_proxy(planned);
    } else {
        // 如果还没初始化，则初始化
        return init(proxy_url);
    }

    // 更新代理 URL 记录
    if let Some(lock) = CURRENT_PROXY_URL.get() {
        let mut url = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-002] Failed to acquire URL write lock: {e}");
            "Failed to update proxy URL record: lock poisoned".to_string()
        })?;
        *url = effective_url.map(|s| s.to_string());
    }

    log::info!(
        "[GlobalProxy] Updated: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// 获取全局 HTTP 客户端
///
/// 返回配置了代理的客户端（如果已配置代理），否则按跟随开关返回跟随系统代理或直连的客户端。
pub fn get() -> Client {
    GLOBAL_CLIENT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|c| c.clone())
        .unwrap_or_else(|| {
            log::warn!("[GlobalProxy] [GP-004] Client not initialized, using fallback");
            build_client(None, follow_system_proxy()).unwrap_or_default()
        })
}

/// 获取当前代理 URL
///
/// 返回当前配置的代理 URL，None 表示直连。
pub fn get_current_proxy_url() -> Option<String> {
    CURRENT_PROXY_URL
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|url| url.clone())
}

/// 检查是否正在使用代理
#[allow(dead_code)]
pub fn is_proxy_enabled() -> bool {
    get_current_proxy_url().is_some()
}

/// 构建 HTTP 客户端
fn build_client(proxy_url: Option<&str>, follow_system_proxy: bool) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60))
        // 禁用 reqwest 自动解压：防止 reqwest 覆盖客户端原始 accept-encoding header。
        // 响应解压由 response_processor 根据 content-encoding 手动处理。
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd();

    if let Some(url) = proxy_url {
        // 先验证 URL 格式和 scheme
        let parsed = url::Url::parse(url)
            .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(url), e))?;

        let scheme = parsed.scheme();
        if !["http", "https", "socks5", "socks5h"].contains(&scheme) {
            return Err(format!(
                "Invalid proxy scheme '{}' in URL '{}'. Supported: http, https, socks5, socks5h",
                scheme,
                mask_url(url)
            ));
        }

        let proxy = reqwest::Proxy::all(url)
            .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(url), e))?;
        builder = builder.proxy(proxy);
        log::debug!("[GlobalProxy] Proxy configured: {}", mask_url(url));
    } else if !follow_system_proxy {
        // 用户关闭跟随
        builder = builder.no_proxy();
        log::debug!("[GlobalProxy] System proxy following disabled, using direct connection");
    } else if system_proxy_points_to_loopback() {
        // 系统代理指向本机自身端口，跟随会导致请求打回自己
        builder = builder.no_proxy();
        log::warn!("[GlobalProxy] System proxy points to localhost, bypassing to avoid recursion");
    } else {
        log::debug!("[GlobalProxy] Following system proxy (no explicit proxy configured)");
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

/// 环境变量代理是否指向本应用自己的代理端口；跟随分支据此直连，更新器复用同一判断。
pub(crate) fn system_proxy_points_to_loopback() -> bool {
    const KEYS: [&str; 6] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ];

    KEYS.iter()
        .filter_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .any(|value| proxy_points_to_loopback(&value))
}

fn proxy_points_to_loopback(value: &str) -> bool {
    fn host_is_loopback(host: &str) -> bool {
        if host.eq_ignore_ascii_case("localhost") {
            return true;
        }
        host.parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
    }

    // 检查是否指向 CC Switch 自己的代理端口
    // 只有指向自己的代理才需要跳过，避免递归
    fn is_cc_switch_proxy_port(port: Option<u16>) -> bool {
        let cc_switch_port = get_proxy_port();
        port == Some(cc_switch_port)
    }

    if let Ok(parsed) = url::Url::parse(value) {
        if let Some(host) = parsed.host_str() {
            // 只有当主机是 loopback 且端口是 CC Switch 的端口时才返回 true
            return host_is_loopback(host) && is_cc_switch_proxy_port(parsed.port());
        }
        return false;
    }

    let with_scheme = format!("http://{value}");
    if let Ok(parsed) = url::Url::parse(&with_scheme) {
        if let Some(host) = parsed.host_str() {
            return host_is_loopback(host) && is_cc_switch_proxy_port(parsed.port());
        }
    }

    false
}

/// 隐藏 URL 中的敏感信息（用于日志）
pub fn mask_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        // 隐藏用户名和密码，保留 scheme、host 和端口
        let host = parsed.host_str().unwrap_or("?");
        match parsed.port() {
            Some(port) => format!("{}://{}:{}", parsed.scheme(), host, port),
            None => format!("{}://{}", parsed.scheme(), host),
        }
    } else {
        // URL 解析失败，返回部分内容。截断点回退到最近的字符边界，
        // 避免在多字节 UTF-8 字符中间切割导致 panic。
        if url.len() > 20 {
            let cut = (0..=20)
                .rev()
                .find(|&i| url.is_char_boundary(i))
                .unwrap_or(0);
            format!("{}...", &url[..cut])
        } else {
            url.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// 放行被毒化的锁：否则任一改 env 的用例 panic 之后，其余用例会跟着在取锁处 panic，
    /// 真正的首个失败被一堆连带失败淹没。
    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    const PROXY_ENV_KEYS: [&str; 8] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ];

    /// 清空全部代理相关环境变量后按需写入 HTTP(S)_PROXY，返回旧值供恢复。
    /// 调用方必须持有 `env_lock()`。
    ///
    /// 环境变量是进程级的，同进程并行跑的其他用例会在这个窗口里构造自己的 reqwest 客户端。
    /// 它们几乎都请求 127.0.0.1，若被这里的假代理接管就会连带失败，因此一并写入 NO_PROXY
    /// 豁免 v4 回环；本模块的用例改用 IPv6 回环发请求，不受该豁免影响。
    fn set_proxy_env(proxy: Option<&str>) -> Vec<(&'static str, Option<String>)> {
        let previous = PROXY_ENV_KEYS
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect();
        for key in PROXY_ENV_KEYS {
            std::env::remove_var(key);
        }
        if let Some(url) = proxy {
            // 豁免必须先落地：只要有一瞬「死代理已设、回环豁免未设」，同进程并发建客户端的
            // 其他用例就会被路由进死代理
            std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
            std::env::set_var("HTTP_PROXY", url);
            std::env::set_var("HTTPS_PROXY", url);
        }
        previous
    }

    fn restore_proxy_env(previous: Vec<(&'static str, Option<String>)>) {
        for (key, value) in previous {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    /// 绑定后立即释放一个本机端口，得到一个大概率无人监听的「死代理」地址。
    fn dead_loopback_url() -> String {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        format!("http://127.0.0.1:{port}")
    }

    /// 只回 200 的本地 HTTP 服务；后台线程随进程结束。
    /// 绑 IPv6 回环，使其落在 `set_proxy_env` 写的 NO_PROXY 豁免之外。
    fn spawn_ok_server() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("[::1]:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            }
        });
        format!("http://{addr}/")
    }

    /// 接管 `set_proxy_env` 返回的旧值，出作用域即写回。
    /// 用例里任何一个 `.unwrap()` panic 都会跳过手写的恢复调用，把指向死端口的
    /// HTTP_PROXY 连同被覆盖的 NO_PROXY 留给同进程后续用例；Drop 是 panic 路径上也必经的。
    struct ProxyEnvGuard {
        previous: Vec<(&'static str, Option<String>)>,
    }

    impl ProxyEnvGuard {
        fn set(proxy: Option<&str>) -> Self {
            Self {
                previous: set_proxy_env(proxy),
            }
        }
    }

    impl Drop for ProxyEnvGuard {
        fn drop(&mut self) {
            restore_proxy_env(std::mem::take(&mut self.previous));
        }
    }

    /// 接管 `PROXY_ENV_KEYS` 之外的单个环境变量，同样靠 Drop 在 panic 路径上写回。
    struct ScopedEnvVar {
        key: &'static str,
        previous: Option<String>,
    }

    impl ScopedEnvVar {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn baked_url() -> Option<String> {
        baked_system_proxy().map(|(url, _)| url)
    }

    /// 测试里只关心 https 槽：与生产路径共用同一套解析
    fn detect_system_proxy() -> Option<ResolvedSystemProxy> {
        resolve_probe_proxy(&Matcher::from_system(), HTTPS_PROBE_TARGET)
    }

    fn system_proxy_source() -> Option<SystemProxySource> {
        detect_system_proxy()
            .map(|proxy| proxy_source(&Matcher::from_env(), HTTPS_PROBE_TARGET, &proxy))
    }

    fn baked_source() -> Option<SystemProxySource> {
        baked_system_proxy().map(|(_, source)| source)
    }

    /// 同理接管跟随开关，避免 panic 把翻转后的全局标志留给后续用例。
    struct FollowFlagGuard(bool);

    impl FollowFlagGuard {
        fn capture() -> Self {
            Self(follow_system_proxy())
        }
    }

    impl Drop for FollowFlagGuard {
        fn drop(&mut self) {
            // Drop 中不能 panic；重建失败也无妨，标志本身已复位
            let _ = set_follow_system_proxy(self.0);
        }
    }

    #[test]
    fn test_mask_url() {
        assert_eq!(mask_url("http://127.0.0.1:7890"), "http://127.0.0.1:7890");
        assert_eq!(
            mask_url("http://user:pass@127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            mask_url("socks5://admin:secret@proxy.example.com:1080"),
            "socks5://proxy.example.com:1080"
        );
        // 无端口的 URL 不应显示 ":?"
        assert_eq!(
            mask_url("http://proxy.example.com"),
            "http://proxy.example.com"
        );
        assert_eq!(
            mask_url("https://user:pass@proxy.example.com"),
            "https://proxy.example.com"
        );
    }

    #[test]
    fn test_mask_url_keeps_ipv6_brackets() {
        // url 的 host_str() 对 IPv6 字面量自带方括号，脱敏后仍是可解析的地址
        assert_eq!(mask_url("http://[::1]:7890"), "http://[::1]:7890");
        assert_eq!(mask_url("http://user:pass@[::1]:7890"), "http://[::1]:7890");
        assert_eq!(mask_url("socks5://[fe80::1]"), "socks5://[fe80::1]");
    }

    #[test]
    fn test_mask_url_does_not_panic_on_multibyte_boundary() {
        // 一个无法被 Url::parse 解析、且在字节 20 处正好切在多字节字符中间的字符串。
        // 回归 https://github.com/farion1231/cc-switch 的 mask_url 越界 panic。
        let bad = "这是一个无效的代理地址不能解析";
        assert!(bad.len() > 20 && !bad.is_char_boundary(20));
        let masked = mask_url(bad);
        assert!(masked.ends_with("..."));
    }

    #[test]
    fn test_build_client_direct() {
        let result = build_client(None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_http_proxy() {
        let result = build_client(Some("http://127.0.0.1:7890"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_socks5_proxy() {
        let result = build_client(Some("socks5://127.0.0.1:1080"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_invalid_url() {
        // reqwest::Proxy::all 对某些无效 URL 不会立即报错
        // 使用明确无效的 scheme 来触发错误
        let result = build_client(Some("invalid-scheme://127.0.0.1:7890"), true);
        assert!(result.is_err(), "Should reject invalid proxy scheme");
    }

    #[test]
    fn test_proxy_points_to_loopback() {
        // 设置 CC Switch 代理端口为 15721（默认值）
        set_proxy_port(15721);

        // 只有指向 CC Switch 自己端口的 loopback 地址才返回 true
        assert!(proxy_points_to_loopback("http://127.0.0.1:15721"));
        assert!(proxy_points_to_loopback("socks5://localhost:15721"));
        assert!(proxy_points_to_loopback("127.0.0.1:15721"));

        // 其他 loopback 端口不应该被跳过（允许使用其他本地代理工具）
        assert!(!proxy_points_to_loopback("http://127.0.0.1:7890"));
        assert!(!proxy_points_to_loopback("socks5://localhost:1080"));

        // 非 loopback 地址不应该被跳过
        assert!(!proxy_points_to_loopback("http://192.168.1.10:7890"));
        assert!(!proxy_points_to_loopback("http://192.168.1.10:15721"));
    }

    #[test]
    fn test_system_proxy_points_to_loopback() {
        let _guard = lock_env();

        // 设置 CC Switch 代理端口
        set_proxy_port(15721);

        let keys = [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ];

        for key in &keys {
            std::env::remove_var(key);
        }

        // 指向 CC Switch 端口的代理应该被跳过
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:15721");
        assert!(system_proxy_points_to_loopback());

        // 指向其他端口的本地代理不应该被跳过
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7890");
        assert!(!system_proxy_points_to_loopback());

        // 非 loopback 地址不应该被跳过
        std::env::set_var("HTTP_PROXY", "http://10.0.0.2:7890");
        assert!(!system_proxy_points_to_loopback());

        for key in &keys {
            std::env::remove_var(key);
        }
    }

    #[tokio::test]
    async fn build_client_without_following_ignores_env_proxy() {
        let dead_proxy = dead_loopback_url();
        let server = spawn_ok_server();

        // 环境变量只在构建客户端那一刻被读取，构建完即出作用域恢复并释放锁，避免跨 await 持锁
        let (direct, following) = {
            let _lock = lock_env();
            let _env = ProxyEnvGuard::set(Some(&dead_proxy));
            (
                build_client(None, false).unwrap(),
                build_client(None, true).unwrap(),
            )
        };

        let direct_result = direct.get(&server).send().await;
        let following_result = following.get(&server).send().await;

        assert_eq!(
            direct_result
                .expect("direct client must bypass env proxy")
                .status(),
            200
        );
        assert!(
            following_result.is_err(),
            "following client must be routed into the dead proxy"
        );
    }

    #[test]
    fn detect_system_proxy_reads_env_and_reports_source() {
        let _lock = lock_env();
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:7890"));

        assert_eq!(
            detect_system_proxy().map(|proxy| proxy.url).as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(system_proxy_source(), Some(SystemProxySource::Env));
    }

    #[test]
    fn detect_system_proxy_honors_no_proxy_for_probe_target() {
        let _lock = lock_env();
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:7890"));
        // 探测目标被豁免后即使代理变量还在，对外请求实际也走直连。
        // 回环两项必须一并留着：这里写的是进程级变量，抹掉豁免会让其他模块并发建出的
        // 客户端把 127.0.0.1 的请求送进上面那个死代理
        std::env::set_var("NO_PROXY", "api.anthropic.com,127.0.0.1,localhost");

        assert!(detect_system_proxy().is_none());
    }

    #[test]
    fn system_proxy_source_is_never_env_when_env_misses_the_probe_target() {
        let _lock = lock_env();
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:7890"));
        // 只留 HTTP_PROXY：探测目标是 https，仅凭 env 解析不出代理，https 槽只可能由
        // hyper-util 从系统设置回填。无论本机有无系统代理，都不该把结果记在 env 头上。
        std::env::remove_var("HTTPS_PROXY");

        assert_ne!(system_proxy_source(), Some(SystemProxySource::Env));
    }

    #[test]
    fn system_proxy_is_direct_without_source_under_cgi() {
        let _lock = lock_env();
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:7890"));
        // CGI 环境下 hyper-util 直接返回空 matcher（防 `Proxy:` 请求头投毒），
        // 代理变量再全都设着也一律不生效
        let _cgi = ScopedEnvVar::set("REQUEST_METHOD", "GET");

        assert!(detect_system_proxy().is_none());
        assert_eq!(system_proxy_source(), None);
    }

    #[test]
    fn credential_only_proxy_change_is_detected() {
        let _lock = lock_env();
        let _flag = FollowFlagGuard::capture();
        let _env = ProxyEnvGuard::set(Some("http://user:one@127.0.0.1:7890"));

        init(None).unwrap();
        set_follow_system_proxy(true).unwrap();
        apply_proxy(None).unwrap();
        assert!(!system_proxy_status().1);

        // 同一 scheme://host:port，只换密码
        std::env::set_var("HTTPS_PROXY", "http://user:two@127.0.0.1:7890");
        std::env::set_var("HTTP_PROXY", "http://user:two@127.0.0.1:7890");
        assert!(
            system_proxy_status().1,
            "credential rotation must count as a change"
        );
        // 展示地址不含凭据，也不因凭据变化而改变
        assert_eq!(baked_url().as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(
            system_proxy_status().0.as_deref(),
            Some("http://127.0.0.1:7890")
        );

        apply_proxy(None).unwrap();
        assert!(
            !system_proxy_status().1,
            "re-applying bakes the new credentials"
        );
    }

    #[test]
    fn baked_system_proxy_tracks_client_rebuilds() {
        let _lock = lock_env();
        let _flag = FollowFlagGuard::capture();
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:7890"));

        init(None).unwrap();
        set_follow_system_proxy(true).unwrap();
        apply_proxy(None).unwrap();
        assert_eq!(baked_url().as_deref(), Some("http://127.0.0.1:7890"));

        set_follow_system_proxy(false).unwrap();
        assert_eq!(baked_url(), None, "direct mode bakes nothing");

        set_follow_system_proxy(true).unwrap();
        // 不走 apply_proxy：它会把这个死的显式代理装进进程级客户端，而显式代理不吃 NO_PROXY，
        // 同进程并发的其他用例会被它接管。这里要验的只是记录规则，直接调记录函数
        store_baked_system_proxy(plan_baked_system_proxy(Some("http://127.0.0.1:7891"), true));
        assert_eq!(baked_url(), None, "explicit proxy bakes nothing");

        // 提前归还环境变量，让最后一次重建读到本机真实配置：本机可能有真实系统代理，
        // 只能断言烘焙值与当次检测一致，不能断言 None
        drop(_env);
        apply_proxy(None).unwrap();
        assert_eq!(baked_url(), system_proxy_status().0);
    }

    #[test]
    fn own_port_system_proxy_is_not_reported_as_changed() {
        let _lock = lock_env();
        let _flag = FollowFlagGuard::capture();
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:15721"));
        set_proxy_port(15721);

        init(None).unwrap();
        set_follow_system_proxy(true).unwrap();
        apply_proxy(None).unwrap();

        // 指向自身端口时客户端直连、烘焙值为空；状态查询必须拿到同一个空值，
        // 否则「系统代理已变化」会恒亮且重新应用也清不掉
        assert_eq!(baked_url(), None);
        let (current, changed) = system_proxy_status();
        assert_eq!(current, None);
        assert!(!changed, "own-port carve-out must not read as a change");
    }

    #[test]
    fn http_only_proxy_change_is_detected() {
        let _lock = lock_env();
        let _flag = FollowFlagGuard::capture();
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:7890"));
        // 只保留 http 槽的环境变量；https 槽由本机系统设置决定，断言不依赖它
        std::env::remove_var("HTTPS_PROXY");

        init(None).unwrap();
        set_follow_system_proxy(true).unwrap();
        apply_proxy(None).unwrap();
        assert!(!system_proxy_status().1);
        // http 槽只参与比较，不顶上展示：展示值必须仍是 https 槽的解析结果
        assert_eq!(baked_url(), detect_system_proxy().map(|proxy| proxy.url));

        // 走 http 供应商的转发用的是这一槽，只探 https 会漏掉它的变化
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7891");
        assert!(
            system_proxy_status().1,
            "an HTTP-only proxy change must count as a change"
        );
    }

    #[test]
    fn stored_snapshot_is_the_planned_value_not_a_reread() {
        let _lock = lock_env();
        let _flag = FollowFlagGuard::capture();
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:7890"));
        init(None).unwrap();

        let (_client, planned) = build_client_with_snapshot(None, true).unwrap();
        // 模拟 build 之后、记录之前系统配置被改动：记录必须仍是 build 前读到的值，
        // 之后的比较才能把这次改动报出来，而不是新值对新值永远沉默
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:7891");
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7891");
        store_baked_system_proxy(planned);

        assert_eq!(baked_url().as_deref(), Some("http://127.0.0.1:7890"));
        assert!(
            system_proxy_status().1,
            "a change after planning must surface as changed"
        );
    }

    #[test]
    fn socks_credential_rotation_is_detected() {
        let _lock = lock_env();
        let _flag = FollowFlagGuard::capture();
        let _env = ProxyEnvGuard::set(Some("socks5h://user:one@127.0.0.1:1080"));

        init(None).unwrap();
        set_follow_system_proxy(true).unwrap();
        apply_proxy(None).unwrap();
        assert!(!system_proxy_status().1);

        // SOCKS 凭据不走 Basic 头而是 raw_auth，同样必须参与比较
        std::env::set_var("HTTPS_PROXY", "socks5h://user:two@127.0.0.1:1080");
        std::env::set_var("HTTP_PROXY", "socks5h://user:two@127.0.0.1:1080");
        assert!(system_proxy_status().1);
        assert_eq!(baked_url().as_deref(), Some("socks5h://127.0.0.1:1080"));
    }

    #[test]
    fn baked_source_is_captured_at_rebuild() {
        let _lock = lock_env();
        let _flag = FollowFlagGuard::capture();
        // HTTPS_PROXY 一并写入，探测目标是 https，env 槽在任何平台都会命中
        let _env = ProxyEnvGuard::set(Some("http://127.0.0.1:7890"));

        init(None).unwrap();
        set_follow_system_proxy(true).unwrap();
        apply_proxy(None).unwrap();
        assert_eq!(baked_source(), Some(SystemProxySource::Env));

        // 烘焙之后打掉 env 槽（CGI 短路）：此刻实时重解析只会给出 System，
        // 仍读到 Env 才说明来源确实是随地址一起快照下来的
        let _cgi = ScopedEnvVar::set("REQUEST_METHOD", "GET");
        assert_eq!(baked_source(), Some(SystemProxySource::Env));

        set_follow_system_proxy(false).unwrap();
        assert_eq!(baked_source(), None, "direct mode bakes nothing");
    }

    #[tokio::test]
    async fn set_follow_system_proxy_rebuilds_global_client() {
        let dead_proxy = dead_loopback_url();
        let server = spawn_ok_server();

        // _flag 先于 _env 声明：反序析构时先还原 env，标志复位重建客户端才用得上真实 env
        let (following, direct) = {
            let _lock = lock_env();
            let _flag = FollowFlagGuard::capture();
            let _env = ProxyEnvGuard::set(Some(&dead_proxy));
            init(None).unwrap();
            set_follow_system_proxy(true).unwrap();
            let following = get();
            set_follow_system_proxy(false).unwrap();
            let direct = get();
            (following, direct)
        };

        assert!(following.get(&server).send().await.is_err());
        assert_eq!(direct.get(&server).send().await.unwrap().status(), 200);
    }
}

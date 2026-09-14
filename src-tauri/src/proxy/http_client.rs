//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

use once_cell::sync::OnceCell;
use reqwest::Client;
use std::env;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

/// 全局 HTTP 客户端实例
static GLOBAL_CLIENT: OnceCell<RwLock<Client>> = OnceCell::new();

/// 当前代理 URL（用于日志和状态查询）
static CURRENT_PROXY_URL: OnceCell<RwLock<Option<String>>> = OnceCell::new();

/// CC Switch 代理服务器当前监听的端口
static CC_SWITCH_PROXY_PORT: OnceCell<RwLock<u16>> = OnceCell::new();

/// "跟随系统代理"的结构化解析快照（分协议 + NO_PROXY）。
///
/// reqwest/hyper-util 内部按 http/https/all 三类代理外加 NO_PROXY 分别生效，
/// 单个 `Option<String>` 无法表达这些语义：HTTPS 不变、仅 HTTP 变化，或仅
/// NO_PROXY 变化时，单 URL 签名都看不见。快照按 hyper-util 的规则构造
/// （环境变量优先，macOS 系统配置只填补仍为空的协议槽），用作变化检测签名。
///
/// macOS 上 reqwest 还把系统代理的 bypass 规则（ExceptionsList 与
/// ExcludeSimpleHostnames）烘焙进客户端，因此快照以 `system_bypass` 记录
/// 其规范化签名；仅改 bypass 列表时端点槽不变，没有这个字段就检测不到。
/// 非 macOS 恒为 `None`。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SystemProxySnapshot {
    http: Option<String>,
    https: Option<String>,
    all: Option<String>,
    no_proxy: Option<String>,
    system_bypass: Option<String>,
}

/// 当前全局客户端烘焙时的系统代理快照；`None` 表示显式代理生效
/// （刷新逻辑让位于用户设置）或尚未初始化。
static BAKED_SYSTEM_SNAPSHOT: OnceCell<RwLock<Option<SystemProxySnapshot>>> = OnceCell::new();

/// 状态代数：每次 init/apply/update 显式修改全局客户端前递增。
/// 自动刷新提交重建结果时会校验代数，避免覆盖并发发生的显式配置。
static STATE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 客户端因系统代理变化而重建的次数（诊断与回归测试用）
static SYSTEM_PROXY_REBUILD_COUNT: AtomicU64 = AtomicU64::new(0);

/// 上次检查系统代理变化的时间（单调时钟，系统时间回拨不影响节流）
static LAST_SYSTEM_PROXY_CHECK: Mutex<Option<Instant>> = Mutex::new(None);

/// 跟随系统代理的变化检查间隔
const SYSTEM_PROXY_RECHECK_INTERVAL: Duration = Duration::from_secs(5);

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
    // 与 apply_proxy 相同：先捕获快照再构建，构建期间的变化留待下一轮
    // 刷新检测，不会把旧配置的 Client 标记成新配置。
    let baked = if effective_url.is_some() {
        None
    } else {
        Some(read_system_proxy_snapshot())
    };
    let client = build_client(effective_url)?;

    // 尝试初始化全局客户端，如果已存在则记录警告并使用 apply_proxy 更新
    if GLOBAL_CLIENT.set(RwLock::new(client)).is_err() {
        log::warn!(
            "[GlobalProxy] [GP-003] Already initialized, updating instead: {}",
            effective_url
                .map(mask_url)
                .unwrap_or_else(|| "direct connection".to_string())
        );
        // 已初始化，改用 apply_proxy 更新
        return apply_proxy(proxy_url);
    }

    // 初始化代理 URL 记录（OnceCell 首次写入，无并发切换窗口）
    let _ = CURRENT_PROXY_URL.set(RwLock::new(effective_url.map(|s| s.to_string())));
    STATE_GENERATION.fetch_add(1, Ordering::AcqRel);
    store_baked_system_snapshot(baked);

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
    build_client(effective_url)?;
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
    // 先捕获快照再构建：若构建期间系统代理恰好变化，最多造成一次可检测的
    // snapshot mismatch，下一轮刷新自动修正；反过来（先构建后读快照）会把
    // 旧配置的 Client 标记成新配置，之后永远检测不到差异。
    let baked = if effective_url.is_some() {
        None
    } else {
        Some(read_system_proxy_snapshot())
    };
    let new_client = build_client(effective_url)?;

    // Client、CURRENT_PROXY_URL、snapshot 三份状态在同一个 GLOBAL_CLIENT
    // 写锁临界区内切换（锁序与 commit_system_refresh 一致：
    // GLOBAL_CLIENT → CURRENT_PROXY_URL → snapshot）。自动刷新的提交同样
    // 需要先取得该写锁，无法插入到切换中间造成"记录为显式、Client 却跟随
    // 系统"的错位；代数递增也放在临界区内，供已过期的刷新提交复核。
    if let Some(lock) = GLOBAL_CLIENT.get() {
        let mut client = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        STATE_GENERATION.fetch_add(1, Ordering::AcqRel);
        *client = new_client;
        if let Some(url_lock) = CURRENT_PROXY_URL.get() {
            if let Ok(mut url) = url_lock.write() {
                *url = effective_url.map(|s| s.to_string());
            }
        }
        store_baked_system_snapshot(baked);
    } else {
        // 如果还没初始化，则初始化
        return init(proxy_url);
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
    let baked = if effective_url.is_some() {
        None
    } else {
        Some(read_system_proxy_snapshot())
    };
    let new_client = build_client(effective_url)?;

    // 与 apply_proxy 相同：三份状态在同一个 GLOBAL_CLIENT 写锁临界区内
    // 切换，锁序与 commit_system_refresh 一致，刷新无法插入切换中间。
    if let Some(lock) = GLOBAL_CLIENT.get() {
        let mut client = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-001] Failed to acquire write lock: {e}");
            "Failed to update proxy: lock poisoned".to_string()
        })?;
        STATE_GENERATION.fetch_add(1, Ordering::AcqRel);
        *client = new_client;
        if let Some(url_lock) = CURRENT_PROXY_URL.get() {
            if let Ok(mut url) = url_lock.write() {
                *url = effective_url.map(|s| s.to_string());
            }
        }
        store_baked_system_snapshot(baked);
    } else {
        // 如果还没初始化，则初始化
        return init(proxy_url);
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
/// 返回配置了代理的客户端（如果已配置代理），否则返回跟随系统代理的客户端。
/// 未配置显式代理时（macOS），会节流地检测系统代理配置变化并按需重建客户端，
/// 避免应用启动后系统代理关闭/切换导致请求持续打到已移除的代理上。
///
/// 注意：这里只跟随"配置"变化，不做代理性存活性探测——系统仍配置代理但
/// 代理进程未监听时保持 fail-closed（请求按 reqwest 语义正常报错），不静默
/// 直连，避免绕过用户依赖代理实现的流量隔离/审计策略。
pub fn get() -> Client {
    // 先做节流的变化检测再取客户端：若系统代理配置已变化，本次调用就直接
    // 拿到重建后的客户端，而不是把旧客户端多返回一次。
    refresh_system_proxy_if_changed();

    GLOBAL_CLIENT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|c| c.clone())
        .unwrap_or_else(|| {
            log::warn!("[GlobalProxy] [GP-004] Client not initialized, using fallback");
            build_client(None).unwrap_or_default()
        })
}

/// 写入当前全局客户端烘焙时的系统代理快照（未初始化则初始化）
fn store_baked_system_snapshot(baked: Option<SystemProxySnapshot>) {
    match BAKED_SYSTEM_SNAPSHOT.get() {
        Some(lock) => {
            if let Ok(mut baked_lock) = lock.write() {
                *baked_lock = baked;
            }
        }
        None => {
            let _ = BAKED_SYSTEM_SNAPSHOT.set(RwLock::new(baked));
        }
    }
}

/// 读取当前烘焙快照的克隆
fn baked_system_snapshot() -> Option<SystemProxySnapshot> {
    BAKED_SYSTEM_SNAPSHOT
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|snapshot| snapshot.clone())
}

/// 节流地检查"跟随系统代理"的配置是否变化，变化则重建全局客户端
///
/// 仅在未配置显式代理时生效；显式代理由用户管理，不参与自动刷新。
/// 平台边界：reqwest 只在构建时读取一次系统代理，长驻进程里配置关闭/
/// 切换后旧客户端会一直指向失效目标，这个检测只服务该场景的 macOS 桌面
/// 应用；其余平台维持原有行为，不做周期性解析。
fn refresh_system_proxy_if_changed() {
    if !cfg!(target_os = "macos") {
        return;
    }

    // 显式用户代理生效时，跳过自动刷新
    if get_current_proxy_url().is_some() {
        return;
    }

    // 单调时钟节流。try_lock 失败说明已有并发检测在进行，直接让位，
    // 避免多个线程同时通过间隔检查并各自重建一次。
    let Ok(mut last_check) = LAST_SYSTEM_PROXY_CHECK.try_lock() else {
        return;
    };
    if last_check.is_some_and(|last| last.elapsed() < SYSTEM_PROXY_RECHECK_INTERVAL) {
        return;
    }
    *last_check = Some(Instant::now());
    drop(last_check);

    let seen_generation = STATE_GENERATION.load(Ordering::Acquire);
    let baked = baked_system_snapshot();
    // 快照为 None 说明显式代理刚被应用（或未初始化），让位
    let Some(previous) = baked else {
        return;
    };

    let current = read_system_proxy_snapshot();
    if current == previous {
        return;
    }

    // 系统代理配置发生变化（开启/关闭/换地址/换端口/NO_PROXY 调整），重建
    let new_client = match build_client(None) {
        Ok(client) => client,
        Err(e) => {
            log::warn!("[GlobalProxy] Failed to rebuild client after system proxy change: {e}");
            return;
        }
    };
    log::info!(
        "[GlobalProxy] System proxy configuration changed, rebuilt client (http={:?}, https={:?}, all={:?}, no_proxy={:?}, bypass={:?})",
        current.http.as_deref().map(mask_url),
        current.https.as_deref().map(mask_url),
        current.all.as_deref().map(mask_url),
        current.no_proxy.is_some(),
        current.system_bypass
    );
    if commit_system_refresh(new_client, seen_generation, previous, current) {
        SYSTEM_PROXY_REBUILD_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

/// 提交一次系统代理刷新的重建结果
///
/// 提交前在写锁内校验：显式代理仍为空、状态代数未变、烘焙快照仍是检测时
/// 的那份。任一不满足即放弃（例如用户在检测期间应用了显式代理，或另一路
/// 刷新已经提交过），防止自动刷新覆盖用户设置或造成新旧状态错位。
///
/// 锁序契约：所有同时触碰 GLOBAL_CLIENT 与其余状态的路径（本函数与
/// init/apply_proxy/update_proxy）都必须按 GLOBAL_CLIENT → CURRENT_PROXY_URL
/// → BAKED_SYSTEM_SNAPSHOT 的顺序嵌套加锁，保证三份状态在同一个
/// GLOBAL_CLIENT 写锁临界区内切换，刷新无法观察到"半切换"状态。
fn commit_system_refresh(
    candidate: Client,
    seen_generation: u64,
    previous: SystemProxySnapshot,
    updated: SystemProxySnapshot,
) -> bool {
    let Some(client_lock) = GLOBAL_CLIENT.get() else {
        return false;
    };
    let mut client = match client_lock.write() {
        Ok(client) => client,
        Err(_) => return false,
    };

    if STATE_GENERATION.load(Ordering::Acquire) != seen_generation {
        return false;
    }
    if get_current_proxy_url().is_some() {
        return false;
    }
    if baked_system_snapshot().as_ref() != Some(&previous) {
        return false;
    }

    *client = candidate;
    store_baked_system_snapshot(Some(updated));
    true
}

/// 读取"跟随系统代理"的结构化解析结果（环境变量优先，系统配置填空）
fn read_system_proxy_snapshot() -> SystemProxySnapshot {
    // mut 仅在下方 macOS cfg 块中使用，其他平台不触发 unused_mut
    #[allow(unused_mut)]
    let mut snapshot = env_system_proxy_snapshot();

    // macOS 系统配置只填补环境变量仍为空的协议槽（与 hyper-util 的
    // "环境变量优先、系统配置补缺"顺序一致）
    #[cfg(target_os = "macos")]
    {
        let (system_http, system_https, system_bypass) = macos_system_proxy_state();
        if snapshot.https.is_none() {
            snapshot.https = system_https;
        }
        if snapshot.http.is_none() {
            snapshot.http = system_http;
        }
        // bypass 规则恒记录：它与 NO_PROXY 环境变量在 reqwest 里是独立的
        // 匹配层，没有"环境变量存在则系统 bypass 无效"的可靠结论。多记的
        // 代价最多是一次幂等的重建，漏记的代价是旧 bypass 规则驻留。
        snapshot.system_bypass = system_bypass;
    }

    snapshot
}

/// 从环境变量解析分协议代理 URL（优先级：大写与小写同组同权，组间
/// 按协议独立，不再折叠成单个 URL）
fn env_system_proxy_snapshot() -> SystemProxySnapshot {
    let env_group = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            env::var(key)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    };
    SystemProxySnapshot {
        http: env_group(&["HTTP_PROXY", "http_proxy"]),
        https: env_group(&["HTTPS_PROXY", "https_proxy"]),
        all: env_group(&["ALL_PROXY", "all_proxy"]),
        no_proxy: env_group(&["NO_PROXY", "no_proxy"]),
        system_bypass: None,
    }
}

/// macOS 系统代理的分协议条目（SCDynamicStore，与 reqwest/hyper-util 的
/// 数据源一致），返回 (http, https, bypass 签名)
#[cfg(target_os = "macos")]
fn macos_system_proxy_state() -> (Option<String>, Option<String>, Option<String>) {
    use system_configuration::core_foundation::base::CFType;
    use system_configuration::core_foundation::dictionary::CFDictionary;
    use system_configuration::core_foundation::number::CFNumber;
    use system_configuration::core_foundation::string::{CFString, CFStringRef};
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    use system_configuration::sys::schema_definitions::{
        kSCPropNetProxiesExceptionsList, kSCPropNetProxiesExcludeSimpleHostnames,
        kSCPropNetProxiesHTTPEnable, kSCPropNetProxiesHTTPPort, kSCPropNetProxiesHTTPProxy,
        kSCPropNetProxiesHTTPSEnable, kSCPropNetProxiesHTTPSPort, kSCPropNetProxiesHTTPSProxy,
    };

    fn read_entry(
        proxies_map: &CFDictionary<CFString, CFType>,
        enabled_key: CFStringRef,
        host_key: CFStringRef,
        port_key: CFStringRef,
    ) -> Option<String> {
        let enabled = proxies_map
            .find(enabled_key)
            .and_then(|flag| flag.downcast::<CFNumber>())
            .and_then(|flag| flag.to_i32())
            .unwrap_or(0)
            == 1;
        if !enabled {
            return None;
        }
        let host = proxies_map
            .find(host_key)
            .and_then(|v| v.downcast::<CFString>())
            .map(|v| v.to_string())?;
        let port = proxies_map
            .find(port_key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|v| v.to_i32())?;
        Some(format_proxy_entry(&host, port))
    }

    /// ExceptionsList（CFArray<CFString>）+ ExcludeSimpleHostnames 的规范化
    /// 签名。crate 的 `CFType::downcast` 只支持 `CFArray<*const c_void>`，
    /// 元素再按 CFStringRef 逐个还原。
    fn read_bypass(proxies_map: &CFDictionary<CFString, CFType>) -> Option<String> {
        use system_configuration::core_foundation::array::CFArray;
        use system_configuration::core_foundation::base::TCFType;
        let exceptions: Vec<String> = proxies_map
            .find(unsafe { kSCPropNetProxiesExceptionsList })
            .and_then(|value| value.downcast::<CFArray<*const std::ffi::c_void>>())
            .map(|array| {
                array
                    .iter()
                    .map(|entry| unsafe {
                        CFString::wrap_under_get_rule(*entry as CFStringRef).to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let exclude_simple_hostnames = proxies_map
            .find(unsafe { kSCPropNetProxiesExcludeSimpleHostnames })
            .and_then(|flag| flag.downcast::<CFNumber>())
            .and_then(|flag| flag.to_i32())
            .unwrap_or(0)
            == 1;
        canonical_macos_bypass(exceptions, exclude_simple_hostnames)
    }

    fn read_entries() -> Option<(Option<String>, Option<String>, Option<String>)> {
        let store = SCDynamicStoreBuilder::new("cc-switch").build()?;
        let proxies_map = store.get_proxies()?;
        Some((
            read_entry(
                &proxies_map,
                unsafe { kSCPropNetProxiesHTTPEnable },
                unsafe { kSCPropNetProxiesHTTPProxy },
                unsafe { kSCPropNetProxiesHTTPPort },
            ),
            read_entry(
                &proxies_map,
                unsafe { kSCPropNetProxiesHTTPSEnable },
                unsafe { kSCPropNetProxiesHTTPSProxy },
                unsafe { kSCPropNetProxiesHTTPSPort },
            ),
            read_bypass(&proxies_map),
        ))
    }

    // SCDynamicStore 读取失败时按"无系统代理"处理，交给环境变量与直连语义
    read_entries().unwrap_or((None, None, None))
}

/// bypass 配置的规范化签名：主机名比较不区分大小写，列表顺序无语义，
/// 统一小写 + 排序去重，避免无语义变化（大小写调整、条目重排）触发重建。
/// 两者都为空时返回 `None`（与"没有 bypass 规则"同义）。
#[cfg(target_os = "macos")]
fn canonical_macos_bypass(
    exceptions: Vec<String>,
    exclude_simple_hostnames: bool,
) -> Option<String> {
    let mut entries: Vec<String> = exceptions
        .into_iter()
        .map(|entry| entry.trim().to_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect();
    entries.sort();
    entries.dedup();
    if entries.is_empty() && !exclude_simple_hostnames {
        return None;
    }
    Some(format!(
        "exclude_simple_hostnames={};list={}",
        u8::from(exclude_simple_hostnames),
        entries.join(",")
    ))
}

/// 把系统代理条目的 host:port 组装成 URL（裸 IPv6 地址补方括号）
#[cfg(target_os = "macos")]
fn format_proxy_entry(host: &str, port: i32) -> String {
    let is_bare_ipv6 = host
        .parse::<IpAddr>()
        .map(|address| address.is_ipv6())
        .unwrap_or(false);
    if is_bare_ipv6 {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
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
fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
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

    // 有代理地址则使用代理，否则跟随系统代理
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
    } else if system_proxy_points_to_loopback() {
        builder = builder.no_proxy();
        log::warn!("[GlobalProxy] System proxy points to localhost, bypassing to avoid recursion");
    } else {
        // 跟随系统代理。reqwest 内建的自动跟随只在客户端构建时读取一次系统
        // 状态：长驻进程里系统代理配置关闭/切换（代理工具退出、转为 TUN
        // 模式）会让客户端继续使用已移除的代理。macOS 上由 get() 节流检测
        // 配置变化并重建（见 refresh_system_proxy_if_changed）；代理语义
        // （分协议、NO_PROXY）完全交由 reqwest 处理，不做存活探测，也不在
        // 配置仍指向代理时静默直连。
        log::debug!("[GlobalProxy] Following system proxy (no explicit proxy configured)");
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

fn system_proxy_points_to_loopback() -> bool {
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

fn host_is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // url crate 对 IPv6 host 保留方括号（如 "[::1]"），解析前需剥掉
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
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
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    const ENV_KEYS: [&str; 8] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ];

    fn clear_proxy_env() {
        for key in ENV_KEYS {
            std::env::remove_var(key);
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
        let result = build_client(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_http_proxy() {
        let result = build_client(Some("http://127.0.0.1:7890"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_socks5_proxy() {
        let result = build_client(Some("socks5://127.0.0.1:1080"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_invalid_url() {
        // reqwest::Proxy::all 对某些无效 URL 不会立即报错
        // 使用明确无效的 scheme 来触发错误
        let result = build_client(Some("invalid-scheme://127.0.0.1:7890"));
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
        let _guard = env_lock().lock().unwrap();

        // 设置 CC Switch 代理端口
        set_proxy_port(15721);

        clear_proxy_env();

        // 指向 CC Switch 端口的代理应该被跳过
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:15721");
        assert!(system_proxy_points_to_loopback());

        // 指向其他端口的本地代理不应该被跳过
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7890");
        assert!(!system_proxy_points_to_loopback());

        // 非 loopback 地址不应该被跳过
        std::env::set_var("HTTP_PROXY", "http://10.0.0.2:7890");
        assert!(!system_proxy_points_to_loopback());

        clear_proxy_env();
    }

    #[test]
    fn test_env_system_proxy_snapshot_per_protocol() {
        let _guard = env_lock().lock().unwrap();

        clear_proxy_env();
        assert_eq!(
            env_system_proxy_snapshot(),
            SystemProxySnapshot::default(),
            "no env vars must produce an empty snapshot"
        );

        // 分协议独立解析，不再折叠成单个 URL
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:8899");
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7800");
        std::env::set_var("ALL_PROXY", "socks5://127.0.0.1:7801");
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1");
        assert_eq!(
            env_system_proxy_snapshot(),
            SystemProxySnapshot {
                http: Some("http://127.0.0.1:7800".to_string()),
                https: Some("http://127.0.0.1:8899".to_string()),
                all: Some("socks5://127.0.0.1:7801".to_string()),
                no_proxy: Some("localhost,127.0.0.1".to_string()),
                system_bypass: None,
            }
        );

        // 小写变量同样生效
        clear_proxy_env();
        std::env::set_var("https_proxy", "http://127.0.0.1:8899");
        assert_eq!(
            env_system_proxy_snapshot(),
            SystemProxySnapshot {
                https: Some("http://127.0.0.1:8899".to_string()),
                ..Default::default()
            }
        );

        clear_proxy_env();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_format_proxy_entry_brackets_bare_ipv6() {
        assert_eq!(
            format_proxy_entry("127.0.0.1", 7890),
            "http://127.0.0.1:7890"
        );
        assert_eq!(format_proxy_entry("::1", 7890), "http://[::1]:7890");
        assert_eq!(
            format_proxy_entry("proxy.example.com", 8080),
            "http://proxy.example.com:8080"
        );
    }

    #[test]
    fn test_snapshot_detects_http_only_and_no_proxy_only_changes() {
        let _guard = env_lock().lock().unwrap();
        clear_proxy_env();

        // HTTPS 固定，仅 HTTP 变化：签名必须能看见
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:8899");
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7800");
        let before = read_system_proxy_snapshot();
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7801");
        assert_ne!(read_system_proxy_snapshot(), before);

        // 仅 NO_PROXY 变化：签名必须能看见
        let before = read_system_proxy_snapshot();
        std::env::set_var("NO_PROXY", "localhost");
        assert_ne!(read_system_proxy_snapshot(), before);

        clear_proxy_env();
    }

    #[test]
    fn test_snapshot_detects_system_bypass_only_change() {
        // macOS 用户只改 bypass/exception 列表、HTTP/HTTPS 端点不变时，
        // reqwest 烘焙进客户端的匹配器已变；快照必须能看见（这是
        // refresh_system_proxy_if_changed 里 `current == previous` 的判定
        // 输入），否则客户端会一直沿用旧的 bypass 规则。
        let endpoints = SystemProxySnapshot {
            http: Some("http://127.0.0.1:7890".to_string()),
            https: Some("http://127.0.0.1:7890".to_string()),
            ..Default::default()
        };
        let with_exceptions = SystemProxySnapshot {
            system_bypass: Some(
                "exclude_simple_hostnames=0;list=*.internal,10.0.0.0/8".to_string(),
            ),
            ..endpoints.clone()
        };
        assert_ne!(endpoints, with_exceptions);

        let changed_exceptions = SystemProxySnapshot {
            system_bypass: Some("exclude_simple_hostnames=0;list=*.internal".to_string()),
            ..endpoints.clone()
        };
        assert_ne!(with_exceptions, changed_exceptions);

        // ExcludeSimpleHostnames 翻转同样是有效变化，即使列表为空。
        let exclude_only = SystemProxySnapshot {
            system_bypass: Some("exclude_simple_hostnames=1;list=".to_string()),
            ..endpoints
        };
        assert_ne!(exclude_only, SystemProxySnapshot::default());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_canonical_macos_bypass_normalizes_semantically_equal_lists() {
        // 大小写、空白、顺序、重复项都不构成语义变化。
        let normalized = canonical_macos_bypass(
            vec![
                "  *.Internal ".to_string(),
                "10.0.0.0/8".to_string(),
                "*.internal".to_string(),
            ],
            false,
        );
        assert_eq!(
            normalized,
            Some("exclude_simple_hostnames=0;list=*.internal,10.0.0.0/8".to_string())
        );

        // 无任何规则时为 None（与"没有 bypass"同义）。
        assert_eq!(canonical_macos_bypass(vec![], false), None);
        assert_eq!(
            canonical_macos_bypass(vec!["  ".to_string()], false),
            None,
            "空白条目不构成规则"
        );

        // ExcludeSimpleHostnames 单独开启也是有效签名。
        assert_eq!(
            canonical_macos_bypass(vec![], true),
            Some("exclude_simple_hostnames=1;list=".to_string())
        );
    }

    /// 初始化全局客户端并烘焙当前快照（消除 init 路径差异）
    #[cfg(target_os = "macos")]
    fn setup_global_client_with_current_snapshot() {
        if GLOBAL_CLIENT.get().is_none() {
            let _ = init(None);
        }
        store_baked_system_snapshot(Some(read_system_proxy_snapshot()));
    }

    #[cfg(target_os = "macos")]
    fn reset_system_proxy_check_throttle() {
        if let Ok(mut last_check) = LAST_SYSTEM_PROXY_CHECK.try_lock() {
            *last_check = None;
        }
    }

    #[test]
    #[serial_test::serial]
    #[cfg(target_os = "macos")]
    fn test_no_rebuild_when_system_proxy_unchanged() {
        let _guard = env_lock().lock().unwrap();
        clear_proxy_env();

        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:8899");

        setup_global_client_with_current_snapshot();
        let before = system_proxy_rebuild_count();

        // 代理解析结果未变化：多次触发检测都不应重建客户端
        // （回归防护：若快照未正确写入/读取，这里每次都会误判为变化）
        for _ in 0..3 {
            reset_system_proxy_check_throttle();
            refresh_system_proxy_if_changed();
            assert_eq!(
                system_proxy_rebuild_count(),
                before,
                "unchanged system proxy must not trigger client rebuild"
            );
        }

        clear_proxy_env();
    }

    #[test]
    #[serial_test::serial]
    #[cfg(target_os = "macos")]
    fn test_rebuild_once_when_system_proxy_config_removed() {
        let _guard = env_lock().lock().unwrap();
        clear_proxy_env();

        // 代理配置存在时初始化快照，随后移除配置模拟系统代理被关闭
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:8899");

        setup_global_client_with_current_snapshot();
        let before = system_proxy_rebuild_count();

        std::env::remove_var("HTTPS_PROXY");
        reset_system_proxy_check_throttle();
        refresh_system_proxy_if_changed();
        let after_change = system_proxy_rebuild_count();
        assert_eq!(
            after_change,
            before + 1,
            "system proxy config removal must trigger exactly one rebuild"
        );

        // 配置保持移除：后续检测不应再次重建
        reset_system_proxy_check_throttle();
        refresh_system_proxy_if_changed();
        assert_eq!(
            system_proxy_rebuild_count(),
            after_change,
            "stable config must not rebuild repeatedly"
        );

        clear_proxy_env();
    }

    #[test]
    #[serial_test::serial]
    #[cfg(target_os = "macos")]
    fn test_refresh_yields_to_explicit_proxy() {
        let _guard = env_lock().lock().unwrap();
        clear_proxy_env();

        setup_global_client_with_current_snapshot();

        // 用户应用显式代理后，配置变化检测必须让位：不重建、不覆盖
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:8899");
        apply_proxy(Some("http://127.0.0.1:7890")).expect("apply explicit proxy");
        let before = system_proxy_rebuild_count();

        reset_system_proxy_check_throttle();
        refresh_system_proxy_if_changed();
        assert_eq!(
            system_proxy_rebuild_count(),
            before,
            "explicit proxy must suppress automatic refresh"
        );
        assert_eq!(
            get_current_proxy_url().as_deref(),
            Some("http://127.0.0.1:7890"),
            "explicit proxy must stay recorded"
        );
        assert_eq!(
            baked_system_snapshot(),
            None,
            "snapshot must be cleared while explicit proxy is active"
        );

        // 恢复直连（跟随系统）以便后续测试
        apply_proxy(None).expect("restore direct");
        clear_proxy_env();
    }

    #[test]
    #[serial_test::serial]
    #[cfg(target_os = "macos")]
    fn test_stale_refresh_cannot_overwrite_explicit_proxy() {
        let _guard = env_lock().lock().unwrap();
        clear_proxy_env();

        setup_global_client_with_current_snapshot();
        let previous = baked_system_snapshot().expect("snapshot after setup");

        // 模拟竞态：刷新流程读到旧代数后，用户应用了显式代理
        let stale_generation = STATE_GENERATION.load(Ordering::Acquire);
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:8899");
        apply_proxy(Some("http://127.0.0.1:7890")).expect("apply explicit proxy");
        let updated = read_system_proxy_snapshot();
        let candidate = build_client(None).expect("candidate client");

        // 过期代数的提交必须被拒绝：客户端与记录都保持显式代理
        assert!(!commit_system_refresh(
            candidate,
            stale_generation,
            previous,
            updated
        ));
        assert_eq!(
            get_current_proxy_url().as_deref(),
            Some("http://127.0.0.1:7890"),
            "stale refresh must not clobber the explicit proxy record"
        );

        apply_proxy(None).expect("restore direct");
        clear_proxy_env();
    }

    #[cfg(target_os = "macos")]
    fn system_proxy_rebuild_count() -> u64 {
        SYSTEM_PROXY_REBUILD_COUNT.load(Ordering::Relaxed)
    }
}

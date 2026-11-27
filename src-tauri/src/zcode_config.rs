//! ZCode 配置文件读写
//!
//! ZCode (zcode.z.ai) 的配置分散在两个文件：
//!
//! - Provider 配置：`~/.zcode/v2/provider_config.json`（个人供应商层）
//! - MCP 配置：`~/.zcode/cli/config.json`，顶层有 `mcp.servers` 字段
//!
//! MCP 使用标准 `type: "stdio"` 格式，与 Claude Code 一致；连接字段直接透传，
//! 启用状态由 MCP 同步层在 `enabled` 与 `apps.zcode` 之间归一化。
//!
//! # 为什么 Provider 写 `provider_config.json` 而不是 `v2/config.json`
//!
//! 早期实现写到 `~/.zcode/v2/config.json` 的 `provider` 字段，但实测 ZCode
//! 桌面端（app.asar）与运行时（zcode.cjs）都不读取该文件——配置写了等于没写。
//! 真实的供应商解析链是：
//!
//! 1. 内置目录：随应用分发的 `resources/config/provider/zcode-builtin.json`
//!    （模板 + account 套餐 + 模型规则），运行时物化到
//!    `~/.zcode/v2/runtime/provider/<平台>/<版本>/endpoint-*/`；
//! 2. 个人层：`~/.zcode/v2/provider_config.json`（本模块的写入目标）；
//! 3. 激活指针：`~/.zcode/v2/credentials.json` 的 `oauth:active_provider`
//!    （加密存储），以及桌面端 `setting.json` 的家族选择状态。
//!
//! 个人规则 schema（运行时 zod strict 校验，多余字段会被拒）：
//!
//! ```json
//! {
//!   "providerId": "...", "templateId": "...", "providerName": "...", "enabled": true,
//!   "config": {
//!     "group": "standard-personal",
//!     "access": { "type": "api-key", "apiKey": "sk-..." },
//!     "personalModelIds": [], "modelOrder": []
//!   }
//! }
//! ```
//!
//! 约束：个人层不能声明 `templateRules`；`templateId` 必须存在于内置目录；
//! `account:` 前缀的 providerId 禁止携带 access。已知供应商按 `options.baseURL`
//! 推断模板（见 [`ZCODE_TEMPLATES`]），模板自带 endpoint 与内置模型；未知
//! endpoint 写成无模板规则并覆写 `config.api`。

use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::provider::{ZCodeModel, ZCodeModelLimit, ZCodeProviderConfig};
use crate::settings::get_zcode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 进程级写锁：provider 写入与 MCP 同步都是"读-改-写"整个文件，
/// 并发交错会互相覆盖（OpenCode 用同样的锁模式）。
fn zcode_config_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(8);
/// 与 ZCode `acquireFileLock` 调用方的轮询节奏一致。
const LOCK_POLL_INTERVAL_MS: u64 = 25;
/// ownerless 锁（元数据没有可用 pid）超过该宽限期即可回收，对齐 ZCode
/// atomicFileLock.ts 调用方传入的 ownerlessGraceMs（100ms）。
const OWNERLESS_LOCK_GRACE_MS: u64 = 100;
/// 旧版 cc-switch 的单文件锁只在持有期间存在，存留超过该时长必是崩溃残留。
const LEGACY_STALE_LOCK_AGE_MS: u64 = 30_000;
/// 锁元数据来自另一进程的时钟，未来值超出该偏移视为无效
/// （对齐 ZCode 的 MAX_LOCK_METADATA_CLOCK_SKEW_MS）。
const CLOCK_SKEW_ALLOWANCE_MS: u64 = 5 * 60_000;

struct ZCodeFileLock {
    dir: PathBuf,
    owner_file: PathBuf,
}

impl ZCodeFileLock {
    fn acquire(target: &Path) -> Result<Self, AppError> {
        let mut lock_name = target.as_os_str().to_os_string();
        lock_name.push(".lock");
        let dir = PathBuf::from(lock_name);
        Self::acquire_deadline(&dir, Instant::now() + LOCK_WAIT_TIMEOUT)
    }

    /// 镜像 ZCode `acquireFileLock` 的目录协议（packages/shared/src/node/
    /// atomicFileLock.ts）：`mkdir` 抢 `<target>.lock` 目录 → 在目录里写入
    /// `owner-<token>.json` → 复核自己是唯一 owner。目录被占时先按 ZCode 的
    /// 规则回收废弃锁（owner 进程已死，或 ownerless 超过宽限期），否则轮询等待。
    /// 协议必须逐字对齐：两边都是"能解析对方锁、能判断对方死活"才算互斥成立。
    fn acquire_deadline(dir: &Path, deadline: Instant) -> Result<Self, AppError> {
        let pid = std::process::id();
        let created_at_ms = unix_epoch_ms();
        // token 对应 ZCode 的 `${pid}-${Date.now()}-${Math.random()...}`。
        // 同进程的调用被进程级互斥串行化，pid + 毫秒 + 纳秒足以保证唯一。
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.subsec_nanos())
            .unwrap_or(0);
        let token = format!("{pid}-{created_at_ms}-{nanos}");
        let owner_file = dir.join(format!("owner-{token}.json"));
        // payload 与 ZCode 的 owner 文件完全同形：JSON.stringify({pid, createdAt, token})
        // + 换行。有效 pid 是 ZCode 判定"持锁进程存活"的依据，不能省略。
        let payload =
            format!("{{\"pid\":{pid},\"createdAt\":{created_at_ms},\"token\":\"{token}\"}}\n");

        loop {
            match try_acquire_once(dir, &owner_file, &payload) {
                AcquireStep::Acquired => {
                    return Ok(Self {
                        dir: dir.to_path_buf(),
                        owner_file,
                    });
                }
                AcquireStep::Busy => {}
                AcquireStep::Io(error) => return Err(AppError::io(dir, error)),
            }
            if Instant::now() >= deadline {
                return Err(AppError::Config(format!(
                    "Timed out waiting for ZCode config lock: {}",
                    dir.display()
                )));
            }
            thread::sleep(Duration::from_millis(LOCK_POLL_INTERVAL_MS));
        }
    }

    fn release(owner_file: &Path, dir: &Path) {
        // 只删除自己的 owner 文件；锁已被接管时不会碰到后来 writer 的 token。
        if let Err(error) = std::fs::remove_file(owner_file) {
            if error.kind() != ErrorKind::NotFound {
                log::warn!(
                    "Failed to remove ZCode lock owner file '{}': {error}",
                    owner_file.display()
                );
            }
        }
        // 目录非空说明有后来 writer 已进入，rmdir 失败是预期内的 best-effort。
        if let Err(error) = std::fs::remove_dir(dir) {
            if !matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty
            ) {
                log::warn!(
                    "Failed to remove ZCode config lock dir '{}': {error}",
                    dir.display()
                );
            }
        }
    }
}

impl Drop for ZCodeFileLock {
    fn drop(&mut self) {
        Self::release(&self.owner_file, &self.dir);
    }
}

enum AcquireStep {
    Acquired,
    Busy,
    Io(std::io::Error),
}

fn try_acquire_once(dir: &Path, owner_file: &Path, payload: &str) -> AcquireStep {
    match std::fs::create_dir(dir) {
        Ok(()) => match claim_ownership(dir, owner_file, payload) {
            Ok(true) => AcquireStep::Acquired,
            Ok(false) => {
                // 写入期间目录被并发替换/接管，退避重试。
                ZCodeFileLock::release(owner_file, dir);
                AcquireStep::Busy
            }
            Err(error) => {
                ZCodeFileLock::release(owner_file, dir);
                // mkdir 成功后目录被并发清走（NotFound）按竞争处理；其余是真实 IO 错误。
                if error.kind() == ErrorKind::NotFound {
                    AcquireStep::Busy
                } else {
                    AcquireStep::Io(error)
                }
            }
        },
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            reclaim_abandoned_lock(dir);
            AcquireStep::Busy
        }
        Err(error) => AcquireStep::Io(error),
    }
}

/// 写入 owner 文件并复核所有权，返回 false 表示目录在写入期间被并发接管。
/// ZCode 用 dev/ino 复核目录身份；这里用"唯一 owner 且名字是自己"等价判定——
/// 目录被替换时我们的 owner 文件必然随之消失，而 token 含 pid + 时间，不可能撞名。
fn claim_ownership(dir: &Path, owner_file: &Path, payload: &str) -> std::io::Result<bool> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owner_file)?;
    file.write_all(payload.as_bytes())?;

    let own_name = owner_file
        .file_name()
        .expect("owner file is joined onto the lock dir");
    Ok(owner_entry_names(dir)?.as_slice() == [own_name.to_os_string()])
}

/// 锁目录里形如 `owner-*.json` 的条目名（与 ZCode 的过滤规则一致）。
fn owner_entry_names(dir: &Path) -> std::io::Result<Vec<std::ffi::OsString>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let name = entry?.file_name();
        if is_owner_entry_name(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

fn is_owner_entry_name(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.starts_with("owner-") && name.ends_with(".json"))
}

/// 回收废弃锁（镜像 ZCode removeAbandonedLock）：唯一 owner 的进程已退出，
/// 或元数据无效（ownerless）超过宽限期。存活的锁一律不碰。
fn reclaim_abandoned_lock(dir: &Path) {
    let Ok(metadata) = std::fs::metadata(dir) else {
        return; // 锁刚消失，直接重试
    };
    if !metadata.is_dir() {
        // 新协议下存活锁一定是目录；单文件锁只能来自旧版 cc-switch，其仅在
        // 持有期间存在，存留超时必是崩溃残留。
        remove_stale_legacy_lock(dir, &metadata);
        return;
    }

    let Ok(owners) = owner_entry_names(dir) else {
        return;
    };
    if owners.len() == 1 {
        let owner_file = dir.join(&owners[0]);
        if !owner_file_reclaimable(&owner_file) {
            return;
        }
        // 按唯一 owner 文件删除相当于所有权校验：目录已被替换时新 owner 的
        // 文件名不同，remove 不会命中；随后 rmdir 也会因目录非空而拒绝。
        if std::fs::remove_file(&owner_file).is_err() {
            return;
        }
        let _ = std::fs::remove_dir(dir);
        return;
    }

    // 0 个（mkdir 后 owner 落盘前崩溃）或多个 owner：目录时间超过宽限期且
    // 所有 owner 均可回收才清理；后来者新增 owner 会让 rmdir 失败。
    if !timestamp_exceeds_grace(metadata.modified().ok()) {
        return;
    }
    for owner in &owners {
        if !owner_file_reclaimable(&dir.join(owner)) {
            return;
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// 镜像 ZCode isOwnerFileReclaimable：pid 有效且进程已退出，或无 pid 且
/// 创建时间超过 ownerless 宽限期。
fn owner_file_reclaimable(owner_file: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(owner_file) else {
        return false;
    };
    let observed_at_ms = unix_epoch_ms();
    let metadata = parse_lock_metadata(&raw, observed_at_ms);
    match metadata.pid {
        Some(pid) => !is_process_alive(pid),
        None => {
            let created_at = metadata.created_at.or_else(|| {
                validated_timestamp_ms(std::fs::metadata(owner_file).ok()?.modified().ok())
            });
            match created_at {
                Some(created_at) => {
                    observed_at_ms.saturating_sub(created_at) >= OWNERLESS_LOCK_GRACE_MS
                }
                None => false,
            }
        }
    }
}

struct LockMetadata {
    pid: Option<u64>,
    created_at: Option<u64>,
}

/// 镜像 ZCode parseLockMetadata：只有 `{"pid", "createdAt"}` 对象才有 owner
/// 语义；裸数字（旧版单文件锁）、空串等一律视为 ownerless，交给宽限期回收。
fn parse_lock_metadata(raw: &str, observed_at_ms: u64) -> LockMetadata {
    let parsed = match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(object)) => object,
        _ => {
            return LockMetadata {
                pid: None,
                created_at: None,
            };
        }
    };
    let created_at = parsed
        .get("createdAt")
        .and_then(Value::as_f64)
        .filter(|created_at| {
            created_at.is_finite()
                && *created_at >= 0.0
                && *created_at <= observed_at_ms as f64 + CLOCK_SKEW_ALLOWANCE_MS as f64
        })
        // 已过滤为非负有限值，f64 → u64 的 saturating 转换不会失真。
        .map(|created_at| created_at as u64);
    let pid = parsed
        .get("pid")
        .and_then(Value::as_u64)
        .filter(|pid| *pid > 0);
    LockMetadata { pid, created_at }
}

/// 与 ZCode `process.kill(pid, 0)` 的判定一致：只有"进程不存在"才算已退出；
/// 无权限（跨用户/受保护进程）等其它错误按存活处理，宁可等待也不误删活锁。
#[cfg(windows)]
fn is_process_alive(pid: u64) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // 超出 OS pid 范围按存活处理，避免把无法探测的锁误判为废弃。
    let Ok(pid) = u32::try_from(pid) else {
        return true;
    };
    // SAFETY: 只查询进程状态；句柄在下面关掉。
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // ERROR_ACCESS_DENIED 说明进程存在但受保护；其余（含参数无效，
        // 即进程不存在）视为已退出。
        return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
    }
    let mut exit_code = 0u32;
    // SAFETY: 句柄有效，输出参数指向本地变量。
    let alive = unsafe { GetExitCodeProcess(handle, &mut exit_code) != 0 }
        && exit_code == STILL_ACTIVE as u32;
    // SAFETY: OpenProcess 返回的句柄只关这一次。
    unsafe { CloseHandle(handle) };
    alive
}

#[cfg(unix)]
fn is_process_alive(pid: u64) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return true;
    };
    // SAFETY: 信号 0 只做存在性探测，无副作用。
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    errno != libc::ESRCH
}

#[cfg(not(any(windows, unix)))]
fn is_process_alive(_pid: u64) -> bool {
    true
}

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// 镜像 ZCode parseLockTimestamp：未来时间超出时钟偏移容忍视为无效。
fn validated_timestamp_ms(time: Option<SystemTime>) -> Option<u64> {
    let ms = u64::try_from(time?.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()?;
    (ms <= unix_epoch_ms().saturating_add(CLOCK_SKEW_ALLOWANCE_MS)).then_some(ms)
}

fn timestamp_exceeds_grace(time: Option<SystemTime>) -> bool {
    match validated_timestamp_ms(time) {
        Some(ms) => unix_epoch_ms().saturating_sub(ms) >= OWNERLESS_LOCK_GRACE_MS,
        None => false,
    }
}

fn remove_stale_legacy_lock(lock_path: &Path, metadata: &std::fs::Metadata) {
    let Some(modified_ms) = validated_timestamp_ms(metadata.modified().ok()) else {
        return;
    };
    if unix_epoch_ms().saturating_sub(modified_ms) < LEGACY_STALE_LOCK_AGE_MS {
        return;
    }
    let _ = std::fs::remove_file(lock_path);
}

fn with_zcode_file_lock<T>(
    target: &Path,
    operation: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    }
    let _process_guard = zcode_config_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _file_guard = ZCodeFileLock::acquire(target)?;
    operation()
}

/// 获取 ZCode 配置根目录（`~/.zcode`），支持 settings 覆盖
pub fn get_zcode_dir() -> PathBuf {
    if let Some(override_dir) = get_zcode_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir().join(".zcode")
}

/// Provider 配置路径：`~/.zcode/v2/provider_config.json`
pub fn get_zcode_provider_config_path() -> PathBuf {
    get_zcode_dir().join("v2").join("provider_config.json")
}

/// MCP 配置路径：`~/.zcode/cli/config.json`（与 provider 是不同文件！）
pub fn get_zcode_cli_config_path() -> PathBuf {
    get_zcode_dir().join("cli").join("config.json")
}

/// Usage 数据库路径：`~/.zcode/cli/db/db.sqlite`
pub fn get_zcode_usage_db_path() -> PathBuf {
    get_zcode_dir().join("cli").join("db").join("db.sqlite")
}

// ============================================================================
// ZCode 内置模板目录（resources/config/provider/zcode-builtin.json 摘录）
// ============================================================================

/// ZCode 内置供应商模板。`base_url` + `api_type` 用于从 cc-switch 供应商的
/// `options.baseURL` + `kind` 反查 `templateId`。模板自带哪些模型不在这里抄，
/// 由 ZCode 按自己的 zcode-builtin.json 决定（会随版本变）。ZCode 升级后模板若更名，
/// 未匹配的供应商会退化为无模板规则（自定义 api 覆写），仍可用。
struct ZCodeTemplate {
    template_id: &'static str,
    base_url: &'static str,
    access_type: &'static str,
    api_type: &'static str,
}

const ZCODE_TEMPLATES: &[ZCodeTemplate] = &[
    ZCodeTemplate {
        template_id: "zai-api",
        base_url: "https://api.z.ai/api/anthropic",
        access_type: "zhipu-coding-plan-api-key",
        api_type: "anthropic-messages",
    },
    ZCodeTemplate {
        template_id: "zai-standard-api",
        base_url: "https://api.z.ai/api/paas/v4",
        access_type: "api-key",
        api_type: "openai-chat-completions",
    },
    ZCodeTemplate {
        template_id: "bigmodel-api",
        base_url: "https://open.bigmodel.cn/api/anthropic",
        access_type: "zhipu-coding-plan-api-key",
        api_type: "anthropic-messages",
    },
    ZCodeTemplate {
        template_id: "bigmodel-standard-api",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        access_type: "api-key",
        api_type: "openai-chat-completions",
    },
    ZCodeTemplate {
        template_id: "moonshot-kimi",
        base_url: "https://api.moonshot.cn/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
    },
    ZCodeTemplate {
        template_id: "minimax",
        base_url: "https://api.minimaxi.com/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
    },
    ZCodeTemplate {
        template_id: "deepseek",
        base_url: "https://api.deepseek.com/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
    },
    ZCodeTemplate {
        template_id: "qwen-alibaba-model-studio-cn",
        base_url: "https://dashscope.aliyuncs.com/apps/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
    },
    ZCodeTemplate {
        template_id: "qwen-alibaba-model-studio-intl",
        base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        access_type: "api-key",
        api_type: "openai-chat-completions",
    },
    ZCodeTemplate {
        template_id: "xiaomi-mimo",
        base_url: "https://api.xiaomimimo.com/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
    },
    ZCodeTemplate {
        template_id: "openai",
        base_url: "https://api.openai.com/v1",
        access_type: "api-key",
        api_type: "openai-responses",
    },
    ZCodeTemplate {
        template_id: "anthropic",
        base_url: "https://api.anthropic.com/v1",
        access_type: "api-key",
        api_type: "anthropic-messages",
    },
    ZCodeTemplate {
        template_id: "xai",
        base_url: "https://api.x.ai/v1",
        access_type: "api-key",
        api_type: "openai-responses",
    },
    ZCodeTemplate {
        template_id: "openrouter",
        base_url: "https://openrouter.ai/api",
        access_type: "api-key",
        api_type: "anthropic-messages",
    },
];

/// cc-switch 的 provider kind → ZCode 内置目录的 api 类型
fn api_type_for_kind(kind: &str) -> &'static str {
    match kind {
        "openai-compatible" => "openai-chat-completions",
        "openai" => "openai-responses",
        // anthropic 是 ZCode 预设的默认 kind；未知值也按 anthropic 处理
        _ => "anthropic-messages",
    }
}

/// ZCode api 类型 → cc-switch 的 provider kind
fn kind_for_api_type(api_type: &str) -> &'static str {
    match api_type {
        "openai-chat-completions" => "openai-compatible",
        "openai-responses" => "openai",
        _ => "anthropic",
    }
}

fn find_template(base_url: &str, kind: &str) -> Option<&'static ZCodeTemplate> {
    let normalized = base_url.trim().trim_end_matches('/');
    let want_api = api_type_for_kind(kind);
    ZCODE_TEMPLATES
        .iter()
        .find(|t| t.api_type == want_api && t.base_url == normalized)
}

fn template_by_id(template_id: &str) -> Option<&'static ZCodeTemplate> {
    ZCODE_TEMPLATES
        .iter()
        .find(|t| t.template_id == template_id)
}

/// ZCode 自管连接的 providerId（按模板 id 建号，或 builtin/account 套餐）。
/// 这些是用户在 ZCode 内登录/添加产生的，CC Switch 不导入、不删除，
/// 与 OpenCode 等 additive 应用「自管节点跳过」的策略一致。
pub fn is_zcode_managed_provider_id(id: &str) -> bool {
    id.starts_with("builtin:") || id.starts_with("account:") || template_by_id(id).is_some()
}

/// `id` 是不是 ZCode 给「从模板新建」或「新建空白」分配的编号：`<base>` 或 `<base>-<n>`
/// （ZCode config-service 的 nextPersonalProviderId）。
fn is_zcode_numbered_id(id: &str, base: &str) -> bool {
    id == base
        || id
            .strip_prefix(base)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// 这条个人规则是不是用户在 ZCode 里自己建的（不归 CC Switch 管）。
///
/// 这类规则带着 ZCode 侧的模板、端点、logo、请求头等字段，CC Switch 的结构表达不全，
/// 导入后一回写就会丢字段，所以导入时一律跳过：
/// - 内置 / 账号连接、模板 id 本身（[`is_zcode_managed_provider_id`]）；
/// - 带 `templateId` 且 id 是 `<templateId>` 或 `<templateId>-<n>` 的（ZCode 从模板新建）；
/// - 模板不在 CC Switch 已知表里的（还原不出端点）；
/// - `new-provider`、`new-provider-<n>`（ZCode 新建空白供应商的编号）。
pub fn is_zcode_user_owned_rule(rule: &Value) -> bool {
    let Some(id) = rule.get("providerId").and_then(Value::as_str) else {
        return true;
    };
    if is_zcode_managed_provider_id(id) || is_zcode_numbered_id(id, "new-provider") {
        return true;
    }
    match rule.get("templateId").and_then(Value::as_str) {
        Some(template) => template_by_id(template).is_none() || is_zcode_numbered_id(id, template),
        None => false,
    }
}

/// provider_config.json 里用户在 ZCode 自建、导入时要跳过的规则 id。
pub fn zcode_user_owned_provider_ids() -> Result<std::collections::HashSet<String>, AppError> {
    let doc = read_zcode_provider_config()?;
    Ok(read_provider_rules(&doc)
        .iter()
        .filter(|rule| is_zcode_user_owned_rule(rule))
        .filter_map(|rule| {
            rule.get("providerId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect())
}

// ============================================================================
// Provider 配置（v2/provider_config.json）
// ============================================================================

/// 读取 ZCode provider 配置文件
///
/// 文件不存在时返回空对象 `Ok(json!({}))`，不抛错。
pub fn read_zcode_provider_config() -> Result<Value, AppError> {
    let path = get_zcode_provider_config_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse ZCode provider config: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 ZCode provider 配置文件
///
/// 用 `atomic_write_private`（0600）：文件里是 API key，不能写成全局可读。
fn write_zcode_provider_config_unlocked(path: &Path, config: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_private(path, json.as_bytes())?;

    log::debug!("ZCode provider config written to {path:?}");
    Ok(())
}

/// 读取 providerConfigRules.providerRules 数组（只读克隆，无则返回空）
fn read_provider_rules(doc: &Value) -> Vec<Value> {
    doc.get("config")
        .and_then(|c| c.get("providerConfigRules"))
        .and_then(|r| r.get("providerRules"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// 在 doc 上以 read-modify-write 语义更新 providerRules。
/// 只替换（或追加）`providerId == id` 的那条规则，用户的其余规则与
/// modelConfigRules 等原样保留。
///
/// 不写 `providerOrder`：ZCode 会按 builtin 段 + 个人段自行重建该数组
/// （create/delete/reorder 时），外部写入的残缺数组会让新 provider 在
/// 有效顺序里跳到已有个人 provider 之前。字段缺省时 ZCode 按 rules
/// 数组顺序推导，等价于「追加到末尾」。
///
/// `models` 为 `Some` 时同步这家各模型的上下文 / 输出上限（见 [`sync_model_limit_rules`]）；
/// 结构解析不全的兜底写入传 `None`，模型规则原样不动。
fn upsert_provider_rule(
    doc: &mut Value,
    id: &str,
    rule: Value,
    models: Option<&IndexMap<String, ZCodeModel>>,
) -> Result<(), AppError> {
    let shape_error = |what: &str| {
        AppError::Config(format!(
            "ZCode provider_config.json has an unexpected shape ({what}); refusing to rewrite it"
        ))
    };
    // 在副本上改，全部成功才替换：拒绝时调用方手里的文档保持原样
    let mut work = doc.clone();
    let root = work
        .as_object_mut()
        .ok_or_else(|| shape_error("root is not an object"))?;
    // 只认识 v1：更新的格式原样留给 ZCode，别按旧结构改写
    match root.get("schemaVersion") {
        None => {
            root.insert("schemaVersion".into(), json!(1));
        }
        Some(v) if v.as_i64() == Some(1) => {}
        Some(_) => return Err(shape_error("unsupported schemaVersion")),
    }
    let cfg = root
        .entry("config")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| shape_error("config is not an object"))?;
    // ZCode 解码时 modelConfigRules 必须带两个数组，缺了整个个人层都读不进去
    cfg.entry("modelConfigRules")
        .or_insert_with(|| json!({"providerModelRules": [], "manualProviderModelRules": []}));
    let arr = cfg
        .entry("providerConfigRules")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| shape_error("providerConfigRules is not an object"))?
        .entry("providerRules")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| shape_error("providerRules is not an array"))?;
    let previous_model_ids = match arr
        .iter_mut()
        .find(|r| r.get("providerId").and_then(Value::as_str) == Some(id))
    {
        Some(slot) => std::mem::replace(slot, rule)
            .get("config")
            .map(rule_model_ids)
            .unwrap_or_default(),
        None => {
            arr.push(rule);
            Vec::new()
        }
    };
    if let Some(models) = models {
        let model_rules = cfg
            .get_mut("modelConfigRules")
            .ok_or_else(|| shape_error("modelConfigRules is missing"))?;
        sync_model_limit_rules(model_rules, id, models, &previous_model_ids)
            .map_err(shape_error)?;
    }
    *doc = work;
    Ok(())
}

/// 规则 `config` 里列出的模型 id（`personalModelIds` + `modelOrder`，去重保序）。
fn rule_model_ids(rule_config: &Value) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for key in ["modelOrder", "personalModelIds"] {
        for model_id in rule_config
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !model_id.is_empty() && !ids.iter().any(|m| m == model_id) {
                ids.push(model_id.to_string());
            }
        }
    }
    ids
}

// ZCode 模型规则里上下文 / 输出上限所在的路径（`ModelConfig` 的 properties / optionSpecs）。
const CONTEXT_WINDOW_PATH: &[&str] = &["config", "properties", "contextWindow"];
const MAX_OUTPUT_PATH: &[&str] = &["config", "optionSpecs", "maxOutputTokens", "max"];
const PROVIDER_MODEL_RULES: &str = "providerModelRules";
const MANUAL_PROVIDER_MODEL_RULES: &str = "manualProviderModelRules";

/// ZCode 只收正整数（zod `int().positive()`），0 或超出 JS 安全整数的值会让整份个人层作废，
/// 这类值当作没填。
fn positive_limit(value: Option<u64>) -> Option<u64> {
    const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
    value.filter(|n| (1..=MAX_SAFE_INTEGER).contains(n))
}

fn is_exact_model_rule(rule: &Value, provider_id: &str, model_id: &str) -> bool {
    rule.get("providerId").and_then(Value::as_str) == Some(provider_id)
        && rule.get("modelId").and_then(Value::as_str) == Some(model_id)
}

/// 沿 `path` 写入叶子，缺的（或为 null 的）中间层补成对象。
fn set_leaf(root: &mut Value, path: &[&str], leaf: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut node = root;
    for key in parents {
        if !node.is_object() {
            *node = json!({});
        }
        node = node
            .as_object_mut()
            .expect("just ensured object")
            .entry(*key)
            .or_insert_with(|| json!({}));
    }
    if !node.is_object() {
        *node = json!({});
    }
    node.as_object_mut()
        .expect("just ensured object")
        .insert((*last).to_string(), leaf);
}

/// 删掉 `path` 的叶子，并把因此变空的上层对象一并删掉。返回 `root` 是否已变空。
fn clear_leaf(root: &mut Value, path: &[&str]) -> bool {
    let Some(obj) = root.as_object_mut() else {
        return false;
    };
    match path {
        [] => {}
        [leaf] => {
            obj.remove(*leaf);
        }
        [head, rest @ ..] => {
            if obj
                .get_mut(*head)
                .is_some_and(|child| clear_leaf(child, rest))
            {
                obj.remove(*head);
            }
        }
    }
    obj.is_empty()
}

/// 把 CC Switch 表单里的模型上下文 / 输出上限同步进 `modelConfigRules`。
///
/// 写法与 ZCode 自己界面的「推荐配置」模式一致：`providerModelRules` 里一条只带用户填了的
/// 叶子的稀疏规则，其余能力项（图片、推理档位及其请求映射）继续沿用 ZCode 内置推荐值。
/// 不新建手动规则：ZCode 要求手动规则写全所有能力项，连推理参数的映射表达式也要带上。
///
/// - 该模型已有手动规则（用户在 ZCode 里切了手动）：只改填了的两项，空着的保留 ZCode 原值，
///   因为手动规则里这两项是必填的；
/// - 已有推荐规则：填了的写入、空着的删掉，ZCode 写的 `enabled` 等其余字段不动，删空了整条去掉；
/// - 没有规则：至少填了一项才新建；
/// - 从列表里移除的模型：只撤掉这两项，规则里 ZCode 自己的字段留着。
fn sync_model_limit_rules(
    model_rules: &mut Value,
    provider_id: &str,
    models: &IndexMap<String, ZCodeModel>,
    previous_model_ids: &[String],
) -> Result<(), &'static str> {
    let rules = model_rules
        .as_object_mut()
        .ok_or("modelConfigRules is not an object")?;
    for key in [PROVIDER_MODEL_RULES, MANUAL_PROVIDER_MODEL_RULES] {
        if !rules.entry(key).or_insert_with(|| json!([])).is_array() {
            return Err("modelConfigRules entry is not an array");
        }
    }

    let removed = previous_model_ids
        .iter()
        .filter(|m| !models.contains_key(m.as_str()))
        .map(|m| (m.as_str(), None, None));
    let current = models
        .iter()
        .filter(|(m, _)| !m.is_empty())
        .map(|(m, model)| {
            let limit = model.limit.as_ref();
            (
                m.as_str(),
                positive_limit(limit.and_then(|l| l.context)),
                positive_limit(limit.and_then(|l| l.output)),
            )
        });
    let targets: Vec<(&str, Option<u64>, Option<u64>)> = removed.chain(current).collect();

    for (model_id, context, output) in targets {
        let manual = rules[MANUAL_PROVIDER_MODEL_RULES]
            .as_array_mut()
            .expect("checked above");
        if let Some(rule) = manual
            .iter_mut()
            .find(|r| is_exact_model_rule(r, provider_id, model_id))
        {
            if let Some(c) = context {
                set_leaf(rule, CONTEXT_WINDOW_PATH, json!(c));
            }
            if let Some(o) = output {
                set_leaf(rule, MAX_OUTPUT_PATH, json!(o));
            }
            continue;
        }

        let smart = rules[PROVIDER_MODEL_RULES]
            .as_array_mut()
            .expect("checked above");
        let existing = smart
            .iter()
            .position(|r| is_exact_model_rule(r, provider_id, model_id));
        let index = match existing {
            Some(i) => i,
            None if context.is_some() || output.is_some() => {
                smart.push(json!({"providerId": provider_id, "modelId": model_id, "config": {}}));
                smart.len() - 1
            }
            None => continue,
        };
        let rule = &mut smart[index];
        for (path, value) in [(CONTEXT_WINDOW_PATH, context), (MAX_OUTPUT_PATH, output)] {
            match value {
                Some(v) => set_leaf(rule, path, json!(v)),
                None => {
                    clear_leaf(rule, path);
                }
            }
        }
        // 两项都撤掉后规则里只剩身份：整条去掉，免得留一条空规则
        if rule
            .get("config")
            .is_none_or(|c| c.as_object().is_some_and(Map::is_empty))
        {
            smart.remove(index);
        }
    }
    Ok(())
}

/// 从 `modelConfigRules` 读回某个模型的上下文 / 输出上限（手动规则优先，它覆盖推荐规则）。
fn read_model_limit(
    model_rules: Option<&Value>,
    provider_id: &str,
    model_id: &str,
) -> ZCodeModelLimit {
    let find = |key: &str| {
        model_rules
            .and_then(|r| r.get(key))
            .and_then(Value::as_array)
            .and_then(|rules| {
                rules
                    .iter()
                    .find(|r| is_exact_model_rule(r, provider_id, model_id))
            })
    };
    let leaf = |path: &[&str]| {
        [MANUAL_PROVIDER_MODEL_RULES, PROVIDER_MODEL_RULES]
            .into_iter()
            .filter_map(find)
            .find_map(|rule| {
                path.iter()
                    .try_fold(rule, |node, key| node.get(*key))
                    .and_then(Value::as_u64)
            })
    };
    ZCodeModelLimit {
        context: leaf(CONTEXT_WINDOW_PATH),
        output: leaf(MAX_OUTPUT_PATH),
    }
}

/// 删掉 `id` 的规则；返回是否真有改动（没改就不必写盘）。
fn remove_provider_rule(doc: &mut Value, id: &str) -> bool {
    let Some(cfg) = doc.get_mut("config").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    if let Some(arr) = cfg.get_mut("providerOrder").and_then(Value::as_array_mut) {
        let before = arr.len();
        arr.retain(|v| v.as_str() != Some(id));
        changed |= arr.len() != before;
    }
    if let Some(rules) = cfg
        .get_mut("providerConfigRules")
        .and_then(|r| r.get_mut("providerRules"))
        .and_then(Value::as_array_mut)
    {
        let before = rules.len();
        rules.retain(|r| r.get("providerId").and_then(Value::as_str) != Some(id));
        changed |= rules.len() != before;
    }
    // ZCode 删供应商时也会删掉它的全部模型规则（deleteExactForProvider），这里照做
    if let Some(model_rules) = cfg
        .get_mut("modelConfigRules")
        .and_then(Value::as_object_mut)
    {
        for key in [PROVIDER_MODEL_RULES, MANUAL_PROVIDER_MODEL_RULES] {
            if let Some(rules) = model_rules.get_mut(key).and_then(Value::as_array_mut) {
                let before = rules.len();
                rules.retain(|r| r.get("providerId").and_then(Value::as_str) != Some(id));
                changed |= rules.len() != before;
            }
        }
    }
    if cfg
        .get("defaultModelSelection")
        .and_then(|selection| selection.get("providerId"))
        .and_then(Value::as_str)
        == Some(id)
    {
        cfg.remove("defaultModelSelection");
        changed = true;
    }
    changed
}

/// 把 cc-switch 的 ZCodeProviderConfig 转成 provider_config.json 的个人规则。
///
/// 已知 endpoint 按 baseURL + kind 反查模板：模板自带 api 定义与内置模型，
/// 个人规则只补 access（key）与 modelOrder；未知 endpoint 写无模板规则并
/// 覆写 `config.api`（自定义 baseUrl 的唯一通路）。
fn provider_config_to_rule(id: &str, config: &ZCodeProviderConfig) -> Value {
    let base_url = config.options.base_url.as_deref().map(str::trim);
    let template = base_url.and_then(|u| find_template(u, &config.kind));

    let mut access = json!({ "type": template.map(|t| t.access_type).unwrap_or("api-key") });
    if let Some(key) = config
        .options
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        access["apiKey"] = json!(key);
    }

    // 用户选的模型全部写进 personalModelIds：ZCode 解析时会自己剔掉和模板内置重复的
    // （resolver.ts 的 personalIdsInOrder），不用 CC Switch 维护一份会过时的模板模型表。
    // 一个都没选就都留空，ZCode 按模板内置的模型和顺序来。
    let user_model_ids: Vec<Value> = config
        .models
        .keys()
        .filter(|s| !s.is_empty())
        .map(|m| json!(m))
        .collect();
    let (model_order, personal_ids) = (user_model_ids.clone(), user_model_ids);

    let mut rule_config = json!({
        "group": "standard-personal",
        "access": access,
        "personalModelIds": personal_ids,
        "modelOrder": model_order,
    });
    if template.is_none() {
        let mut api = json!({ "type": api_type_for_kind(&config.kind) });
        if let Some(u) = base_url.filter(|u| !u.is_empty()) {
            api["baseUrl"] = json!(u);
        }
        rule_config["api"] = api;
    }

    let mut rule = json!({ "providerId": id, "config": rule_config });
    if let Some(name) = config
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        rule["providerName"] = json!(name);
    }
    if let Some(enabled) = config.enabled {
        rule["enabled"] = json!(enabled);
    }
    if let Some(t) = template {
        rule["templateId"] = json!(t.template_id);
    }
    rule
}

/// 个人规则 → cc-switch 的 ZCodeProviderConfig（import / live 读取用）。
/// 解析不出 providerId 的规则返回 None。
///
/// `model_rules` 是文件里的 `modelConfigRules`：每个模型都带上 `limit`（没设上限就是空的），
/// 让启动导入以 live 为准覆盖数据库里的旧上限。
fn provider_rule_to_config(
    rule: &Value,
    model_rules: Option<&Value>,
) -> Option<(String, ZCodeProviderConfig)> {
    use crate::provider::ZCodeProviderOptions;
    use std::collections::HashMap;

    let id = rule.get("providerId")?.as_str()?.to_string();
    let rule_config = rule.get("config")?;
    let access = rule_config.get("access");
    let api = rule_config.get("api");
    let template = rule
        .get("templateId")
        .and_then(Value::as_str)
        .and_then(template_by_id);

    let api_key = access
        .and_then(|a| a.get("apiKey"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let base_url = api
        .and_then(|a| a.get("baseUrl"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| template.map(|t| t.base_url.to_string()));
    let api_type = api
        .and_then(|a| a.get("type"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| template.map(|t| t.api_type.to_string()));

    let models: IndexMap<String, ZCodeModel> = rule_model_ids(rule_config)
        .into_iter()
        .map(|model_id| {
            let limit = read_model_limit(model_rules, &id, &model_id);
            let model = ZCodeModel {
                limit: Some(limit),
                ..Default::default()
            };
            (model_id, model)
        })
        .collect();

    Some((
        id,
        ZCodeProviderConfig {
            name: rule
                .get("providerName")
                .and_then(Value::as_str)
                .map(str::to_string),
            kind: api_type
                .as_deref()
                .map(kind_for_api_type)
                .unwrap_or("anthropic")
                .to_string(),
            options: ZCodeProviderOptions {
                base_url,
                api_key,
                ..Default::default()
            },
            enabled: rule.get("enabled").and_then(Value::as_bool),
            source: None,
            models,
            extra: HashMap::new(),
        },
    ))
}

/// 兜底：settings_config 解析不成 ZCodeProviderConfig 时，从原始 JSON 里摘出
/// 构成规则所需的最小字段（live.rs 的 raw fallback 路径）。
fn raw_value_to_rule(id: &str, value: &Value) -> Value {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("anthropic");
    let options = value.get("options").cloned().unwrap_or_else(|| json!({}));
    let fallback_config = ZCodeProviderConfig {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        kind: kind.to_string(),
        options: serde_json::from_value(options).unwrap_or_default(),
        enabled: value.get("enabled").and_then(Value::as_bool),
        source: None,
        models: IndexMap::new(),
        extra: HashMap::new(),
    };
    provider_config_to_rule(id, &fallback_config)
}

/// 获取所有 provider 条目（id → cc-switch 格式的 provider JSON）
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let doc = read_zcode_provider_config()?;
    let mut result = Map::new();

    let model_rules = doc.get("config").and_then(|c| c.get("modelConfigRules"));
    for rule in read_provider_rules(&doc) {
        match provider_rule_to_config(&rule, model_rules) {
            Some((id, config)) => {
                let value = match serde_json::to_value(&config) {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!("Failed to serialize ZCode provider '{id}': {e}");
                        continue;
                    }
                };
                result.insert(id, value);
            }
            None => {
                log::warn!("Skipping malformed ZCode provider rule (missing providerId/config)");
            }
        }
    }

    Ok(result)
}

/// 设置（新增/更新）一个 provider 条目
///
/// `config` 应为 ZCodeProviderConfig 形状；解析失败时退化为从原始 JSON 摘
/// 最小字段（与 live.rs 的 raw fallback 对应）。
pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let typed = serde_json::from_value::<ZCodeProviderConfig>(config.clone());
    let rule = match &typed {
        Ok(typed) => provider_config_to_rule(id, typed),
        Err(e) => {
            log::warn!(
                "Failed to parse ZCode provider config for '{id}', writing minimal rule: {e}"
            );
            raw_value_to_rule(id, &config)
        }
    };
    let models = typed.as_ref().ok().map(|t| &t.models);

    let path = get_zcode_provider_config_path();
    with_zcode_file_lock(&path, || {
        let mut doc = read_zcode_provider_config()?;
        upsert_provider_rule(&mut doc, id, rule, models)?;
        write_zcode_provider_config_unlocked(&path, &doc)
    })
}

/// 移除一个 provider 条目（规则 + providerOrder）
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let path = get_zcode_provider_config_path();
    with_zcode_file_lock(&path, || {
        if !path.exists() {
            return Ok(());
        }
        let mut doc = read_zcode_provider_config()?;
        if !remove_provider_rule(&mut doc, id) {
            return Ok(());
        }
        write_zcode_provider_config_unlocked(&path, &doc)
    })
}

/// 以类型化结构写入一个 provider
pub fn set_typed_provider(id: &str, config: &ZCodeProviderConfig) -> Result<(), AppError> {
    let rule = provider_config_to_rule(id, config);
    let path = get_zcode_provider_config_path();
    with_zcode_file_lock(&path, || {
        let mut doc = read_zcode_provider_config()?;
        upsert_provider_rule(&mut doc, id, rule, Some(&config.models))?;
        write_zcode_provider_config_unlocked(&path, &doc)
    })
}

// ============================================================================
// MCP 配置（cli/config.json，独立文件！）
// ============================================================================

/// 读取 ZCode CLI 配置（含 MCP / skills）
///
/// 文件不存在时返回空对象 `Ok(json!({}))`，不抛错。
pub fn read_zcode_cli_config() -> Result<Value, AppError> {
    let path = get_zcode_cli_config_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse ZCode CLI config: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 ZCode CLI 配置
///
/// 同样 0600 + 保持键序：cli/config.json 里还有 hooks 和插件状态，
/// 重排序键会在每次 MCP 同步时制造无意义的大 diff。
fn write_zcode_cli_config_unlocked(path: &Path, config: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_private(path, json.as_bytes())?;

    log::debug!("ZCode CLI config written to {path:?}");
    Ok(())
}

/// 获取所有 MCP server 条目（cli/config.json 的 `mcp.servers` 字段）
///
/// ZCode 的 MCP 使用标准 `type: "stdio"` 格式，直接透传，无需转换。
pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_zcode_cli_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.get("servers"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置（新增/更新）一个 MCP server 条目（透传，不转换格式）
pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let path = get_zcode_cli_config_path();
    with_zcode_file_lock(&path, || {
        let mut full_config = read_zcode_cli_config()?;

        let mcp = full_config
            .as_object_mut()
            .ok_or_else(|| AppError::Config("ZCode CLI config root is not a JSON object".into()))?
            .entry("mcp")
            .or_insert_with(|| json!({}));

        let mcp_obj = mcp.as_object_mut().ok_or_else(|| {
            AppError::Config("ZCode CLI config 'mcp' is not a JSON object".into())
        })?;

        let servers = mcp_obj.entry("servers").or_insert_with(|| json!({}));
        let servers = servers.as_object_mut().ok_or_else(|| {
            AppError::Config("ZCode CLI config 'mcp.servers' is not a JSON object".into())
        })?;
        servers.insert(id.to_string(), config);

        write_zcode_cli_config_unlocked(&path, &full_config)
    })
}

/// 移除一个 MCP server 条目
pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let path = get_zcode_cli_config_path();
    with_zcode_file_lock(&path, || {
        let mut config = read_zcode_cli_config()?;

        if let Some(servers) = config
            .get_mut("mcp")
            .and_then(|v| v.get_mut("servers"))
            .and_then(|v| v.as_object_mut())
        {
            servers.remove(id);
        }

        write_zcode_cli_config_unlocked(&path, &config)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ZCodeModel, ZCodeProviderOptions};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn file_lock_exists_during_write_and_is_removed_afterward() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("provider_config.json");
        let lock_path = temp.path().join("provider_config.json.lock");

        with_zcode_file_lock(&target, || {
            assert!(lock_path.exists());
            Ok(())
        })
        .unwrap();

        assert!(!lock_path.exists());
    }

    #[test]
    fn lock_dir_contains_zcode_shaped_owner_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("provider_config.json");
        let lock_dir = temp.path().join("provider_config.json.lock");

        with_zcode_file_lock(&target, || {
            // ZCode 读取我们的锁靠目录 + owner-*.json 这两个形状特征。
            assert!(lock_dir.is_dir());
            let owners = owner_entry_names(&lock_dir).unwrap();
            assert_eq!(owners.len(), 1);
            let raw = std::fs::read_to_string(lock_dir.join(&owners[0])).unwrap();
            assert!(raw.ends_with('\n'));
            let metadata = parse_lock_metadata(&raw, unix_epoch_ms());
            assert_eq!(metadata.pid, Some(u64::from(std::process::id())));
            assert!(metadata.created_at.is_some());
            Ok(())
        })
        .unwrap();

        assert!(!lock_dir.exists());
    }

    /// 起一个立即退出的子进程，取其 pid：保证探测时进程已不存在。
    fn dead_process_pid() -> u32 {
        #[cfg(windows)]
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    fn write_abandoned_lock(lock_dir: &Path, owner_name: &str, payload: &str) {
        std::fs::create_dir_all(lock_dir).unwrap();
        std::fs::write(lock_dir.join(owner_name), payload).unwrap();
    }

    #[test]
    fn lock_with_dead_owner_process_is_reclaimed() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("provider_config.json");
        let lock_dir = temp.path().join("provider_config.json.lock");
        let pid = dead_process_pid();
        let token = format!("{pid}-{}-1", unix_epoch_ms());
        write_abandoned_lock(
            &lock_dir,
            &format!("owner-{token}.json"),
            &format!(
                "{{\"pid\":{pid},\"createdAt\":{},\"token\":\"{token}\"}}\n",
                unix_epoch_ms()
            ),
        );

        ZCodeFileLock::acquire(&target).unwrap();
    }

    #[test]
    fn ownerless_lock_is_reclaimed_after_grace() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("provider_config.json");
        let lock_dir = temp.path().join("provider_config.json.lock");
        // 无 pid 的元数据交给 ownerless 宽限期回收（与 ZCode 读到损坏 owner 时的路径一致）。
        write_abandoned_lock(&lock_dir, "owner-1-1-broken.json", "{}\n");
        thread::sleep(Duration::from_millis(OWNERLESS_LOCK_GRACE_MS + 50));

        ZCodeFileLock::acquire(&target).unwrap();
    }

    #[test]
    fn lock_with_live_owner_process_times_out_without_reclaim() {
        let temp = tempfile::tempdir().unwrap();
        let lock_dir = temp.path().join("provider_config.json.lock");
        let token = format!("{}-{}-1", std::process::id(), unix_epoch_ms());
        write_abandoned_lock(
            &lock_dir,
            &format!("owner-{token}.json"),
            &format!(
                "{{\"pid\":{},\"createdAt\":{},\"token\":\"{token}\"}}\n",
                std::process::id(),
                unix_epoch_ms()
            ),
        );

        let result =
            ZCodeFileLock::acquire_deadline(&lock_dir, Instant::now() + Duration::from_millis(60));
        assert!(result.is_err());
        // 存活 owner 的锁不得被回收，也不得被写入方破坏。
        assert!(lock_dir.is_dir());
        assert_eq!(owner_entry_names(&lock_dir).unwrap().len(), 1);
    }

    #[test]
    fn legacy_file_lock_is_left_alone_until_stale() {
        let temp = tempfile::tempdir().unwrap();
        let lock_path = temp.path().join("provider_config.json.lock");
        // 旧版单文件锁（裸 pid）且 mtime 很新：视为可能仍被旧版本持有，等待而不清理。
        std::fs::write(&lock_path, format!("{}\n", std::process::id())).unwrap();

        let result =
            ZCodeFileLock::acquire_deadline(&lock_path, Instant::now() + Duration::from_millis(60));
        assert!(result.is_err());
        assert!(lock_path.is_file());
    }

    fn provider(
        name: &str,
        kind: &str,
        base_url: &str,
        api_key: &str,
        models: &[&str],
    ) -> ZCodeProviderConfig {
        ZCodeProviderConfig {
            name: Some(name.to_string()),
            kind: kind.to_string(),
            options: ZCodeProviderOptions {
                base_url: Some(base_url.to_string()),
                api_key: Some(api_key.to_string()),
                ..Default::default()
            },
            enabled: Some(true),
            source: Some("custom".to_string()),
            models: models
                .iter()
                .map(|m| (m.to_string(), ZCodeModel::default()))
                .collect::<IndexMap<_, _>>(),
            extra: HashMap::new(),
        }
    }

    #[test]
    fn deepseek_config_maps_to_template_rule() {
        let rule = provider_config_to_rule(
            "dp-1",
            &provider(
                "DeepSeek",
                "anthropic",
                "https://api.deepseek.com/anthropic",
                "sk-test",
                &["deepseek-flash", "deepseek-v4-pro"],
            ),
        );

        assert_eq!(rule["templateId"], json!("deepseek"));
        assert_eq!(rule["providerName"], json!("DeepSeek"));
        assert_eq!(rule["enabled"], json!(true));
        assert_eq!(
            rule["config"]["access"],
            json!({"type": "api-key", "apiKey": "sk-test"})
        );
        assert_eq!(
            rule["config"]["modelOrder"],
            json!(["deepseek-flash", "deepseek-v4-pro"])
        );
        assert_eq!(rule["config"]["group"], json!("standard-personal"));
        // 模板自带 api 定义，个人规则不得覆写
        assert!(rule["config"].get("api").is_none());
    }

    fn write_rule_and_roundtrip(config: &ZCodeProviderConfig) -> (Value, ZCodeProviderConfig) {
        let rule = provider_config_to_rule("dp-1", config);
        let restored = provider_rule_to_config(&rule, None).unwrap().1;
        (rule, restored)
    }

    #[test]
    fn unknown_endpoint_writes_api_override() {
        let (rule, restored) = write_rule_and_roundtrip(&provider(
            "Custom",
            "openai-compatible",
            "https://api.example.com/v1",
            "sk-x",
            &[],
        ));

        assert!(rule.get("templateId").is_none());
        assert_eq!(
            rule["config"]["api"],
            json!({"type": "openai-chat-completions", "baseUrl": "https://api.example.com/v1"})
        );
        assert_eq!(restored.kind, "openai-compatible");
        assert_eq!(
            restored.options.base_url.as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(restored.options.api_key.as_deref(), Some("sk-x"));
    }

    #[test]
    fn template_rule_roundtrips_kind_and_base_url() {
        let (_, restored) = write_rule_and_roundtrip(&provider(
            "Kimi",
            "anthropic",
            "https://api.moonshot.cn/anthropic/",
            "sk-kimi",
            &[],
        ));

        assert_eq!(restored.kind, "anthropic");
        assert_eq!(
            restored.options.base_url.as_deref(),
            // baseURL 来自模板（回读时模板 URL 无尾斜杠）
            Some("https://api.moonshot.cn/anthropic")
        );
        // 用户没配模型时规则里不列模型，ZCode 按模板内置的模型和顺序来
        assert!(restored.models.is_empty());
    }

    #[test]
    fn upsert_preserves_user_rules_and_order() {
        let mut doc = json!({
            "schemaVersion": 1,
            "config": {
                "providerOrder": ["bigmodel-api"],
                "providerConfigRules": {"providerRules": [
                    {"providerId": "bigmodel-api", "templateId": "bigmodel-api",
                     "config": {"group": "standard-personal",
                                "access": {"type": "zhipu-coding-plan-api-key", "apiKey": "keep"}}}
                ]},
                "modelConfigRules": {"providerModelRules": [], "manualProviderModelRules": []}
            }
        });

        upsert_provider_rule(
            &mut doc,
            "uuid-1",
            json!({"providerId": "uuid-1", "config": {}}),
            None,
        )
        .unwrap();
        upsert_provider_rule(
            &mut doc,
            "uuid-1",
            json!({"providerId": "uuid-1", "config": {"group": "standard-personal"}}),
            None,
        )
        .unwrap();

        let rules = read_provider_rules(&doc);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["providerId"], json!("bigmodel-api"));
        // 用户的 coding-plan access 原样保留
        assert_eq!(rules[0]["config"]["access"]["apiKey"], json!("keep"));
        assert_eq!(rules[1]["providerId"], json!("uuid-1"));
        // 已有的 providerOrder 原样保留：ZCode 自行重建该数组，外部只追加
        // 会产生残缺列表，让新 provider 跳到已有个人 provider 之前
        assert_eq!(doc["config"]["providerOrder"], json!(["bigmodel-api"]));
        // 无关段不动
        assert!(doc["config"]["modelConfigRules"].is_object());
    }

    #[test]
    fn upsert_does_not_create_provider_order() {
        let mut doc = json!({
            "schemaVersion": 1,
            "config": {
                "providerConfigRules": {"providerRules": []},
                "modelConfigRules": {"providerModelRules": [], "manualProviderModelRules": []}
            }
        });

        upsert_provider_rule(
            &mut doc,
            "uuid-1",
            json!({"providerId": "uuid-1", "config": {}}),
            None,
        )
        .unwrap();

        assert!(read_provider_rules(&doc).len() == 1);
        // 字段缺省保持缺省：ZCode 的 encode 对 undefined 本就省略该键
        assert!(doc["config"].get("providerOrder").is_none());
    }

    #[test]
    fn first_write_has_the_shape_zcode_requires() {
        let mut doc = json!({});
        upsert_provider_rule(
            &mut doc,
            "a",
            json!({"providerId": "a", "config": {}}),
            None,
        )
        .unwrap();
        assert_eq!(doc["schemaVersion"], json!(1));
        assert_eq!(
            doc["config"]["modelConfigRules"],
            json!({"providerModelRules": [], "manualProviderModelRules": []})
        );
        assert_eq!(read_provider_rules(&doc).len(), 1);
    }

    #[test]
    fn unknown_layout_is_refused_instead_of_rewritten() {
        for doc in [
            json!([]),
            json!({"schemaVersion": 2, "config": {}}),
            json!({"schemaVersion": 1, "config": []}),
            json!({"schemaVersion": 1, "config": {"providerConfigRules": {"providerRules": {}}}}),
        ] {
            let mut copy = doc.clone();
            assert!(
                upsert_provider_rule(&mut copy, "a", json!({"providerId": "a"}), None).is_err()
            );
            assert_eq!(
                copy, doc,
                "a refused write must leave the document untouched"
            );
        }
    }

    #[test]
    fn removing_an_absent_rule_reports_no_change() {
        let mut doc =
            json!({"schemaVersion": 1, "config": {"providerConfigRules": {"providerRules": []}}});
        assert!(!remove_provider_rule(&mut doc, "missing"));
        assert!(!remove_provider_rule(&mut json!({}), "missing"));
    }

    #[test]
    fn rules_created_in_zcode_are_left_to_zcode() {
        let owned = |rule: Value| is_zcode_user_owned_rule(&rule);
        assert!(owned(json!({"providerId": "builtin:bigmodel-coding-plan"})));
        assert!(owned(
            json!({"providerId": "zai-api", "templateId": "zai-api"})
        ));
        assert!(owned(
            json!({"providerId": "zai-api-2", "templateId": "zai-api"})
        ));
        assert!(owned(
            json!({"providerId": "volcengine-ark", "templateId": "volcengine-ark"})
        ));
        assert!(owned(
            json!({"providerId": "my-ark", "templateId": "volcengine-ark"})
        ));
        assert!(owned(json!({"providerId": "new-provider-3"})));
        assert!(owned(json!({"config": {}})));
        // CC Switch 写的：自定义 key，模板要么没有，要么是已知模板
        assert!(!owned(json!({"providerId": "my-glm"})));
        assert!(!owned(
            json!({"providerId": "my-glm", "templateId": "zai-api"})
        ));
        assert!(!owned(
            json!({"providerId": "zai-api-x", "templateId": "zai-api"})
        ));
    }

    #[test]
    fn remove_provider_rule_cleans_order_and_rules() {
        let mut doc = json!({
            "config": {
                "providerOrder": ["a", "b"],
                "defaultModelSelection": {"providerId": "a", "modelId": "model-a"},
                "providerConfigRules": {"providerRules": [
                    {"providerId": "a", "config": {}},
                    {"providerId": "b", "config": {}}
                ]}
            }
        });

        assert!(remove_provider_rule(&mut doc, "a"));

        assert_eq!(doc["config"]["providerOrder"], json!(["b"]));
        let rules = read_provider_rules(&doc);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["providerId"], json!("b"));
        assert!(doc["config"].get("defaultModelSelection").is_none());
    }

    fn limited(context: Option<u64>, output: Option<u64>) -> ZCodeModel {
        ZCodeModel {
            limit: Some(ZCodeModelLimit { context, output }),
            ..Default::default()
        }
    }

    fn model_rules(doc: &Value, key: &str) -> Vec<Value> {
        doc["config"]["modelConfigRules"][key]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn rule_for<'a>(rules: &'a [Value], provider: &str, model: &str) -> Option<&'a Value> {
        rules
            .iter()
            .find(|r| is_exact_model_rule(r, provider, model))
    }

    #[test]
    fn limits_are_written_as_sparse_recommended_rules() {
        let manual_config = json!({
            "enabled": true,
            "properties": {"contextWindow": 1000, "supportsJsonSchemaOutput": true,
                           "supportsNativeWebSearch": false, "supportsMidConversationSystem": false,
                           "inputFormat": {"supportsImage": true, "supportsVideo": false, "supportsPdf": true}},
            "optionSpecs": {"reasoningLevel": {"values": ["disabled", "enabled"], "map": "{}"},
                            "maxOutputTokens": {"max": 2000}}
        });
        let mut doc = json!({
            "schemaVersion": 1,
            "config": {
                "providerConfigRules": {"providerRules": [
                    {"providerId": "p", "config": {"personalModelIds": ["m1", "gone", "gone-zcode"],
                                                   "modelOrder": ["m1", "gone", "gone-zcode"]}}
                ]},
                "modelConfigRules": {
                    "providerModelRules": [
                        // ZCode 加模型时自己写的规则：enabled 必须原样留下
                        {"providerId": "p", "modelId": "m1", "config": {"enabled": true}},
                        {"providerId": "p", "modelId": "gone",
                         "config": {"properties": {"contextWindow": 5}}},
                        {"providerId": "p", "modelId": "gone-zcode",
                         "config": {"enabled": true, "properties": {"contextWindow": 5}}},
                        {"providerId": "other", "modelId": "m1",
                         "config": {"properties": {"contextWindow": 9}}}
                    ],
                    "manualProviderModelRules": [
                        {"providerId": "p", "modelId": "manual", "config": manual_config}
                    ]
                }
            }
        });
        let models: IndexMap<String, ZCodeModel> = [
            ("m1".to_string(), limited(Some(60000), Some(4321))),
            ("zero".to_string(), limited(Some(0), None)),
            ("manual".to_string(), limited(Some(70000), None)),
            ("plain".to_string(), ZCodeModel::default()),
        ]
        .into_iter()
        .collect();

        upsert_provider_rule(
            &mut doc,
            "p",
            json!({"providerId": "p", "config": {}}),
            Some(&models),
        )
        .unwrap();

        let smart = model_rules(&doc, PROVIDER_MODEL_RULES);
        assert_eq!(
            rule_for(&smart, "p", "m1").unwrap()["config"],
            json!({"enabled": true,
                   "properties": {"contextWindow": 60000},
                   "optionSpecs": {"maxOutputTokens": {"max": 4321}}})
        );
        // 0 不合 ZCode 的正整数约束，按没填处理；没填的不新建规则
        assert!(rule_for(&smart, "p", "zero").is_none());
        assert!(rule_for(&smart, "p", "plain").is_none());
        // 手动规则不另建推荐规则，只改填了的那项
        assert!(rule_for(&smart, "p", "manual").is_none());
        let manual = model_rules(&doc, MANUAL_PROVIDER_MODEL_RULES);
        let mut expected_manual = manual_config.clone();
        expected_manual["properties"]["contextWindow"] = json!(70000);
        assert_eq!(
            rule_for(&manual, "p", "manual").unwrap()["config"],
            expected_manual
        );
        // 移出列表的模型：只剩上限的规则整条去掉，ZCode 的 enabled 留着
        assert!(rule_for(&smart, "p", "gone").is_none());
        assert_eq!(
            rule_for(&smart, "p", "gone-zcode").unwrap()["config"],
            json!({"enabled": true})
        );
        // 别家的规则不碰
        assert_eq!(
            rule_for(&smart, "other", "m1").unwrap()["config"],
            json!({"properties": {"contextWindow": 9}})
        );
    }

    #[test]
    fn clearing_a_limit_keeps_other_zcode_leaves() {
        let mut doc = json!({
            "schemaVersion": 1,
            "config": {
                "providerConfigRules": {"providerRules": []},
                "modelConfigRules": {
                    "providerModelRules": [
                        {"providerId": "p", "modelId": "m", "config": {
                            "properties": {"contextWindow": 60000, "supportsJsonSchemaOutput": true},
                            "optionSpecs": {"maxOutputTokens": {"max": 4321}}}}
                    ],
                    "manualProviderModelRules": []
                }
            }
        });
        let models: IndexMap<String, ZCodeModel> = [("m".to_string(), limited(None, Some(8000)))]
            .into_iter()
            .collect();

        upsert_provider_rule(
            &mut doc,
            "p",
            json!({"providerId": "p", "config": {}}),
            Some(&models),
        )
        .unwrap();

        assert_eq!(
            model_rules(&doc, PROVIDER_MODEL_RULES)[0]["config"],
            json!({"properties": {"supportsJsonSchemaOutput": true},
                   "optionSpecs": {"maxOutputTokens": {"max": 8000}}})
        );
    }

    #[test]
    fn limits_round_trip_through_the_live_reader() {
        let mut doc = json!({});
        let mut config = provider("Custom", "anthropic", "https://api.example.com", "sk", &[]);
        config.models = [
            ("m1".to_string(), limited(Some(60000), Some(4321))),
            ("m2".to_string(), ZCodeModel::default()),
        ]
        .into_iter()
        .collect();
        let rule = provider_config_to_rule("p", &config);
        upsert_provider_rule(&mut doc, "p", rule, Some(&config.models)).unwrap();

        let model_rules = doc["config"].get("modelConfigRules");
        let (_, restored) =
            provider_rule_to_config(&read_provider_rules(&doc)[0], model_rules).unwrap();
        let m1 = restored.models["m1"].limit.as_ref().unwrap();
        assert_eq!((m1.context, m1.output), (Some(60000), Some(4321)));
        // 没设上限的模型也带空 limit：导入时以 live 为准清掉数据库里的旧值
        let m2 = restored.models["m2"].limit.as_ref().unwrap();
        assert_eq!((m2.context, m2.output), (None, None));
    }

    #[test]
    fn removing_a_provider_drops_its_model_rules() {
        let mut doc = json!({
            "config": {
                "providerConfigRules": {"providerRules": [{"providerId": "a", "config": {}}]},
                "modelConfigRules": {
                    "providerModelRules": [
                        {"providerId": "a", "modelId": "m", "config": {"enabled": true}},
                        {"providerId": "b", "modelId": "m", "config": {"enabled": true}}
                    ],
                    "manualProviderModelRules": [
                        {"providerId": "a", "modelId": "x", "config": {}}
                    ]
                }
            }
        });

        assert!(remove_provider_rule(&mut doc, "a"));

        let smart = model_rules(&doc, PROVIDER_MODEL_RULES);
        assert_eq!(smart.len(), 1);
        assert_eq!(smart[0]["providerId"], json!("b"));
        assert!(model_rules(&doc, MANUAL_PROVIDER_MODEL_RULES).is_empty());
    }

    #[test]
    fn managed_ids_cover_builtin_and_templates() {
        assert!(is_zcode_managed_provider_id("builtin:bigmodel-coding-plan"));
        assert!(is_zcode_managed_provider_id(
            "account:bigmodel-individual-coding-plan"
        ));
        assert!(is_zcode_managed_provider_id("bigmodel-api"));
        assert!(is_zcode_managed_provider_id("deepseek"));
        assert!(!is_zcode_managed_provider_id("dp"));
        assert!(!is_zcode_managed_provider_id(
            "3f7d3d1e-2f26-4a44-9a01-6b0b0d3f683b"
        ));
    }
}

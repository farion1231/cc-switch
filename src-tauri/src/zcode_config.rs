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
use crate::provider::ZCodeProviderConfig;
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
/// `options.baseURL` + `kind` 反查 `templateId`；`models` 是模板内置模型，
/// 用作 `modelOrder` 默认值。列表摘自 ZCode 3.14.x 的 zcode-builtin.json
///（revision 30）；ZCode 升级后模板若更名，未匹配的供应商会退化为无模板
/// 规则（自定义 api 覆写），仍可用。
struct ZCodeTemplate {
    template_id: &'static str,
    base_url: &'static str,
    access_type: &'static str,
    api_type: &'static str,
    models: &'static [&'static str],
}

const GLM_CODING_PLAN_MODELS: &[&str] = &["GLM-5.3", "GLM-5.3-Flash"];
const GLM_STANDARD_MODELS: &[&str] = &[
    "GLM-5.3",
    "GLM-5.3-Flash",
    "GLM-5V-Turbo",
    "GLM-5.1",
    "GLM-5.1-Highspeed",
    "GLM-5",
    "GLM-5-Turbo",
    "GLM-4.7",
    "GLM-4.7-FlashX",
    "GLM-4.7-Flash",
    "GLM-4.6",
    "GLM-4.5-Air",
    "GLM-4.5",
    "GLM-4.6V",
    "GLM-4.6V-Flash",
    "GLM-4.6V-FlashX",
];

const ZCODE_TEMPLATES: &[ZCodeTemplate] = &[
    ZCodeTemplate {
        template_id: "zai-api",
        base_url: "https://api.z.ai/api/anthropic",
        access_type: "zhipu-coding-plan-api-key",
        api_type: "anthropic-messages",
        models: GLM_CODING_PLAN_MODELS,
    },
    ZCodeTemplate {
        template_id: "zai-standard-api",
        base_url: "https://api.z.ai/api/paas/v4",
        access_type: "api-key",
        api_type: "openai-chat-completions",
        models: GLM_STANDARD_MODELS,
    },
    ZCodeTemplate {
        template_id: "bigmodel-api",
        base_url: "https://open.bigmodel.cn/api/anthropic",
        access_type: "zhipu-coding-plan-api-key",
        api_type: "anthropic-messages",
        models: GLM_CODING_PLAN_MODELS,
    },
    ZCodeTemplate {
        template_id: "bigmodel-standard-api",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        access_type: "api-key",
        api_type: "openai-chat-completions",
        models: GLM_STANDARD_MODELS,
    },
    ZCodeTemplate {
        template_id: "moonshot-kimi",
        base_url: "https://api.moonshot.cn/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
        models: &["kimi-k3", "kimi-k2.7-code", "kimi-k2.6", "k3", "k3-256k"],
    },
    ZCodeTemplate {
        template_id: "minimax",
        base_url: "https://api.minimaxi.com/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
        models: &["MiniMax-M3", "MiniMax-M2.7", "MiniMax-M2.5", "MiniMax-M2"],
    },
    ZCodeTemplate {
        template_id: "deepseek",
        base_url: "https://api.deepseek.com/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
        models: &["deepseek-flash", "deepseek-v4-pro"],
    },
    ZCodeTemplate {
        template_id: "qwen-alibaba-model-studio-cn",
        base_url: "https://dashscope.aliyuncs.com/apps/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
        models: &[
            "qwen3.8-max",
            "qwen3.8-flash",
            "qwen3.7-max",
            "qwen3.5-plus",
        ],
    },
    ZCodeTemplate {
        template_id: "qwen-alibaba-model-studio-intl",
        base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        access_type: "api-key",
        api_type: "openai-chat-completions",
        models: &[
            "qwen3.8-max",
            "qwen3.8-flash",
            "qwen3.7-max",
            "qwen3.5-plus",
        ],
    },
    ZCodeTemplate {
        template_id: "xiaomi-mimo",
        base_url: "https://api.xiaomimimo.com/anthropic",
        access_type: "api-key",
        api_type: "anthropic-messages",
        models: &["mimo-v2.5-pro", "mimo-v2.5"],
    },
    ZCodeTemplate {
        template_id: "openai",
        base_url: "https://api.openai.com/v1",
        access_type: "api-key",
        api_type: "openai-responses",
        models: &["gpt-5.6", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex"],
    },
    ZCodeTemplate {
        template_id: "anthropic",
        base_url: "https://api.anthropic.com/v1",
        access_type: "api-key",
        api_type: "anthropic-messages",
        models: &[
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-haiku-4-5-20251001",
        ],
    },
    ZCodeTemplate {
        template_id: "xai",
        base_url: "https://api.x.ai/v1",
        access_type: "api-key",
        api_type: "openai-responses",
        models: &["grok-4.6", "grok-build-0.1", "grok-4.3"],
    },
    ZCodeTemplate {
        template_id: "openrouter",
        base_url: "https://openrouter.ai/api",
        access_type: "api-key",
        api_type: "anthropic-messages",
        models: &[
            "anthropic/claude-opus-5",
            "openai/gpt-5.6",
            "deepseek/deepseek-v4-pro",
            "z-ai/glm-5.3",
        ],
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
fn upsert_provider_rule(doc: &mut Value, id: &str, rule: Value) {
    if doc.get("schemaVersion").is_none() {
        doc["schemaVersion"] = json!(1);
    }
    if !doc.get("config").is_some_and(Value::is_object) {
        doc["config"] = json!({});
    }
    let cfg = doc
        .get_mut("config")
        .and_then(Value::as_object_mut)
        .expect("config forced to object above");

    let rules_root = cfg
        .entry("providerConfigRules")
        .or_insert_with(|| json!({}));
    if !rules_root.is_object() {
        *rules_root = json!({});
    }
    let rules_obj = rules_root
        .as_object_mut()
        .expect("providerConfigRules normalized to object above");
    let arr = rules_obj
        .entry("providerRules")
        .or_insert_with(|| json!([]));
    if !arr.is_array() {
        *arr = json!([]);
    }
    let arr = arr
        .as_array_mut()
        .expect("providerRules normalized to array");
    if let Some(slot) = arr
        .iter_mut()
        .find(|r| r.get("providerId").and_then(Value::as_str) == Some(id))
    {
        *slot = rule;
    } else {
        arr.push(rule);
    }
}

fn remove_provider_rule(doc: &mut Value, id: &str) {
    let Some(cfg) = doc.get_mut("config").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(arr) = cfg.get_mut("providerOrder").and_then(Value::as_array_mut) {
        arr.retain(|v| v.as_str() != Some(id));
    }
    if let Some(rules) = cfg
        .get_mut("providerConfigRules")
        .and_then(|r| r.get_mut("providerRules"))
        .and_then(Value::as_array_mut)
    {
        rules.retain(|r| r.get("providerId").and_then(Value::as_str) != Some(id));
    }
    if cfg
        .get("defaultModelSelection")
        .and_then(|selection| selection.get("providerId"))
        .and_then(Value::as_str)
        == Some(id)
    {
        cfg.remove("defaultModelSelection");
    }
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

    let template_models = template.map(|t| t.models).unwrap_or(&[]);
    let user_model_ids: Vec<&str> = config
        .models
        .keys()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    let (model_order, personal_ids): (Vec<Value>, Vec<Value>) = if user_model_ids.is_empty() {
        (
            template_models.iter().map(|m| json!(m)).collect(),
            Vec::new(),
        )
    } else {
        (
            user_model_ids.iter().map(|m| json!(m)).collect(),
            user_model_ids
                .iter()
                .filter(|m| !template_models.contains(m))
                .map(|m| json!(m))
                .collect(),
        )
    };

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
fn provider_rule_to_config(rule: &Value) -> Option<(String, ZCodeProviderConfig)> {
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

    let mut models = IndexMap::new();
    for key in ["modelOrder", "personalModelIds"] {
        if let Some(ids) = rule_config.get(key).and_then(Value::as_array) {
            for model_id in ids.iter().filter_map(Value::as_str) {
                if !model_id.is_empty() {
                    models.entry(model_id.to_string()).or_default();
                }
            }
        }
    }

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

    for rule in read_provider_rules(&doc) {
        match provider_rule_to_config(&rule) {
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
    let rule = match serde_json::from_value::<ZCodeProviderConfig>(config.clone()) {
        Ok(typed) => provider_config_to_rule(id, &typed),
        Err(e) => {
            log::warn!(
                "Failed to parse ZCode provider config for '{id}', writing minimal rule: {e}"
            );
            raw_value_to_rule(id, &config)
        }
    };

    let path = get_zcode_provider_config_path();
    with_zcode_file_lock(&path, || {
        let mut doc = read_zcode_provider_config()?;
        upsert_provider_rule(&mut doc, id, rule);
        write_zcode_provider_config_unlocked(&path, &doc)
    })
}

/// 移除一个 provider 条目（规则 + providerOrder）
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let path = get_zcode_provider_config_path();
    with_zcode_file_lock(&path, || {
        let mut doc = read_zcode_provider_config()?;
        remove_provider_rule(&mut doc, id);
        write_zcode_provider_config_unlocked(&path, &doc)
    })
}

/// 以类型化结构读取所有 provider
pub fn get_typed_providers() -> Result<IndexMap<String, ZCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<ZCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse ZCode provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

/// 以类型化结构写入一个 provider
pub fn set_typed_provider(id: &str, config: &ZCodeProviderConfig) -> Result<(), AppError> {
    let rule = provider_config_to_rule(id, config);
    let path = get_zcode_provider_config_path();
    with_zcode_file_lock(&path, || {
        let mut doc = read_zcode_provider_config()?;
        upsert_provider_rule(&mut doc, id, rule);
        write_zcode_provider_config_unlocked(&path, &doc)
    })
}

fn apply_default_model_selection(doc: &mut Value, id: &str) -> Result<(), AppError> {
    let model_id = read_provider_rules(doc)
        .into_iter()
        .find(|rule| rule.get("providerId").and_then(Value::as_str) == Some(id))
        .and_then(|rule| {
            rule.get("config")
                .and_then(|config| config.get("modelOrder"))
                .and_then(Value::as_array)
                .and_then(|models| models.first())
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "ZCode provider '{id}' has no selectable model; add at least one model before switching"
            ))
        })?;

    let config = doc
        .get_mut("config")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| AppError::Config("ZCode provider config root is invalid".into()))?;
    config.insert(
        "defaultModelSelection".to_string(),
        json!({"providerId": id, "modelId": model_id}),
    );
    Ok(())
}

/// Make a provider's first ordered model the configured ZCode default.
///
/// Provider insertion and activation stay separate so adding a provider in
/// additive mode does not unexpectedly switch the running ZCode model.
pub fn set_default_model_selection(id: &str) -> Result<(), AppError> {
    let path = get_zcode_provider_config_path();
    with_zcode_file_lock(&path, || {
        let mut doc = read_zcode_provider_config()?;
        apply_default_model_selection(&mut doc, id)?;
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
        let restored = provider_rule_to_config(&rule).unwrap().1;
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
        // 用户没配模型时回填模板内置模型
        assert!(restored.models.contains_key("kimi-k2.6"));
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
        );
        upsert_provider_rule(
            &mut doc,
            "uuid-1",
            json!({"providerId": "uuid-1", "config": {"group": "standard-personal"}}),
        );

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
        );

        assert!(read_provider_rules(&doc).len() == 1);
        // 字段缺省保持缺省：ZCode 的 encode 对 undefined 本就省略该键
        assert!(doc["config"].get("providerOrder").is_none());
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

        remove_provider_rule(&mut doc, "a");

        assert_eq!(doc["config"]["providerOrder"], json!(["b"]));
        let rules = read_provider_rules(&doc);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["providerId"], json!("b"));
        assert!(doc["config"].get("defaultModelSelection").is_none());
    }

    #[test]
    fn switch_sets_the_first_ordered_model_as_default() {
        let mut doc = json!({
            "schemaVersion": 1,
            "config": {
                "providerConfigRules": {"providerRules": [{
                    "providerId": "custom",
                    "config": {"modelOrder": ["model-b", "model-a"]}
                }]},
                "modelConfigRules": {"providerModelRules": [], "manualProviderModelRules": []}
            }
        });

        apply_default_model_selection(&mut doc, "custom").unwrap();

        assert_eq!(
            doc["config"]["defaultModelSelection"],
            json!({"providerId": "custom", "modelId": "model-b"})
        );
    }

    #[test]
    fn switch_rejects_a_provider_without_models() {
        let mut doc = json!({
            "config": {
                "providerConfigRules": {"providerRules": [{
                    "providerId": "empty",
                    "config": {"modelOrder": []}
                }]}
            }
        });

        let error = apply_default_model_selection(&mut doc, "empty").unwrap_err();
        assert!(error.to_string().contains("no selectable model"));
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

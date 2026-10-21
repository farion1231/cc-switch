//! 暴露边界配置：绑定范围、CORS 模式、Origin 白名单。
//!
//! 判据与状态都在这里，命令层（`commands::remote`）只是薄壳。
//!
//! 绑定范围**不是**一个可以直接 `set` 的字段。它走两段式：
//!
//! 1. [`BindScopeGate::request_bind_scope_change`]：
//!    - **收紧**（`AllInterfaces → Tailscale → Loopback`）立即落盘，直接生效；
//!    - **放宽**只记下一条待确认请求并返回 confirmation id —— 渲染层能做的仅止于此。
//! 2. 原生弹窗（桌面）或 REST 确认端点（浏览器）确认后，
//!    [`BindScopeGate::confirm_bind_scope_change`] 才兑现并落盘。
//!
//! 第二段缺了任何一条，第二段之前配置都不变，也没有文件被写。
//!
//! 为什么放宽必须两段式：`BindScope` 每一档扩大的都是「哪些机器能连上这个 HTTP
//! 服务」的面。如果放宽只是一次 `set`，那么任何能在渲染层执行一行代码的路径
//! （XSS、被替换的页面、被攻陷的 webview）都能把服务推到 `0.0.0.0`，而用户
//! 从未看见过任何提示。确认这一步把「决定」从渲染层挪回 Rust 侧：前端只能**申请**。
//!
//! # 诚实的边界声明
//!
//! - 非回环来源的请求**仍然必须持有配对凭证**：这是既有鉴权模型
//!   （[`crate::remote::auth::AuthScope::Protected`]），本功能不改它。
//! - 对「已被完全攻陷、且已配对的客户端」，两段式是**提高门槛 + 留痕**，
//!   不是硬边界。它能挡住的是「渲染层一行代码就想放宽」，挡不住一枚合法 token
//!   背后的攻击者自己走完申请 + 确认两步。
//! - 唯一的硬边界是「必须回到那台机器上点原生弹窗」。本设计**没有**选它：
//!   浏览器内也能完成整条链路（这是产品要求），所以不能声称自己防得住一切。
//!
//! CORS 则相反：收紧（选白名单）是随时可做的，放宽不在这个文件的能力范围内 —
//! [`CorsMode`] 本身就没有「任意 origin」那个变体。

use super::{BindScope, CorsMode, RemoteConfig};
use crate::remote::auth::SessionIdentity;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 一次点火的有效期。用户没管那个弹窗时，待确认记录过期，不得事后兑现。
pub const BIND_SCOPE_CONFIRM_TTL: Duration = Duration::from_secs(300);

/// 落盘文件名（在凭据目录里，与配对凭据同目录）。
const FILE_NAME: &str = "exposure.json";

/// 落盘文件路径。测试与生产走同一条路径，测试因此能断言「有没有写文件」。
fn exposure_file(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// 落盘形态。三个字段就是全部的可配置暴露边界。
///
/// `bind_scope` 是 `Option`：`None` 表示「这份文件从未显式声明过绑定范围」。
/// 这个区分是旧设置迁移的判据 —— 老用户只开过 `remote_tailscale_enabled`，
/// `exposure.json` 里没有档位，启动时应当按 `Tailscale` 处理；一旦用户显式
/// 选过（包括选回 `Loopback`），就不再让旧旗标翻案。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Persisted {
    #[serde(skip_serializing_if = "Option::is_none")]
    bind_scope: Option<BindScope>,
    cors_mode: CorsMode,
    origin_whitelist: Vec<String>,
}

/// 一次申请的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindScopeRequest {
    /// 收紧：已经生效（调用方随后重绑 listener）。
    Applied(BindScope),
    /// 放宽：只登记了一条待确认请求，**配置未变、未落盘**。
    Pending {
        confirmation_id: String,
        /// Unix 秒。仅用于展示剩余时间。
        expires_at_unix: i64,
    },
}

/// 白名单里的单个 origin 是否可接受。
///
/// 三条判据都不是「洁癖」，每一条都对应一类真实存在、能被利用的来源：
///
/// - `Origin: null` — sandbox iframe、`file:`、`data:` 页面发出的请求都带它。
///   它不归属任何站点，把它列进白名单等于把整个浏览器-extension / sandbox
///   面交给使用者。
/// - `*` — 它就是「任意 origin」的另一种写法；既然 [`CorsMode`] 上不放这条路，
///   列表里也不给。
/// - 空串 / 非 `http(s)` — 要么是白名单写错了，要么是想用 `file:` 那一类
///   本地 scheme 绕过归属判断。
fn is_allowed_origin(origin: &str) -> bool {
    let trimmed = origin.trim();
    if trimmed.is_empty() || trimmed == "*" || trimmed == "null" {
        return false;
    }
    // scheme 前缀之外若还有空白/通配，一并拒。
    if trimmed.contains(char::is_whitespace) {
        return false;
    }
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

/// 校验整份白名单。全过或全不过 —— 半份写入的列表比一份都没有更难推理。
pub fn validate_whitelist(origins: &[String]) -> Result<(), String> {
    for origin in origins {
        if !is_allowed_origin(origin) {
            return Err(format!(
                "origin whitelist entry rejected: {origin:?} \
                 (allowed: explicit http(s) origins; \"null\", \"*\" and blank are never allowed)"
            ));
        }
    }
    Ok(())
}

/// 暴露边界的持久化与两段式闸门。
///
/// `dir` 是凭据目录（与配对凭据同一个）：暴露边界和凭据同属「这台机器上的远程
/// 服务开放到什么程度」，分开存只会让两条线的答案对不上。
pub struct BindScopeGate {
    dir: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl Clone for BindScopeGate {
    /// 克隆的是同一份状态，不是快照：待确认请求登记在一处，就得能被另一处兑现。
    fn clone(&self) -> Self {
        Self {
            dir: self.dir.clone(),
            inner: self.inner.clone(),
        }
    }
}

/// 一条待确认请求。
#[derive(Debug, Clone)]
struct PendingRequest {
    scope: BindScope,
    /// 点火时刻。过期判定用注入的 `now`，测试不必真的睡五分钟。
    requested_at: Instant,
    /// 发起这次申请的那个会话。确认方必须与它相同（见 [`SessionIdentity::same_session`]）。
    identity: SessionIdentity,
}

#[derive(Debug)]
struct Inner {
    persisted: Persisted,
    /// confirmation id → 待确认请求。
    pending: HashMap<String, PendingRequest>,
    /// 待确认请求的有效期。生产恒为 [`BIND_SCOPE_CONFIRM_TTL`]；E2E 用短 TTL
    /// 在几百毫秒内验到同一条过期判据。
    ttl: Duration,
}

/// 拿锁。毒了就当作配置读不出来 —— 一份被 panic 过的锁不值得让整个命令层挂掉。
fn lock(inner: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

impl BindScopeGate {
    /// 全部默认（回环 / 同源 / 空白名单），不读盘。
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_ttl(dir, BIND_SCOPE_CONFIRM_TTL)
    }

    fn with_ttl(dir: impl Into<PathBuf>, ttl: Duration) -> Self {
        Self {
            dir: dir.into(),
            inner: Arc::new(Mutex::new(Inner {
                persisted: Persisted::default(),
                pending: HashMap::new(),
                ttl,
            })),
        }
    }

    /// 从 `dir` 读出已落盘的暴露边界；没有文件就是默认值。
    ///
    /// 文件存在但读不出来 / 字段不合法时同样回默认值：这三条判据都是「收紧」
    /// 方向的默认值，宁可退回最紧的一档，也不要因为一个坏文件让服务带着更宽的
    /// 边界起来。
    pub fn load(dir: impl Into<PathBuf>) -> Self {
        Self::load_with_ttl(dir, BIND_SCOPE_CONFIRM_TTL)
    }

    /// [`Self::load`] 的可注入 TTL 版本（见 [`Inner::ttl`]）。
    pub fn load_with_ttl(dir: impl Into<PathBuf>, ttl: Duration) -> Self {
        let dir = dir.into();
        let gate = Self::with_ttl(dir.clone(), ttl);
        let persisted = read_persisted(&exposure_file(&dir)).unwrap_or_default();
        lock(&gate.inner).persisted = persisted;
        gate
    }

    /// 当前暴露边界的一份快照（`base` 之外的字段由落盘值覆盖）。
    ///
    /// 用 `base` 而不是 `RemoteConfig::default()` 拼：端口 / enabled 这些不属于
    /// 本闸门的字段必须来自调用方的真实配置，否则 `get_remote_config` 会永远
    /// 报默认端口 4000。
    pub fn overlay(&self, base: &RemoteConfig) -> RemoteConfig {
        let persisted = lock(&self.inner).persisted.clone();
        RemoteConfig {
            bind_scope: persisted.bind_scope.unwrap_or_default(),
            cors_mode: persisted.cors_mode,
            origin_whitelist: persisted.origin_whitelist,
            ..base.clone()
        }
    }

    /// 当前暴露边界的一份快照（端口取 `RemoteConfig::default()`）。
    pub fn read_remote_config(&self) -> RemoteConfig {
        self.overlay(&RemoteConfig::default())
    }

    /// 第一段：申请把绑定范围改成 `scope`。
    ///
    /// - 目标不比当前宽（**收紧**）：立即落盘并返回 [`BindScopeRequest::Applied`]。
    ///   收紧不需要第二段 —— 它缩小的是可达面，任何攻击者都不会借它获益。
    /// - 目标更宽（**放宽**）：只登记一条待确认请求，返回
    ///   [`BindScopeRequest::Pending`]。**不改配置、不落盘** —— 这是本函数存在的
    ///   全部理由。返回的 id 是第二段确认时的凭据。
    pub fn request_bind_scope_change(
        &self,
        scope: BindScope,
        identity: SessionIdentity,
    ) -> Result<BindScopeRequest, String> {
        self.request_bind_scope_change_at(scope, identity, Instant::now())
    }

    /// [`Self::request_bind_scope_change`] 的注入时钟版本（测试用）。
    pub fn request_bind_scope_change_at(
        &self,
        scope: BindScope,
        identity: SessionIdentity,
        now: Instant,
    ) -> Result<BindScopeRequest, String> {
        let mut guard = lock(&self.inner);
        let ttl = guard.ttl;
        purge_expired(&mut guard.pending, &ttl, now);
        let current = guard.persisted.bind_scope.unwrap_or_default();

        if !scope.widens_from(current) {
            guard.persisted.bind_scope = Some(scope);
            let snapshot = guard.persisted.clone();
            drop(guard);
            write_persisted(&self.dir, &snapshot)?;
            log::info!(
                "[Remote] bind scope tightened to {scope:?} (was {current:?}); applied immediately"
            );
            return Ok(BindScopeRequest::Applied(scope));
        }

        let confirmation_id = uuid::Uuid::new_v4().to_string();
        guard.pending.insert(
            confirmation_id.clone(),
            PendingRequest {
                scope,
                requested_at: now,
                identity,
            },
        );
        let ttl = guard.ttl;
        drop(guard);

        log::info!(
            "[Remote] bind scope widening requested: {current:?} -> {scope:?} \
             (confirmation={confirmation_id}); awaiting confirmation"
        );
        Ok(BindScopeRequest::Pending {
            confirmation_id,
            expires_at_unix: unix_now() + ttl.as_secs() as i64,
        })
    }

    /// 第二段：兑现一次放宽申请。
    ///
    /// 四条判据缺一不可：
    ///
    /// 1. **id 存在**（不存在 / 已用过 / 已过期都是同一个出口）；
    /// 2. **未过期**（`now - requested_at <= ttl`）；
    /// 3. **同一会话** —— 发起方与确认方必须是同一个已认证身份。REST 侧的身份
    ///    由鉴权中间件从凭据摘要换出（[`SessionIdentity::Credential`]），桌面侧
    ///    固定为 [`SessionIdentity::LocalWindow`]；
    /// 4. **一次性** —— 成功或过期都会把记录从表里摘掉，重复确认自然走判据 1。
    ///
    /// 身份不匹配时**不消费**这条记录：一个拿错 id 的会话不该替真正的主人
    /// 把申请作废（那会把两段式变成一条 DoS）。
    pub fn confirm_bind_scope_change(
        &self,
        confirmation_id: &str,
        identity: SessionIdentity,
        now: Instant,
    ) -> Result<BindScope, String> {
        let mut guard = lock(&self.inner);
        let ttl = guard.ttl;
        let Some(request) = guard.pending.get(confirmation_id) else {
            return Err("unknown, expired, or already confirmed bind-scope request".to_string());
        };
        if !request.identity.same_session(&identity) {
            return Err("unknown, expired, or already confirmed bind-scope request".to_string());
        }
        if now.saturating_duration_since(request.requested_at) > ttl {
            guard.pending.remove(confirmation_id);
            return Err("unknown, expired, or already confirmed bind-scope request".to_string());
        }

        // 一次性：先摘掉再兑现。即便后面的落盘失败，这个 id 也不能再用。
        let request = guard
            .pending
            .remove(confirmation_id)
            .expect("上面刚确认过 it exists");
        guard.persisted.bind_scope = Some(request.scope);
        let snapshot = guard.persisted.clone();
        drop(guard);

        write_persisted(&self.dir, &snapshot)?;
        log::info!(
            "[Remote] bind scope confirmed: {:?} (confirmation={confirmation_id})",
            request.scope
        );
        Ok(request.scope)
    }

    /// 拒绝一次放宽申请，并把记录摘掉。同样要求同一会话。
    pub fn reject_bind_scope_change(
        &self,
        confirmation_id: &str,
        identity: SessionIdentity,
        now: Instant,
    ) -> Result<(), String> {
        let mut guard = lock(&self.inner);
        let ttl = guard.ttl;
        let Some(request) = guard.pending.get(confirmation_id) else {
            return Err("unknown, expired, or already confirmed bind-scope request".to_string());
        };
        if !request.identity.same_session(&identity) {
            return Err("unknown, expired, or already confirmed bind-scope request".to_string());
        }
        let expired = now.saturating_duration_since(request.requested_at) > ttl;
        guard.pending.remove(confirmation_id);
        if expired {
            return Err("unknown, expired, or already confirmed bind-scope request".to_string());
        }
        log::info!("[Remote] bind scope request rejected (confirmation={confirmation_id})");
        Ok(())
    }

    /// 旧设置迁移：`remote_tailscale_enabled == true` 且 `exposure.json` 里
    /// **没有显式档位**时，按 [`BindScope::Tailscale`] 处理并落盘。
    ///
    /// 一旦文件里有了显式档位（包括用户主动选回 `Loopback`），旧旗标不再翻案。
    /// 返回迁移后的生效档位。
    pub fn migrate_legacy_tailscale_flag(&self, legacy_tailscale_enabled: bool) -> BindScope {
        let mut guard = lock(&self.inner);
        if guard.persisted.bind_scope.is_none() && legacy_tailscale_enabled {
            guard.persisted.bind_scope = Some(BindScope::Tailscale);
            let snapshot = guard.persisted.clone();
            drop(guard);
            if let Err(e) = write_persisted(&self.dir, &snapshot) {
                log::warn!("[Remote] failed to persist legacy Tailscale migration: {e}");
            } else {
                log::info!(
                    "[Remote] migrated legacy remote_tailscale_enabled=true to bind scope Tailscale"
                );
            }
            return BindScope::Tailscale;
        }
        guard.persisted.bind_scope.unwrap_or_default()
    }

    /// 设置 CORS 模式与白名单。**不含绑定范围** —— 那条路只有两段式。
    pub fn set_cors(&self, cors_mode: CorsMode, origin_whitelist: &[String]) -> Result<(), String> {
        validate_whitelist(origin_whitelist)?;
        let mut guard = lock(&self.inner);
        guard.persisted.cors_mode = cors_mode;
        guard.persisted.origin_whitelist = origin_whitelist.to_vec();
        let snapshot = guard.persisted.clone();
        drop(guard);
        write_persisted(&self.dir, &snapshot)
    }
}

/// 丢掉过期的待确认记录，让 `pending` 不随点火次数无限增长。
fn purge_expired(pending: &mut HashMap<String, PendingRequest>, ttl: &Duration, now: Instant) {
    pending.retain(|_, request| now.saturating_duration_since(request.requested_at) <= *ttl);
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn read_persisted(path: &Path) -> Option<Persisted> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    // 旧版本的线上值 `"lan"` 在新枚举里不存在。这里做一次**读侧**迁移：
    // 旧文件里的 `lan` 语义就是「回环 + Tailscale」，对应新档位 `tailscale`。
    // 序列化永远只写新名字（见 `Persisted` 的 serde 派生）。
    if value.get("bindScope").and_then(serde_json::Value::as_str) == Some("lan") {
        value["bindScope"] = serde_json::Value::String("tailscale".to_string());
    }
    let persisted: Persisted = serde_json::from_value(value).ok()?;
    // 读回来的白名单同样要过一遍判据：文件是上一版程序或手改出来的，
    // 不能因为「已经落盘了」就绕开 `Origin: null` 这条。
    validate_whitelist(&persisted.origin_whitelist).ok()?;
    Some(persisted)
}

fn write_persisted(dir: &Path, persisted: &Persisted) -> Result<(), String> {
    let path = exposure_file(dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(persisted).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    // temp + rename：写到一半断电不会留下半个 JSON，下次启动读不出就退回默认值。
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// 启动路径（`lib.rs`）使用的配置构造。
///
/// 单独成函数是为了让「启动时必须读 `exposure.json`」这条契约可以被测试直接
/// 调到 —— 应用启动本身需要一个 Tauri event loop，测试里起不来。
pub fn startup_remote_config(
    config_dir: &Path,
    enabled: bool,
    port: u16,
    legacy_tailscale_enabled: bool,
) -> RemoteConfig {
    let gate = BindScopeGate::load(config_dir.to_path_buf());
    gate.migrate_legacy_tailscale_flag(legacy_tailscale_enabled);
    gate.overlay(&RemoteConfig {
        enabled,
        port,
        tailscale_enabled: legacy_tailscale_enabled,
        ..RemoteConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::auth::{ClientDigest, SessionIdentity};
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn cred(byte: u8) -> SessionIdentity {
        SessionIdentity::Credential(ClientDigest::new([byte; 32]))
    }

    /// `overlay` 只覆盖暴露边界三项，端口 / enabled 来自调用方的真实配置。
    ///
    /// 这一条钉的是 `get_remote_config` 曾经永远返回 port 4000 的回归：
    /// 用 `..RemoteConfig::default()` 拼快照，会把真实端口抹成默认值。
    #[test]
    fn overlay_preserves_real_port_and_enabled_from_the_base() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());
        let base = RemoteConfig {
            enabled: false,
            port: 4567,
            ..RemoteConfig::default()
        };
        let cfg = gate.overlay(&base);
        assert_eq!(cfg.port, 4567, "端口必须来自 base，而不是默认 4000");
        assert!(!cfg.enabled, "enabled 必须来自 base");
        assert_eq!(cfg.bind_scope, BindScope::Loopback);
    }

    /// 任何 origin 的取值在**枚举层面**就不存在。
    #[test]
    fn cors_mode_has_no_wildcard_variant() {
        assert_eq!(
            CorsMode::ALL.len(),
            2,
            "CorsMode 的取值集合必须恰为 {{SameOrigin, ExplicitWhitelist}}，\
             多出来的每一个都是一条「放宽到任意 origin」的路"
        );
        assert!(CorsMode::ALL.contains(&CorsMode::SameOrigin));
        assert!(CorsMode::ALL.contains(&CorsMode::ExplicitWhitelist));

        // 线上的名字也一样只有两个：反序列化是唯一能让「配置文件里塞一个通配值」
        // 进到程序里的入口，它认不出来才算真的堵住。
        assert_eq!(
            serde_json::from_value::<CorsMode>(json!("sameOrigin")).unwrap(),
            CorsMode::SameOrigin
        );
        assert_eq!(
            serde_json::from_value::<CorsMode>(json!("explicitWhitelist")).unwrap(),
            CorsMode::ExplicitWhitelist
        );
        for loose in ["any", "anyOrigin", "wildcard", "*", "All"] {
            assert!(
                serde_json::from_value::<CorsMode>(json!(loose)).is_err(),
                "CorsMode 反序列化不得接受 {loose}：本枚举里不存在「任意 origin」"
            );
        }

        // 默认收紧。
        assert_eq!(CorsMode::default(), CorsMode::SameOrigin);
    }

    /// 三档的顺序就是「变宽 / 变窄」的判据。
    #[test]
    fn bind_scope_tiers_are_ordered_and_serialize_as_camel_case() {
        assert!(BindScope::Tailscale.widens_from(BindScope::Loopback));
        assert!(BindScope::AllInterfaces.widens_from(BindScope::Loopback));
        assert!(BindScope::AllInterfaces.widens_from(BindScope::Tailscale));
        assert!(!BindScope::Loopback.widens_from(BindScope::Tailscale));
        assert!(!BindScope::Tailscale.widens_from(BindScope::Tailscale));

        assert_eq!(
            serde_json::to_value(BindScope::Loopback).unwrap(),
            json!("loopback")
        );
        assert_eq!(
            serde_json::to_value(BindScope::Tailscale).unwrap(),
            json!("tailscale")
        );
        assert_eq!(
            serde_json::to_value(BindScope::AllInterfaces).unwrap(),
            json!("allInterfaces")
        );
        for wire in ["loopback", "tailscale", "allInterfaces"] {
            assert_eq!(
                serde_json::from_value::<BindScope>(json!(wire)).unwrap(),
                match wire {
                    "loopback" => BindScope::Loopback,
                    "tailscale" => BindScope::Tailscale,
                    _ => BindScope::AllInterfaces,
                }
            );
        }
        // 旧名字不再被枚举接受（读侧迁移在 `read_persisted`，不是这里）。
        for loose in ["lan", "all", "any", "*", "0.0.0.0"] {
            assert!(
                serde_json::from_value::<BindScope>(json!(loose)).is_err(),
                "BindScope 反序列化不得接受 {loose:?}"
            );
        }
        assert_eq!(BindScope::default(), BindScope::Loopback);
    }

    /// 放宽：申请不落盘；确认之后才落盘，且 id 一次性。
    #[test]
    fn widening_requires_confirmation_and_ids_are_single_use() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());
        let me = cred(1);

        let request = gate
            .request_bind_scope_change(BindScope::AllInterfaces, me)
            .expect("request must be accepted");
        let BindScopeRequest::Pending {
            confirmation_id,
            expires_at_unix,
        } = request
        else {
            panic!("放宽必须返回待确认请求，不能立即生效");
        };
        assert!(!confirmation_id.is_empty());
        assert!(expires_at_unix > 0);

        // 只申请：配置不变、没有落盘。
        assert_eq!(
            gate.read_remote_config().bind_scope,
            BindScope::Loopback,
            "申请不得直接改配置"
        );
        assert!(
            !exposure_file(dir.path()).exists(),
            "申请不得落盘：落盘必须发生在确认之后"
        );

        // 别人的 id / 猜的 id 同样不得改配置。
        assert!(gate
            .confirm_bind_scope_change("not-a-real-confirmation-id", me, Instant::now())
            .is_err());
        assert_eq!(gate.read_remote_config().bind_scope, BindScope::Loopback);

        // 同一个会话确认之后才真正落盘。
        assert_eq!(
            gate.confirm_bind_scope_change(&confirmation_id, me, Instant::now())
                .expect("确认必须兑现"),
            BindScope::AllInterfaces
        );
        assert!(exposure_file(dir.path()).exists(), "确认之后必须落盘");
        assert_eq!(
            BindScopeGate::load(dir.path())
                .read_remote_config()
                .bind_scope,
            BindScope::AllInterfaces,
            "落盘是真的，不是内存里那份"
        );

        // 同一个 id 只能兑现一次。
        assert!(
            gate.confirm_bind_scope_change(&confirmation_id, me, Instant::now())
                .is_err(),
            "重复确认必须被拒"
        );
    }

    /// 收紧立即生效，且不需要确认。
    #[test]
    fn narrowing_applies_immediately() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());
        let me = cred(2);
        // 先把当前档位放到最宽。
        gate.request_bind_scope_change(BindScope::AllInterfaces, me)
            .expect("request");
        let id = match gate
            .request_bind_scope_change(BindScope::Loopback, me)
            .expect("rolling back to loopback is a narrowing")
        {
            BindScopeRequest::Applied(scope) => scope,
            other => panic!("收紧必须立即生效，实际 {other:?}"),
        };
        assert_eq!(id, BindScope::Loopback);

        let request = gate
            .request_bind_scope_change(BindScope::AllInterfaces, me)
            .expect("request widening again");
        let confirmation_id = match request {
            BindScopeRequest::Pending {
                confirmation_id, ..
            } => confirmation_id,
            other => panic!("expected pending, got {other:?}"),
        };
        gate.confirm_bind_scope_change(&confirmation_id, me, Instant::now())
            .expect("confirm");

        // AllInterfaces -> Tailscale 是收紧：立即生效，且不产生 pending。
        let request = gate
            .request_bind_scope_change(BindScope::Tailscale, me)
            .expect("narrowing from all interfaces");
        assert_eq!(request, BindScopeRequest::Applied(BindScope::Tailscale));
        assert_eq!(gate.read_remote_config().bind_scope, BindScope::Tailscale);
        assert!(
            exposure_file(dir.path()).exists(),
            "收紧同样要落盘（它也是配置变更）"
        );
    }

    /// 过期：用注入的 `now` 验证，不真的 sleep 五分钟。
    #[test]
    fn expired_requests_cannot_be_confirmed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());
        let me = cred(3);
        let now = Instant::now();

        let BindScopeRequest::Pending {
            confirmation_id, ..
        } = gate
            .request_bind_scope_change_at(BindScope::Tailscale, me, now)
            .expect("request")
        else {
            panic!("widening must be pending");
        };

        let late = now + BIND_SCOPE_CONFIRM_TTL + Duration::from_secs(1);
        assert!(
            gate.confirm_bind_scope_change(&confirmation_id, me, late)
                .is_err(),
            "过期之后确认必须被拒"
        );
        assert_eq!(gate.read_remote_config().bind_scope, BindScope::Loopback);
        // 过期记录被收尸：再用同一个 id 仍是拒绝。
        assert!(gate
            .confirm_bind_scope_change(&confirmation_id, me, late)
            .is_err());
    }

    /// 跨会话：A 申请、B 确认必须被拒，且不能把 A 的申请消耗掉。
    #[test]
    fn a_different_session_cannot_confirm_or_reject() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());
        let alice = cred(4);
        let bob = cred(5);

        let BindScopeRequest::Pending {
            confirmation_id, ..
        } = gate
            .request_bind_scope_change(BindScope::Tailscale, alice)
            .expect("alice request")
        else {
            panic!("widening must be pending");
        };

        assert!(
            gate.confirm_bind_scope_change(&confirmation_id, bob, Instant::now())
                .is_err(),
            "B 会话不得确认 A 的申请"
        );
        assert!(
            gate.reject_bind_scope_change(&confirmation_id, bob, Instant::now())
                .is_err(),
            "B 会话也不得撤销 A 的申请"
        );
        // A 自己仍然可以确认 —— 错 id 的会话没有替主人作废这条申请。
        assert_eq!(
            gate.confirm_bind_scope_change(&confirmation_id, alice, Instant::now())
                .expect("alice can still confirm"),
            BindScope::Tailscale
        );
    }

    /// 桌面窗口与远程凭据不是同一个会话身份。
    #[test]
    fn local_window_and_remote_credentials_are_distinct_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());
        let BindScopeRequest::Pending {
            confirmation_id, ..
        } = gate
            .request_bind_scope_change(BindScope::Tailscale, SessionIdentity::LocalWindow)
            .expect("request")
        else {
            panic!("widening must be pending");
        };
        assert!(gate
            .confirm_bind_scope_change(&confirmation_id, cred(9), Instant::now())
            .is_err());
        assert!(gate
            .confirm_bind_scope_change(
                &confirmation_id,
                SessionIdentity::LocalLoopback,
                Instant::now()
            )
            .is_err());
        assert!(gate
            .confirm_bind_scope_change(
                &confirmation_id,
                SessionIdentity::LocalWindow,
                Instant::now()
            )
            .is_ok());
    }

    /// 拒绝能把 pending 撤销，撤销之后同样不能再确认。
    #[test]
    fn reject_removes_the_pending_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());
        let me = cred(6);
        let BindScopeRequest::Pending {
            confirmation_id, ..
        } = gate
            .request_bind_scope_change(BindScope::Tailscale, me)
            .expect("request")
        else {
            panic!("widening must be pending");
        };
        gate.reject_bind_scope_change(&confirmation_id, me, Instant::now())
            .expect("reject");
        assert!(gate
            .confirm_bind_scope_change(&confirmation_id, me, Instant::now())
            .is_err());
        assert_eq!(gate.read_remote_config().bind_scope, BindScope::Loopback);
    }

    /// 旧设置迁移：只在「没有显式档位」时生效。
    #[test]
    fn legacy_tailscale_flag_migrates_only_without_an_explicit_scope() {
        let dir = tempfile::tempdir().expect("tempdir");

        // 没有 exposure.json + 旧旗标 true → Tailscale，并落盘为显式档位。
        let cfg = startup_remote_config(dir.path(), true, 4567, true);
        assert_eq!(cfg.bind_scope, BindScope::Tailscale);
        assert_eq!(cfg.port, 4567, "端口必须来自真实设置，不是默认 4000");
        let persisted: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(exposure_file(dir.path())).expect("migration must persist"),
        )
        .expect("json");
        assert_eq!(persisted["bindScope"], json!("tailscale"));

        // 显式档位（这里先收紧回 Loopback）+ 旧旗标 true → Loopback 获胜。
        let gate = BindScopeGate::load(dir.path());
        gate.request_bind_scope_change(BindScope::Loopback, cred(7))
            .expect("narrowing");
        let cfg = startup_remote_config(dir.path(), true, 4567, true);
        assert_eq!(
            cfg.bind_scope,
            BindScope::Loopback,
            "显式档位不得被旧旗标翻案"
        );

        // 旧旗标 false + 没有文件 → Loopback。
        let empty = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            startup_remote_config(empty.path(), true, 4000, false).bind_scope,
            BindScope::Loopback
        );
    }

    /// 旧文件里的 `"lan"` 在读取时迁移成 `"tailscale"`，同时保住 CORS 配置。
    #[test]
    fn legacy_lan_scope_is_migrated_on_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            exposure_file(dir.path()),
            r#"{"bindScope":"lan","corsMode":"explicitWhitelist","originWhitelist":["https://ok.example"]}"#,
        )
        .unwrap();
        let cfg = BindScopeGate::load(dir.path()).read_remote_config();
        assert_eq!(cfg.bind_scope, BindScope::Tailscale);
        assert_eq!(cfg.cors_mode, CorsMode::ExplicitWhitelist);
        assert_eq!(cfg.origin_whitelist, vec!["https://ok.example".to_string()]);
    }

    /// `set_cors` 不写绑定范围：没显式选过档位的文件里不应凭空多出 `bindScope`。
    #[test]
    fn set_cors_does_not_materialize_a_bind_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());
        gate.set_cors(CorsMode::SameOrigin, &[]).expect("set cors");
        let raw = std::fs::read_to_string(exposure_file(dir.path())).unwrap();
        assert!(
            !raw.contains("bindScope"),
            "只写 CORS 时不得顺手把档位写成显式值：{raw}"
        );
        let persisted: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(persisted.get("bindScope").is_none());
    }

    /// 白名单里不得出现 `Origin: null`。
    #[test]
    fn whitelist_rejects_null_origin() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gate = BindScopeGate::new(dir.path());

        for null_origin in ["null", "NULL", "Null", " null", "null "] {
            assert!(
                validate_whitelist(&[null_origin.to_string()]).is_err(),
                "白名单不得接受 {null_origin:?}：sandbox / file: / data: 页面发出的 \
                 Origin: null 不是可归属的来源"
            );
        }
        assert!(gate
            .set_cors(CorsMode::ExplicitWhitelist, &["null".to_string()])
            .is_err());
        assert_eq!(gate.read_remote_config().cors_mode, CorsMode::SameOrigin);
        assert!(gate.read_remote_config().origin_whitelist.is_empty());

        // 通配同样不接受：它就是「任意 origin」另一种写法。
        assert!(validate_whitelist(&["*".to_string()]).is_err());

        // 合法的显式白名单能进，并跟着落盘。
        let allowed = vec![
            "http://127.0.0.1:4000".to_string(),
            "https://desk.example.invalid".to_string(),
        ];
        gate.set_cors(CorsMode::ExplicitWhitelist, &allowed)
            .expect("显式白名单应当被接受");
        assert_eq!(
            gate.read_remote_config().cors_mode,
            CorsMode::ExplicitWhitelist
        );
        assert_eq!(gate.read_remote_config().origin_whitelist, allowed);
        assert_eq!(
            BindScopeGate::load(dir.path())
                .read_remote_config()
                .origin_whitelist,
            allowed
        );

        // 白名单里混进一个 null，整份一起拒，不做部分写入。
        let mut mixed = allowed.clone();
        mixed.push("null".to_string());
        assert!(gate.set_cors(CorsMode::ExplicitWhitelist, &mixed).is_err());
        assert_eq!(
            gate.read_remote_config().origin_whitelist,
            allowed,
            "被拒的白名单不得留下部分写入"
        );
    }
}

//! 每个写入目标的最近结果（设备本地，`app-targets-state.json`，不同步）。
//!
//! 主环境的「当前供应商」仍由现有指针负责；这里只记跟随环境写到了哪家、什么时候、
//! 托管字段的指纹（用来发现外部修改）以及失败原因。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetStatus {
    /// 已同步。
    Ok,
    /// 供应商保持可用，但 MCP / 提示词 / Skills 有未完成的同步。
    Partial,
    /// 未同步（写入失败、超时）。
    Error,
    /// 待同步（用户暂缓、主环境失败本次未开始、路由模式下暂不写）。
    Deferred,
    /// 保持原配置（托管登录只写 Windows 主环境）。
    Skipped,
    /// 外部已修改（托管字段和上次写入不同）。
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetRecord {
    pub status: TargetStatus,
    /// 原因代码（前端翻译），比如 `primary_failed`、`managed_login`、`timeout`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 失败时的错误详情（不含凭据）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 最近一次成功写进这个目标的供应商。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// 写入后托管字段的指纹。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// 这个环境现在接在共用的路由上（客户端文件是代理契约）。
    #[serde(default, skip_serializing_if = "is_false")]
    pub routed: bool,
    pub at: i64,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl TargetRecord {
    pub fn new(status: TargetStatus) -> Self {
        Self {
            status,
            reason: None,
            detail: None,
            provider_id: None,
            fingerprint: None,
            dir: None,
            warnings: Vec::new(),
            routed: false,
            at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn reason(mut self, reason: &str) -> Self {
        self.reason = Some(reason.to_string());
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// 保留供应商 / 路由错误；共享内容的失败独立可见，不能被下一次局部同步隐藏。
    pub fn with_sync_warnings(mut self, warnings: Vec<String>) -> Self {
        let failed = warnings.iter().any(|w| is_sync_failure(w));
        self.warnings = warnings;
        if failed && self.status != TargetStatus::Error {
            self.status = TargetStatus::Partial;
            self.reason = Some("partial_sync".to_string());
        } else if !failed && self.status == TargetStatus::Partial {
            // 累加式应用（OpenCode）没有 `provider_id`，同步成功后会有 `fingerprint`；
            // 回到 `Ok` 而非 `Skipped("no_provider")`。
            if self.provider_id.is_some() || self.routed || self.fingerprint.is_some() {
                self.status = TargetStatus::Ok;
                self.reason = self.routed.then(|| "routed".to_string());
            } else {
                self.status = TargetStatus::Skipped;
                self.reason = Some("no_provider".to_string());
            }
            self.detail = None;
        }
        self
    }
}

pub fn is_sync_failure(warning: &str) -> bool {
    ["mcp_failed:", "prompt_failed:", "skills_failed:"]
        .iter()
        .any(|prefix| warning.starts_with(prefix))
}

type AppRecords = BTreeMap<String, TargetRecord>;

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    apps: BTreeMap<String, AppRecords>,
}

fn canonical_key(target: &str) -> String {
    target
        .strip_prefix("wsl:")
        .map(|distro| format!("wsl:{}", distro.to_ascii_lowercase()))
        .unwrap_or_else(|| target.to_string())
}

fn find_key<'a>(records: &'a AppRecords, target: &str) -> Option<&'a str> {
    if records.contains_key(target) {
        return records.get_key_value(target).map(|(k, _)| k.as_str());
    }
    let want = canonical_key(target);
    records
        .keys()
        .find(|k| canonical_key(k) == want)
        .map(String::as_str)
}

fn take_key(records: &mut AppRecords, target: &str) -> Option<TargetRecord> {
    let want = canonical_key(target);
    let keys: Vec<String> = records
        .keys()
        .filter(|k| canonical_key(k) == want)
        .cloned()
        .collect();
    let mut found = None;
    for k in keys {
        let removed = records.remove(&k);
        if found.is_none() || k == target {
            found = removed;
        }
    }
    found
}

fn path() -> PathBuf {
    crate::config::get_home_dir()
        .join(".cc-switch")
        .join("app-targets-state.json")
}

fn lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

fn read() -> StateFile {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write(state: &StateFile) {
    match serde_json::to_vec_pretty(state) {
        Ok(bytes) => {
            if let Err(err) = crate::config::atomic_write_private(&path(), &bytes) {
                log::warn!("保存写入目标状态失败: {err}");
            }
        }
        Err(err) => log::warn!("序列化写入目标状态失败: {err}"),
    }
}

pub fn records(app: &str) -> AppRecords {
    let _guard = lock().lock().unwrap_or_else(|e| e.into_inner());
    read().apps.remove(app).unwrap_or_default()
}

pub fn get(app: &str, target: &str) -> Option<TargetRecord> {
    let mut all = records(app);
    find_key(&all, target)
        .map(str::to_string)
        .and_then(|k| all.remove(&k))
}

/// 记下一个目标的结果。没写供应商就沿用旧记录的（失败不改变「这个目标上次写到了哪家」）。
pub fn set(app: &str, target: &str, mut record: TargetRecord) {
    let _guard = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read();
    let entry = state.apps.entry(app.to_string()).or_default();
    if let Some(previous) = take_key(entry, target) {
        if record.provider_id.is_none() {
            record.provider_id = previous.provider_id;
        }
        if record.fingerprint.is_none() && record.status != TargetStatus::Ok {
            record.fingerprint = previous.fingerprint;
        }
    }
    entry.insert(target.to_string(), record);
    write(&state);
}

/// 整条替换，不沿用旧记录的供应商（取消一个环境的供应商时用）。
pub fn replace(app: &str, target: &str, record: TargetRecord) {
    let _guard = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read();
    let entry = state.apps.entry(app.to_string()).or_default();
    let _ = take_key(entry, target);
    entry.insert(target.to_string(), record);
    write(&state);
}

/// 只改状态（比如发现外部修改），要求指纹与预期原状态仍一致才写入，防止覆盖并发写入的新结果。
pub fn mark(
    app: &str,
    target: &str,
    expected_fingerprint: &str,
    expected_status: TargetStatus,
    status: TargetStatus,
    reason: Option<&str>,
) {
    let _guard = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read();
    let Some(records) = state.apps.get_mut(app) else {
        return;
    };
    let Some(matched_key) = find_key(records, target).map(str::to_string) else {
        return;
    };
    if let Some(record) = records.get_mut(&matched_key) {
        if record.status != expected_status
            || record.fingerprint.as_deref() != Some(expected_fingerprint)
        {
            return;
        }
        if record.status == status && record.reason.as_deref() == reason {
            return;
        }
        record.status = status;
        record.reason = reason.map(str::to_string);
        write(&state);
    }
}

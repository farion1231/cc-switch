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
}

type AppRecords = BTreeMap<String, TargetRecord>;

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    apps: BTreeMap<String, AppRecords>,
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
    records(app).remove(target)
}

/// 记下一个目标的结果。`keep_provider` 为真时，新记录没写供应商就沿用旧记录的（失败
/// 不改变「这个目标上次写到了哪家」）。
pub fn set(app: &str, target: &str, mut record: TargetRecord) {
    let _guard = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read();
    let entry = state.apps.entry(app.to_string()).or_default();
    if let Some(previous) = entry.get(target) {
        if record.provider_id.is_none() {
            record.provider_id = previous.provider_id.clone();
        }
        if record.fingerprint.is_none() && record.status != TargetStatus::Ok {
            record.fingerprint = previous.fingerprint.clone();
        }
    }
    entry.insert(target.to_string(), record);
    write(&state);
}

/// 只改状态（比如发现外部修改），其余字段不动。
pub fn mark(app: &str, target: &str, status: TargetStatus, reason: Option<&str>) {
    let _guard = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read();
    if let Some(record) = state
        .apps
        .get_mut(app)
        .and_then(|records| records.get_mut(target))
    {
        if record.status == status && record.reason.as_deref() == reason {
            return;
        }
        record.status = status;
        record.reason = reason.map(str::to_string);
        write(&state);
    }
}

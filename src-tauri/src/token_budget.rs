//! Token Budget 领域类型
//!
//! 与 `database/dao/token_budget.rs` 和 `services/token_budget.rs` 共享。
//!
//! 设计要点：
//! - `scope` + `scope_value` 配合定位预算作用域；`global` 时 `scope_value=None`。
//! - `limit_tokens` 与 `limit_usd` 至少有一个 `Some`，两者都 `Some` 时先到为准。
//!   DAO 层不做强制约束（允许临时性"双未设置"中间态在内存中流转），但写入前
//!   service 层应校验并返回 `AppError`。
//! - `period_start_day`：monthly=1~28（避开 29/30/31 防月末漂移）；weekly=0..=6
//!   (0=Sun..6=Sat)；daily 一律忽略，约定为 1。

use serde::{Deserialize, Serialize};

/// 预算作用域
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetScope {
    /// 全局，跨所有 app/provider/model
    Global,
    /// 限定 app_type（claude / codex / gemini / opencode / hermes）
    App,
    /// 限定 provider_id（与 app_type 无关，按 id 唯一）
    Provider,
    /// 限定 model id（与 model_pricing.model_id 对齐）
    Model,
}

impl BudgetScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::App => "app",
            Self::Provider => "provider",
            Self::Model => "model",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "global" => Some(Self::Global),
            "app" => Some(Self::App),
            "provider" => Some(Self::Provider),
            "model" => Some(Self::Model),
            _ => None,
        }
    }
}

/// 预算周期
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetPeriod {
    Daily,
    Weekly,
    Monthly,
}

impl BudgetPeriod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "monthly" => Some(Self::Monthly),
            _ => None,
        }
    }
}

/// 持久化的预算定义
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBudget {
    pub id: String,
    pub name: String,
    pub scope: BudgetScope,
    /// `scope='global'` 时为 None；其它作用域必填。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_value: Option<String>,
    pub period: BudgetPeriod,
    /// monthly: 1~28；weekly: 0..=6 (Sun..Sat)；daily: 忽略（约定写 1）
    pub period_start_day: i32,
    /// 预设的 token 上限（按 real_total_tokens 计算，cache 归一化）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_tokens: Option<i64>,
    /// 预设的 USD 上限（成本四项之和）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_usd: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

fn default_true() -> bool {
    true
}

/// 创建预算的入参
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTokenBudgetInput {
    pub name: String,
    pub scope: BudgetScope,
    pub scope_value: Option<String>,
    pub period: BudgetPeriod,
    #[serde(default = "default_start_day")]
    pub period_start_day: i32,
    pub limit_tokens: Option<i64>,
    pub limit_usd: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_start_day() -> i32 {
    1
}

/// 更新预算的入参；所有字段都是 `Option`，None 表示"不改动"。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTokenBudgetInput {
    pub name: Option<String>,
    pub scope: Option<BudgetScope>,
    pub scope_value: Option<Option<String>>, // Some(None) 表示清空
    pub period: Option<BudgetPeriod>,
    pub period_start_day: Option<i32>,
    pub limit_tokens: Option<Option<i64>>,
    pub limit_usd: Option<Option<String>>,
    pub enabled: Option<bool>,
}

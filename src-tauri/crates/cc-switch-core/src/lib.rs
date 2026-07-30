//! 可被桌面端与临时 Agent 共同使用的无界面业务核心。
//!
//! 该边界禁止依赖 Tauri、托盘、窗口和代理服务器生命周期。Provider 状态将在下一阶段
//! 迁入此处；先建立独立包可以让依赖审计在业务迁移前持续生效。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use cc_switch_protocol::capabilities::{CommandCapabilityRegistry, RemoteCapabilityError};

mod error;
mod provider;
mod schema;
mod state;

pub use error::CoreError;
// Provider 的完整 DTO 与数据库服务统一从独立模块导出，RPC 与桌面适配层不得再维护私有 SQL。
pub use provider::{ProviderRecord, ProviderService, ProviderSortUpdate};
pub use schema::{SchemaError, DESKTOP_SCHEMA_VERSION};
pub use state::HeadlessState;

/// 协议握手的兼容导出；其值现在代表桌面规范数据库版本，不再维护 Agent 私有版本。
pub const SCHEMA_VERSION: i32 = DESKTOP_SCHEMA_VERSION;

/// 将稳定 RPC 命令名映射到无界面 Provider 服务。
///
/// 参数解析放在 core 而非 Agent 入口，保证进程内测试、桌面兼容层和 SSH 进程使用同一套
/// camelCase DTO 与错误码，不在各传输层复制业务分发规则。
pub fn dispatch_provider_command(
    state: &HeadlessState,
    command: &str,
    args: Value,
) -> Result<Value, ProviderCommandError> {
    CommandCapabilityRegistry::provider_phase().require(command)?;
    match command {
        "provider.list" => {
            let args: AppArgs = parse_args(args)?;
            to_value(ProviderService::list(state, &args.app)?)
        }
        "provider.current" => {
            let args: AppArgs = parse_args(args)?;
            to_value(ProviderService::current(state, &args.app)?)
        }
        "provider.add" => {
            let args: AddArgs = parse_args(args)?;
            to_value(ProviderService::add(
                state,
                &args.app,
                args.provider,
                args.add_to_live.unwrap_or(true),
            )?)
        }
        "provider.update" => {
            let args: UpdateArgs = parse_args(args)?;
            let original_id = args.original_id.unwrap_or_else(|| args.provider.id.clone());
            to_value(ProviderService::update(
                state,
                &args.app,
                &original_id,
                args.provider,
            )?)
        }
        "provider.delete" => {
            let args: IdArgs = parse_args(args)?;
            ProviderService::delete(state, &args.app, &args.id)?;
            Ok(Value::Bool(true))
        }
        "provider.switch" => {
            let args: IdArgs = parse_args(args)?;
            ProviderService::switch(state, &args.app, &args.id)?;
            Ok(serde_json::json!({ "warnings": [] }))
        }
        "provider.update_sort_order" => {
            let args: SortArgs = parse_args(args)?;
            ProviderService::update_sort_order(state, &args.app, &args.updates)?;
            Ok(Value::Bool(true))
        }
        _ => Err(ProviderCommandError::CommandNotExposed(command.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct AppArgs {
    app: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddArgs {
    app: String,
    provider: ProviderRecord,
    add_to_live: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateArgs {
    app: String,
    provider: ProviderRecord,
    original_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdArgs {
    app: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct SortArgs {
    app: String,
    updates: Vec<ProviderSortUpdate>,
}

fn parse_args<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, ProviderCommandError> {
    serde_json::from_value(args)
        .map_err(|error| ProviderCommandError::InvalidArgument(error.to_string()))
}

fn to_value<T: Serialize>(value: T) -> Result<Value, ProviderCommandError> {
    serde_json::to_value(value).map_err(ProviderCommandError::Serialization)
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderCommandError {
    #[error("远程命令未开放: {0}")]
    CommandNotExposed(String),
    #[error("远程命令参数无效: {0}")]
    InvalidArgument(String),
    #[error("远程业务执行失败: {0}")]
    Business(#[from] CoreError),
    #[error("远程结果序列化失败: {0}")]
    Serialization(serde_json::Error),
}

impl ProviderCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CommandNotExposed(_) => "COMMAND_NOT_EXPOSED",
            Self::InvalidArgument(_) => "INVALID_ARGUMENT",
            Self::Business(_) => "REMOTE_BUSINESS_ERROR",
            Self::Serialization(_) => "REMOTE_SERIALIZATION_ERROR",
        }
    }
}

impl From<RemoteCapabilityError> for ProviderCommandError {
    fn from(value: RemoteCapabilityError) -> Self {
        match value {
            RemoteCapabilityError::CommandNotExposed(command) => Self::CommandNotExposed(command),
        }
    }
}

use serde::{Deserialize, Serialize};
use serde_json::Value;

use cc_switch_protocol::capabilities::{CommandCapabilityRegistry, RemoteCapabilityError};

use crate::{CoreError, HeadlessState, ProviderRecord, ProviderService, ProviderSortUpdate};

/// 通用远程分发入口先经过协议白名单，再按领域委托；任何未迁移领域都显式失败。
pub fn dispatch_command(
    state: &HeadlessState,
    command: &str,
    args: Value,
) -> Result<Value, CommandError> {
    CommandCapabilityRegistry::remote_supported().require(command)?;
    if command.starts_with("provider.") {
        return dispatch_provider(state, command, args);
    }
    crate::usage::dispatch(state, command, args)
}

fn dispatch_provider(
    state: &HeadlessState,
    command: &str,
    args: Value,
) -> Result<Value, CommandError> {
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
            to_value(ProviderService::switch(state, &args.app, &args.id)?)
        }
        "provider.update_sort_order" => {
            let args: SortArgs = parse_args(args)?;
            ProviderService::update_sort_order(state, &args.app, &args.updates)?;
            Ok(Value::Bool(true))
        }
        _ => Err(CommandError::CommandNotExposed(command.to_string())),
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

fn parse_args<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, CommandError> {
    serde_json::from_value(args).map_err(|error| CommandError::InvalidArgument(error.to_string()))
}

fn to_value<T: Serialize>(value: T) -> Result<Value, CommandError> {
    serde_json::to_value(value).map_err(CommandError::Serialization)
}

/// Agent 与桌面远程网关共享的稳定命令错误；错误码不依赖自然语言消息解析。
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("远程命令未开放: {0}")]
    CommandNotExposed(String),
    #[error("远程能力尚不可用: {0}")]
    CapabilityUnavailable(String),
    #[error("远程命令参数无效: {0}")]
    InvalidArgument(String),
    #[error("远程业务执行失败: {0}")]
    Business(#[from] CoreError),
    #[error("远程结果序列化失败: {0}")]
    Serialization(serde_json::Error),
}

impl CommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CommandNotExposed(_) => "COMMAND_NOT_EXPOSED",
            Self::CapabilityUnavailable(_) => "CAPABILITY_UNAVAILABLE",
            Self::InvalidArgument(_) => "INVALID_ARGUMENT",
            Self::Business(error) => error.code(),
            Self::Serialization(_) => "REMOTE_SERIALIZATION_ERROR",
        }
    }
}

impl From<RemoteCapabilityError> for CommandError {
    fn from(value: RemoteCapabilityError) -> Self {
        match value {
            RemoteCapabilityError::CommandNotExposed(command) => Self::CommandNotExposed(command),
        }
    }
}

use std::path::PathBuf;

use crate::schema::SchemaError;

/// 无界面 Core 的稳定错误边界。
///
/// 该类型同时服务桌面适配层与远端 Agent；错误消息不得携带 Provider 配置正文或凭据，
/// 传输层后续只根据变体映射稳定错误码，不能解析自然语言文本。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("无界面状态锁已损坏")]
    StatePoisoned,
    #[error("不支持的应用类型: {0}")]
    UnsupportedApp(String),
    #[error("无界面模式尚未支持该应用的 live 写入: {0}")]
    LiveWriteUnsupported(String),
    #[error("目标平台不支持该能力: {0}")]
    CapabilityUnavailable(String),
    #[error("目标主机 live 配置写入失败: {0}")]
    LiveWriteFailed(String),
    #[error("供应商不存在: {0}")]
    ProviderNotFound(String),
    #[error("不能删除当前供应商")]
    CurrentProviderDeletion,
    #[error("当前阶段不支持修改供应商 ID")]
    ProviderIdChangeUnsupported,
    #[error("供应商配置无效: {0}")]
    InvalidProvider(String),
    #[error("Usage 查询范围无效: {0}")]
    InvalidUsageRange(String),
    #[error("Usage 定价输入无效: {0}")]
    InvalidPricing(String),
    #[error("Usage 脚本执行失败: {0}")]
    UsageScript(String),
    #[error("远程操作超时: {0}")]
    RemoteOperationTimeout(String),
    #[error("远程操作已取消")]
    RemoteOperationCancelled,
    #[error("远程业务执行失败: {0}")]
    RemoteBusiness(String),
    #[error("数据库正被其他 CC Switch 进程占用，请稍后重试")]
    DatabaseBusy,
    #[error("远程结果超过协议帧上限: actual={actual}, limit={limit}")]
    PayloadTooLarge { actual: usize, limit: usize },
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error("数据库操作失败: {0}")]
    Database(rusqlite::Error),
    #[error("JSON 操作失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("文件操作失败 {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl CoreError {
    /// 向 RPC/桌面适配层提供稳定错误码；传输层不得依赖可能调整的中文错误文本。
    pub fn code(&self) -> &'static str {
        match self {
            Self::StatePoisoned => "STATE_POISONED",
            Self::UnsupportedApp(_)
            | Self::InvalidProvider(_)
            | Self::InvalidUsageRange(_)
            | Self::InvalidPricing(_) => "INVALID_ARGUMENT",
            Self::LiveWriteUnsupported(_) | Self::CapabilityUnavailable(_) => {
                "CAPABILITY_UNAVAILABLE"
            }
            Self::LiveWriteFailed(_) => "LIVE_WRITE_FAILED",
            Self::ProviderNotFound(_) => "PROVIDER_NOT_FOUND",
            Self::CurrentProviderDeletion => "CURRENT_PROVIDER_DELETION",
            Self::ProviderIdChangeUnsupported => "PROVIDER_ID_CHANGE_UNSUPPORTED",
            Self::DatabaseBusy => "DATABASE_BUSY",
            Self::PayloadTooLarge { .. } => "PAYLOAD_TOO_LARGE",
            Self::UsageScript(_) => "REMOTE_BUSINESS_ERROR",
            Self::RemoteOperationTimeout(_) => "REMOTE_OPERATION_TIMEOUT",
            Self::RemoteOperationCancelled => "REMOTE_OPERATION_CANCELLED",
            Self::RemoteBusiness(_) => "REMOTE_BUSINESS_ERROR",
            Self::Schema(_) => "DATABASE_INCOMPATIBLE",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Json(_) => "SERIALIZATION_ERROR",
            Self::Io { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied => {
                "REMOTE_PERMISSION_DENIED"
            }
            Self::Io { .. } => "IO_ERROR",
        }
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(error: rusqlite::Error) -> Self {
        // busy 与 locked 都代表目标数据库暂时被占用；统一后上层可以安全提示重试，
        // 其余 SQLite 错误仍保留原始类型供日志和后续精确分类使用。
        if matches!(
            &error,
            rusqlite::Error::SqliteFailure(sqlite_error, _)
                if matches!(
                    sqlite_error.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                )
        ) {
            Self::DatabaseBusy
        } else {
            Self::Database(error)
        }
    }
}

//! Oh My Pi 会话用量导入。
//!
//! OMP 会话文件与 Pi 完全同构（见 `session_manager::providers::omp`），解析与
//! 落库复用方言化的 Pi 导入器，仅替换 app_type / data_source / 标识前缀。

use crate::database::Database;
use crate::error::AppError;
use crate::services::session_usage::SessionSyncResult;
use crate::services::session_usage_pi::{sync_usage_files, OMP_DIALECT};

pub fn sync_omp_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let files = crate::session_manager::providers::omp::session_files()
        .map_err(|error| AppError::Config(format!("无法发现 Oh My Pi 会话: {error}")))?;
    Ok(sync_usage_files(db, &OMP_DIALECT, &files))
}

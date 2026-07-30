//! 订阅额度历史命令（fork 附加功能）
//!
//! 前端的每小时额度探针把快照写进来，「额度趋势」图从这里读。存储细节见
//! `database::dao::quota_history`。

use crate::database::dao::quota_history::{QuotaHistoryRow, QuotaTierSample};
use crate::error::AppError;
use crate::store::AppState;
use tauri::State;

/// 记录一份额度快照；返回 false 表示没带来新信息（调用方可跳过缓存失效）
#[tauri::command]
pub fn record_quota_history(
    state: State<'_, AppState>,
    app_id: String,
    measured_at: i64,
    tiers: Vec<QuotaTierSample>,
) -> Result<bool, AppError> {
    state.db.record_quota_history(&app_id, measured_at, &tiers)
}

/// 查询 `[start_hour, end_hour]` 闭区间的额度历史；`app_id` 省略则返回全部应用
#[tauri::command]
pub fn get_quota_history(
    state: State<'_, AppState>,
    app_id: Option<String>,
    start_hour: i64,
    end_hour: i64,
) -> Result<Vec<QuotaHistoryRow>, AppError> {
    state
        .db
        .get_quota_history(app_id.as_deref(), start_hour, end_hour)
}

//! Claude Code 子代理编辑器的模型下拉选项。

use tauri::State;

use super::run_db_task;
use crate::services::subagent_models::{self, SubagentModelOptions};
use crate::store::AppState;

/// 要读供应商行（等数据库连接锁），放到阻塞线程池里跑，见 [`run_db_task`]。
#[tauri::command]
pub async fn get_subagent_model_options(
    state: State<'_, AppState>,
) -> Result<SubagentModelOptions, String> {
    run_db_task(&state, |state| {
        subagent_models::get_subagent_model_options(&state.db).map_err(|e| e.to_string())
    })
    .await
}

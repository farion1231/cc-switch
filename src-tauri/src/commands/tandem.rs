use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::tandem::{
    database::TandemDatabase,
    domain::NewTask,
    repository::{TaskLedger, TaskLedgerItem},
    TandemState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTandemTaskInput {
    pub project_name: String,
    pub project_root_path: String,
    pub title: String,
    pub original_instruction: String,
}

pub async fn create_tandem_task_impl(
    db: Arc<TandemDatabase>,
    input: CreateTandemTaskInput,
    now: i64,
) -> Result<TaskLedgerItem, String> {
    db.create_task(
        NewTask {
            project_name: input.project_name,
            project_root_path: input.project_root_path,
            title: input.title,
            original_instruction: input.original_instruction,
        },
        now,
    )
    .map_err(|error| error.to_string())
}

pub async fn list_tandem_ledger_impl(db: Arc<TandemDatabase>) -> Result<TaskLedger, String> {
    db.list_ledger().map_err(|error| error.to_string())
}

pub async fn confirm_tandem_task_completed_impl(
    db: Arc<TandemDatabase>,
    task_id: String,
    now: i64,
) -> Result<TaskLedgerItem, String> {
    db.confirm_task_completed(&task_id, now)
        .map_err(|error| error.to_string())
}

pub async fn create_tandem_task_with_state(
    state: &TandemState,
    input: CreateTandemTaskInput,
) -> Result<TaskLedgerItem, String> {
    let db = state.database()?;
    let now = state.now_ms();
    create_tandem_task_impl(db, input, now).await
}

pub async fn list_tandem_ledger_with_state(state: &TandemState) -> Result<TaskLedger, String> {
    list_tandem_ledger_impl(state.database()?).await
}

pub async fn confirm_tandem_task_completed_with_state(
    state: &TandemState,
    task_id: String,
) -> Result<TaskLedgerItem, String> {
    let db = state.database()?;
    let now = state.now_ms();
    confirm_tandem_task_completed_impl(db, task_id, now).await
}

#[tauri::command]
pub async fn create_tandem_task(
    app: tauri::AppHandle,
    state: State<'_, TandemState>,
    input: CreateTandemTaskInput,
) -> Result<TaskLedgerItem, String> {
    let item = create_tandem_task_with_state(&state, input).await?;
    crate::tray::schedule_tandem_tray_refresh(&app);
    Ok(item)
}

#[tauri::command]
pub async fn list_tandem_ledger(state: State<'_, TandemState>) -> Result<TaskLedger, String> {
    list_tandem_ledger_with_state(&state).await
}

#[tauri::command]
pub async fn confirm_tandem_task_completed(
    app: tauri::AppHandle,
    state: State<'_, TandemState>,
    task_id: String,
) -> Result<TaskLedgerItem, String> {
    let item = confirm_tandem_task_completed_with_state(&state, task_id).await?;
    crate::tray::schedule_tandem_tray_refresh(&app);
    Ok(item)
}

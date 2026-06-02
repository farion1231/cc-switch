//! Token Budget Tauri 命令
//!
//! 暴露给前端 invoke 的入口。仅做参数解构 + 转发到 `TokenBudgetService`，
//! 不在 command 层做业务判断（保持与 commands/usage.rs 等一致）。

use crate::error::AppError;
use crate::services::token_budget::{BudgetStatus, TokenBudgetService};
use crate::store::AppState;
use crate::token_budget::{CreateTokenBudgetInput, TokenBudget, UpdateTokenBudgetInput};
use tauri::State;

#[tauri::command]
pub fn list_token_budgets(state: State<'_, AppState>) -> Result<Vec<TokenBudget>, AppError> {
    TokenBudgetService::list(&state)
}

#[tauri::command]
pub fn create_token_budget(
    state: State<'_, AppState>,
    input: CreateTokenBudgetInput,
) -> Result<TokenBudget, AppError> {
    TokenBudgetService::create(&state, input)
}

#[tauri::command]
pub fn update_token_budget(
    state: State<'_, AppState>,
    id: String,
    patch: UpdateTokenBudgetInput,
) -> Result<TokenBudget, AppError> {
    TokenBudgetService::update(&state, &id, patch)
}

#[tauri::command]
pub fn delete_token_budget(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    TokenBudgetService::delete(&state, &id)
}

#[tauri::command]
pub fn get_token_budget_status(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<BudgetStatus>, AppError> {
    TokenBudgetService::get_status(&state, &id)
}

#[tauri::command]
pub fn get_all_token_budget_statuses(
    state: State<'_, AppState>,
) -> Result<Vec<BudgetStatus>, AppError> {
    TokenBudgetService::get_all_statuses(&state)
}

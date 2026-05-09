use crate::codex_state::{CodexStateDiagnosis, CodexStateRepairResult};

#[tauri::command]
pub fn diagnose_codex_state() -> Result<CodexStateDiagnosis, String> {
    crate::codex_state::diagnose_codex_state().map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn repair_codex_state(dry_run: bool) -> Result<CodexStateRepairResult, String> {
    crate::codex_state::repair_codex_state(dry_run).map_err(|e| e.to_string())
}

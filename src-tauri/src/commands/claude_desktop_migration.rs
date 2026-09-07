//! Tauri commands for the opt-in Claude Desktop 1P -> 3P data migration.
//!
//! These are thin wrappers around [`crate::claude_desktop_data_migration`]:
//! they resolve the default macOS application roots, enforce user consent and
//! the "Claude Desktop must be quit" precondition, and forward to the engine.
//! All heavy file work runs on the blocking thread pool.

use std::path::PathBuf;

use serde::Deserialize;

use crate::claude_desktop_data_migration as engine;
use crate::claude_desktop_data_migration::{
    MigrationApplyResult, MigrationAudit, MigrationPlan, MigrationRestoreResult,
    MigrationVerifyResult, PathMapping, RootOverrides,
};
use crate::config::get_home_dir;
use crate::error::AppError;

/// Optional root overrides + path mappings shared by plan/apply/verify.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRootsRequest {
    pub source_app: Option<String>,
    pub target_app: Option<String>,
    pub source_code_root: Option<String>,
    pub target_code_root: Option<String>,
    pub source_cowork_root: Option<String>,
    pub target_cowork_root: Option<String>,
    #[serde(default)]
    pub path_maps: Vec<PathMapping>,
}

/// Apply request: roots + component selection + explicit consent.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationApplyRequest {
    #[serde(flatten)]
    pub roots: MigrationRootsRequest,
    /// Components to migrate; empty means all supported components.
    #[serde(default)]
    pub components: Vec<String>,
    /// Must be true. The UI sets this only after the user reviews the plan and
    /// explicitly confirms; it is the API-level consent guard.
    #[serde(default)]
    pub confirmed: bool,
}

fn unsupported_platform() -> AppError {
    AppError::localized(
        "claude_desktop.migration_unsupported_platform",
        "当前平台暂不支持 Claude Desktop 数据迁移。第一阶段仅支持 macOS（也可以显式传入源/目标根目录用于测试）。",
        "Claude Desktop data migration is not supported on this platform yet. Phase 1 targets macOS (explicit source/target roots may be passed for testing).",
    )
}

/// Resolve (home, source_app, target_app) from optional overrides, falling back
/// to the default macOS roots.
fn resolve_apps(
    source_app: Option<&str>,
    target_app: Option<&str>,
) -> Result<(PathBuf, PathBuf, PathBuf), AppError> {
    let home = get_home_dir();
    let defaults = engine::default_app_roots(&home);
    let source = match (source_app, &defaults) {
        (Some(s), _) if !s.trim().is_empty() => PathBuf::from(s),
        (_, Some((d, _))) => d.clone(),
        _ => return Err(unsupported_platform()),
    };
    let target = match (target_app, &defaults) {
        (Some(t), _) if !t.trim().is_empty() => PathBuf::from(t),
        (_, Some((_, d))) => d.clone(),
        _ => return Err(unsupported_platform()),
    };
    Ok((home, source, target))
}

fn to_overrides(req: &MigrationRootsRequest) -> RootOverrides {
    RootOverrides {
        source_code: req.source_code_root.as_ref().map(PathBuf::from),
        target_code: req.target_code_root.as_ref().map(PathBuf::from),
        source_cowork: req.source_cowork_root.as_ref().map(PathBuf::from),
        target_cowork: req.target_cowork_root.as_ref().map(PathBuf::from),
    }
}

/// Read-only inventory of the 1P/3P roots: candidate account roots, counts,
/// sizes, deployment modes, and shared transcript/document locations.
#[tauri::command]
pub async fn audit_claude_desktop_data_migration(
    source_app: Option<String>,
    target_app: Option<String>,
) -> Result<MigrationAudit, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (home, source, target) = resolve_apps(source_app.as_deref(), target_app.as_deref())?;
        Ok(engine::build_audit(&home, &source, &target))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e: AppError| e.to_string())
}

/// Build a read-only migration plan for the user to review. Reports blocking
/// issues (ambiguous roots, missing target seed, invalid JSON) that must be
/// resolved before apply.
#[tauri::command]
pub async fn plan_claude_desktop_data_migration(
    request: MigrationRootsRequest,
) -> Result<MigrationPlan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (home, source, target) =
            resolve_apps(request.roots_source_app(), request.roots_target_app())?;
        engine::build_plan(
            &home,
            &source,
            &target,
            &to_overrides(&request),
            &request.path_maps,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e: AppError| e.to_string())
}

/// Apply a reviewed plan. Requires `confirmed: true` (explicit consent) and a
/// fully-quit Claude Desktop. Backs up the target account roots, stages and
/// verifies Cowork sessions, installs without overwriting, and writes a ledger
/// for later verify/restore.
#[tauri::command]
pub async fn apply_claude_desktop_data_migration(
    request: MigrationApplyRequest,
) -> Result<MigrationApplyResult, String> {
    if !request.confirmed {
        return Err(AppError::InvalidInput(
            "迁移需要明确的用户确认（confirmed: true）".to_string(),
        )
        .to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let (home, source, target) = resolve_apps(
            request.roots.source_app.as_deref(),
            request.roots.target_app.as_deref(),
        )?;
        let claude_running = engine::claude_desktop_process_running();
        engine::apply_migration(
            &home,
            &source,
            &target,
            &to_overrides(&request.roots),
            &request.roots.path_maps,
            &request.components,
            &engine::migration_backup_root(),
            claude_running,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e: AppError| e.to_string())
}

/// Read-only structural verification of the target after a migration.
#[tauri::command]
pub async fn verify_claude_desktop_data_migration(
    request: MigrationRootsRequest,
    components: Option<Vec<String>>,
) -> Result<MigrationVerifyResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (home, source, target) =
            resolve_apps(request.roots_source_app(), request.roots_target_app())?;
        engine::verify_migration(
            &home,
            &source,
            &target,
            &to_overrides(&request),
            &components.unwrap_or_default(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e: AppError| e.to_string())
}

/// Undo exactly what a migration installed, driven by its ledger. When
/// `ledger_path` is omitted, the most recent ledger for this migration is used.
/// Records created after the migration are never removed.
#[tauri::command]
pub async fn restore_claude_desktop_data_migration(
    ledger_path: Option<String>,
) -> Result<MigrationRestoreResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let backup_parent = engine::migration_backup_root();
        engine::restore_migration(
            ledger_path.as_deref().map(PathBuf::from).as_deref(),
            &backup_parent,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e: AppError| e.to_string())
}

impl MigrationRootsRequest {
    fn roots_source_app(&self) -> Option<&str> {
        self.source_app.as_deref()
    }
    fn roots_target_app(&self) -> Option<&str> {
        self.target_app.as_deref()
    }
}

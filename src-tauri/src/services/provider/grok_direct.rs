//! 写 Grok Build 的 `config.toml`：只替换 `models.default` 和 CC Switch 写的那张模型表，
//! 其余字节不碰。
//!
//! 写 Grok live 的入口（切换、新增第一个供应商、编辑当前供应商、同步、统一供应商、
//! 进入 / 退出代理）都走这里：先拿应用写锁，再经 `mode::operation` 记下 pending，文件、
//! 指针和写入记录在同一个操作里提交。不回填：用户的设置和 MCP 本来就留在 live 里。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::grok_config::get_grok_config_path;

use crate::live::engine::{DeviceStore, LiveFile};
use crate::live::patch::toml::{TomlDocPatch, TomlSteps};
use crate::live::project::claude::PROXY_TOKEN_PLACEHOLDER;
use crate::live::project::grok::{GrokConfigPatch, GrokProjection};
use crate::mode::operation::{AppWrite, FileChange, OperationReport};
use crate::mode::state::{self, op, PendingTarget, Written};
use crate::provider::Provider;

use super::editor_toml::TomlEdits;

fn app() -> &'static str {
    AppType::GrokBuild.as_str()
}

/// `~/.grok/config.toml`：模型表里有 Key，按 0600 写。
pub(crate) fn config_file() -> LiveFile {
    LiveFile::private(get_grok_config_path())
}

/// `~/.grok/auth.json`：存放 xAI OAuth 登录凭据，按 0600 写。
pub(crate) fn auth_file() -> LiveFile {
    LiveFile::private(crate::grok_config::get_grok_auth_path())
}

/// 官方卡：不写模型表，Grok 回落到内置模型和自带的 xAI 登录。
pub(crate) fn is_official(provider: &Provider) -> bool {
    provider.category.as_deref() == Some("official")
        || provider.id == crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID
}

/// 供应商要写的模型表。写之前校验，校验不过什么都不写。
pub(crate) fn projection(provider: &Provider) -> Result<GrokProjection, AppError> {
    GrokProjection::of(&provider.settings_config, is_official(provider))
}

/// 上次写进 live 的表。有写入记录就按记录；这台设备上新版还没写过（刚从旧版升级）时，
/// 按旧版的写法推断：旧版把 `live_owner` 的行整份写进 live，它的 `models.default` 指的就是
/// CC Switch 的表。用户的其他表旧版也会原样写回，但证明不了是 CC Switch 的，不删。
pub(crate) fn retired_tables(
    store: &DeviceStore,
    live_owner: Option<&Provider>,
) -> Result<Vec<String>, AppError> {
    if let Some(written) = state::written(store, app())? {
        return Ok(written.tables);
    }
    let Some(owner) = live_owner.filter(|owner| !is_official(owner)) else {
        return Ok(Vec::new());
    };
    if let Ok(projection) = projection(owner) {
        return Ok(projection.written_tables());
    }
    // 行本身过不了校验（比如旧版回填把默认模型改成了内置的），就按行里写的名字删。
    Ok(owner
        .settings_config
        .get("config")
        .and_then(|config| config.as_str())
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .and_then(|doc| {
            doc.get("models")
                .and_then(|models| models.get("default"))
                .and_then(toml_edit::Item::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
        .into_iter()
        .collect())
}

/// 切到 `target`：同一个操作里写 live、改指针、记下写的表。`live_owner` 是 live 现在
/// 对应的供应商（直连指针那家），只在没有写入记录时用来推断旧版写的表。
pub(crate) fn switch_to(
    db: &Database,
    live_owner: Option<&Provider>,
    target: &Provider,
    xai_manager: Option<
        &std::sync::Arc<
            tokio::sync::RwLock<crate::proxy::providers::xai_oauth_auth::XaiOAuthManager>,
        >,
    >,
) -> Result<OperationReport, AppError> {
    write(
        db,
        op::SWITCH,
        live_owner,
        target,
        Some(&target.id),
        xai_manager,
    )
}

/// 把当前供应商重新投影到 live，不改指针。
pub(crate) fn reapply(
    db: &Database,
    live_owner: Option<&Provider>,
    target: &Provider,
    xai_manager: Option<
        &std::sync::Arc<
            tokio::sync::RwLock<crate::proxy::providers::xai_oauth_auth::XaiOAuthManager>,
        >,
    >,
) -> Result<OperationReport, AppError> {
    write(db, op::APPLY, live_owner, target, None, xai_manager)
}

fn write(
    db: &Database,
    op: &str,
    live_owner: Option<&Provider>,
    target: &Provider,
    pointer: Option<&str>,
    xai_manager: Option<
        &std::sync::Arc<
            tokio::sync::RwLock<crate::proxy::providers::xai_oauth_auth::XaiOAuthManager>,
        >,
    >,
) -> Result<OperationReport, AppError> {
    let projection = projection(target)?;
    let mut auth_bytes: Option<Vec<u8>> = None;

    if is_official(target) {
        if let Some(xai_manager) = xai_manager {
            let auth_path = crate::grok_config::get_grok_auth_path();
            if auth_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&auth_path) {
                    let mgr = std::sync::Arc::clone(xai_manager);
                    let _ = std::thread::spawn(move || {
                        tauri::async_runtime::block_on(async move {
                            let manager = mgr.read().await;
                            let _ = manager.sync_live_auth_json_refresh(&content).await;
                        });
                    })
                    .join();
                }
            }

            if let Some(account_id) = target
                .meta
                .as_ref()
                .and_then(|m| m.managed_account_id_for("xai_oauth"))
            {
                let mgr = std::sync::Arc::clone(xai_manager);
                let acc_id = account_id.to_string();
                let creds_result = std::thread::spawn(move || {
                    tauri::async_runtime::block_on(async move {
                        let manager = mgr.read().await;
                        manager.get_auth_json_credentials_for_account(&acc_id).await
                    })
                })
                .join()
                .map_err(|_| AppError::Message("Failed to join OAuth task".to_string()))?;

                let creds = creds_result.map_err(|e| {
                    AppError::localized(
                        "provider.grokbuild.oauth.failed",
                        format!("获取 xAI 账号凭据失败: {e}"),
                        format!("Failed to get credentials for xAI account: {e}"),
                    )
                })?;

                let auth_json = crate::grok_config::build_grok_auth_json(
                    &creds.access_token,
                    &creds.refresh_token,
                    &creds.expires_at,
                    creds.email.as_deref(),
                    &creds.account_id,
                );
                let mut bytes = serde_json::to_vec_pretty(&auth_json)
                    .map_err(|e| AppError::Message(e.to_string()))?;
                bytes.push(b'\n');
                auth_bytes = Some(bytes);
            }
        }
    }

    run_with_edits_and_auth(
        db,
        op,
        live_owner,
        Some(&projection),
        PendingTarget::pointer(pointer.map(str::to_string)),
        None,
        auth_bytes,
    )
}

/// 把 `projection` 写进 `config.toml`，和 `target` 在同一个操作里提交，并记下写的表；
/// `projection` 为空时只落定状态、不读也不写文件。
pub(crate) fn run(
    db: &Database,
    op: &str,
    live_owner: Option<&Provider>,
    projection: Option<&GrokProjection>,
    target: PendingTarget,
) -> Result<OperationReport, AppError> {
    run_with_edits(db, op, live_owner, projection, target, None)
}

/// 同 [`run`]，另把编辑器保存的全局改动在同一次写入里写进去（先改动，后模型表）。
pub(crate) fn run_with_edits(
    db: &Database,
    op: &str,
    live_owner: Option<&Provider>,
    projection: Option<&GrokProjection>,
    target: PendingTarget,
    edits: Option<&TomlEdits>,
) -> Result<OperationReport, AppError> {
    run_with_edits_and_auth(db, op, live_owner, projection, target, edits, None)
}

pub(crate) fn run_with_edits_and_auth(
    db: &Database,
    op: &str,
    live_owner: Option<&Provider>,
    projection: Option<&GrokProjection>,
    mut target: PendingTarget,
    edits: Option<&TomlEdits>,
    auth_bytes: Option<Vec<u8>>,
) -> Result<OperationReport, AppError> {
    // 先补完上一次没做完的操作：它可能改了写入记录和指针，下面要按最新的记录删表。
    let app_write = AppWrite::begin(db, app())?;
    let patch = match projection {
        Some(projection) => {
            target.written = Some(Written {
                tables: projection.written_tables(),
                ..Written::default()
            });
            Some(GrokConfigPatch::direct(
                projection,
                retired_tables(&app_write.store, live_owner)?,
                PROXY_TOKEN_PLACEHOLDER,
            ))
        }
        None => None,
    };
    let edits = edits.filter(|edits| !edits.is_empty());
    // 先按三方比较应用编辑器的改动，再换模型表。
    let mut steps: Vec<&dyn TomlDocPatch> = Vec::new();
    if let Some(edits) = edits {
        steps.push(edits);
    }
    if let Some(patch) = &patch {
        steps.push(patch);
    }
    let write = TomlSteps(steps);
    let mut changes: Vec<FileChange<'_>> = Vec::new();
    if !write.0.is_empty() {
        changes.push(FileChange {
            file: config_file(),
            patch: &write,
        });
    }

    let auth_patch = auth_bytes.map(crate::live::patch::WholeFile::Write);
    if let Some(ref patch) = auth_patch {
        changes.push(FileChange {
            file: auth_file(),
            patch,
        });
    }

    app_write.run(op, &changes, target)
}

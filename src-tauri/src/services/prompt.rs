use indexmap::IndexMap;
use std::path::Path;

use crate::app_config::AppType;
use crate::config::write_text_file;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::prompt_files::prompt_file_path;
use crate::services::pi_prompt_files::PiAgentsFileGuard;
use crate::store::AppState;

/// 安全地获取当前 Unix 时间戳
fn get_unix_timestamp() -> Result<i64, AppError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| AppError::Message(format!("Failed to get system time: {e}")))
}

pub struct PromptService;

fn project_prompt_set_to_path(
    prompts: &IndexMap<String, Prompt>,
    target_path: &Path,
) -> Result<Option<String>, AppError> {
    let enabled: Vec<(&String, &Prompt)> = prompts
        .iter()
        .filter(|(_, prompt)| prompt.enabled)
        .collect();

    if let Some((_, prompt)) = enabled.first() {
        write_text_file(target_path, &prompt.content)?;
    }
    // With nothing enabled, leave the target file untouched. This projection
    // only runs after a database restore, and the live file is not part of
    // the sync payload — clearing it here would wipe local content the
    // restored snapshot never contained. Disabling the last prompt from the
    // UI still clears the file via `PromptService::upsert_prompt`.

    if enabled.len() <= 1 {
        return Ok(None);
    }

    let ids = enabled
        .iter()
        .map(|(id, _)| id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(Some(format!(
        "多个 Prompt 同时启用，已按稳定顺序投影第一个；enabled IDs: {ids}"
    )))
}

impl PromptService {
    pub fn get_prompts(
        state: &AppState,
        app: AppType,
    ) -> Result<IndexMap<String, Prompt>, AppError> {
        if matches!(app, AppType::Pi) {
            return get_pi_prompts(state);
        }
        state.db.get_prompts(app.as_str())
    }

    pub fn upsert_prompt(
        state: &AppState,
        app: AppType,
        id: &str,
        prompt: Prompt,
    ) -> Result<(), AppError> {
        if matches!(app, AppType::Pi) {
            return upsert_pi_prompt(state, id, prompt);
        }

        // 检查是否为已启用的提示词
        let is_enabled = prompt.enabled;

        state.db.save_prompt(app.as_str(), &prompt)?;

        if is_enabled {
            // 启用提示词：写入内容到文件
            let target_path = prompt_file_path(&app)?;
            write_text_file(&target_path, &prompt.content)?;
        } else {
            // 禁用提示词：检查是否还有其他已启用的提示词
            let prompts = state.db.get_prompts(app.as_str())?;
            let any_enabled = prompts.values().any(|p| p.enabled);

            if !any_enabled {
                // 所有提示词都已禁用，清空文件
                let target_path = prompt_file_path(&app)?;
                if target_path.exists() {
                    write_text_file(&target_path, "")?;
                }
            }
        }

        Ok(())
    }

    pub fn delete_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        if matches!(app, AppType::Pi) {
            return delete_pi_prompt(state, id);
        }
        let prompts = Self::get_prompts(state, app.clone())?;

        if let Some(prompt) = prompts.get(id) {
            if prompt.enabled {
                return Err(AppError::InvalidInput("无法删除已启用的提示词".to_string()));
            }
        }

        state.db.delete_prompt(app.as_str(), id)?;
        Ok(())
    }

    pub fn enable_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        if matches!(app, AppType::Pi) {
            return enable_pi_prompt(state, id);
        }

        // 回填当前 live 文件内容到已启用的提示词，或创建备份
        let target_path = prompt_file_path(&app)?;
        if target_path.exists() {
            if let Ok(live_content) = std::fs::read_to_string(&target_path) {
                if !live_content.trim().is_empty() {
                    let mut prompts = state.db.get_prompts(app.as_str())?;

                    // 尝试回填到当前已启用的提示词
                    if let Some((enabled_id, enabled_prompt)) = prompts
                        .iter_mut()
                        .find(|(_, p)| p.enabled)
                        .map(|(id, p)| (id.clone(), p))
                    {
                        let timestamp = get_unix_timestamp()?;
                        enabled_prompt.content = live_content.clone();
                        enabled_prompt.updated_at = Some(timestamp);
                        log::info!("回填 live 提示词内容到已启用项: {enabled_id}");
                        state.db.save_prompt(app.as_str(), enabled_prompt)?;
                    } else {
                        // 没有已启用的提示词，则创建一次备份（避免重复备份）
                        let content_exists = prompts
                            .values()
                            .any(|p| p.content.trim() == live_content.trim());
                        if !content_exists {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64;
                            let backup_id = format!("backup-{timestamp}");
                            let backup_prompt = Prompt {
                                id: backup_id.clone(),
                                name: format!(
                                    "原始提示词 {}",
                                    chrono::Local::now().format("%Y-%m-%d %H:%M")
                                ),
                                content: live_content,
                                description: Some("自动备份的原始提示词".to_string()),
                                enabled: false,
                                created_at: Some(timestamp),
                                updated_at: Some(timestamp),
                            };
                            log::info!("回填 live 提示词内容，创建备份: {backup_id}");
                            state.db.save_prompt(app.as_str(), &backup_prompt)?;
                        }
                    }
                }
            }
        }

        // 启用目标提示词并写入文件
        let mut prompts = state.db.get_prompts(app.as_str())?;

        for prompt in prompts.values_mut() {
            prompt.enabled = false;
        }

        if let Some(prompt) = prompts.get_mut(id) {
            prompt.enabled = true;
            write_text_file(&target_path, &prompt.content)?; // 原子写入
            state.db.save_prompt(app.as_str(), prompt)?;
        } else {
            return Err(AppError::InvalidInput(format!("提示词 {id} 不存在")));
        }

        // Save all prompts to disable others
        for (_, prompt) in prompts.iter() {
            state.db.save_prompt(app.as_str(), prompt)?;
        }

        Ok(())
    }

    pub fn import_from_file(state: &AppState, app: AppType) -> Result<String, AppError> {
        let content = if matches!(app, AppType::Pi) {
            PiAgentsFileGuard::acquire()?
                .read()?
                .content
                .ok_or_else(|| AppError::Message("提示词文件不存在".to_string()))?
        } else {
            let file_path = prompt_file_path(&app)?;
            if !file_path.exists() {
                return Err(AppError::Message("提示词文件不存在".to_string()));
            }
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?
        };
        let timestamp = get_unix_timestamp()?;

        let id = format!("imported-{timestamp}");
        let prompt = Prompt {
            id: id.clone(),
            name: format!(
                "导入的提示词 {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("从现有配置文件导入".to_string()),
            enabled: false,
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        Self::upsert_prompt(state, app, &id, prompt)?;
        Ok(id)
    }

    pub fn get_current_file_content(app: AppType) -> Result<Option<String>, AppError> {
        if matches!(app, AppType::Pi) {
            return Ok(PiAgentsFileGuard::acquire()?.read()?.content);
        }
        let file_path = prompt_file_path(&app)?;
        if !file_path.exists() {
            return Ok(None);
        }
        let content =
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?;
        Ok(Some(content))
    }

    /// Project the database SSOT to one application's managed prompt file.
    ///
    /// This deliberately does not call `enable_prompt`: restore paths must not
    /// read stale live content and write it back into the freshly imported DB.
    pub fn sync_to_live(state: &AppState, app: AppType) -> Result<(), AppError> {
        // Pi derives activation from its native AGENTS.md; its persisted prompt
        // rows are intentionally disabled and must not drive generic projection.
        if matches!(app, AppType::ClaudeDesktop | AppType::Pi) {
            return Ok(());
        }

        let prompts = state.db.get_prompts(app.as_str())?;
        let target_path = prompt_file_path(&app)?;
        if let Some(warning) = project_prompt_set_to_path(&prompts, &target_path)? {
            return Err(AppError::Message(warning));
        }
        Ok(())
    }

    /// Best-effort projection for every Prompt-capable application.
    pub fn sync_all_to_live(state: &AppState) -> Result<(), AppError> {
        let mut failures = Vec::new();
        for app in AppType::all() {
            if matches!(app, AppType::ClaudeDesktop) {
                continue;
            }
            if let Err(error) = Self::sync_to_live(state, app.clone()) {
                log::warn!("同步 Prompt 到 {app:?} 失败: {error}");
                failures.push(format!("{}: {error}", app.as_str()));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "部分应用 Prompt 同步失败: {}",
                failures.join("; ")
            )))
        }
    }

    /// 首次启动时从现有提示词文件自动导入（如果存在）
    /// 返回导入的数量
    pub fn import_from_file_on_first_launch(
        state: &AppState,
        app: AppType,
    ) -> Result<usize, AppError> {
        // 幂等性保护：该应用已有提示词则跳过
        let existing = state.db.get_prompts(app.as_str())?;
        if !existing.is_empty() {
            return Ok(0);
        }

        let file_path = prompt_file_path(&app)?;

        // 读取文件内容。Pi 与交互式管理路径共用限长读取和协调锁。
        let content = if matches!(app, AppType::Pi) {
            match PiAgentsFileGuard::acquire().and_then(|guard| guard.read()) {
                Ok(snapshot) => match snapshot.content {
                    Some(content) => content,
                    None => return Ok(0),
                },
                Err(error) => {
                    log::warn!("读取提示词文件失败: {file_path:?}, 错误: {error}");
                    return Ok(0);
                }
            }
        } else {
            if !file_path.exists() {
                return Ok(0);
            }
            match std::fs::read_to_string(&file_path) {
                Ok(content) => content,
                Err(error) => {
                    log::warn!("读取提示词文件失败: {file_path:?}, 错误: {error}");
                    return Ok(0);
                }
            }
        };

        // 检查内容是否为空
        if content.trim().is_empty() {
            return Ok(0);
        }

        log::info!("发现提示词文件，自动导入: {file_path:?}");

        // 创建提示词对象
        let timestamp = get_unix_timestamp()?;
        let id = format!("auto-imported-{timestamp}");
        let prompt = Prompt {
            id: id.clone(),
            name: format!(
                "Auto-imported Prompt {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("Automatically imported on first launch".to_string()),
            // Pi derives active state from AGENTS.md. Other apps retain their
            // established persisted prompt selection.
            enabled: !matches!(app, AppType::Pi),
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        // 保存到数据库
        state.db.save_prompt(app.as_str(), &prompt)?;

        log::info!("自动导入完成: {}", app.as_str());
        Ok(1)
    }
}

fn pi_active_prompt_id(
    prompts: &IndexMap<String, Prompt>,
    live_content: Option<&str>,
) -> Option<String> {
    let live_content = live_content?;
    prompts
        .iter()
        .find(|(_, prompt)| prompt.content == live_content)
        .map(|(id, _)| id.clone())
}

fn unique_pi_backup_id(prompts: &IndexMap<String, Prompt>, timestamp: i64) -> String {
    let base = format!("backup-{timestamp}");
    if !prompts.contains_key(&base) {
        return base;
    }
    for suffix in 2_u64.. {
        let candidate = format!("{base}-{suffix}");
        if !prompts.contains_key(&candidate) {
            return candidate;
        }
    }
    unreachable!("the backup suffix space is finite only after u64 exhaustion")
}

fn get_pi_prompts(state: &AppState) -> Result<IndexMap<String, Prompt>, AppError> {
    let guard = PiAgentsFileGuard::acquire()?;
    let mut prompts = state.db.get_prompts(AppType::Pi.as_str())?;
    let snapshot = guard.read()?;
    let active_id = pi_active_prompt_id(&prompts, snapshot.content.as_deref());

    for (id, prompt) in &mut prompts {
        prompt.enabled = active_id.as_ref() == Some(id);
    }
    Ok(prompts)
}

fn upsert_pi_prompt(state: &AppState, id: &str, prompt: Prompt) -> Result<(), AppError> {
    if prompt.id != id {
        return Err(AppError::InvalidInput(
            "Pi prompt id does not match the requested id".to_string(),
        ));
    }

    let guard = PiAgentsFileGuard::acquire()?;
    let prompts = state.db.get_prompts(AppType::Pi.as_str())?;
    let snapshot = guard.read()?;
    let was_active =
        pi_active_prompt_id(&prompts, snapshot.content.as_deref()).as_deref() == Some(id);
    let previous = prompts.get(id).cloned();
    let requested_active = prompt.enabled;
    let mut stored = prompt;
    stored.enabled = false;

    if requested_active && !was_active {
        return Err(AppError::Conflict(
            "Pi AGENTS.md changed outside CC Switch; reload before editing it".to_string(),
        ));
    }

    persist_pi_prompt_with_native_update(state, id, &stored, previous.as_ref(), || {
        if requested_active {
            guard.replace(&snapshot.revision, &stored.content)
        } else if was_active {
            guard.delete(&snapshot.revision)
        } else {
            Ok(())
        }
    })
}

fn persist_pi_prompt_with_native_update(
    state: &AppState,
    id: &str,
    stored: &Prompt,
    previous: Option<&Prompt>,
    update_native: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    state.db.save_prompt(AppType::Pi.as_str(), stored)?;
    if let Err(native_error) = update_native() {
        let rollback = match previous {
            Some(previous) => state.db.save_prompt(AppType::Pi.as_str(), previous),
            None => state.db.delete_prompt(AppType::Pi.as_str(), id),
        };
        if let Err(rollback_error) = rollback {
            return Err(AppError::Message(format!(
                "Pi prompt update failed ({native_error}); database rollback also failed: {rollback_error}"
            )));
        }
        return Err(native_error);
    }
    Ok(())
}

fn enable_pi_prompt(state: &AppState, id: &str) -> Result<(), AppError> {
    let guard = PiAgentsFileGuard::acquire()?;
    let prompts = state.db.get_prompts(AppType::Pi.as_str())?;
    let target = prompts
        .get(id)
        .cloned()
        .ok_or_else(|| AppError::InvalidInput(format!("提示词 {id} 不存在")))?;
    let snapshot = guard.read()?;

    if let Some(content) = snapshot.content.as_ref() {
        let already_saved = prompts.values().any(|prompt| prompt.content == *content);
        if !content.trim().is_empty() && !already_saved {
            let timestamp = get_unix_timestamp()?;
            let backup = Prompt {
                id: unique_pi_backup_id(&prompts, timestamp),
                name: format!(
                    "原始提示词 {}",
                    chrono::Local::now().format("%Y-%m-%d %H:%M")
                ),
                content: content.clone(),
                description: Some("自动备份的原始提示词".to_string()),
                enabled: false,
                created_at: Some(timestamp),
                updated_at: Some(timestamp),
            };
            state.db.save_prompt(AppType::Pi.as_str(), &backup)?;
        }
    }

    guard.replace(&snapshot.revision, &target.content)
}

fn delete_pi_prompt(state: &AppState, id: &str) -> Result<(), AppError> {
    let guard = PiAgentsFileGuard::acquire()?;
    let prompts = state.db.get_prompts(AppType::Pi.as_str())?;
    let snapshot = guard.read()?;
    if pi_active_prompt_id(&prompts, snapshot.content.as_deref()).as_deref() == Some(id) {
        return Err(AppError::InvalidInput("无法删除已启用的提示词".to_string()));
    }
    state.db.delete_prompt(AppType::Pi.as_str(), id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::project_prompt_set_to_path;
    use crate::prompt::Prompt;
    use indexmap::IndexMap;
    use tempfile::tempdir;

    fn prompt(id: &str, content: &str, enabled: bool) -> Prompt {
        Prompt {
            id: id.to_string(),
            name: id.to_string(),
            content: content.to_string(),
            description: None,
            enabled,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn restored_prompt_projection_writes_the_enabled_content() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        let mut prompts = IndexMap::new();
        prompts.insert("off".to_string(), prompt("off", "old", false));
        prompts.insert("on".to_string(), prompt("on", "restored", true));

        let warning = project_prompt_set_to_path(&prompts, &path).expect("project prompt");
        assert!(warning.is_none());
        assert_eq!(
            std::fs::read_to_string(path).expect("read prompt"),
            "restored"
        );
    }

    #[test]
    fn restored_prompt_projection_preserves_the_live_file_when_none_are_enabled() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        std::fs::write(&path, "local content").expect("seed live prompt file");
        let mut prompts = IndexMap::new();
        prompts.insert("off".to_string(), prompt("off", "managed", false));

        let warning = project_prompt_set_to_path(&prompts, &path).expect("project prompt");
        assert!(warning.is_none());
        // The live file is not part of the sync payload, so a restore with no
        // enabled prompt must not wipe local content it never contained.
        assert_eq!(
            std::fs::read_to_string(path).expect("read prompt"),
            "local content"
        );
    }

    #[test]
    fn restored_prompt_projection_selects_the_first_enabled_prompt_deterministically() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        let mut prompts = IndexMap::new();
        prompts.insert("first".to_string(), prompt("first", "first body", true));
        prompts.insert("second".to_string(), prompt("second", "second body", true));

        let warning = project_prompt_set_to_path(&prompts, &path)
            .expect("project prompt")
            .expect("duplicate enabled prompts should warn");
        assert!(warning.contains("first, second"));
        assert_eq!(
            std::fs::read_to_string(path).expect("read prompt"),
            "first body"
        );
    }
}

#[cfg(test)]
mod pi_prompt_tests {
    use super::*;
    use crate::database::Database;
    use crate::pi_config::test_support::TestAgentDir;
    use serial_test::serial;
    use std::sync::Arc;

    fn prompt(enabled: bool) -> Prompt {
        Prompt {
            id: "test-prompt".to_string(),
            name: "Test prompt".to_string(),
            content: "managed content".to_string(),
            description: None,
            enabled,
            created_at: Some(1),
            updated_at: Some(1),
        }
    }

    #[test]
    #[serial]
    fn pi_active_prompt_is_derived_from_agents_file() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        state
            .db
            .save_prompt(AppType::Pi.as_str(), &prompt(true))
            .expect("save prompt");

        let saved = PromptService::get_prompts(&state, AppType::Pi).expect("load prompts");
        assert!(!saved["test-prompt"].enabled);

        let path = prompt_file_path(&AppType::Pi).expect("prompt path");
        write_text_file(&path, "managed content").expect("write AGENTS.md");
        let active = PromptService::get_prompts(&state, AppType::Pi).expect("load prompts");
        assert!(active["test-prompt"].enabled);

        write_text_file(&path, "external edit").expect("edit AGENTS.md externally");
        let drifted = PromptService::get_prompts(&state, AppType::Pi).expect("load prompts");
        assert!(!drifted["test-prompt"].enabled);
        assert!(
            PromptService::upsert_prompt(&state, AppType::Pi, "test-prompt", prompt(true),)
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read AGENTS.md"),
            "external edit"
        );

        write_text_file(&path, "managed content").expect("restore AGENTS.md");
        PromptService::upsert_prompt(&state, AppType::Pi, "test-prompt", prompt(false))
            .expect("disable prompt");
        assert!(!path.exists());
    }

    #[test]
    #[serial]
    fn generic_prompt_projection_does_not_rewrite_pi_agents_file() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        state
            .db
            .save_prompt(AppType::Pi.as_str(), &prompt(false))
            .expect("save Pi prompt");

        let path = prompt_file_path(&AppType::Pi).expect("prompt path");
        write_text_file(&path, "native instructions").expect("write AGENTS.md");

        PromptService::sync_to_live(&state, AppType::Pi).expect("sync prompts");

        assert_eq!(
            std::fs::read_to_string(path).expect("read AGENTS.md"),
            "native instructions"
        );
    }

    #[test]
    #[serial]
    fn editing_an_inactive_duplicate_pi_prompt_preserves_agents_file() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let first = prompt(false);
        let mut duplicate = first.clone();
        duplicate.id = "duplicate-prompt".to_string();
        duplicate.name = "Duplicate prompt".to_string();
        duplicate.created_at = Some(2);
        state
            .db
            .save_prompt(AppType::Pi.as_str(), &first)
            .expect("save first prompt");
        state
            .db
            .save_prompt(AppType::Pi.as_str(), &duplicate)
            .expect("save duplicate prompt");
        let path = prompt_file_path(&AppType::Pi).expect("prompt path");
        write_text_file(&path, "managed content").expect("write AGENTS.md");

        let hydrated = PromptService::get_prompts(&state, AppType::Pi).expect("load prompts");
        assert!(hydrated["test-prompt"].enabled);
        assert!(!hydrated["duplicate-prompt"].enabled);

        duplicate.content = "edited duplicate".to_string();
        PromptService::upsert_prompt(&state, AppType::Pi, "duplicate-prompt", duplicate)
            .expect("edit inactive duplicate");

        assert_eq!(
            std::fs::read_to_string(&path).expect("read AGENTS.md"),
            "managed content"
        );
        let refreshed = PromptService::get_prompts(&state, AppType::Pi).expect("reload prompts");
        assert!(refreshed["test-prompt"].enabled);
        assert!(!refreshed["duplicate-prompt"].enabled);
    }

    #[test]
    #[serial]
    fn failed_pi_native_update_restores_the_previous_database_prompt() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let previous = prompt(false);
        state
            .db
            .save_prompt(AppType::Pi.as_str(), &previous)
            .expect("save previous prompt");
        let mut edited = previous.clone();
        edited.content = "edited content".to_string();

        let result = persist_pi_prompt_with_native_update(
            &state,
            &edited.id,
            &edited,
            Some(&previous),
            || Err(AppError::Message("native write failed".to_string())),
        );

        assert!(result.is_err());
        let saved = state
            .db
            .get_prompts(AppType::Pi.as_str())
            .expect("reload prompts");
        assert_eq!(saved["test-prompt"].content, "managed content");
    }

    #[test]
    fn pi_backup_ids_do_not_replace_an_existing_same_second_backup() {
        let mut prompts = IndexMap::new();
        let mut first = prompt(false);
        first.id = "backup-42".to_string();
        prompts.insert(first.id.clone(), first);
        let mut second = prompt(false);
        second.id = "backup-42-2".to_string();
        prompts.insert(second.id.clone(), second);

        assert_eq!(unique_pi_backup_id(&prompts, 42), "backup-42-3");
    }
}

use indexmap::IndexMap;

use crate::app_config::AppType;
use crate::config::write_text_file;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::prompt_files::prompt_file_path;
use crate::store::AppState;

/// 安全地获取当前 Unix 时间戳
fn get_unix_timestamp() -> Result<i64, AppError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| AppError::Message(format!("Failed to get system time: {e}")))
}

pub struct PromptService;

impl PromptService {
    pub fn get_prompts(
        state: &AppState,
        app: AppType,
    ) -> Result<IndexMap<String, Prompt>, AppError> {
        state.db.get_prompts(app.as_str())
    }

    /// 写入 live 文件前调用：若 live 文件与 DB 期望内容不一致（存在外部/手动修改），
    /// 将 live 内容回填到当前已启用的提示词，或创建一条禁用备份（去重）。
    /// 返回是否发生过不一致（true = 已回填或已备份）。
    ///
    /// "DB 期望内容"定义：当前 enabled 提示词的 content；若无 enabled 项则为空串。
    /// 仅在 `live.trim() != expected.trim()` 时才动作，避免无谓的回填与冗余备份。
    ///
    /// live 为空（文件不存在、被外部删除或内容全为空白）时一律不动 DB：空文件不
    /// 表达"用户想清空提示词"这个意图，若据此回填会把已启用项的内容抹成空串。
    ///
    /// `exclude_id`：若不一致的合并目标恰好是这个 id（即将被调用方自己的写入覆盖
    /// 的那一条），跳过合并、强制走备份分支——否则合并进去的内容会在紧随其后的
    /// 保存里被覆盖，等于没保护。
    fn backfill_live_if_dirty(
        state: &AppState,
        app: AppType,
        exclude_id: Option<&str>,
    ) -> Result<bool, AppError> {
        let target_path = prompt_file_path(&app)?;
        let live = if target_path.exists() {
            std::fs::read_to_string(&target_path).unwrap_or_default()
        } else {
            String::new()
        };

        if live.trim().is_empty() {
            return Ok(false);
        }

        // 计算 DB 期望内容
        let prompts = state.db.get_prompts(app.as_str())?;
        let expected = prompts
            .values()
            .find(|p| p.enabled)
            .map(|p| p.content.clone())
            .unwrap_or_default();

        // 一致：无需处理
        if live.trim() == expected.trim() {
            return Ok(false);
        }

        // 不一致：有 enabled 项（且不是被排除的 id）→ 回填；否则 → 创建一次备份（去重）
        if let Some((enabled_id, enabled_prompt)) = prompts
            .iter()
            .find(|(id, p)| p.enabled && exclude_id != Some(id.as_str()))
            .map(|(id, p)| (id.clone(), p.clone()))
        {
            let mut updated = enabled_prompt;
            updated.content = live;
            updated.updated_at = Some(get_unix_timestamp()?);
            log::info!("回填 live 提示词内容到已启用项: {enabled_id}");
            state.db.save_prompt(app.as_str(), &updated)?;
        } else {
            // 没有已启用的提示词，则创建一次备份（避免重复备份）
            let content_exists = prompts.values().any(|p| p.content.trim() == live.trim());
            if !content_exists {
                let timestamp = get_unix_timestamp()?;
                let backup = Prompt {
                    id: format!("backup-{timestamp}"),
                    name: format!(
                        "原始提示词 {}",
                        chrono::Local::now().format("%Y-%m-%d %H:%M")
                    ),
                    content: live,
                    description: Some("自动备份的原始提示词".to_string()),
                    enabled: false,
                    created_at: Some(timestamp),
                    updated_at: Some(timestamp),
                };
                log::info!("回填 live 提示词内容，创建备份: {}", backup.id);
                state.db.save_prompt(app.as_str(), &backup)?;
            }
        }

        Ok(true)
    }

    pub fn upsert_prompt(
        state: &AppState,
        app: AppType,
        _id: &str,
        prompt: Prompt,
    ) -> Result<(), AppError> {
        // 检查是否为已启用的提示词
        let is_enabled = prompt.enabled;

        if is_enabled {
            // 覆盖 live 文件前先保护外部修改；这条即将被本次编辑覆盖，不能合并回它自己
            Self::backfill_live_if_dirty(state, app.clone(), Some(&prompt.id))?;
        }

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
                // 所有提示词都已禁用，清空前先保留 live 文件的外部修改
                Self::backfill_live_if_dirty(state, app.clone(), None)?;
                let target_path = prompt_file_path(&app)?;
                if target_path.exists() {
                    write_text_file(&target_path, "")?;
                }
            }
        }

        Ok(())
    }

    pub fn delete_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        let prompts = state.db.get_prompts(app.as_str())?;

        if let Some(prompt) = prompts.get(id) {
            if prompt.enabled {
                return Err(AppError::InvalidInput("无法删除已启用的提示词".to_string()));
            }
        }

        state.db.delete_prompt(app.as_str(), id)?;
        Ok(())
    }

    pub fn enable_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        // 写入前先保留 live 文件的外部修改（若与 DB 期望内容不一致）
        Self::backfill_live_if_dirty(state, app.clone(), None)?;

        // 启用目标提示词并写入文件
        let target_path = prompt_file_path(&app)?;
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
        let file_path = prompt_file_path(&app)?;

        if !file_path.exists() {
            return Err(AppError::Message("提示词文件不存在".to_string()));
        }

        let content =
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?;
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
        let file_path = prompt_file_path(&app)?;
        if !file_path.exists() {
            return Ok(None);
        }
        let content =
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?;
        Ok(Some(content))
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

        // 检查文件是否存在
        if !file_path.exists() {
            return Ok(0);
        }

        // 读取文件内容
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("读取提示词文件失败: {file_path:?}, 错误: {e}");
                return Ok(0);
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
            enabled: true, // 首次导入时自动启用
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        // 保存到数据库
        state.db.save_prompt(app.as_str(), &prompt)?;

        log::info!("自动导入完成: {}", app.as_str());
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::store::AppState;
    use serial_test::serial;
    use std::sync::{Arc, Mutex, OnceLock};

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    fn with_test_home<T>(test: impl FnOnce(&AppState) -> T) -> T {
        let _guard = test_guard();
        let temp = tempfile::tempdir().expect("tempdir");
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let db = Arc::new(Database::memory().expect("in-memory database"));
        let state = AppState::new(db);
        let result = test(&state);

        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }

        result
    }

    fn make_prompt(id: &str, content: &str, enabled: bool) -> Prompt {
        Prompt {
            id: id.to_string(),
            name: id.to_string(),
            content: content.to_string(),
            description: None,
            enabled,
            created_at: Some(0),
            updated_at: Some(0),
        }
    }

    #[test]
    #[serial]
    fn backfill_is_noop_when_live_matches_expected() {
        with_test_home(|state| {
            let app = AppType::Claude;
            let prompt = make_prompt("p1", "hello", true);
            state.db.save_prompt(app.as_str(), &prompt).unwrap();

            let target_path = prompt_file_path(&app).unwrap();
            std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
            write_text_file(&target_path, "hello").unwrap();

            let dirty = PromptService::backfill_live_if_dirty(state, app.clone(), None).unwrap();
            assert!(!dirty);

            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert_eq!(prompts.len(), 1);
            assert_eq!(prompts.get("p1").unwrap().content, "hello");
        });
    }

    #[test]
    #[serial]
    fn backfill_is_noop_when_live_is_empty_or_missing() {
        with_test_home(|state| {
            let app = AppType::Claude;
            let prompt = make_prompt("p1", "db content", true);
            state.db.save_prompt(app.as_str(), &prompt).unwrap();

            // live 文件不存在：不能把空内容当作"用户清空了文件"回填进 DB
            let dirty = PromptService::backfill_live_if_dirty(state, app.clone(), None).unwrap();
            assert!(!dirty);
            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert_eq!(prompts.get("p1").unwrap().content, "db content");

            // live 文件存在但为空：同样不回填
            let target_path = prompt_file_path(&app).unwrap();
            std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
            write_text_file(&target_path, "   \n").unwrap();

            let dirty = PromptService::backfill_live_if_dirty(state, app.clone(), None).unwrap();
            assert!(!dirty);
            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert_eq!(prompts.len(), 1);
            assert_eq!(prompts.get("p1").unwrap().content, "db content");
        });
    }

    #[test]
    #[serial]
    fn backfill_updates_enabled_prompt_when_live_differs() {
        with_test_home(|state| {
            let app = AppType::Claude;
            let prompt = make_prompt("p1", "old content", true);
            state.db.save_prompt(app.as_str(), &prompt).unwrap();

            let target_path = prompt_file_path(&app).unwrap();
            std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
            write_text_file(&target_path, "manually edited content").unwrap();

            let dirty = PromptService::backfill_live_if_dirty(state, app.clone(), None).unwrap();
            assert!(dirty);

            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert_eq!(prompts.len(), 1);
            assert_eq!(
                prompts.get("p1").unwrap().content,
                "manually edited content"
            );
        });
    }

    #[test]
    #[serial]
    fn backfill_creates_backup_when_no_enabled_prompt() {
        with_test_home(|state| {
            let app = AppType::Claude;
            let prompt = make_prompt("p1", "some disabled content", false);
            state.db.save_prompt(app.as_str(), &prompt).unwrap();

            let target_path = prompt_file_path(&app).unwrap();
            std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
            write_text_file(&target_path, "manually edited content").unwrap();

            let dirty = PromptService::backfill_live_if_dirty(state, app.clone(), None).unwrap();
            assert!(dirty);

            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert_eq!(prompts.len(), 2);
            assert!(prompts
                .values()
                .any(|p| p.id.starts_with("backup-") && p.content == "manually edited content"));
        });
    }

    #[test]
    #[serial]
    fn backfill_skips_duplicate_backup_when_content_already_exists() {
        with_test_home(|state| {
            let app = AppType::Claude;
            let prompt = make_prompt("p1", "manually edited content", false);
            state.db.save_prompt(app.as_str(), &prompt).unwrap();

            let target_path = prompt_file_path(&app).unwrap();
            std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
            write_text_file(&target_path, "manually edited content").unwrap();

            let dirty = PromptService::backfill_live_if_dirty(state, app.clone(), None).unwrap();
            assert!(dirty);

            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert_eq!(prompts.len(), 1);
        });
    }

    #[test]
    #[serial]
    fn upsert_prompt_backs_up_external_content_before_clearing() {
        with_test_home(|state| {
            let app = AppType::Claude;
            let prompt = make_prompt("p1", "old content", true);
            state.db.save_prompt(app.as_str(), &prompt).unwrap();

            let target_path = prompt_file_path(&app).unwrap();
            std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
            write_text_file(&target_path, "manually edited content").unwrap();

            let disabled = make_prompt("p1", "old content", false);
            PromptService::upsert_prompt(state, app.clone(), "p1", disabled).unwrap();

            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert!(prompts
                .values()
                .any(|p| p.id.starts_with("backup-") && p.content == "manually edited content"));

            let live = std::fs::read_to_string(&target_path).unwrap();
            assert_eq!(live, "");
        });
    }

    #[test]
    #[serial]
    fn upsert_prompt_backs_up_external_edit_when_editing_enabled_prompt() {
        with_test_home(|state| {
            let app = AppType::Claude;
            let prompt = make_prompt("p1", "A", true);
            state.db.save_prompt(app.as_str(), &prompt).unwrap();

            let target_path = prompt_file_path(&app).unwrap();
            std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
            write_text_file(&target_path, "C").unwrap(); // 手动改过 live 文件

            let edited = make_prompt("p1", "B", true); // 表单编辑与外部修改 "C" 无关
            PromptService::upsert_prompt(state, app.clone(), "p1", edited).unwrap();

            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert_eq!(prompts.get("p1").unwrap().content, "B");
            assert!(prompts
                .values()
                .any(|p| p.id.starts_with("backup-") && p.content == "C"));

            let live = std::fs::read_to_string(&target_path).unwrap();
            assert_eq!(live, "B");
        });
    }

    #[test]
    #[serial]
    fn upsert_prompt_is_noop_when_editing_enabled_prompt_without_external_change() {
        with_test_home(|state| {
            let app = AppType::Claude;
            let prompt = make_prompt("p1", "A", true);
            state.db.save_prompt(app.as_str(), &prompt).unwrap();

            let target_path = prompt_file_path(&app).unwrap();
            std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
            write_text_file(&target_path, "A").unwrap(); // live 与编辑前的 DB 内容一致，没有外部修改

            // 用户主动把内容从 "A" 改成 "B" 并保存，这是正常编辑，不是外部修改
            let edited = make_prompt("p1", "B", true);
            PromptService::upsert_prompt(state, app.clone(), "p1", edited).unwrap();

            let prompts = state.db.get_prompts(app.as_str()).unwrap();
            assert_eq!(prompts.len(), 1); // 不应该产生多余的 backup-*
            assert_eq!(prompts.get("p1").unwrap().content, "B");
        });
    }
}

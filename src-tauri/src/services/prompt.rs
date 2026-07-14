use indexmap::IndexMap;

use crate::app_config::AppType;
use crate::config::write_text_file;
use crate::database::PromptSortUpdate;
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

    pub fn upsert_prompt(
        state: &AppState,
        app: AppType,
        _id: &str,
        prompt: Prompt,
    ) -> Result<(), AppError> {
        Self::backup_external_live_content(state, &app, Some(&prompt.content))?;
        state.db.save_prompt(app.as_str(), &prompt)?;
        Self::write_enabled_prompts(state, &app)
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
        Self::set_prompt_enabled(state, app, id, true)
    }

    pub fn set_prompt_enabled(
        state: &AppState,
        app: AppType,
        id: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        Self::backup_external_live_content(state, &app, None)?;
        state.db.set_prompt_enabled(app.as_str(), id, enabled)?;
        Self::write_enabled_prompts(state, &app)
    }

    /// 替换完整启用集合，供 Profile 切换等批量场景使用。
    pub fn set_enabled_prompts(
        state: &AppState,
        app: AppType,
        enabled_ids: &[String],
    ) -> Result<(), AppError> {
        Self::backup_external_live_content(state, &app, None)?;
        state.db.set_enabled_prompts(app.as_str(), enabled_ids)?;
        Self::write_enabled_prompts(state, &app)
    }

    /// 更新列表顺序；启用项的合并顺序与列表顺序始终一致。
    pub fn update_sort_order(
        state: &AppState,
        app: AppType,
        updates: &[PromptSortUpdate],
    ) -> Result<(), AppError> {
        Self::backup_external_live_content(state, &app, None)?;
        state.db.update_prompts_sort_order(app.as_str(), updates)?;
        Self::write_enabled_prompts(state, &app)
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

    fn write_enabled_prompts(state: &AppState, app: &AppType) -> Result<(), AppError> {
        let prompts = state.db.get_prompts(app.as_str())?;
        let content = merge_enabled_prompt_content(prompts.values());
        let target_path = prompt_file_path(app)?;

        // 没有启用项且文件本就不存在时，不额外创建空文件。
        if content.is_empty() && !target_path.exists() {
            return Ok(());
        }

        write_text_file(&target_path, &content)
    }

    /// 如果 live 文件已被外部修改，则整体备份；多提示词场景无法安全地把
    /// 合并后的改动反向拆回某一条提示词。
    fn backup_external_live_content(
        state: &AppState,
        app: &AppType,
        incoming_content: Option<&str>,
    ) -> Result<(), AppError> {
        let target_path = prompt_file_path(app)?;
        if !target_path.exists() {
            return Ok(());
        }

        let live_content =
            std::fs::read_to_string(&target_path).map_err(|e| AppError::io(&target_path, e))?;
        if live_content.trim().is_empty() {
            return Ok(());
        }

        let prompts = state.db.get_prompts(app.as_str())?;
        let expected = merge_enabled_prompt_content(prompts.values());
        if live_content.trim() == expected.trim()
            || (expected.is_empty()
                && (incoming_content.is_some_and(|content| content.trim() == live_content.trim())
                    || prompts
                        .values()
                        .any(|prompt| prompt.content.trim() == live_content.trim())))
        {
            return Ok(());
        }

        let timestamp = get_unix_timestamp()?;
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
        log::info!("检测到外部修改的 live 提示词，创建整体备份: {backup_id}");
        state.db.save_prompt(app.as_str(), &backup_prompt)
    }
}

fn merge_enabled_prompt_content<'a>(prompts: impl Iterator<Item = &'a Prompt>) -> String {
    prompts
        .filter(|prompt| prompt.enabled)
        .map(|prompt| prompt.content.as_str())
        .filter(|content| !content.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn merges_enabled_prompts_in_iteration_order() {
        let prompts = [
            prompt("first", "  first  ", true),
            prompt("disabled", "ignored", false),
            prompt("second", "second\n", true),
        ];
        assert_eq!(
            merge_enabled_prompt_content(prompts.iter()),
            "  first  \n\nsecond\n"
        );
    }
}

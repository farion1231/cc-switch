//! 全局提示词：一份内容同时用到多个应用（类似 Skills 的统一管理）。
//!
//! 设计要点：
//! - 内容存在 `global_prompts` 表里，是唯一来源。
//! - 用到某个应用时，在该应用的 `prompts` 里落一条 id 为 `global-<id>` 的普通记录，
//!   再走 [`PromptService`] 原有的启用 / 写文件流程。Pi 按 AGENTS.md 内容判定启用、
//!   MiniMax Code 的事务写入和 32 KB 上限、启用前把文件内容存成「原始提示词」、
//!   恢复备份后的投影等既有行为因此全部照旧。
//! - 「在哪些应用里启用」按这些记录现算，不另存开关：在应用自己的列表里换成别的
//!   提示词，全局视图自然就显示为未启用，不会出现两边状态不一致。

use std::collections::BTreeMap;
use std::sync::Mutex;

use indexmap::IndexMap;
use serde::Serialize;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::prompt::{linked_prompt_id, GlobalPrompt, Prompt, GLOBAL_PROMPT_LINK_PREFIX};
use crate::prompt_files::validate_prompt_content;
use crate::services::profile::ProfilePayload;
use crate::services::prompt::PromptService;
use crate::store::AppState;

/// 全局提示词的写操作要跨多个应用，串行执行，避免两次操作交错。
static GLOBAL_PROMPT_LOCK: Mutex<()> = Mutex::new(());

/// 列表里的一条全局提示词。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalPromptEntry {
    #[serde(flatten)]
    pub prompt: GlobalPrompt,
    /// 当前正在使用这条提示词的应用（只列出启用中的）
    pub apps: BTreeMap<String, bool>,
    /// 启用中、但目标文件内容已和全局内容不同的应用（在别处改过文件）
    pub modified_apps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalPromptAppFailure {
    pub app: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalPromptSaveResult {
    pub id: String,
    /// 内容已保存，但同步到这些应用时失败
    pub failures: Vec<GlobalPromptAppFailure>,
}

/// 可以使用全局提示词的应用。Claude Desktop 用 Claude Code 的提示词；
/// OpenClaw 的 AGENTS.md 在它自己的「工作区」里管理。
pub fn global_prompt_apps() -> impl Iterator<Item = AppType> {
    AppType::all().filter(|app| !matches!(app, AppType::ClaudeDesktop | AppType::OpenClaw))
}

fn ensure_supported(app: &AppType) -> Result<(), AppError> {
    if global_prompt_apps().any(|candidate| &candidate == app) {
        Ok(())
    } else {
        Err(AppError::localized(
            "prompts.global_unsupported_app",
            format!("{} 不支持全局提示词", app.as_str()),
            format!("{} does not support global prompts", app.as_str()),
        ))
    }
}

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, AppError> {
    GLOBAL_PROMPT_LOCK
        .lock()
        .map_err(|error| AppError::Message(error.to_string()))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// 某个应用的提示词，`enabled` 表示实际生效：Pi 按 AGENTS.md 内容现算，其余看库里的选择。
/// 不做 live 文件回填，不写任何东西。
fn app_prompts(state: &AppState, app: &AppType) -> Result<IndexMap<String, Prompt>, AppError> {
    if matches!(app, AppType::Pi) {
        PromptService::get_prompts(state, AppType::Pi)
    } else {
        state.db.get_prompts(app.as_str())
    }
}

/// 全局提示词落到某个应用里的那条记录。创建时间沿用已有记录，保持该应用列表里的顺序。
fn linked_row(global: &GlobalPrompt, existing: Option<&Prompt>, enabled: bool) -> Prompt {
    Prompt {
        id: linked_prompt_id(&global.id),
        name: global.name.clone(),
        content: global.content.clone(),
        description: global.description.clone(),
        enabled,
        created_at: existing
            .and_then(|prompt| prompt.created_at)
            .or(global.created_at),
        updated_at: global.updated_at,
    }
}

fn same_payload(prompt: &Prompt, global: &GlobalPrompt) -> bool {
    prompt.name == global.name
        && prompt.content == global.content
        && prompt.description == global.description
}

pub struct GlobalPromptService;

impl GlobalPromptService {
    pub fn list(state: &AppState) -> Result<Vec<GlobalPromptEntry>, AppError> {
        let globals = state.db.get_global_prompts()?;
        if globals.is_empty() {
            return Ok(Vec::new());
        }

        // 每个应用里正在生效的那条全局提示词（每个应用最多一条生效）
        let mut active: Vec<(AppType, String)> = Vec::new();
        for app in global_prompt_apps() {
            match app_prompts(state, &app) {
                Ok(prompts) => {
                    if let Some((id, _)) = prompts.iter().find(|(id, prompt)| {
                        prompt.enabled && id.starts_with(GLOBAL_PROMPT_LINK_PREFIX)
                    }) {
                        active.push((app, id.clone()));
                    }
                }
                Err(error) => {
                    log::warn!(
                        "读取 {} 的提示词失败，全局视图按未启用显示: {error}",
                        app.as_str()
                    );
                }
            }
        }

        Ok(globals
            .into_values()
            .map(|prompt| {
                let linked_id = linked_prompt_id(&prompt.id);
                let mut apps = BTreeMap::new();
                let mut modified_apps = Vec::new();
                for (app, id) in &active {
                    if *id != linked_id {
                        continue;
                    }
                    apps.insert(app.as_str().to_string(), true);
                    // Pi 按内容判定启用，文件一改就不算启用了，不会出现「启用但内容不同」
                    if matches!(app, AppType::Pi) {
                        continue;
                    }
                    match PromptService::get_current_file_content(app.clone()) {
                        Ok(live) => {
                            if live.unwrap_or_default().trim() != prompt.content.trim() {
                                modified_apps.push(app.as_str().to_string());
                            }
                        }
                        Err(error) => log::warn!(
                            "读取 {} 的提示词文件失败，跳过外部修改检测: {error}",
                            app.as_str()
                        ),
                    }
                }
                GlobalPromptEntry {
                    prompt,
                    apps,
                    modified_apps,
                }
            })
            .collect())
    }

    /// 新建或修改全局提示词，并把改动同步到所有已关联的应用。
    ///
    /// 内容先校验（比如 MiniMax Code 的 32 KB 上限），任何一个已关联的应用不接受就整体拒绝，
    /// 不留下半改的状态。之后逐个应用同步，个别应用写文件失败只记进结果，不回滚已保存的内容。
    pub fn upsert(
        state: &AppState,
        mut prompt: GlobalPrompt,
    ) -> Result<GlobalPromptSaveResult, AppError> {
        let _guard = lock()?;
        if prompt.name.trim().is_empty() {
            return Err(AppError::InvalidInput("提示词名称不能为空".to_string()));
        }
        if prompt.id.trim().is_empty() {
            prompt.id = uuid::Uuid::new_v4().to_string();
        }
        let timestamp = now();
        let previous = state.db.get_global_prompt(&prompt.id)?;
        prompt.created_at = previous
            .as_ref()
            .and_then(|previous| previous.created_at)
            .or(prompt.created_at)
            .or(Some(timestamp));
        prompt.updated_at = Some(timestamp);

        let linked_id = linked_prompt_id(&prompt.id);
        let mut links = Vec::new();
        for app in global_prompt_apps() {
            if state.db.get_prompts(app.as_str())?.contains_key(&linked_id) {
                validate_prompt_content(&app, &prompt.content)?;
                links.push(app);
            }
        }

        state.db.save_global_prompt(&prompt)?;

        let mut failures = Vec::new();
        for app in links {
            if let Err(error) = sync_linked_row(state, &app, &prompt) {
                log::warn!("同步全局提示词到 {} 失败: {error}", app.as_str());
                failures.push(GlobalPromptAppFailure {
                    app: app.as_str().to_string(),
                    error: error.to_string(),
                });
            }
        }

        Ok(GlobalPromptSaveResult {
            id: prompt.id,
            failures,
        })
    }

    /// 删除全局提示词。还在某个应用里启用时拒绝（和应用自己的列表一致：启用中的不能删）；
    /// 未启用的关联记录随之删除。
    pub fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
        let _guard = lock()?;
        let linked_id = linked_prompt_id(id);
        let mut active_apps = Vec::new();
        for app in global_prompt_apps() {
            match app_prompts(state, &app) {
                Ok(prompts) => {
                    if prompts.get(&linked_id).is_some_and(|prompt| prompt.enabled) {
                        active_apps.push(app.as_str().to_string());
                    }
                }
                Err(error) => log::warn!(
                    "读取 {} 的提示词失败，删除时按未启用处理: {error}",
                    app.as_str()
                ),
            }
        }
        if !active_apps.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "全局提示词仍在 {} 中启用，先停用再删除",
                active_apps.join(", ")
            )));
        }
        state.db.delete_global_prompt_with_links(id, &linked_id)
    }

    /// 在某个应用里启用 / 停用全局提示词。
    ///
    /// - 启用：确保该应用里有对应记录且内容最新，然后走该应用原有的启用流程
    ///   （替换掉它当前启用的那条；文件里有库里没有的内容会先存成「原始提示词」）。
    ///   已经启用时重新写一次文件，用来覆盖在别处做的修改。
    /// - 停用：和在应用列表里停用一样会清空目标文件。关联记录保留在该应用的列表里
    ///   （带「全局」徽标），和停用普通提示词一样不会从库里消失——项目（Profiles）
    ///   按 id 记着启用的提示词，删掉记录会让它们再也启用不回来。
    pub fn set_app_enabled(
        state: &AppState,
        id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        ensure_supported(&app)?;
        let _guard = lock()?;
        let global = state
            .db
            .get_global_prompt(id)?
            .ok_or_else(|| AppError::InvalidInput(format!("全局提示词 {id} 不存在")))?;
        let linked_id = linked_prompt_id(id);
        let existing = app_prompts(state, &app)?.get(&linked_id).cloned();

        if !enabled {
            return match existing {
                Some(row) if row.enabled => PromptService::upsert_prompt(
                    state,
                    app,
                    &linked_id,
                    Prompt {
                        enabled: false,
                        ..row
                    },
                ),
                _ => Ok(()),
            };
        }

        validate_prompt_content(&app, &global.content)?;
        if existing.as_ref().is_some_and(|row| row.enabled) {
            return PromptService::upsert_prompt(
                state,
                app,
                &linked_id,
                linked_row(&global, existing.as_ref(), true),
            );
        }

        let created = existing.is_none();
        if existing
            .as_ref()
            .is_none_or(|row| !same_payload(row, &global))
        {
            PromptService::upsert_prompt(
                state,
                app.clone(),
                &linked_id,
                linked_row(&global, existing.as_ref(), false),
            )?;
        }
        if let Err(error) = PromptService::enable_prompt(state, app.clone(), &linked_id) {
            if created {
                if let Err(cleanup) = state.db.delete_prompt(app.as_str(), &linked_id) {
                    log::warn!(
                        "启用失败后清理 {} 的关联记录也失败: {cleanup}",
                        app.as_str()
                    );
                }
            }
            return Err(error);
        }
        Ok(())
    }

    /// 把应用里已有的一条提示词设为全局提示词：内容搬进全局库，原记录换成关联记录
    /// （启用状态不变），目标文件不动。返回新的全局提示词 id。
    pub fn promote(state: &AppState, app: AppType, id: &str) -> Result<String, AppError> {
        ensure_supported(&app)?;
        let _guard = lock()?;
        if let Some(global_id) = id.strip_prefix(GLOBAL_PROMPT_LINK_PREFIX) {
            if state.db.get_global_prompt(global_id)?.is_some() {
                return Err(AppError::InvalidInput(
                    "这条提示词已经是全局提示词".to_string(),
                ));
            }
        }
        let source = state
            .db
            .get_prompts(app.as_str())?
            .shift_remove(id)
            .ok_or_else(|| AppError::InvalidInput(format!("提示词 {id} 不存在")))?;

        let timestamp = now();
        let global = GlobalPrompt {
            id: uuid::Uuid::new_v4().to_string(),
            name: source.name.clone(),
            content: source.content.clone(),
            description: source.description.clone(),
            created_at: source.created_at.or(Some(timestamp)),
            updated_at: Some(timestamp),
        };
        let linked = Prompt {
            id: linked_prompt_id(&global.id),
            ..source
        };
        state
            .db
            .promote_prompt_to_global(app.as_str(), id, &linked, &global)?;
        repoint_profile_prompts(state, &app, id, &linked.id);
        Ok(global.id)
    }
}

/// 项目（Profiles）快照按 id 记着各应用启用的提示词。设为全局会把记录换成关联 id，
/// 这里把指向旧 id 的引用一并改掉，否则应用项目时会当成已删除的提示词跳过。
/// 尽力而为：个别项目改写失败只记日志，不影响已完成的转换。
fn repoint_profile_prompts(state: &AppState, app: &AppType, old_id: &str, new_id: &str) {
    let profiles = match state.db.get_all_profiles() {
        Ok(profiles) => profiles,
        Err(error) => {
            log::warn!("读取项目失败，未能更新其中的提示词引用: {error}");
            return;
        }
    };
    for mut profile in profiles {
        let Ok(mut payload) = serde_json::from_str::<ProfilePayload>(&profile.payload) else {
            continue;
        };
        // 项目只管 Claude / Codex 等少数应用；其余应用的槽位不存在
        let Some(slot) = payload.prompts.get_mut(app) else {
            return;
        };
        if slot.as_deref() != Some(old_id) {
            continue;
        }
        *slot = Some(new_id.to_string());
        let saved = serde_json::to_string(&payload)
            .map_err(|error| AppError::Message(error.to_string()))
            .and_then(|json| {
                profile.payload = json;
                state.db.save_profile(&profile)
            });
        if let Err(error) = saved {
            log::warn!("更新项目 {} 的提示词引用失败: {error}", profile.id);
        }
    }
}

/// 把全局内容写进一个已关联应用的记录；该应用正在用它时同时重写目标文件。
fn sync_linked_row(state: &AppState, app: &AppType, global: &GlobalPrompt) -> Result<(), AppError> {
    let linked_id = linked_prompt_id(&global.id);
    let Some(row) = app_prompts(state, app)?.get(&linked_id).cloned() else {
        return Ok(());
    };
    if !row.enabled && same_payload(&row, global) {
        return Ok(());
    }
    PromptService::upsert_prompt(
        state,
        app.clone(),
        &linked_id,
        linked_row(global, Some(&row), row.enabled),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::prompt_files::prompt_file_path;
    use serial_test::serial;
    use std::sync::Arc;

    /// 把各应用的目录指到临时主目录（#[serial] 保证互斥，这里只负责恢复原值）。
    struct TestHomeGuard {
        _dir: tempfile::TempDir,
        previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl TestHomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp home");
            let vars = ["CC_SWITCH_TEST_HOME", "MINIMAX_DATA_DIR", "MAVIS_DATA_DIR"];
            let previous = vars
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect();
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            std::env::remove_var("MINIMAX_DATA_DIR");
            std::env::remove_var("MAVIS_DATA_DIR");
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            for (name, value) in self.previous.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    fn state() -> AppState {
        AppState::new(Arc::new(Database::memory().expect("in-memory database")))
    }

    fn global(id: &str, content: &str) -> GlobalPrompt {
        GlobalPrompt {
            id: id.to_string(),
            name: format!("{id} name"),
            content: content.to_string(),
            description: None,
            created_at: None,
            updated_at: None,
        }
    }

    fn local(id: &str, content: &str, enabled: bool) -> Prompt {
        Prompt {
            id: id.to_string(),
            name: id.to_string(),
            content: content.to_string(),
            description: None,
            enabled,
            created_at: Some(1),
            updated_at: Some(1),
        }
    }

    fn read(app: &AppType) -> String {
        std::fs::read_to_string(prompt_file_path(app).expect("path")).unwrap_or_default()
    }

    fn apps_of(state: &AppState, id: &str) -> Vec<String> {
        GlobalPromptService::list(state)
            .expect("list")
            .into_iter()
            .find(|entry| entry.prompt.id == id)
            .expect("entry")
            .apps
            .into_keys()
            .collect()
    }

    #[test]
    #[serial]
    fn one_global_prompt_drives_several_apps_and_edits_propagate() {
        let _home = TestHomeGuard::new();
        let state = state();
        let saved = GlobalPromptService::upsert(&state, global("", "shared v1")).expect("save");
        assert!(saved.failures.is_empty());
        let id = saved.id;

        GlobalPromptService::set_app_enabled(&state, &id, AppType::Claude, true).expect("claude");
        GlobalPromptService::set_app_enabled(&state, &id, AppType::Codex, true).expect("codex");
        assert_eq!(read(&AppType::Claude), "shared v1");
        assert_eq!(read(&AppType::Codex), "shared v1");
        assert_eq!(apps_of(&state, &id), vec!["claude", "codex"]);

        let mut edited = state.db.get_global_prompt(&id).unwrap().unwrap();
        edited.content = "shared v2".to_string();
        let result = GlobalPromptService::upsert(&state, edited).expect("edit");
        assert!(result.failures.is_empty());
        assert_eq!(read(&AppType::Claude), "shared v2");
        assert_eq!(read(&AppType::Codex), "shared v2");

        // 停用：清空该应用的文件；关联记录留在列表里（项目快照还能按 id 启用回来）
        GlobalPromptService::set_app_enabled(&state, &id, AppType::Codex, false).expect("off");
        assert_eq!(read(&AppType::Codex), "");
        let codex = state.db.get_prompts("codex").unwrap();
        assert!(!codex[&linked_prompt_id(&id)].enabled);
        assert_eq!(read(&AppType::Claude), "shared v2");
        assert_eq!(apps_of(&state, &id), vec!["claude"]);

        // 未启用的关联记录也跟着全局内容更新，再启用时就是最新内容
        let mut edited = state.db.get_global_prompt(&id).unwrap().unwrap();
        edited.content = "shared v3".to_string();
        GlobalPromptService::upsert(&state, edited).expect("edit again");
        assert_eq!(
            state.db.get_prompts("codex").unwrap()[&linked_prompt_id(&id)].content,
            "shared v3"
        );
        PromptService::enable_prompt(&state, AppType::Codex, &linked_prompt_id(&id)).unwrap();
        assert_eq!(read(&AppType::Codex), "shared v3");
        assert_eq!(apps_of(&state, &id), vec!["claude", "codex"]);
    }

    #[test]
    #[serial]
    fn enabling_backs_up_unmanaged_file_and_switching_locally_is_reflected() {
        let _home = TestHomeGuard::new();
        let state = state();
        let path = prompt_file_path(&AppType::Claude).unwrap();
        crate::config::write_text_file(&path, "hand written").unwrap();
        state
            .db
            .save_prompt("claude", &local("mine", "mine body", false))
            .unwrap();

        let id = GlobalPromptService::upsert(&state, global("g", "shared"))
            .unwrap()
            .id;
        GlobalPromptService::set_app_enabled(&state, &id, AppType::Claude, true).unwrap();
        assert_eq!(read(&AppType::Claude), "shared");
        // 文件里原来的内容按旧流程存成了「原始提示词」
        assert!(state
            .db
            .get_prompts("claude")
            .unwrap()
            .values()
            .any(|prompt| prompt.content == "hand written" && !prompt.enabled));

        // 在应用自己的列表里换成别的提示词，全局视图跟着显示未启用
        PromptService::enable_prompt(&state, AppType::Claude, "mine").unwrap();
        assert!(apps_of(&state, &id).is_empty());
        assert_eq!(read(&AppType::Claude), "mine body");
    }

    #[test]
    #[serial]
    fn external_edits_are_flagged_and_reapplying_overwrites_them() {
        let _home = TestHomeGuard::new();
        let state = state();
        let id = GlobalPromptService::upsert(&state, global("g", "shared"))
            .unwrap()
            .id;
        GlobalPromptService::set_app_enabled(&state, &id, AppType::Gemini, true).unwrap();
        let path = prompt_file_path(&AppType::Gemini).unwrap();
        crate::config::write_text_file(&path, "edited elsewhere").unwrap();

        let entry = GlobalPromptService::list(&state).unwrap().remove(0);
        assert_eq!(entry.modified_apps, vec!["gemini"]);

        GlobalPromptService::set_app_enabled(&state, &id, AppType::Gemini, true).unwrap();
        assert_eq!(read(&AppType::Gemini), "shared");
        assert!(GlobalPromptService::list(&state).unwrap()[0]
            .modified_apps
            .is_empty());
    }

    #[test]
    #[serial]
    fn deleting_is_blocked_while_active_and_removes_inactive_links() {
        let _home = TestHomeGuard::new();
        let state = state();
        let id = GlobalPromptService::upsert(&state, global("g", "shared"))
            .unwrap()
            .id;
        GlobalPromptService::set_app_enabled(&state, &id, AppType::Claude, true).unwrap();
        assert!(GlobalPromptService::delete(&state, &id).is_err());

        // 在应用列表里换掉后只剩未启用的关联记录，删除时一并清掉
        state
            .db
            .save_prompt("claude", &local("other", "other", false))
            .unwrap();
        PromptService::enable_prompt(&state, AppType::Claude, "other").unwrap();
        GlobalPromptService::delete(&state, &id).expect("delete");
        assert!(state.db.get_global_prompt(&id).unwrap().is_none());
        assert!(!state
            .db
            .get_prompts("claude")
            .unwrap()
            .contains_key(&linked_prompt_id(&id)));
        assert_eq!(read(&AppType::Claude), "other");
    }

    #[test]
    #[serial]
    fn promoting_keeps_activation_and_leaves_the_file_alone() {
        let _home = TestHomeGuard::new();
        let state = state();
        state
            .db
            .save_prompt("claude", &local("team", "team rules", false))
            .unwrap();
        PromptService::enable_prompt(&state, AppType::Claude, "team").unwrap();

        // 项目快照按 id 记着 Claude 启用的是 team
        let mut payload = ProfilePayload::default();
        payload.prompts.claude = Some("team".to_string());
        payload.prompts.codex = Some("team".to_string());
        state
            .db
            .save_profile(&crate::database::Profile {
                id: "p1".to_string(),
                name: "Project".to_string(),
                payload: serde_json::to_string(&payload).unwrap(),
                sort_order: None,
                created_at: Some(1),
                updated_at: Some(1),
            })
            .unwrap();

        let id = GlobalPromptService::promote(&state, AppType::Claude, "team").unwrap();

        let repointed: ProfilePayload =
            serde_json::from_str(&state.db.get_profile("p1").unwrap().unwrap().payload).unwrap();
        assert_eq!(repointed.prompts.claude, Some(linked_prompt_id(&id)));
        // 别的应用的同名 id 是另一条提示词，不动
        assert_eq!(repointed.prompts.codex, Some("team".to_string()));
        let prompts = state.db.get_prompts("claude").unwrap();
        assert!(!prompts.contains_key("team"));
        let linked = &prompts[&linked_prompt_id(&id)];
        assert!(linked.enabled);
        assert_eq!(linked.created_at, Some(1));
        assert_eq!(read(&AppType::Claude), "team rules");
        assert_eq!(apps_of(&state, &id), vec!["claude"]);
        assert!(
            GlobalPromptService::promote(&state, AppType::Claude, &linked_prompt_id(&id)).is_err()
        );

        GlobalPromptService::set_app_enabled(&state, &id, AppType::Codex, true).unwrap();
        assert_eq!(read(&AppType::Codex), "team rules");
    }

    #[test]
    #[serial]
    fn mcode_size_limit_rejects_the_edit_before_anything_changes() {
        let _home = TestHomeGuard::new();
        let state = state();
        let id = GlobalPromptService::upsert(&state, global("g", "small"))
            .unwrap()
            .id;
        GlobalPromptService::set_app_enabled(&state, &id, AppType::Mcode, true).unwrap();

        let mut huge = state.db.get_global_prompt(&id).unwrap().unwrap();
        huge.content = "a".repeat(32 * 1024 + 1);
        assert!(GlobalPromptService::upsert(&state, huge).is_err());
        assert_eq!(
            state.db.get_global_prompt(&id).unwrap().unwrap().content,
            "small"
        );
        assert_eq!(read(&AppType::Mcode), "small");
    }

    #[test]
    #[serial]
    fn pi_follows_its_content_based_activation() {
        let _home = TestHomeGuard::new();
        let _agent = crate::pi_config::test_support::TestAgentDir::new();
        let state = state();
        let path = prompt_file_path(&AppType::Pi).unwrap();
        crate::config::write_text_file(&path, "native pi instructions").unwrap();

        let id = GlobalPromptService::upsert(&state, global("g", "shared"))
            .unwrap()
            .id;
        GlobalPromptService::set_app_enabled(&state, &id, AppType::Pi, true).unwrap();
        assert_eq!(read(&AppType::Pi), "shared");
        assert_eq!(apps_of(&state, &id), vec!["pi"]);
        // Pi 的启用前备份照旧
        assert!(state
            .db
            .get_prompts("pi")
            .unwrap()
            .values()
            .any(|prompt| prompt.content == "native pi instructions"));

        // 改全局内容会重写 AGENTS.md，Pi 仍判定为启用
        let mut edited = state.db.get_global_prompt(&id).unwrap().unwrap();
        edited.content = "shared v2".to_string();
        assert!(GlobalPromptService::upsert(&state, edited)
            .unwrap()
            .failures
            .is_empty());
        assert_eq!(read(&AppType::Pi), "shared v2");
        assert_eq!(apps_of(&state, &id), vec!["pi"]);

        // 停用：和 Pi 列表里停用一样删除 AGENTS.md
        GlobalPromptService::set_app_enabled(&state, &id, AppType::Pi, false).unwrap();
        assert!(!path.exists());
        assert!(apps_of(&state, &id).is_empty());
    }

    #[test]
    #[serial]
    fn unsupported_apps_are_rejected() {
        let state = state();
        let id = GlobalPromptService::upsert(&state, global("g", "x"))
            .unwrap()
            .id;
        assert!(
            GlobalPromptService::set_app_enabled(&state, &id, AppType::ClaudeDesktop, true)
                .is_err()
        );
        assert!(
            GlobalPromptService::set_app_enabled(&state, &id, AppType::OpenClaw, true).is_err()
        );
    }
}

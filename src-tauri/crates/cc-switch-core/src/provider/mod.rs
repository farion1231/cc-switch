mod model;
mod repository;

use serde_json::Value;

use crate::{CoreError, HeadlessState};

pub use model::{ProviderRecord, ProviderSortUpdate};

/// Provider 领域门面：协调规范数据库事务与目标主机 live 配置投影。
pub struct ProviderService;

impl ProviderService {
    /// 按桌面排序规则读取完整 Provider，并把 endpoint 合并回开放 meta。
    pub fn list(
        state: &HeadlessState,
        app: &str,
    ) -> Result<indexmap::IndexMap<String, ProviderRecord>, CoreError> {
        repository::list(state, app)
    }

    /// 返回指定应用的当前 Provider ID；尚未配置时保持既有空字符串协议。
    pub fn current(state: &HeadlessState, app: &str) -> Result<String, CoreError> {
        repository::current(state, app)
    }

    /// 新增 Provider；若它成为首个当前项，则同步建立 live 配置以维持数据库与 CLI 一致。
    pub fn add(
        state: &HeadlessState,
        app: &str,
        provider: ProviderRecord,
        _add_to_live: bool,
    ) -> Result<bool, CoreError> {
        let had_current = !repository::current(state, app)?.is_empty();
        let settings = provider.settings_config.clone();
        let added = repository::add(state, app, provider)?;
        // 保持既有首项行为：独占模式首个 Provider 必须立即形成 live 配置。
        if !had_current {
            write_live(state, app, &settings)?;
        }
        Ok(added)
    }

    /// 更新完整 Provider；当前项的 live 投影会同步刷新，非当前项只修改规范数据库。
    pub fn update(
        state: &HeadlessState,
        app: &str,
        original_id: &str,
        provider: ProviderRecord,
    ) -> Result<bool, CoreError> {
        let is_current = repository::current(state, app)? == original_id;
        let settings = provider.settings_config.clone();
        let updated = repository::update(state, app, original_id, provider)?;
        if is_current {
            write_live(state, app, &settings)?;
        }
        Ok(updated)
    }

    /// 删除非当前 Provider；当前项必须先切换，防止目标 CLI 失去可追踪的配置来源。
    pub fn delete(state: &HeadlessState, app: &str, id: &str) -> Result<(), CoreError> {
        repository::delete(state, app, id)
    }

    /// 在数据库中原子切换当前项，并把选中配置投影到目标 HOME。
    pub fn switch(state: &HeadlessState, app: &str, id: &str) -> Result<(), CoreError> {
        let provider = repository::switch(state, app, id)?;
        write_live(state, app, &provider.settings_config)
    }

    /// 原子更新同一应用的排序；任一未知 ID 都会回滚整批操作。
    pub fn update_sort_order(
        state: &HeadlessState,
        app: &str,
        updates: &[ProviderSortUpdate],
    ) -> Result<(), CoreError> {
        repository::update_sort_order(state, app, updates)
    }
}

/// Task 5 会用各应用的无界面 writer 替换此兼容实现；当前仅维持已有 Claude/Gemini 回归。
fn write_live(state: &HeadlessState, app: &str, settings: &Value) -> Result<(), CoreError> {
    let path = match app {
        "claude" => state.home().join(".claude").join("settings.json"),
        "gemini" => state.home().join(".gemini").join("settings.json"),
        _ => return Err(CoreError::LiveWriteUnsupported(app.to_string())),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let bytes = serde_json::to_vec_pretty(settings)?;
    std::fs::write(&path, bytes).map_err(|source| CoreError::Io { path, source })
}

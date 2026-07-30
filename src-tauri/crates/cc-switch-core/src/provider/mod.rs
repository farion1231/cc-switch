pub mod live;
mod model;
mod repository;

use crate::{CoreError, HeadlessState};

pub use live::{project_provider, LiveContext, SwitchResult, TargetPlatform};
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
        let projected_provider = provider.clone();
        let added = repository::add(state, app, provider)?;
        // 保持既有首项行为：独占模式首个 Provider 必须立即形成 live 配置。
        if !had_current && should_project_implicitly(state, app) {
            let context = live_context(state);
            live::project_provider(&context, app, &projected_provider)?;
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
        let projected_provider = provider.clone();
        let updated = repository::update(state, app, original_id, provider)?;
        if is_current && should_project_implicitly(state, app) {
            let context = live_context(state);
            live::project_provider(&context, app, &projected_provider)?;
        }
        Ok(updated)
    }

    /// 删除非当前 Provider；当前项必须先切换，防止目标 CLI 失去可追踪的配置来源。
    pub fn delete(state: &HeadlessState, app: &str, id: &str) -> Result<(), CoreError> {
        repository::delete(state, app, id)
    }

    /// 在数据库中原子切换当前项，并把选中配置投影到目标 HOME。
    pub fn switch(state: &HeadlessState, app: &str, id: &str) -> Result<SwitchResult, CoreError> {
        let context = live_context(state);
        // 条件能力必须在事务前检查；Linux 不可写 Claude Desktop 时数据库保持原 current。
        live::ensure_projection_supported(&context, app)?;
        let provider = repository::switch(state, app, id)?;
        live::project_provider(&context, app, &provider)
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

fn live_context(state: &HeadlessState) -> LiveContext<'_> {
    // Context 只携带目标状态自身的信息，不能读取桌面宿主机 HOME 或环境覆盖。
    LiveContext {
        home: state.home(),
        platform: state.platform(),
    }
}

fn should_project_implicitly(state: &HeadlessState, app: &str) -> bool {
    // Linux 可保存和编辑 Claude Desktop 数据，但没有可用客户端路径；只有显式 switch
    // 需要向用户返回条件能力错误，普通 CRUD 不应在数据库成功后伪装成失败。
    !(app == "claude-desktop" && state.platform() == TargetPlatform::Linux)
}

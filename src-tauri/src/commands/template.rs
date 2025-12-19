use tauri::State;

use crate::database::lock_conn;
use crate::error::AppError;
use crate::services::{
    BatchInstallResult, ComponentDetail, InstalledComponent, PaginatedResult, TemplateComponent,
    TemplateRepo, TemplateService,
};
use crate::store::AppState;

/// 刷新模板索引
#[tauri::command]
pub async fn refresh_template_index(state: State<'_, AppState>) -> Result<(), String> {
    let service = TemplateService::new().map_err(|e| e.to_string())?;
    let db = state.db.clone();

    // 使用 spawn_blocking 在后台线程中执行数据库操作
    tokio::task::spawn_blocking(move || {
        let conn = lock_conn!(db.conn);
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            service
                .refresh_index(&conn)
                .await
                .map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))??;

    Ok(())
}

/// 获取模板组件列表
#[tauri::command]
pub fn list_template_components(
    state: State<'_, AppState>,
    component_type: Option<String>,
    category: Option<String>,
    search: Option<String>,
    page: u32,
    page_size: u32,
    app_type: Option<String>,
) -> Result<PaginatedResult<TemplateComponent>, AppError> {
    let (mut components, total) = state.db.list_components(
        component_type.as_deref(),
        category.as_deref(),
        search.as_deref(),
        page,
        page_size,
    )?;

    // 填充 installed 字段
    if let Some(app) = &app_type {
        let installed_ids = state.db.get_installed_component_ids(app)?;
        for component in &mut components {
            if let Some(id) = component.id {
                component.installed = installed_ids.contains(&id);
            }
        }
    }

    Ok(PaginatedResult {
        items: components,
        total: total as i64,
        page,
        page_size,
    })
}

/// 获取组件详情
#[tauri::command]
pub async fn get_template_component(
    state: State<'_, AppState>,
    id: i64,
) -> Result<ComponentDetail, String> {
    let service = TemplateService::new().map_err(|e| e.to_string())?;
    let db = state.db.clone();

    let detail = tokio::task::spawn_blocking(move || {
        let conn = lock_conn!(db.conn);
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            service
                .get_component(&conn, id)
                .await
                .map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))??;

    Ok(detail)
}

/// 安装组件
#[tauri::command]
pub async fn install_template_component(
    state: State<'_, AppState>,
    id: i64,
    app_type: String,
) -> Result<(), String> {
    let service = TemplateService::new().map_err(|e| e.to_string())?;
    let db = state.db.clone();

    tokio::task::spawn_blocking(move || {
        let conn = lock_conn!(db.conn);
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            service
                .install_component(&conn, id, &app_type)
                .await
                .map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))??;

    Ok(())
}

/// 卸载组件
#[tauri::command]
pub fn uninstall_template_component(
    state: State<'_, AppState>,
    id: i64,
    app_type: String,
) -> Result<(), AppError> {
    let service = TemplateService::new().map_err(|e| AppError::Config(e.to_string()))?;
    let conn = lock_conn!(state.db.conn);

    service
        .uninstall_component(&conn, id, &app_type)
        .map_err(|e| AppError::Config(e.to_string()))?;
    Ok(())
}

/// 批量安装组件
#[tauri::command]
pub async fn batch_install_template_components(
    state: State<'_, AppState>,
    ids: Vec<i64>,
    app_type: String,
) -> Result<BatchInstallResult, String> {
    let service = TemplateService::new().map_err(|e| e.to_string())?;
    let db = state.db.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = lock_conn!(db.conn);
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            service
                .batch_install(&conn, ids, &app_type)
                .await
                .map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))??;

    Ok(result)
}

/// 获取模板仓库列表
#[tauri::command]
pub fn list_template_repos(state: State<'_, AppState>) -> Result<Vec<TemplateRepo>, AppError> {
    state.db.list_repos()
}

/// 添加模板仓库
#[tauri::command]
pub fn add_template_repo(
    state: State<'_, AppState>,
    owner: String,
    name: String,
    branch: String,
) -> Result<i64, AppError> {
    let repo = TemplateRepo::new(owner, name, branch);
    state.db.insert_repo(&repo)
}

/// 删除模板仓库
#[tauri::command]
pub fn remove_template_repo(state: State<'_, AppState>, id: i64) -> Result<(), AppError> {
    state.db.delete_repo(id)
}

/// 切换仓库启用状态
#[tauri::command]
pub fn toggle_template_repo(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> Result<(), AppError> {
    state.db.toggle_repo_enabled(id, enabled)
}

/// 获取组件分类列表
#[tauri::command]
pub fn list_template_categories(
    state: State<'_, AppState>,
    component_type: Option<String>,
) -> Result<Vec<String>, AppError> {
    let conn = lock_conn!(state.db.conn);

    // 构建查询语句
    let sql = if let Some(ct) = component_type {
        format!(
            "SELECT DISTINCT category FROM template_components WHERE component_type = '{ct}' AND category IS NOT NULL ORDER BY category"
        )
    } else {
        "SELECT DISTINCT category FROM template_components WHERE category IS NOT NULL ORDER BY category".to_string()
    };

    let mut stmt = conn.prepare(&sql)?;
    let categories = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<String>, _>>()?;

    Ok(categories)
}

/// 获取已安装组件列表
#[tauri::command]
pub fn list_installed_components(
    state: State<'_, AppState>,
    app_type: Option<String>,
    component_type: Option<String>,
) -> Result<Vec<InstalledComponent>, AppError> {
    state
        .db
        .list_installed_components(app_type.as_deref(), component_type.as_deref())
}

/// 预览组件内容
#[tauri::command]
pub async fn preview_component_content(
    state: State<'_, AppState>,
    id: i64,
) -> Result<String, String> {
    let service = TemplateService::new().map_err(|e| e.to_string())?;
    let db = state.db.clone();

    tokio::task::spawn_blocking(move || {
        let conn = lock_conn!(db.conn);
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            service
                .preview_content(&conn, id)
                .await
                .map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))?
}

/// 获取市场组合列表
#[tauri::command]
pub async fn list_marketplace_bundles(
    state: State<'_, AppState>,
) -> Result<Vec<crate::services::MarketplaceBundle>, String> {
    let service = TemplateService::new().map_err(|e| e.to_string())?;
    let db = state.db.clone();

    tokio::task::spawn_blocking(move || {
        let conn = lock_conn!(db.conn);
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            service
                .fetch_marketplace_bundles(&conn)
                .await
                .map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))?
}

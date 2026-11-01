#![allow(non_snake_case)]

use indexmap::IndexMap;
use std::collections::HashMap;

use serde::Serialize;
use tauri::State;

use crate::app_config::AppType;
use crate::claude_mcp;
use crate::services::McpService;
use crate::store::AppState;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiImportRequest {
    pub url: Option<String>,
    pub document: Option<String>,
    pub base_url: Option<String>,
}

/// Preview only: never calls a business endpoint or changes client files.
#[tauri::command]
pub async fn preview_mcp_api(
    request: ApiImportRequest,
) -> Result<crate::mcp_api::ApiPreview, String> {
    let source = request.url.filter(|v| !v.trim().is_empty());
    let document = request.document.filter(|v| !v.trim().is_empty());
    let document = match (source.as_deref(), document) {
        (Some(url), None) => crate::mcp_api::fetch_document(url).await?,
        (None, Some(document)) => document,
        _ => return Err("Provide either a document URL or JSON/YAML text".into()),
    };
    tokio::task::spawn_blocking(move || {
        crate::mcp_api::compile_document(&document, source.as_deref(), request.base_url.as_deref())
    })
    .await
    .map_err(|_| "API document processing failed".to_string())?
}

/// Returns a standard stdio spec; persistence and sync remain with McpService.
#[tauri::command]
pub fn build_mcp_api_spec(
    tools: Vec<crate::mcp_api::ApiTool>,
    headers: std::collections::BTreeMap<String, String>,
) -> Result<serde_json::Value, String> {
    crate::mcp_api::validate_headers(&headers)?;
    let encoded =
        crate::mcp_api::encode_profile(&crate::mcp_api::ApiProfile { version: 1, tools })?;
    let executable = crate::mcp_api::runtime_executable(
        std::env::current_exe().map_err(|_| "Cannot locate CC Switch executable")?,
        if cfg!(target_os = "linux") {
            std::env::var_os("APPIMAGE").map(std::path::PathBuf::from)
        } else {
            None
        },
        cfg!(target_os = "linux") && std::env::var_os("FLATPAK_ID").is_some(),
    )?;
    let headers = serde_json::to_string(&headers).map_err(|_| "Cannot encode request headers")?;
    if headers.len() > 16 * 1024 {
        return Err("Static headers exceed 16 KiB".into());
    }
    if headers.len() + encoded.len() > crate::mcp_api::MAX_ENCODED {
        return Err("Combined API definition and headers exceed 24 KiB; select fewer tools".into());
    }
    Ok(
        serde_json::json!({"type":"stdio","command":executable.to_str().ok_or("Executable path is not UTF-8")?,"args":["--mcp-api"],
        "env":{crate::mcp_api::CONFIG_ENV:encoded,crate::mcp_api::HEADERS_ENV:headers}}),
    )
}

#[tauri::command]
pub fn decode_mcp_api_spec(spec: serde_json::Value) -> Result<crate::mcp_api::ApiPreview, String> {
    let profile = crate::mcp_api::decode_profile(
        spec["env"][crate::mcp_api::CONFIG_ENV]
            .as_str()
            .ok_or("API profile is missing")?,
    )?;
    Ok(crate::mcp_api::ApiPreview {
        title: "API".into(),
        tools: profile.tools,
        skipped: Vec::new(),
    })
}

/// 获取 Claude MCP 状态
#[tauri::command]
pub async fn get_claude_mcp_status() -> Result<claude_mcp::McpStatus, String> {
    claude_mcp::get_mcp_status().map_err(|e| e.to_string())
}

/// 读取 mcp.json 文本内容
#[tauri::command]
pub async fn read_claude_mcp_config() -> Result<Option<String>, String> {
    claude_mcp::read_mcp_json().map_err(|e| e.to_string())
}

/// 新增或更新一个 MCP 服务器条目
#[tauri::command]
pub async fn upsert_claude_mcp_server(id: String, spec: serde_json::Value) -> Result<bool, String> {
    claude_mcp::upsert_mcp_server(&id, spec).map_err(|e| e.to_string())
}

/// 删除一个 MCP 服务器条目
#[tauri::command]
pub async fn delete_claude_mcp_server(id: String) -> Result<bool, String> {
    claude_mcp::delete_mcp_server(&id).map_err(|e| e.to_string())
}

/// 校验命令是否在 PATH 中可用（不执行）
#[tauri::command]
pub async fn validate_mcp_command(cmd: String) -> Result<bool, String> {
    claude_mcp::validate_command_in_path(&cmd).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct McpConfigResponse {
    pub config_path: String,
    pub servers: HashMap<String, serde_json::Value>,
}

/// 获取 MCP 配置（来自 ~/.cc-switch/config.json）
use std::str::FromStr;

#[tauri::command]
#[allow(deprecated)] // 兼容层命令，内部调用已废弃的 Service 方法
pub async fn get_mcp_config(
    state: State<'_, AppState>,
    app: String,
) -> Result<McpConfigResponse, String> {
    let config_path = crate::config::get_app_config_path()
        .to_string_lossy()
        .to_string();
    let app_ty = AppType::from_str(&app).map_err(|e| e.to_string())?;
    let servers = McpService::get_servers(&state, app_ty).map_err(|e| e.to_string())?;
    Ok(McpConfigResponse {
        config_path,
        servers,
    })
}

/// 在 config.json 中新增或更新一个 MCP 服务器定义
/// [已废弃] 该命令仍然使用旧的分应用API，会转换为统一结构
#[tauri::command]
pub async fn upsert_mcp_server_in_config(
    state: State<'_, AppState>,
    app: String,
    id: String,
    spec: serde_json::Value,
    sync_other_side: Option<bool>,
) -> Result<bool, String> {
    use crate::app_config::McpServer;

    let app_ty = AppType::from_str(&app).map_err(|e| e.to_string())?;

    // 读取现有的服务器（如果存在）
    let existing_server = {
        let servers = state.db.get_all_mcp_servers().map_err(|e| e.to_string())?;
        servers.get(&id).cloned()
    };

    // 构建新的统一服务器结构
    let mut new_server = if let Some(mut existing) = existing_server {
        // 更新现有服务器
        existing.server = spec.clone();
        existing.apps.set_enabled_for(&app_ty, true);
        existing
    } else {
        // 创建新服务器
        let mut apps = crate::app_config::McpApps::default();
        apps.set_enabled_for(&app_ty, true);

        // 尝试从 spec 中提取 name，否则使用 id
        let name = spec
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();

        McpServer {
            id: id.clone(),
            name,
            server: spec,
            apps,
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        }
    };

    // 如果 sync_other_side 为 true，也启用其他应用
    if sync_other_side.unwrap_or(false) {
        new_server.apps.claude = true;
        new_server.apps.codex = true;
        new_server.apps.gemini = true;
        new_server.apps.opencode = true;
    }

    McpService::upsert_server(&state, new_server)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// 在 config.json 中删除一个 MCP 服务器定义
#[tauri::command]
pub async fn delete_mcp_server_in_config(
    state: State<'_, AppState>,
    _app: String, // 参数保留用于向后兼容，但在统一结构中不再需要
    id: String,
) -> Result<bool, String> {
    McpService::delete_server(&state, &id).map_err(|e| e.to_string())
}

/// 设置启用状态并同步到客户端配置（兼容层命令，同 `toggle_mcp_app`）
#[tauri::command]
pub async fn set_mcp_enabled(
    state: State<'_, AppState>,
    app: String,
    id: String,
    enabled: bool,
) -> Result<bool, String> {
    let app_ty = AppType::from_str(&app).map_err(|e| e.to_string())?;
    McpService::toggle_app_locked(&state, &id, app_ty, enabled)
        .await
        .map(|_| true)
        .map_err(|e| e.to_string())
}

// ============================================================================
// v3.7.0 新增：统一 MCP 管理命令
// ============================================================================

use crate::app_config::McpServer;

/// 获取所有 MCP 服务器（统一结构）
#[tauri::command]
pub async fn get_mcp_servers(
    state: State<'_, AppState>,
) -> Result<IndexMap<String, McpServer>, String> {
    McpService::get_all_servers(&state).map_err(|e| e.to_string())
}

/// 添加或更新 MCP 服务器
#[tauri::command]
pub async fn upsert_mcp_server(
    state: State<'_, AppState>,
    server: McpServer,
) -> Result<(), String> {
    McpService::upsert_server(&state, server).map_err(|e| e.to_string())
}

/// 删除 MCP 服务器
#[tauri::command]
pub async fn delete_mcp_server(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    McpService::delete_server(&state, &id).map_err(|e| e.to_string())
}

/// 切换 MCP 服务器在指定应用的启用状态
#[tauri::command]
pub async fn toggle_mcp_app(
    state: State<'_, AppState>,
    server_id: String,
    app: String,
    enabled: bool,
) -> Result<(), String> {
    let app_ty = AppType::from_str(&app).map_err(|e| e.to_string())?;
    McpService::toggle_app_locked(&state, &server_id, app_ty, enabled)
        .await
        .map_err(|e| e.to_string())
}

/// 从所有应用导入 MCP 服务器（复用已有的导入逻辑）
#[tauri::command]
pub async fn import_mcp_from_apps(state: State<'_, AppState>) -> Result<usize, String> {
    McpService::import_from_all_apps(&state).map_err(|e| e.to_string())
}

/// 按数据库里的开关把 MCP 重新写进各应用的 live 配置，逐应用返回结果。
///
/// `apps` 缺省或为空时同步全部受管应用。每个应用先拿它的切换锁再写，
/// 和切换供应商互斥；单个应用失败（如配置文件解析失败）不影响其余应用。
#[tauri::command]
pub async fn resync_mcp_to_apps(
    state: State<'_, AppState>,
    apps: Option<Vec<String>>,
) -> Result<Vec<crate::services::mcp::McpAppSyncOutcome>, String> {
    let targets = McpService::resync_targets(apps.as_deref()).map_err(|e| e.to_string())?;
    let mut outcomes = Vec::with_capacity(targets.len());
    for app in targets {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        outcomes.push(McpService::resync_app(&state, &app));
    }
    Ok(outcomes)
}

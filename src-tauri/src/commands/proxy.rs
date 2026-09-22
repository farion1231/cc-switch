//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::error::AppError;
use crate::proxy::types::*;
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};
use crate::store::AppState;
use std::str::FromStr;
use tauri_plugin_opener::OpenerExt;

fn require_proxy_app(app_type: &str) -> Result<crate::app_config::AppType, String> {
    let app = crate::app_config::AppType::from_str(app_type)
        .map_err(|error| format!("无效的应用类型: {error}"))?;
    if !app.supports_local_proxy() {
        return Err(format!("{} 不支持本地路由", app.as_str()));
    }
    Ok(app)
}

/// 启动代理服务器（仅启动服务，不接管 Live 配置）
#[tauri::command]
pub async fn start_proxy_server(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyServerInfo, String> {
    state.proxy_service.start().await
}

/// 停止代理服务器（仅停止服务，不恢复/清理 Live 接管状态）
#[tauri::command]
pub async fn stop_proxy_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let takeover = state.proxy_service.get_takeover_status().await?;
    if takeover.claude
        || takeover.codex
        || takeover.gemini
        || takeover.grokbuild
        || takeover.opencode
        || takeover.openclaw
    {
        return Err(
            "仍有应用处于代理接管状态，请先在设置中关闭对应应用接管后再停止本地路由。".to_string(),
        );
    }

    state.proxy_service.stop().await
}

/// 停止代理服务器（恢复 Live 配置）
#[tauri::command]
pub async fn stop_proxy_with_restore(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.proxy_service.stop_with_restore().await
}

/// 获取各应用接管状态
#[tauri::command]
pub async fn get_proxy_takeover_status(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyTakeoverStatus, String> {
    state.proxy_service.get_takeover_status().await
}

/// 为指定应用开启/关闭接管
#[tauri::command]
pub async fn set_proxy_takeover_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .proxy_service
        .set_takeover_for_app(&app_type, enabled)
        .await
}

/// 获取代理服务器状态
#[tauri::command]
pub async fn get_proxy_status(state: tauri::State<'_, AppState>) -> Result<ProxyStatus, String> {
    state.proxy_service.get_status().await
}

/// 获取代理配置
#[tauri::command]
pub async fn get_proxy_config(state: tauri::State<'_, AppState>) -> Result<ProxyConfig, String> {
    state.proxy_service.get_config().await
}

/// 更新代理配置
#[tauri::command]
pub async fn update_proxy_config(
    state: tauri::State<'_, AppState>,
    config: ProxyConfig,
) -> Result<(), String> {
    state.proxy_service.update_config(&config).await
}

// ==================== Global & Per-App Config ====================

/// 获取全局代理配置
///
/// 返回统一的全局配置字段（代理开关、监听地址、端口、日志开关）
#[tauri::command]
pub async fn get_global_proxy_config(
    state: tauri::State<'_, AppState>,
) -> Result<GlobalProxyConfig, String> {
    let db = &state.db;
    db.get_global_proxy_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新全局代理配置
///
/// 更新统一的全局配置字段，会同时更新三行（claude/codex/gemini）
#[tauri::command]
pub async fn update_global_proxy_config(
    state: tauri::State<'_, AppState>,
    config: GlobalProxyConfig,
) -> Result<(), String> {
    let db = &state.db;
    db.update_global_proxy_config(config.clone())
        .await
        .map_err(|e| e.to_string())?;

    state
        .proxy_service
        .apply_logging_runtime(config.enable_logging)
        .await;
    // 同步 request_log_max_sessions 到运行中的代理 state（如果服务已在跑）
    state
        .proxy_service
        .apply_request_log_max_sessions_runtime(config.request_log_max_sessions)
        .await;

    Ok(())
}

// ==================== Request-Log 查看器 ====================

/// request-log 会话文件元信息
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestLogFileMeta {
    pub app_type: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub modified_at_ms: i64,
    /// 复用 session_manager 解析出的会话标题（custom-title / 首条用户消息 /
    /// 项目目录名），匹配不到为 None
    pub session_title: Option<String>,
}

const REQUEST_LOG_MAX_LIMIT: u32 = 200;
const REQUEST_LOG_DEFAULT_LIMIT: u32 = 50;

/// 列表行的轻量字段（不含请求/响应体）。列表页只需这些 + 展示用 token 数
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestLogListRow {
    pub line_no: u64,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub method: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub duration_ms: Option<u64>,
    pub is_streaming: bool,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    /// prompt/completion token 数（从 responseBody.usage 提取，展示列用）
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

/// request-log 记录分页结果（轻量行；详情展开时按 lineNo 单独取整条）
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestLogRecordsPage {
    pub records: Vec<ProxyRequestLogListRow>,
    pub total: u64,
}

/// 从响应体提取 token 用量（Anthropic / OpenAI / Gemini 三种形态）
fn extract_usage_tokens(body: &serde_json::Value) -> (Option<u64>, Option<u64>) {
    let read = |paths: &[(&str, &str)]| -> Option<u64> {
        for (a, b) in paths {
            if let Some(v) = body
                .get(*a)
                .and_then(|u| u.get(*b))
                .and_then(|n| n.as_u64())
            {
                return Some(v);
            }
        }
        None
    };
    let prompt = read(&[
        ("usage", "input_tokens"),
        ("usage", "prompt_tokens"),
        ("usageMetadata", "promptTokenCount"),
    ]);
    let completion = read(&[
        ("usage", "output_tokens"),
        ("usage", "completion_tokens"),
        ("usageMetadata", "candidatesTokenCount"),
    ]);
    (prompt, completion)
}

/// 单个 JSONL 文件的解析缓存：轻量行 + 每行的字节偏移（取详情时 seek 读）。
/// 以 (mtime, size) 为有效性凭据——文件被追加/轮换后自动失效重扫。
struct RequestLogFileIndex {
    rows: Vec<ProxyRequestLogListRow>,
    /// 与 rows 同长：每行在文件中的起始字节偏移
    offsets: Vec<u64>,
}

/// 进程内缓存。key 为 (app_type, file_name)。
/// 单个 1GB 文件的行索引（几十万条 × ~150B）约几十 MB，
/// 上限 8 个文件防止极端多文件场景内存失控，超出按插入序逐出最旧的。
/// 缓存值：(mtime, size, 行索引)——前两项是有效性凭据
type RequestLogCacheValue = (std::time::SystemTime, u64, RequestLogFileIndex);
type RequestLogCache =
    std::sync::Mutex<std::collections::HashMap<(String, String), RequestLogCacheValue>>;

/// 进程内缓存。key 为 (app_type, file_name)。
/// 单个 1GB 文件的行索引（几十万条 × ~150B）约几十 MB，
/// 上限 8 个文件防止极端多文件场景内存失控，超出逐出任意一项。
static REQUEST_LOG_INDEX_CACHE: std::sync::OnceLock<RequestLogCache> = std::sync::OnceLock::new();

fn index_cache() -> &'static RequestLogCache {
    REQUEST_LOG_INDEX_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

const REQUEST_LOG_INDEX_CACHE_MAX_FILES: usize = 8;

fn take_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|s| s.as_str()).map(|s| s.to_string())
}

/// 解析单条记录为轻量行（失败返回 None，与旧版「跳过坏行」语义一致）
fn parse_list_row(line_no: u64, value: &serde_json::Value) -> Option<ProxyRequestLogListRow> {
    let body = value.get("responseBody");
    let (prompt_tokens, completion_tokens) = match body {
        Some(b) => extract_usage_tokens(b),
        None => (None, None),
    };
    Some(ProxyRequestLogListRow {
        line_no,
        start_time: take_str(value, "startTime"),
        end_time: take_str(value, "endTime"),
        method: take_str(value, "method"),
        endpoint: take_str(value, "endpoint"),
        model: take_str(value, "model"),
        duration_ms: value.get("durationMs").and_then(|n| n.as_u64()),
        is_streaming: value
            .get("isStreaming")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        status_code: value
            .get("statusCode")
            .and_then(|n| n.as_u64())
            .and_then(|n| u16::try_from(n).ok()),
        error: take_str(value, "error"),
        prompt_tokens,
        completion_tokens,
    })
}

/// 取（或构建）指定文件的行索引。文件 mtime/size 变化时重扫。
fn get_file_index(
    app_type: &str,
    file_name: &str,
) -> Result<(std::time::SystemTime, u64, RequestLogFileIndex), String> {
    let dir = crate::config::get_proxy_request_log_dir().map_err(|e| e.to_string())?;
    let app_dir = dir.join(crate::proxy::request_logger::sanitize_path_component(
        app_type,
    ));
    let path = app_dir.join(file_name);

    let meta = std::fs::metadata(&path).map_err(|e| format!("打开日志文件失败: {e}"))?;
    let mtime = meta
        .modified()
        .map_err(|e| format!("读取文件时间失败: {e}"))?;
    let size = meta.len();

    let cache_key = (app_type.to_string(), file_name.to_string());
    {
        let cache = index_cache().lock().unwrap();
        if let Some((c_mtime, c_size, index)) = cache.get(&cache_key) {
            if *c_mtime == mtime && *c_size == size {
                return Ok((*c_mtime, *c_size, clone_index(index)));
            }
        }
    }

    // 全量扫描一遍（只在文件变化后的首次查询发生）
    let file = std::fs::File::open(&path).map_err(|e| format!("打开日志文件失败: {e}"))?;
    let reader = std::io::BufReader::new(file);
    let mut rows = Vec::new();
    let mut offsets = Vec::new();
    let mut line_no: u64 = 0;
    let mut offset: u64 = 0;
    for line in std::io::BufRead::lines(reader) {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line_start = offset;
        offset += line.len() as u64 + 1; // +1 为换行符
        if line.trim().is_empty() {
            continue;
        }
        line_no += 1;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(row) = parse_list_row(line_no, &value) {
                rows.push(row);
                offsets.push(line_start);
            }
        }
    }

    let index = RequestLogFileIndex { rows, offsets };
    let mut cache = index_cache().lock().unwrap();
    // 上限逐出：HashMap 无序，插入序近似用「先移除任一超额项」——遍历移除第一个即可
    if !cache.contains_key(&cache_key) && cache.len() >= REQUEST_LOG_INDEX_CACHE_MAX_FILES {
        if let Some(k) = cache.keys().next().cloned() {
            cache.remove(&k);
        }
    }
    cache.insert(cache_key, (mtime, size, clone_index(&index)));
    Ok((mtime, size, index))
}

/// 行索引会从缓存中 clone 出来（几万条 × 小结构，微秒级），
/// 避免在持锁状态下做分页/搜索的长计算
fn clone_index(index: &RequestLogFileIndex) -> RequestLogFileIndex {
    RequestLogFileIndex {
        rows: index.rows.clone(),
        offsets: index.offsets.clone(),
    }
}

/// 列出所有应用的 request-log 会话文件（按修改时间降序）
///
/// 会话标题复用 `session_manager::scan_sessions()` 的解析结果
/// （读各 CLI 的会话文件头尾行提取 custom-title 等），按
/// `(app_type, session_id)` 匹配；request-log 文件名中的 session-id
/// 与 CLI 会话 UUID 一致（Codex 的 `codex_` 前缀在落盘时已剥离）。
#[tauri::command]
pub async fn list_proxy_request_log_files() -> Result<Vec<ProxyRequestLogFileMeta>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let titles = crate::session_manager::scan_sessions()
            .into_iter()
            .filter_map(|s| {
                let title = s.title?;
                Some(((s.provider_id, s.session_id), title))
            })
            .collect::<std::collections::HashMap<(String, String), String>>();
        scan_request_log_files(&titles)
    })
    .await
    .map_err(|e| format!("扫描日志文件失败: {e}"))?
}

fn scan_request_log_files(
    titles: &std::collections::HashMap<(String, String), String>,
) -> Result<Vec<ProxyRequestLogFileMeta>, String> {
    let dir = match crate::config::get_proxy_request_log_dir() {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };

    let mut files = Vec::new();
    let app_dirs = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(Vec::new()),
    };

    for app_dir in app_dirs.flatten() {
        let app_path = app_dir.path();
        if !app_path.is_dir() {
            continue;
        }
        let Some(app_type) = app_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
        else {
            continue;
        };
        let entries = match std::fs::read_dir(&app_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(file_name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified_at_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            files.push(ProxyRequestLogFileMeta {
                app_type: app_type.clone(),
                session_title: file_session_title(&app_type, &file_name, titles),
                file_name,
                size_bytes: meta.len(),
                modified_at_ms,
            });
        }
    }

    files.sort_by_key(|f| std::cmp::Reverse(f.modified_at_ms));
    Ok(files)
}

/// 从 request-log 文件名 `<session-id>.jsonl` 提取 session-id 并查标题映射。
fn file_session_title(
    app_type: &str,
    file_name: &str,
    titles: &std::collections::HashMap<(String, String), String>,
) -> Option<String> {
    if file_name == "unknown-session.jsonl" {
        return None;
    }
    let session_id = file_name.strip_suffix(".jsonl")?;
    titles
        .get(&(app_type.to_string(), session_id.to_string()))
        .cloned()
}

/// 读取指定会话文件的 request-log 记录（分页，轻量行）
///
/// - `offset`：分页偏移。`order="desc"`（默认）跳过最新的 N 条；`order="asc"` 跳过最旧的 N 条
/// - `limit`：本页条数（默认 50，上限 200）
/// - `search`：可选关键词，对列表轻量字段（时间/方法/端点/模型/错误）做小写子串匹配
/// - `before_line_no` / `after_line_no`：可选行号游标，用于滚动加载时避免文件追加造成 offset 漂移
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_proxy_request_log_records(
    app_type: String,
    file_name: String,
    offset: u32,
    limit: Option<u32>,
    order: Option<String>,
    search: Option<String>,
    before_line_no: Option<u64>,
    after_line_no: Option<u64>,
) -> Result<ProxyRequestLogRecordsPage, String> {
    require_proxy_app(&app_type)?;
    let mut offset = offset.min(100_000);
    let limit = limit
        .unwrap_or(REQUEST_LOG_DEFAULT_LIMIT)
        .clamp(1, REQUEST_LOG_MAX_LIMIT);
    let order = order.unwrap_or_else(|| "desc".to_string());
    if order != "desc" && order != "asc" {
        return Err(format!("无效的排序方向: {order}"));
    }
    let search = search
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if before_line_no.is_some() || after_line_no.is_some() {
        offset = 0;
    }

    tauri::async_runtime::spawn_blocking(move || {
        read_request_log_records(
            &app_type,
            &file_name,
            offset,
            limit,
            &order,
            search.as_deref(),
            before_line_no,
            after_line_no,
        )
    })
    .await
    .map_err(|e| format!("读取日志失败: {e}"))?
}

/// 路径安全：file_name 必须经 get_file_index 校验（metadata 打不开即报错），
/// 不接受任意路径。行索引有进程内缓存（mtime/size 失效），翻页/搜索在缓存上行进。
#[allow(clippy::too_many_arguments)]
fn read_request_log_records(
    app_type: &str,
    file_name: &str,
    offset: u32,
    limit: u32,
    order: &str,
    search: Option<&str>,
    before_line_no: Option<u64>,
    after_line_no: Option<u64>,
) -> Result<ProxyRequestLogRecordsPage, String> {
    let (_, _, index) = get_file_index(app_type, file_name)?;

    // 搜索匹配轻量字段（时间/方法/端点/模型/错误/状态码）；
    // body 内容级搜索不再支持——列表行不携带 body，这是轻量化换来的取舍
    let search_lower = search.map(|s| s.to_lowercase());
    let matches = |row: &ProxyRequestLogListRow| -> bool {
        let Some(kw) = &search_lower else {
            return true;
        };
        let hay = format!(
            "{} {} {} {} {} {} {}",
            row.start_time.as_deref().unwrap_or(""),
            row.end_time.as_deref().unwrap_or(""),
            row.method.as_deref().unwrap_or(""),
            row.endpoint.as_deref().unwrap_or(""),
            row.model.as_deref().unwrap_or(""),
            row.error.as_deref().unwrap_or(""),
            row.status_code.map(|c| c.to_string()).unwrap_or_default(),
        )
        .to_lowercase();
        hay.contains(kw)
    };

    // 行号游标（rows 按文件序构建，数组下标 i 的行号 = i+1）：
    // desc 模式 before_line_no 取「该行号之前（更旧）」；asc 模式 after_line_no 取「该行号之后（更新）」
    let in_range = |i: usize| -> bool {
        let line_no = (i + 1) as u64;
        if let Some(b) = before_line_no {
            if line_no >= b {
                return false;
            }
        }
        if let Some(a) = after_line_no {
            if line_no <= a {
                return false;
            }
        }
        true
    };

    let total = index
        .rows
        .iter()
        .enumerate()
        .filter(|(i, row)| in_range(*i) && matches(row))
        .count() as u64;

    let iter = (0..index.rows.len()).filter(|&i| in_range(i) && matches(&index.rows[i]));
    let selected: Vec<ProxyRequestLogListRow> = if order == "desc" {
        iter.rev()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|i| index.rows[i].clone())
            .collect()
    } else {
        iter.skip(offset as usize)
            .take(limit as usize)
            .map(|i| index.rows[i].clone())
            .collect()
    };

    Ok(ProxyRequestLogRecordsPage {
        records: selected,
        total,
    })
}

/// 读取单条完整记录（展开详情时按 lineNo 定位，seek + 读一行，不进缓存）
#[tauri::command]
pub async fn get_proxy_request_log_record(
    app_type: String,
    file_name: String,
    line_no: u64,
) -> Result<serde_json::Value, String> {
    require_proxy_app(&app_type)?;
    tauri::async_runtime::spawn_blocking(move || {
        read_request_log_record(&app_type, &file_name, line_no)
    })
    .await
    .map_err(|e| format!("读取日志失败: {e}"))?
}

fn read_request_log_record(
    app_type: &str,
    file_name: &str,
    line_no: u64,
) -> Result<serde_json::Value, String> {
    let (_, _, index) = get_file_index(app_type, file_name)?;
    let pos = (line_no as usize)
        .checked_sub(1)
        .filter(|&p| p < index.rows.len() && index.rows[p].line_no == line_no)
        .ok_or_else(|| format!("记录不存在: {line_no}"))?;

    let dir = crate::config::get_proxy_request_log_dir().map_err(|e| e.to_string())?;
    let app_dir = dir.join(crate::proxy::request_logger::sanitize_path_component(
        app_type,
    ));
    let mut file = std::fs::File::open(app_dir.join(file_name))
        .map_err(|e| format!("打开日志文件失败: {e}"))?;

    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    file.seek(SeekFrom::Start(index.offsets[pos]))
        .map_err(|e| format!("定位日志记录失败: {e}"))?;
    let mut line = String::new();
    BufReader::new(file)
        .read_line(&mut line)
        .map_err(|e| format!("读取日志记录失败: {e}"))?;

    let mut value = serde_json::from_str::<serde_json::Value>(&line)
        .map_err(|e| format!("解析日志记录失败: {e}"))?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("lineNo".to_string(), serde_json::json!(line_no));
    }
    Ok(value)
}

/// 在系统文件管理器中打开 request-log 日志目录
#[tauri::command]
pub async fn open_proxy_request_log_dir(app_handle: tauri::AppHandle) -> Result<(), String> {
    let dir = crate::config::get_proxy_request_log_dir().map_err(|e| e.to_string())?;
    app_handle
        .opener()
        .open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开日志目录失败: {e}"))
}

/// 获取指定应用的代理配置
///
/// 返回应用级配置（enabled、auto_failover、超时、熔断器等）
#[tauri::command]
pub async fn get_proxy_config_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<AppProxyConfig, String> {
    require_proxy_app(&app_type)?;
    let db = &state.db;
    db.get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 更新指定应用的代理配置
///
/// 更新应用级配置（enabled、auto_failover、超时、熔断器等）
#[tauri::command]
pub async fn update_proxy_config_for_app(
    state: tauri::State<'_, AppState>,
    config: AppProxyConfig,
) -> Result<(), String> {
    let db = &state.db;
    let app_type = config.app_type.clone();
    require_proxy_app(&app_type)?;
    let circuit_config = CircuitBreakerConfig::from(&config);

    db.update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;

    state
        .proxy_service
        .update_circuit_breaker_config_for_app(&app_type, circuit_config)
        .await
}

async fn get_default_cost_multiplier_internal(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    let db = &state.db;
    db.get_default_cost_multiplier(app_type).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn get_default_cost_multiplier_test_hook(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    get_default_cost_multiplier_internal(state, app_type).await
}

/// 获取默认成本倍率
#[tauri::command]
pub async fn get_default_cost_multiplier(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<String, String> {
    get_default_cost_multiplier_internal(&state, &app_type)
        .await
        .map_err(|e| e.to_string())
}

async fn set_default_cost_multiplier_internal(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    let db = &state.db;
    db.set_default_cost_multiplier(app_type, value).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn set_default_cost_multiplier_test_hook(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    set_default_cost_multiplier_internal(state, app_type, value).await
}

/// 设置默认成本倍率
#[tauri::command]
pub async fn set_default_cost_multiplier(
    state: tauri::State<'_, AppState>,
    app_type: String,
    value: String,
) -> Result<(), String> {
    set_default_cost_multiplier_internal(&state, &app_type, &value)
        .await
        .map_err(|e| e.to_string())
}

async fn get_pricing_model_source_internal(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    let db = &state.db;
    db.get_pricing_model_source(app_type).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn get_pricing_model_source_test_hook(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    get_pricing_model_source_internal(state, app_type).await
}

/// 获取计费模式来源
#[tauri::command]
pub async fn get_pricing_model_source(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<String, String> {
    get_pricing_model_source_internal(&state, &app_type)
        .await
        .map_err(|e| e.to_string())
}

async fn set_pricing_model_source_internal(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    let db = &state.db;
    db.set_pricing_model_source(app_type, value).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn set_pricing_model_source_test_hook(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    set_pricing_model_source_internal(state, app_type, value).await
}

/// 设置计费模式来源
#[tauri::command]
pub async fn set_pricing_model_source(
    state: tauri::State<'_, AppState>,
    app_type: String,
    value: String,
) -> Result<(), String> {
    set_pricing_model_source_internal(&state, &app_type, &value)
        .await
        .map_err(|e| e.to_string())
}

/// 检查代理服务器是否正在运行
#[tauri::command]
pub async fn is_proxy_running(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    Ok(state.proxy_service.is_running().await)
}

/// 检查是否处于 Live 接管模式
#[tauri::command]
pub async fn is_live_takeover_active(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state.proxy_service.is_takeover_active().await
}

/// 代理模式下切换供应商（热切换）
#[tauri::command]
pub async fn switch_proxy_provider(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    let app = require_proxy_app(&app_type)?;
    // Codex official account cards can use the client's native OpenAI login
    // through takeover. Other apps' official providers remain blocked.
    let provider = state
        .db
        .get_provider_by_id(&provider_id, &app_type)
        .map_err(|e| format!("读取供应商失败: {e}"))?
        .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
    if provider.category.as_deref() == Some("official")
        && !crate::services::provider::official_provider_supports_proxy_takeover(&app, &provider)
    {
        return Err(
            "代理接管模式下不能切换到官方供应商 (Cannot switch to official provider during proxy takeover)"
                .to_string(),
        );
    }

    state
        .proxy_service
        .switch_proxy_target(&app_type, &provider_id)
        .await
}

// ==================== 故障转移相关命令 ====================

/// 获取供应商健康状态
#[tauri::command]
pub async fn get_provider_health(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<ProviderHealth, String> {
    require_proxy_app(&app_type)?;
    let db = &state.db;
    db.get_provider_health(&provider_id, &app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 重置熔断器
///
/// 重置后会检查是否应该切回队列中优先级更高的供应商：
/// 1. 检查自动故障转移是否开启
/// 2. 如果恢复的供应商在队列中优先级更高（queue_order 更小），则自动切换
#[tauri::command]
pub async fn reset_circuit_breaker(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<(), String> {
    require_proxy_app(&app_type)?;
    // 1. 重置数据库健康状态
    let db = &state.db;
    db.update_provider_health(&provider_id, &app_type, true, None)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 如果代理正在运行，重置内存中的熔断器状态
    state
        .proxy_service
        .reset_provider_circuit_breaker(&provider_id, &app_type)
        .await?;

    // 3. 检查是否应该切回优先级更高的供应商（从 proxy_config 表读取）
    // 只有当该应用已被代理接管（enabled=true）且开启了自动故障转移时才执行
    let (app_enabled, auto_failover_enabled) = match db.get_proxy_config_for_app(&app_type).await {
        Ok(config) => (config.enabled, config.auto_failover_enabled),
        Err(e) => {
            log::error!("[{app_type}] Failed to read proxy_config: {e}, defaulting to disabled");
            (false, false)
        }
    };

    if app_enabled && auto_failover_enabled && state.proxy_service.is_running().await {
        // 获取当前供应商 ID
        let current_id = db
            .get_current_provider(&app_type)
            .map_err(|e| e.to_string())?;

        if let Some(current_id) = current_id {
            // 获取故障转移队列
            let queue = db
                .get_failover_queue(&app_type)
                .map_err(|e| e.to_string())?;

            // 找到恢复的供应商和当前供应商在队列中的位置（使用 sort_index）
            let restored_order = queue
                .iter()
                .find(|item| item.provider_id == provider_id)
                .and_then(|item| item.sort_index);

            let current_order = queue
                .iter()
                .find(|item| item.provider_id == current_id)
                .and_then(|item| item.sort_index);

            // 如果恢复的供应商优先级更高（sort_index 更小），则切换
            if let (Some(restored), Some(current)) = (restored_order, current_order) {
                if restored < current {
                    log::info!(
                        "[Recovery] 供应商 {provider_id} 已恢复且优先级更高 (P{restored} vs P{current})，自动切换"
                    );

                    // 获取供应商名称用于日志和事件
                    let provider_name = db
                        .get_all_providers(&app_type)
                        .ok()
                        .and_then(|providers| providers.get(&provider_id).map(|p| p.name.clone()))
                        .unwrap_or_else(|| provider_id.clone());

                    // 创建故障转移切换管理器并执行切换
                    let switch_manager =
                        crate::proxy::failover_switch::FailoverSwitchManager::new(db.clone());
                    if let Err(e) = switch_manager
                        .try_switch(Some(&app_handle), &app_type, &provider_id, &provider_name)
                        .await
                    {
                        log::error!("[Recovery] 自动切换失败: {e}");
                    }
                }
            }
        }
    }

    Ok(())
}

/// 获取熔断器配置
#[tauri::command]
pub async fn get_circuit_breaker_config(
    state: tauri::State<'_, AppState>,
) -> Result<CircuitBreakerConfig, String> {
    let db = &state.db;
    db.get_circuit_breaker_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新熔断器配置
#[tauri::command]
pub async fn update_circuit_breaker_config(
    state: tauri::State<'_, AppState>,
    config: CircuitBreakerConfig,
) -> Result<(), String> {
    let db = &state.db;

    // 1. 更新数据库配置
    db.update_circuit_breaker_config(&config)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 如果代理正在运行，热更新内存中的熔断器配置
    state
        .proxy_service
        .update_circuit_breaker_configs(config)
        .await?;

    Ok(())
}

/// 获取熔断器统计信息（仅当代理服务器运行时）
#[tauri::command]
pub async fn get_circuit_breaker_stats(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<Option<CircuitBreakerStats>, String> {
    require_proxy_app(&app_type)?;
    // 这个功能需要访问运行中的代理服务器的内存状态
    // 目前先返回 None，后续可以通过 ProxyService 暴露接口来实现
    let _ = (state, provider_id, app_type);
    Ok(None)
}

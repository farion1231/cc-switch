#![allow(non_snake_case)]

use serde_json::{json, Value};

use crate::error::AppError;
use crate::settings::{self, SharedMemorySettings};

fn require_settings() -> Result<SharedMemorySettings, AppError> {
    let settings = settings::get_shared_memory_settings().unwrap_or_default();
    settings.validate()?;
    Ok(settings)
}

fn worker_api_url(settings: &SharedMemorySettings) -> String {
    format!("{}/api", settings.url.trim_end_matches('/'))
}

fn parse_cloud_response(status: reqwest::StatusCode, body: String) -> Result<Value, AppError> {
    if !status.is_success() {
        return Err(AppError::Message(format!(
            "共享记忆云端返回 {status}: {body}"
        )));
    }
    let data: Value = serde_json::from_str(&body)
        .map_err(|e| AppError::Message(format!("共享记忆响应解析失败: {e}")))?;
    if data.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let error = data
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        return Err(AppError::Message(format!("共享记忆云端返回错误: {error}")));
    }
    Ok(data)
}

/// 读取共享记忆设置（含令牌，仅供本机 UI 使用）。
#[tauri::command]
pub fn shared_memory_get_settings() -> Result<SharedMemorySettings, String> {
    Ok(settings::get_shared_memory_settings().unwrap_or_default())
}

/// 保存共享记忆设置；令牌留空时保留已有令牌。
#[tauri::command]
pub fn shared_memory_save_settings(mut incoming: SharedMemorySettings) -> Result<Value, String> {
    if let Some(existing) = settings::get_shared_memory_settings() {
        if incoming.token.is_empty() && !existing.token.is_empty() {
            incoming.token = existing.token;
        }
    }
    incoming.normalize();
    incoming.validate().map_err(|e| e.to_string())?;
    settings::set_shared_memory_settings(Some(incoming)).map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

/// 从云端拉取共享记忆（GET {url}/api，返回 { ok, updatedAt, bytes, content }）。
#[tauri::command]
pub async fn shared_memory_fetch() -> Result<Value, String> {
    let settings = require_settings().map_err(|e| e.to_string())?;
    let response = crate::proxy::http_client::get()
        .get(worker_api_url(&settings))
        .send()
        .await
        .map_err(|e| AppError::Message(format!("共享记忆请求失败: {e}")).to_string())?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| AppError::Message(format!("共享记忆响应读取失败: {e}")).to_string())?;
    parse_cloud_response(status, body).map_err(|e| e.to_string())
}

/// 推送共享记忆到云端（PUT {url}/api，X-Auth-Token）。
#[tauri::command]
pub async fn shared_memory_push(content: String) -> Result<Value, String> {
    let settings = require_settings().map_err(|e| e.to_string())?;
    if settings.token.trim().is_empty() {
        return Err(AppError::localized(
            "sharedMemory.token.required",
            "请先填写共享记忆访问令牌",
            "Shared memory access token is required.",
        )
        .to_string());
    }
    let token = settings.token.trim().to_string();
    let response = crate::proxy::http_client::get()
        .put(worker_api_url(&settings))
        .header("X-Auth-Token", token)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(content)
        .send()
        .await
        .map_err(|e| AppError::Message(format!("共享记忆请求失败: {e}")).to_string())?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| AppError::Message(format!("共享记忆响应读取失败: {e}")).to_string())?;
    let data = parse_cloud_response(status, body).map_err(|e| e.to_string())?;

    // 记录最近一次同步元数据，便于面板展示状态。
    let updated_at = data
        .get("updatedAt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let bytes = data.get("bytes").and_then(|v| v.as_u64());
    if updated_at.is_some() || bytes.is_some() {
        if let Some(mut sm) = settings::get_shared_memory_settings() {
            sm.last_sync_at = updated_at;
            sm.last_sync_bytes = bytes;
            sm.enabled = true;
            let _ = settings::set_shared_memory_settings(Some(sm));
        }
    }
    Ok(data)
}

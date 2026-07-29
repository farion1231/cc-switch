use serde::Serialize;
use serde_json::Value;
use tauri::{Emitter, Manager, State};

use crate::remote::models::{RemoteRuntimeSnapshot, RemoteTargetConfig};
use crate::remote::runtime::{RemoteRuntimeError, RemoteRuntimeState};
use crate::remote::ssh::RemotePlatform;
use crate::remote::ssh_config::{discover_current_user_ssh_targets, DiscoveredSshTarget};

#[tauri::command]
pub fn remote_discover_ssh_targets() -> Result<Vec<DiscoveredSshTarget>, String> {
    // 发现过程只读取本机用户配置，不依赖当前远程运行时，也不会发起网络连接。
    discover_current_user_ssh_targets().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn remote_list_targets(
    state: State<'_, RemoteRuntimeState>,
) -> Result<Vec<RemoteTargetConfig>, String> {
    state.list_targets().map_err(serialize_error)
}

#[tauri::command]
pub fn remote_upsert_target(
    state: State<'_, RemoteRuntimeState>,
    target: RemoteTargetConfig,
) -> Result<bool, String> {
    state.upsert_target(target).map_err(serialize_error)?;
    Ok(true)
}

#[tauri::command]
pub async fn remote_test_target(
    app_handle: tauri::AppHandle,
    target: RemoteTargetConfig,
) -> Result<RemotePlatform, String> {
    // SSH 进程可能等待网络超时，必须离开 Tauri 命令线程，避免阻塞其他本地操作。
    tauri::async_runtime::spawn_blocking(move || {
        app_handle
            .state::<RemoteRuntimeState>()
            .test_target(&target)
            .map_err(serialize_error)
    })
    .await
    .map_err(|error| format!("远程连接测试任务失败: {error}"))?
}

#[tauri::command]
pub fn remote_delete_target(
    state: State<'_, RemoteRuntimeState>,
    #[allow(non_snake_case)] targetId: String,
) -> Result<bool, String> {
    state.delete_target(&targetId).map_err(serialize_error)
}

#[tauri::command]
pub fn remote_get_runtime_snapshot(
    state: State<'_, RemoteRuntimeState>,
) -> Result<RemoteRuntimeSnapshot, String> {
    state.snapshot().map_err(serialize_error)
}

#[tauri::command]
pub async fn remote_set_active_target(
    app_handle: tauri::AppHandle,
    #[allow(non_snake_case)] targetId: Option<String>,
) -> Result<RemoteRuntimeSnapshot, String> {
    let task_handle = app_handle.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = task_handle.state::<RemoteRuntimeState>();
        match targetId {
            Some(target_id) => state.connect_target(&target_id),
            None => state.use_local(),
        }
    })
    .await
    .map_err(|error| format!("远程环境切换任务失败: {error}"))?;

    // 即使连接失败也广播最终快照，让前端进入明确的离线态而不是一直显示连接中。
    let snapshot = app_handle
        .state::<RemoteRuntimeState>()
        .snapshot()
        .map_err(serialize_error)?;
    let _ = app_handle.emit("remote-runtime-status", &snapshot);
    result.map_err(serialize_error)
}

#[tauri::command]
pub async fn remote_invoke(
    app_handle: tauri::AppHandle,
    command: String,
    #[allow(non_snake_case)] args: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app_handle
            .state::<RemoteRuntimeState>()
            .invoke_remote(&command, args)
            .map_err(serialize_error)
    })
    .await
    .map_err(|error| format!("远程命令任务失败: {error}"))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteErrorPayload<'a> {
    code: &'static str,
    message: &'a str,
}

fn serialize_error(error: RemoteRuntimeError) -> String {
    let message = error.to_string();
    serde_json::to_string(&RemoteErrorPayload {
        code: error.code(),
        message: &message,
    })
    .unwrap_or(message)
}

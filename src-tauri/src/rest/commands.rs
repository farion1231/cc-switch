//! REST 控制面配置命令（`get_rest_config` / `set_rest_config`）。
//!
//! **`rest_api` feature-gated**（D-7）：feature off 时不编译、不注册。
//! 读写 `settings.json` 的 `rest_api` 字段（不走 wholesale `save_settings`）。
//! `set` 内：首次启用空 token 则生成、`0.0.0.0` 空 token 拒绝、触发 RestService 重启。

use tauri::State;

use crate::error::AppError;
use crate::rest::config::RestConfig;
use crate::rest::service::RestService;
use crate::settings;
use crate::store::AppState;

/// 生成 32 字节随机 token（base64url，无 padding）。
fn generate_token() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::prelude::*;
    let mut bytes = [0u8; 32];
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    bytes[..16].copy_from_slice(a_bytes);
    bytes[16..].copy_from_slice(b_bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// 读取 REST 配置（从 `settings.json`，经共享 RwLock 反映运行期热更新值）。
#[tauri::command]
pub async fn get_rest_config(rest: State<'_, RestService>) -> Result<RestConfig, String> {
    Ok(rest.shared_config().read().await.clone())
}

/// 写入 REST 配置。
///
/// - `enabled=true` 且 token 空 → 生成新 token，返回值含生成 token（一次性）。
/// - `bind=0.0.0.0` 且 token 空 → 拒绝（R-010/A-012）。
/// - 地址/端口变更或 enabled 切换 → 触发 RestService restart/stop。
#[tauri::command]
pub async fn set_rest_config(
    _state: State<'_, AppState>,
    rest: State<'_, RestService>,
    config: RestConfig,
) -> Result<RestConfig, String> {
    // 1. 校验 0.0.0.0 必须有 token
    if config.is_bind_all_without_token() {
        return Err("绑定 0.0.0.0 时必须设置 Bearer token".to_string());
    }
    // 2. 首次启用且 token 空 → 生成
    let mut config = config;
    if config.enabled && config.token.trim().is_empty() {
        config.token = generate_token();
    }
    // 3. 持久化到 settings.json（仅改 rest_api 字段）
    let mut s = settings::get_settings();
    s.rest_api = config.clone();
    settings::update_settings(s).map_err(|e: AppError| e.to_string())?;
    // 4. 应用到运行中的 RestService（热更新 token；地址/端口变更则 restart）
    rest.apply_config(config.clone())
        .await
        .map_err(|e| e.to_string())?;
    Ok(config)
}

/// 重新生成 token（保持其余字段不变）。
#[tauri::command]
pub async fn regenerate_rest_token(
    _state: State<'_, AppState>,
    rest: State<'_, RestService>,
) -> Result<RestConfig, String> {
    let mut cfg = rest.shared_config().read().await.clone();
    cfg.token = generate_token();
    let mut s = settings::get_settings();
    s.rest_api = cfg.clone();
    settings::update_settings(s).map_err(|e: AppError| e.to_string())?;
    rest.apply_config(cfg.clone())
        .await
        .map_err(|e| e.to_string())?;
    Ok(cfg)
}

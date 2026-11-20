//! 故障转移队列命令
//!
//! 管理代理模式下的故障转移队列（基于 providers 表的 in_failover_queue 字段）

use crate::app_config::AppType;
use crate::database::FailoverQueueItem;
use crate::provider::Provider;
use crate::store::AppState;
use std::str::FromStr;
use tauri::Emitter;

fn require_failover_app(app_type: &str) -> Result<AppType, String> {
    let app = AppType::from_str(app_type).map_err(|error| format!("无效的应用类型: {error}"))?;
    if !app.spec().local_proxy {
        return Err(format!("{app} 不支持故障转移"));
    }
    Ok(app)
}

fn require_failover_provider(
    db: &crate::database::Database,
    app_type: &AppType,
    provider_id: &str,
) -> Result<Provider, String> {
    let provider = db
        .get_provider_by_id(provider_id, app_type)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
    if !crate::proxy::provider_router::provider_supports_failover(app_type, &provider) {
        return Err("Codex Official 账号卡不支持自动故障转移".to_string());
    }
    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::{require_failover_app, require_failover_provider};
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::{AuthBinding, AuthBindingSource, Provider, ProviderMeta};
    use serde_json::json;

    #[test]
    fn failover_rejects_apps_without_a_proxy_data_plane() {
        assert!(require_failover_app("claude").is_ok());
        assert!(require_failover_app("pi").is_err());
    }

    #[test]
    fn failover_rejects_codex_official_account_cards() {
        let db = Database::memory().expect("memory db");
        let mut official = Provider::with_id(
            "official-a".to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        official.category = Some("official".to_string());
        official.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".to_string()),
                account_id: Some("account-a".to_string()),
            }),
            ..Default::default()
        });
        db.save_provider(&AppType::Codex, &official)
            .expect("save official");

        assert!(require_failover_provider(&db, &AppType::Codex, &official.id).is_err());
    }
}

#[tauri::command]
pub async fn get_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<FailoverQueueItem>, String> {
    let app_enum = require_failover_app(&app_type)?;
    let queue = state.db.get_failover_queue(&app_enum)?;
    if app_enum != AppType::Codex {
        return Ok(queue);
    }
    let providers = state
        .db
        .get_provider_rows(&app_enum)?
        .into_readable("故障转移队列");
    Ok(queue
        .into_iter()
        .filter(|item| {
            providers.get(&item.provider_id).is_some_and(|provider| {
                crate::proxy::provider_router::provider_supports_failover(&app_enum, provider)
            })
        })
        .collect())
}

/// 添加供应商到故障转移队列。托盘的故障转移子菜单只列队列成员：队列一变就重建。
#[tauri::command]
pub async fn add_to_failover_queue(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    let app_enum = require_failover_app(&app_type)?;
    require_failover_provider(&state.db, &app_enum, &provider_id)?;
    state.db.add_to_failover_queue(&app_enum, &provider_id)?;
    crate::tray::refresh_tray_menu(&app);
    Ok(())
}

/// 从故障转移队列移除供应商
#[tauri::command]
pub async fn remove_from_failover_queue(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    let app_enum = require_failover_app(&app_type)?;
    state
        .proxy_service
        .remove_from_failover_queue(&app_enum, &provider_id)
        .await?;
    crate::tray::refresh_tray_menu(&app);
    Ok(())
}

/// 获取指定应用的自动故障转移开关状态（从 proxy_config 表读取）
#[tauri::command]
pub async fn get_auto_failover_enabled(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<bool, String> {
    let app_enum = require_failover_app(&app_type)?;
    Ok(state
        .db
        .get_proxy_config_for_app(&app_enum)
        .await?
        .auto_failover_enabled)
}

/// 设置指定应用的自动故障转移开关状态（写入 proxy_config 表）
///
/// 注意：关闭故障转移时不会清除队列，队列内容会保留供下次开启时使用
#[tauri::command]
pub async fn set_auto_failover_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    let app_enum = require_failover_app(&app_type)?;
    log::info!(
        "[Failover] Setting auto_failover_enabled: app_type='{app_type}', enabled={enabled}"
    );

    // 读取当前配置
    let mut config = state.db.get_proxy_config_for_app(&app_enum).await?;

    if enabled && !crate::mode::current::is_proxy(&app_enum) {
        return Err("需要先让该应用进入路由模式，再开启故障转移".to_string());
    }

    // 队列为空时把当前供应商自动加入作为 P1，避免用户陷入"必须先加队列才能开启"的死锁
    let mut auto_added_provider_id: Option<String> = None;
    let p1_provider_id = if enabled {
        let all_providers = state
            .db
            .get_provider_rows(&app_enum)?
            .into_readable("开启故障转移");
        let mut queue = state
            .db
            .get_failover_queue(&app_enum)?
            .into_iter()
            .filter(|item| {
                all_providers
                    .get(&item.provider_id)
                    .is_some_and(|provider| {
                        crate::proxy::provider_router::provider_supports_failover(
                            &app_enum, provider,
                        )
                    })
            })
            .collect::<Vec<_>>();

        if queue.is_empty() {
            let current_id = crate::mode::current::provider_for(
                &state.db,
                &app_enum,
                crate::mode::current::Purpose::InUse,
            )?;

            let Some(current_id) = current_id else {
                return Err("故障转移队列为空，且未设置当前供应商，无法开启故障转移".to_string());
            };

            require_failover_provider(&state.db, &app_enum, &current_id)?;

            state.db.add_to_failover_queue(&app_enum, &current_id)?;
            auto_added_provider_id = Some(current_id);

            queue = state
                .db
                .get_failover_queue(&app_enum)?
                .into_iter()
                .filter(|item| {
                    all_providers
                        .get(&item.provider_id)
                        .is_some_and(|provider| {
                            crate::proxy::provider_router::provider_supports_failover(
                                &app_enum, provider,
                            )
                        })
                })
                .collect();
        }

        queue
            .first()
            .map(|item| item.provider_id.clone())
            .ok_or_else(|| "故障转移队列为空，无法开启故障转移".to_string())?
    } else {
        String::new()
    };

    // 开启前先切到 P1。只有切换成功后才写入 auto_failover_enabled=true，
    // 避免 P1 不可切换（例如 official provider）时留下“开关已开但目标未切”的脏状态。
    // Stack 模式不做故障转移，切换在锁内拒绝（见 `switch_route_for_failover`）。
    if enabled {
        if let Err(e) = crate::mode::controller::switch_route_for_failover(
            state.inner(),
            &app_enum,
            &p1_provider_id,
        )
        .await
        {
            if let Some(provider_id) = auto_added_provider_id {
                let _ = state
                    .proxy_service
                    .remove_from_failover_queue(&app_enum, &provider_id)
                    .await;
            }
            return Err(e);
        }
    }

    // 更新 auto_failover_enabled 字段（enabled 是模式的镜像，保持和模式一致）
    config.auto_failover_enabled = enabled;
    config.enabled = crate::mode::current::is_proxy(&app_enum);

    // 写回数据库
    state.db.update_proxy_config_for_app(config).await?;

    if enabled {
        // 发射 provider-switched 事件（让前端刷新当前供应商）
        let event_data = serde_json::json!({
            "appType": app_type,
            "providerId": p1_provider_id,
            "source": "failoverEnabled"
        });
        let _ = app.emit("provider-switched", event_data);
    }

    // 刷新托盘，确保状态同步（Linux 重建原生菜单，其它平台通知托盘面板，不挂原生菜单）
    crate::tray::refresh_tray_menu(&app);

    Ok(())
}

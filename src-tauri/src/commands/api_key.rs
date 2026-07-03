//! Provider API Key Tauri commands.
//!
//! Frontend-facing surface for the `provider_api_keys` child table
//! (Phase 2 DAO). Each command is a thin wrapper that maps `AppError`
//! to a `String` for `tauri::command` return. Heavy lifting (transactions,
//! runtime flush, etc.) lives in `database::dao::api_keys` and
//! `proxy::providers::key_ring`.
//!
//! The 8 commands here are the canonical CRUD + the runtime "make this
//! key the one written into settings.json" set. We deliberately keep the
//! `KeyRing` rotation logic off this path — that is owned by the proxy
//! service and is not user-driven.

use crate::database::dao::api_keys::ProviderApiKey;
use crate::store::AppState;
use crate::services::provider::per_key_live::regenerate_per_key_live_files;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

/// Wire DTO mirrored on the JS side as `ApiKeyDto`.
///
/// We don't expose `ProviderApiKey` directly because that would tie the
/// frontend to the DB struct's field names (`provider_id` vs the
/// `providerId` already used elsewhere in the app). This struct gives us
/// a single, explicit conversion point in the future.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyDto {
    pub id: String,
    pub provider_id: String,
    pub app_type: String,
    pub label: String,
    pub api_key: String,
    pub tags: Vec<String>,
    pub notes: String,
    pub enabled: bool,
    pub sort_index: i64,
    pub is_active: bool,
    pub cooldown_until: i64,
    pub failure_count: i64,
    pub last_used_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<ProviderApiKey> for ApiKeyDto {
    fn from(k: ProviderApiKey) -> Self {
        Self {
            id: k.id,
            provider_id: k.provider_id,
            app_type: k.app_type,
            label: k.label,
            api_key: k.api_key,
            tags: k.tags,
            notes: k.notes,
            enabled: k.enabled,
            sort_index: k.sort_index,
            is_active: k.is_active,
            cooldown_until: k.cooldown_until,
            failure_count: k.failure_count,
            last_used_at: k.last_used_at,
            last_error: k.last_error,
            created_at: k.created_at,
            updated_at: k.updated_at,
        }
    }
}

/// Payload for `cmd_create_api_key`. The DAO assigns timestamps and the
/// `is_active` flag is forced server-side (no key is active on create —
/// the user promotes one explicitly).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyInput {
    pub provider_id: String,
    pub app_type: String,
    pub label: String,
    pub api_key: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Payload for `cmd_update_api_key`. All fields optional — only the
/// provided ones are written. The DAO's dynamic SET clause relies on
/// `Some` vs `None` to decide what to touch.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApiKeyInput {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub sort_index: Option<i64>,
    #[serde(default)]
    pub is_active: Option<bool>,
}

// =====================================================================
// 1. 列出某 provider 的全部 key
// =====================================================================
#[tauri::command]
pub async fn cmd_list_api_keys(
    state: State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<Vec<ApiKeyDto>, String> {
    let keys = state
        .db
        .list_api_keys(&provider_id, &app_type)
        .map_err(|e| e.to_string())?;
    Ok(keys.into_iter().map(ApiKeyDto::from).collect())
}

// =====================================================================
// 2. 取单个 key
// =====================================================================
#[tauri::command]
pub async fn cmd_get_api_key(
    state: State<'_, AppState>,
    key_id: String,
) -> Result<Option<ApiKeyDto>, String> {
    let opt = state
        .db
        .get_api_key(&key_id)
        .map_err(|e| e.to_string())?;
    Ok(opt.map(ApiKeyDto::from))
}

// =====================================================================
// 3. 取当前 active key（写入 settings.json 用的那把）
// =====================================================================
#[tauri::command]
pub async fn cmd_get_active_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<Option<ApiKeyDto>, String> {
    let opt = state
        .db
        .get_active_api_key(&provider_id, &app_type)
        .map_err(|e| e.to_string())?;
    Ok(opt.map(ApiKeyDto::from))
}

// =====================================================================
// 4. 新建一把 key
// =====================================================================
#[tauri::command]
pub async fn cmd_create_api_key(
    state: State<'_, AppState>,
    payload: CreateApiKeyInput,
) -> Result<ApiKeyDto, String> {
    let now = Utc::now().timestamp();
    let sort_index = state
        .db
        .list_api_keys(&payload.provider_id, &payload.app_type)
        .map_err(|e| e.to_string())?
        .len() as i64;

    let row = ProviderApiKey {
        id: format!("ak-{}", Uuid::new_v4()),
        provider_id: payload.provider_id,
        app_type: payload.app_type,
        label: payload.label,
        api_key: payload.api_key,
        tags: payload.tags,
        notes: payload.notes,
        enabled: payload.enabled,
        // 新 key 默认排在末尾；用户可拖拽改 sort_index
        sort_index,
        // 创建时不主动激活——避免误把还没测过的 key 写到 settings.json
        is_active: false,
        cooldown_until: 0,
        failure_count: 0,
        last_used_at: None,
        last_error: None,
        created_at: now,
        updated_at: now,
    };
    let saved = state
        .db
        .insert_api_key(&row)
        .map_err(|e| e.to_string())?;

    // N-key per-key live config：新建后立即为该 provider 重新生成所有
    // per-key 文件。失败仅告警（不阻塞 UI）。
    refresh_per_key_live(&state, &saved.provider_id, &saved.app_type, Some(&saved.id));

    Ok(saved.into())
}

// =====================================================================
// 5. 更新 key（label/api_key/tags/notes/enabled/sort_index/is_active）
// =====================================================================
#[tauri::command]
pub async fn cmd_update_api_key(
    state: State<'_, AppState>,
    key_id: String,
    payload: UpdateApiKeyInput,
) -> Result<ApiKeyDto, String> {
    let now = Utc::now().timestamp();

    // 显式启用：disabled → enabled。同步把 runtime 状态拉回 0——
    // 这把 key 是被自动停用（5 次连续失败）还是用户手动停用的不重要，
    // 一旦用户决定"再试一次"，前一轮的 failure_count / cooldown_until
    // 都不该继续压着这把 key。否则下一次失败直接 1+old → 极可能再次
    // 立刻撞 5/5 触发自动停用，UI 看起来"启用后没动就又废了"。
    // 与 KeyRing 内存同步：下次 next_key 立即能用，不等下一次 flush tick。
    let resetting_runtime: bool = match payload.enabled {
        Some(true) => state
            .db
            .get_api_key(&key_id)
            .ok()
            .flatten()
            .map(|prev| !prev.enabled)
            .unwrap_or(false),
        _ => false,
    };

    let updated = state
        .db
        .update_api_key_fields(
            &key_id,
            payload.label.as_deref(),
            payload.api_key.as_deref(),
            payload.tags.as_ref(),
            payload.notes.as_deref(),
            payload.enabled,
            payload.sort_index,
            payload.is_active,
            now,
        )
        .map_err(|e| e.to_string())?;

    if resetting_runtime {
        if let Err(e) = state.db.write_api_key_runtime(&key_id, 0, 0, None, None, now) {
            log::warn!(
                "[api_key] 重置 enabled key {key_id} 的 runtime 失败（已 enabled 但 failure_count 未清零）: {e}"
            );
        }
        // KeyRing 内存同步：让代理进程立即看到清零后的状态。
        // 代理未运行时 reload 时自然会从 DB 读到清零后的值。
        state.proxy_service.notify_key_re_enabled(&key_id).await;
    }

    // Per-key live 文件刷新。`api_key` 或 `enabled` 变化都会让该
    // key 在 on-disk 的副本过时——统一调一次。
    refresh_per_key_live(&state, &updated.provider_id, &updated.app_type, Some(&updated.id));

    Ok(updated.into())
}

// =====================================================================
// 6. 删除 key
// =====================================================================

/// 删 key 之后返回给前端的元数据——前端用它精确 invalidate
/// `["apiKeys", providerId, appType]` 这一格缓存，而不是广播到所有
/// `["apiKeys"]`（旧版有多个 provider 的 list query 时会触发多余的 refetch）。
///
/// `rename_all = "camelCase"` 让 Rust 的 `key_id` / `provider_id` / `app_type`
/// 序列化成 `keyId` / `providerId` / `appType`，与 TS 端 `DeletedApiKeyInfo`
/// 接口对齐——不写 rename 的话 serde 默认会输出 snake_case，前端 `info.providerId`
/// 拿到 `undefined`，invalidate 的 query key 与实际 key 不匹配，缓存不刷新。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedApiKeyInfo {
    pub key_id: String,
    pub provider_id: String,
    pub app_type: String,
}

#[tauri::command]
pub async fn cmd_delete_api_key(
    state: State<'_, AppState>,
    key_id: String,
) -> Result<DeletedApiKeyInfo, String> {
    // 先取 row（用于在 DB 删完后调 per-key live 重生成 + 给前端回包元数据）；
    // 此时 DB 尚未变更。
    let row = state
        .db
        .get_api_key(&key_id)
        .map_err(|e| e.to_string())?;

    let Some(r) = row else {
        return Err(format!("key {key_id} 不存在"));
    };

    state
        .db
        .delete_api_key(&key_id)
        .map_err(|e| e.to_string())?;

    // DB row 已经删除——regen 内 stale-cleanup 会清掉 on-disk 文件。
    refresh_per_key_live(&state, &r.provider_id, &r.app_type, None);

    Ok(DeletedApiKeyInfo {
        key_id: r.id,
        provider_id: r.provider_id,
        app_type: r.app_type,
    })
}

// =====================================================================
// 7. 拖拽重排（批量更新 sort_index）
// =====================================================================
#[tauri::command]
pub async fn cmd_reorder_api_keys(
    state: State<'_, AppState>,
    app_type: String,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let now = Utc::now().timestamp();
    state
        .db
        .reorder_api_keys(&app_type, &ordered_ids, now)
        .map_err(|e| e.to_string())
}

// =====================================================================
// 8. 把某 key 设为「写入 settings.json 的那把」+ 触达 KeyRing 热更新
// =====================================================================
//
// 设计要点：
// - DB 层的事务保证只有一个 active key（`set_active_api_key`）。
// - 同步把 provider 的 `settings_config.apiKey` 改成新 active key 的 raw value，
//   这样 `write_live_with_common_config` 写到 settings.json 的就是新 key。
// - 若未处于 Live 接管模式：直接 rewrite Live 配置。
//   若处于接管模式：Live 配置里是占位符（PROXY_MANAGED），不动它；
//   KeyRing 才是当前在用的真相——热 reload 该 provider 的池子。
// - KeyRing 跟随 ProxyServer 生命周期；代理未运行时 `reload_provider_keys`
//   是 no-op，DB 一致性已经由 set_active_api_key 保证。
#[tauri::command]
pub async fn cmd_set_active_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    app_type: String,
    key_id: String,
) -> Result<ApiKeyDto, String> {
    let now = Utc::now().timestamp();

    // 先取新 active key 整行（raw api_key 在这里）
    let row = state
        .db
        .get_api_key(&key_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("set_active_api_key: key {key_id} 不存在"))?;
    let raw_key = row.api_key.clone();

    // 解析 AppType——settings_config 字段路径由 `AppType::api_key_settings_path`
    // 单真理来源决定，DAO 内部直接走 Provider::set_api_key 写入。
    let app_type_enum: crate::app_config::AppType = match app_type.as_str() {
        "claude" => crate::app_config::AppType::Claude,
        "codex" => crate::app_config::AppType::Codex,
        "gemini" => crate::app_config::AppType::Gemini,
        "hermes" => crate::app_config::AppType::Hermes,
        "openclaw" => crate::app_config::AppType::OpenClaw,
        "opencode" => crate::app_config::AppType::OpenCode,
        "claude-desktop" => crate::app_config::AppType::ClaudeDesktop,
        other => {
            return Err(format!("set_active_api_key: 未知 app_type '{other}'"));
        }
    };

    // 原子事务：关旧 active → 开新 active → 同步 settings_config
    // 任一步骤失败整体回滚（Review finding #16 修复点）。
    // settings_config 字段路径由 app_type_enum.api_key_settings_path() 决定。
    state
        .db
        .set_active_api_key_with_settings(
            &provider_id,
            &app_type_enum,
            &key_id,
            &raw_key,
            now,
        )
        .map_err(|e| e.to_string())?;

    // KeyRing 状态：先 reset cursor 让下一轮 next_key 从池头开始，
    // 再 reload provider 池保证新 active 已载入（Review finding #15 修复点）
    state
        .proxy_service
        .reset_provider_cursor(&provider_id, &app_type_enum)
        .await;
    state
        .proxy_service
        .reload_provider_keys(&provider_id, &app_type_enum)
        .await
        .ok(); // 失败仅告警，不阻塞 UI（DB 已更新）

    // 未接管时：把 live 配置 rewrite 成新 key（next 启动 CLI 时直接用）。
    // 已接管时：live 里是占位符，由代理读 KeyRing 决定真 key；不动 live。
    let takeover_active = state
        .proxy_service
        .is_takeover_active()
        .await
        .unwrap_or(false);
    if !takeover_active {
        if let Some(provider) = state
            .db
            .get_provider_by_id(&provider_id, &app_type)
            .map_err(|e| e.to_string())?
        {
            // settings_config 已被 set_active_api_key_with_settings 在事务里
            // 同步写到新 key；这里只重写 live 文件（不接管时）。覆盖 path
            // 与 provider.set_api_key 路径不再需要——读 DB 已是新 key。
            use crate::services::provider::write_live_with_common_config;
            write_live_with_common_config(state.db.as_ref(), &app_type_enum, &provider)
                .map_err(|e| format!("rewrite live 配置失败: {e}"))?;
        }
    }

    // 重新读出 row（DB 已更新）—— 返回新 active key 完整信息
    let row = state
        .db
        .get_api_key(&key_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("set_active_api_key: key {key_id} 刚刚存在但读不到"))?;

    // Per-key live 文件同步：active key 切换不影响每把 key 自身的 raw
    // api_key 值，但 regen 是 cheap 的——保持 on-disk 与 DB 严格对齐。
    refresh_per_key_live(&state, &provider_id, &app_type, Some(&key_id));

    Ok(row.into())
}

// =====================================================================
// 9. 通知 KeyRing「这把 key 用量接近上限」——proactive rotation
// =====================================================================
//
// 调用场景：前端 `useKeyUsage` / `useApiKeys` 的 React Query 在
// `autoQueryInterval` 周期内 fetch 到 `usage_percent >= 90%` 时，调用
// 本命令把 key 提前送进 cooldown。
//
// 命令是 fire-and-forget：返回 `bool` 仅代表「是否需要应用 cooldown
// 阈值」（false = 用量还在安全区，KeyRing 不动）。失败（KeyRing 未
// 加载 / DB 异常）一律 `Ok(false)`，不阻塞前端的轮询循环。
//
// 跟 mark_rate_limited（429 路径）的区别：
// - mark_rate_limited：上游硬性 429，必须立即切 key
// - cmd_mark_key_usage_high：5h/7d 窗口即将耗尽，提前切 key 避免浪费
//   一次请求失败延迟
//
// 实际 cooldown 策略由 `KeyRing::mark_usage_high` 决定（90% / 100%
// 两档），命令侧只做参数转发。
#[tauri::command]
pub async fn cmd_mark_key_usage_high(
    state: State<'_, crate::AppState>,
    key_id: String,
    usage_percent: f64,
    reset_at: i64,
) -> Result<bool, String> {
    if !usage_percent.is_finite() {
        return Ok(false);
    }
    Ok(state
        .proxy_service
        .notify_key_usage_high(&key_id, usage_percent, reset_at)
        .await)
}

// =====================================================================
// Per-key live config helper
// =====================================================================
//
// 把 mutation 后重新生成 per-key 文件的逻辑合并到一个 helper：拿到
// `(provider_id, app_type)` → 取最新 provider 对象 → 调
// `regenerate_per_key_live_files`。一处告警风格一致（log::warn!），
// 失败完全 silent——这与 N-key 文件作为 best-effort 副本的语义吻合
//（canonical settings.json 才是用户主体验，per-key 是 optional 副本）。
fn refresh_per_key_live(
    state: &State<'_, AppState>,
    provider_id: &str,
    app_type: &str,
    changed_key_id: Option<&str>,
) {
    use crate::app_config::AppType;
    let app_type_enum = match app_type {
        "claude" => AppType::Claude,
        "codex" => AppType::Codex,
        "gemini" => AppType::Gemini,
        // 其它 app type (ClaudeDesktop / OpenCode / OpenClaw / Hermes)
        // 不在 per-key 范围内——直接返回。
        _ => return,
    };
    let provider = match state
        .db
        .get_provider_by_id(provider_id, app_type)
        .ok()
        .flatten()
    {
        Some(p) => p,
        None => return,
    };
    if let Err(e) = regenerate_per_key_live_files(
        state.db.as_ref(),
        &app_type_enum,
        &provider,
        changed_key_id,
    ) {
        log::warn!(
            "[api_key] regenerate_per_key_live_files failed for {provider_id} ({app_type}): {e}"
        );
    }
}

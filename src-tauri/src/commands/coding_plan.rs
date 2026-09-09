use crate::provider::UsageScript;
use crate::services::coding_plan::{VolcengineAccount, VolcengineAkSkStatus};
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;
use tauri::State;

fn usage_script_aksk(state: &AppState, app: &str, provider_id: &str) -> Option<UsageScript> {
    let providers = state.db.get_all_providers(app).ok()?;
    providers
        .get(provider_id)?
        .meta
        .as_ref()
        .and_then(|m| m.usage_script.clone())
}

fn provider_name(state: &AppState, app: &str, provider_id: &str) -> Option<String> {
    let providers = state.db.get_all_providers(app).ok()?;
    providers.get(provider_id).map(|p| p.name.clone())
}

#[tauri::command]
pub async fn get_coding_plan_quota(
    state: State<'_, AppState>,
    base_url: String,
    api_key: String,
    // 火山方舟用控制面 AK/SK 签名查询用量；其他供应商不传，沿用 api_key。
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    // 智谱团队版（zhipu_team）靠显式标识路由（base_url 与个人版相同无法区分）。
    coding_plan_provider: Option<String>,
    team_organization_id: Option<String>,
    team_project_id: Option<String>,
    // 火山账号池：条目引用的账号 id + 条目名（惰性迁移入池时作默认名称）。
    aksk_account_id: Option<String>,
    provider_name: Option<String>,
) -> Result<SubscriptionQuota, String> {
    // 火山方舟凭据来源解析（账号池）：弹窗「测试查询」与后台刷新路径保持一致。
    let base_lc = base_url.to_lowercase();
    let is_volcengine =
        base_lc.contains("volces.com/api/plan") || base_lc.contains("volces.com/api/coding");
    let (access_key_id, secret_access_key) = if is_volcengine {
        let cred = crate::services::coding_plan::resolve_volcengine_aksk(
            &state.db,
            aksk_account_id.as_deref(),
            access_key_id.as_deref(),
            secret_access_key.as_deref(),
            provider_name.as_deref(),
        )?;
        (Some(cred.ak), Some(cred.sk))
    } else {
        (access_key_id, secret_access_key)
    };
    crate::services::coding_plan::get_coding_plan_quota(
        &base_url,
        &api_key,
        access_key_id.as_deref(),
        secret_access_key.as_deref(),
        coding_plan_provider.as_deref(),
        team_organization_id.as_deref(),
        team_project_id.as_deref(),
    )
    .await
}

/// 账号池列表（不含 SK 明文，下拉展示用）。
#[tauri::command]
pub fn list_volcengine_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<VolcengineAccount>, String> {
    Ok(state
        .db
        .get_setting(crate::services::coding_plan::VOLCENGINE_ACCOUNTS_KEY)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<VolcengineAccount>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|a| VolcengineAccount {
            secret_access_key: String::new(),
            ..a
        })
        .collect())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveVolcengineAccountRequest {
    pub id: Option<String>,
    pub label: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[tauri::command]
pub fn save_volcengine_account(
    state: State<'_, AppState>,
    request: SaveVolcengineAccountRequest,
) -> Result<VolcengineAccount, String> {
    crate::services::coding_plan::save_volcengine_account(
        &state.db,
        request.id.as_deref(),
        request.label.as_deref(),
        &request.access_key_id,
        &request.secret_access_key,
    )
}

#[tauri::command]
pub fn delete_volcengine_account(state: State<'_, AppState>, id: String) -> Result<(), String> {
    crate::services::coding_plan::delete_volcengine_account(&state.db, &id)
}

#[tauri::command]
pub fn rename_volcengine_account(
    state: State<'_, AppState>,
    id: String,
    label: String,
) -> Result<(), String> {
    crate::services::coding_plan::rename_volcengine_account(&state.db, &id, &label)
}

/// 弹窗状态行：当前条目的生效凭据（脱敏）。
#[tauri::command]
pub fn get_volcengine_aksk_status(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
) -> Result<VolcengineAkSkStatus, String> {
    let script = usage_script_aksk(&state, &app, &provider_id);
    let provider_name = provider_name(&state, &app, &provider_id);
    Ok(crate::services::coding_plan::get_volcengine_aksk_status(
        &state.db,
        script.as_ref().and_then(|s| s.aksk_account_id.as_deref()),
        script.as_ref().and_then(|s| s.access_key_id.as_deref()),
        script.as_ref().and_then(|s| s.secret_access_key.as_deref()),
        provider_name.as_deref(),
    ))
}

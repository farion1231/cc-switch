//! Syncs one provider card per saved official login account on each app's provider page.
//!
//! Supported apps:
//! - Claude Code (`claude_official`)
//! - Codex (`codex_oauth`)
//! - Grok Build (`xai_oauth`)
//! - Antigravity (`agy_oauth`)
//! - GitHub Copilot CLI (`github_copilot`)
//! - WorkBuddy (`workbuddy_account`)
//! - CodeBuddy (`codebuddy_oauth`)
//!
//! Kimi (`kimi_oauth`) already maintains one card per saved account in its native config, and
//! DeepSeek Harness (`dsh_account`) has a single native account rendered directly via `DshAccountCard`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::app_config::AppType;
use crate::database::{official_seed_for_app, Database, OFFICIAL_ACCOUNT_CARD_INFIX};
use crate::error::AppError;
use crate::provider::{AuthBinding, AuthBindingSource, Provider, ProviderMeta, UsageScript};
use crate::store::AppState;

use super::{
    agy_direct, claude_official, codex_direct, copilot_direct, grok_direct,
    lock_native_provider_operations,
};

const DISMISSED_FILENAME: &str = "official-account-cards-dismissed.json";

pub(crate) const AUTH_PROVIDER_CLAUDE_OFFICIAL: &str = "claude_official";
pub(crate) const AUTH_PROVIDER_CODEX_OAUTH: &str = "codex_oauth";
pub(crate) const AUTH_PROVIDER_XAI_OAUTH: &str = "xai_oauth";
pub(crate) const AUTH_PROVIDER_AGY_OAUTH: &str = "agy_oauth";
pub(crate) const AUTH_PROVIDER_GITHUB_COPILOT: &str = "github_copilot";
pub(crate) const AUTH_PROVIDER_WORKBUDDY_ACCOUNT: &str = "workbuddy_account";
pub(crate) const AUTH_PROVIDER_CODEBUDDY_OAUTH: &str = "codebuddy_oauth";

/// Canonical mapping between supported app types and their Auth Center official providers.
pub(crate) const OFFICIAL_CARD_APPS: &[(AppType, &str)] = &[
    (AppType::Claude, AUTH_PROVIDER_CLAUDE_OFFICIAL),
    (AppType::Codex, AUTH_PROVIDER_CODEX_OAUTH),
    (AppType::GrokBuild, AUTH_PROVIDER_XAI_OAUTH),
    (AppType::Agy, AUTH_PROVIDER_AGY_OAUTH),
    (AppType::Copilot, AUTH_PROVIDER_GITHUB_COPILOT),
    (AppType::WorkBuddy, AUTH_PROVIDER_WORKBUDDY_ACCOUNT),
    (AppType::Codebuddy, AUTH_PROVIDER_CODEBUDDY_OAUTH),
];

static DISMISSED_FILE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OfficialAccountEntry {
    pub id: String,
    pub login: String,
}

/// Maps an app type to the Auth Center provider whose saved accounts appear as cards on its page.
pub(crate) fn auth_provider_for_app(app_type: &AppType) -> Option<&'static str> {
    OFFICIAL_CARD_APPS
        .iter()
        .find(|(app, _)| app == app_type)
        .map(|(_, provider)| *provider)
}

/// Inverse of [`auth_provider_for_app`].
pub(crate) fn app_for_auth_provider(auth_provider: &str) -> Option<AppType> {
    OFFICIAL_CARD_APPS
        .iter()
        .find(|(_, provider)| *provider == auth_provider)
        .map(|(app, _)| app.clone())
}

/// Deterministic provider card ID for an auto-created official account card.
pub(crate) fn official_account_card_id(app_type: &AppType, account_id: &str) -> String {
    let digest = Sha256::digest(account_id.trim().as_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("{}{OFFICIAL_ACCOUNT_CARD_INFIX}{hex}", app_type.as_str())
}

/// Whether `id` follows the deterministic prefix of an auto-created official account card.
#[cfg(test)]
pub(crate) fn is_official_account_card_id(id: &str) -> bool {
    crate::database::is_official_account_card_id(id)
}

fn dismissed_file_path() -> PathBuf {
    crate::config::get_app_config_dir().join(DISMISSED_FILENAME)
}

fn read_dismissed_map() -> BTreeMap<String, BTreeSet<String>> {
    let path = dismissed_file_path();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    serde_json::from_str::<BTreeMap<String, BTreeSet<String>>>(&raw).unwrap_or_default()
}

fn write_dismissed_map(map: &BTreeMap<String, BTreeSet<String>>) -> Result<(), AppError> {
    let path = dismissed_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let bytes =
        serde_json::to_vec_pretty(map).map_err(|e| AppError::JsonSerialize { source: e })?;
    crate::config::atomic_write_private(&path, &bytes)
}

pub(crate) fn load_dismissed(app_type: &AppType) -> BTreeSet<String> {
    let _guard = DISMISSED_FILE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    read_dismissed_map()
        .remove(app_type.as_str())
        .unwrap_or_default()
}

fn mark_dismissed(app_type: &AppType, account_id: &str) -> Result<(), AppError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Ok(());
    }
    let _guard = DISMISSED_FILE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut map = read_dismissed_map();
    let set = map.entry(app_type.as_str().to_string()).or_default();
    if set.insert(account_id.to_string()) {
        write_dismissed_map(&map)?;
    }
    Ok(())
}

pub(crate) fn clear_dismissed(app_type: &AppType, account_ids: &[String]) {
    if account_ids.is_empty() || auth_provider_for_app(app_type).is_none() {
        return;
    }
    let _guard = DISMISSED_FILE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut map = read_dismissed_map();
    let Some(set) = map.get_mut(app_type.as_str()) else {
        return;
    };
    let mut changed = false;
    for id in account_ids {
        let trimmed = id.trim();
        if !trimmed.is_empty() && set.remove(trimmed) {
            changed = true;
        }
    }
    if changed {
        if set.is_empty() {
            map.remove(app_type.as_str());
        }
        if let Err(error) = write_dismissed_map(&map) {
            log::warn!(
                "[OfficialAccountCards] Failed to clear dismissed accounts for {}: {error}",
                app_type.as_str()
            );
        }
    }
}

pub(crate) fn is_official_for_app(app_type: &AppType, provider: &Provider) -> bool {
    match app_type {
        AppType::Claude => claude_official::is_official(provider),
        AppType::Codex => codex_direct::is_official(provider),
        AppType::GrokBuild => grok_direct::is_official(provider),
        AppType::Agy => agy_direct::is_official(provider),
        AppType::Copilot => copilot_direct::is_official(provider),
        AppType::WorkBuddy => crate::workbuddy_config::is_official_provider(provider),
        AppType::Codebuddy => crate::codebuddy_config::is_official_provider(provider),
        _ => false,
    }
}

/// Returns the trimmed managed account ID if `provider` is an official card bound to `app_type`'s
/// official Auth Center provider.
pub(crate) fn bound_official_account_id(app_type: &AppType, provider: &Provider) -> Option<String> {
    let auth_provider = auth_provider_for_app(app_type)?;
    if !is_official_for_app(app_type, provider) {
        return None;
    }
    provider
        .meta
        .as_ref()?
        .managed_account_id_for(auth_provider)
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

/// Ensures an existing bound official card cannot have its Auth Center account binding reassigned
/// or removed. Pre-upgrade unbound official cards may bind an account once.
pub(crate) fn ensure_official_binding_unchanged(
    app_type: &AppType,
    existing: Option<&Provider>,
    incoming: &Provider,
) -> Result<(), AppError> {
    if auth_provider_for_app(app_type).is_none() {
        return Ok(());
    }
    let Some(existing) = existing else {
        return Ok(());
    };
    if !is_official_for_app(app_type, existing) {
        return Ok(());
    }
    let existing_bound = bound_official_account_id(app_type, existing);
    let incoming_bound = bound_official_account_id(app_type, incoming);
    if existing_bound.is_some() && existing_bound != incoming_bound {
        return Err(AppError::InvalidInput(
            "官方登录卡片不支持切换账号，请在授权中心管理账号".to_string(),
        ));
    }
    Ok(())
}

fn seed_card_defaults(
    seed_app: &AppType,
    display_login: &str,
) -> (
    String,
    Value,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let seed = official_seed_for_app(seed_app).expect("missing official seed for app");
    let settings_config: Value =
        serde_json::from_str(seed.settings_config_json).expect("valid official seed JSON");
    (
        format!("{} ({display_login})", seed.name),
        settings_config,
        Some(seed.website_url.to_string()),
        Some(seed.icon.to_string()),
        Some(seed.icon_color.to_string()),
    )
}

fn build_official_card(
    app_type: &AppType,
    auth_provider: &str,
    card_id: String,
    account_id: &str,
    login: &str,
    sort_index: usize,
    created_at: i64,
) -> Provider {
    let display_login = if login.trim().is_empty() {
        account_id
    } else {
        login.trim()
    };

    let (name, settings_config, website_url, icon, icon_color, common_config_enabled, usage_script) =
        match app_type {
            AppType::Claude | AppType::Codex => {
                let (name, settings_config, website_url, icon, icon_color) =
                    seed_card_defaults(app_type, display_login);
                (
                    name,
                    settings_config,
                    website_url,
                    icon,
                    icon_color,
                    Some(true),
                    None,
                )
            }
            AppType::GrokBuild => {
                let (name, settings_config, website_url, icon, icon_color) =
                    seed_card_defaults(app_type, display_login);
                (
                    name,
                    settings_config,
                    website_url,
                    icon,
                    icon_color,
                    None,
                    None,
                )
            }
            AppType::Agy => {
                let (_, settings_config, website_url, icon, icon_color) =
                    seed_card_defaults(&AppType::Gemini, display_login);
                (
                    format!("Antigravity ({display_login})"),
                    settings_config,
                    website_url,
                    icon,
                    icon_color,
                    None,
                    Some(UsageScript {
                        enabled: true,
                        language: "javascript".to_string(),
                        code: String::new(),
                        timeout: None,
                        api_key: None,
                        base_url: None,
                        access_token: None,
                        user_id: None,
                        template_type: Some("official_subscription".to_string()),
                        auto_query_interval: None,
                        coding_plan_provider: None,
                        access_key_id: None,
                        secret_access_key: None,
                        team_organization_id: None,
                        team_project_id: None,
                    }),
                )
            }
            AppType::Copilot => (
                format!("GitHub Copilot ({display_login})"),
                json!({ "env": {} }),
                Some("https://github.com/features/copilot/cli".to_string()),
                Some("githubcopilot".to_string()),
                Some("currentColor".to_string()),
                None,
                None,
            ),
            AppType::WorkBuddy => (
                display_login.to_string(),
                Value::Object(Default::default()),
                None,
                Some("workbuddy".to_string()),
                None,
                None,
                None,
            ),
            AppType::Codebuddy => (
                display_login.to_string(),
                Value::Object(Default::default()),
                Some("https://www.codebuddy.ai".to_string()),
                Some("codebuddy".to_string()),
                None,
                None,
                None,
            ),
            _ => unreachable!("unsupported app_type in build_official_card"),
        };

    let live_config_managed = if app_type.spec().additive {
        Some(false)
    } else {
        None
    };

    let mut provider = Provider::with_id(card_id, name, settings_config, website_url);
    provider.category = Some("official".to_string());
    provider.icon = icon;
    provider.icon_color = icon_color;
    provider.sort_index = Some(sort_index);
    provider.created_at = Some(created_at);
    provider.meta = Some(ProviderMeta {
        common_config_enabled,
        usage_script,
        live_config_managed,
        auth_binding: Some(AuthBinding {
            source: AuthBindingSource::ManagedAccount,
            auth_provider: Some(auth_provider.to_string()),
            account_id: Some(account_id.to_string()),
        }),
        ..Default::default()
    });
    provider
}

/// Ensures every saved official account (not dismissed locally and not already bound to an official
/// card) has a dedicated provider card in `app_type`. Caller must already hold the per-app native
/// lock when calling during provider list/mutation flows, or use [`sync_app_cards_locked`].
pub(crate) fn sync_app_cards(
    db: &Database,
    app_type: &AppType,
    accounts: &[OfficialAccountEntry],
) -> Result<usize, AppError> {
    if accounts.is_empty() {
        return Ok(0);
    }
    let Some(auth_provider) = auth_provider_for_app(app_type) else {
        return Ok(0);
    };

    let rows = db.get_provider_rows(app_type)?;
    let mut covered_accounts: HashSet<String> = rows
        .readable
        .values()
        .filter_map(|provider| bound_official_account_id(app_type, provider))
        .collect();
    let mut taken_ids: HashSet<String> = rows.readable.keys().cloned().collect();
    for entry in rows.unreadable {
        taken_ids.insert(entry.id);
    }

    let dismissed = load_dismissed(app_type);
    let mut next_sort = db.next_sort_index_for_app(app_type)?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut created = 0usize;

    for account in accounts {
        let account_id = account.id.trim();
        if account_id.is_empty()
            || covered_accounts.contains(account_id)
            || dismissed.contains(account_id)
        {
            continue;
        }
        let card_id = official_account_card_id(app_type, account_id);
        if taken_ids.contains(&card_id) {
            continue;
        }
        let provider = build_official_card(
            app_type,
            auth_provider,
            card_id.clone(),
            account_id,
            &account.login,
            next_sort,
            now_ms,
        );
        db.save_provider(app_type, &provider)?;
        covered_accounts.insert(account_id.to_string());
        taken_ids.insert(card_id);
        next_sort += 1;
        created += 1;
    }

    Ok(created)
}

/// Acquires the per-app native provider lock before calling [`sync_app_cards`].
pub(crate) fn sync_app_cards_locked(
    state: &AppState,
    app_type: &AppType,
    accounts: &[OfficialAccountEntry],
) -> Result<usize, AppError> {
    let _native_guard = lock_native_provider_operations(state, app_type);
    sync_app_cards(&state.db, app_type, accounts)
}

/// Clears any local dismissed markers for `accounts` and syncs cards under the per-app native lock.
pub(crate) fn clear_dismissed_and_sync_locked(
    state: &AppState,
    app_type: &AppType,
    accounts: &[OfficialAccountEntry],
) -> Result<usize, AppError> {
    if accounts.is_empty() {
        return Ok(0);
    }
    let _native_guard = lock_native_provider_operations(state, app_type);
    let ids: Vec<String> = accounts.iter().map(|entry| entry.id.clone()).collect();
    clear_dismissed(app_type, &ids);
    sync_app_cards(&state.db, app_type, accounts)
}

/// Records an account as locally dismissed when the user deletes the last official card in
/// `app_type` bound to it, so startup/passive syncs do not recreate the card. Auth Center
/// accounts are never deleted by deleting provider cards; account deletion lives in Auth Center.
pub(crate) fn record_deleted_card(db: &Database, app_type: &AppType, deleted: &Provider) {
    let Some(account_id) = bound_official_account_id(app_type, deleted) else {
        return;
    };
    let still_covered = match db.get_provider_rows(app_type) {
        Ok(rows) => rows.readable.values().any(|provider| {
            bound_official_account_id(app_type, provider).as_deref() == Some(&account_id)
        }),
        Err(error) => {
            log::warn!(
                "[OfficialAccountCards] Failed to inspect remaining cards after deleting {}: {error}",
                deleted.id
            );
            return;
        }
    };
    if !still_covered {
        if let Err(error) = mark_dismissed(app_type, &account_id) {
            log::warn!(
                "[OfficialAccountCards] Failed to mark dismissed account {account_id} for {}: {error}",
                app_type.as_str()
            );
        }
    }
}

/// Cleans up auto-created cards (and clears dismissal flags) when accounts are removed/logged out
/// from Auth Center. User-created cards and any card still referenced/in-queue/in-stack are spared.
pub(crate) fn cleanup_removed_accounts(
    state: &AppState,
    app_type: &AppType,
    account_ids: &[String],
) -> usize {
    cleanup_removed_accounts_inner(state, app_type, account_ids, true)
}

/// Same as [`cleanup_removed_accounts`], for callers that already hold the app's switch lock
/// (such as Codex account removal/logout) so we do not re-enter `lock_settled_blocking`.
pub(crate) fn cleanup_removed_accounts_with_switch_lock_held(
    state: &AppState,
    app_type: &AppType,
    account_ids: &[String],
) -> usize {
    cleanup_removed_accounts_inner(state, app_type, account_ids, false)
}

fn cleanup_removed_accounts_inner(
    state: &AppState,
    app_type: &AppType,
    account_ids: &[String],
    acquire_switch_lock: bool,
) -> usize {
    if account_ids.is_empty() || auth_provider_for_app(app_type).is_none() {
        return 0;
    }

    let _native_guard = lock_native_provider_operations(state, app_type);
    let _switch_guard =
        if acquire_switch_lock && !app_type.spec().additive && !matches!(app_type, AppType::Agy) {
            match crate::mode::controller::lock_settled_blocking(state, app_type) {
                Ok(guard) => Some(guard),
                Err(error) => {
                    log::warn!(
                        "[OfficialAccountCards] Failed to lock {} during account cleanup: {error}",
                        app_type.as_str()
                    );
                    return 0;
                }
            }
        } else {
            None
        };

    clear_dismissed(app_type, account_ids);

    let queued_ids: HashSet<String> = if app_type.spec().local_proxy {
        state
            .db
            .get_failover_queue(app_type)
            .unwrap_or_default()
            .into_iter()
            .map(|item| item.provider_id)
            .collect()
    } else {
        HashSet::new()
    };

    let mut removed = 0usize;
    for raw_id in account_ids {
        let account_id = raw_id.trim();
        if account_id.is_empty() {
            continue;
        }
        let card_id = official_account_card_id(app_type, account_id);
        let Ok(Some(provider)) = state.db.get_provider_by_id(&card_id, app_type) else {
            continue;
        };
        if bound_official_account_id(app_type, &provider).as_deref() != Some(account_id) {
            continue;
        }
        if !app_type.spec().additive
            && !matches!(app_type, AppType::Agy)
            && crate::mode::current::is_referenced(&state.db, app_type, &card_id).unwrap_or(true)
        {
            continue;
        }
        if queued_ids.contains(&card_id) {
            continue;
        }
        if crate::mode::stack::is_member(app_type, &card_id).unwrap_or(true) {
            continue;
        }
        if let Err(error) = state.db.delete_provider(app_type, &card_id) {
            log::warn!(
                "[OfficialAccountCards] Failed to clean up card {card_id} for {}: {error}",
                app_type.as_str()
            );
            continue;
        }
        removed += 1;
    }

    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::services::provider::ProviderService;
    use crate::test_support::{EnvVars, TempUserHome};
    use serial_test::serial;
    use std::sync::Arc;

    fn test_state() -> (TempUserHome, EnvVars, AppState) {
        let home = TempUserHome::new();
        let env = EnvVars::new().set("CC_SWITCH_TEST_HOME", home.path());
        let db = Arc::new(Database::memory().expect("in-memory db"));
        let state = AppState::new(db);
        (home, env, state)
    }

    fn entry(id: &str, login: &str) -> OfficialAccountEntry {
        OfficialAccountEntry {
            id: id.to_string(),
            login: login.to_string(),
        }
    }

    #[test]
    #[serial]
    fn sync_creates_one_card_per_saved_account_across_all_supported_apps() {
        let (_home, _env, state) = test_state();
        let accounts = vec![
            entry("acct-1", "alice@example.com"),
            entry("acct-2", "bob@example.com"),
        ];

        for (app, expected_auth_provider) in OFFICIAL_CARD_APPS {
            let created = sync_app_cards(&state.db, app, &accounts).unwrap();
            assert_eq!(created, 2, "expected 2 cards for {}", app.as_str());
            assert_eq!(auth_provider_for_app(app), Some(*expected_auth_provider));
            assert_eq!(
                app_for_auth_provider(expected_auth_provider).as_ref(),
                Some(app)
            );

            let providers = state.db.get_all_providers(app).unwrap();
            assert_eq!(providers.len(), 2);

            let card1_id = official_account_card_id(app, "acct-1");
            assert!(is_official_account_card_id(&card1_id));
            let card1 = providers.get(&card1_id).expect("card1 exists");
            assert_eq!(card1.category.as_deref(), Some("official"));
            assert_eq!(
                bound_official_account_id(app, card1).as_deref(),
                Some("acct-1")
            );
            if *app == AppType::Codebuddy {
                assert!(card1_id.starts_with("codebuddy-official-"));
            }
            if *app == AppType::Agy {
                let usage = card1
                    .meta
                    .as_ref()
                    .and_then(|m| m.usage_script.as_ref())
                    .expect("agy card has official usage_script");
                assert!(usage.enabled);
                assert_eq!(
                    usage.template_type.as_deref(),
                    Some("official_subscription")
                );
            }
        }
    }

    #[test]
    #[serial]
    fn sync_is_idempotent_preserves_edits_and_skips_already_bound_accounts() {
        let (_home, _env, state) = test_state();
        state.db.init_default_official_providers().unwrap();

        let mut user_card = Provider::with_id(
            "custom-claude-work".into(),
            "My Work Claude".into(),
            json!({ "env": {} }),
            None,
        );
        user_card.category = Some("official".into());
        user_card.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("claude_official".into()),
                account_id: Some("acct-1".into()),
            }),
            ..Default::default()
        });
        state
            .db
            .save_provider(&AppType::Claude, &user_card)
            .unwrap();

        let accounts = vec![
            entry("acct-1", "alice@example.com"),
            entry("acct-2", "bob@example.com"),
        ];
        assert_eq!(
            sync_app_cards(&state.db, &AppType::Claude, &accounts).unwrap(),
            1
        );

        let providers = state.db.get_all_providers(&AppType::Claude).unwrap();
        assert!(providers.contains_key("claude-official"));
        assert!(providers.contains_key("custom-claude-work"));
        assert!(!providers.contains_key(&official_account_card_id(&AppType::Claude, "acct-1")));
        let acct2_id = official_account_card_id(&AppType::Claude, "acct-2");
        assert!(providers.contains_key(&acct2_id));

        let mut renamed = providers.get(&acct2_id).unwrap().clone();
        renamed.name = "Bob Custom Name".into();
        state.db.save_provider(&AppType::Claude, &renamed).unwrap();

        assert_eq!(
            sync_app_cards(&state.db, &AppType::Claude, &accounts).unwrap(),
            0
        );
        let after = state
            .db
            .get_provider_by_id(&acct2_id, &AppType::Claude)
            .unwrap()
            .unwrap();
        assert_eq!(after.name, "Bob Custom Name");
    }

    #[test]
    #[serial]
    fn deleting_card_marks_local_dismissed_and_prevents_resurrection_until_cleared() {
        let (_home, _env, state) = test_state();
        let accounts = vec![entry("acct-1", "alice@example.com")];
        sync_app_cards(&state.db, &AppType::Agy, &accounts).unwrap();

        let card_id = official_account_card_id(&AppType::Agy, "acct-1");
        ProviderService::delete(&state, AppType::Agy, &card_id).unwrap();
        assert!(state
            .db
            .get_provider_by_id(&card_id, &AppType::Agy)
            .unwrap()
            .is_none());
        assert!(load_dismissed(&AppType::Agy).contains("acct-1"));

        // Subsequent sync does not resurrect the deleted card.
        assert_eq!(
            sync_app_cards(&state.db, &AppType::Agy, &accounts).unwrap(),
            0
        );
        assert!(state
            .db
            .get_provider_by_id(&card_id, &AppType::Agy)
            .unwrap()
            .is_none());

        // Re-login / explicit re-import clears dismissal and restores the card.
        clear_dismissed(&AppType::Agy, &["acct-1".to_string()]);
        assert!(load_dismissed(&AppType::Agy).is_empty());
        assert_eq!(
            sync_app_cards(&state.db, &AppType::Agy, &accounts).unwrap(),
            1
        );
        assert!(state
            .db
            .get_provider_by_id(&card_id, &AppType::Agy)
            .unwrap()
            .is_some());
    }

    #[test]
    #[serial]
    fn dismissed_state_is_local_only_and_excluded_from_cloud_sync() {
        let (_home, _env, state) = test_state();
        mark_dismissed(&AppType::Claude, "acct-local").unwrap();

        // Sync export must not contain the local-only dismissed marker.
        let exported = state.db.export_sql_string_for_sync().unwrap();
        assert!(!exported.contains("official_account_cards_dismissed"));
        assert!(!exported.contains("acct-local"));

        // Importing a remote sync snapshot preserves local dismissed state.
        let remote_db = Database::memory().unwrap();
        remote_db.init_default_official_providers().unwrap();
        let remote_sql = remote_db.export_sql_string_for_sync().unwrap();
        state.db.import_sql_string_for_sync(&remote_sql).unwrap();

        assert!(load_dismissed(&AppType::Claude).contains("acct-local"));
    }

    #[test]
    #[serial]
    fn removing_account_cleans_unreferenced_auto_cards_and_spares_user_or_referenced_cards() {
        let (_home, _env, state) = test_state();
        let accounts = vec![
            entry("acct-free", "free@example.com"),
            entry("acct-active", "active@example.com"),
        ];
        sync_app_cards(&state.db, &AppType::Claude, &accounts).unwrap();

        let active_card_id = official_account_card_id(&AppType::Claude, "acct-active");
        state
            .db
            .set_current_provider(&AppType::Claude, &active_card_id)
            .unwrap();
        crate::settings::set_current_provider(&AppType::Claude, Some(&active_card_id)).unwrap();

        let mut user_card = Provider::with_id(
            "user-created-claude".into(),
            "User Created".into(),
            json!({ "env": {} }),
            None,
        );
        user_card.category = Some("official".into());
        user_card.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("claude_official".into()),
                account_id: Some("acct-free".into()),
            }),
            ..Default::default()
        });
        state
            .db
            .save_provider(&AppType::Claude, &user_card)
            .unwrap();

        let removed = cleanup_removed_accounts(
            &state,
            &AppType::Claude,
            &["acct-free".to_string(), "acct-active".to_string()],
        );
        assert_eq!(removed, 1);

        let free_card_id = official_account_card_id(&AppType::Claude, "acct-free");
        assert!(state
            .db
            .get_provider_by_id(&free_card_id, &AppType::Claude)
            .unwrap()
            .is_none());
        assert!(state
            .db
            .get_provider_by_id(&active_card_id, &AppType::Claude)
            .unwrap()
            .is_some());
        assert!(state
            .db
            .get_provider_by_id("user-created-claude", &AppType::Claude)
            .unwrap()
            .is_some());
    }

    #[test]
    #[serial]
    fn has_non_official_seed_provider_ignores_official_account_cards() {
        let (_home, _env, state) = test_state();
        state.db.init_default_official_providers().unwrap();
        let accounts = vec![entry("acct-1", "alice@example.com")];
        sync_app_cards(&state.db, &AppType::Claude, &accounts).unwrap();

        assert!(!state
            .db
            .has_non_official_seed_provider(&AppType::Claude)
            .unwrap());

        let custom = Provider::with_id(
            "custom-relay".into(),
            "Relay".into(),
            json!({"env":{}}),
            None,
        );
        state.db.save_provider(&AppType::Claude, &custom).unwrap();
        assert!(state
            .db
            .has_non_official_seed_provider(&AppType::Claude)
            .unwrap());
    }

    #[test]
    #[serial]
    fn codex_cleanup_runs_before_unbind_and_spares_referenced_cards_while_allowing_relogin() {
        let (_home, _env, state) = test_state();
        let accounts = vec![
            entry("acct-free", "free@example.com"),
            entry("acct-active", "active@example.com"),
        ];
        sync_app_cards_locked(&state, &AppType::Codex, &accounts).unwrap();

        let free_card_id = official_account_card_id(&AppType::Codex, "acct-free");
        let active_card_id = official_account_card_id(&AppType::Codex, "acct-active");

        state
            .db
            .set_current_provider(&AppType::Codex, &active_card_id)
            .unwrap();
        crate::settings::set_current_provider(&AppType::Codex, Some(&active_card_id)).unwrap();

        let mut user_card = Provider::with_id(
            "user-codex-card".into(),
            "User Codex".into(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        user_card.category = Some("official".into());
        user_card.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".into()),
                account_id: Some("acct-free".into()),
            }),
            ..Default::default()
        });
        state.db.save_provider(&AppType::Codex, &user_card).unwrap();

        let _switch_guard = tauri::async_runtime::block_on(
            state
                .proxy_service
                .lock_switch_for_app(AppType::Codex.as_str()),
        );
        let removed_ids = vec!["acct-free".to_string(), "acct-active".to_string()];
        let removed =
            cleanup_removed_accounts_with_switch_lock_held(&state, &AppType::Codex, &removed_ids);
        assert_eq!(removed, 1);

        state
            .db
            .unbind_codex_managed_accounts(&removed_ids)
            .unwrap();

        assert!(state
            .db
            .get_provider_by_id(&free_card_id, &AppType::Codex)
            .unwrap()
            .is_none());
        let active_after = state
            .db
            .get_provider_by_id(&active_card_id, &AppType::Codex)
            .unwrap()
            .expect("active card preserved");
        assert!(bound_official_account_id(&AppType::Codex, &active_after).is_none());
        let user_after = state
            .db
            .get_provider_by_id("user-codex-card", &AppType::Codex)
            .unwrap()
            .expect("user card preserved");
        assert!(bound_official_account_id(&AppType::Codex, &user_after).is_none());

        assert_eq!(
            clear_dismissed_and_sync_locked(
                &state,
                &AppType::Codex,
                &[entry("acct-free", "free@example.com")],
            )
            .unwrap(),
            1
        );
        let recreated = state
            .db
            .get_provider_by_id(&free_card_id, &AppType::Codex)
            .unwrap()
            .expect("recreated card");
        assert_eq!(
            bound_official_account_id(&AppType::Codex, &recreated).as_deref(),
            Some("acct-free")
        );
    }

    #[test]
    #[serial]
    fn record_deleted_card_marks_dismissed_only_after_last_official_card() {
        let (_home, _env, state) = test_state();
        let accounts = vec![entry("acct-1", "alice@example.com")];
        sync_app_cards(&state.db, &AppType::Claude, &accounts).unwrap();

        let auto_id = official_account_card_id(&AppType::Claude, "acct-1");
        let mut duplicate = Provider::with_id(
            "custom-claude-acct-1".into(),
            "Custom Official".into(),
            json!({ "env": {} }),
            None,
        );
        duplicate.category = Some("official".into());
        duplicate.meta = Some(ProviderMeta {
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("claude_official".into()),
                account_id: Some("acct-1".into()),
            }),
            ..Default::default()
        });
        state
            .db
            .save_provider(&AppType::Claude, &duplicate)
            .unwrap();

        let non_official = Provider::with_id(
            "relay-card".into(),
            "Relay".into(),
            json!({ "env": {} }),
            None,
        );
        state
            .db
            .save_provider(&AppType::Claude, &non_official)
            .unwrap();
        state
            .db
            .delete_provider(&AppType::Claude, "relay-card")
            .unwrap();
        record_deleted_card(&state.db, &AppType::Claude, &non_official);
        assert!(load_dismissed(&AppType::Claude).is_empty());

        let auto_card = state
            .db
            .get_provider_by_id(&auto_id, &AppType::Claude)
            .unwrap()
            .unwrap();
        state
            .db
            .delete_provider(&AppType::Claude, &auto_id)
            .unwrap();
        record_deleted_card(&state.db, &AppType::Claude, &auto_card);
        assert!(load_dismissed(&AppType::Claude).is_empty());

        state
            .db
            .delete_provider(&AppType::Claude, "custom-claude-acct-1")
            .unwrap();
        record_deleted_card(&state.db, &AppType::Claude, &duplicate);
        assert!(load_dismissed(&AppType::Claude).contains("acct-1"));
    }
}

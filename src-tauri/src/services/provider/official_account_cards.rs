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

use std::collections::{BTreeSet, HashSet};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::{AuthBinding, AuthBindingSource, Provider, ProviderMeta, UsageScript};
use crate::store::AppState;

use super::{
    agy_direct, claude_official, codex_direct, copilot_direct, grok_direct,
    lock_native_provider_operations,
};

const DISMISSED_SETTING_PREFIX: &str = "official_account_cards_dismissed:";
const OFFICIAL_ACCOUNT_CARD_INFIX: &str = "-official-acct-";

pub(crate) const AUTH_PROVIDER_CLAUDE_OFFICIAL: &str = "claude_official";
pub(crate) const AUTH_PROVIDER_CODEX_OAUTH: &str = "codex_oauth";
pub(crate) const AUTH_PROVIDER_XAI_OAUTH: &str = "xai_oauth";
pub(crate) const AUTH_PROVIDER_AGY_OAUTH: &str = "agy_oauth";
pub(crate) const AUTH_PROVIDER_GITHUB_COPILOT: &str = "github_copilot";
pub(crate) const AUTH_PROVIDER_WORKBUDDY_ACCOUNT: &str = "workbuddy_account";
pub(crate) const AUTH_PROVIDER_CODEBUDDY_OAUTH: &str = "codebuddy_oauth";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OfficialAccountEntry {
    pub id: String,
    pub login: String,
}

/// Maps an app type to the Auth Center provider whose saved accounts appear as cards on its page.
pub(crate) fn auth_provider_for_app(app_type: &AppType) -> Option<&'static str> {
    match app_type {
        AppType::Claude => Some(AUTH_PROVIDER_CLAUDE_OFFICIAL),
        AppType::Codex => Some(AUTH_PROVIDER_CODEX_OAUTH),
        AppType::GrokBuild => Some(AUTH_PROVIDER_XAI_OAUTH),
        AppType::Agy => Some(AUTH_PROVIDER_AGY_OAUTH),
        AppType::Copilot => Some(AUTH_PROVIDER_GITHUB_COPILOT),
        AppType::WorkBuddy => Some(AUTH_PROVIDER_WORKBUDDY_ACCOUNT),
        AppType::Codebuddy => Some(AUTH_PROVIDER_CODEBUDDY_OAUTH),
        _ => None,
    }
}

/// Inverse of [`auth_provider_for_app`].
pub(crate) fn app_for_auth_provider(auth_provider: &str) -> Option<AppType> {
    match auth_provider {
        AUTH_PROVIDER_CLAUDE_OFFICIAL => Some(AppType::Claude),
        AUTH_PROVIDER_CODEX_OAUTH => Some(AppType::Codex),
        AUTH_PROVIDER_XAI_OAUTH => Some(AppType::GrokBuild),
        AUTH_PROVIDER_AGY_OAUTH => Some(AppType::Agy),
        AUTH_PROVIDER_GITHUB_COPILOT => Some(AppType::Copilot),
        AUTH_PROVIDER_WORKBUDDY_ACCOUNT => Some(AppType::WorkBuddy),
        AUTH_PROVIDER_CODEBUDDY_OAUTH => Some(AppType::Codebuddy),
        _ => None,
    }
}

/// Deterministic provider card ID for an auto-created official account card.
pub(crate) fn official_account_card_id(app_type: &AppType, account_id: &str) -> String {
    let digest = Sha256::digest(account_id.trim().as_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("{}{OFFICIAL_ACCOUNT_CARD_INFIX}{hex}", app_type.as_str())
}

/// Whether `id` follows the deterministic prefix of an auto-created official account card.
pub(crate) fn is_official_account_card_id(id: &str) -> bool {
    [
        AppType::Claude,
        AppType::Codex,
        AppType::GrokBuild,
        AppType::Agy,
        AppType::Copilot,
        AppType::WorkBuddy,
        AppType::Codebuddy,
    ]
    .iter()
    .any(|app| {
        let prefix = format!("{}{OFFICIAL_ACCOUNT_CARD_INFIX}", app.as_str());
        id.strip_prefix(&prefix)
            .is_some_and(|rest| rest.len() == 16 && rest.bytes().all(|b| b.is_ascii_hexdigit()))
    })
}

fn dismissed_setting_key(app_type: &AppType) -> String {
    format!("{DISMISSED_SETTING_PREFIX}{}", app_type.as_str())
}

fn load_dismissed(db: &Database, app_type: &AppType) -> BTreeSet<String> {
    let key = dismissed_setting_key(app_type);
    match db.get_setting(&key) {
        Ok(Some(raw)) => serde_json::from_str::<BTreeSet<String>>(&raw).unwrap_or_default(),
        _ => BTreeSet::new(),
    }
}

fn save_dismissed(
    db: &Database,
    app_type: &AppType,
    dismissed: &BTreeSet<String>,
) -> Result<(), AppError> {
    let key = dismissed_setting_key(app_type);
    let json =
        serde_json::to_string(dismissed).map_err(|e| AppError::JsonSerialize { source: e })?;
    db.set_setting(&key, &json)
}

fn mark_dismissed(db: &Database, app_type: &AppType, account_id: &str) -> Result<(), AppError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Ok(());
    }
    let mut dismissed = load_dismissed(db, app_type);
    if dismissed.insert(account_id.to_string()) {
        save_dismissed(db, app_type, &dismissed)?;
    }
    Ok(())
}

pub(crate) fn clear_dismissed_locked(state: &AppState, app_type: &AppType, account_ids: &[String]) {
    if account_ids.is_empty() || auth_provider_for_app(app_type).is_none() {
        return;
    }
    let _native_guard = lock_native_provider_operations(state, app_type);
    clear_dismissed(&state.db, app_type, account_ids);
}

pub(crate) fn clear_dismissed(db: &Database, app_type: &AppType, account_ids: &[String]) {
    if account_ids.is_empty() || auth_provider_for_app(app_type).is_none() {
        return;
    }
    let mut dismissed = load_dismissed(db, app_type);
    let mut changed = false;
    for id in account_ids {
        let trimmed = id.trim();
        if !trimmed.is_empty() && dismissed.remove(trimmed) {
            changed = true;
        }
    }
    if changed {
        if let Err(error) = save_dismissed(db, app_type, &dismissed) {
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
            AppType::Claude => (
                format!("Claude Official ({display_login})"),
                json!({ "env": {} }),
                Some("https://www.anthropic.com/claude-code".to_string()),
                Some("anthropic".to_string()),
                Some("#D4915D".to_string()),
                Some(true),
                None,
            ),
            AppType::Codex => (
                format!("OpenAI Official ({display_login})"),
                json!({ "auth": {}, "config": "" }),
                Some("https://chatgpt.com/codex".to_string()),
                Some("openai".to_string()),
                Some("#00A67E".to_string()),
                Some(true),
                None,
            ),
            AppType::GrokBuild => (
                format!("Grok Official ({display_login})"),
                json!({ "config": "" }),
                Some("https://x.ai/grok".to_string()),
                Some("grok".to_string()),
                Some("currentColor".to_string()),
                None,
                None,
            ),
            AppType::Agy => (
                format!("Antigravity ({display_login})"),
                json!({ "env": {}, "config": {} }),
                Some("https://ai.google.dev/".to_string()),
                Some("gemini".to_string()),
                Some("#4285F4".to_string()),
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
            ),
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

/// Ensures every saved official account (not dismissed and not already bound to an official card)
/// has a dedicated provider card in `app_type`. Caller must already hold the per-app native lock
/// when calling during provider list/mutation flows, or use [`sync_app_cards_locked`].
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

    let dismissed = load_dismissed(db, app_type);
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
#[allow(dead_code)]
pub(crate) fn sync_app_cards_locked(
    state: &AppState,
    app_type: &AppType,
    accounts: &[OfficialAccountEntry],
) -> Result<usize, AppError> {
    let _native_guard = lock_native_provider_operations(state, app_type);
    sync_app_cards(&state.db, app_type, accounts)
}

/// Clears any dismissed markers for `accounts` and syncs cards under the per-app native provider lock.
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
    clear_dismissed(&state.db, app_type, &ids);
    sync_app_cards(&state.db, app_type, accounts)
}

/// Records an account as dismissed if the user deletes the last official card bound to it,
/// so background/list syncs do not immediately recreate a deleted card. Returns `Some(account_id)`
/// when the deleted card was the last remaining official card in `app_type` covering that account.
pub(crate) fn record_deleted_card(
    db: &Database,
    app_type: &AppType,
    deleted: &Provider,
) -> Option<String> {
    let account_id = bound_official_account_id(app_type, deleted)?;
    let still_covered = match db.get_provider_rows(app_type) {
        Ok(rows) => rows.readable.values().any(|provider| {
            bound_official_account_id(app_type, provider).as_deref() == Some(&account_id)
        }),
        Err(error) => {
            log::warn!(
                "[OfficialAccountCards] Failed to inspect remaining cards after deleting {}: {error}",
                deleted.id
            );
            return None;
        }
    };
    if !still_covered {
        if let Err(error) = mark_dismissed(db, app_type, &account_id) {
            log::warn!(
                "[OfficialAccountCards] Failed to mark dismissed account {account_id} for {}: {error}",
                app_type.as_str()
            );
        }
        Some(account_id)
    } else {
        None
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

    clear_dismissed(&state.db, app_type, account_ids);

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

        for app in [
            AppType::Claude,
            AppType::Codex,
            AppType::GrokBuild,
            AppType::Agy,
            AppType::Copilot,
            AppType::WorkBuddy,
            AppType::Codebuddy,
        ] {
            let created = sync_app_cards(&state.db, &app, &accounts).unwrap();
            assert_eq!(created, 2, "expected 2 cards for {}", app.as_str());

            let providers = state.db.get_all_providers(&app).unwrap();
            assert_eq!(providers.len(), 2);

            let card1_id = official_account_card_id(&app, "acct-1");
            assert!(is_official_account_card_id(&card1_id));
            let card1 = providers.get(&card1_id).expect("card1 exists");
            assert_eq!(card1.category.as_deref(), Some("official"));
            assert_eq!(
                bound_official_account_id(&app, card1).as_deref(),
                Some("acct-1")
            );
            if app == AppType::Codebuddy {
                assert!(card1_id.starts_with("codebuddy-official-"));
            }
            if app == AppType::Agy {
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

        // Bind a user-created card (and a seed card) to acct-1; acct-2 is unbound.
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

        // Unbound seed card claude-official is preserved, user_card covers acct-1, and only acct-2 gets a card.
        let providers = state.db.get_all_providers(&AppType::Claude).unwrap();
        assert!(providers.contains_key("claude-official"));
        assert!(providers.contains_key("custom-claude-work"));
        assert!(!providers.contains_key(&official_account_card_id(&AppType::Claude, "acct-1")));
        let acct2_id = official_account_card_id(&AppType::Claude, "acct-2");
        assert!(providers.contains_key(&acct2_id));

        // Rename acct-2 card and re-sync: must not duplicate or overwrite the custom name.
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
    fn deleting_card_marks_dismissed_and_prevents_resurrection_until_cleared() {
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
        clear_dismissed(&state.db, &AppType::Agy, &["acct-1".to_string()]);
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
    fn removing_account_cleans_unreferenced_auto_cards_and_spares_user_or_referenced_cards() {
        let (_home, _env, state) = test_state();
        let accounts = vec![
            entry("acct-free", "free@example.com"),
            entry("acct-active", "active@example.com"),
        ];
        sync_app_cards(&state.db, &AppType::Claude, &accounts).unwrap();

        // Mark acct-active's card as current direct provider.
        let active_card_id = official_account_card_id(&AppType::Claude, "acct-active");
        state
            .db
            .set_current_provider(&AppType::Claude, &active_card_id)
            .unwrap();
        crate::settings::set_current_provider(&AppType::Claude, Some(&active_card_id)).unwrap();

        // Also add a user-created card bound to acct-free.
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

        // Mark active_card_id as current direct provider.
        state
            .db
            .set_current_provider(&AppType::Codex, &active_card_id)
            .unwrap();
        crate::settings::set_current_provider(&AppType::Codex, Some(&active_card_id)).unwrap();

        // Also add a user-created card bound to acct-free.
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

        // Hold the Codex switch lock just like remove_codex_oauth_account_with_switch_lock does.
        let _switch_guard = tauri::async_runtime::block_on(
            state
                .proxy_service
                .lock_switch_for_app(AppType::Codex.as_str()),
        );
        let removed_ids = vec!["acct-free".to_string(), "acct-active".to_string()];
        let removed =
            cleanup_removed_accounts_with_switch_lock_held(&state, &AppType::Codex, &removed_ids);
        assert_eq!(removed, 1);

        // Unbind remaining cards afterward (just like remove/logout does).
        state
            .db
            .unbind_codex_managed_accounts(&removed_ids)
            .unwrap();

        // Auto card for unreferenced acct-free was deleted (no zombie card);
        // active card and user-created card survived and had authBinding unbound.
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

        // Re-logging in acct-free recreates its bound card cleanly instead of colliding with a zombie.
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
    fn record_deleted_card_returns_uncovered_account_id_only_for_last_official_card() {
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
        assert_eq!(
            record_deleted_card(&state.db, &AppType::Claude, &non_official),
            None
        );

        let auto_card = state
            .db
            .get_provider_by_id(&auto_id, &AppType::Claude)
            .unwrap()
            .unwrap();
        state
            .db
            .delete_provider(&AppType::Claude, &auto_id)
            .unwrap();
        assert_eq!(
            record_deleted_card(&state.db, &AppType::Claude, &auto_card),
            None
        );
        assert!(load_dismissed(&state.db, &AppType::Claude).is_empty());

        state
            .db
            .delete_provider(&AppType::Claude, "custom-claude-acct-1")
            .unwrap();
        assert_eq!(
            record_deleted_card(&state.db, &AppType::Claude, &duplicate).as_deref(),
            Some("acct-1")
        );
        assert!(load_dismissed(&state.db, &AppType::Claude).contains("acct-1"));
    }
}

use super::recovery::{
    credential_diffs, persist_configuration_state, read_configuration_state, ConfigurationState,
    RecoveryMode, RecoveryResult,
};
use super::{
    apply_selected_credentials, extract_provider_credentials, prune_credential_audits,
    prune_snapshots, record_credential_audit, CredentialDiff, CredentialSource,
    ROLLBACK_MAX_AGE_DAYS,
};
use crate::app_config::AppType;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::provider::live::{write_live_snapshot, write_live_with_common_config};
use crate::services::ProviderService;
use rusqlite::params;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
static FAIL_NEXT_LIVE_PROJECTION: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FAIL_NEXT_LIVE_COMPENSATION: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FAIL_NEXT_LIVE_VERIFICATION: AtomicBool = AtomicBool::new(false);

type AppLock = Arc<tokio::sync::Mutex<()>>;
const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1_000;

pub struct ProviderMutationRequest {
    pub app_type: AppType,
    pub provider: Provider,
    pub expected_revision: i64,
    pub source: CredentialSource,
    pub confirmed_credential_fields: BTreeSet<String>,
    /// When true, only persist the DB CAS/audit side of the mutation.
    /// Callers that own Live projection themselves (e.g. proxy takeover
    /// paths inside ProviderService::update) set this to avoid double-writing
    /// or clobbering proxy-owned Live placeholders.
    pub skip_live_projection: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MutationOutcome {
    Saved {
        revision: i64,
        warnings: Vec<String>,
    },
    Conflict {
        current_revision: i64,
        diff: Vec<CredentialDiff>,
    },
}

pub struct ProviderMutationCoordinator {
    db: Arc<Database>,
    app_locks: HashMap<String, AppLock>,
}

impl ProviderMutationCoordinator {
    pub fn new(db: Arc<Database>) -> Self {
        let app_locks = AppType::all()
            .map(|app| {
                (
                    app.as_str().to_string(),
                    Arc::new(tokio::sync::Mutex::new(())),
                )
            })
            .collect();
        Self { db, app_locks }
    }

    fn app_lock(&self, app_type: &AppType) -> Result<AppLock, AppError> {
        self.app_locks
            .get(app_type.as_str())
            .cloned()
            .ok_or_else(|| security_error("configuration_inconsistent"))
    }

    pub async fn mutate(
        &self,
        request: ProviderMutationRequest,
    ) -> Result<MutationOutcome, AppError> {
        let lock = self.app_lock(&request.app_type)?;
        let _guard = lock.lock().await;
        self.mutate_locked(request)
    }

    fn mutate_locked(&self, request: ProviderMutationRequest) -> Result<MutationOutcome, AppError> {
        self.mutate_locked_with_policy(request, false)
    }

    fn mutate_locked_with_policy(
        &self,
        request: ProviderMutationRequest,
        allow_inconsistent_recovery: bool,
    ) -> Result<MutationOutcome, AppError> {
        if !allow_inconsistent_recovery
            && read_configuration_state(&self.db, &request.app_type)?
                == ConfigurationState::Inconsistent
        {
            return Err(security_error("configuration_inconsistent"));
        }

        let app = request.app_type.as_str();
        let old_provider = self
            .db
            .get_provider_by_id(&request.provider.id, app)?
            .ok_or_else(|| AppError::InvalidInput("provider not found".to_string()))?;
        let current_revision = self
            .db
            .get_provider_revision(app, &request.provider.id)?
            .ok_or_else(|| AppError::InvalidInput("provider revision missing".to_string()))?;
        if current_revision != request.expected_revision {
            return Ok(MutationOutcome::Conflict {
                current_revision,
                diff: provider_diffs(&old_provider, &request.provider, &request.app_type)?,
            });
        }

        let old_fields = extract_provider_credentials(&old_provider, &request.app_type);
        let new_fields = extract_provider_credentials(&request.provider, &request.app_type);
        let changed_fields = changed_credential_fields(&old_fields, &new_fields)?;
        for field in &changed_fields {
            if !request.confirmed_credential_fields.contains(*field) {
                return Err(security_error("provider_credentials_missing"));
            }
        }

        let old_live_settings = if request.skip_live_projection {
            None
        } else {
            Some(ProviderService::read_live_settings(request.app_type.clone()))
        };
        let now = chrono::Utc::now().timestamp_millis();
        let snapshot_id = uuid::Uuid::new_v4().to_string();
        let audit_id = (!changed_fields.is_empty()).then(|| uuid::Uuid::new_v4().to_string());
        let next_revision = {
            let mut conn = lock_conn!(self.db.conn);
            let tx = conn
                .transaction()
                .map_err(|e| AppError::Database(format!("begin provider mutation: {e}")))?;
            let next = Database::update_provider_cas_on_conn(
                &tx,
                app,
                &request.provider,
                request.expected_revision,
            )?
            .ok_or_else(|| security_error("provider_revision_conflict"))?;

            tx.execute(
                "INSERT INTO provider_rollback_snapshots (
                    id, provider_id, app_type, provider_json, source_revision, created_at, expires_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    snapshot_id,
                    old_provider.id,
                    app,
                    serde_json::to_string(&old_provider)
                        .map_err(|source| AppError::JsonSerialize { source })?,
                    current_revision,
                    now,
                    now.saturating_add(ROLLBACK_MAX_AGE_DAYS * MILLIS_PER_DAY),
                ],
            )
            .map_err(|e| AppError::Database(format!("write provider rollback snapshot: {e}")))?;

            if let Some(audit_id) = audit_id.as_deref() {
                record_credential_audit(
                    &tx,
                    audit_id,
                    &request.provider.id,
                    &request.app_type,
                    request.source,
                    &changed_fields,
                    &old_fields,
                    &new_fields,
                    now,
                )?;
            }
            tx.commit()
                .map_err(|e| AppError::Database(format!("commit provider mutation: {e}")))?;
            next
        };

        let projection = if request.skip_live_projection {
            Ok(())
        } else {
            project_live_credentials(self.db.as_ref(), &request.app_type, &request.provider)
        };

        if projection.is_err() {
            let db_compensated = self
                .db
                .update_provider_cas(app, &old_provider, next_revision)?
                .is_some();
            #[cfg(test)]
            let force_live_compensation_failure =
                FAIL_NEXT_LIVE_COMPENSATION.swap(false, Ordering::SeqCst);
            #[cfg(not(test))]
            let force_live_compensation_failure = false;

            let live_compensated = if force_live_compensation_failure {
                false
            } else {
                match old_live_settings {
                    Some(Ok(settings)) => {
                        let mut live_provider = old_provider.clone();
                        live_provider.settings_config = settings;
                        write_live_snapshot(&request.app_type, &live_provider).is_ok()
                    }
                    Some(Err(_)) => false,
                    None => true,
                }
            };
            if db_compensated && live_compensated {
                self.discard_failed_mutation_records(&snapshot_id, audit_id.as_deref())?;
            } else {
                persist_configuration_state(
                    &self.db,
                    &request.app_type,
                    ConfigurationState::Inconsistent,
                    Some("live_projection_failed"),
                )?;
                return Err(security_error("configuration_inconsistent"));
            }
            return Err(security_error("live_projection_failed"));
        }

        persist_configuration_state(
            &self.db,
            &request.app_type,
            ConfigurationState::Consistent,
            None,
        )?;
        let warnings = self.prune_mutation_history(now).err().map_or_else(Vec::new, |error| {
            log::warn!("provider mutation history pruning failed: {error}");
            vec!["provider_history_prune_failed".to_string()]
        });
        Ok(MutationOutcome::Saved {
            revision: next_revision,
            warnings,
        })
    }

    fn prune_mutation_history(&self, now: i64) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.db.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("begin mutation history pruning: {e}")))?;
        prune_credential_audits(&tx, now)?;
        prune_snapshots(&tx, now)?;
        tx.commit()
            .map_err(|e| AppError::Database(format!("commit mutation history pruning: {e}")))
    }

    fn discard_failed_mutation_records(
        &self,
        snapshot_id: &str,
        audit_id: Option<&str>,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.db.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("begin mutation record cleanup: {e}")))?;
        tx.execute(
            "DELETE FROM provider_rollback_snapshots WHERE id = ?1",
            params![snapshot_id],
        )
        .map_err(|e| AppError::Database(format!("delete failed rollback snapshot: {e}")))?;
        if let Some(audit_id) = audit_id {
            tx.execute(
                "DELETE FROM provider_credential_audit WHERE id = ?1",
                params![audit_id],
            )
            .map_err(|e| AppError::Database(format!("delete failed credential audit: {e}")))?;
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("commit mutation record cleanup: {e}")))
    }

    pub async fn recover(
        &self,
        app_type: AppType,
        mode: RecoveryMode,
    ) -> Result<RecoveryResult, AppError> {
        let lock = self.app_lock(&app_type)?;
        let _guard = lock.lock().await;
        let provider_id = self
            .db
            .get_current_provider(app_type.as_str())?
            .ok_or_else(|| AppError::InvalidInput("current provider not found".to_string()))?;
        let provider = self
            .db
            .get_provider_by_id(&provider_id, app_type.as_str())?
            .ok_or_else(|| AppError::InvalidInput("current provider not found".to_string()))?;
        let revision = self
            .db
            .get_provider_revision(app_type.as_str(), &provider_id)?
            .ok_or_else(|| AppError::InvalidInput("provider revision missing".to_string()))?;

        match mode {
            RecoveryMode::ProjectDbToLive => {
                write_live_with_common_config(self.db.as_ref(), &app_type, &provider)
                    .map_err(|_| security_error("live_projection_failed"))?;
                verify_live_credentials(&provider, &app_type)?;
                persist_configuration_state(
                    &self.db,
                    &app_type,
                    ConfigurationState::Consistent,
                    None,
                )?;
                Ok(RecoveryResult {
                    state: ConfigurationState::Consistent,
                    revision,
                    live_fingerprint_verified: true,
                    audit_written: false,
                })
            }
            RecoveryMode::ImportLiveToDb => {
                let live_settings = ProviderService::read_live_settings(app_type.clone())
                    .map_err(|_| security_error("provider_credentials_missing"))?;
                let mut live_provider = provider.clone();
                live_provider.settings_config = live_settings.clone();
                let live_fields = extract_provider_credentials(&live_provider, &app_type);
                let mut confirmed_credential_fields = BTreeSet::new();
                if live_fields.api_key.is_some() {
                    confirmed_credential_fields.insert("apiKey".to_string());
                }
                if live_fields.base_url.is_some() {
                    confirmed_credential_fields.insert("baseUrl".to_string());
                }
                if confirmed_credential_fields.is_empty() {
                    return Err(security_error("provider_credentials_missing"));
                }

                let old_fields = extract_provider_credentials(&provider, &app_type);
                let mut imported_provider = provider;
                apply_selected_credentials(
                    &mut imported_provider,
                    &live_settings,
                    &app_type,
                    &confirmed_credential_fields,
                )
                .map_err(|_| security_error("provider_credentials_missing"))?;
                let imported_fields = extract_provider_credentials(&imported_provider, &app_type);
                let audit_written = old_fields != imported_fields;

                match self.mutate_locked_with_policy(
                    ProviderMutationRequest {
                        app_type,
                        provider: imported_provider,
                        expected_revision: revision,
                        source: CredentialSource::ExplicitLiveImport,
                        skip_live_projection: false,
                        confirmed_credential_fields,
                    },
                    true,
                )? {
                    MutationOutcome::Saved { revision, .. } => Ok(RecoveryResult {
                        state: ConfigurationState::Consistent,
                        revision,
                        live_fingerprint_verified: true,
                        audit_written,
                    }),
                    MutationOutcome::Conflict { .. } => {
                        Err(security_error("provider_revision_conflict"))
                    }
                }
            }
        }
    }
}

fn changed_credential_fields<'a>(
    old: &'a super::CredentialFields,
    new: &'a super::CredentialFields,
) -> Result<Vec<&'a str>, AppError> {
    let mut changed = Vec::new();
    if old.api_key != new.api_key {
        changed.push("apiKey");
    }
    if !super::base_urls_equivalent(old.base_url.as_deref(), new.base_url.as_deref())? {
        changed.push("baseUrl");
    }
    Ok(changed)
}

fn provider_diffs(
    old: &Provider,
    new: &Provider,
    app_type: &AppType,
) -> Result<Vec<CredentialDiff>, AppError> {
    super::recovery::credential_diffs(old, &new.settings_config, app_type)
}

fn project_live_credentials(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    #[cfg(test)]
    if FAIL_NEXT_LIVE_PROJECTION.swap(false, Ordering::SeqCst) {
        return Err(security_error("live_projection_failed"));
    }

    if app_type.is_additive_mode() {
        // OMO providers are projected into their profile files rather than the
        // ordinary OpenCode `provider` map.  Keep the same current-provider
        // rule as ProviderService::update: a DB-only/non-current OMO row must
        // not create or rewrite a Live profile as a side effect of editing it.
        if matches!(app_type, AppType::OpenCode) {
            let omo_variant = match provider.category.as_deref() {
                Some("omo") => Some(&crate::services::omo::STANDARD),
                Some("omo-slim") => Some(&crate::services::omo::SLIM),
                _ => None,
            };
            if let Some(variant) = omo_variant {
                if db.is_omo_provider_current(app_type.as_str(), &provider.id, variant.category)? {
                    crate::services::OmoService::write_provider_config_to_file(provider, variant)?;
                }
                return Ok(());
            }
        }

        // Additive apps intentionally do not auto-create a Live provider when
        // the row exists only in the project DB.  A malformed existing file is
        // still an error from provider_exists_in_live_config, not a reason to
        // silently discard the user's Live configuration.
        if !crate::services::provider::live::provider_exists_in_live_config(app_type, &provider.id)?
        {
            return Ok(());
        }

        write_live_with_common_config(db, app_type, provider)?;
        return verify_additive_live_credentials(provider, app_type);
    }

    // Exclusive-mode apps own a single Live file. Only the effective current
    // provider may rewrite it; non-current edits stay DB-only.
    let effective_current = crate::settings::get_effective_current_provider(db, app_type)?;
    if effective_current.as_deref() != Some(provider.id.as_str()) {
        return Ok(());
    }

    write_live_with_common_config(db, app_type, provider)?;
    verify_live_credentials(provider, app_type)
}

fn verify_additive_live_credentials(
    provider: &Provider,
    app_type: &AppType,
) -> Result<(), AppError> {
    #[cfg(test)]
    if FAIL_NEXT_LIVE_VERIFICATION.swap(false, Ordering::SeqCst) {
        return Err(security_error("live_projection_failed"));
    }

    let live = match app_type {
        AppType::OpenCode => crate::opencode_config::get_providers()?
            .get(&provider.id)
            .cloned(),
        AppType::OpenClaw => crate::openclaw_config::get_provider(&provider.id)?,
        AppType::Hermes => crate::hermes_config::get_provider(&provider.id)?,
        _ => None,
    }
    .ok_or_else(|| security_error("live_projection_failed"))?;

    if super::recovery::credential_diffs(provider, &live, app_type)?.is_empty() {
        Ok(())
    } else {
        Err(security_error("live_projection_failed"))
    }
}

fn verify_live_credentials(provider: &Provider, app_type: &AppType) -> Result<(), AppError> {
    #[cfg(test)]
    if FAIL_NEXT_LIVE_VERIFICATION.swap(false, Ordering::SeqCst) {
        return Err(security_error("live_projection_failed"));
    }

    let live = ProviderService::read_live_settings(app_type.clone())
        .map_err(|_| security_error("live_projection_failed"))?;
    if super::recovery::credential_diffs(provider, &live, app_type)?.is_empty() {
        Ok(())
    } else {
        Err(security_error("live_projection_failed"))
    }
}

fn security_error(code: &str) -> AppError {
    AppError::Message(format!("ERROR:{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::services::provider::ProviderService;
    use crate::store::AppState;
    use serde_json::json;
    use serial_test::serial;
    use std::ffi::OsString;
    use tempfile::TempDir;

    struct IsolatedHome {
        previous: Option<OsString>,
        _temp: TempDir,
    }

    impl IsolatedHome {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("create isolated test home");
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
            Self {
                previous,
                _temp: temp,
            }
        }
    }

    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => std::env::set_var("CC_SWITCH_TEST_HOME", previous),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn provider_with_id(id: &str, key: &str, base_url: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            id.to_uppercase(),
            json!({"env": {
                "ANTHROPIC_AUTH_TOKEN": key,
                "ANTHROPIC_BASE_URL": base_url
            }}),
            None,
        )
    }

    fn provider(key: &str) -> Provider {
        provider_with_id("p1", key, "https://example.com")
    }

    fn openclaw_provider(id: &str, name: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            name.to_string(),
            json!({
                "baseUrl": "https://api.example.com",
                "apiKey": "sk-openclaw"
            }),
            None,
        )
    }

    fn confirmed_api_key() -> BTreeSet<String> {
        BTreeSet::from(["apiKey".to_string()])
    }

    fn setup_claude_provider(provider: &Provider) -> Result<(Arc<Database>, AppState), AppError> {
        let db = Arc::new(Database::memory()?);
        db.save_provider(AppType::Claude.as_str(), provider)?;
        db.set_current_provider(AppType::Claude.as_str(), &provider.id)?;
        crate::settings::set_current_provider(&AppType::Claude, Some(&provider.id))?;
        write_live_with_common_config(db.as_ref(), &AppType::Claude, provider)?;
        let state = AppState::new(db.clone());
        Ok((db, state))
    }

    #[test]
    fn stale_revision_is_rejected_without_overwrite() -> Result<(), AppError> {
        let db = Database::memory()?;
        let original = provider("sk-original");
        db.save_provider("claude", &original)?;
        let first = db
            .update_provider_cas("claude", &provider("sk-first"), 1)?
            .expect("first CAS");
        assert_eq!(first, 2);
        assert!(db
            .update_provider_cas("claude", &provider("sk-stale"), 1)?
            .is_none());
        let saved = db
            .get_provider_by_id("p1", "claude")?
            .expect("saved provider");
        assert_eq!(
            extract_provider_credentials(&saved, &AppType::Claude)
                .api_key
                .as_deref(),
            Some("sk-first")
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn switching_projects_db_credentials_over_different_live_credentials() -> Result<(), AppError> {
        let _home = IsolatedHome::new();
        let provider_a = provider_with_id("a", "sk-db-a", "https://a.example");
        let provider_b = provider_with_id("b", "sk-db-b", "https://b.example");
        let (db, state) = setup_claude_provider(&provider_a)?;
        db.save_provider(AppType::Claude.as_str(), &provider_b)?;

        let live_drift = provider_with_id("a", "sk-live-drift", "https://drift.example");
        write_live_snapshot(&AppType::Claude, &live_drift)?;

        ProviderService::switch(&state, AppType::Claude, "b")?;

        let stored_a = db
            .get_provider_by_id("a", AppType::Claude.as_str())?
            .expect("provider a remains in DB");
        let stored_a_fields = extract_provider_credentials(&stored_a, &AppType::Claude);
        assert_eq!(stored_a_fields.api_key.as_deref(), Some("sk-db-a"));
        assert_eq!(
            stored_a_fields.base_url.as_deref(),
            Some("https://a.example")
        );

        let live = ProviderService::read_live_settings(AppType::Claude)?;
        assert!(credential_diffs(&provider_b, &live, &AppType::Claude)?
            .is_empty());
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn stale_provider_revision_rejects_second_writer() -> Result<(), AppError> {
        let _home = IsolatedHome::new();
        let original = provider("sk-original");
        let (db, state) = setup_claude_provider(&original)?;

        let first = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::Claude,
                provider: provider("sk-first"),
                expected_revision: 1,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await?;
        assert!(matches!(first, MutationOutcome::Saved { revision: 2, .. }));

        let stale = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::Claude,
                provider: provider("sk-stale"),
                expected_revision: 1,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await?;
        assert!(matches!(
            stale,
            MutationOutcome::Conflict {
                current_revision: 2,
                ..
            }
        ));
        let stored = db
            .get_provider_by_id("p1", AppType::Claude.as_str())?
            .expect("provider remains");
        assert_eq!(
            extract_provider_credentials(&stored, &AppType::Claude)
                .api_key
                .as_deref(),
            Some("sk-first")
        );
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn compensated_projection_failure_removes_unapplied_audit_and_snapshot(
    ) -> Result<(), AppError> {
        let _home = IsolatedHome::new();
        let original = provider("sk-original");
        let (db, state) = setup_claude_provider(&original)?;

        FAIL_NEXT_LIVE_PROJECTION.store(true, Ordering::SeqCst);
        let error = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::Claude,
                provider: provider("sk-new"),
                expected_revision: 1,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await
            .expect_err("projection should fail");
        assert!(error.to_string().contains("live_projection_failed"));

        let (audit_count, snapshot_count): (i64, i64) = {
            let conn = lock_conn!(db.conn);
            (
                conn.query_row("SELECT COUNT(*) FROM provider_credential_audit", [], |row| {
                    row.get(0)
                })?,
                conn.query_row(
                    "SELECT COUNT(*) FROM provider_rollback_snapshots",
                    [],
                    |row| row.get(0),
                )?,
            )
        };
        assert_eq!(audit_count, 0);
        assert_eq!(snapshot_count, 0);
        assert_eq!(
            extract_provider_credentials(
                &db.get_provider_by_id("p1", AppType::Claude.as_str())?
                    .expect("provider remains"),
                &AppType::Claude,
            )
            .api_key
            .as_deref(),
            Some("sk-original")
        );
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn failed_projection_does_not_prune_a_valid_snapshot_before_compensation(
    ) -> Result<(), AppError> {
        let _home = IsolatedHome::new();
        let original = provider("sk-original");
        let (db, state) = setup_claude_provider(&original)?;
        {
            let conn = lock_conn!(db.conn);
            let now = chrono::Utc::now().timestamp_millis();
            for index in 0..crate::services::provider_security::ROLLBACK_MAX_VERSIONS {
                conn.execute(
                    "INSERT INTO provider_rollback_snapshots (
                        id, provider_id, app_type, provider_json, source_revision,
                        created_at, expires_at
                     ) VALUES (?1, 'p1', 'claude', '{}', ?2, ?3, ?4)",
                    params![
                        format!("existing-{index}"),
                        index as i64,
                        now - index as i64,
                        i64::MAX,
                    ],
                )?;
            }
        }

        FAIL_NEXT_LIVE_PROJECTION.store(true, Ordering::SeqCst);
        state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::Claude,
                provider: provider("sk-new"),
                expected_revision: 1,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await
            .expect_err("projection should fail");

        let count: i64 = lock_conn!(db.conn).query_row(
            "SELECT COUNT(*) FROM provider_rollback_snapshots WHERE provider_id = 'p1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            count,
            crate::services::provider_security::ROLLBACK_MAX_VERSIONS as i64
        );
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn unreadable_live_snapshot_is_not_replaced_during_compensation() -> Result<(), AppError>
    {
        let _home = IsolatedHome::new();
        let original = provider("sk-original");
        let (_db, state) = setup_claude_provider(&original)?;
        let live_path = crate::config::get_claude_settings_path();
        std::fs::write(&live_path, "{ malformed live settings").expect("write malformed live");

        FAIL_NEXT_LIVE_PROJECTION.store(true, Ordering::SeqCst);
        let error = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::Claude,
                provider: provider("sk-new"),
                expected_revision: 1,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await
            .expect_err("unsafe compensation must be rejected");

        assert!(error.to_string().contains("configuration_inconsistent"));
        assert_eq!(
            std::fs::read_to_string(live_path).expect("read malformed live"),
            "{ malformed live settings"
        );
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn live_projection_failure_rolls_back_and_blocks_until_recovery() -> Result<(), AppError>
    {
        let _home = IsolatedHome::new();
        let original = provider("sk-original");
        let (db, state) = setup_claude_provider(&original)?;

        FAIL_NEXT_LIVE_PROJECTION.store(true, Ordering::SeqCst);
        FAIL_NEXT_LIVE_COMPENSATION.store(true, Ordering::SeqCst);
        let failure = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::Claude,
                provider: provider("sk-new"),
                expected_revision: 1,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await
            .expect_err("failed compensation must lock the app");
        assert!(failure.to_string().contains("configuration_inconsistent"));
        assert_eq!(
            read_configuration_state(db.as_ref(), &AppType::Claude)?,
            ConfigurationState::Inconsistent
        );
        assert_eq!(
            read_configuration_state(db.as_ref(), &AppType::Gemini)?,
            ConfigurationState::Consistent,
            "the inconsistency lock is scoped to one app"
        );

        let revision = db
            .get_provider_revision(AppType::Claude.as_str(), "p1")?
            .expect("compensated revision");
        let blocked = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::Claude,
                provider: provider("sk-blocked"),
                expected_revision: revision,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await
            .expect_err("mutations remain blocked before recovery");
        assert!(blocked.to_string().contains("configuration_inconsistent"));

        let recovered = state
            .provider_mutation_coordinator
            .recover(AppType::Claude, RecoveryMode::ProjectDbToLive)
            .await?;
        assert!(recovered.live_fingerprint_verified);
        assert_eq!(recovered.state, ConfigurationState::Consistent);

        let after_recovery = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::Claude,
                provider: provider("sk-after-recovery"),
                expected_revision: recovered.revision,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await?;
        assert!(matches!(after_recovery, MutationOutcome::Saved { .. }));
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn db_only_additive_mutation_does_not_create_live_provider() -> Result<(), AppError> {
        let _home = IsolatedHome::new();
        let db = Arc::new(Database::memory()?);
        let original = openclaw_provider("db-only", "Original");
        db.save_provider(AppType::OpenClaw.as_str(), &original)?;
        let state = AppState::new(db.clone());

        let mut edited = original.clone();
        edited.name = "Edited".to_string();
        let outcome = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::OpenClaw,
                provider: edited,
                expected_revision: 1,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: BTreeSet::new(),
                skip_live_projection: false,
            })
            .await?;

        assert!(matches!(
            outcome,
            MutationOutcome::Saved { revision: 2, .. }
        ));
        assert!(
            !crate::openclaw_config::get_openclaw_config_path().exists(),
            "editing a DB-only additive provider must not auto-create Live configuration"
        );
        assert_eq!(
            db.get_provider_by_id("db-only", AppType::OpenClaw.as_str())?
                .expect("edited provider")
                .name,
            "Edited"
        );
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn additive_mutation_projects_credentials_when_provider_exists_in_live(
    ) -> Result<(), AppError> {
        let _home = IsolatedHome::new();
        let db = Arc::new(Database::memory()?);
        let original = openclaw_provider("live-managed", "Original");
        db.save_provider(AppType::OpenClaw.as_str(), &original)?;
        crate::openclaw_config::set_provider(&original.id, original.settings_config.clone())?;
        let state = AppState::new(db.clone());

        let mut edited = original.clone();
        edited.settings_config["apiKey"] = json!("sk-openclaw-edited");
        let outcome = state
            .provider_mutation_coordinator
            .mutate(ProviderMutationRequest {
                app_type: AppType::OpenClaw,
                provider: edited,
                expected_revision: 1,
                source: CredentialSource::ProviderEdit,
                confirmed_credential_fields: confirmed_api_key(),
                skip_live_projection: false,
            })
            .await?;

        assert!(matches!(
            outcome,
            MutationOutcome::Saved { revision: 2, .. }
        ));
        assert_eq!(
            crate::openclaw_config::get_provider("live-managed")?
                .expect("live provider after edit")
                .get("apiKey")
                .and_then(serde_json::Value::as_str),
            Some("sk-openclaw-edited")
        );
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn recovery_requires_write_read_fingerprint_agreement() -> Result<(), AppError> {
        let _home = IsolatedHome::new();
        let original = provider("sk-original");
        let (db, state) = setup_claude_provider(&original)?;
        persist_configuration_state(
            db.as_ref(),
            &AppType::Claude,
            ConfigurationState::Inconsistent,
            Some("test_inconsistent"),
        )?;

        FAIL_NEXT_LIVE_VERIFICATION.store(true, Ordering::SeqCst);
        let unverified = state
            .provider_mutation_coordinator
            .recover(AppType::Claude, RecoveryMode::ProjectDbToLive)
            .await
            .expect_err("write without verified readback must not unlock");
        assert!(unverified.to_string().contains("live_projection_failed"));
        assert_eq!(
            read_configuration_state(db.as_ref(), &AppType::Claude)?,
            ConfigurationState::Inconsistent
        );

        let verified = state
            .provider_mutation_coordinator
            .recover(AppType::Claude, RecoveryMode::ProjectDbToLive)
            .await?;
        assert!(verified.live_fingerprint_verified);
        assert_eq!(verified.state, ConfigurationState::Consistent);
        assert_eq!(
            read_configuration_state(db.as_ref(), &AppType::Claude)?,
            ConfigurationState::Consistent
        );
        Ok(())
    }
}

use std::future::Future;
use std::sync::{mpsc, Arc};
use std::task::Context;
use std::time::Duration;

use tauri::Manager;

use crate::database::Database;
use crate::provider::{Provider, UniversalProvider};
use crate::services::model_pricing::ModelPricingInfo;
use crate::store::AppState;

pub(super) struct TestHome {
    previous: Option<std::ffi::OsString>,
    native_overrides: Vec<(&'static str, Option<std::ffi::OsString>)>,
    _directory: tempfile::TempDir,
}

impl TestHome {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", directory.path());
        let native_overrides = [
            ("KIMI_CODE_HOME", ".kimi-code"),
            ("MINIMAX_DATA_DIR", ".minimax"),
            ("DSH_HOME", ".dsh"),
        ]
        .into_iter()
        .map(|(key, dir)| {
            let previous = std::env::var_os(key);
            std::env::set_var(key, directory.path().join(dir));
            (key, previous)
        })
        .collect();
        crate::settings::reload_settings().unwrap();
        Self {
            previous,
            native_overrides,
            _directory: directory,
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        for (key, previous) in &self.native_overrides {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        match &self.previous {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        let _ = crate::settings::reload_settings();
    }
}

/// Hold the same connection mutex as session sync, then poll the actual command.
/// It must yield before the lock is released, including on a single-thread runtime.
/// The watchdog only prevents a regression from hanging the test process.
fn while_connection_locked<F: Future>(db: &Arc<Database>, name: &str, command: F) -> F::Output {
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let db = db.clone();
    let holder = std::thread::spawn(move || {
        let _guard = db.conn.lock().unwrap();
        locked_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).is_ok()
    });
    locked_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let mut command = std::pin::pin!(command);
    let first_poll = command
        .as_mut()
        .poll(&mut Context::from_waker(futures::task::noop_waker_ref()));
    let _ = release_tx.send(());
    let released_by_caller = holder.join().unwrap();
    assert!(
        released_by_caller,
        "{name} blocked its polling thread on the database lock"
    );
    assert!(
        first_poll.is_pending(),
        "{name} must wait asynchronously for the database"
    );

    tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), command)
            .await
            .unwrap_or_else(|_| panic!("{name} did not complete after releasing the database"))
    })
}

#[test]
#[serial_test::serial]
fn startup_database_commands_yield_during_session_sync_contention() {
    let _home = TestHome::new();
    let db = Arc::new(Database::memory().unwrap());
    let provider = Provider::with_id(
        "test-provider".into(),
        "Test provider".into(),
        serde_json::json!({}),
        None,
    );
    db.save_provider("opencode", &provider).unwrap();
    db.save_provider("claude", &provider).unwrap();
    db.set_current_provider("claude", &provider.id).unwrap();
    let app = tauri::test::mock_builder()
        .manage(AppState::new(db.clone()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();

    macro_rules! query {
        ($name:ident $(, $arg:expr)*) => {
            while_connection_locked(&db, stringify!($name), super::$name(app.state() $(, $arg)*))
                .unwrap()
        };
    }

    let providers = query!(get_providers, "opencode".into());
    assert_eq!(providers[&provider.id].name, provider.name);
    assert_eq!(query!(get_current_provider, "claude".into()), provider.id);
    assert_eq!(
        query!(get_direct_provider, "claude".into()),
        Some(provider.id.clone())
    );
    let mode = query!(get_app_mode, "claude".into());
    assert_eq!(mode.mode, "direct");
    assert_eq!(mode.direct_provider_id, Some(provider.id));
    assert!(query!(list_profiles).profiles.is_empty());

    let pricing = ModelPricingInfo {
        model_id: "issue-9084-test-model".into(),
        display_name: "Test model".into(),
        input_cost_per_million: "1".into(),
        output_cost_per_million: "2".into(),
        cache_read_cost_per_million: "0".into(),
        cache_creation_cost_per_million: "0".into(),
    };
    let sync = query!(get_models_dev_sync_config);
    query!(save_models_dev_sync_config, sync.config);
    assert_eq!(query!(update_model_pricing_batch, vec![pricing.clone()]), 1);
    let prices = query!(get_model_pricing);
    assert!(prices
        .iter()
        .any(|entry| entry.model_id == pricing.model_id));
    query!(
        record_models_dev_sync_result,
        Some(1234),
        Some("offline".into())
    );
    let sync = query!(get_models_dev_sync_config);
    assert_eq!(sync.config.last_sync_at, Some(1234));
    assert_eq!(sync.config.last_sync_error.as_deref(), Some("offline"));
    query!(delete_model_pricing, pricing.model_id);
    assert!(query!(get_usage_data_sources).is_empty());

    // Validation errors must still reach the caller after moving work off-thread.
    assert!(
        tauri::async_runtime::block_on(super::get_providers(app.state(), "invalid".into()))
            .is_err()
    );
    let error = tauri::async_runtime::block_on(super::update_model_pricing_batch(
        app.state(),
        vec![ModelPricingInfo {
            model_id: String::new(),
            ..pricing
        }],
    ))
    .unwrap_err();
    assert!(matches!(
        error,
        crate::error::AppError::Localized {
            key: "usage.modelIdRequired",
            ..
        }
    ));

    // Native mutation commands can also wait behind a list refresh's app lock.
    // Their database work must not move that wait back onto the calling thread.
    // Keep another model selected so Kimi permits removing the test provider.
    let kimi_path = crate::kimi_config::config_path();
    std::fs::create_dir_all(kimi_path.parent().unwrap()).unwrap();
    std::fs::write(
        kimi_path,
        "default_model = 'kept'\n[providers.kept]\ntype = 'openai'\nbase_url = 'https://example.invalid/v1'\napi_key = 'test'\n[models.kept]\nprovider = 'kept'\nmodel = 'kept'\nmax_context_size = 32000\n",
    )
    .unwrap();
    let kimi = Provider::with_id(
        "test-kimi".into(),
        "Test Kimi".into(),
        serde_json::json!({
            "provider": {"type": "openai", "base_url": "https://example.invalid/v1", "api_key": "test"},
            "models": {"test-model": {"model": "test-model", "max_context_size": 32000}}
        }),
        None,
    );
    db.save_provider("kimi", &kimi).unwrap();
    query!(add_provider_to_live_config, "kimi".into(), kimi.id.clone());
    query!(
        remove_provider_from_live_config,
        "kimi".into(),
        kimi.id.clone()
    );
    query!(delete_provider, "kimi".into(), kimi.id.clone());
    assert!(db.get_provider_by_id(&kimi.id, "kimi").unwrap().is_none());
}

/// Representative commands per page a user can reach while session sync still
/// holds the lock: Skills, icon picker, universal providers, endpoint speed test,
/// profiles, network settings, MCP connectivity.
#[test]
#[serial_test::serial]
fn remaining_database_commands_yield_during_session_sync_contention() {
    let _home = TestHome::new();
    let db = Arc::new(Database::memory().unwrap());
    let provider = Provider::with_id(
        "test-provider".into(),
        "Test provider".into(),
        serde_json::json!({}),
        None,
    );
    db.save_provider("claude", &provider).unwrap();
    db.set_current_provider("claude", &provider.id).unwrap();
    let app = tauri::test::mock_builder()
        .manage(AppState::new(db.clone()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();

    macro_rules! query {
        ($name:ident $(, $arg:expr)*) => {
            while_connection_locked(&db, stringify!($name), super::$name(app.state() $(, $arg)*))
                .unwrap()
        };
    }
    // Skill commands take the state last.
    macro_rules! skill_query {
        ($name:ident $(, $arg:expr)*) => {
            while_connection_locked(&db, stringify!($name), super::$name($($arg,)* app.state()))
                .unwrap()
        };
    }

    assert!(query!(get_custom_icons).is_empty());
    assert_eq!(query!(get_global_proxy_url), None);
    assert!(query!(get_universal_providers).is_empty());
    assert!(query!(get_custom_endpoints, "claude".into(), provider.id.clone()).is_empty());
    assert!(query!(ensure_claude_desktop_official_provider));
    let batch = query!(
        start_mcp_connectivity_checks,
        Vec::new(),
        tauri::ipc::Channel::new(|_| Ok(()))
    );
    assert!(!batch.is_empty());

    let profile = query!(create_profile, "Test profile".into(), "claude".into());
    assert_eq!(
        query!(
            update_profile,
            profile.id.clone(),
            Some("Renamed".into()),
            None,
            None
        )
        .name,
        "Renamed"
    );
    query!(delete_profile, profile.id);
    query!(clear_current_profile, "claude".into());

    assert!(skill_query!(get_installed_skills).is_empty());
    skill_query!(scan_unmanaged_skills);
    assert!(skill_query!(get_skill_repos).is_empty());
    let repo = crate::services::skill::SkillRepo {
        owner: "octocat".into(),
        name: "skills".into(),
        branch: "main".into(),
        enabled: true,
    };
    assert!(skill_query!(add_skill_repo, repo));
    assert_eq!(skill_query!(get_skill_repos).len(), 1);
    assert!(skill_query!(
        remove_skill_repo,
        "octocat".into(),
        "skills".into()
    ));

    // Validation errors must still reach the caller after moving work off-thread.
    assert!(tauri::async_runtime::block_on(super::get_custom_endpoints(
        app.state(),
        "invalid".into(),
        provider.id
    ))
    .is_err());
    assert!(tauri::async_runtime::block_on(super::create_profile(
        app.state(),
        "x".into(),
        "nope".into()
    ))
    .is_err());
}

/// Pi and Oh My Pi read their state behind the app's switch lock, which a
/// provider switch holds while it waits for the database.
#[test]
#[serial_test::serial]
fn pi_state_commands_yield_while_a_provider_switch_holds_the_lock() {
    let _home = TestHome::new();
    let db = Arc::new(Database::memory().unwrap());
    let app = tauri::test::mock_builder()
        .manage(AppState::new(db))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();

    fn while_switch_locked<F: Future>(
        state: &AppState,
        app_type: &str,
        name: &str,
        command: F,
    ) -> F::Output {
        let guard =
            tauri::async_runtime::block_on(state.proxy_service.lock_switch_for_app(app_type));
        let mut command = std::pin::pin!(command);
        let first_poll = command
            .as_mut()
            .poll(&mut Context::from_waker(futures::task::noop_waker_ref()));
        drop(guard);
        assert!(
            first_poll.is_pending(),
            "{name} must wait asynchronously for the switch lock"
        );
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(5), command)
                .await
                .unwrap_or_else(|_| {
                    panic!("{name} did not complete after releasing the switch lock")
                })
        })
    }

    let state = app.state::<AppState>();
    let pi = while_switch_locked(
        &state,
        "pi",
        "get_pi_current_state",
        super::get_pi_current_state(app.state()),
    )
    .unwrap();
    assert!(pi.enabled_provider_ids.is_empty());
    let omp = while_switch_locked(
        &state,
        "omp",
        "get_omp_current_state",
        super::get_omp_current_state(app.state()),
    )
    .unwrap();
    assert!(omp.enabled_provider_ids.is_empty());
}

/// Profile and universal-provider mutations are read-modify-write and used to
/// queue on the main thread; now they must queue on their serial locks.
#[test]
#[serial_test::serial]
fn profile_and_universal_mutations_wait_for_their_serial_locks() {
    let _home = TestHome::new();
    let db = Arc::new(Database::memory().unwrap());
    let provider = Provider::with_id(
        "test-provider".into(),
        "Test provider".into(),
        serde_json::json!({}),
        None,
    );
    db.save_provider("claude", &provider).unwrap();
    db.set_current_provider("claude", &provider.id).unwrap();
    let app = tauri::test::mock_builder()
        .manage(AppState::new(db.clone()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();

    fn while_serialized<F: Future>(
        lock: &'static tokio::sync::Mutex<()>,
        name: &str,
        command: F,
    ) -> F::Output {
        let guard = tauri::async_runtime::block_on(lock.lock());
        let mut command = std::pin::pin!(command);
        let first_poll = command
            .as_mut()
            .poll(&mut Context::from_waker(futures::task::noop_waker_ref()));
        drop(guard);
        assert!(
            first_poll.is_pending(),
            "{name} must wait for the serial lock"
        );
        // tokio hands the released permit straight to the queued waiter.
        assert!(
            lock.try_lock().is_err(),
            "{name} must be queued on the serial lock"
        );
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(5), command)
                .await
                .unwrap_or_else(|_| panic!("{name} did not complete after releasing the lock"))
        })
    }

    while_serialized(
        &super::profile::PROFILE_LOCK,
        "clear_current_profile",
        super::clear_current_profile(app.state(), "claude".into()),
    )
    .unwrap();
    let profile = while_serialized(
        &super::profile::PROFILE_LOCK,
        "create_profile",
        super::create_profile(app.state(), "Serialized".into(), "claude".into()),
    )
    .unwrap();
    while_serialized(
        &super::profile::PROFILE_LOCK,
        "delete_profile",
        super::delete_profile(app.state(), profile.id),
    )
    .unwrap();

    let universal: UniversalProvider = serde_json::from_value(serde_json::json!({
        "id": "universal-test",
        "name": "Universal",
        "providerType": "custom",
        "apps": {},
        "baseUrl": "https://example.invalid/v1",
        "apiKey": "test",
    }))
    .unwrap();
    let state = app.state::<AppState>();
    assert!(while_serialized(
        &super::provider::UNIVERSAL_LOCK,
        "upsert_universal_provider",
        super::provider::universal_mutation(&state, move |state| {
            crate::services::ProviderService::upsert_universal(state, universal)
                .map_err(|e| e.to_string())
        }),
    )
    .unwrap());
    assert!(db
        .get_universal_provider("universal-test")
        .unwrap()
        .is_some());
}

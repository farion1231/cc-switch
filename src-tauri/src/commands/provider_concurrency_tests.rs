use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

use super::startup_tests::TestHome;
use crate::database::Database;
use crate::provider::Provider;
use crate::store::AppState;

#[test]
#[serial_test::serial]
fn native_provider_lists_wait_for_mutations_before_reading_snapshots() {
    let _home = TestHome::new();
    let db = Arc::new(Database::memory().unwrap());
    let app = tauri::test::mock_builder()
        .manage(AppState::new(db.clone()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();

    for app_type in ["kimi", "mcode", "dsh"] {
        let provider = Provider::with_id(
            "deleted".into(),
            "Deleted".into(),
            serde_json::json!({}),
            None,
        );
        db.save_provider(app_type, &provider).unwrap();
        let state = app.state::<AppState>();
        let guard =
            tauri::async_runtime::block_on(state.proxy_service.lock_switch_for_app(app_type));
        let mut listing = std::pin::pin!(super::get_providers(app.state(), app_type.into()));
        let early_result = tauri::async_runtime::block_on(async {
            // A mutation for this app must not stall unrelated provider reads.
            tokio::time::timeout(
                Duration::from_secs(5),
                super::get_providers(app.state(), "opencode".into()),
            )
            .await
            .unwrap()
            .unwrap();
            tokio::time::timeout(Duration::from_millis(200), listing.as_mut()).await
        });
        // Complete the in-flight mutation while still owning the app lock.
        db.delete_provider(app_type, &provider.id).unwrap();
        drop(guard);
        let waited = early_result.is_err();
        let listed = match early_result {
            Ok(result) => result,
            Err(_) => tauri::async_runtime::block_on(async {
                tokio::time::timeout(Duration::from_secs(5), listing)
                    .await
                    .unwrap()
            }),
        }
        .unwrap();
        assert!(waited, "{app_type} listed an unfinished mutation");
        assert!(!listed.contains_key(&provider.id));
        assert!(db
            .get_provider_by_id(&provider.id, app_type)
            .unwrap()
            .is_none());
    }
}

/// Pause a real list after it has read native and database snapshots. A FIFO
/// makes the nickname-file read wait without adding hooks to production code.
#[cfg(unix)]
#[test]
#[serial_test::serial]
fn deleting_during_native_list_refresh_does_not_resurrect_the_provider() {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::Instant;

    let _home = TestHome::new();
    let db = Arc::new(Database::memory().unwrap());
    let app = tauri::test::mock_builder()
        .manage(AppState::new(db.clone()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let native_file = crate::kimi_config::config_path();
    std::fs::create_dir_all(native_file.parent().unwrap()).unwrap();
    std::fs::write(
        &native_file,
        "[providers.deleted]\ntype = 'openai'\nbase_url = 'https://example.invalid/v1'\napi_key = 'test'\n",
    )
    .unwrap();
    db.save_provider(
        "kimi",
        &Provider::with_id(
            "deleted".into(),
            "Deleted".into(),
            serde_json::json!({"provider": {"type": "openai"}, "models": {}}),
            None,
        ),
    )
    .unwrap();
    let nickname_file = crate::config::get_app_config_dir().join("kimi_accounts.json");
    std::fs::create_dir_all(nickname_file.parent().unwrap()).unwrap();
    let fifo = std::ffi::CString::new(nickname_file.as_os_str().as_bytes()).unwrap();
    // SAFETY: fifo is a live, NUL-terminated path in this test's temporary directory.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);

    let handle = app.handle().clone();
    let listing = tauri::async_runtime::spawn(async move {
        super::get_providers(handle.state(), "kimi".into()).await
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut writer = loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&nickname_file)
        {
            Ok(writer) => break writer,
            Err(error)
                if error.raw_os_error() == Some(libc::ENXIO) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("list did not reach its nickname read: {error}"),
        }
    };

    let mut deleting = std::pin::pin!(super::delete_provider(
        app.state(),
        "kimi".into(),
        "deleted".into()
    ));
    let early_delete = tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_millis(200), deleting.as_mut()).await
    });
    // Always release the list before asserting, even if deletion ran too early.
    writer.write_all(b"{}").unwrap();
    drop(writer);
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), listing)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    });
    let waited = early_delete.is_err();
    match early_delete {
        Ok(result) => result,
        Err(_) => tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(5), deleting)
                .await
                .unwrap()
        }),
    }
    .unwrap();

    assert!(waited, "delete must wait for the in-flight list refresh");
    assert!(db.get_provider_by_id("deleted", "kimi").unwrap().is_none());
    assert!(!std::fs::read_to_string(&native_file)
        .unwrap()
        .contains("deleted"));
    std::fs::remove_file(&nickname_file).unwrap();
    std::fs::write(&nickname_file, "{}").unwrap();
    let refreshed =
        tauri::async_runtime::block_on(super::get_providers(app.state(), "kimi".into())).unwrap();
    assert!(!refreshed.contains_key("deleted"));
}

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codex_plus_core::launcher::{
    CodexLaunch, LaunchHooks, LaunchOptions, ProcessWaitStrategy, launch_and_inject_with_hooks,
};
use codex_plus_core::models::{DeleteResult, DeleteStatus, ExportResult, ExportStatus, SessionRef};
use codex_plus_core::routes::{
    BridgeContext, BridgeDataService, BridgeRuntimeService, BridgeSettingsService,
    CoreRuntimeService, handle_bridge_request,
};
use codex_plus_core::settings::BackendSettings;
use codex_plus_core::status::StatusStore;
use codex_plus_core::user_scripts::UserScriptManager;
use serde_json::{Value, json};

#[tokio::test]
async fn bridge_routes_cover_all_current_paths() {
    let ctx = test_context();

    let cases = [
        ("/settings/get", json!({})),
        ("/settings/set", json!({"providerSyncEnabled": true})),
        ("/user-scripts/list", json!({})),
        ("/user-scripts/set-enabled", json!({"enabled": false})),
        (
            "/user-scripts/set-script-enabled",
            json!({"key": "user:a.js", "enabled": false}),
        ),
        ("/user-scripts/delete", json!({"key": "user:a.js"})),
        ("/user-scripts/reload", json!({})),
        ("/devtools/open", json!({})),
        ("/manager/open", json!({})),
        ("/backend/status", json!({})),
        ("/codex-model-catalog", json!({})),
        ("/codex-config-model", json!({})),
        ("/ads", json!({})),
        ("/zed-remote/status", json!({})),
        (
            "/zed-remote/resolve-host",
            json!({"hostId": "remote-ssh-codex-managed:remote"}),
        ),
        (
            "/zed-remote/fallback-request",
            json!({"hostId": "remote-ssh-codex-managed:remote"}),
        ),
        (
            "/zed-remote/open",
            json!({"ssh": {"host": "example.com"}, "path": "/home/app.py"}),
        ),
        ("/zed-remote/projects", json!({})),
        (
            "/zed-remote/remember-project",
            json!({"ssh": {"host": "example.com"}, "path": "/home/app.py"}),
        ),
        (
            "/zed-remote/forget-project",
            json!({"id": "zed-remote-project:test"}),
        ),
        ("/upstream-worktree/status", json!({})),
        ("/upstream-worktree/defaults", json!({"repoPath": "/repo"})),
        (
            "/upstream-worktree/prepare",
            json!({"repoPath": "/repo", "remote": "upstream", "baseBranch": "main"}),
        ),
        (
            "/upstream-worktree/create",
            json!({"repoPath": "/repo", "branchName": "feature/demo"}),
        ),
        ("/stepwise/settings", json!({})),
        (
            "/stepwise/generate",
            json!({"request": {"lastUserMessage": "请继续", "lastAssistantMessage": "已完成"}}),
        ),
        ("/stepwise/test", json!({})),
        ("/delete", json!({"session_id": "s1", "title": "First"})),
        ("/undo", json!({"undo_token": "undo-1"})),
        (
            "/export-markdown",
            json!({"session_id": "s1", "title": "First"}),
        ),
        (
            "/thread-usage-history",
            json!({"session_id": "s1", "title": "First"}),
        ),
        ("/archived-thread", json!({"title": "Archived"})),
        (
            "/move-thread-workspace",
            json!({"session_id": "s1", "title": "First", "target_cwd": "/new"}),
        ),
        (
            "/thread-sort-key",
            json!({"session_id": "s1", "title": "First"}),
        ),
        (
            "/thread-sort-keys",
            json!({"sessions": [{"session_id": "s1", "title": "First"}]}),
        ),
    ];

    for (path, payload) in cases {
        let result = handle_bridge_request(ctx.clone(), path, payload).await;
        assert_ne!(
            result["message"], "Unknown bridge path",
            "{path} should be routed"
        );
    }
}

#[tokio::test]
async fn settings_get_includes_runtime_codex_app_version() {
    let ctx = BridgeContext::new(
        Arc::new(FakeSettings::with_codex_app_version("26.601.21317")),
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeData::default()),
    );

    let result = handle_bridge_request(ctx, "/settings/get", json!({})).await;

    assert_eq!(result["codexAppVersion"], json!("26.601.21317"));
    assert_eq!(result["codexAppPluginMarketplaceUnlock"], json!(true));
    assert_eq!(result.get("codexAppForcePluginInstall"), None);
    assert_eq!(result["codexAppThreadIdBadge"], json!(false));
}

#[tokio::test]
async fn settings_get_does_not_expose_stepwise_api_key_to_renderer() {
    let settings = BackendSettings {
        codex_app_stepwise_api_key: "sk-secret".to_string(),
        ..BackendSettings::default()
    };
    let ctx = BridgeContext::new(
        Arc::new(FakeSettings::with_settings(settings)),
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeData::default()),
    );

    let result = handle_bridge_request(ctx, "/settings/get", json!({})).await;

    assert!(result.get("codexAppStepwiseApiKey").is_none());
    assert_eq!(
        result["codexAppStepwiseApiKeyEnv"],
        json!("CODEX_STEPWISE_API_KEY")
    );
}

#[tokio::test]
async fn settings_set_does_not_persist_runtime_codex_app_version() {
    let settings = Arc::new(FakeSettings::with_codex_app_version("26.601.21317"));
    let ctx = BridgeContext::new(
        settings.clone(),
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeData::default()),
    );

    let result = handle_bridge_request(
        ctx,
        "/settings/set",
        json!({
            "codexAppVersion": "1.2.3",
            "codexAppPluginMarketplaceUnlock": false
        }),
    )
    .await;

    assert_eq!(result["codexAppVersion"], json!("26.601.21317"));
    assert_eq!(result["codexAppPluginMarketplaceUnlock"], json!(false));

    let persisted = settings.settings.lock().unwrap().clone();
    let persisted_value = serde_json::to_value(persisted).unwrap();
    assert!(persisted_value.get("codexAppVersion").is_none());
}

#[tokio::test]
async fn bridge_context_core_with_app_dir_exposes_runtime_codex_app_version() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp
        .path()
        .join("OpenAI.Codex_26.601.21317.0_x64__abc")
        .join("app");
    std::fs::create_dir_all(&app_dir).unwrap();
    std::fs::write(app_dir.join("Codex.exe"), "").unwrap();
    let ctx = BridgeContext::core_with_data_and_app_dir(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeData::default()),
        app_dir,
    );

    let result = handle_bridge_request(ctx, "/settings/get", json!({})).await;

    assert_eq!(result["codexAppVersion"], json!("26.601.21317.0"));
}

#[tokio::test]
async fn upstream_worktree_routes_are_dispatched_to_runtime() {
    let ctx = test_context();

    assert_eq!(
        handle_bridge_request(ctx.clone(), "/upstream-worktree/status", json!({})).await,
        json!({"status": "ok", "feature": "upstream-worktree"})
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/upstream-worktree/defaults",
            json!({"repoPath": "/repo"}),
        )
        .await,
        json!({
            "status": "ok",
            "repoRoot": "/repo",
            "defaultRemote": "upstream",
            "defaultBaseBranch": "main",
        })
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/upstream-worktree/create",
            json!({"repoPath": "/repo", "branchName": "feature/demo"}),
        )
        .await,
        json!({
            "status": "ok",
            "repoRoot": "/repo",
            "branchName": "feature/demo",
            "worktreePath": "/repo-feature-demo",
        })
    );
    assert_eq!(
        handle_bridge_request(
            ctx,
            "/upstream-worktree/prepare",
            json!({"repoPath": "/repo", "remote": "upstream", "baseBranch": "main"}),
        )
        .await,
        json!({
            "status": "ok",
            "repoRoot": "/repo",
            "sourceRef": "upstream/main",
            "qualifiedSourceRef": "refs/remotes/upstream/main",
        })
    );
}

#[tokio::test]
async fn stepwise_routes_use_settings_service() {
    let settings = BackendSettings {
        codex_app_stepwise_enabled: false,
        codex_app_stepwise_direct_send: true,
        codex_app_stepwise_model: "settings-service-stepwise".to_string(),
        codex_app_stepwise_max_items: 3,
        ..BackendSettings::default()
    };
    let ctx = BridgeContext::new(
        Arc::new(FakeSettings::with_settings(settings)),
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeData::default()),
    );

    let public_settings = handle_bridge_request(ctx.clone(), "/stepwise/settings", json!({})).await;
    assert_eq!(public_settings["settings"]["enabled"], json!(false));
    assert_eq!(public_settings["settings"]["directSend"], json!(true));
    assert_eq!(
        public_settings["settings"]["model"],
        json!("settings-service-stepwise")
    );
    assert_eq!(public_settings["settings"]["maxItems"], json!(3));
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/stepwise/generate",
            json!({"request": {"lastUserMessage": "请继续", "lastAssistantMessage": "已完成"}}),
        )
        .await,
        json!({
            "status": "ok",
            "disabled": true,
            "items": []
        })
    );
    assert_eq!(
        handle_bridge_request(ctx, "/stepwise/test", json!({})).await,
        json!({
            "status": "ok",
            "disabled": true,
            "items": []
        })
    );
}

#[tokio::test]
async fn unknown_bridge_path_preserves_empty_session_id_shape() {
    let result = handle_bridge_request(
        test_context(),
        "/missing",
        json!({"session_id": "should-not-leak"}),
    )
    .await;

    assert_eq!(
        result,
        json!({
            "status": "failed",
            "session_id": "",
            "message": "Unknown bridge path"
        })
    );
}

#[tokio::test]
async fn settings_routes_use_settings_service() {
    let ctx = test_context();

    let updated = handle_bridge_request(
        ctx.clone(),
        "/settings/set",
        json!({"providerSyncEnabled": true, "codexAppSessionDelete": false, "codexAppServiceTierControls": true, "codexAppPetRealMouseLook": true}),
    )
    .await;
    let loaded = handle_bridge_request(ctx, "/settings/get", json!({})).await;

    assert_eq!(updated["providerSyncEnabled"], true);
    assert_eq!(updated["codexAppSessionDelete"], false);
    assert_eq!(updated["codexAppServiceTierControls"], true);
    assert_eq!(updated["codexAppPetRealMouseLook"], true);
    assert_eq!(loaded, updated);
}

#[tokio::test]
async fn runtime_routes_keep_user_script_inventory_shape() {
    let ctx = test_context();

    let listed = handle_bridge_request(ctx.clone(), "/user-scripts/list", json!({})).await;
    let global = handle_bridge_request(
        ctx.clone(),
        "/user-scripts/set-enabled",
        json!({"enabled": false}),
    )
    .await;
    let script = handle_bridge_request(
        ctx.clone(),
        "/user-scripts/set-script-enabled",
        json!({"key": "user:a.js", "enabled": false}),
    )
    .await;
    let reloaded = handle_bridge_request(ctx, "/user-scripts/reload", json!({})).await;

    assert_eq!(listed["enabled"], true);
    assert_eq!(listed["scripts"][0]["key"], "builtin:demo.js");
    assert_eq!(global["enabled"], false);
    assert_eq!(script["scripts"][1]["enabled"], false);
    assert_eq!(reloaded["reloaded"], true);
    assert_eq!(reloaded["scripts"][0]["key"], "builtin:demo.js");
}

#[tokio::test]
async fn runtime_status_devtools_repair_and_ads_routes_are_dispatched() {
    let ctx = test_context();

    assert_eq!(
        handle_bridge_request(ctx.clone(), "/devtools/open", json!({})).await,
        json!({"status": "ok", "opened": true})
    );
    assert_eq!(
        handle_bridge_request(ctx.clone(), "/manager/open", json!({})).await,
        json!({"status": "ok", "opened": "manager"})
    );
    assert_eq!(
        handle_bridge_request(ctx.clone(), "/backend/status", json!({})).await,
        json!({"status": "ok", "message": "后端已连接", "version": codex_plus_core::version::VERSION})
    );
    assert_eq!(
        handle_bridge_request(ctx.clone(), "/ads", json!({})).await,
        json!({"version": 1, "ads": [{"id": "runtime-ad"}]})
    );
    assert_eq!(
        handle_bridge_request(ctx.clone(), "/zed-remote/status", json!({})).await,
        json!({"status": "ok", "platformSupported": true, "zedAppFound": true, "zedCliFound": false})
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/zed-remote/resolve-host",
            json!({"hostId": "remote-ssh-codex-managed:remote"}),
        )
        .await,
        json!({"status": "ok", "ssh": {"user": "longnv", "host": "192.168.100.31", "port": null}})
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/zed-remote/fallback-request",
            json!({"hostId": "remote-ssh-codex-managed:remote"}),
        )
        .await,
        json!({
            "status": "ok",
            "request": {
                "hostId": "remote-ssh-codex-managed:remote",
                "ssh": {"user": "longnv", "host": "192.168.100.31", "port": null},
                "path": "/Users/longnv/bin/repo/sealos-skills",
            }
        })
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/zed-remote/open",
            json!({"ssh": {"host": "example.com"}, "path": "/home/app.py"}),
        )
        .await,
        json!({"status": "ok", "url": "ssh://example.com/home/app.py", "strategy": "addToFocusedWorkspace"})
    );
    assert_eq!(
        handle_bridge_request(ctx.clone(), "/zed-remote/projects", json!({})).await,
        json!({
            "status": "ok",
            "projects": [{
                "id": "zed-remote-project:test",
                "label": "sealos-skills",
                "hostId": "remote-ssh-codex-managed:remote",
                "ssh": {"user": "longnv", "host": "192.168.100.31", "port": null},
                "path": "/Users/longnv/bin/repo/sealos-skills",
                "url": "ssh://longnv@192.168.100.31/Users/longnv/bin/repo/sealos-skills",
                "source": "codexRemoteProject",
                "lastOpenedAtMs": null,
                "isCurrent": false
            }]
        })
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/zed-remote/remember-project",
            json!({"ssh": {"host": "example.com"}, "path": "/home/app.py"}),
        )
        .await,
        json!({"status": "ok", "remembered": true})
    );
    assert_eq!(
        handle_bridge_request(
            ctx,
            "/zed-remote/forget-project",
            json!({"id": "zed-remote-project:test"}),
        )
        .await,
        json!({"status": "ok", "removed": 1})
    );
}

#[tokio::test]
async fn data_routes_forward_payloads_to_data_service() {
    let ctx = test_context();

    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/delete",
            json!({"session_id": "s1", "title": "First"}),
        )
        .await["undo_token"],
        "undo-s1"
    );
    assert_eq!(
        handle_bridge_request(ctx.clone(), "/undo", json!({"undo_token": "undo-s1"})).await,
        json!({
            "status": "undone",
            "session_id": "s1",
            "message": "undone",
            "undo_token": "undo-s1",
            "backup_path": null
        })
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/export-markdown",
            json!({"session_id": "s1", "title": "First"}),
        )
        .await["filename"],
        "First.md"
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/thread-usage-history",
            json!({"session_id": "s1", "title": "First"}),
        )
        .await,
        json!({
            "status": "ok",
            "session_id": "s1",
            "history": [
                {
                    "source": "rollout-history",
                    "conversation_id": "local:s1",
                    "turn_id": "turn-1",
                    "observed_at": "2026-06-02T05:00:00Z",
                    "usage": {
                        "inputTokens": 1200,
                        "outputTokens": 120,
                        "totalTokens": 1320,
                        "cachedTokens": 900,
                        "cacheReadTokens": 0,
                        "cacheCreationTokens": 0,
                        "contextUsed": 1320,
                        "contextLimit": 258400,
                        "hasBreakdown": true
                    }
                }
            ]
        })
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/archived-thread",
            json!({"title": "Archived"})
        )
        .await,
        json!({"session_id": "archived-1", "title": "Archived"})
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/move-thread-workspace",
            json!({"session_id": "s1", "title": "First", "target_cwd": "/new"}),
        )
        .await,
        json!({"status": "moved", "session_id": "s1", "target_cwd": "/new"})
    );
    assert_eq!(
        handle_bridge_request(
            ctx.clone(),
            "/thread-sort-key",
            json!({"session_id": "s1", "title": "First"}),
        )
        .await,
        json!({"status": "ok", "session_id": "s1", "updated_at": 123})
    );
    assert_eq!(
        handle_bridge_request(
            ctx,
            "/thread-sort-keys",
            json!({"sessions": [{"session_id": "s1", "title": "First"}, null, {"session_id": "s2"}]}),
        )
        .await,
        json!({"status": "ok", "sort_keys": [{"session_id": "s1"}, {"session_id": "s2"}]})
    );
}

#[tokio::test]
async fn bridge_context_core_with_data_uses_injected_data_service() {
    let ctx = BridgeContext::core_with_data(
        Arc::new(CoreRuntimeService::new(9229, StatusStore::default())),
        Arc::new(FakeData::default()),
    );

    let result = handle_bridge_request(
        ctx,
        "/delete",
        json!({"session_id": "s1", "title": "First"}),
    )
    .await;

    assert_eq!(result["status"], "local_deleted");
    assert_eq!(result["undo_token"], "undo-s1");
    assert_ne!(
        result["message"],
        "Delete service is not wired in core launcher hooks"
    );
}

#[tokio::test]
async fn user_script_manager_scans_and_persists_inventory_shape() {
    let temp = tempfile::tempdir().unwrap();
    let builtin_dir = temp.path().join("builtin");
    let user_dir = temp.path().join("user");
    std::fs::create_dir_all(&builtin_dir).unwrap();
    std::fs::write(builtin_dir.join("demo.js"), "window.demo = true;").unwrap();
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(user_dir.join("a.js"), "window.a = true;").unwrap();
    std::fs::write(user_dir.join("ignore.txt"), "not js").unwrap();
    let manager = UserScriptManager::new(
        builtin_dir.clone(),
        user_dir.clone(),
        temp.path().join("user_scripts.json"),
    );

    let listed = manager.inventory().unwrap();
    manager.set_global_enabled(false).unwrap();
    let disabled = manager.inventory().unwrap();
    manager.set_script_enabled("user:a.js", false).unwrap();
    let script_disabled = manager.inventory().unwrap();
    manager.delete_user_script("user:a.js").unwrap();
    let deleted = manager.inventory().unwrap();

    assert_eq!(listed["enabled"], true);
    assert_eq!(
        listed["builtin_dir"].as_str().unwrap(),
        builtin_dir.to_string_lossy()
    );
    assert_eq!(
        listed["user_dir"].as_str().unwrap(),
        user_dir.to_string_lossy()
    );
    assert_eq!(listed["scripts"][0]["key"], "builtin:demo.js");
    assert_eq!(listed["scripts"][0]["source"], "builtin");
    assert_eq!(listed["scripts"][0]["enabled"], true);
    assert_eq!(listed["scripts"][0]["status"], "not_loaded");
    assert_eq!(listed["scripts"][0]["error"], "");
    assert_eq!(listed["scripts"][1]["key"], "user:a.js");
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["scripts"][0]["status"], "disabled");
    assert_eq!(script_disabled["scripts"][1]["enabled"], false);
    assert_eq!(deleted["scripts"].as_array().unwrap().len(), 1);
    assert!(!user_dir.join("a.js").exists());
    assert_eq!(
        serde_json::from_str::<Value>(
            &std::fs::read_to_string(temp.path().join("user_scripts.json")).unwrap()
        )
        .unwrap(),
        json!({"enabled": false, "scripts": {}})
    );
}

#[tokio::test]
async fn user_script_manager_deletes_market_script_metadata_and_rejects_builtin_delete() {
    let temp = tempfile::tempdir().unwrap();
    let builtin_dir = temp.path().join("builtin");
    let user_dir = temp.path().join("user");
    std::fs::create_dir_all(&builtin_dir).unwrap();
    std::fs::write(builtin_dir.join("demo.js"), "window.demo = true;").unwrap();
    std::fs::create_dir_all(&user_dir).unwrap();
    let manager = UserScriptManager::new(
        builtin_dir,
        user_dir.clone(),
        temp.path().join("user_scripts.json"),
    );
    let script = codex_plus_core::script_market::MarketScript {
        id: "demo".to_string(),
        name: "Demo".to_string(),
        description: String::new(),
        version: "1.0.0".to_string(),
        author: String::new(),
        tags: Vec::new(),
        homepage: "https://example.com/demo".to_string(),
        script_url: "https://example.com/demo.js".to_string(),
        sha256: String::new(),
    };

    codex_plus_core::script_market::install_market_script_content(
        &manager,
        &script,
        b"window.demo = true;",
    )
    .unwrap();
    manager
        .set_script_enabled("user:market-demo.js", false)
        .unwrap();

    let error = manager.delete_user_script("builtin:demo.js").unwrap_err();
    assert!(error.to_string().contains("only user scripts"));
    manager.delete_user_script("user:market-demo.js").unwrap();

    assert!(!user_dir.join("market-demo.js").exists());
    assert!(
        manager.inventory().unwrap()["scripts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|script| script["market_id"] != "demo")
    );
    let saved = serde_json::from_str::<Value>(
        &std::fs::read_to_string(temp.path().join("user_scripts.json")).unwrap(),
    )
    .unwrap();
    assert!(saved.get("market").is_none());
    assert_eq!(saved["scripts"], json!({}));
}

#[tokio::test]
async fn core_runtime_reload_evaluates_enabled_user_bundle_and_status_is_ok() {
    let temp = tempfile::tempdir().unwrap();
    let builtin_dir = temp.path().join("builtin");
    std::fs::create_dir_all(&builtin_dir).unwrap();
    std::fs::write(builtin_dir.join("demo.js"), "window.demo = true;").unwrap();
    let manager = UserScriptManager::new(
        builtin_dir,
        temp.path().join("user"),
        temp.path().join("user_scripts.json"),
    );
    let evaluated = Arc::new(Mutex::new(Vec::<String>::new()));
    let runtime = CoreRuntimeService::new(9229, StatusStore::default())
        .with_user_scripts(manager)
        .with_user_script_evaluator({
            let evaluated = evaluated.clone();
            Arc::new(move |websocket_url, script| {
                evaluated
                    .lock()
                    .unwrap()
                    .push(format!("{websocket_url}:{script}"));
                Ok(json!({"status": "ok"}))
            })
        })
        .with_websocket_url("ws://page");
    let ctx = BridgeContext::core_with_data(Arc::new(runtime), Arc::new(FakeData::default()));

    let status = handle_bridge_request(ctx.clone(), "/backend/status", json!({})).await;
    let reloaded = handle_bridge_request(ctx, "/user-scripts/reload", json!({})).await;

    assert_eq!(
        status,
        json!({"status": "ok", "message": "后端已连接", "version": codex_plus_core::version::VERSION})
    );
    assert_eq!(reloaded["scripts"][0]["key"], "builtin:demo.js");
    let evaluated = evaluated.lock().unwrap();
    assert_eq!(evaluated.len(), 1);
    assert!(evaluated[0].starts_with("ws://page:"));
    assert!(evaluated[0].contains("window.demo = true;"));
}

#[tokio::test]
async fn core_runtime_open_devtools_uses_inspector_url_opener() {
    let opened = Arc::new(Mutex::new(Vec::<String>::new()));
    let runtime = CoreRuntimeService::new(9229, StatusStore::default())
        .with_devtools_opener({
            let opened = opened.clone();
            Arc::new(move |url| {
                opened.lock().unwrap().push(url.to_string());
                Ok(())
            })
        })
        .with_devtools_target_id("page-1");
    let ctx = BridgeContext::core_with_data(Arc::new(runtime), Arc::new(FakeData::default()));

    let result = handle_bridge_request(ctx, "/devtools/open", json!({})).await;

    assert_eq!(result["status"], "ok");
    assert_eq!(result["target_id"], "page-1");
    assert_eq!(
        opened.lock().unwrap().as_slice(),
        ["http://127.0.0.1:9229/devtools/inspector.html?ws=127.0.0.1:9229/devtools/page/page-1"]
    );
}

#[tokio::test]
async fn core_runtime_manager_route_attempts_to_open_manager_binary() {
    let ctx = BridgeContext::core(Arc::new(CoreRuntimeService::new(
        9229,
        StatusStore::default(),
    )));

    let result = handle_bridge_request(ctx, "/manager/open", json!({})).await;

    assert_ne!(result["message"], "管理工具启动未接入当前运行时");
}

#[tokio::test]
async fn bridge_backend_status_writes_diagnostic_log() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("codex-plus.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let ctx = BridgeContext::core(Arc::new(CoreRuntimeService::new(
        9229,
        StatusStore::default(),
    )));

    let result = handle_bridge_request(ctx, "/backend/status", json!({})).await;

    assert_eq!(result["status"], "ok");
    let contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("bridge.request"));
    assert!(contents.contains("bridge.backend_status_ok"));
    assert!(contents.contains("/backend/status"));
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
}

#[test]
fn user_script_manager_tolerates_bad_config_fields_and_updates_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("user_scripts.json");
    std::fs::write(
        &config_path,
        r#"{"enabled":"not bool","scripts":{"user:a.js":false,"user:b.js":"bad"},"custom":true}"#,
    )
    .unwrap();
    let manager = UserScriptManager::new(
        temp.path().join("builtin"),
        temp.path().join("user"),
        config_path.clone(),
    );

    assert_eq!(manager.load_config().enabled, true);
    assert_eq!(manager.load_config().scripts.get("user:a.js"), Some(&false));
    assert!(!manager.load_config().scripts.contains_key("user:b.js"));

    manager.set_script_enabled("user:c.js", false).unwrap();
    let saved = serde_json::from_str::<Value>(&std::fs::read_to_string(config_path).unwrap())
        .expect("config should remain valid JSON");

    assert_eq!(saved["enabled"], true);
    assert_eq!(saved["scripts"]["user:a.js"], false);
    assert_eq!(saved["scripts"]["user:c.js"], false);
}

#[test]
fn script_market_manifest_filters_invalid_entries() {
    let raw = serde_json::json!({
        "version": 1,
        "updated_at": "2026-05-21T00:00:00Z",
        "scripts": [
            {
                "id": "demo",
                "name": "Demo",
                "description": "Useful demo",
                "version": "1.0.0",
                "author": "BigPizzaV3",
                "tags": ["ui", 42],
                "homepage": "https://example.com/demo",
                "script_url": "https://example.com/demo.js",
                "sha256": ""
            },
            { "id": "", "name": "Bad", "version": "1", "script_url": "https://example.com/bad.js" },
            { "id": "missing-url", "name": "Bad", "version": "1" }
        ]
    });

    let manifest = codex_plus_core::script_market::parse_market_manifest(raw).unwrap();

    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.updated_at.as_deref(), Some("2026-05-21T00:00:00Z"));
    assert_eq!(manifest.scripts.len(), 1);
    assert_eq!(manifest.scripts[0].id, "demo");
    assert_eq!(manifest.scripts[0].tags, vec!["ui"]);
}

#[test]
fn user_script_inventory_includes_market_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let user_dir = temp.path().join("user");
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(user_dir.join("market-demo.js"), "window.demo = true;").unwrap();
    let manager = UserScriptManager::new(
        temp.path().join("builtin"),
        user_dir,
        temp.path().join("user_scripts.json"),
    );

    manager
        .record_market_install(&codex_plus_core::script_market::MarketScript {
            id: "demo".to_string(),
            name: "Demo".to_string(),
            description: "Useful demo".to_string(),
            version: "1.0.0".to_string(),
            author: "BigPizzaV3".to_string(),
            tags: vec!["ui".to_string()],
            homepage: "https://example.com/demo".to_string(),
            script_url: "https://example.com/demo.js".to_string(),
            sha256: String::new(),
        })
        .unwrap();

    let inventory = manager.inventory().unwrap();

    assert_eq!(inventory["scripts"][0]["key"], "user:market-demo.js");
    assert_eq!(inventory["scripts"][0]["market_id"], "demo");
    assert_eq!(inventory["scripts"][0]["version"], "1.0.0");
    assert_eq!(inventory["scripts"][0]["installed"], true);
    assert_eq!(
        inventory["scripts"][0]["source_url"],
        "https://example.com/demo.js"
    );
    assert_eq!(
        inventory["scripts"][0]["homepage"],
        "https://example.com/demo"
    );
}

#[test]
fn install_market_script_writes_file_and_records_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let manager = UserScriptManager::new(
        temp.path().join("builtin"),
        temp.path().join("user"),
        temp.path().join("user_scripts.json"),
    );
    let script = codex_plus_core::script_market::MarketScript {
        id: "demo".to_string(),
        name: "Demo".to_string(),
        description: String::new(),
        version: "1.0.0".to_string(),
        author: String::new(),
        tags: Vec::new(),
        homepage: "https://example.com/demo".to_string(),
        script_url: "https://example.com/demo.js".to_string(),
        sha256: String::new(),
    };

    codex_plus_core::script_market::install_market_script_content(
        &manager,
        &script,
        b"window.demo = true;",
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(temp.path().join("user").join("market-demo.js")).unwrap(),
        "window.demo = true;"
    );
    let inventory = manager.inventory().unwrap();
    assert_eq!(inventory["scripts"][0]["market_id"], "demo");
}

#[test]
fn install_market_script_ignores_checksum_mismatch_and_replaces_existing_file() {
    let temp = tempfile::tempdir().unwrap();
    let user_dir = temp.path().join("user");
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(user_dir.join("market-demo.js"), "old").unwrap();
    let manager = UserScriptManager::new(
        temp.path().join("builtin"),
        user_dir.clone(),
        temp.path().join("user_scripts.json"),
    );
    let script = codex_plus_core::script_market::MarketScript {
        id: "demo".to_string(),
        name: "Demo".to_string(),
        description: String::new(),
        version: "1.0.0".to_string(),
        author: String::new(),
        tags: Vec::new(),
        homepage: String::new(),
        script_url: "https://example.com/demo.js".to_string(),
        sha256: "0000".to_string(),
    };

    codex_plus_core::script_market::install_market_script_content(&manager, &script, b"new")
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(user_dir.join("market-demo.js")).unwrap(),
        "new"
    );
}

#[tokio::test]
async fn launch_lifecycle_uses_hook_supplied_bridge_context_for_injection() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = ContextHooks {
        events: events.clone(),
    };

    launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 57321,
            status_store: StatusStore::new(temp.path().join("latest-status.json")),
        },
        &hooks,
    )
    .await
    .unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "bridge-context:9229",
            "inject-bridge:9229:57321",
            "watchdog:9229:57321",
            "status:running",
        ]
    );
}

fn test_context() -> BridgeContext {
    BridgeContext::new(
        Arc::new(FakeSettings::default()),
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeData::default()),
    )
}

#[derive(Default)]
struct FakeSettings {
    settings: Mutex<BackendSettings>,
    codex_app_version: Mutex<String>,
}

impl FakeSettings {
    fn with_settings(settings: BackendSettings) -> Self {
        Self {
            settings: Mutex::new(settings),
            codex_app_version: Mutex::new(String::new()),
        }
    }

    fn with_codex_app_version(version: &str) -> Self {
        Self {
            settings: Mutex::new(BackendSettings::default()),
            codex_app_version: Mutex::new(version.to_string()),
        }
    }
}

#[async_trait]
impl BridgeSettingsService for FakeSettings {
    async fn get_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.settings.lock().unwrap().clone())
    }

    async fn set_settings(&self, payload: Value) -> anyhow::Result<BackendSettings> {
        let current = self.settings.lock().unwrap().clone();
        let mut raw = serde_json::to_value(current).unwrap();
        let raw = raw.as_object_mut().unwrap();
        if let Some(value) = payload.get("providerSyncEnabled").and_then(Value::as_bool) {
            raw.insert("providerSyncEnabled".to_string(), json!(value));
        }
        if let Some(value) = payload.get("enhancementsEnabled").and_then(Value::as_bool) {
            raw.insert("enhancementsEnabled".to_string(), json!(value));
        }
        for key in [
            "codexAppPluginMarketplaceUnlock",
            "codexAppModelWhitelistUnlock",
            "codexAppSessionDelete",
            "codexAppMarkdownExport",
            "codexAppForceChineseLocale",
            "codexAppProjectMove",
            "codexAppThreadIdBadge",
            "codexAppConversationView",
            "codexAppThreadScrollRestore",
            "codexAppZedRemoteOpen",
            "codexAppUpstreamWorktreeCreate",
            "codexAppNativeMenuPlacement",
            "codexAppServiceTierControls",
            "codexAppPetRealMouseLook",
        ] {
            if let Some(value) = payload.get(key).and_then(Value::as_bool) {
                raw.insert(key.to_string(), json!(value));
            }
        }
        if let Some(value) = payload.get("launchMode").and_then(Value::as_str) {
            raw.insert("launchMode".to_string(), json!(value));
        }
        if let Some(value) = payload.get("relayBaseUrl").and_then(Value::as_str) {
            raw.insert("relayBaseUrl".to_string(), json!(value));
        }
        if let Some(value) = payload.get("relayApiKey").and_then(Value::as_str) {
            raw.insert("relayApiKey".to_string(), json!(value));
        }
        let updated: BackendSettings = serde_json::from_value(Value::Object(raw.clone())).unwrap();
        *self.settings.lock().unwrap() = updated.clone();
        Ok(updated)
    }

    async fn codex_app_version(&self) -> anyhow::Result<String> {
        Ok(self.codex_app_version.lock().unwrap().clone())
    }
}

struct FakeRuntime {
    enabled: Mutex<bool>,
    script_enabled: Mutex<bool>,
}

impl Default for FakeRuntime {
    fn default() -> Self {
        Self {
            enabled: Mutex::new(true),
            script_enabled: Mutex::new(true),
        }
    }
}

#[async_trait]
impl BridgeRuntimeService for FakeRuntime {
    async fn user_script_inventory(&self) -> anyhow::Result<Value> {
        Ok(self.inventory(false))
    }

    async fn set_user_scripts_enabled(&self, enabled: bool) -> anyhow::Result<Value> {
        *self.enabled.lock().unwrap() = enabled;
        Ok(self.inventory(false))
    }

    async fn set_user_script_enabled(&self, key: String, enabled: bool) -> anyhow::Result<Value> {
        assert_eq!(key, "user:a.js");
        *self.script_enabled.lock().unwrap() = enabled;
        Ok(self.inventory(false))
    }

    async fn delete_user_script(&self, key: String) -> anyhow::Result<Value> {
        assert_eq!(key, "user:a.js");
        *self.script_enabled.lock().unwrap() = false;
        Ok(self.inventory(false))
    }

    async fn reload_user_scripts(&self) -> anyhow::Result<Value> {
        Ok(self.inventory(true))
    }

    async fn open_devtools(&self) -> anyhow::Result<Value> {
        Ok(json!({"status": "ok", "opened": true}))
    }

    async fn open_manager(&self) -> anyhow::Result<Value> {
        Ok(json!({"status": "ok", "opened": "manager"}))
    }

    async fn backend_status(&self) -> anyhow::Result<Value> {
        Ok(
            json!({"status": "ok", "message": "后端已连接", "version": codex_plus_core::version::VERSION}),
        )
    }

    async fn codex_model_catalog(&self) -> anyhow::Result<Value> {
        Ok(json!({
            "status": "ok",
            "model": "qwen3-coder",
            "default_model": "qwen3-coder",
            "model_provider": "relay",
            "provider_name": "Relay",
            "models": ["qwen3-coder"],
            "sources": []
        }))
    }

    async fn ads(&self) -> anyhow::Result<Value> {
        Ok(json!({"version": 1, "ads": [{"id": "runtime-ad"}]}))
    }

    async fn zed_remote_status(&self) -> anyhow::Result<Value> {
        Ok(json!({
            "status": "ok",
            "platformSupported": true,
            "zedAppFound": true,
            "zedCliFound": false
        }))
    }

    async fn resolve_zed_remote_host(&self, payload: Value) -> anyhow::Result<Value> {
        assert_eq!(payload["hostId"], json!("remote-ssh-codex-managed:remote"));
        Ok(json!({
            "status": "ok",
            "ssh": {"user": "longnv", "host": "192.168.100.31", "port": null}
        }))
    }

    async fn fallback_zed_remote_request(&self, payload: Value) -> anyhow::Result<Value> {
        assert_eq!(payload["hostId"], json!("remote-ssh-codex-managed:remote"));
        Ok(json!({
            "status": "ok",
            "request": {
                "hostId": "remote-ssh-codex-managed:remote",
                "ssh": {"user": "longnv", "host": "192.168.100.31", "port": null},
                "path": "/Users/longnv/bin/repo/sealos-skills",
            }
        }))
    }

    async fn open_zed_remote(&self, payload: Value) -> anyhow::Result<Value> {
        assert_eq!(payload["path"], json!("/home/app.py"));
        Ok(
            json!({"status": "ok", "url": "ssh://example.com/home/app.py", "strategy": "addToFocusedWorkspace"}),
        )
    }

    async fn list_zed_remote_projects(&self, _payload: Value) -> anyhow::Result<Value> {
        Ok(json!({
            "status": "ok",
            "projects": [{
                "id": "zed-remote-project:test",
                "label": "sealos-skills",
                "hostId": "remote-ssh-codex-managed:remote",
                "ssh": {"user": "longnv", "host": "192.168.100.31", "port": null},
                "path": "/Users/longnv/bin/repo/sealos-skills",
                "url": "ssh://longnv@192.168.100.31/Users/longnv/bin/repo/sealos-skills",
                "source": "codexRemoteProject",
                "lastOpenedAtMs": null,
                "isCurrent": false
            }]
        }))
    }

    async fn remember_zed_remote_project(&self, payload: Value) -> anyhow::Result<Value> {
        assert_eq!(payload["path"], json!("/home/app.py"));
        Ok(json!({"status": "ok", "remembered": true}))
    }

    async fn forget_zed_remote_project(&self, payload: Value) -> anyhow::Result<Value> {
        assert_eq!(payload["id"], json!("zed-remote-project:test"));
        Ok(json!({"status": "ok", "removed": 1}))
    }

    async fn upstream_worktree_status(&self) -> anyhow::Result<Value> {
        Ok(json!({"status": "ok", "feature": "upstream-worktree"}))
    }

    async fn upstream_worktree_defaults(&self, payload: Value) -> anyhow::Result<Value> {
        assert_eq!(payload["repoPath"], json!("/repo"));
        Ok(json!({
            "status": "ok",
            "repoRoot": "/repo",
            "defaultRemote": "upstream",
            "defaultBaseBranch": "main",
        }))
    }

    async fn upstream_worktree_prepare(&self, payload: Value) -> anyhow::Result<Value> {
        assert_eq!(payload["repoPath"], json!("/repo"));
        assert_eq!(payload["remote"], json!("upstream"));
        assert_eq!(payload["baseBranch"], json!("main"));
        Ok(json!({
            "status": "ok",
            "repoRoot": "/repo",
            "sourceRef": "upstream/main",
            "qualifiedSourceRef": "refs/remotes/upstream/main",
        }))
    }

    async fn upstream_worktree_create(&self, payload: Value) -> anyhow::Result<Value> {
        assert_eq!(payload["repoPath"], json!("/repo"));
        assert_eq!(payload["branchName"], json!("feature/demo"));
        Ok(json!({
            "status": "ok",
            "repoRoot": "/repo",
            "branchName": "feature/demo",
            "worktreePath": "/repo-feature-demo",
        }))
    }
}

impl FakeRuntime {
    fn inventory(&self, reloaded: bool) -> Value {
        json!({
            "enabled": *self.enabled.lock().unwrap(),
            "reloaded": reloaded,
            "scripts": [
                {"key": "builtin:demo.js", "name": "demo.js", "enabled": true},
                {"key": "user:a.js", "name": "a.js", "enabled": *self.script_enabled.lock().unwrap()}
            ]
        })
    }
}

struct FakeData;

impl Default for FakeData {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl BridgeDataService for FakeData {
    async fn delete(&self, session: SessionRef) -> anyhow::Result<DeleteResult> {
        Ok(DeleteResult {
            status: DeleteStatus::LocalDeleted,
            session_id: session.session_id.clone(),
            message: format!("deleted {}", session.title),
            undo_token: Some(format!("undo-{}", session.session_id)),
            backup_path: None,
        })
    }

    async fn undo(&self, undo_token: String) -> anyhow::Result<DeleteResult> {
        Ok(DeleteResult {
            status: DeleteStatus::Undone,
            session_id: "s1".to_string(),
            message: "undone".to_string(),
            undo_token: Some(undo_token),
            backup_path: None,
        })
    }

    async fn export_markdown(&self, session: SessionRef) -> anyhow::Result<ExportResult> {
        Ok(ExportResult {
            status: ExportStatus::Exported,
            session_id: session.session_id,
            message: "exported".to_string(),
            filename: Some("First.md".to_string()),
            markdown: Some("# First\n".to_string()),
        })
    }

    async fn thread_usage_history(&self, session: SessionRef) -> anyhow::Result<Value> {
        Ok(json!({
            "status": "ok",
            "session_id": session.session_id,
            "history": [
                {
                    "source": "rollout-history",
                    "conversation_id": "local:s1",
                    "turn_id": "turn-1",
                    "observed_at": "2026-06-02T05:00:00Z",
                    "usage": {
                        "inputTokens": 1200,
                        "outputTokens": 120,
                        "totalTokens": 1320,
                        "cachedTokens": 900,
                        "cacheReadTokens": 0,
                        "cacheCreationTokens": 0,
                        "contextUsed": 1320,
                        "contextLimit": 258400,
                        "hasBreakdown": true
                    }
                }
            ]
        }))
    }

    async fn find_archived_thread_by_title(
        &self,
        title: String,
    ) -> anyhow::Result<Option<SessionRef>> {
        Ok(Some(SessionRef {
            session_id: "archived-1".to_string(),
            title,
        }))
    }

    async fn move_thread_workspace(
        &self,
        session: SessionRef,
        target_cwd: String,
    ) -> anyhow::Result<Value> {
        Ok(json!({"status": "moved", "session_id": session.session_id, "target_cwd": target_cwd}))
    }

    async fn thread_sort_key(&self, session: SessionRef) -> anyhow::Result<Value> {
        Ok(json!({"status": "ok", "session_id": session.session_id, "updated_at": 123}))
    }

    async fn thread_sort_keys(&self, sessions: Vec<SessionRef>) -> anyhow::Result<Value> {
        Ok(json!({
            "status": "ok",
            "sort_keys": sessions
                .into_iter()
                .map(|session| json!({"session_id": session.session_id}))
                .collect::<Vec<_>>()
        }))
    }
}

#[derive(Clone)]
struct ContextHooks {
    events: Arc<Mutex<Vec<String>>>,
}

impl ContextHooks {
    fn event(&self, event: impl Into<String>) {
        self.events.lock().unwrap().push(event.into());
    }
}

#[async_trait(?Send)]
impl LaunchHooks for ContextHooks {
    fn resolve_app_dir(
        &self,
        app_dir: Option<&std::path::Path>,
        _settings: &BackendSettings,
    ) -> anyhow::Result<std::path::PathBuf> {
        app_dir
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("missing app dir"))
    }

    fn select_debug_port(&self, requested: u16) -> u16 {
        requested
    }

    fn select_helper_port(&self, requested: u16) -> u16 {
        requested
    }

    async fn load_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(BackendSettings::default())
    }

    async fn run_provider_sync(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_helper(&self, _helper_port: u16) -> anyhow::Result<()> {
        Ok(())
    }

    async fn launch_codex(
        &self,
        _app_dir: &std::path::Path,
        _debug_port: u16,
        _settings: &BackendSettings,
        _extra_args: &[String],
    ) -> anyhow::Result<CodexLaunch> {
        Ok(CodexLaunch::Process {
            command: vec!["codex".to_string()],
            wait_strategy: ProcessWaitStrategy::TrackedChild,
            macos_cleanup_policy: None,
        })
    }

    async fn bridge_context(
        &self,
        debug_port: u16,
        _app_dir: &std::path::Path,
    ) -> anyhow::Result<Option<BridgeContext>> {
        self.event(format!("bridge-context:{debug_port}"));
        Ok(Some(test_context()))
    }

    async fn inject(&self, _debug_port: u16, _helper_port: u16) -> anyhow::Result<()> {
        anyhow::bail!("legacy inject should not run when bridge context is supplied")
    }

    async fn inject_bridge(
        &self,
        debug_port: u16,
        helper_port: u16,
        _ctx: BridgeContext,
    ) -> anyhow::Result<()> {
        self.event(format!("inject-bridge:{debug_port}:{helper_port}"));
        Ok(())
    }

    async fn start_bridge_watchdog(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        self.event(format!("watchdog:{debug_port}:{helper_port}"));
        Ok(())
    }

    async fn write_status(&self, status: &str) {
        self.event(format!("status:{status}"));
    }

    async fn wait_for_codex_exit(&self, _launch: &CodexLaunch) -> anyhow::Result<()> {
        Ok(())
    }

    async fn shutdown_helper(&self, _helper_port: u16) {}

    async fn terminate_codex(&self, _launch: &CodexLaunch) {}
}

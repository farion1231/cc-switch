//! 双模式的验收：进入 / 退出只动关键字段和独有字段；契约相同的换路由不碰客户端文件；
//! 代理路由和直连指针互相独立；每一步崩溃都能按 pending 补完；旧版遗留的接管状态
//! 在启动时迁移掉。
use super::*;
use crate::database::Database;
use crate::live::engine::DeviceStore;
use crate::mode::operation::failpoint;
use crate::mode::stack::StackMemberView;
use crate::mode::state::{self, Mode};
use crate::services::provider::ProviderService;
use serde_json::{json, Value};
use serial_test::serial;
use std::ffi::OsString;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

struct Home {
    dir: TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl Home {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp home");
        let saved = ["HOME", "USERPROFILE", "CC_SWITCH_TEST_HOME"]
            .into_iter()
            .map(|key| {
                let old = std::env::var_os(key);
                std::env::set_var(key, dir.path());
                (key, old)
            })
            .collect();
        crate::settings::reload_settings().expect("reload settings");
        Self { dir, saved }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        for (key, old) in self.saved.drain(..) {
            match old {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let _ = crate::settings::reload_settings();
    }
}

fn renamed(mut provider: Provider, name: &str) -> Provider {
    provider.name = name.to_string();
    provider
}

fn claude(id: &str, url: &str, extra: Value) -> Provider {
    let mut env = json!({
        "ANTHROPIC_BASE_URL": url,
        "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}"),
        "ANTHROPIC_MODEL": "claude-sonnet-4-6"
    });
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            env[key] = value.clone();
        }
    }
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({ "env": env }),
        None,
    )
}

async fn state_with(app: AppType, rows: &[Provider], current: &str) -> AppState {
    let db = Arc::new(Database::memory().expect("memory db"));
    for row in rows {
        db.save_provider(&app, row).expect("save provider");
    }
    db.set_current_provider(&app, current).expect("set current");
    crate::settings::set_current_provider(&app, Some(current)).expect("local current");
    let mut proxy = db.get_global_proxy_config().await.expect("ephemeral port");
    proxy.listen_port = 0;
    db.update_global_proxy_config(proxy)
        .await
        .expect("ephemeral port");
    AppState::new(db)
}

fn settings_path() -> std::path::PathBuf {
    crate::config::get_claude_settings_path()
}

fn seed_settings(text: &str) {
    let path = settings_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// 落定后的 `settings.json`：切换写进去的空值墓碑几秒后才删（见 `claude_direct`），
/// 这里先把它们删完再读。
fn settled_bytes() -> Vec<u8> {
    crate::services::provider::claude_direct::settle_tombstones();
    fs::read(settings_path()).unwrap()
}

fn settled_string() -> String {
    String::from_utf8(settled_bytes()).unwrap()
}

fn settings() -> Value {
    serde_json::from_slice(&settled_bytes()).unwrap()
}

fn mode(app: &AppType) -> ModeState {
    state::mode_state(&DeviceStore::for_device(), app.as_str()).unwrap()
}

fn in_use(state: &AppState, app: &AppType) -> Option<String> {
    current::provider_for(&state.db, app, Purpose::InUse).unwrap()
}

fn direct(state: &AppState, app: &AppType) -> Option<String> {
    current::provider_for(&state.db, app, Purpose::Direct).unwrap()
}

const USER_SETTINGS: &str = r#"{
  "hooks": {
    "Stop": []
  },
  "env": {
    "ANTHROPIC_BASE_URL": "https://a.example",
    "ANTHROPIC_AUTH_TOKEN": "sk-a",
    "ANTHROPIC_MODEL": "claude-sonnet-4-6",
    "DISABLE_TELEMETRY": "1"
  },
  "permissions": {
    "allow": [
      "Bash"
    ]
  }
}
"#;

/// 回到直连后：值和用户的原文件相同；非关键字段的顺序不变（关键字段可能挪到末尾）。
fn assert_back_to_user_settings() {
    let original: Value = serde_json::from_str(USER_SETTINGS).unwrap();
    let live = settings();
    assert_eq!(live, original);
    let keys = |value: &Value| -> Vec<String> {
        value
            .as_object()
            .unwrap()
            .keys()
            .filter(|key| !crate::live::floor::claude_floor_env(key))
            .cloned()
            .collect()
    };
    assert_eq!(keys(&live), keys(&original));
    assert_eq!(keys(&live["env"]), keys(&original["env"]));
}

#[tokio::test]
#[serial]
async fn entering_and_leaving_proxy_mode_only_touches_key_and_exclusive_fields() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;

    enter(&state, &AppType::Claude, false).await.expect("enter");
    let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
    let live = settings();
    assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], proxy_url.as_str());
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], PROXY_TOKEN_PLACEHOLDER);
    assert_eq!(
        live["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
        "claude-sonnet-5"
    );
    assert!(live["env"].get("ANTHROPIC_MODEL").is_none());
    assert_eq!(live["env"]["DISABLE_TELEMETRY"], "1");
    assert_eq!(live["hooks"], json!({ "Stop": [] }));
    let entered = mode(&AppType::Claude);
    assert_eq!(entered.mode, Some(Mode::Proxy));
    assert!(entered.attached);
    assert_eq!(entered.proxy_route.as_deref(), Some("a"));
    assert!(entered.contract.is_some());
    assert!(
        state.db.get_proxy_flags_sync(&AppType::Claude).0,
        "enabled mirrors the mode"
    );

    exit(&state, &AppType::Claude).await.expect("exit");
    assert_back_to_user_settings();
    // 第一次写入可能挪动关键字段的位置，之后的往返字节稳定。
    let settled = settled_bytes();
    enter(&state, &AppType::Claude, false)
        .await
        .expect("enter again");
    exit(&state, &AppType::Claude).await.expect("exit again");
    assert_eq!(settled_bytes(), settled);
    let left = mode(&AppType::Claude);
    assert_eq!(left.mode, Some(Mode::Direct));
    assert_eq!(left.proxy_route.as_deref(), Some("a"), "the route is kept");
    assert!(!state.db.get_proxy_flags_sync(&AppType::Claude).0);
    assert!(!state.proxy_service.is_running().await);
}

#[test]
fn route_switch_results_serialize_the_renderer_contract() {
    for (kind, name) in [
        (RouteSwitchKind::Remembered, "remembered"),
        (RouteSwitchKind::Route, "route"),
        (RouteSwitchKind::Stack, "stack"),
    ] {
        for changed in [false, true] {
            let result = RouteSwitchResult {
                kind: kind.clone(),
                client_config_changed: changed,
            };
            assert_eq!(
                serde_json::to_value(result).unwrap(),
                json!({ "kind": name, "clientConfigChanged": changed })
            );
        }
    }
    assert_eq!(
        serde_json::to_value(crate::services::provider::SwitchResult::default()).unwrap(),
        json!({ "warnings": [] })
    );
}

#[tokio::test]
#[serial]
async fn route_switch_results_follow_the_applied_mode_not_the_viewed_tab() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[
            claude("a", "https://a.example", json!({})),
            // 名字也一样：`/model` 的说明带供应商名，名字不同契约就不同。
            renamed(claude("b", "https://b.example", json!({})), "A"),
        ],
        "a",
    )
    .await;
    let unchanged = settled_bytes();
    for _ in 0..2 {
        let result = set_route(&state, &AppType::Claude, "b").await.unwrap();
        assert_eq!(
            result,
            RouteSwitchResult {
                kind: RouteSwitchKind::Remembered,
                client_config_changed: false,
            }
        );
    }
    assert_eq!(settled_bytes(), unchanged);
    assert!(!current::is_proxy(&AppType::Claude));
    enter(&state, &AppType::Claude, false).await.unwrap();
    let result = set_route(&state, &AppType::Claude, "a").await.unwrap();
    assert_eq!(
        result,
        RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: false,
        }
    );
    enter(&state, &AppType::Claude, true).await.unwrap();
    let result = set_route(&state, &AppType::Claude, "b").await.unwrap();
    assert_eq!(result.kind, RouteSwitchKind::Stack);
    let result = ProviderService::switch(&state, AppType::Claude, "a").unwrap();
    assert_eq!(result.route.unwrap().kind, RouteSwitchKind::Stack);
    // A detached proxy mode only remembers a target: the client is back on direct config.
    detach_all(&state).await;
    let result = set_route(&state, &AppType::Claude, "b").await.unwrap();
    assert_eq!(
        result,
        RouteSwitchResult {
            kind: RouteSwitchKind::Remembered,
            client_config_changed: false,
        }
    );
    exit(&state, &AppType::Claude).await.unwrap();
    let result = ProviderService::switch(&state, AppType::Claude, "b").unwrap();
    assert!(result.route.is_none());
}

#[tokio::test]
#[serial]
async fn a_route_switch_with_the_same_contract_leaves_the_client_file_alone() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[
            claude("a", "https://a.example", json!({})),
            // 名字也一样：`/model` 的说明带供应商名，名字不同契约就不同。
            renamed(claude("b", "https://b.example", json!({})), "A"),
        ],
        "a",
    )
    .await;
    enter(&state, &AppType::Claude, false).await.expect("enter");
    let before = settled_bytes();
    let mtime = fs::metadata(settings_path()).unwrap().modified().unwrap();

    // 契约相同时客户端文件不读也不写：连读权限都拿掉，换路由照样成功。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(settings_path(), fs::Permissions::from_mode(0o000)).unwrap();
    }
    ProviderService::switch(&state, AppType::Claude, "b").expect("switch route");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(settings_path(), fs::Permissions::from_mode(0o600)).unwrap();
    }

    assert_eq!(settled_bytes(), before);
    assert_eq!(
        fs::metadata(settings_path()).unwrap().modified().unwrap(),
        mtime
    );
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("b"));
    assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("a"));

    exit(&state, &AppType::Claude).await.expect("exit");
    assert_eq!(settings()["env"]["ANTHROPIC_BASE_URL"], "https://a.example");
}

#[tokio::test]
#[serial]
async fn the_route_providers_exclusive_fields_follow_the_contract() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[
            claude("a", "https://a.example", json!({})),
            claude(
                "deepseek",
                "https://deepseek.example",
                json!({ "CLAUDE_CODE_DISABLE_ARTIFACT": "1" }),
            ),
        ],
        "a",
    )
    .await;
    enter(&state, &AppType::Claude, false).await.expect("enter");
    ProviderService::switch(&state, AppType::Claude, "deepseek").expect("switch route");
    assert_eq!(settings()["env"]["CLAUDE_CODE_DISABLE_ARTIFACT"], "1");
    assert_eq!(
        mode(&AppType::Claude).contract.unwrap().exclusive["CLAUDE_CODE_DISABLE_ARTIFACT"],
        "1"
    );

    exit(&state, &AppType::Claude).await.expect("exit");
    assert!(
        settings()["env"]
            .get("CLAUDE_CODE_DISABLE_ARTIFACT")
            .is_none(),
        "the direct provider does not need it, so leaving proxy mode removes it"
    );
    assert_back_to_user_settings();
}

#[tokio::test]
#[serial]
async fn the_proxy_route_is_independent_of_the_direct_pointer() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[
            claude("a", "https://a.example", json!({})),
            claude("b", "https://b.example", json!({})),
        ],
        "a",
    )
    .await;
    enter(&state, &AppType::Claude, false).await.expect("enter");
    ProviderService::switch(&state, AppType::Claude, "b").expect("route to b");
    exit(&state, &AppType::Claude).await.expect("exit");
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));

    enter(&state, &AppType::Claude, false)
        .await
        .expect("enter again");
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("b"));

    // 退出 CC Switch 再启动：分离时写回直连，启动时按保存的路由接上。
    detach_all(&state).await;
    assert!(!mode(&AppType::Claude).attached);
    assert_eq!(settings()["env"]["ANTHROPIC_BASE_URL"], "https://a.example");
    startup(&state).await;
    let restarted = mode(&AppType::Claude);
    assert!(restarted.attached);
    assert_eq!(restarted.proxy_route.as_deref(), Some("b"));
    assert_eq!(
        settings()["env"]["ANTHROPIC_AUTH_TOKEN"],
        PROXY_TOKEN_PLACEHOLDER
    );
    exit(&state, &AppType::Claude).await.expect("exit");
}

/// 代理模式下不能切到官方供应商：经代理访问官方 API 可能导致账号被封。拒绝时路由和
/// 客户端文件都保持原样。
#[tokio::test]
#[serial]
async fn proxy_mode_refuses_an_official_provider_and_keeps_the_route() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let mut official = claude("official", "https://api.anthropic.com", json!({}));
    official.category = Some("official".to_string());
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({})), official],
        "a",
    )
    .await;
    enter(&state, &AppType::Claude, false).await.expect("enter");
    let before = settled_bytes();

    let err = ProviderService::switch(&state, AppType::Claude, "official")
        .expect_err("an official provider is refused in proxy mode");

    assert!(
        matches!(
            err,
            AppError::Localized {
                key: "switch.official_blocked_by_proxy",
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(mode(&AppType::Claude).proxy_route.as_deref(), Some("a"));
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));
    assert_eq!(settled_bytes(), before);
    exit(&state, &AppType::Claude).await.expect("exit");
}

/// 保存当前供应商要等进入代理写完契约，之后按代理模式处理，不能拿进入前读到的直连
/// 模式把关键字段写回 live。
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn saving_the_current_provider_waits_for_entering_proxy_mode() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state: &'static AppState = Box::leak(Box::new(
        state_with(
            AppType::Claude,
            &[claude("a", "https://a.example", json!({}))],
            "a",
        )
        .await,
    ));

    let guard = lock_settled(state, &AppType::Claude).await.unwrap();
    let updater = std::thread::spawn(move || {
        ProviderService::update(
            state,
            AppType::Claude,
            None,
            claude("a", "https://a2.example", json!({})),
        )
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(!updater.is_finished(), "the save waits for the switch lock");
    assert_eq!(settings()["env"]["ANTHROPIC_BASE_URL"], "https://a.example");

    enter_locked(state, &AppType::Claude, op::ENTER, Some(false), None)
        .await
        .expect("enter");
    drop(guard);
    tokio::task::spawn_blocking(move || updater.join())
        .await
        .unwrap()
        .unwrap()
        .expect("save");

    let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
    let live = settings();
    assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], proxy_url.as_str());
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], PROXY_TOKEN_PLACEHOLDER);

    exit(state, &AppType::Claude).await.expect("exit");
    assert_eq!(
        settings()["env"]["ANTHROPIC_BASE_URL"],
        "https://a2.example",
        "the saved row is what leaving proxy mode writes back"
    );

    // 同步当前供应商（导入、云同步、统一供应商）同样等进入代理写完。
    let guard = lock_settled(state, &AppType::Claude).await.unwrap();
    let syncer = std::thread::spawn(move || {
        ProviderService::sync_current_provider_for_app(state, AppType::Claude)
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(!syncer.is_finished(), "the sync waits for the switch lock");
    enter_locked(state, &AppType::Claude, op::ENTER, Some(false), None)
        .await
        .expect("enter");
    drop(guard);
    tokio::task::spawn_blocking(move || syncer.join())
        .await
        .unwrap()
        .unwrap()
        .expect("sync");
    assert_eq!(
        settings()["env"]["ANTHROPIC_AUTH_TOKEN"],
        PROXY_TOKEN_PLACEHOLDER
    );
    exit(state, &AppType::Claude).await.expect("exit");
}

/// 上一次的操作补不完（这里是落定状态失败）时，保存编辑器直接拒绝、行不动。否则按补完
/// 之前的指针判断「b 不是当前供应商」只存了行，等那次操作补完 b 成了当前供应商，live
/// 里却是它的旧 Key。
#[tokio::test]
#[serial]
async fn nothing_is_saved_while_the_previous_write_cannot_be_finished() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[
            claude("a", "https://a.example", json!({})),
            claude("b", "https://b.example", json!({})),
        ],
        "a",
    )
    .await;
    failpoint::crash_at(Some("published:0"));
    let interrupted = ProviderService::switch(&state, AppType::Claude, "b");
    failpoint::crash_at(None);
    assert!(interrupted.is_err());

    let mut row = state
        .db
        .get_provider_by_id("b", &AppType::Claude)
        .unwrap()
        .unwrap();
    let base = ProviderService::editor_view(&state, AppType::Claude, &row.settings_config, None)
        .expect("view")
        .settings;
    let mut edited = base.clone();
    edited["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("sk-b-new");
    row.settings_config = edited;
    failpoint::crash_at(Some("recover:target"));
    let refused = ProviderService::update_from_editor(
        &state,
        AppType::Claude,
        None,
        row,
        Some(crate::services::provider::EditorSave {
            base,
            draft: None,
            on_conflict: Default::default(),
        }),
    );
    failpoint::crash_at(None);
    let error = refused.expect_err("refused while unsettled");
    assert!(error.to_string().contains("补不完"), "{error}");
    let b_token = |state: &AppState| {
        state
            .db
            .get_provider_by_id("b", &AppType::Claude)
            .unwrap()
            .unwrap()
            .settings_config["env"]["ANTHROPIC_AUTH_TOKEN"]
            .clone()
    };
    assert_eq!(b_token(&state), "sk-b", "the row is untouched");

    crate::mode::operation::recover_on_startup(&state.db);
    assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("b"));
    assert_eq!(settings()["env"]["ANTHROPIC_AUTH_TOKEN"], b_token(&state));
}

#[tokio::test]
#[serial]
async fn failover_moves_the_route_without_writing_client_files() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[
            claude("a", "https://a.example", json!({})),
            claude(
                "b",
                "https://b.example",
                json!({ "CLAUDE_CODE_DISABLE_ARTIFACT": "1" }),
            ),
        ],
        "a",
    )
    .await;
    enter(&state, &AppType::Claude, false).await.expect("enter");
    let app = AppType::Claude;
    let mut config = state.db.get_proxy_config_for_app(&app).await.unwrap();
    config.auto_failover_enabled = true;
    state.db.update_proxy_config_for_app(config).await.unwrap();
    let revision = state.db.route_revision(&app);
    let before = settled_bytes();

    assert!(record_failover_route(&state, &app, "b", revision)
        .await
        .expect("record failover"));
    assert_eq!(settled_bytes(), before);
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("b"));
    assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("a"));
    exit(&state, &AppType::Claude).await.expect("exit");
}

#[tokio::test]
#[serial]
async fn in_flight_failover_cannot_override_a_manual_route_while_enabled() {
    check_in_flight_failover_preserves_manual_route(false).await;
}

#[tokio::test]
#[serial]
async fn in_flight_failover_cannot_override_a_manual_route_after_disabling() {
    check_in_flight_failover_preserves_manual_route(true).await;
}

async fn check_in_flight_failover_preserves_manual_route(disable_failover: bool) {
    use crate::proxy::{
        failover_switch::FailoverSwitchManager, forwarder::ForwardBody,
        handler_context::RequestContext, server::ProxyState,
    };
    use axum::{http::StatusCode, routing::post, Json, Router};
    use std::time::Duration;
    use tokio::sync::Notify;

    // 应用启动时会安装 rustls provider；测试进程里手动装一次（同 forwarder 测试）。
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let reached_b = Arc::new(Notify::new());
    let release_b = Arc::new(Notify::new());
    let response = || Json(json!({"id":"msg_test","type":"message","content":[],"usage":{}}));
    let upstream = Router::new()
        .route(
            "/a/v1/messages",
            post(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        )
        .route(
            "/b/v1/messages",
            post({
                let reached_b = reached_b.clone();
                let release_b = release_b.clone();
                move || {
                    let reached_b = reached_b.clone();
                    let release_b = release_b.clone();
                    async move {
                        reached_b.notify_one();
                        release_b.notified().await;
                        response()
                    }
                }
            }),
        )
        .route("/c/v1/messages", post(move || async move { response() }));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let upstream = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });

    let app = AppType::Claude;
    let rows = ["a", "b", "c"].map(|id| claude(id, &format!("{origin}/{id}"), json!({})));
    let state = state_with(app.clone(), &rows, "a").await;
    enter(&state, &app, false).await.unwrap();
    for id in ["a", "b"] {
        state.db.add_to_failover_queue(&app, id).unwrap();
    }
    let mut config = state.db.get_proxy_config_for_app(&app).await.unwrap();
    config.auto_failover_enabled = true;
    config.max_retries = 1;
    state.db.update_proxy_config_for_app(config).await.unwrap();

    let (manager, mut completed) = FailoverSwitchManager::for_test(state.clone());
    let mut proxy = ProxyState::for_test(state.db.clone());
    proxy.failover_manager = Arc::new(manager);
    let body = json!({"model":"claude-sonnet-4-6","messages":[]});
    let headers = axum::http::HeaderMap::new();
    let context = || {
        RequestContext::new(
            &proxy,
            &body,
            &headers,
            app.clone(),
            "Claude",
            "claude",
            None,
        )
    };
    let forward = async |ctx: &RequestContext| {
        ctx.create_forwarder(&proxy)
            .forward_with_retry(
                &app,
                http::Method::POST,
                "/v1/messages",
                ForwardBody::from_parsed(body.clone(), bytes::Bytes::from(body.to_string())),
                headers.clone(),
                http::Extensions::new(),
                ctx.get_providers(),
            )
            .await
            .map_err(|failure| failure.error)
            .expect("forward request")
    };
    let old_context = context().await.unwrap();
    assert_eq!(old_context.current_provider_id, "a");
    assert!(old_context.app_config.auto_failover_enabled);
    let switch = async {
        reached_b.notified().await;
        if disable_failover {
            let _guard = lock_settled(&state, &app).await.unwrap();
            let mut config = state.db.get_proxy_config_for_app(&app).await.unwrap();
            config.auto_failover_enabled = false;
            state.db.update_proxy_config_for_app(config).await.unwrap();
        }
        set_route(&state, &app, "c").await.unwrap();
        release_b.notify_one();
    };
    let (old_response, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(forward(&old_context), switch)
    })
    .await
    .expect("B receives the old request before C is selected");
    assert_eq!(old_response.provider.id, "b");
    assert_eq!(old_response.response.status(), StatusCode::OK);

    let mut writeback = async || {
        tokio::time::timeout(Duration::from_secs(10), completed.recv())
            .await
            .expect("the forwarder runs the background route writeback")
            .expect("writeback completion channel is open")
            .expect("record failover route")
    };
    let changed = writeback().await;
    assert_eq!(
        mode(&app).proxy_route.as_deref(),
        Some("c"),
        "B's success cannot replace C"
    );
    assert!(!changed);

    let new_context = context().await.unwrap();
    assert_eq!(
        new_context.app_config.auto_failover_enabled,
        !disable_failover
    );
    if !disable_failover {
        // A fresh request may still fail over to B and persist its route.
        release_b.notify_one();
    }
    let next = tokio::time::timeout(Duration::from_secs(10), forward(&new_context))
        .await
        .expect("forward the next request");
    if disable_failover {
        assert_eq!(next.provider.id, "c");
    } else {
        assert_eq!(next.provider.id, "b");
        assert!(writeback().await, "fresh failover persists B");
        assert_eq!(mode(&app).proxy_route.as_deref(), Some("b"));
    }
    exit(&state, &app).await.unwrap();
    upstream.abort();
}

#[tokio::test]
#[serial]
async fn a_crash_at_any_step_is_finished_or_discarded_on_startup() {
    for (point, expect_proxy) in [("pending", false), ("published:0", true), ("target", true)] {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(
            AppType::Claude,
            &[claude("a", "https://a.example", json!({}))],
            "a",
        )
        .await;
        failpoint::crash_at(Some(point));
        let result = enter(&state, &AppType::Claude, false).await;
        failpoint::crash_at(None);
        assert!(result.is_err(), "{point}");

        crate::mode::operation::recover_on_startup(&state.db);
        let recovered = mode(&AppType::Claude);
        let live = settings();
        let live_is_proxy = live["env"]["ANTHROPIC_AUTH_TOKEN"] == PROXY_TOKEN_PLACEHOLDER;
        assert_eq!(recovered.is_proxy(), expect_proxy, "{point}");
        assert_eq!(recovered.attached, expect_proxy, "{point}");
        assert_eq!(live_is_proxy, expect_proxy, "{point}: file and state agree");
        assert_eq!(
            state.db.get_proxy_flags_sync(&AppType::Claude).0,
            expect_proxy,
            "{point}"
        );
        assert!(state::pending(&DeviceStore::for_device(), "claude")
            .unwrap()
            .is_none());
        if !expect_proxy {
            assert_eq!(settled_string(), USER_SETTINGS);
        }
        if state.proxy_service.is_running().await {
            state.proxy_service.stop().await.unwrap();
        }
    }
}

#[tokio::test]
#[serial]
async fn a_crash_while_leaving_proxy_mode_is_finished_on_startup() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;
    enter(&state, &AppType::Claude, false).await.expect("enter");
    failpoint::crash_at(Some("published:0"));
    let result = exit(&state, &AppType::Claude).await;
    failpoint::crash_at(None);
    assert!(result.is_err());

    crate::mode::operation::recover_on_startup(&state.db);
    assert_eq!(mode(&AppType::Claude).mode, Some(Mode::Direct));
    // 补完的是崩溃前备好的文件，带着给开着的会话看的空值墓碑：写之前记下了，启动时接着删
    crate::services::provider::claude_direct::resume_tombstone_sweep();
    assert_back_to_user_settings();
    if state.proxy_service.is_running().await {
        state.proxy_service.stop().await.unwrap();
    }
}

#[tokio::test]
#[serial]
async fn startup_moves_legacy_takeover_state_over_to_the_modes() {
    for enabled in [true, false] {
        let home = Home::new();
        seed_settings(
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:15721","ANTHROPIC_AUTH_TOKEN":"PROXY_MANAGED"},"hooks":{}}"#,
        );
        let state = state_with(
            AppType::Claude,
            &[claude("a", "https://a.example", json!({}))],
            "a",
        )
        .await;
        state
            .db
            .save_live_backup(
                &AppType::Claude,
                r#"{"env":{"ANTHROPIC_BASE_URL":"https://stale.example"}}"#,
            )
            .await
            .unwrap();
        state
            .db
            .set_proxy_flags_sync(&AppType::Claude, enabled, false)
            .unwrap();

        startup(&state).await;

        assert!(
            state
                .db
                .get_live_backup(&AppType::Claude)
                .await
                .unwrap()
                .is_none(),
            "the old version would replay a leftover backup row after a downgrade"
        );
        let drained = home.dir.path().join(".cc-switch/backups/proxy-live-backup");
        assert_eq!(
            fs::read_dir(&drained).unwrap().count(),
            1,
            "kept aside as a file"
        );
        let migrated = mode(&AppType::Claude);
        let live = settings();
        assert_eq!(live["hooks"], json!({}));
        if enabled {
            assert!(migrated.is_proxy() && migrated.attached);
            let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
            assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], proxy_url.as_str());
            exit(&state, &AppType::Claude).await.unwrap();
        } else {
            assert_eq!(migrated.mode, Some(Mode::Direct));
            assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], "https://a.example");
            assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");
            assert!(!state.proxy_service.is_running().await);
        }
    }
}

#[tokio::test]
#[serial]
async fn gemini_proxy_contract_keeps_the_other_env_lines() {
    let _home = Home::new();
    let env_path = crate::gemini_config::get_gemini_env_path();
    fs::create_dir_all(env_path.parent().unwrap()).unwrap();
    fs::write(
        &env_path,
        "# my notes\nGEMINI_SANDBOX=true\nGEMINI_API_KEY=real-key\nGOOGLE_GEMINI_BASE_URL=https://g.example\n",
    )
    .unwrap();
    let row = Provider::with_id(
        "g".to_string(),
        "G".to_string(),
        json!({ "env": {
            "GEMINI_API_KEY": "real-key",
            "GOOGLE_GEMINI_BASE_URL": "https://g.example"
        }}),
        None,
    );
    let state = state_with(AppType::Gemini, &[row], "g").await;

    enter(&state, &AppType::Gemini, false).await.expect("enter");
    let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
    assert_eq!(
        fs::read_to_string(&env_path).unwrap(),
        format!(
            "# my notes\nGEMINI_SANDBOX=true\nGEMINI_API_KEY=PROXY_MANAGED\nGOOGLE_GEMINI_BASE_URL={proxy_url}\n"
        )
    );
    exit(&state, &AppType::Gemini).await.expect("exit");
    let text = fs::read_to_string(&env_path).unwrap();
    assert!(text.contains("GEMINI_API_KEY=real-key"), "{text}");
    assert!(!text.contains("PROXY_MANAGED"), "{text}");
}

/// 代理换了端口：一个应用重写失败（配置文件解析不了），其余接上代理的应用照样按新
/// 地址重写，失败的应用在报错里。
#[tokio::test]
#[serial]
async fn a_new_proxy_address_reaches_every_app_even_if_one_fails() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    seed_gemini(
        "GEMINI_API_KEY=real-key\nGOOGLE_GEMINI_BASE_URL=https://g.example\n",
        "{}",
    );
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;
    let row = gemini(
        "g",
        json!({ "GEMINI_API_KEY": "real-key", "GOOGLE_GEMINI_BASE_URL": "https://g.example" }),
        json!({}),
    );
    state.db.save_provider(&AppType::Gemini, &row).unwrap();
    state
        .db
        .set_current_provider(&AppType::Gemini, "g")
        .unwrap();
    crate::settings::set_current_provider(&AppType::Gemini, Some("g")).unwrap();
    enter(&state, &AppType::Claude, false)
        .await
        .expect("enter claude");
    enter(&state, &AppType::Gemini, false)
        .await
        .expect("enter gemini");
    let old_url = state.proxy_service.build_proxy_urls().await.unwrap().0;

    fs::write(settings_path(), "not json").unwrap();
    let mut config = state.db.get_global_proxy_config().await.unwrap();
    config.listen_port = 0;
    assert!(state
        .proxy_service
        .update_global_config(&config)
        .await
        .unwrap());
    let new_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
    assert_ne!(new_url, old_url);

    let error = resync_routes(&state).await.expect_err("claude fails");
    assert!(error.starts_with("claude:"), "{error}");
    assert!(
        gemini_env().contains(&format!("GOOGLE_GEMINI_BASE_URL={new_url}\n")),
        "{}",
        gemini_env()
    );
    state.proxy_service.stop().await.unwrap();
}

/// 客户端地址存在本机 settings.json，不进会同步到别的设备的库；保存时重写接上路由的
/// 客户端，第一次重写失败后改好客户端文件、原样再保存一次就是重试。
#[tokio::test]
#[serial]
async fn saving_the_client_host_rewrites_routed_clients_and_retries_on_the_same_value() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;
    enter(&state, &AppType::Claude, false)
        .await
        .expect("enter claude");
    let routed = settled_string();

    fs::write(settings_path(), "not json").unwrap();
    crate::commands::save_proxy_client_host(&state, " 172.25.144.1 ")
        .await
        .expect_err("claude settings unreadable");
    assert_eq!(
        crate::settings::get_proxy_client_host().as_deref(),
        Some("172.25.144.1")
    );
    let snapshot = state.db.export_sql_string_for_sync().unwrap();
    assert!(!snapshot.contains("172.25.144.1"));

    fs::write(settings_path(), &routed).unwrap();
    crate::commands::save_proxy_client_host(&state, "172.25.144.1")
        .await
        .expect("retry the same save");
    let settings = settled_string();
    assert!(settings.contains("http://172.25.144.1:"), "{settings}");
    assert!(
        crate::claude_desktop_config::proxy_gateway_base_url_from_db(&state.db)
            .unwrap()
            .starts_with("http://172.25.144.1:")
    );

    crate::commands::save_proxy_client_host(&state, "")
        .await
        .expect("clear the client host");
    assert_eq!(crate::settings::get_proxy_client_host(), None);
    let settings = settled_string();
    assert!(settings.contains("http://127.0.0.1:"), "{settings}");
    state.proxy_service.stop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn codex_routes_between_official_and_third_party_contracts() {
    let _home = Home::new();
    let native_auth = json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": "native-id",
            "access_token": "native-access",
            "refresh_token": "native-refresh",
            "account_id": "acct-native"
        },
        "last_refresh": "2026-01-01T00:00:00Z"
    });
    crate::codex_config::write_codex_live_atomic(&native_auth, Some("model = \"gpt-5.4\"\n"))
        .unwrap();
    let mut official = Provider::with_id(
        crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
        None,
    );
    official.category = Some("official".to_string());
    let relay = Provider::with_id(
        "relay".to_string(),
        "Relay".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "sk-relay" },
            "config": "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n"
        }),
        None,
    );
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .unwrap();
    let state = state_with(AppType::Codex, &[official.clone(), relay], "relay").await;
    // 直连在 relay 上：config.toml 是 relay 的，auth.json 保留原生登录。
    ProviderService::switch(&state, AppType::Codex, "relay").expect("direct relay");
    let config_path = crate::codex_config::get_codex_config_path();
    let auth_path = crate::codex_config::get_codex_auth_path();
    let auth = || -> Value { crate::config::read_json_file(&auth_path).unwrap() };

    enter(&state, &AppType::Codex, false).await.expect("enter");
    let third_party = fs::read_to_string(&config_path).unwrap();
    assert!(
        third_party.contains(PROXY_TOKEN_PLACEHOLDER),
        "{third_party}"
    );
    assert_eq!(
        auth(),
        native_auth,
        "the third-party contract never writes auth.json"
    );
    let relay_row = state
        .db
        .get_provider_by_id("relay", &AppType::Codex)
        .unwrap()
        .unwrap();
    assert!(
        !relay_row
            .settings_config
            .to_string()
            .contains("native-access"),
        "backfilling on enter must not copy the ChatGPT login into a third-party row: {}",
        relay_row.settings_config
    );

    ProviderService::switch(&state, AppType::Codex, &official.id).expect("route to official");
    let official_contract = fs::read_to_string(&config_path).unwrap();
    let doc: toml::Table = toml::from_str(&official_contract).unwrap();
    // 和官方直连同一个会话桶（内置 openai）：不写选路，顶层改道到代理，客户端带自己的登录。
    assert!(doc.get("model_provider").is_none(), "{official_contract}");
    let base_url = doc["openai_base_url"].as_str().unwrap();
    assert!(
        base_url.starts_with("http://127.0.0.1:") && base_url.ends_with("/v1"),
        "{official_contract}"
    );
    assert!(
        doc["model_providers"].get("cc-switch-official").is_none(),
        "{official_contract}"
    );
    assert!(state
        .proxy_service
        .live_has_proxy_placeholder(&AppType::Codex));
    // 第三方路由留下的 custom 表改成休眠形态：指向本地代理、只有占位 Key。
    let dormant = &doc["model_providers"]["custom"];
    assert_eq!(
        dormant["experimental_bearer_token"].as_str(),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
    assert!(dormant.get("requires_openai_auth").is_none());
    assert!(!official_contract.contains("sk-relay"));
    assert_eq!(auth(), native_auth);

    // 本机有 ChatGPT 登录：第三方路由也写成内置 openai（和官方路由同一个会话组），
    // 之前的 custom 表仍是休眠形态，第三方的 Key 不进客户端文件。
    ProviderService::switch(&state, AppType::Codex, "relay").expect("route back");
    let third_party = fs::read_to_string(&config_path).unwrap();
    let doc: toml::Table = toml::from_str(&third_party).unwrap();
    assert!(doc.get("model_provider").is_none(), "{third_party}");
    assert_eq!(doc["openai_base_url"].as_str(), Some(base_url));
    assert_eq!(
        doc["model_providers"]["custom"]["experimental_bearer_token"].as_str(),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
    assert!(!third_party.contains("sk-relay"), "{third_party}");
    assert_eq!(auth(), native_auth);
    exit(&state, &AppType::Codex).await.expect("exit");
    assert_eq!(auth(), native_auth);
    assert!(!state
        .proxy_service
        .live_has_proxy_placeholder(&AppType::Codex));
}

/// 官方做路由时会话和官方直连落在同一个桶：进出代理不换 Codex 选中的 provider id。
#[tokio::test]
#[serial]
async fn codex_official_route_keeps_the_direct_session_bucket() {
    for unified in [false, true] {
        let _home = Home::new();
        let native_auth = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "native-id",
                "access_token": "native-access",
                "refresh_token": "native-refresh",
                "account_id": "acct-native"
            },
            "last_refresh": "2026-01-01T00:00:00Z"
        });
        crate::codex_config::write_codex_live_atomic(&native_auth, Some("model = \"gpt-5.4\"\n"))
            .unwrap();
        let mut official = Provider::with_id(
            crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
            None,
        );
        official.category = Some("official".to_string());
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: true,
            unify_codex_session_history: unified,
            ..Default::default()
        })
        .unwrap();
        let state = state_with(AppType::Codex, &[official.clone()], &official.id).await;
        ProviderService::switch(&state, AppType::Codex, &official.id).expect("direct official");
        let config_path = crate::codex_config::get_codex_config_path();
        let selector = || -> Option<String> {
            let text = fs::read_to_string(&config_path).unwrap();
            let doc: toml::Table = toml::from_str(&text).unwrap();
            doc.get("model_provider")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        let bucket = if unified {
            Some("custom".to_string())
        } else {
            None
        };
        assert_eq!(selector(), bucket, "direct, unified={unified}");

        enter(&state, &AppType::Codex, false).await.expect("enter");
        let contract = fs::read_to_string(&config_path).unwrap();
        assert_eq!(selector(), bucket, "proxy, unified={unified}: {contract}");
        let doc: toml::Table = toml::from_str(&contract).unwrap();
        if unified {
            assert!(doc.get("openai_base_url").is_none(), "{contract}");
            let mirror = &doc["model_providers"]["custom"];
            assert_eq!(mirror["name"].as_str(), Some("OpenAI"));
            assert_eq!(mirror["requires_openai_auth"].as_bool(), Some(true));
            assert_eq!(mirror["supports_websockets"].as_bool(), Some(false));
            assert!(mirror.get("experimental_bearer_token").is_none());
            assert!(
                mirror["base_url"].as_str().unwrap().ends_with("/v1"),
                "{contract}"
            );
        } else {
            assert!(doc["openai_base_url"].as_str().is_some(), "{contract}");
            assert!(doc.get("model_providers").is_none(), "{contract}");
        }
        assert!(state
            .proxy_service
            .live_has_proxy_placeholder(&AppType::Codex));
        assert_eq!(
            crate::config::read_json_file::<Value>(&crate::codex_config::get_codex_auth_path())
                .unwrap(),
            native_auth
        );

        exit(&state, &AppType::Codex).await.expect("exit");
        let restored = fs::read_to_string(&config_path).unwrap();
        assert_eq!(selector(), bucket, "exit, unified={unified}: {restored}");
        assert!(!restored.contains("openai_base_url"), "{restored}");
        assert!(!state
            .proxy_service
            .live_has_proxy_placeholder(&AppType::Codex));
    }
}

// ---------- Codex：只替换关键字段 ----------

fn codex_row(id: &str, url: &str, extra: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({
            "auth": { "OPENAI_API_KEY": format!("sk-{id}") },
            "config": format!(
                "model_provider = \"{id}\"\nmodel = \"gpt-{id}\"\n{extra}\n[model_providers.{id}]\nname = \"{id}\"\nbase_url = \"{url}\"\nwire_api = \"responses\"\n"
            ),
        }),
        None,
    )
}

fn codex_official() -> Provider {
    let mut official = Provider::with_id(
        crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": {}, "config": "" }),
        None,
    );
    official.category = Some("official".to_string());
    official
}

fn codex_config_path() -> std::path::PathBuf {
    crate::codex_config::get_codex_config_path()
}

fn codex_auth_path() -> std::path::PathBuf {
    crate::codex_config::get_codex_auth_path()
}

fn seed_codex(config: &str, auth: Option<&Value>) {
    let path = codex_config_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, config).unwrap();
    match auth {
        Some(auth) => fs::write(codex_auth_path(), auth.to_string()).unwrap(),
        None => {
            let _ = fs::remove_file(codex_auth_path());
        }
    }
}

fn codex_text() -> String {
    fs::read_to_string(codex_config_path()).unwrap()
}

fn codex_doc() -> toml::Table {
    toml::from_str(&codex_text()).unwrap()
}

/// CC Switch 写进 live 的目录指针，按当前配置目录现算：本机目录是绝对路径，
/// `\\wsl.localhost\...` 这类 UNC 目录是裸文件名（WSL2 夜测的 HOME 就在那里）。
fn ours_catalog_pointer() -> String {
    crate::live::project::codex::catalog_pointer_value(&crate::codex_config::get_codex_config_dir())
}

fn set_preservation(on: bool) {
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: on,
        ..Default::default()
    })
    .unwrap();
}

fn chatgpt_login(account: &str) -> Value {
    json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": "id",
            "access_token": format!("access-{account}"),
            "refresh_token": format!("refresh-{account}"),
            "account_id": account
        },
        "last_refresh": "2026-09-01T00:00:00Z"
    })
}

/// live 里 A 的关键字段之外，都是用户和 Codex 自己的东西。
const CODEX_USER_LIVE: &str = r#"# 用户的注释
approval_policy = "on-request"
model_provider = "custom"
model = "gpt-a"
model_context_window = 200000

[projects."/work"]
trust_level = "trusted"

[agents]
default_subagent_model = "gpt-a-mini"
max_threads = 4

[model_providers.custom]
name = "a"
base_url = "https://a.example/v1"
wire_api = "responses"
experimental_bearer_token = "sk-a"

[model_providers.ollama_local]
name = "Ollama"
base_url = "http://localhost:11434/v1"

[mcp_servers.fs]
command = "fs-server"
"#;

fn codex_a_b() -> [Provider; 2] {
    [
        codex_row(
            "a",
            "https://a.example/v1",
            "model_context_window = 200000\n[agents]\ndefault_subagent_model = \"gpt-a-mini\"\n",
        ),
        codex_row("b", "https://b.example/v1", ""),
    ]
}

/// 关键字段之外的部分（用户的表、注释、MCP、项目信任）。
fn codex_user_parts(text: &str) -> Vec<&str> {
    [
        "# 用户的注释",
        "approval_policy = \"on-request\"",
        "[projects.\"/work\"]",
        "max_threads = 4",
        "[model_providers.ollama_local]",
        "[mcp_servers.fs]",
    ]
    .into_iter()
    .filter(|part| text.contains(part))
    .collect()
}

#[tokio::test]
#[serial]
async fn codex_direct_switch_replaces_only_key_fields_and_round_trips() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, None);
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

    ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
    let on_b = codex_text();
    let doc = codex_doc();
    assert_eq!(doc["model_provider"].as_str(), Some("custom"));
    assert_eq!(doc["model"].as_str(), Some("gpt-b"));
    let route = &doc["model_providers"]["custom"];
    assert_eq!(route["base_url"].as_str(), Some("https://b.example/v1"));
    assert_eq!(route["experimental_bearer_token"].as_str(), Some("sk-b"));
    assert!(!on_b.contains("sk-a"), "A's key is gone: {on_b}");
    // A 带进来的独有字段（值没被改过）删掉；嵌在 [agents] 里的模型名只删那一个键。
    assert!(doc.get("model_context_window").is_none(), "{on_b}");
    assert!(doc["agents"].get("default_subagent_model").is_none());
    assert_eq!(doc["agents"]["max_threads"].as_integer(), Some(4));
    assert_eq!(codex_user_parts(&on_b).len(), 6, "{on_b}");

    ProviderService::switch(&state, AppType::Codex, "a").expect("back to a");
    let on_a = codex_text();
    let doc = codex_doc();
    assert_eq!(doc["model"].as_str(), Some("gpt-a"));
    assert_eq!(doc["model_context_window"].as_integer(), Some(200000));
    assert_eq!(
        doc["agents"]["default_subagent_model"].as_str(),
        Some("gpt-a-mini")
    );
    assert_eq!(codex_user_parts(&on_a).len(), 6, "{on_a}");

    // 第二轮往返字节稳定。
    ProviderService::switch(&state, AppType::Codex, "b").expect("to b again");
    assert_eq!(codex_text(), on_b);
    ProviderService::switch(&state, AppType::Codex, "a").expect("to a again");
    assert_eq!(codex_text(), on_a);
}

/// 行里自己指定的模型目录指针跟着这一家走：切走时删掉，切到生成了目录的那家就换成
/// CC Switch 自己的指针；代理契约带进来的，退出代理时同样删掉。用户直接写进 live 的
/// 指针一直留着。
#[tokio::test]
#[serial]
async fn codex_a_row_catalog_pointer_leaves_with_its_provider() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, None);
    let a = codex_row(
        "a",
        "https://a.example/v1",
        "model_catalog_json = \"/work/a-catalog.json\"",
    );
    let mut b = codex_row("b", "https://b.example/v1", "");
    b.settings_config["modelCatalog"] = json!({ "models": [{ "model": "gpt-b" }] });
    let c = codex_row("c", "https://c.example/v1", "");
    let state = state_with(AppType::Codex, &[a, b, c], "c").await;
    let pointer = || {
        codex_doc()
            .get("model_catalog_json")
            .and_then(|value| value.as_str().map(str::to_string))
    };
    // cc-switch 接管指针时写绝对路径（严格重读的 Codex 只认绝对形态；UNC 目录写裸文件名）。
    let ours = ours_catalog_pointer();

    ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
    assert_eq!(pointer().as_deref(), Some("/work/a-catalog.json"));
    ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
    assert_eq!(
        pointer().as_deref(),
        Some(ours.as_str()),
        "{}",
        codex_text()
    );
    ProviderService::switch(&state, AppType::Codex, "a").expect("back to a");
    ProviderService::switch(&state, AppType::Codex, "c").expect("to c");
    assert_eq!(pointer(), None, "{}", codex_text());

    // 代理模式：路由从 c 换到 a，契约带进 a 的指针；退出代理写回直连 c 时删掉。
    enter(&state, &AppType::Codex, false).await.expect("enter");
    ProviderService::switch(&state, AppType::Codex, "a").expect("route to a");
    assert_eq!(pointer().as_deref(), Some("/work/a-catalog.json"));
    exit(&state, &AppType::Codex).await.expect("exit");
    assert_eq!(pointer(), None, "{}", codex_text());

    // 指针是关键字段：手写进 live 的值不保留，有目录时换成 CC Switch 的，没有就删。
    let with_user = format!("model_catalog_json = \"/work/mine.json\"\n{}", codex_text());
    fs::write(codex_config_path(), with_user).unwrap();
    ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
    assert_eq!(
        pointer().as_deref(),
        Some(ours.as_str()),
        "{}",
        codex_text()
    );
    let with_user = codex_text().replace(
        &format!("model_catalog_json = \"{ours}\""),
        "model_catalog_json = \"/work/mine.json\"",
    );
    fs::write(codex_config_path(), with_user).unwrap();
    ProviderService::switch(&state, AppType::Codex, "c").expect("to c");
    assert_eq!(pointer(), None, "{}", codex_text());
}

#[tokio::test]
#[serial]
async fn codex_exclusive_fields_the_user_changed_stay() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, None);
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
    let edited = CODEX_USER_LIVE.replace(
        "model_context_window = 200000",
        "model_context_window = 150000",
    );
    seed_codex(&edited, None);

    ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
    assert_eq!(
        codex_doc()["model_context_window"].as_integer(),
        Some(150000),
        "a value the user changed is not A's to remove"
    );
}

#[tokio::test]
#[serial]
async fn codex_official_switch_leaves_a_dormant_route_table() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
    let [a, b] = codex_a_b();
    let state = state_with(AppType::Codex, &[a, b, codex_official()], "a").await;

    ProviderService::switch(
        &state,
        AppType::Codex,
        crate::database::CODEX_OFFICIAL_PROVIDER_ID,
    )
    .expect("switch to official");
    let text = codex_text();
    let doc = codex_doc();
    assert!(doc.get("model_provider").is_none(), "{text}");
    let dormant = &doc["model_providers"]["custom"];
    assert_eq!(
        dormant["base_url"].as_str(),
        Some("http://127.0.0.1:15721/v1"),
        "the dormant table points at the configured local proxy: {text}"
    );
    assert_eq!(
        dormant["experimental_bearer_token"].as_str(),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
    assert!(dormant.get("name").is_some(), "Codex loads it: {text}");
    assert!(!text.contains("sk-a"), "no real key stays behind: {text}");
    assert_eq!(codex_user_parts(&text).len(), 6, "{text}");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(codex_auth_path()).unwrap()).unwrap(),
        chatgpt_login("acct"),
        "the official login is untouched"
    );
}

#[tokio::test]
#[serial]
async fn codex_an_active_profile_overriding_the_route_is_refused_without_side_effects() {
    let _home = Home::new();
    set_preservation(true);
    let live = format!(
        "profile = \"work\"\n{CODEX_USER_LIVE}\n[profiles.work]\nmodel_provider = \"ollama_local\"\n"
    );
    seed_codex(&live, None);
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
    let mtime = fs::metadata(codex_config_path())
        .unwrap()
        .modified()
        .unwrap();

    let err = ProviderService::switch(&state, AppType::Codex, "b").expect_err("refused");
    assert!(err.to_string().contains("work"), "{err}");
    assert_eq!(codex_text(), live);
    assert_eq!(
        fs::metadata(codex_config_path())
            .unwrap()
            .modified()
            .unwrap(),
        mtime
    );
    assert_eq!(direct(&state, &AppType::Codex).as_deref(), Some("a"));
}

/// 生效的 profile 显式选了内置的 `openai`：和官方卡不写 model_provider 去的是同一个
/// 地方，切到官方卡不拒绝；切到第三方仍然拒绝。
#[tokio::test]
#[serial]
async fn codex_a_profile_selecting_the_built_in_openai_allows_the_official_card() {
    let _home = Home::new();
    set_preservation(true);
    let live = format!(
        "profile = \"work\"\n{CODEX_USER_LIVE}\n[profiles.work]\nmodel_provider = \"openai\"\n"
    );
    seed_codex(&live, Some(&chatgpt_login("acct")));
    let [a, b] = codex_a_b();
    let state = state_with(AppType::Codex, &[a, b, codex_official()], "a").await;

    ProviderService::switch(
        &state,
        AppType::Codex,
        crate::database::CODEX_OFFICIAL_PROVIDER_ID,
    )
    .expect("switch to official");
    let doc = codex_doc();
    assert!(doc.get("model_provider").is_none());
    assert_eq!(
        doc["profiles"]["work"]["model_provider"].as_str(),
        Some("openai")
    );

    let err = ProviderService::switch(&state, AppType::Codex, "b").expect_err("refused");
    assert!(err.to_string().contains("work"), "{err}");
}

#[tokio::test]
#[serial]
async fn codex_migration_retires_only_tables_cc_switch_wrote() {
    let _home = Home::new();
    set_preservation(true);
    // 旧版按行的 id 整份写进来的表：a（id 和地址都对得上 a 的行）、b 的地址被用户改过、
    // 被 profile 引用的 c、代理占位残留、用户自己的 ollama_local。
    let live = r#"model_provider = "a"
model = "gpt-a"

[model_providers.a]
name = "a"
base_url = "https://a.example/v1"
experimental_bearer_token = "sk-a"

[model_providers.b]
name = "b"
base_url = "https://my-own-b.example/v1"

[model_providers.c]
name = "c"
base_url = "https://c.example/v1"

[model_providers.deepseek]
name = "deepseek"
base_url = "http://127.0.0.1:15721/v1"
experimental_bearer_token = "PROXY_MANAGED"

[model_providers.ollama_local]
name = "Ollama"
base_url = "http://localhost:11434/v1"

[profiles.side]
model_provider = "c"
"#;
    seed_codex(live, None);
    let [a, b] = codex_a_b();
    let c = codex_row("c", "https://c.example/v1", "");
    let state = state_with(AppType::Codex, &[a, b, c], "a").await;

    ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
    let text = codex_text();
    let providers = codex_doc()["model_providers"].as_table().unwrap().clone();
    assert!(!providers.contains_key("a"), "provably ours: {text}");
    assert!(
        !providers.contains_key("deepseek"),
        "placeholder leftover: {text}"
    );
    assert!(
        providers.contains_key("b"),
        "address differs, not provably ours"
    );
    assert!(providers.contains_key("c"), "a profile still selects it");
    assert!(
        providers.contains_key("ollama_local"),
        "the user's own table"
    );
    assert!(!text.contains("sk-a"), "{text}");
}

#[tokio::test]
#[serial]
async fn codex_switch_crash_rolls_every_file_forward() {
    let _home = Home::new();
    set_preservation(false);
    seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
    let [a, mut b] = codex_a_b();
    b.settings_config["modelCatalog"] = json!({ "models": [{ "model": "gpt-b" }] });
    let state = state_with(AppType::Codex, &[a, b, codex_official()], "a").await;
    ProviderService::switch(
        &state,
        AppType::Codex,
        crate::database::CODEX_OFFICIAL_PROVIDER_ID,
    )
    .expect("official");

    // 官方 → b：删 auth.json（暂存登录）、改 config.toml、写模型目录，一起提交。
    for point in ["published:0", "published:1", "published:2", "target"] {
        ProviderService::switch(
            &state,
            AppType::Codex,
            crate::database::CODEX_OFFICIAL_PROVIDER_ID,
        )
        .expect("reset to official");
        assert!(codex_auth_path().exists(), "{point}: login restored");
        failpoint::crash_at(Some(point));
        let crashed = ProviderService::switch(&state, AppType::Codex, "b");
        failpoint::crash_at(None);
        assert!(crashed.is_err(), "{point}");

        crate::mode::operation::recover_on_startup(&state.db);
        assert!(!codex_auth_path().exists(), "{point}: auth.json deleted");
        assert_eq!(codex_doc()["model"].as_str(), Some("gpt-b"), "{point}");
        assert!(
            crate::codex_config::get_codex_model_catalog_path().exists(),
            "{point}: catalog written"
        );
        assert_eq!(
            direct(&state, &AppType::Codex).as_deref(),
            Some("b"),
            "{point}"
        );
    }
}

#[tokio::test]
#[serial]
async fn codex_preservation_off_gives_the_login_back_on_the_way_to_official() {
    let _home = Home::new();
    set_preservation(false);
    seed_codex("", Some(&chatgpt_login("acct")));
    let [a, b] = codex_a_b();
    let official = codex_official();
    let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;
    let login = || -> Option<Value> {
        fs::read(codex_auth_path())
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes).unwrap())
    };

    ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
    assert_eq!(login(), None, "no login next to a third-party route");
    ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
    ProviderService::switch(&state, AppType::Codex, &official.id).expect("to official");
    assert_eq!(
        login(),
        Some(chatgpt_login("acct")),
        "the same login comes back"
    );
    let row = state
        .db
        .get_provider_by_id(&official.id, &AppType::Codex)
        .unwrap()
        .unwrap();
    assert_eq!(
        row.settings_config["auth"],
        json!({}),
        "the login never goes into the row (it would sync to the cloud)"
    );

    // 在官方卡上登出后切走再切回：保持登出。
    fs::remove_file(codex_auth_path()).unwrap();
    ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
    ProviderService::switch(&state, AppType::Codex, &official.id).expect("to official");
    assert_eq!(login(), None, "logging out sticks");
}

fn codex_login_file() -> Option<Value> {
    fs::read(codex_auth_path())
        .ok()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
}

/// 直连指针在第三方卡上：退出 CC Switch 的分离不暂存登录，重启接上后登录还在。Codex
/// 桌面版续会话要 ChatGPT 登录，分离吞掉它就得每次重启都重新登录（#8431）。
#[tokio::test]
#[serial]
async fn codex_detaching_and_attaching_keep_the_login_next_to_a_third_party_route() {
    let _home = Home::new();
    set_preservation(false);
    seed_codex("", Some(&chatgpt_login("acct")));
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

    enter(&state, &AppType::Codex, false).await.expect("enter");
    assert_eq!(codex_login_file(), Some(chatgpt_login("acct")));

    detach_all(&state).await;
    assert!(!mode(&AppType::Codex).attached);
    assert_eq!(
        codex_login_file(),
        Some(chatgpt_login("acct")),
        "detaching keeps the login"
    );
    assert_eq!(
        codex_doc()["model_providers"]["custom"]["requires_openai_auth"].as_bool(),
        Some(true),
        "Codex sees the login it keeps"
    );

    startup(&state).await;
    assert!(mode(&AppType::Codex).attached);
    assert_eq!(codex_login_file(), Some(chatgpt_login("acct")));

    // 真正退出路由仍按保留登录开关走。
    exit(&state, &AppType::Codex).await.expect("exit");
    assert_eq!(codex_login_file(), None);
}

/// 登录已经被暂存走了（旧版的分离、或进路由前直连切到了第三方）：进入、接上代理时还回去。
#[tokio::test]
#[serial]
async fn codex_entering_the_proxy_gives_back_a_stashed_login() {
    let _home = Home::new();
    set_preservation(false);
    seed_codex("", Some(&chatgpt_login("acct")));
    let [a, b] = codex_a_b();
    let official = codex_official();
    let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;

    ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
    assert_eq!(
        codex_login_file(),
        None,
        "stashed next to a third-party route"
    );

    enter(&state, &AppType::Codex, false).await.expect("enter");
    assert_eq!(codex_login_file(), Some(chatgpt_login("acct")));
    // 还回来的登录算数：第三方路由也写成内置 openai，会话和官方同组。
    let doc = codex_doc();
    assert!(doc.get("model_provider").is_none(), "{}", codex_text());
    assert!(doc["openai_base_url"]
        .as_str()
        .is_some_and(|url| url.starts_with("http://127.0.0.1:")));

    // 只还一次：在 Codex 里登出后换路由，不会又被登上；没有登录就退回 custom 表。
    fs::remove_file(codex_auth_path()).unwrap();
    set_route(&state, &AppType::Codex, "b")
        .await
        .expect("route b");
    assert_eq!(codex_login_file(), None);
    let doc = codex_doc();
    assert_eq!(doc["model_provider"].as_str(), Some("custom"));
    assert!(doc.get("openai_base_url").is_none(), "{}", codex_text());
    assert_eq!(
        doc["model_providers"]["custom"]["requires_openai_auth"].as_bool(),
        Some(false)
    );
    exit(&state, &AppType::Codex).await.expect("exit");
}

/// 有登录也仍写 custom 表的两种情况：统一会话历史开着（本来全在 custom 组），行里写了
/// 内置 openai 表达不了的客户端设置（流空闲超时等，Codex 只认 provider 表上的）。
#[tokio::test]
#[serial]
async fn codex_third_party_route_keeps_the_custom_table_when_openai_cannot_carry_it() {
    let slow = Provider::with_id(
        "slow".to_string(),
        "Slow".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "sk-slow" },
            "config": "model_provider = \"custom\"\nmodel = \"m\"\n\n[model_providers.custom]\nname = \"slow\"\nbase_url = \"https://slow.example/v1\"\nwire_api = \"responses\"\nstream_idle_timeout_ms = 900000\n"
        }),
        None,
    );
    let [a, _] = codex_a_b();
    for (unified, route) in [(true, "a"), (false, "slow")] {
        let _home = Home::new();
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: true,
            unify_codex_session_history: unified,
            ..Default::default()
        })
        .unwrap();
        seed_codex("", Some(&chatgpt_login("acct")));
        let state = state_with(AppType::Codex, &[a.clone(), slow.clone()], route).await;
        ProviderService::switch(&state, AppType::Codex, route).expect("direct");
        enter(&state, &AppType::Codex, false).await.expect("enter");
        let doc = codex_doc();
        assert_eq!(
            doc.get("model_provider").and_then(|v| v.as_str()),
            Some("custom"),
            "unified={unified}: {}",
            codex_text()
        );
        assert!(doc.get("openai_base_url").is_none(), "{}", codex_text());
        let table = &doc["model_providers"]["custom"];
        assert_eq!(table["requires_openai_auth"].as_bool(), Some(true));
        if route == "slow" {
            assert_eq!(table["stream_idle_timeout_ms"].as_integer(), Some(900000));
        }
        exit(&state, &AppType::Codex).await.expect("exit");
    }
}

/// 暂停使用官方登录：官方路由和第三方路由都写成不要求登录的 custom 表（Codex 不按官方
/// 额度锁发送键），登录留在 auth.json；关掉后回到内置 openai 的写法。
#[tokio::test]
#[serial]
async fn codex_paused_official_login_writes_a_login_free_custom_table() {
    let _home = Home::new();
    let set_paused = |paused: bool| {
        let mut settings = crate::settings::get_settings();
        settings.preserve_codex_official_auth_on_switch = true;
        settings.codex_proxy_without_official_login = paused;
        crate::settings::update_settings(settings).unwrap();
    };
    set_paused(false);
    seed_codex("", Some(&chatgpt_login("acct")));
    let official_id = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    let mut official = Provider::with_id(
        official_id.to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
        None,
    );
    official.category = Some("official".to_string());
    let [a, _] = codex_a_b();
    let state = state_with(AppType::Codex, &[official, a], official_id).await;
    ProviderService::switch(&state, AppType::Codex, official_id).expect("direct");
    enter(&state, &AppType::Codex, false).await.expect("enter");
    assert!(
        codex_doc().get("openai_base_url").is_some(),
        "{}",
        codex_text()
    );

    set_paused(true);
    resync_codex_proxy(&state).await.expect("pause");
    for route in [official_id, "a"] {
        if route == "a" {
            set_route(&state, &AppType::Codex, "a")
                .await
                .expect("route a");
        }
        let doc = codex_doc();
        assert_eq!(
            doc["model_provider"].as_str(),
            Some("custom"),
            "{route}: {}",
            codex_text()
        );
        assert!(doc.get("openai_base_url").is_none(), "{}", codex_text());
        let table = &doc["model_providers"]["custom"];
        assert_ne!(
            table
                .get("requires_openai_auth")
                .and_then(|value| value.as_bool()),
            Some(true),
            "{route}: {}",
            codex_text()
        );
        assert_eq!(
            table["experimental_bearer_token"].as_str(),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert_eq!(codex_login_file(), Some(chatgpt_login("acct")));
    }

    set_paused(false);
    resync_codex_proxy(&state).await.expect("resume");
    let doc = codex_doc();
    assert!(doc.get("model_provider").is_none(), "{}", codex_text());
    assert!(doc.get("openai_base_url").is_some(), "{}", codex_text());
    exit(&state, &AppType::Codex).await.expect("exit");
}

fn codex_login_on_disk() -> Value {
    crate::config::read_json_file(&codex_auth_path()).unwrap()
}

/// 官方 → a：删掉 auth.json 之后失败。登录只在暂存的临时文件里，指针没动。
async fn codex_switch_interrupted_after_auth_json() -> (AppState, Provider) {
    set_preservation(false);
    seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
    let [a, b] = codex_a_b();
    let official = codex_official();
    let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;
    ProviderService::switch(&state, AppType::Codex, &official.id).expect("official");
    failpoint::crash_at(Some("published:0"));
    let failed = ProviderService::switch(&state, AppType::Codex, "a");
    failpoint::crash_at(None);
    assert!(failed.is_err());
    assert!(!codex_auth_path().exists());
    assert_eq!(
        direct(&state, &AppType::Codex).as_deref(),
        Some(official.id.as_str())
    );
    (state, official)
}

#[tokio::test]
#[serial]
async fn codex_a_retry_after_a_failed_switch_finishes_it_first() {
    let _home = Home::new();
    let (state, official) = codex_switch_interrupted_after_auth_json().await;

    // 重试切到 b：先补完到 a，再按补完后的 auth.json 和暂存从 a 切到 b。
    ProviderService::switch(&state, AppType::Codex, "b").expect("retry");
    assert_eq!(direct(&state, &AppType::Codex).as_deref(), Some("b"));
    assert_eq!(codex_doc()["model"].as_str(), Some("gpt-b"));
    ProviderService::switch(&state, AppType::Codex, &official.id).expect("to official");
    assert_eq!(codex_login_on_disk(), chatgpt_login("acct"));
}

#[tokio::test]
#[serial]
async fn codex_an_interrupted_switch_keeps_the_login_when_codex_changed_config_toml() {
    let _home = Home::new();
    let (state, official) = codex_switch_interrupted_after_auth_json().await;
    // 补完之前 Codex 自己改了 config.toml（信任了一个新项目）。
    let mut text = codex_text();
    text.push_str("\n[projects.\"/new\"]\ntrust_level = \"trusted\"\n");
    fs::write(codex_config_path(), &text).unwrap();

    crate::mode::operation::recover_on_startup(&state.db);
    assert_eq!(
        direct(&state, &AppType::Codex).as_deref(),
        Some("a"),
        "the pointer follows the auth.json already deleted"
    );
    assert_eq!(codex_text(), text, "Codex's own change is left alone");
    ProviderService::switch(&state, AppType::Codex, &official.id).expect("to official");
    assert_eq!(
        codex_login_on_disk(),
        chatgpt_login("acct"),
        "the login made it into the stash"
    );
}

/// 发布过的文件在补完之前又被客户端改掉（这里是用户在 Codex 里重新登录、写了新的
/// auth.json）：单看文件分不出发布开始过没有，按 pending 里的「已开始发布」照样前滚，
/// 还没写出去的登录暂存不能丢，客户端写的 auth.json 不动。
#[tokio::test]
#[serial]
async fn codex_an_interrupted_switch_keeps_the_stash_when_the_published_file_changed_again() {
    let _home = Home::new();
    let (state, _official) = codex_switch_interrupted_after_auth_json().await;
    fs::write(codex_auth_path(), chatgpt_login("other").to_string()).unwrap();

    crate::mode::operation::recover_on_startup(&state.db);
    assert_eq!(direct(&state, &AppType::Codex).as_deref(), Some("a"));
    assert_eq!(codex_doc()["model"].as_str(), Some("gpt-a"));
    let stash = fs::read_to_string(DeviceStore::for_device().file("codex-login-stash.json"))
        .expect("the stash was published");
    assert!(stash.contains("refresh-acct"), "{stash}");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(codex_auth_path()).unwrap()).unwrap(),
        chatgpt_login("other")
    );
}

#[tokio::test]
#[serial]
async fn codex_an_unreadable_login_stash_is_never_overwritten() {
    let _home = Home::new();
    set_preservation(false);
    seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
    let stash = DeviceStore::for_device().file("codex-login-stash.json");
    fs::create_dir_all(stash.parent().unwrap()).unwrap();
    let broken = br#"{"logins":{"account:old":{"tokens":{"refresh_token":"salvageable"}}},"#;
    fs::write(&stash, broken).unwrap();
    let [a, b] = codex_a_b();
    let official = codex_official();
    let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;

    // 切到第三方要把 auth.json 里的登录存进暂存：停下，什么都不写。
    let err = ProviderService::switch(&state, AppType::Codex, "a").expect_err("refused");
    assert!(err.to_string().contains("codex-login-stash.json"), "{err}");
    assert_eq!(fs::read(&stash).unwrap(), broken);
    assert_eq!(codex_login_on_disk(), chatgpt_login("acct"));
    assert_eq!(codex_text(), CODEX_USER_LIVE);

    // 用不着暂存的切换照常。
    fs::remove_file(codex_auth_path()).unwrap();
    ProviderService::switch(&state, AppType::Codex, "b").expect("nothing to stash");
    assert_eq!(fs::read(&stash).unwrap(), broken);
}

#[tokio::test]
#[serial]
async fn codex_official_routes_for_different_accounts_swap_the_login() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex("", Some(&chatgpt_login("acct-a")));
    // 两张没绑托管账号的官方卡，行里各存着一个账号（旧版回填的，暂存第一次建立时
    // 收进去）。
    let official = |id: &str| {
        let mut row = Provider::with_id(
            id.to_string(),
            id.to_uppercase(),
            json!({ "auth": chatgpt_login(id), "config": "" }),
            None,
        );
        row.category = Some("official".to_string());
        row
    };
    let state = state_with(
        AppType::Codex,
        &[official("acct-a"), official("acct-b")],
        "acct-a",
    )
    .await;
    let account = || codex_login_on_disk()["tokens"]["account_id"].clone();
    ProviderService::switch(&state, AppType::Codex, "acct-a").expect("direct a");
    assert_eq!(account(), json!("acct-a"));
    ProviderService::switch(&state, AppType::Codex, "acct-b").expect("direct b");
    assert_eq!(
        account(),
        json!("acct-b"),
        "the direct switch swaps accounts"
    );
    ProviderService::switch(&state, AppType::Codex, "acct-a").expect("direct a again");

    enter(&state, &AppType::Codex, false).await.expect("enter");
    let result = ProviderService::switch(&state, AppType::Codex, "acct-b").expect("route to b");
    assert_eq!(
        result.route,
        Some(RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: true,
        })
    );
    assert_eq!(
        account(),
        json!("acct-b"),
        "Codex signs in as the route's account"
    );
    set_route(&state, &AppType::Codex, "acct-a")
        .await
        .expect("route back to a");
    assert_eq!(account(), json!("acct-a"));
    exit(&state, &AppType::Codex).await.expect("exit");
}

#[tokio::test]
#[serial]
async fn claude_a_retry_after_a_failed_switch_removes_the_failed_targets_exclusive_fields() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let a = claude("a", "https://a.example", json!({}));
    let b = claude(
        "b",
        "https://b.example",
        json!({ "CLAUDE_CODE_DISABLE_ARTIFACT": "1" }),
    );
    let c = claude("c", "https://c.example", json!({}));
    let state = state_with(AppType::Claude, &[a, b, c], "a").await;

    // 切到 b：settings.json 已经写好，指针落定前失败。
    failpoint::crash_at(Some("published:0"));
    let failed = ProviderService::switch(&state, AppType::Claude, "b");
    failpoint::crash_at(None);
    assert!(failed.is_err());
    assert_eq!(settings()["env"]["CLAUDE_CODE_DISABLE_ARTIFACT"], "1");
    assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("a"));

    // 重试切到 c：先补完到 b，再按 b 删它带进来的独有字段。
    ProviderService::switch(&state, AppType::Claude, "c").expect("retry");
    assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("c"));
    let env = &settings()["env"];
    assert_eq!(env["ANTHROPIC_BASE_URL"], "https://c.example");
    assert!(env.get("CLAUDE_CODE_DISABLE_ARTIFACT").is_none(), "{env}");
}

#[tokio::test]
#[serial]
async fn codex_keyring_logins_keep_requires_openai_auth_on_the_preservation_setting() {
    let _home = Home::new();
    for preserve in [true, false] {
        set_preservation(preserve);
        seed_codex("cli_auth_credentials_store = \"keyring\"\n", None);
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
        ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
        let doc = codex_doc();
        assert_eq!(
            doc["model_providers"]["custom"]["requires_openai_auth"].as_bool(),
            Some(preserve),
            "the login lives in the keyring, auth.json says nothing (preserve={preserve})"
        );
        assert_eq!(doc["cli_auth_credentials_store"].as_str(), Some("keyring"));
    }
}

#[tokio::test]
#[serial]
async fn codex_route_switch_with_the_same_contract_leaves_the_client_files_alone() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, None);
    // b、c 在客户端看来一样（同一个模型名，没有独有字段），只是上游和 Key 不同。
    let [a, b] = codex_a_b();
    let mut c = codex_row("c", "https://c.example/v1", "");
    c.settings_config["config"] = json!(c.settings_config["config"]
        .as_str()
        .unwrap()
        .replace("gpt-c", "gpt-b"));
    // d 和 b 只差模型名。
    let d = codex_row("d", "https://d.example/v1", "");
    let state = state_with(AppType::Codex, &[a, b, c, d], "b").await;
    enter(&state, &AppType::Codex, false).await.expect("enter");
    let entered = codex_text();
    assert!(entered.contains(PROXY_TOKEN_PLACEHOLDER), "{entered}");
    assert!(!entered.contains("sk-b"), "{entered}");
    let mtime = fs::metadata(codex_config_path())
        .unwrap()
        .modified()
        .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(codex_config_path(), fs::Permissions::from_mode(0o000)).unwrap();
    }
    let result = ProviderService::switch(&state, AppType::Codex, "c").expect("switch route");
    assert_eq!(
        result.route,
        Some(RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: false,
        })
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(codex_config_path(), fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(codex_text(), entered);
    assert_eq!(
        fs::metadata(codex_config_path())
            .unwrap()
            .modified()
            .unwrap(),
        mtime
    );
    assert_eq!(in_use(&state, &AppType::Codex).as_deref(), Some("c"));

    // 换到只有模型名不同的 d：契约变了，客户端先改写。
    let result = ProviderService::switch(&state, AppType::Codex, "d").expect("route to d");
    assert_eq!(
        result.route,
        Some(RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: true,
        })
    );
    assert_eq!(codex_doc()["model"].as_str(), Some("gpt-d"));
    // 换到独有字段也不同的 a：同样改写。
    ProviderService::switch(&state, AppType::Codex, "a").expect("route to a");
    assert_eq!(codex_doc()["model"].as_str(), Some("gpt-a"));
    assert_eq!(direct(&state, &AppType::Codex).as_deref(), Some("b"));

    exit(&state, &AppType::Codex).await.expect("exit");
    let back = codex_text();
    assert_eq!(codex_doc()["model"].as_str(), Some("gpt-b"));
    assert!(
        back.contains("sk-b") && !back.contains(PROXY_TOKEN_PLACEHOLDER),
        "{back}"
    );
    assert_eq!(codex_user_parts(&back).len(), 6, "{back}");
}

#[tokio::test]
#[serial]
async fn codex_editor_saves_key_fields_to_the_row_and_global_edits_to_live() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, None);
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
    let save = |id: &str, settings: Value, base: Value| {
        let mut row = state
            .db
            .get_provider_by_id(id, &AppType::Codex)
            .unwrap()
            .unwrap();
        row.settings_config = settings;
        ProviderService::update_from_editor(
            &state,
            AppType::Codex,
            Some(id),
            row,
            Some(crate::services::provider::EditorSave {
                base,
                draft: None,
                on_conflict: Default::default(),
            }),
        )
    };

    // 编辑非当前的 b：显示的是切到 b 之后的 config.toml（Key 在输入框里，不在 TOML 里）。
    let b_row = state
        .db
        .get_provider_by_id("b", &AppType::Codex)
        .unwrap()
        .unwrap();
    let view = ProviderService::editor_view(&state, AppType::Codex, &b_row.settings_config, None)
        .expect("view b");
    let shown = view.settings["config"].as_str().unwrap().to_string();
    assert!(
        shown.contains("gpt-b") && shown.contains("https://b.example/v1"),
        "{shown}"
    );
    assert!(!shown.contains("sk-b"), "{shown}");
    assert_eq!(codex_user_parts(&shown).len(), 6, "{shown}");

    let mut edited = view.settings.clone();
    edited["config"] = json!(
        shown
            .replace("\"on-request\"", "\"never\"")
            .replace("\"gpt-b\"", "\"gpt-b2\"")
            + "\n[mcp_servers.git]\ncommand = \"git\"\n"
    );
    save("b", edited, view.settings.clone()).expect("save b");
    let live = codex_text();
    assert!(
        live.contains("approval_policy = \"never\""),
        "global edit applied: {live}"
    );
    assert!(live.contains("[mcp_servers.git]"), "{live}");
    assert_eq!(
        codex_doc()["model"].as_str(),
        Some("gpt-a"),
        "b is not current: {live}"
    );
    let b_row = state
        .db
        .get_provider_by_id("b", &AppType::Codex)
        .unwrap()
        .unwrap();
    let b_config = b_row.settings_config["config"].as_str().unwrap();
    assert!(
        b_config.contains("gpt-b2") && !b_config.contains("approval_policy"),
        "{b_config}"
    );

    // 编辑当前的 a：关键字段立刻换进 live。
    let a_row = state
        .db
        .get_provider_by_id("a", &AppType::Codex)
        .unwrap()
        .unwrap();
    let view = ProviderService::editor_view(&state, AppType::Codex, &a_row.settings_config, None)
        .expect("view a");
    let mut edited = view.settings.clone();
    edited["config"] = json!(view.settings["config"]
        .as_str()
        .unwrap()
        .replace("\"gpt-a\"", "\"gpt-a2\""));
    save("a", edited, view.settings.clone()).expect("save a");
    assert_eq!(codex_doc()["model"].as_str(), Some("gpt-a2"));
    assert!(codex_text().contains("[mcp_servers.git]"));

    // 打开编辑器之后别的程序改了同一个键：保存时报冲突，什么都不写。
    let a_row = state
        .db
        .get_provider_by_id("a", &AppType::Codex)
        .unwrap()
        .unwrap();
    let view = ProviderService::editor_view(&state, AppType::Codex, &a_row.settings_config, None)
        .expect("view a again");
    let outside = codex_text().replace("\"never\"", "\"untrusted\"");
    fs::write(codex_config_path(), &outside).unwrap();
    let mut edited = view.settings.clone();
    edited["config"] = json!(view.settings["config"]
        .as_str()
        .unwrap()
        .replace("\"never\"", "\"on-failure\""));
    let err = save("a", edited, view.settings.clone()).expect_err("conflict");
    assert!(
        err.to_string()
            .contains(crate::live::patch::EDIT_CONFLICT_CODE),
        "{err}"
    );
    assert_eq!(codex_text(), outside);
}

/// live 里用户自己写的独有字段（`model_verbosity` 这类）不归当前供应商：原样保存不会
/// 把它收进行，切走时也就不会删掉；在编辑器里删掉它就从 live 删；新加的独有字段归
/// 供应商。
#[tokio::test]
#[serial]
async fn codex_editor_leaves_exclusive_fields_from_live_to_the_user() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(
        &CODEX_USER_LIVE.replace(
            "model = \"gpt-a\"\n",
            "model = \"gpt-a\"\nmodel_verbosity = \"high\"\nmodel_supports_reasoning_summaries = true\n",
        ),
        None,
    );
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
    let open = |id: &str| {
        let row = state
            .db
            .get_provider_by_id(id, &AppType::Codex)
            .unwrap()
            .unwrap();
        let view = ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
            .expect("view");
        (row, view.settings)
    };
    let save = |mut row: Provider, edited: Value, base: Value| {
        row.settings_config = edited;
        ProviderService::update_from_editor(
            &state,
            AppType::Codex,
            None,
            row,
            Some(crate::services::provider::EditorSave {
                base,
                draft: None,
                on_conflict: Default::default(),
            }),
        )
    };
    let a_config = |state: &AppState| {
        state
            .db
            .get_provider_by_id("a", &AppType::Codex)
            .unwrap()
            .unwrap()
            .settings_config["config"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // 只改名、配置原样保存。
    let (mut row, base) = open("a");
    row.name = "renamed".into();
    save(row, base.clone(), base).expect("save as is");
    let config = a_config(&state);
    assert!(
        config.contains("model_context_window = 200000")
            && !config.contains("model_verbosity")
            && !config.contains("model_supports_reasoning_summaries"),
        "{config}"
    );

    // 删掉一个从 live 带进来的，再加一个供应商自己的。
    let (row, base) = open("a");
    let mut edited = base.clone();
    edited["config"] = json!(base["config"]
        .as_str()
        .unwrap()
        .replace("model_supports_reasoning_summaries = true\n", "")
        .replace(
            "model_verbosity",
            "model_auto_compact_token_limit = 100000\nmodel_verbosity"
        ));
    save(row, edited, base).expect("save edits");
    let live = codex_text();
    assert!(
        !live.contains("model_supports_reasoning_summaries")
            && live.contains("model_auto_compact_token_limit = 100000"),
        "{live}"
    );
    let config = a_config(&state);
    assert!(
        config.contains("model_auto_compact_token_limit = 100000")
            && !config.contains("model_verbosity"),
        "{config}"
    );

    ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
    let live = codex_doc();
    assert_eq!(live["model_verbosity"].as_str(), Some("high"));
    assert!(live.get("model_auto_compact_token_limit").is_none());
    assert!(live.get("model_context_window").is_none());
}

/// 打开编辑器之后客户端改了一个从 live 带进来的独有字段：用户没动它，保存时不收进行，
/// 也不把打开时的值写回去。live 里生效的 profile 选着路由表时，编辑和新增都照样能存。
#[tokio::test]
#[serial]
async fn codex_editor_leaves_what_the_user_did_not_touch_to_the_client() {
    let _home = Home::new();
    set_preservation(true);
    let live = CODEX_USER_LIVE.replace(
        "model = \"gpt-a\"\n",
        "model = \"gpt-a\"\nmodel_verbosity = \"high\"\n",
    );
    seed_codex(
        &format!("profile = \"work\"\n{live}\n[profiles.work]\nmodel_provider = \"custom\"\n"),
        None,
    );
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

    let mut row = state
        .db
        .get_provider_by_id("a", &AppType::Codex)
        .unwrap()
        .unwrap();
    let base = ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
        .expect("view")
        .settings;
    let changed = codex_text().replace("model_verbosity = \"high\"", "model_verbosity = \"low\"");
    fs::write(codex_config_path(), &changed).unwrap();
    row.name = "renamed".into();
    row.settings_config = base.clone();
    ProviderService::update_from_editor(
        &state,
        AppType::Codex,
        None,
        row,
        Some(crate::services::provider::EditorSave {
            base,
            draft: None,
            on_conflict: Default::default(),
        }),
    )
    .expect("save with the profile active");
    let doc = codex_doc();
    assert_eq!(
        doc["model_verbosity"].as_str(),
        Some("low"),
        "{}",
        codex_text()
    );
    assert_eq!(
        doc["profiles"]["work"]["model_provider"].as_str(),
        Some("custom")
    );
    let stored = state
        .db
        .get_provider_by_id("a", &AppType::Codex)
        .unwrap()
        .unwrap();
    assert!(!stored.settings_config["config"]
        .as_str()
        .unwrap()
        .contains("model_verbosity"));

    let draft = codex_row("c", "https://c.example/v1", "");
    let view = ProviderService::editor_view(&state, AppType::Codex, &draft.settings_config, None)
        .expect("draft view");
    add_from_editor(
        &state,
        AppType::Codex,
        draft,
        view.settings.clone(),
        view.settings,
    )
    .expect("add with the profile active");
}

/// 新增对话框打开之后，客户端改了一个从 live 带进来的独有字段：草稿里没有它，保存时不
/// 收进新供应商，切到新供应商再切走，客户端改的值还在。预设自己带的独有字段照样归新
/// 供应商。
#[tokio::test]
#[serial]
async fn codex_add_dialog_leaves_a_live_field_changed_after_opening_to_the_client() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(
        &CODEX_USER_LIVE.replace(
            "model = \"gpt-a\"\n",
            "model = \"gpt-a\"\nmodel_verbosity = \"high\"\n",
        ),
        None,
    );
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
    let config_of = |id: &str| {
        state
            .db
            .get_provider_by_id(id, &AppType::Codex)
            .unwrap()
            .unwrap()
            .settings_config["config"]
            .as_str()
            .unwrap()
            .to_string()
    };

    let draft = codex_row(
        "c",
        "https://c.example/v1",
        "model_auto_compact_token_limit = 90000",
    );
    let view = ProviderService::editor_view(&state, AppType::Codex, &draft.settings_config, None)
        .expect("draft view");
    let changed = codex_text().replace("model_verbosity = \"high\"", "model_verbosity = \"low\"");
    fs::write(codex_config_path(), changed).unwrap();
    add_from_editor(
        &state,
        AppType::Codex,
        draft,
        view.settings.clone(),
        view.settings,
    )
    .expect("add c");
    let stored = config_of("c");
    assert!(
        stored.contains("model_auto_compact_token_limit = 90000")
            && !stored.contains("model_verbosity"),
        "{stored}"
    );
    ProviderService::switch(&state, AppType::Codex, "c").expect("to c");
    ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
    assert_eq!(codex_doc()["model_verbosity"].as_str(), Some("low"));

    // 旧的调用方不带草稿：退回和 live 里用户自己的值比（live 没被改过时分得清）。
    let mut legacy = codex_row(
        "d",
        "https://d.example/v1",
        "model_auto_compact_token_limit = 80000",
    );
    let base = ProviderService::editor_view(&state, AppType::Codex, &legacy.settings_config, None)
        .expect("legacy view")
        .settings;
    legacy.settings_config = base.clone();
    ProviderService::add_from_editor(
        &state,
        AppType::Codex,
        legacy,
        true,
        Some(crate::services::provider::EditorSave {
            base,
            draft: None,
            on_conflict: Default::default(),
        }),
    )
    .expect("add d");
    let stored = config_of("d");
    assert!(
        stored.contains("model_auto_compact_token_limit = 80000")
            && !stored.contains("model_verbosity"),
        "{stored}"
    );
}

/// 编辑器里把路由表从 custom 改名成别的表：那张表归供应商（按内容收成 custom 表），
/// 不当成全局设置写进 live，切走后表和里面的 Key 都不会留下。
#[tokio::test]
#[serial]
async fn codex_editor_route_table_renamed_in_the_editor_stays_with_the_provider() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, None);
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

    let mut row = state
        .db
        .get_provider_by_id("a", &AppType::Codex)
        .unwrap()
        .unwrap();
    let view = ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
        .expect("view a");
    let shown = view.settings["config"].as_str().unwrap();
    assert!(shown.contains("[model_providers.custom]\n"), "{shown}");
    let mut edited = view.settings.clone();
    edited["config"] = json!(shown
        .replace(
            "model_provider = \"custom\"",
            "model_provider = \"deepseek\""
        )
        .replace(
            "[model_providers.custom]\n",
            "[model_providers.deepseek]\nexperimental_bearer_token = \"sk-secret\"\n",
        ));
    row.settings_config = edited;
    ProviderService::update_from_editor(
        &state,
        AppType::Codex,
        None,
        row,
        Some(crate::services::provider::EditorSave {
            base: view.settings,
            draft: None,
            on_conflict: Default::default(),
        }),
    )
    .expect("save");
    let live = codex_text();
    assert!(!live.contains("[model_providers.deepseek]"), "{live}");
    assert!(live.contains("[model_providers.ollama_local]"), "{live}");

    ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
    let live = codex_text();
    assert!(
        !live.contains("sk-secret") && !live.contains("deepseek"),
        "{live}"
    );
}

#[tokio::test]
#[serial]
async fn codex_a_login_refreshed_during_the_switch_is_never_overwritten() {
    let _home = Home::new();
    set_preservation(false);
    seed_codex("", Some(&chatgpt_login("acct")));
    let [a, _] = codex_a_b();
    let official = codex_official();
    let state = state_with(AppType::Codex, &[a, official.clone()], &official.id).await;
    let config_before = codex_text();

    // 计划删掉 auth.json 之后、发布之前，Codex CLI 刷新了登录。
    let mut refreshed = chatgpt_login("acct");
    refreshed["tokens"]["refresh_token"] = json!("refresh-acct-2");
    let fresh = refreshed.to_string();
    failpoint::on_before_publish(Some(Box::new(move |_, path: &std::path::Path| {
        if path == codex_auth_path() {
            fs::write(path, &fresh).unwrap();
        }
    })));
    let result = ProviderService::switch(&state, AppType::Codex, "a");
    failpoint::on_before_publish(None);

    assert!(
        result.is_err(),
        "the switch stops instead of deleting a newer login"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(codex_auth_path()).unwrap()).unwrap(),
        refreshed
    );
    assert_eq!(codex_text(), config_before, "nothing else was published");
    assert_eq!(
        direct(&state, &AppType::Codex).as_deref(),
        Some(official.id.as_str())
    );
    assert!(
        state::pending(&DeviceStore::for_device(), "codex")
            .unwrap()
            .is_none(),
        "an operation that never published leaves no pending"
    );
}

// ===== Gemini CLI =====

const GEMINI_USER_ENV: &str = "# my notes\nGEMINI_SANDBOX=docker\nGEMINI_API_KEY=key-a\nDEBUG=1\nGOOGLE_GEMINI_BASE_URL=https://a.example\nGEMINI_MODEL=m-a\n";
const GEMINI_USER_SETTINGS: &str = r#"{
  "model": {
    "name": "m-a",
    "compressionThreshold": 0.5
  },
  "security": {
    "auth": {
      "selectedType": "gemini-api-key"
    }
  },
  "mcpServers": {
    "fs": {
      "command": "fs"
    }
  }
}
"#;

fn gemini_env_path() -> std::path::PathBuf {
    crate::gemini_config::get_gemini_env_path()
}

fn gemini_settings_path() -> std::path::PathBuf {
    crate::gemini_config::get_gemini_settings_path()
}

fn seed_gemini(env: &str, settings: &str) {
    fs::create_dir_all(gemini_env_path().parent().unwrap()).unwrap();
    fs::write(gemini_env_path(), env).unwrap();
    fs::write(gemini_settings_path(), settings).unwrap();
}

fn gemini_env() -> String {
    fs::read_to_string(gemini_env_path()).unwrap()
}

fn gemini_settings() -> Value {
    serde_json::from_slice(&fs::read(gemini_settings_path()).unwrap()).unwrap()
}

/// `.env` 里用户自己的行（注释、非关键字段），按原顺序。
fn gemini_user_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| {
            line.split_once('=')
                .is_none_or(|(key, _)| !crate::live::floor::gemini_floor_env(key.trim()))
        })
        .map(str::to_string)
        .collect()
}

fn gemini(id: &str, env: Value, config: Value) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({ "env": env, "config": config }),
        None,
    )
}

fn gemini_a_vertex() -> [Provider; 2] {
    [
        gemini(
            "a",
            json!({
                "GEMINI_API_KEY": "key-a",
                "GOOGLE_GEMINI_BASE_URL": "https://a.example",
                "GEMINI_MODEL": "m-a"
            }),
            json!({ "model": { "name": "m-a" } }),
        ),
        gemini(
            "vertex",
            json!({
                "GOOGLE_GENAI_USE_VERTEXAI": "true",
                "GOOGLE_CLOUD_PROJECT": "p"
            }),
            json!({}),
        ),
    ]
}

#[tokio::test]
#[serial]
async fn gemini_switch_replaces_only_key_fields_and_round_trips() {
    let _home = Home::new();
    seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
    let state = state_with(AppType::Gemini, &gemini_a_vertex(), "a").await;

    ProviderService::switch(&state, AppType::Gemini, "vertex").expect("to vertex");
    assert_eq!(
        gemini_env(),
        "# my notes\nGEMINI_SANDBOX=docker\nDEBUG=1\nGOOGLE_GENAI_USE_VERTEXAI=true\nGOOGLE_CLOUD_PROJECT=p\n"
    );
    assert_eq!(
        gemini_settings(),
        json!({
            "model": { "compressionThreshold": 0.5 },
            "security": { "auth": { "selectedType": "gemini-api-key" } },
            "mcpServers": { "fs": { "command": "fs" } }
        })
    );

    ProviderService::switch(&state, AppType::Gemini, "a").expect("back to a");
    let env = gemini_env();
    assert_eq!(gemini_user_lines(&env), gemini_user_lines(GEMINI_USER_ENV));
    for line in [
        "GEMINI_API_KEY=key-a",
        "GOOGLE_GEMINI_BASE_URL=https://a.example",
        "GEMINI_MODEL=m-a",
    ] {
        assert!(env.contains(line), "{env}");
    }
    assert!(!env.contains("VERTEX"), "{env}");
    assert_eq!(
        gemini_settings(),
        serde_json::from_str::<Value>(GEMINI_USER_SETTINGS).unwrap()
    );
    assert_eq!(direct(&state, &AppType::Gemini).as_deref(), Some("a"));
}

#[tokio::test]
#[serial]
async fn gemini_official_switch_selects_the_google_login() {
    let _home = Home::new();
    seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
    let [a, _] = gemini_a_vertex();
    let mut official = gemini("google", json!({}), json!({}));
    official.category = Some("official".to_string());
    let state = state_with(AppType::Gemini, &[a, official], "a").await;

    ProviderService::switch(&state, AppType::Gemini, "google").expect("to official");
    assert_eq!(gemini_env(), "# my notes\nGEMINI_SANDBOX=docker\nDEBUG=1\n");
    assert_eq!(
        gemini_settings()["security"]["auth"]["selectedType"],
        json!("oauth-personal")
    );
    assert!(gemini_settings()["model"].get("name").is_none());
}

#[tokio::test]
#[serial]
async fn gemini_proxy_contract_follows_the_route_model_and_skips_same_contract_routes() {
    let _home = Home::new();
    seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
    let [a, _] = gemini_a_vertex();
    let mut b = a.clone();
    b.id = "b".to_string();
    b.settings_config["env"]["GEMINI_API_KEY"] = json!("key-b");
    b.settings_config["env"]["GOOGLE_GEMINI_BASE_URL"] = json!("https://b.example");
    let state = state_with(AppType::Gemini, &[a, b], "a").await;

    enter(&state, &AppType::Gemini, false).await.expect("enter");
    let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
    let env = gemini_env();
    assert!(env.contains("GEMINI_API_KEY=PROXY_MANAGED"), "{env}");
    assert!(
        env.contains(&format!("GOOGLE_GEMINI_BASE_URL={proxy_url}")),
        "{env}"
    );
    assert!(env.contains("GEMINI_MODEL=m-a"), "{env}");
    assert_eq!(gemini_user_lines(&env), gemini_user_lines(GEMINI_USER_ENV));

    // b 的模型名和 a 一样：契约相同，客户端文件不读也不写。
    let before = fs::read(gemini_env_path()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(gemini_env_path(), fs::Permissions::from_mode(0o000)).unwrap();
    }
    let result = ProviderService::switch(&state, AppType::Gemini, "b").expect("route to b");
    assert_eq!(
        result.route,
        Some(RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: false,
        })
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(gemini_env_path(), fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(fs::read(gemini_env_path()).unwrap(), before);
    assert_eq!(in_use(&state, &AppType::Gemini).as_deref(), Some("b"));

    let mut changed = state
        .db
        .get_provider_by_id("a", &AppType::Gemini)
        .unwrap()
        .unwrap();
    changed.settings_config["env"]["GEMINI_MODEL"] = json!("m-changed");
    state.db.save_provider(&AppType::Gemini, &changed).unwrap();
    let result = ProviderService::switch(&state, AppType::Gemini, "a").expect("changed model");
    assert_eq!(
        result.route,
        Some(RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: true,
        })
    );
    assert!(gemini_env().contains("GEMINI_MODEL=m-changed"));

    exit(&state, &AppType::Gemini).await.expect("exit");
    let env = gemini_env();
    assert!(env.contains("GEMINI_API_KEY=key-a"), "{env}");
    assert!(!env.contains(PROXY_TOKEN_PLACEHOLDER), "{env}");
    assert_eq!(gemini_user_lines(&env), gemini_user_lines(GEMINI_USER_ENV));
}

#[tokio::test]
#[serial]
async fn gemini_editor_saves_key_fields_to_the_row_and_global_edits_to_live() {
    let _home = Home::new();
    seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
    let state = state_with(AppType::Gemini, &gemini_a_vertex(), "a").await;

    let a = state
        .db
        .get_provider_by_id("a", &AppType::Gemini)
        .unwrap()
        .unwrap();
    let view = ProviderService::editor_view(&state, AppType::Gemini, &a.settings_config, None)
        .expect("view");
    assert_eq!(view.settings["env"]["GEMINI_SANDBOX"], json!("docker"));
    assert_eq!(
        view.settings["config"]["mcpServers"]["fs"]["command"],
        json!("fs")
    );

    let mut edited = view.settings.clone();
    edited["env"]["GEMINI_API_KEY"] = json!("key-a2");
    edited["env"]["DEBUG"] = json!("2");
    edited["config"]["ui"] = json!({ "theme": "dark" });
    let mut row = a.clone();
    row.settings_config = edited;
    ProviderService::update_from_editor(
        &state,
        AppType::Gemini,
        Some("a"),
        row,
        Some(crate::services::provider::EditorSave {
            base: view.settings.clone(),
            draft: None,
            on_conflict: Default::default(),
        }),
    )
    .expect("save");

    let env = gemini_env();
    assert!(
        env.contains("GEMINI_API_KEY=key-a2") && env.contains("DEBUG=2"),
        "{env}"
    );
    assert_eq!(gemini_settings()["ui"], json!({ "theme": "dark" }));
    let saved = state
        .db
        .get_provider_by_id("a", &AppType::Gemini)
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.settings_config["env"]["GEMINI_API_KEY"],
        json!("key-a2")
    );
    assert!(saved.settings_config["env"].get("DEBUG").is_none());
    assert!(saved.settings_config["config"].get("ui").is_none());

    // 编辑非当前的 vertex：live 的关键字段不动，全局改动照写。
    let vertex = state
        .db
        .get_provider_by_id("vertex", &AppType::Gemini)
        .unwrap()
        .unwrap();
    let view = ProviderService::editor_view(&state, AppType::Gemini, &vertex.settings_config, None)
        .expect("view vertex");
    assert!(view.settings["env"].get("GEMINI_API_KEY").is_none());
    let mut edited = view.settings.clone();
    edited["env"]["GOOGLE_CLOUD_PROJECT"] = json!("p2");
    edited["env"]["DEBUG"] = json!("3");
    let mut row = vertex.clone();
    row.settings_config = edited;
    ProviderService::update_from_editor(
        &state,
        AppType::Gemini,
        Some("vertex"),
        row,
        Some(crate::services::provider::EditorSave {
            base: view.settings,
            draft: None,
            on_conflict: Default::default(),
        }),
    )
    .expect("save vertex");
    let env = gemini_env();
    assert!(
        env.contains("GEMINI_API_KEY=key-a2") && env.contains("DEBUG=3"),
        "{env}"
    );
    assert!(!env.contains("GOOGLE_CLOUD_PROJECT"), "{env}");
    let saved = state
        .db
        .get_provider_by_id("vertex", &AppType::Gemini)
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.settings_config["env"]["GOOGLE_CLOUD_PROJECT"],
        json!("p2")
    );
}

// ===== Grok Build =====

const GROK_USER_LIVE: &str = "# mine\n[ui]\ntheme = \"dark\"\n\n[model.mine]\nmodel = \"m\"\nname = \"Mine\"\n\n[mcp_servers.fs]\ncommand = \"fs\"\n";

fn grok_path() -> std::path::PathBuf {
    crate::grok_config::get_grok_config_path()
}

fn seed_grok(text: &str) {
    fs::create_dir_all(grok_path().parent().unwrap()).unwrap();
    fs::write(grok_path(), text).unwrap();
}

fn grok_text() -> String {
    fs::read_to_string(grok_path()).unwrap()
}

fn grok_doc() -> toml::Table {
    toml::from_str(&grok_text()).unwrap()
}

/// live 里的 `[model.*]` 表名（排好序）。
fn grok_tables() -> Vec<String> {
    grok_doc()
        .get("model")
        .and_then(|model| model.as_table())
        .map(|tables| tables.keys().cloned().collect())
        .unwrap_or_default()
}

fn grok_row(id: &str, table: &str, extra: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({ "config": format!(
            "[models]\ndefault = \"{table}\"\n\n[model.\"{table}\"]\nmodel = \"{id}-model\"\nname = \"{id}\"\nbase_url = \"https://{id}.example/v1\"\napi_key = \"key-{id}\"\napi_backend = \"responses\"\ncontext_window = 500000\n{extra}"
        ) }),
        None,
    )
}

fn grok_official() -> Provider {
    let mut official = Provider::with_id(
        "grok-official".to_string(),
        "Grok Official".to_string(),
        json!({ "config": "" }),
        None,
    );
    official.category = Some("official".to_string());
    official
}

#[tokio::test]
#[serial]
async fn grok_switch_deletes_the_written_table_even_after_the_client_changed_the_default() {
    let _home = Home::new();
    seed_grok(GROK_USER_LIVE);
    let state = state_with(
        AppType::GrokBuild,
        &[grok_row("a", "grok-4.5", ""), grok_official()],
        "grok-official",
    )
    .await;

    ProviderService::switch(&state, AppType::GrokBuild, "a").expect("to a");
    assert_eq!(grok_doc()["models"]["default"].as_str(), Some("grok-4.5"));
    assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);

    // Grok 的 /settings 把默认模型改成了内置的 grok-4.6。
    let changed = grok_text().replace("default = \"grok-4.5\"", "default = \"grok-4.6\"");
    fs::write(grok_path(), changed).unwrap();

    ProviderService::switch(&state, AppType::GrokBuild, "grok-official").expect("to official");
    assert_eq!(grok_tables(), vec!["mine"], "{}", grok_text());
    assert!(grok_doc().get("models").is_none(), "{}", grok_text());
    assert!(grok_text().starts_with("# mine\n[ui]\ntheme = \"dark\"\n"));

    // 切回去照样能用。
    ProviderService::switch(&state, AppType::GrokBuild, "a").expect("back to a");
    assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);
}

#[tokio::test]
#[serial]
async fn grok_table_keys_follow_the_provider_and_renames_replace_the_old_table() {
    let _home = Home::new();
    seed_grok(GROK_USER_LIVE);
    let state = state_with(
        AppType::GrokBuild,
        &[
            grok_row("a", "grok-4.5", "reasoning_summary = \"none\"\n"),
            grok_row("b", "b", ""),
        ],
        "b",
    )
    .await;

    ProviderService::switch(&state, AppType::GrokBuild, "a").expect("to a");
    assert_eq!(
        grok_doc()["model"]["grok-4.5"]["reasoning_summary"].as_str(),
        Some("none")
    );
    ProviderService::switch(&state, AppType::GrokBuild, "b").expect("to b");
    assert_eq!(grok_tables(), vec!["b", "mine"]);
    assert!(
        !grok_text().contains("reasoning_summary"),
        "{}",
        grok_text()
    );

    // 编辑当前供应商、把表名从 b 改成 grok-4.6：live 里只剩新表。
    let mut b = state
        .db
        .get_provider_by_id("b", &AppType::GrokBuild)
        .unwrap()
        .unwrap();
    b.settings_config = grok_row("b", "grok-4.6", "").settings_config;
    ProviderService::update(&state, AppType::GrokBuild, Some("b"), b).expect("rename");
    assert_eq!(grok_tables(), vec!["grok-4.6", "mine"]);
    assert_eq!(grok_doc()["models"]["default"].as_str(), Some("grok-4.6"));
}

#[tokio::test]
#[serial]
async fn grok_without_a_write_record_infers_the_table_an_older_version_wrote() {
    let _home = Home::new();
    // 旧版把 a 的行整份写进了 live：新版没有写入记录。
    let a = grok_row("a", "grok-4.5", "");
    let old = format!(
        "{}\n{}",
        a.settings_config["config"].as_str().unwrap(),
        GROK_USER_LIVE
    );
    seed_grok(&old);
    let state = state_with(AppType::GrokBuild, &[a, grok_row("b", "b", "")], "a").await;

    ProviderService::switch(&state, AppType::GrokBuild, "b").expect("to b");
    assert_eq!(grok_tables(), vec!["b", "mine"], "{}", grok_text());
    assert_eq!(
        state::written(&DeviceStore::for_device(), "grokbuild")
            .unwrap()
            .unwrap()
            .tables,
        vec!["b".to_string()]
    );
}

#[tokio::test]
#[serial]
async fn grok_proxy_writes_the_route_table_through_the_engine() {
    let _home = Home::new();
    seed_grok(GROK_USER_LIVE);
    let state = state_with(
        AppType::GrokBuild,
        &[
            grok_row("a", "grok-4.5", ""),
            grok_row("b", "b", ""),
            grok_official(),
        ],
        "a",
    )
    .await;
    ProviderService::switch(&state, AppType::GrokBuild, "a").expect("direct a");

    enter(&state, &AppType::GrokBuild, false)
        .await
        .expect("enter");
    let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
    let doc = grok_doc();
    let table = &doc["model"]["grok-4.5"];
    assert_eq!(table["api_key"].as_str(), Some(PROXY_TOKEN_PLACEHOLDER));
    assert_eq!(
        table["base_url"].as_str(),
        Some(format!("{proxy_url}/grokbuild/v1").as_str())
    );

    // 换一家表名不同的路由：旧的代理表按写入记录删掉。
    let result = ProviderService::switch(&state, AppType::GrokBuild, "b").expect("route to b");
    assert_eq!(
        result.route,
        Some(RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: true,
        })
    );
    assert_eq!(grok_tables(), vec!["b", "mine"]);
    let result = ProviderService::switch(&state, AppType::GrokBuild, "b").expect("same contract");
    assert!(!result.route.unwrap().client_config_changed);
    assert!(
        ProviderService::switch(&state, AppType::GrokBuild, "grok-official").is_err(),
        "the official account cannot be routed"
    );

    exit(&state, &AppType::GrokBuild).await.expect("exit");
    assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);
    assert_eq!(
        grok_doc()["model"]["grok-4.5"]["api_key"].as_str(),
        Some("key-a")
    );
    assert!(grok_text().contains("[mcp_servers.fs]"));
}

#[tokio::test]
#[serial]
async fn grok_switch_crash_rolls_forward_with_the_write_record() {
    let _home = Home::new();
    seed_grok(GROK_USER_LIVE);
    let state = state_with(
        AppType::GrokBuild,
        &[grok_row("a", "grok-4.5", ""), grok_row("b", "b", "")],
        "a",
    )
    .await;
    ProviderService::switch(&state, AppType::GrokBuild, "a").expect("direct a");

    for point in ["published:0", "target"] {
        ProviderService::switch(&state, AppType::GrokBuild, "a").expect("reset");
        failpoint::crash_at(Some(point));
        let crashed = ProviderService::switch(&state, AppType::GrokBuild, "b");
        failpoint::crash_at(None);
        assert!(crashed.is_err(), "{point}");

        crate::mode::operation::recover_on_startup(&state.db);
        assert_eq!(grok_tables(), vec!["b", "mine"], "{point}");
        assert_eq!(direct(&state, &AppType::GrokBuild).as_deref(), Some("b"));
        assert_eq!(
            state::written(&DeviceStore::for_device(), "grokbuild")
                .unwrap()
                .unwrap()
                .tables,
            vec!["b".to_string()],
            "{point}"
        );
    }
}

#[tokio::test]
#[serial]
async fn grok_editor_saves_the_table_to_the_row_and_global_edits_to_live() {
    let _home = Home::new();
    seed_grok(GROK_USER_LIVE);
    let state = state_with(
        AppType::GrokBuild,
        &[grok_row("a", "grok-4.5", ""), grok_row("b", "b", "")],
        "a",
    )
    .await;
    ProviderService::switch(&state, AppType::GrokBuild, "a").expect("direct a");

    let a = state
        .db
        .get_provider_by_id("a", &AppType::GrokBuild)
        .unwrap()
        .unwrap();
    let view = ProviderService::editor_view(&state, AppType::GrokBuild, &a.settings_config, None)
        .expect("view");
    let shown = view.settings["config"].as_str().unwrap().to_string();
    assert!(
        shown.contains("[model.mine]") && shown.contains("key-a"),
        "{shown}"
    );

    let edited = shown
        .replace("theme = \"dark\"", "theme = \"light\"")
        .replace("a-model", "a-model-2");
    let mut row = a.clone();
    row.settings_config = json!({ "config": edited });
    ProviderService::update_from_editor(
        &state,
        AppType::GrokBuild,
        Some("a"),
        row,
        Some(crate::services::provider::EditorSave {
            base: view.settings,
            draft: None,
            on_conflict: Default::default(),
        }),
    )
    .expect("save");

    let doc = grok_doc();
    assert_eq!(doc["ui"]["theme"].as_str(), Some("light"));
    assert_eq!(
        doc["model"]["grok-4.5"]["model"].as_str(),
        Some("a-model-2")
    );
    let saved = state
        .db
        .get_provider_by_id("a", &AppType::GrokBuild)
        .unwrap()
        .unwrap();
    let row_text = saved.settings_config["config"].as_str().unwrap();
    assert!(row_text.contains("a-model-2"), "{row_text}");
    assert!(
        !row_text.contains("[ui]") && !row_text.contains("[model.mine]"),
        "{row_text}"
    );
}

// ---------- 新增对话框：和编辑器同一套规则 ----------

async fn state_without_providers() -> AppState {
    let db = Arc::new(Database::memory().expect("memory db"));
    let mut proxy = db.get_global_proxy_config().await.expect("ephemeral port");
    proxy.listen_port = 0;
    db.update_global_proxy_config(proxy)
        .await
        .expect("ephemeral port");
    AppState::new(db)
}

fn add_from_editor(
    state: &AppState,
    app: AppType,
    mut row: Provider,
    edited: Value,
    base: Value,
) -> Result<bool, AppError> {
    // 和新增对话框一样：`row` 是投影成 `base` 的草稿。
    let draft = std::mem::replace(&mut row.settings_config, edited);
    ProviderService::add_from_editor(
        state,
        app,
        row,
        true,
        Some(crate::services::provider::EditorSave {
            base,
            draft: Some(draft),
            on_conflict: Default::default(),
        }),
    )
}

#[tokio::test]
#[serial]
async fn codex_add_dialog_saves_key_fields_to_the_row_and_global_edits_to_live() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, None);
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

    // 新增 c：显示的是切到 c 之后的 config.toml，全局部分来自 live。
    let draft = codex_row("c", "https://c.example/v1", "");
    let view = ProviderService::editor_view(&state, AppType::Codex, &draft.settings_config, None)
        .expect("view c");
    let shown = view.settings["config"].as_str().unwrap().to_string();
    assert!(
        shown.contains("gpt-c") && shown.contains("approval_policy"),
        "{shown}"
    );
    let mut edited = view.settings.clone();
    edited["config"] = json!(shown.replace("\"on-request\"", "\"never\""));
    add_from_editor(&state, AppType::Codex, draft, edited, view.settings).expect("add c");

    let live = codex_text();
    assert!(live.contains("approval_policy = \"never\""), "{live}");
    assert_eq!(codex_doc()["model"].as_str(), Some("gpt-a"), "{live}");
    let c = state
        .db
        .get_provider_by_id("c", &AppType::Codex)
        .unwrap()
        .unwrap();
    let c_config = c.settings_config["config"].as_str().unwrap();
    assert!(
        c_config.contains("gpt-c") && !c_config.contains("approval_policy"),
        "{c_config}"
    );
    assert_eq!(
        c.meta.as_ref().and_then(|meta| meta.common_config_enabled),
        Some(true)
    );
}

/// 新增对话框的底已经套了预设：预设带的独有字段归新供应商，live 里用户自己写的不归它。
#[tokio::test]
#[serial]
async fn codex_add_dialog_keeps_the_users_exclusive_fields_out_of_the_new_row() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(
        &CODEX_USER_LIVE.replace(
            "model = \"gpt-a\"\n",
            "model = \"gpt-a\"\nmodel_verbosity = \"high\"\n",
        ),
        None,
    );
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

    let draft = codex_row(
        "c",
        "https://c.example/v1",
        "model_auto_compact_token_limit = 90000\n",
    );
    let view = ProviderService::editor_view(&state, AppType::Codex, &draft.settings_config, None)
        .expect("view c");
    let shown = view.settings["config"].as_str().unwrap();
    assert!(
        shown.contains("model_verbosity") && shown.contains("model_auto_compact_token_limit"),
        "{shown}"
    );
    add_from_editor(
        &state,
        AppType::Codex,
        draft,
        view.settings.clone(),
        view.settings,
    )
    .expect("add c");

    let c = state
        .db
        .get_provider_by_id("c", &AppType::Codex)
        .unwrap()
        .unwrap();
    let c_config = c.settings_config["config"].as_str().unwrap();
    assert!(
        c_config.contains("model_auto_compact_token_limit = 90000")
            && !c_config.contains("model_verbosity"),
        "{c_config}"
    );
}

#[tokio::test]
#[serial]
async fn gemini_add_dialog_first_provider_writes_key_fields_and_sets_the_pointer() {
    let _home = Home::new();
    seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
    let state = state_without_providers().await;

    let draft = Provider::with_id(
        "c".to_string(),
        "C".to_string(),
        json!({ "env": {
            "GEMINI_API_KEY": "key-c",
            "GOOGLE_GEMINI_BASE_URL": "https://c.example",
            "GEMINI_MODEL": "m-c",
        }, "config": {} }),
        None,
    );
    let view = ProviderService::editor_view(&state, AppType::Gemini, &draft.settings_config, None)
        .expect("view c");
    assert_eq!(view.settings["env"]["GEMINI_SANDBOX"], json!("docker"));
    let mut edited = view.settings.clone();
    edited["env"]["DEBUG"] = json!("5");
    edited["config"]["ui"] = json!({ "theme": "dark" });
    add_from_editor(&state, AppType::Gemini, draft, edited, view.settings).expect("add c");

    let env = gemini_env();
    assert!(
        env.contains("GEMINI_API_KEY=key-c")
            && env.contains("GEMINI_MODEL=m-c")
            && env.contains("DEBUG=5")
            && env.contains("# my notes"),
        "{env}"
    );
    assert_eq!(gemini_settings()["ui"], json!({ "theme": "dark" }));
    assert_eq!(
        crate::mode::current::provider_for(
            &state.db,
            &AppType::Gemini,
            crate::mode::current::Purpose::Direct
        )
        .unwrap()
        .as_deref(),
        Some("c")
    );
    let c = state
        .db
        .get_provider_by_id("c", &AppType::Gemini)
        .unwrap()
        .unwrap();
    assert!(c.settings_config["env"].get("DEBUG").is_none());
    assert!(c.settings_config["config"].get("ui").is_none());
}

#[tokio::test]
#[serial]
async fn grok_add_dialog_first_provider_records_the_written_table() {
    let _home = Home::new();
    seed_grok(GROK_USER_LIVE);
    let state = state_without_providers().await;

    let draft = grok_row("a", "grok-4.5", "");
    let view =
        ProviderService::editor_view(&state, AppType::GrokBuild, &draft.settings_config, None)
            .expect("view a");
    let shown = view.settings["config"].as_str().unwrap().to_string();
    assert!(
        shown.contains("[model.mine]") && shown.contains("key-a"),
        "{shown}"
    );
    let edited = json!({ "config": shown.replace("theme = \"dark\"", "theme = \"light\"") });
    add_from_editor(&state, AppType::GrokBuild, draft, edited, view.settings).expect("add a");

    let doc = grok_doc();
    assert_eq!(doc["ui"]["theme"].as_str(), Some("light"));
    assert_eq!(doc["models"]["default"].as_str(), Some("grok-4.5"));
    assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);
    let written = crate::mode::state::written(&DeviceStore::for_device(), "grokbuild")
        .unwrap()
        .expect("write record");
    assert_eq!(written.tables, vec!["grok-4.5".to_string()]);
    let a = state
        .db
        .get_provider_by_id("a", &AppType::GrokBuild)
        .unwrap()
        .unwrap();
    let row_text = a.settings_config["config"].as_str().unwrap();
    assert!(
        !row_text.contains("[ui]") && !row_text.contains("[model.mine]"),
        "{row_text}"
    );

    // 第二个新增的供应商不动 live 的关键字段。
    let draft = grok_row("b", "b", "");
    let view =
        ProviderService::editor_view(&state, AppType::GrokBuild, &draft.settings_config, None)
            .expect("view b");
    let edited = view.settings.clone();
    add_from_editor(&state, AppType::GrokBuild, draft, edited, view.settings).expect("add b");
    assert_eq!(grok_doc()["models"]["default"].as_str(), Some("grok-4.5"));
    assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);
}

// ---- Stack 模型（`mode::stack`） ----

fn stack_rows() -> [Provider; 3] {
    [
        claude("a", "https://a.example", json!({})),
        claude(
            "kimi",
            "https://kimi.example",
            json!({
                "ANTHROPIC_MODEL": "kimi-k3",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "128000"
            }),
        ),
        claude(
            "zhipu",
            "https://zhipu.example",
            json!({ "ANTHROPIC_MODEL": "glm-5.2[1M]" }),
        ),
    ]
}

fn stack_state() -> StackState {
    state::stack(&DeviceStore::for_device(), "claude").unwrap()
}

/// 用户自己可能打开的模型发现开关（聚合模式不再写它）。
const DISCOVERY_ENV: &str = "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY";

/// `/model` 里列的 Stack 模型 id；没有 `modelPicker` 时为 `None`。
fn picker() -> Option<Vec<String>> {
    let picker = settings().get(CLAUDE_MODEL_PICKER)?.clone();
    assert_eq!(picker["replaceBuiltInOptions"], true, "{picker}");
    Some(
        picker["options"]
            .as_array()
            .expect("options")
            .iter()
            .map(|row| row["model"].as_str().expect("model").to_string())
            .collect(),
    )
}

async fn set_member(state: &AppState, id: &str, enabled: bool) -> Vec<StackMemberView> {
    set_stack_member(state, &AppType::Claude, id, enabled)
        .await
        .unwrap_or_else(|error| panic!("set {id}={enabled}: {error:?}"));
    stack_views(state, &AppType::Claude).unwrap().members
}

#[tokio::test]
#[serial]
async fn stack_models_join_the_claude_contract_and_leave_with_it() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    enter(&state, &AppType::Claude, true).await.expect("enter");
    let only_default_bytes = settled_bytes();
    let only_default_contract = mode(&AppType::Claude).contract.unwrap();

    // 进入 Stack 模式时默认那家（a）已经在名单里，照常发布；四档都指向它的第一个模型。
    assert_eq!(stack_state().members, vec!["a"]);
    assert_eq!(
        picker(),
        Some(vec!["ccs-claude-a--claude-sonnet-4-6".to_string()])
    );
    let options = &settings()[CLAUDE_MODEL_PICKER]["options"][0];
    assert_eq!(options["label"], "claude-sonnet-4-6（A）");
    assert_eq!(options["description"], "A · claude-sonnet-4-6 · 200K");
    let env = settings()["env"].clone();
    assert!(env.get(DISCOVERY_ENV).is_none(), "{env}");
    for role in ["HAIKU", "SONNET", "OPUS", "FABLE"] {
        assert_eq!(
            env[format!("ANTHROPIC_DEFAULT_{role}_MODEL")],
            "ccs-claude-a--claude-sonnet-4-6",
            "{role}"
        );
        assert_eq!(
            env[format!("ANTHROPIC_DEFAULT_{role}_MODEL_NAME")],
            "claude-sonnet-4-6",
            "{role}"
        );
    }
    let views = set_member(&state, "kimi", true).await;
    assert_eq!(views.len(), 2);
    assert!(views[0].route);
    assert_eq!(views[0].model_ids, vec!["ccs-claude-a--claude-sonnet-4-6"]);
    assert_eq!(stack_state().key_of("kimi"), Some("kimi"));
    assert_eq!(views[1].model_ids, vec!["ccs-claude-kimi--kimi-k3"]);
    assert_eq!(
        picker(),
        Some(vec![
            "ccs-claude-a--claude-sonnet-4-6".to_string(),
            "ccs-claude-kimi--kimi-k3".to_string(),
        ])
    );
    let env = settings()["env"].clone();
    assert_eq!(env[CLAUDE_MAX_CONTEXT_ENV], "128000");
    assert_eq!(
        env["ANTHROPIC_DEFAULT_SONNET_MODEL"],
        "ccs-claude-a--claude-sonnet-4-6"
    );
    assert_eq!(
        mode(&AppType::Claude).contract.unwrap().exclusive[CLAUDE_MAX_CONTEXT_ENV],
        "128000"
    );

    // 1M 的 Stack 模型不影响 MAX。
    set_member(&state, "zhipu", true).await;
    assert_eq!(settings()["env"][CLAUDE_MAX_CONTEXT_ENV], "128000");
    set_member(&state, "kimi", false).await;
    assert_eq!(
        picker(),
        Some(vec![
            "ccs-claude-a--claude-sonnet-4-6".to_string(),
            "ccs-claude-zhipu--glm-5.2[1m]".to_string(),
        ])
    );
    let env = settings()["env"].clone();
    assert!(env.get(CLAUDE_MAX_CONTEXT_ENV).is_none(), "{env}");

    // 只剩默认那家：客户端文件和契约回到刚进入时的样子，登记簿保留。
    set_member(&state, "zhipu", false).await;
    assert_eq!(settled_bytes(), only_default_bytes);
    assert_eq!(
        mode(&AppType::Claude).contract.unwrap(),
        only_default_contract
    );
    let stack = stack_state();
    assert_eq!(stack.members, vec!["a"]);
    assert_eq!(stack.key_of("kimi"), Some("kimi"));
    assert_eq!(stack.key_of("zhipu"), Some("zhipu"));

    exit(&state, &AppType::Claude).await.expect("exit");
    assert_back_to_user_settings();
}

#[tokio::test]
#[serial]
async fn a_users_own_model_discovery_switch_survives_switches_and_proxy_mode() {
    let _home = Home::new();
    let mut user: Value = serde_json::from_str(USER_SETTINGS).unwrap();
    user["env"][DISCOVERY_ENV] = json!("1");
    seed_settings(&serde_json::to_string_pretty(&user).unwrap());
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    let discovery = || settings()["env"].get(DISCOVERY_ENV).cloned();

    ProviderService::switch(&state, AppType::Claude, "kimi").expect("direct kimi");
    assert_eq!(discovery(), Some(json!("1")));
    enter(&state, &AppType::Claude, false).await.expect("enter");
    exit(&state, &AppType::Claude).await.expect("exit");
    assert_eq!(discovery(), Some(json!("1")), "no stacked models");
}

/// `modelPicker` 是关键字段：聚合模式下换成 Stack 模型列表，用户自己配的不保留；离开
/// 聚合模式（退出代理、直连切换）都清掉。
#[tokio::test]
#[serial]
async fn the_stack_model_picker_replaces_a_users_own_and_leaves_with_stack_mode() {
    let _home = Home::new();
    let mut user: Value = serde_json::from_str(USER_SETTINGS).unwrap();
    user[CLAUDE_MODEL_PICKER] = json!({ "options": [{ "model": "mine" }] });
    seed_settings(&serde_json::to_string_pretty(&user).unwrap());
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;

    enter(&state, &AppType::Claude, true).await.expect("enter");
    assert_eq!(
        picker(),
        Some(vec!["ccs-claude-a--claude-sonnet-4-6".to_string()])
    );

    exit(&state, &AppType::Claude).await.expect("exit");
    assert!(settings().get(CLAUDE_MODEL_PICKER).is_none());
    assert_back_to_user_settings();
}

#[tokio::test]
#[serial]
async fn stack_models_leave_the_client_file_on_exit_and_come_back_on_enter() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;

    // 不在代理模式：只存名单，客户端文件不动。
    set_member(&state, "kimi", true).await;
    assert_eq!(settled_string(), USER_SETTINGS);
    assert!(stack_state().is_member("kimi"));

    enter(&state, &AppType::Claude, true).await.expect("enter");
    assert!(picker().is_some());
    assert_eq!(settings()["env"][CLAUDE_MAX_CONTEXT_ENV], "128000");

    exit(&state, &AppType::Claude).await.expect("exit");
    assert_back_to_user_settings();
    assert!(
        stack_state().is_member("kimi"),
        "the list outlives proxy mode"
    );
}

#[tokio::test]
#[serial]
async fn the_default_leads_the_aliases_and_repeating_a_target_changes_nothing() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    enter(&state, &AppType::Claude, true).await.expect("enter");
    let before = settled_bytes();

    // 路由那家已经在名单里：再加一次什么都不变。
    let views = set_member(&state, "a", true).await;
    assert_eq!(views.len(), 1);
    assert_eq!(settled_bytes(), before);

    set_member(&state, "kimi", true).await;
    let with_kimi = settled_bytes();
    set_member(&state, "kimi", true).await;
    assert_eq!(settled_bytes(), with_kimi);
    assert_eq!(stack_state().members, vec!["a", "kimi"]);

    // 换默认到名单里的 kimi：四档跟着指向 kimi 的第一个模型，两家都照常发布。
    ProviderService::switch(&state, AppType::Claude, "kimi").expect("switch route");
    assert!(picker().is_some());
    let env = settings()["env"].clone();
    assert_eq!(env[CLAUDE_MAX_CONTEXT_ENV], "128000");
    assert_eq!(
        env["ANTHROPIC_DEFAULT_OPUS_MODEL"],
        "ccs-claude-kimi--kimi-k3"
    );
    assert_eq!(env["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"], "kimi-k3");
    let views = stack_views(&state, &AppType::Claude).unwrap().members;
    let published: Vec<(&str, bool, usize)> = views
        .iter()
        .map(|view| (view.provider_id.as_str(), view.route, view.model_ids.len()))
        .collect();
    assert_eq!(published, vec![("a", false, 1), ("kimi", true, 1)]);
}

#[tokio::test]
#[serial]
async fn routing_mode_and_stack_mode_share_the_list() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    set_member(&state, "kimi", true).await;

    // 路由模式：名单在也不发布。
    enter(&state, &AppType::Claude, false)
        .await
        .expect("routing");
    assert_eq!(picker(), None);
    assert!(!stack_views(&state, &AppType::Claude).unwrap().active);

    // 已经在代理模式时换成 Stack 模式：默认那家加入名单，其余的发布。
    enter(&state, &AppType::Claude, true)
        .await
        .expect("stack mode");
    assert!(picker().is_some());
    assert!(stack_views(&state, &AppType::Claude).unwrap().active);
    assert_eq!(stack_state().members, vec!["kimi", "a"]);

    // 换回路由模式：名单留着，客户端里的 Stack 模型去掉。
    enter(&state, &AppType::Claude, false)
        .await
        .expect("routing again");
    assert_eq!(picker(), None);
    assert_eq!(stack_state().members, vec!["kimi", "a"]);
    assert!(!stack_state().enabled);

    // 退出再以 Stack 模式进入：名单原样恢复。
    exit(&state, &AppType::Claude).await.expect("exit");
    assert_back_to_user_settings();
    enter(&state, &AppType::Claude, true)
        .await
        .expect("stack mode again");
    assert!(picker().is_some());
    assert_eq!(stack_state().members, vec!["kimi", "a"]);
}

#[tokio::test]
#[serial]
async fn entering_with_a_picked_route_changes_target_and_mode_in_one_step() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    let view = || app_mode_view(&state, &AppType::Claude).expect("mode view");

    assert_eq!(view().mode, "direct");
    assert_eq!(view().direct_provider_id.as_deref(), Some("a"));

    // 直连 → 路由，路由到确认框里选的那家；直连指针不动。
    enter_with_route(&state, &AppType::Claude, false, Some("kimi"))
        .await
        .expect("route to kimi");
    assert_eq!(view().mode, "route");
    assert_eq!(view().route_provider_id.as_deref(), Some("kimi"));
    assert_eq!(view().direct_provider_id.as_deref(), Some("a"));

    // 路由 → Stack，默认换成另一家：同一把锁里一次写完，不经过直连。
    enter_with_route(&state, &AppType::Claude, true, Some("zhipu"))
        .await
        .expect("stack with zhipu");
    assert_eq!(view().mode, "stack");
    assert_eq!(view().route_provider_id.as_deref(), Some("zhipu"));
    assert!(stack_state().members.contains(&"zhipu".to_string()));

    // 回到直连：路由目标留着，下次进入沿用。
    exit(&state, &AppType::Claude).await.expect("exit");
    assert_eq!(view().mode, "direct");
    assert_eq!(view().route_provider_id.as_deref(), Some("zhipu"));
    assert_back_to_user_settings();

    // 选了不存在的供应商：什么都不改。
    enter_with_route(&state, &AppType::Claude, false, Some("missing"))
        .await
        .expect_err("unknown provider");
    assert_eq!(view().mode, "direct");
}

#[tokio::test]
#[serial]
async fn setting_the_route_in_direct_mode_only_records_it() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let mut official = claude("official", "https://api.anthropic.com", json!({}));
    official.category = Some("official".to_string());
    let [a, kimi, zhipu] = stack_rows();
    let state = state_with(AppType::Claude, &[a, kimi, zhipu, official], "a").await;
    let view = || app_mode_view(&state, &AppType::Claude).expect("mode view");

    // 直连模式：只记下指针。模式、直连指针、名单、客户端文件都不动。
    set_route(&state, &AppType::Claude, "kimi")
        .await
        .expect("remember kimi");
    assert_eq!(view().mode, "direct");
    assert_eq!(view().route_provider_id.as_deref(), Some("kimi"));
    assert_eq!(view().direct_provider_id.as_deref(), Some("a"));
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));
    assert!(stack_state().is_empty());
    assert_back_to_user_settings();

    // 不存在的、不能走代理的官方订阅：拒绝，记下的那家不变。
    set_route(&state, &AppType::Claude, "missing")
        .await
        .expect_err("unknown provider");
    set_route(&state, &AppType::Claude, "official")
        .await
        .expect_err("official subscription");
    assert_eq!(view().route_provider_id.as_deref(), Some("kimi"));

    // 进入聚合模式沿用记下的那家：它是默认，随之加入名单。
    enter(&state, &AppType::Claude, true).await.expect("enter");
    assert_eq!(view().mode, "stack");
    assert_eq!(view().route_provider_id.as_deref(), Some("kimi"));
    assert_eq!(stack_state().members, vec!["kimi"]);

    // 已经在聚合模式：当场换默认，新默认也加入名单。
    set_route(&state, &AppType::Claude, "zhipu")
        .await
        .expect("set default");
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("zhipu"));
    assert_eq!(stack_state().members, vec!["kimi", "zhipu"]);
    exit(&state, &AppType::Claude).await.expect("exit");
    assert_back_to_user_settings();
}

#[tokio::test]
#[serial]
async fn the_default_cannot_be_removed_and_a_new_default_joins_the_list() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    enter(&state, &AppType::Claude, true).await.expect("enter");

    let error = set_stack_member(&state, &AppType::Claude, "a", false)
        .await
        .expect_err("the default");
    assert!(!error.partial);
    assert_eq!(stack_state().members, vec!["a"]);

    // 设为默认（比如托盘里点）一家还没添加的：一起加入名单，原来的默认留在名单里，
    // 之后可以移除。四档指向新默认的第一个模型：1M 模型四档都带标记。
    ProviderService::switch(&state, AppType::Claude, "zhipu").expect("set default");
    assert_eq!(stack_state().members, vec!["a", "zhipu"]);
    assert!(picker().is_some());
    let env = settings()["env"].clone();
    assert_eq!(
        env["ANTHROPIC_DEFAULT_SONNET_MODEL"],
        "ccs-claude-zhipu--glm-5.2[1m]"
    );
    assert_eq!(
        env["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
        "ccs-claude-zhipu--glm-5.2[1m]"
    );
    let with_a = settings();
    set_member(&state, "a", false).await;
    assert_eq!(stack_state().members, vec!["zhipu"]);
    assert_eq!(
        picker(),
        Some(vec!["ccs-claude-zhipu--glm-5.2[1m]".to_string()])
    );
    let env = settings()["env"].clone();
    assert_eq!(env, with_a["env"], "a is not the default any more");
}

#[tokio::test]
#[serial]
async fn changing_the_main_page_switch_sends_apps_in_the_other_mode_back_to_direct() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    set_member(&state, "kimi", true).await;
    enter(&state, &AppType::Claude, true).await.expect("enter");

    // 换成显示路由开关之前，Stack 模式的应用退回直连；路由模式的不动。
    assert!(exit_apps_in_mode(&state, false).await.unwrap().is_empty());
    assert!(current::is_proxy(&AppType::Claude));
    assert_eq!(
        exit_apps_in_mode(&state, true).await.unwrap(),
        vec!["claude"]
    );
    assert!(!current::is_proxy(&AppType::Claude));
    assert_back_to_user_settings();
    assert_eq!(stack_state().members, vec!["kimi", "a"], "the list stays");
}

/// Claude Desktop 的配置目录在 Linux 跟 `XDG_CONFIG_HOME`、在 Windows 跟 `LOCALAPPDATA`：
/// 测试里一并指到临时目录，别写到真实的 Desktop 配置。
struct DesktopDirs {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl DesktopDirs {
    fn under(home: &Home) -> Self {
        let saved = ["XDG_CONFIG_HOME", "LOCALAPPDATA"]
            .into_iter()
            .map(|key| {
                let old = std::env::var_os(key);
                std::env::set_var(key, home.dir.path());
                (key, old)
            })
            .collect();
        Self { saved }
    }
}

impl Drop for DesktopDirs {
    fn drop(&mut self) {
        for (key, old) in self.saved.drain(..) {
            match old {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn desktop_card(id: &str, mapping: bool) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({ "env": {
            "ANTHROPIC_BASE_URL": format!("https://{id}.example"),
            "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}"),
        }}),
        None,
    );
    let (mode, upstream) = if mapping {
        (crate::provider::ClaudeDesktopMode::Proxy, "kimi-k3")
    } else {
        (
            crate::provider::ClaudeDesktopMode::Direct,
            "claude-sonnet-5",
        )
    };
    provider.meta = Some(crate::provider::ProviderMeta {
        claude_desktop_mode: Some(mode),
        claude_desktop_model_routes: [(
            "claude-sonnet-5".to_string(),
            crate::provider::ClaudeDesktopModelRoute {
                model: upstream.to_string(),
                label_override: None,
                supports_1m: None,
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    });
    provider
}

fn desktop_profile() -> Value {
    let path = crate::claude_desktop_config::get_config_library_path()
        .expect("desktop paths")
        .join(format!("{}.json", crate::claude_desktop_config::PROFILE_ID));
    serde_json::from_slice(&std::fs::read(path).expect("read desktop profile"))
        .expect("parse desktop profile")
}

#[tokio::test]
#[serial]
async fn claude_desktop_route_mode_writes_the_gateway_and_keeps_the_server_running() {
    let home = Home::new();
    let _dirs = DesktopDirs::under(&home);
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;
    let desktop = AppType::ClaudeDesktop;
    for card in [desktop_card("direct", false), desktop_card("map", true)] {
        state
            .db
            .save_provider(&desktop, &card)
            .expect("save desktop card");
    }
    crate::settings::set_current_provider(&desktop, Some("direct")).expect("desktop current");

    // 路由模式：Desktop 指向本地路由的 /claude-desktop，模型列表是路由那家的映射表。
    enter_with_route(&state, &desktop, false, Some("map"))
        .await
        .expect("enter desktop route");
    assert!(current::is_proxy(&desktop));
    assert!(state.proxy_service.is_running().await);
    let profile = desktop_profile();
    assert!(profile["inferenceGatewayBaseUrl"]
        .as_str()
        .is_some_and(|url| url.ends_with("/claude-desktop")));
    assert_eq!(profile["inferenceModels"], json!(["claude-sonnet-5"]));
    assert_eq!(profile["inferenceGatewayAuthScheme"], json!("bearer"));

    let result = ProviderService::switch(&state, desktop.clone(), "map").expect("same profile");
    assert_eq!(
        result.route,
        Some(RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: false,
        })
    );
    let mut changed = state
        .db
        .get_provider_by_id("map", &desktop)
        .unwrap()
        .unwrap();
    for route in changed
        .meta
        .as_mut()
        .unwrap()
        .claude_desktop_model_routes
        .values_mut()
    {
        route.label_override = Some("Updated model".to_string());
    }
    state.db.save_provider(&desktop, &changed).unwrap();
    let result = ProviderService::switch(&state, desktop.clone(), "map").expect("changed profile");
    assert_eq!(
        result.route,
        Some(RouteSwitchResult {
            kind: RouteSwitchKind::Route,
            client_config_changed: true,
        })
    );

    // Claude Code 退出路由时，Desktop 还在路由模式，服务不能跟着停。
    enter(&state, &AppType::Claude, false).await.expect("enter");
    exit(&state, &AppType::Claude).await.expect("exit");
    assert!(state.proxy_service.is_running().await);

    // 退出 CC Switch 不分离 Desktop：它只在启动时读配置。
    detach_all(&state).await;
    assert!(current::mode_state(&desktop).attached);
    assert!(desktop_profile()["inferenceGatewayBaseUrl"]
        .as_str()
        .is_some_and(|url| url.ends_with("/claude-desktop")));

    // 退回直连：写回直连那家，没人在用路由了，服务停掉。
    state.proxy_service.start().await.expect("restart");
    exit(&state, &desktop).await.expect("exit desktop");
    assert!(!current::is_proxy(&desktop));
    assert!(!state.proxy_service.is_running().await);
    assert_eq!(
        desktop_profile()["inferenceGatewayBaseUrl"],
        json!("https://direct.example")
    );
    assert_eq!(
        current::mode_state(&desktop).proxy_route.as_deref(),
        Some("map"),
        "the route is kept for next time"
    );
}

#[tokio::test]
#[serial]
async fn claude_desktop_native_direct_card_can_enter_routing_before_becoming_a_mapping_card() {
    let home = Home::new();
    let _dirs = DesktopDirs::under(&home);
    seed_settings(USER_SETTINGS);
    let desktop = AppType::ClaudeDesktop;
    let mut native = desktop_card("native", false);
    // This is the provider state that gets stuck: a native direct card with no saved catalog.
    native.meta = None;
    let state = state_with(desktop.clone(), &[native.clone()], &native.id).await;
    let before = state
        .db
        .get_provider_by_id(&native.id, &desktop)
        .expect("read native Desktop card before routing")
        .expect("native Desktop card is saved");

    check_stack_member(&desktop, &native)
        .expect_err("bootstrap defaults are not an explicit aggregation catalog");
    assert!(stack::desktop_models("native", &native).is_empty());

    enter_with_route(&state, &desktop, false, Some(&native.id))
        .await
        .expect("enter routing with the current native provider");
    assert!(current::is_proxy(&desktop));
    let profile = desktop_profile();
    assert!(profile["inferenceGatewayBaseUrl"]
        .as_str()
        .is_some_and(|url| url.ends_with("/claude-desktop")));
    let models = profile["inferenceModels"]
        .as_array()
        .expect("default native model routes");
    assert_eq!(
        profile["inferenceModels"],
        json!([
            "claude-sonnet-5",
            "claude-opus-5",
            "claude-fable-5",
            "claude-haiku-4-5"
        ])
    );
    assert!(models.iter().all(Value::is_string), "no synthetic 1M claim");

    let stored_native = state
        .db
        .get_provider_by_id(&native.id, &desktop)
        .expect("read native Desktop card")
        .expect("native Desktop card remains saved");
    assert_eq!(stored_native.settings_config, native.settings_config);
    assert_eq!(
        serde_json::to_value(&stored_native.meta).expect("metadata after routing"),
        serde_json::to_value(&before.meta).expect("metadata before routing"),
        "routing must not persist its bootstrap catalog"
    );
    let dated_native_model = "claude-sonnet-5-20250929";
    let passthrough = crate::claude_desktop_config::map_proxy_request_model(
        json!({ "model": dated_native_model, "messages": [] }),
        &stored_native,
    )
    .expect("preserve native dated model ID");
    assert_eq!(passthrough["model"], json!(dated_native_model));

    let mut mapped = stored_native;
    let meta = mapped.meta.get_or_insert_with(Default::default);
    meta.claude_desktop_mode = Some(crate::provider::ClaudeDesktopMode::Proxy);
    meta.claude_desktop_model_routes = [(
        "claude-sonnet-5".to_string(),
        crate::provider::ClaudeDesktopModelRoute {
            model: "edited-native-model".to_string(),
            label_override: None,
            supports_1m: None,
        },
    )]
    .into_iter()
    .collect();

    let update_state = state.clone();
    let update_app = desktop.clone();
    tokio::task::spawn_blocking(move || {
        crate::services::provider::ProviderService::update(&update_state, update_app, None, mapped)
    })
    .await
    .expect("join provider update")
    .expect("save mapping on the same routed provider");

    let stored_mapping = state
        .db
        .get_provider_by_id(&native.id, &desktop)
        .expect("read updated Desktop card")
        .expect("updated Desktop card remains saved");
    let profile = desktop_profile();
    assert_eq!(profile["inferenceModels"], json!(["claude-sonnet-5"]));
    let mapped_request = crate::claude_desktop_config::map_proxy_request_model(
        json!({ "model": dated_native_model, "messages": [] }),
        &stored_mapping,
    )
    .expect("resolve the new mapping");
    assert_eq!(mapped_request["model"], json!("edited-native-model"));

    state
        .proxy_service
        .stop()
        .await
        .expect("stop proxy service");
}

#[tokio::test]
#[serial]
async fn claude_desktop_stack_mode_lists_the_members_after_the_default() {
    let home = Home::new();
    let _dirs = DesktopDirs::under(&home);
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;
    let desktop = AppType::ClaudeDesktop;
    let mut other = desktop_card("other", true);
    if let Some(meta) = other.meta.as_mut() {
        for route in meta.claude_desktop_model_routes.values_mut() {
            route.model = "glm-5.2".to_string();
        }
    }
    for card in [
        desktop_card("direct", false),
        desktop_card("map", true),
        other,
    ] {
        state
            .db
            .save_provider(&desktop, &card)
            .expect("save desktop card");
    }
    crate::settings::set_current_provider(&desktop, Some("direct")).expect("desktop current");

    // 聚合模式：默认那家的映射表在前（第一项是默认模型），别家的模型用数字 id 跟在后面，
    // 关掉模型发现。
    enter_with_route(&state, &desktop, true, Some("map"))
        .await
        .expect("enter desktop stack");
    assert_eq!(app_mode_view(&state, &desktop).unwrap().mode, "stack");
    set_stack_member(&state, &desktop, "other", true)
        .await
        .expect("add other");
    let profile = desktop_profile();
    let models = profile["inferenceModels"].as_array().expect("models");
    assert_eq!(models[0], json!("claude-sonnet-5"));
    assert_eq!(models.len(), 2, "{models:?}");
    let stacked = &models[1];
    assert!(stacked["name"]
        .as_str()
        .is_some_and(|id| id.starts_with("claude-fable-ccs-")));
    assert_eq!(stacked["labelOverride"], json!("OTHER / glm-5.2"));
    assert_eq!(profile["modelDiscoveryEnabled"], json!(true));

    // 选中聚合模型的请求直达那一家。
    let resolved = stack::resolve(
        &state.db,
        &DeviceStore::for_device(),
        &desktop,
        stacked["name"].as_str().unwrap(),
    )
    .unwrap();
    match resolved {
        stack::Resolved::Hit(target) => {
            assert_eq!(target.provider.id, "other");
            assert_eq!(target.upstream_model, "glm-5.2");
        }
        other => panic!("expected a hit, got {other:?}"),
    }

    // 映射表不完整的卡（官方卡）不能加入聚合。
    assert!(set_stack_member(
        &state,
        &desktop,
        crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
        true
    )
    .await
    .is_err());

    // 换回路由模式：名单留着，模型列表只剩路由那家的；模型发现照样开着（只补说明）。
    enter_with_route(&state, &desktop, false, Some("map"))
        .await
        .expect("back to route");
    let profile = desktop_profile();
    assert_eq!(profile["inferenceModels"], json!(["claude-sonnet-5"]));
    assert_eq!(profile["modelDiscoveryEnabled"], json!(true));
    assert!(settled_stack(&desktop).unwrap().is_member("other"));
}

#[tokio::test]
#[serial]
async fn claude_desktop_on_a_mapping_card_starts_in_route_mode_after_upgrade() {
    let home = Home::new();
    let _dirs = DesktopDirs::under(&home);
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;
    let desktop = AppType::ClaudeDesktop;
    state
        .db
        .save_provider(&desktop, &desktop_card("map", true))
        .expect("save mapping card");
    crate::settings::set_current_provider(&desktop, Some("map")).expect("desktop current");

    startup(&state).await;

    let mode = current::mode_state(&desktop);
    assert!(mode.is_proxy() && mode.attached);
    assert_eq!(mode.proxy_route.as_deref(), Some("map"));
    assert!(state.proxy_service.is_running().await);
    // 映射卡直连写不进去：直连指针改成官方，退出路由后显示的就是 Desktop 实际在用的。
    let official = crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID;
    assert_eq!(
        current::direct_provider(&state.db, &desktop)
            .unwrap()
            .map(|provider| provider.id)
            .as_deref(),
        Some(official)
    );

    // 第二次启动按已定下的模式接上，不再看卡片。
    exit(&state, &desktop).await.expect("exit desktop");
    assert_eq!(in_use(&state, &desktop).as_deref(), Some(official));
    assert!(desktop_profile().get("inferenceGatewayApiKey").is_none());
    startup(&state).await;
    assert!(!current::is_proxy(&desktop));
    assert!(!state.proxy_service.is_running().await);
}

#[tokio::test]
#[serial]
async fn claude_desktop_old_mapping_card_can_be_deleted_after_routing_elsewhere() {
    let home = Home::new();
    let _dirs = DesktopDirs::under(&home);
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;
    let desktop = AppType::ClaudeDesktop;
    for card in [desktop_card("map", true), desktop_card("other", true)] {
        state
            .db
            .save_provider(&desktop, &card)
            .expect("save desktop card");
    }
    // 旧版：映射卡同时是本地指针和 DB 的 is_current。
    state
        .db
        .set_current_provider(&desktop, "map")
        .expect("db current");
    crate::settings::set_current_provider(&desktop, Some("map")).expect("desktop current");

    startup(&state).await;
    enter_with_route(&state, &desktop, false, Some("other"))
        .await
        .expect("route to other");

    // 映射卡已经不是直连指针、也不在路由上，界面上可删，后端也要放行（#8506）。
    crate::services::ProviderService::delete(&state, desktop.clone(), "map")
        .expect("delete old mapping card");
    assert!(state
        .db
        .get_provider_by_id("map", &desktop)
        .unwrap()
        .is_none());
    // 直连指针和路由那家照旧删不掉。
    for id in [
        crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
        "other",
    ] {
        assert!(
            crate::services::ProviderService::delete(&state, desktop.clone(), id).is_err(),
            "{id} should stay protected"
        );
    }
}

#[tokio::test]
#[serial]
async fn claude_desktop_upgrade_that_cannot_attach_points_desktop_back_at_official() {
    let home = Home::new();
    let _dirs = DesktopDirs::under(&home);
    seed_settings(USER_SETTINGS);
    let state = state_with(
        AppType::Claude,
        &[claude("a", "https://a.example", json!({}))],
        "a",
    )
    .await;
    let desktop = AppType::ClaudeDesktop;
    state
        .db
        .save_provider(&desktop, &desktop_card("map", true))
        .expect("save mapping card");
    crate::settings::set_current_provider(&desktop, Some("map")).expect("desktop current");
    // 旧版给映射卡写的配置：指着本地路由。
    let old = crate::claude_desktop_config::route_profile(
        &state.db,
        &desktop_card("map", true),
        "http://127.0.0.1:15721",
        &[],
    )
    .expect("old profile");
    crate::claude_desktop_config::run(
        &state.db,
        op::APPLY,
        Some(crate::claude_desktop_config::DesktopTarget::Profile(&old)),
        PendingTarget::default(),
    )
    .expect("old version writes the mapping card");
    assert!(desktop_profile().get("inferenceGatewayApiKey").is_some());

    // 端口被占，路由起不来。
    let busy = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let mut proxy = state.db.get_global_proxy_config().await.expect("busy port");
    proxy.listen_port = busy.local_addr().unwrap().port();
    state
        .db
        .update_global_proxy_config(proxy)
        .await
        .expect("busy port");

    startup(&state).await;

    let mode = current::mode_state(&desktop);
    assert!(!mode.is_proxy() && !mode.attached);
    assert!(!state.proxy_service.is_running().await);
    // Desktop 不再指着起不来的本地路由，界面上的当前供应商也是官方。
    assert!(desktop_profile().get("inferenceGatewayApiKey").is_none());
    assert_eq!(
        in_use(&state, &desktop).as_deref(),
        Some(crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID)
    );
    assert!(take_startup_attach_failures()
        .iter()
        .any(|failure| failure.app_type == desktop.as_str()));
}

#[tokio::test]
#[serial]
async fn changing_the_main_page_switch_with_nothing_to_exit_keeps_the_server() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    // 各应用都直连，服务是用户手动开的（或者 Claude Desktop 的模型映射在用）。
    state.proxy_service.start().await.expect("start");

    assert!(exit_apps_in_mode(&state, true).await.unwrap().is_empty());
    assert!(state.proxy_service.is_running().await);
    state.proxy_service.stop().await.expect("stop");
}

#[tokio::test]
#[serial]
async fn a_failover_finishing_after_entering_stack_mode_is_dropped() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    let app = AppType::Claude;
    enter(&state, &app, false).await.expect("enter route");
    let mut config = state.db.get_proxy_config_for_app(&app).await.unwrap();
    config.auto_failover_enabled = true;
    state.db.update_proxy_config_for_app(config).await.unwrap();
    let revision = state.db.route_revision(&app);
    enter(&state, &app, true).await.expect("enter Stack");
    let before = settled_bytes();

    // 进入 Stack 模式之前发出的路由请求，这时才转移到 kimi 成功。
    assert!(!record_failover_route(&state, &app, "kimi", revision)
        .await
        .expect("record failover"));
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));
    assert_eq!(stack_state().members, vec!["a"]);
    assert_eq!(settled_bytes(), before);
}

#[tokio::test]
#[serial]
async fn enabling_failover_cannot_move_the_default_in_stack_mode() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    enter(&state, &AppType::Claude, false)
        .await
        .expect("routing");
    set_member(&state, "kimi", true).await;

    // 进入 Stack 模式做到一半失败：锁外读到的还是路由模式，拿切换锁补完之后才是 Stack 模式。
    failpoint::crash_at(Some("published:0"));
    enter(&state, &AppType::Claude, true)
        .await
        .expect_err("crash");
    failpoint::crash_at(None);
    assert!(!crate::mode::stack::stack_mode_now(&AppType::Claude));

    let switch = async {
        let _guard = lock_settled(&state, &AppType::Claude)
            .await
            .map_err(String::from)?;
        switch_route_for_failover_locked(&state, &AppType::Claude, "kimi").await
    };
    switch.await.expect_err("stack mode");
    assert!(crate::mode::stack::stack_mode_now(&AppType::Claude));
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));

    // 路由模式照常切到 P1。
    enter(&state, &AppType::Claude, false)
        .await
        .expect("routing");
    let _guard = lock_settled(&state, &AppType::Claude).await.unwrap();
    switch_route_for_failover_locked(&state, &AppType::Claude, "kimi")
        .await
        .expect("switch");
    assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("kimi"));
    state.proxy_service.stop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn stack_mode_sends_default_requests_to_the_default_only() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    // 故障转移开着，队列里还有别家。
    let mut config = state
        .db
        .get_proxy_config_for_app(&AppType::Claude)
        .await
        .unwrap();
    config.auto_failover_enabled = true;
    state.db.update_proxy_config_for_app(config).await.unwrap();
    for id in ["a", "kimi"] {
        state
            .db
            .add_to_failover_queue(&AppType::Claude, id)
            .unwrap();
    }
    let proxy = crate::proxy::server::ProxyState::for_test(state.db.clone());
    let body = json!({ "model": "claude-sonnet-5", "messages": [] });
    let headers = axum::http::HeaderMap::new();
    let context = || {
        crate::proxy::handler_context::RequestContext::new(
            &proxy,
            &body,
            &headers,
            AppType::Claude,
            "Claude",
            "claude",
            None,
        )
    };
    let chain = |ctx: &crate::proxy::handler_context::RequestContext| {
        ctx.get_providers()
            .iter()
            .map(|provider| provider.id.clone())
            .collect::<Vec<_>>()
    };

    enter(&state, &AppType::Claude, false)
        .await
        .expect("routing");
    let ctx = context().await.expect("routing context");
    assert_eq!(chain(&ctx), vec!["a", "kimi"]);
    assert!(ctx.app_config.auto_failover_enabled);

    enter(&state, &AppType::Claude, true)
        .await
        .expect("stack mode");
    let ctx = context().await.expect("stack mode context");
    assert_eq!(chain(&ctx), vec!["a"]);
    assert!(!ctx.app_config.auto_failover_enabled);
    // 队列留着，回到路由模式恢复。
    assert_eq!(
        state.db.get_failover_queue(&AppType::Claude).unwrap().len(),
        2
    );
}

#[tokio::test]
#[serial]
async fn official_accounts_cannot_be_stacked() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let mut official = claude("official", "https://api.anthropic.com", json!({}));
    official.category = Some("official".to_string());
    let rows = [claude("a", "https://a.example", json!({})), official];
    let state = state_with(AppType::Claude, &rows, "a").await;
    let error = set_stack_member(&state, &AppType::Claude, "official", true)
        .await
        .expect_err("official");
    assert!(!error.partial);
    assert!(stack_state().is_empty());
    let error = set_stack_member(&state, &AppType::Claude, "missing", true)
        .await
        .expect_err("missing");
    assert!(!error.partial);
}

#[tokio::test]
#[serial]
async fn a_failed_stack_change_says_whether_it_will_be_finished() {
    for (point, partial) in [
        ("pending", false),
        ("marked", true),
        ("published:0", true),
        ("target", true),
    ] {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        enter(&state, &AppType::Claude, true).await.expect("enter");

        failpoint::crash_at(Some(point));
        let error = set_stack_member(&state, &AppType::Claude, "kimi", true)
            .await
            .expect_err(point);
        failpoint::crash_at(None);
        assert_eq!(error.partial, partial, "{point}: {error:?}");
        // 状态落定之前失败，已落定的名单还是旧的。
        assert_eq!(
            stack_state().is_member("kimi"),
            point == "target",
            "{point}"
        );

        // 再发一次同样的目标值：先补完（或丢弃）上一次，再应用，结果都是新名单。
        set_member(&state, "kimi", true).await;
        assert!(stack_state().is_member("kimi"), "{point}");
        assert!(picker().is_some(), "{point}");
        assert!(state::pending(&DeviceStore::for_device(), "claude")
            .unwrap()
            .is_none());
        state.proxy_service.stop().await.unwrap();
    }
}

#[tokio::test]
#[serial]
async fn operations_that_do_not_touch_the_list_keep_it() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    set_member(&state, "kimi", true).await;
    let stack = stack_state();

    // 一个不带 `stack` 的 pending（旧版本写的、或者换路由这类操作）前滚时不清名单。
    commit_state(
        &state,
        &AppType::Claude,
        &PendingTarget::mode(mode(&AppType::Claude)),
    )
    .unwrap();
    enter(&state, &AppType::Claude, false).await.expect("enter");
    exit(&state, &AppType::Claude).await.expect("exit");
    assert_eq!(stack_state(), stack);
}

#[tokio::test]
#[serial]
async fn editing_or_deleting_an_stacked_provider_rewrites_the_contract() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    enter(&state, &AppType::Claude, true).await.expect("enter");
    set_member(&state, "kimi", true).await;

    let mut kimi = state
        .db
        .get_provider_by_id("kimi", &AppType::Claude)
        .unwrap()
        .unwrap();
    kimi.settings_config["env"][CLAUDE_MAX_CONTEXT_ENV] = json!("64000");
    ProviderService::update(&state, AppType::Claude, None, kimi).expect("update kimi");
    assert_eq!(settings()["env"][CLAUDE_MAX_CONTEXT_ENV], "64000");

    ProviderService::delete(&state, AppType::Claude, "kimi").expect("delete kimi");
    let env = settings()["env"].clone();
    assert_eq!(
        picker(),
        Some(vec!["ccs-claude-a--claude-sonnet-4-6".to_string()]),
        "a still publishes"
    );
    assert!(env.get(CLAUDE_MAX_CONTEXT_ENV).is_none(), "{env}");
    assert!(state
        .db
        .get_provider_by_id("kimi", &AppType::Claude)
        .unwrap()
        .is_none());
    let stack = stack_state();
    assert_eq!(stack.members, vec!["a"]);
    assert_eq!(stack.key_of("kimi"), Some("kimi"), "the key stays taken");
}

#[tokio::test]
#[serial]
async fn the_first_model_of_the_defaults_list_leads_the_aliases() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    enter(&state, &AppType::Claude, true).await.expect("enter");
    let mapped_contract = mode(&AppType::Claude).contract.unwrap();
    let sonnet = || settings()["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"].clone();
    assert_eq!(sonnet(), "ccs-claude-a--claude-sonnet-4-6");

    // 配了列表：整张照常发布，第一个占四档。
    let mut a = state
        .db
        .get_provider_by_id("a", &AppType::Claude)
        .unwrap()
        .unwrap();
    a.meta.get_or_insert_with(Default::default).stack_models = serde_json::from_value(
        json!([{ "model": "a-vision", "displayName": "A Vision" }, { "model": "claude-sonnet-4-6" }]),
    )
    .unwrap();
    ProviderService::update(&state, AppType::Claude, None, a.clone()).expect("update a");
    let views = stack_views(&state, &AppType::Claude).unwrap().members;
    assert!(views[0].route);
    assert_eq!(
        views[0].model_ids,
        vec!["ccs-claude-a--a-vision", "ccs-claude-a--claude-sonnet-4-6"]
    );
    assert_eq!(sonnet(), "ccs-claude-a--a-vision");
    assert_eq!(
        settings()["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME"],
        "A Vision"
    );

    // 调整顺序就换了默认模型。
    a.meta
        .as_mut()
        .unwrap()
        .stack_models
        .as_mut()
        .unwrap()
        .reverse();
    ProviderService::update(&state, AppType::Claude, None, a.clone()).expect("reorder a");
    assert_eq!(sonnet(), "ccs-claude-a--claude-sonnet-4-6");

    // 用户清空了列表：什么都不发布，四档回到路由契约的写法。
    a.meta.as_mut().unwrap().stack_models = Some(Vec::new());
    ProviderService::update(&state, AppType::Claude, None, a.clone()).expect("clear a");
    let views = stack_views(&state, &AppType::Claude).unwrap().members;
    assert!(views[0].model_ids.is_empty());
    assert!(!sonnet().as_str().unwrap().starts_with("ccs-claude-"));

    // 没配列表：回到按映射发布，第一个是 `ANTHROPIC_MODEL`。
    a.meta.as_mut().unwrap().stack_models = None;
    ProviderService::update(&state, AppType::Claude, None, a).expect("unset a");
    assert_eq!(mode(&AppType::Claude).contract.unwrap(), mapped_contract);
}

#[tokio::test]
#[serial]
async fn routing_mode_keeps_the_claude_aliases() {
    let _home = Home::new();
    seed_settings(USER_SETTINGS);
    let state = state_with(AppType::Claude, &stack_rows(), "a").await;
    set_member(&state, "kimi", true).await;
    enter(&state, &AppType::Claude, false)
        .await
        .expect("routing");
    let env = settings()["env"].clone();
    assert_eq!(env["ANTHROPIC_DEFAULT_SONNET_MODEL"], "claude-sonnet-5");
    assert_eq!(env["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "claude-haiku-5-5");
    // `/model` 的说明写在四档变量里，不写菜单。
    assert_eq!(
        env["ANTHROPIC_DEFAULT_SONNET_MODEL_DESCRIPTION"],
        "A · claude-sonnet-4-6"
    );
    assert_eq!(picker(), None);
}

fn codex_native(id: &str, url: &str, extra: &str, catalog: Option<Value>) -> Provider {
    let mut provider = codex_row(id, url, extra);
    if let Some(catalog) = catalog {
        provider.settings_config["modelCatalog"] = catalog;
    }
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        ..Default::default()
    });
    provider
}

fn codex_stack_rows() -> [Provider; 3] {
    [
        codex_native(
            "a",
            "https://a.example/v1",
            "model_context_window = 200000\nmodel_auto_compact_token_limit = 150000\n",
            None,
        ),
        codex_native(
            "deepseek",
            "https://api.deepseek.com/v1",
            "",
            Some(json!({ "models": [
                { "model": "deepseek-v4-pro", "displayName": "DeepSeek V4 Pro" }
            ]})),
        ),
        codex_native("zhipu", "https://open.bigmodel.cn/api/v1", "", None),
    ]
}

async fn set_codex_member(state: &AppState, id: &str, enabled: bool) -> Vec<StackMemberView> {
    set_stack_member(state, &AppType::Codex, id, enabled)
        .await
        .unwrap_or_else(|error| panic!("set {id}={enabled}: {error:?}"));
    stack_views(state, &AppType::Codex).unwrap().members
}

fn codex_catalog() -> Value {
    crate::config::read_json_file(&crate::codex_config::get_codex_model_catalog_path()).unwrap()
}

async fn check_codex_proxy_compaction_preference(name: &str, capability: Option<&str>) {
    for stack in [false, true] {
        let _home = Home::new();
        seed_codex("[features]\ngoals = true\n", None);
        let mut relay = codex_native(
            "relay",
            "https://relay.example/v1",
            "model_context_window = 372000\nmodel_auto_compact_token_limit = 353400\n",
            Some(json!({"models": [{"model": "gpt-relay", "displayName": "Relay"}]})),
        );
        let mut config = relay.settings_config["config"]
            .as_str()
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let provider = &mut config["model_providers"]["relay"];
        provider["name"] = toml_edit::value(name);
        provider["http_headers"]["X-Upstream-Only"] = toml_edit::value("private");
        provider["capabilities"]["external_web_access"] = toml_edit::value(false);
        provider["stream_max_retries"] = toml_edit::value(50);
        provider["stream_idle_timeout_ms"] = toml_edit::value(600000);
        if let Some(value) = capability {
            provider["capabilities"]["remote_compaction"] = toml_edit::value(value);
        }
        relay.settings_config["config"] = json!(config.to_string());
        let state = state_with(AppType::Codex, &[relay.clone()], "relay").await;
        enter(&state, &AppType::Codex, stack)
            .await
            .expect("enter proxy");
        let check = |published_stack: bool| {
            let doc = codex_doc();
            assert_eq!(doc["model_provider"].as_str(), Some("custom"));
            let proxy = &doc["model_providers"]["custom"];
            assert_eq!(
                proxy["name"].as_str(),
                Some(if name == "OpenAI" { "OpenAI" } else { "custom" })
            );
            assert_eq!(
                proxy
                    .get("capabilities")
                    .and_then(|v| v.get("remote_compaction"))
                    .and_then(|v| v.as_str()),
                capability
            );
            assert!(proxy["base_url"]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:"));
            assert_ne!(
                proxy["experimental_bearer_token"].as_str(),
                Some("sk-relay")
            );
            assert!(proxy.get("http_headers").is_none());
            assert!(proxy
                .get("capabilities")
                .and_then(|v| v.get("external_web_access"))
                .is_none());
            // Codex 自己的重试和流空闲超时跟着路由那家走（#8265）。
            assert_eq!(proxy["stream_max_retries"].as_integer(), Some(50));
            assert_eq!(proxy["stream_idle_timeout_ms"].as_integer(), Some(600000));
            if published_stack {
                // Stack keeps each provider's window on its own catalog row.
                assert!(doc.get("model_context_window").is_none());
                assert!(doc.get("model_auto_compact_token_limit").is_none());
                let catalog = codex_catalog();
                let route = catalog["models"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|model| model["slug"] == "gpt-relay")
                    .unwrap();
                assert_eq!(route["context_window"], 372000);
                assert_eq!(route["auto_compact_token_limit"], 353400);
            } else {
                assert_eq!(doc["model_context_window"].as_integer(), Some(372000));
                assert_eq!(
                    doc["model_auto_compact_token_limit"].as_integer(),
                    Some(353400)
                );
            }
            assert_eq!(doc["features"]["goals"].as_bool(), Some(true));
        };
        check(false);
        if stack {
            let member = codex_native(
                "member",
                "https://member.example/v1",
                "",
                Some(json!({"models": [{"model": "gpt-member", "displayName": "Member"}]})),
            );
            state.db.save_provider(&AppType::Codex, &member).unwrap();
            set_codex_member(&state, "member", true).await;
            check(true);
        }
        let catalog = codex_catalog();
        ProviderService::update(&state, AppType::Codex, None, relay.clone()).unwrap();
        check(stack);
        assert_eq!(codex_catalog(), catalog);
        detach_all(&state).await;
        startup(&state).await;
        check(stack);
        assert_eq!(codex_catalog(), catalog);

        // Switching off must remove a previously projected capability rather than
        // retaining the outgoing provider's opt-in in the shared custom table.
        let disabled = codex_native("disabled", "https://disabled.example/v1", "", None);
        state.db.save_provider(&AppType::Codex, &disabled).unwrap();
        set_route(&state, &AppType::Codex, "disabled")
            .await
            .unwrap();
        let doc = codex_doc();
        let proxy = &doc["model_providers"]["custom"];
        assert_eq!(proxy["name"].as_str(), Some("custom"));
        assert!(proxy.get("capabilities").is_none());
        assert!(proxy.get("stream_max_retries").is_none());
        assert!(proxy.get("stream_idle_timeout_ms").is_none());
        exit(&state, &AppType::Codex).await.unwrap();
    }
}

#[tokio::test]
#[serial]
async fn codex_proxy_preserves_legacy_remote_compaction_toggle() {
    check_codex_proxy_compaction_preference("OpenAI", None).await;
    check_codex_proxy_compaction_preference("Relay", None).await;
}

#[tokio::test]
#[serial]
async fn codex_proxy_preserves_explicit_remote_compaction_capability() {
    check_codex_proxy_compaction_preference("Relay", Some("v2")).await;
    check_codex_proxy_compaction_preference("OpenAI", Some("unsupported")).await;
}

#[tokio::test]
#[serial]
async fn codex_stack_models_join_the_catalog_and_leave_with_it() {
    let _home = Home::new();
    seed_codex("approval_policy = \"on-request\"\n", None);
    let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    let plain_text = codex_text();
    let plain_contract = mode(&AppType::Codex).contract.unwrap();
    assert_eq!(
        codex_doc()["model_context_window"].as_integer(),
        Some(200000)
    );

    // 默认那家（a）在名单最前面，不发布。
    let views = set_codex_member(&state, "deepseek", true).await;
    assert!(views[0].route);
    assert_eq!(views[1].model_ids, vec!["ccs-deepseek/deepseek-v4-pro"]);
    let doc = codex_doc();
    assert_eq!(
        doc["model"].as_str(),
        Some("gpt-a"),
        "the route keeps the default model"
    );
    assert_eq!(
        doc["model_catalog_json"].as_str(),
        Some(ours_catalog_pointer().as_str())
    );
    // 窗口类全局键会覆盖每一行，改写进路由那家自己的行。
    assert!(doc.get("model_context_window").is_none(), "{doc:?}");
    assert!(
        doc.get("model_auto_compact_token_limit").is_none(),
        "{doc:?}"
    );

    let catalog = codex_catalog();
    let models = catalog["models"].as_array().unwrap();
    let slugs: Vec<&str> = models.iter().map(|m| m["slug"].as_str().unwrap()).collect();
    assert_eq!(slugs, vec!["gpt-a", "ccs-deepseek/deepseek-v4-pro"]);
    let (route, stacked) = (&models[0], &models[1]);
    assert_eq!(route["priority"], 1);
    assert_eq!(route["context_window"], 200000);
    assert_eq!(route["auto_compact_token_limit"], 150000);
    // 第三方路由那家的行和 Stack 行同一个值，互相切换不会先用上一家压缩（#8485）。
    assert_eq!(route["comp_hash"], "cc-switch");
    assert_eq!(stacked["priority"], 2);
    assert_eq!(stacked["display_name"], "DeepSeek V4 Pro（DEEPSEEK）");
    // DeepSeek 官方目录的 "3000" 不带过来，窗口按它自己的行算。
    assert_eq!(stacked["comp_hash"], "cc-switch");
    let window = stacked["context_window"].as_u64().unwrap();
    assert_eq!(
        stacked["auto_compact_token_limit"].as_u64(),
        Some(window * 9 / 10)
    );

    // 没有配置模型目录的行只发布它的 `model`。
    let views = set_codex_member(&state, "zhipu", true).await;
    assert_eq!(views[2].model_ids, vec!["ccs-zhipu/gpt-zhipu"]);
    let slugs: Vec<String> = codex_catalog()["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["slug"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        slugs,
        vec![
            "gpt-a",
            "ccs-deepseek/deepseek-v4-pro",
            "ccs-zhipu/gpt-zhipu"
        ]
    );

    // 只剩默认那家：config.toml 和契约回到没有 Stack 模型时的样子。
    set_codex_member(&state, "deepseek", false).await;
    set_codex_member(&state, "zhipu", false).await;
    assert_eq!(codex_text(), plain_text);
    assert_eq!(mode(&AppType::Codex).contract.unwrap(), plain_contract);

    exit(&state, &AppType::Codex).await.expect("exit");
    assert!(!state
        .proxy_service
        .live_has_proxy_placeholder(&AppType::Codex));
}

#[tokio::test]
#[serial]
async fn codex_custom_context_windows_survive_updates_and_restart() {
    let _home = Home::new();
    seed_codex("", None);
    let mut relay = codex_native(
        "relay",
        "https://relay.example/v1",
        "",
        Some(json!({ "models": [
            { "model": "gpt-5.5", "contextWindow": 1_050_000 },
            { "model": "gpt-5.5-high", "contextWindow": 512_000 }
        ] })),
    );
    let gemini = codex_native(
        "gemini",
        "https://gemini.example/v1",
        "",
        Some(json!({ "models": [
            { "model": "gemini-3.8-flash-high", "contextWindow": 272_000 }
        ] })),
    );
    let state = state_with(AppType::Codex, &[relay.clone(), gemini], "relay").await;
    enter(&state, &AppType::Codex, true).await.unwrap();
    let check = |gpt_window: u64, stacked: bool| {
        let catalog = codex_catalog();
        let models = catalog["models"].as_array().unwrap();
        for (slug, window) in [("gpt-5.5", gpt_window), ("gpt-5.5-high", 512_000)] {
            let model = models.iter().find(|model| model["slug"] == slug).unwrap();
            assert_eq!(model["context_window"], window);
            assert_eq!(model["max_context_window"], window);
            assert_eq!(model["auto_compact_token_limit"], window * 9 / 10);
        }
        if stacked {
            let gemini = models
                .iter()
                .find(|model| model["slug"] == "ccs-gemini/gemini-3.8-flash-high")
                .unwrap();
            assert_eq!(gemini["context_window"], 272_000);
            assert_eq!(gemini["auto_compact_token_limit"], 244_800);
        }
    };
    check(1_050_000, false);
    set_codex_member(&state, "gemini", true).await;
    check(1_050_000, true);
    relay.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(750_000);
    ProviderService::update(&state, AppType::Codex, None, relay).unwrap();
    check(750_000, true);
    let catalog = codex_catalog();
    detach_all(&state).await;
    startup(&state).await;
    check(750_000, true);
    assert_eq!(codex_catalog(), catalog);
    set_codex_member(&state, "gemini", false).await;
    check(750_000, false);
    exit(&state, &AppType::Codex).await.unwrap();
}

/// 子代理模型：路由模式照写路由那家的；Stack 模式不写（子代理跟随主模型），只删这两个
/// 键，`[agents]` 里用户自己的设置不动；Stack 模型都移除后恢复。
#[tokio::test]
#[serial]
async fn codex_stack_mode_leaves_the_subagent_model_to_the_main_model() {
    let _home = Home::new();
    seed_codex("[agents]\nmax_threads = 4\n", None);
    let [_, deepseek, zhipu] = codex_stack_rows();
    let route = codex_native(
        "a",
        "https://a.example/v1",
        "[agents]\ndefault_subagent_model = \"gpt-a-mini\"\ndefault_subagent_reasoning_effort = \"low\"\n",
        None,
    );
    let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    let agents = |doc: &toml::Table| {
        let agents = &doc["agents"];
        let text = |key: &str| agents.get(key).and_then(|v| v.as_str()).map(str::to_string);
        (
            text("default_subagent_model"),
            text("default_subagent_reasoning_effort"),
            agents.get("max_threads").and_then(|v| v.as_integer()),
        )
    };
    let routed = (
        Some("gpt-a-mini".to_string()),
        Some("low".to_string()),
        Some(4),
    );
    assert_eq!(agents(&codex_doc()), routed);

    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(agents(&codex_doc()), (None, None, Some(4)));

    set_codex_member(&state, "deepseek", false).await;
    assert_eq!(agents(&codex_doc()), routed);

    exit(&state, &AppType::Codex).await.expect("exit");
    assert_eq!(agents(&codex_doc()), routed);
}

fn set_stack_subagent(model: Option<&str>, effort: Option<&str>) {
    crate::settings::update_settings(crate::settings::AppSettings {
        codex_stack_subagent_model: model.map(str::to_string),
        codex_stack_subagent_reasoning_effort: effort.map(str::to_string),
        ..crate::settings::get_settings()
    })
    .unwrap();
}

/// 聚合面板的子代理模型和强度：Stack 模式下按合并目录校验后写进 `[agents]`（路由那家行里
/// 的照旧不写，`[agents]` 里用户自己的键不动）；强度不在档位里只写模型；模型不在目录里
/// （成员移除）就跟随主模型，加回来自动恢复；退出代理回到路由那家行里的。
#[tokio::test]
#[serial]
async fn codex_stack_subagent_setting_follows_the_merged_catalog() {
    let _home = Home::new();
    seed_codex("[agents]\nmax_threads = 4\n", None);
    let [_, deepseek, zhipu] = codex_stack_rows();
    let route = codex_native(
        "a",
        "https://a.example/v1",
        "[agents]\ndefault_subagent_model = \"gpt-a-mini\"\ndefault_subagent_reasoning_effort = \"low\"\n",
        None,
    );
    let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;
    let agents = |doc: &toml::Table| {
        let agents = &doc["agents"];
        let text = |key: &str| agents.get(key).and_then(|v| v.as_str()).map(str::to_string);
        (
            text("default_subagent_model"),
            text("default_subagent_reasoning_effort"),
            agents.get("max_threads").and_then(|v| v.as_integer()),
        )
    };
    let routed = (
        Some("gpt-a-mini".to_string()),
        Some("low".to_string()),
        Some(4),
    );

    // 还没进 Stack：设置只存着，不碰客户端文件。
    set_stack_subagent(Some("ccs-deepseek/deepseek-v4-pro"), None);
    let before = codex_text();
    resync_codex_stack_catalog(&state).await.expect("no-op");
    assert_eq!(codex_text(), before);
    set_stack_subagent(None, None);

    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    set_codex_member(&state, "zhipu", true).await;
    assert_eq!(agents(&codex_doc()), (None, None, Some(4)));

    // 面板的选项：名单里每一家的模型都列上，带上合并目录里的档位。路由那家（a）也在：
    // 切换到聚合的确认框里还能换默认那家，由前端按默认那家去掉。
    let all = stack_views(&state, &AppType::Codex)
        .unwrap()
        .subagent_models;
    let ids: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "ccs-a/gpt-a",
            "ccs-deepseek/deepseek-v4-pro",
            "ccs-zhipu/gpt-zhipu"
        ]
    );
    assert_eq!(all[0].provider_id, "a");
    let choices: Vec<_> = all.into_iter().filter(|c| c.provider_id != "a").collect();
    // 默认那家的 `ccs-*` 不在合并目录里：选了也不写，跟随主模型。
    set_stack_subagent(Some("ccs-a/gpt-a"), None);
    resync_codex_stack_catalog(&state)
        .await
        .expect("route model");
    assert_eq!(agents(&codex_doc()), (None, None, Some(4)));
    set_stack_subagent(None, None);
    let choice = &choices[0];
    assert_eq!(choice.provider_id, "deepseek");
    assert_eq!(choice.display_name, "DeepSeek V4 Pro（DEEPSEEK）");
    let catalog_efforts: Vec<String> = codex_catalog()["models"][1]["supported_reasoning_levels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|level| level["effort"].as_str().unwrap().to_string())
        .collect();
    assert!(!catalog_efforts.is_empty());
    assert_eq!(choice.efforts, catalog_efforts);
    let effort = choice.efforts.last().unwrap().clone();
    let id = choice.id.clone();

    set_stack_subagent(Some(&id), Some(&effort));
    resync_codex_stack_catalog(&state).await.expect("set");
    assert_eq!(
        agents(&codex_doc()),
        (Some(id.clone()), Some(effort.clone()), Some(4))
    );

    set_stack_subagent(Some(&id), Some("not-a-level"));
    resync_codex_stack_catalog(&state)
        .await
        .expect("bad effort");
    assert_eq!(agents(&codex_doc()), (Some(id.clone()), None, Some(4)));

    set_stack_subagent(Some(&id), Some(&effort));
    resync_codex_stack_catalog(&state).await.expect("set again");
    set_codex_member(&state, "deepseek", false).await;
    assert_eq!(agents(&codex_doc()), (None, None, Some(4)));
    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(
        agents(&codex_doc()),
        (Some(id.clone()), Some(effort.clone()), Some(4))
    );

    exit(&state, &AppType::Codex).await.expect("exit");
    assert_eq!(agents(&codex_doc()), routed);
}

fn set_stack_auto_review(model: Option<&str>) {
    crate::settings::update_settings(crate::settings::AppSettings {
        codex_stack_auto_review_model: model.map(str::to_string),
        ..crate::settings::get_settings()
    })
    .unwrap();
}

/// 合并目录每一行的 `auto_review_model_override`（没写或写 null 都是 `None`）。
fn catalog_auto_review() -> Vec<Option<String>> {
    codex_catalog()["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| {
            model
                .get("auto_review_model_override")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

/// 聚合设置里的自动审批模型：Stack 模式下按合并目录校验后写进每一行（主模型可以是任何
/// 一行）；不在目录里（成员移除、默认那家的 `ccs-*`）就不写，加回来自动恢复。路由那家
/// 卡上选的审批模型在聚合下不用：它的行照原样进合并目录。
#[tokio::test]
#[serial]
async fn codex_stack_auto_review_setting_follows_the_merged_catalog() {
    let _home = Home::new();
    seed_codex("approval_policy = \"on-request\"\n", None);
    let [_, deepseek, zhipu] = codex_stack_rows();
    let route = codex_native(
        "a",
        "https://a.example/v1",
        "",
        Some(json!({
            "models": [{ "model": "gpt-a" }, { "model": "gpt-a-mini" }],
            "autoReviewModel": "gpt-a-mini"
        })),
    );
    let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;

    // 还没进 Stack：设置只存着，不碰客户端文件。
    set_stack_auto_review(Some("ccs-deepseek/deepseek-v4-pro"));
    let before = codex_text();
    resync_codex_stack_catalog(&state).await.expect("no-op");
    assert_eq!(codex_text(), before);
    set_stack_auto_review(None);

    // 进代理、名单为空：目录就是路由那家卡上的，卡上选的审批模型照写。
    enter(&state, &AppType::Codex, true).await.expect("enter");
    assert_eq!(
        catalog_auto_review(),
        vec![Some("gpt-a-mini".to_string()); 2]
    );

    // 名单非空：合并目录里路由那家的行不带卡上的值，没设聚合审批模型时一行都不写。
    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(catalog_auto_review(), vec![None; 3]);

    let id = "ccs-deepseek/deepseek-v4-pro";
    set_stack_auto_review(Some(id));
    resync_codex_stack_catalog(&state).await.expect("set");
    assert_eq!(catalog_auto_review(), vec![Some(id.to_string()); 3]);

    // 默认那家的 `ccs-*` 不在合并目录里：不写。
    set_stack_auto_review(Some("ccs-a/gpt-a"));
    resync_codex_stack_catalog(&state).await.expect("route id");
    assert_eq!(catalog_auto_review(), vec![None; 3]);

    // 成员移除后不写，加回来自动恢复。
    set_stack_auto_review(Some(id));
    resync_codex_stack_catalog(&state).await.expect("set again");
    set_codex_member(&state, "zhipu", true).await;
    set_codex_member(&state, "deepseek", false).await;
    assert!(catalog_auto_review().iter().all(Option::is_none));
    set_codex_member(&state, "deepseek", true).await;
    let restored = catalog_auto_review();
    assert!(!restored.is_empty());
    assert!(restored.iter().all(|value| value.as_deref() == Some(id)));

    set_stack_auto_review(None);
    resync_codex_stack_catalog(&state).await.expect("unset");
    assert!(catalog_auto_review().iter().all(Option::is_none));
}

fn set_classic_subagents(enabled: bool) {
    crate::settings::update_settings(crate::settings::AppSettings {
        codex_stack_classic_tools: enabled,
        ..crate::settings::get_settings()
    })
    .unwrap();
}

fn catalog_agent_versions() -> Vec<Value> {
    codex_catalog()["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["multi_agent_version"].clone())
        .collect()
}

/// 「经典子 agent」开关：Stack 模式下立刻按开关重写合并目录，关掉后回到原来的目录；
/// 不在 Stack 模式时什么都不写。
#[tokio::test]
#[serial]
async fn classic_subagents_toggle_rewrites_the_stack_catalog() {
    let _home = Home::new();
    seed_codex("approval_policy = \"on-request\"\n", None);
    let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;

    // 还没进 Stack：开关只存设置，不碰客户端文件。
    set_classic_subagents(true);
    let before = codex_text();
    resync_codex_stack_catalog(&state).await.expect("no-op");
    assert_eq!(codex_text(), before);
    set_classic_subagents(false);

    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    let native_catalog = codex_catalog();
    let native_contract = mode(&AppType::Codex).contract.unwrap();
    assert!(catalog_agent_versions().iter().all(|v| v != "v1"));

    set_classic_subagents(true);
    resync_codex_stack_catalog(&state).await.expect("classic");
    assert_eq!(catalog_agent_versions(), vec![json!("v1"), json!("v1")]);
    assert_ne!(mode(&AppType::Codex).contract.unwrap(), native_contract);

    // 之后增删 Stack 模型照样按开关写。
    set_codex_member(&state, "zhipu", true).await;
    assert_eq!(
        catalog_agent_versions(),
        vec![json!("v1"), json!("v1"), json!("v1")]
    );
    set_codex_member(&state, "zhipu", false).await;

    set_classic_subagents(false);
    resync_codex_stack_catalog(&state).await.expect("native");
    assert_eq!(codex_catalog(), native_catalog);
    assert_eq!(mode(&AppType::Codex).contract.unwrap(), native_contract);

    exit(&state, &AppType::Codex).await.expect("exit");
}

/// 路由那家自己管理模型目录文件：Stack 模型发布不了，名单照存，结果带提示而不是静默成功。
#[tokio::test]
#[serial]
async fn codex_route_with_its_own_catalog_reports_stack_models_unpublished() {
    let _home = Home::new();
    seed_codex("", None);
    let [_, deepseek, zhipu] = codex_stack_rows();
    let route = codex_native(
        "a",
        "https://a.example/v1",
        "model_catalog_json = \"/opt/team/models.json\"\n",
        None,
    );
    let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    let plain_text = codex_text();

    let notice = set_stack_member(&state, &AppType::Codex, "deepseek", true)
        .await
        .expect("stack");
    assert_eq!(notice, Some("routeOwnsCatalog"));
    assert!(stack_state_of(&AppType::Codex).is_member("deepseek"));
    assert_eq!(codex_text(), plain_text, "nothing to publish");
    assert_eq!(
        stack_views(&state, &AppType::Codex).unwrap().notice,
        Some("routeOwnsCatalog")
    );

    // 名单清空后不再提示。
    let notice = set_stack_member(&state, &AppType::Codex, "deepseek", false)
        .await
        .expect("detach");
    assert_eq!(notice, None);
}

/// 假的进程表和时钟（见 `codex_client_catalog::Env`），结束时换回真的。
struct FakeClients {
    now_ms: Arc<std::sync::atomic::AtomicU64>,
    table: Arc<std::sync::Mutex<String>>,
}

impl FakeClients {
    const START_MS: u64 = 1_800_000_000_000;

    fn install() -> Self {
        let now_ms = Arc::new(std::sync::atomic::AtomicU64::new(Self::START_MS));
        let table = Arc::new(std::sync::Mutex::new(String::new()));
        let (clock, rows) = (now_ms.clone(), table.clone());
        codex_client_catalog::set_test_env(codex_client_catalog::Env {
            process_table: Box::new(move || Some(rows.lock().unwrap().clone())),
            daemon_started: Box::new(|| None),
            now_ms: Box::new(move || clock.load(std::sync::atomic::Ordering::SeqCst)),
            restart: Box::new(|_| Err("not in tests".to_string())),
        });
        Self { now_ms, table }
    }

    /// 桌面版的 app-server，已经跑了 `etime`（`ps` 的格式）。
    fn desktop_running_for(&self, etime: &str) {
        *self.table.lock().unwrap() = format!(
            "62347 {etime} /Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex -c features.code_mode_host=true app-server"
        );
    }

    fn advance(&self, ms: u64) {
        self.now_ms
            .fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for FakeClients {
    fn drop(&mut self) {
        codex_client_catalog::reset_test_env();
    }
}

async fn stale_clients_of(state: &AppState) -> Option<codex_client_catalog::StaleClients> {
    stack_view_with_clients(state, &AppType::Codex)
        .await
        .expect("stack view")
        .stale_clients
}

/// 直连切号也提示可能缓存旧登录，不要求生成模型目录。
#[tokio::test]
#[serial]
async fn codex_direct_account_switch_reports_the_cached_login() {
    let _home = Home::new();
    let clients = FakeClients::install();
    seed_codex("", Some(&chatgpt_login("acct-a")));
    let official = |id: &str| {
        let mut row = Provider::with_id(
            id.to_string(),
            id.to_uppercase(),
            json!({ "auth": chatgpt_login(id), "config": "" }),
            None,
        );
        row.category = Some("official".to_string());
        row
    };
    let state = state_with(
        AppType::Codex,
        &[official("acct-a"), official("acct-b")],
        "acct-a",
    )
    .await;
    ProviderService::switch(&state, AppType::Codex, "acct-a").unwrap();
    clients.advance(10_000);
    clients.desktop_running_for("00:05");
    assert_eq!(stale_clients_of(&state).await, None);
    clients.advance(10_000);
    ProviderService::switch(&state, AppType::Codex, "acct-b").unwrap();
    clients.desktop_running_for("00:15");
    assert_eq!(codex_login_on_disk()["tokens"]["account_id"], "acct-b");
    assert!(stale_clients_of(&state).await.is_some());
    clients.advance(10_000);
    clients.desktop_running_for("00:05");
    assert_eq!(stale_clients_of(&state).await, None);
}

/// 桌面版在目录变化之前启动：Stack 视图带上 `staleClients`；之后启动的不算旧。
#[tokio::test]
#[serial]
async fn codex_stack_view_reports_clients_on_an_old_catalog() {
    let _home = Home::new();
    let clients = FakeClients::install();
    seed_codex("", None);
    clients.desktop_running_for("10:00");
    let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(
        stale_clients_of(&state).await,
        Some(codex_client_catalog::StaleClients {
            daemon: false,
            others: true,
            auth: false,
            unverified: false
        })
    );
    // 普通的 Stack 视图不读进程表。
    assert_eq!(
        stack_views(&state, &AppType::Codex).unwrap().stale_clients,
        None
    );

    // 桌面版重开之后启动：读到的正是现在这份目录。
    clients.advance(10_000);
    clients.desktop_running_for("00:05");
    assert_eq!(stale_clients_of(&state).await, None);

    // 目录又变了（再加一家）：刚才那个桌面版又旧了。
    clients.advance(10_000);
    set_codex_member(&state, "zhipu", true).await;
    clients.desktop_running_for("00:15");
    assert!(stale_clients_of(&state).await.is_some());

    // 换成路由模式，路由那家没有模型目录：指针撤掉了，桌面版还拿着 Stack 的目录，照样报。
    clients.advance(10_000);
    enter(&state, &AppType::Codex, false)
        .await
        .expect("routing");
    assert_eq!(codex_doc().get("model_catalog_json"), None);
    assert!(stale_clients_of(&state).await.is_some());
    // 重开之后启动：读的是内置列表，和现在一致。
    clients.advance(10_000);
    clients.desktop_running_for("00:05");
    assert_eq!(stale_clients_of(&state).await, None);
}

/// 从带目录的路由退回没有目录的直连：路由时启动的客户端还拿着路由那家的目录，要报；
/// 从没进过代理的直连用户（一直没有目录）不报。
#[tokio::test]
#[serial]
async fn codex_leaving_route_reports_clients_still_on_the_route_catalog() {
    let _home = Home::new();
    let clients = FakeClients::install();
    seed_codex("", None);
    let route = codex_native(
        "b",
        "https://b.example/v1",
        "",
        Some(json!({ "models": [{ "model": "b-main", "displayName": "B Main" }] })),
    );
    let state = state_with(
        AppType::Codex,
        &[codex_native("a", "https://a.example/v1", "", None), route],
        "a",
    )
    .await;
    // 一直直连、没有目录：早就在跑的客户端不报。
    clients.desktop_running_for("10:00");
    assert_eq!(stale_clients_of(&state).await, None);

    enter_with_route(&state, &AppType::Codex, false, Some("b"))
        .await
        .expect("routing");
    clients.advance(10_000);
    clients.desktop_running_for("00:05");
    assert_eq!(stale_clients_of(&state).await, None);

    clients.advance(10_000);
    exit(&state, &AppType::Codex).await.expect("exit");
    assert_eq!(codex_doc().get("model_catalog_json"), None);
    assert_eq!(
        stale_clients_of(&state).await,
        Some(codex_client_catalog::StaleClients {
            daemon: false,
            others: true,
            auth: false,
            unverified: false
        })
    );

    clients.advance(10_000);
    clients.desktop_running_for("00:05");
    assert_eq!(stale_clients_of(&state).await, None);
}

/// 直连切到路由模式后写进了模型目录，直连时启动的客户端还拿着旧配置：路由模式（非聚合）
/// 的视图也要报 `staleClients`；客户端重开、读到现在这份后不报。
#[tokio::test]
#[serial]
async fn codex_route_view_reports_clients_stale_after_a_mode_switch() {
    let _home = Home::new();
    let clients = FakeClients::install();
    seed_codex("", None);
    let route = codex_native(
        "b",
        "https://b.example/v1",
        "",
        Some(json!({ "models": [{ "model": "b-main", "displayName": "B Main" }] })),
    );
    let state = state_with(
        AppType::Codex,
        &[codex_native("a", "https://a.example/v1", "", None), route],
        "a",
    )
    .await;
    // 客户端在直连时启动（这时没有目录指针）。
    clients.desktop_running_for("10:00");
    assert_eq!(stale_clients_of(&state).await, None);

    enter_with_route(&state, &AppType::Codex, false, Some("b"))
        .await
        .expect("routing");
    assert_eq!(
        codex_doc()["model_catalog_json"].as_str(),
        Some(ours_catalog_pointer().as_str())
    );
    assert_eq!(
        stale_clients_of(&state).await,
        Some(codex_client_catalog::StaleClients {
            daemon: false,
            others: true,
            auth: false,
            unverified: false
        })
    );

    // 客户端重开之后启动：读到的正是现在这份目录。
    clients.advance(10_000);
    clients.desktop_running_for("00:05");
    assert_eq!(stale_clients_of(&state).await, None);
}

/// 路由那家自己管理模型目录时（行的指针指向别处），路由模式的视图同样不查：CC Switch 的
/// 目录不在线上，重启也看不到它。
#[tokio::test]
#[serial]
async fn codex_route_mode_with_its_own_catalog_does_not_report_clients() {
    let _home = Home::new();
    let clients = FakeClients::install();
    seed_codex("", None);
    clients.desktop_running_for("10:00");
    let route = codex_native(
        "a",
        "https://a.example/v1",
        "model_catalog_json = \"/opt/team/models.json\"\n",
        None,
    );
    let state = state_with(AppType::Codex, &[route], "a").await;
    enter(&state, &AppType::Codex, false)
        .await
        .expect("routing");
    assert_eq!(
        codex_doc()["model_catalog_json"].as_str(),
        Some("/opt/team/models.json")
    );
    assert_eq!(stale_clients_of(&state).await, None);
}

#[tokio::test]
#[serial]
async fn codex_route_with_its_own_catalog_skips_the_client_check() {
    let _home = Home::new();
    let clients = FakeClients::install();
    seed_codex("", None);
    clients.desktop_running_for("10:00");
    let [_, deepseek, zhipu] = codex_stack_rows();
    let route = codex_native(
        "a",
        "https://a.example/v1",
        "model_catalog_json = \"/opt/team/models.json\"\n",
        None,
    );
    let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    let view = stack_view_with_clients(&state, &AppType::Codex)
        .await
        .unwrap();
    assert_eq!(view.notice, Some("routeOwnsCatalog"));
    assert_eq!(view.stale_clients, None);
}

/// 用户直接在 config.toml 里指定的模型目录：写入时照留，生成的目录不生效，同样要提示。
/// 手写进 config.toml 的指针挡不住聚合：进代理时按关键字段清掉，加进 Stack 后换上合并目录。
#[tokio::test]
#[serial]
async fn codex_a_hand_written_catalog_pointer_gives_way_to_the_stack_catalog() {
    let _home = Home::new();
    seed_codex("model_catalog_json = \"/work/global-models.json\"\n", None);
    let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    assert!(
        codex_doc().get("model_catalog_json").is_none(),
        "{}",
        codex_text()
    );

    let notice = set_stack_member(&state, &AppType::Codex, "deepseek", true)
        .await
        .expect("stack");
    assert_eq!(notice, None);
    assert_eq!(
        codex_doc()["model_catalog_json"].as_str(),
        Some(ours_catalog_pointer().as_str())
    );
}

/// 提示上的「改用 CC Switch 的模型目录」：去掉路由那家行里的指针，契约带进 live 的那份
/// 跟着删掉，换上合并目录，不再提示。行里其余内容原样。
#[tokio::test]
#[serial]
async fn codex_adopting_the_catalog_drops_the_route_rows_own_pointer() {
    let _home = Home::new();
    seed_codex("", None);
    let [_, deepseek, zhipu] = codex_stack_rows();
    let route = codex_native(
        "a",
        "https://a.example/v1",
        "model_catalog_json = \"/opt/team/models.json\"\nmodel_verbosity = \"high\"\n",
        None,
    );
    let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(
        codex_doc()["model_catalog_json"].as_str(),
        Some("/opt/team/models.json")
    );

    let notice = adopt_codex_stack_catalog(&state).await.expect("adopt");
    assert_eq!(notice, None);
    let row = state
        .db
        .get_provider_by_id("a", &AppType::Codex)
        .unwrap()
        .unwrap();
    let row_config = row.settings_config["config"].as_str().unwrap();
    assert!(
        !row_config.contains("model_catalog_json") && row_config.contains("model_verbosity"),
        "{row_config}"
    );
    assert_eq!(
        codex_doc()["model_catalog_json"].as_str(),
        Some(ours_catalog_pointer().as_str())
    );
    let slugs: Vec<String> = codex_catalog()["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["slug"].as_str().unwrap().to_string())
        .collect();
    assert!(
        slugs.contains(&"ccs-deepseek/deepseek-v4-pro".to_string()),
        "{slugs:?}"
    );
    assert_eq!(stack_views(&state, &AppType::Codex).unwrap().notice, None);
}

/// 编辑器里的模型目录指针归这张卡：live 里手写的不显示、不收进行，保存当前卡时清掉；用户在
/// 编辑器里写的外来指针存进行、随卡写入；指向 CC Switch 目录的指针不存（由写入方决定）。
#[tokio::test]
#[serial]
async fn codex_editor_catalog_pointer_belongs_to_the_card() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(
        &format!("model_catalog_json = \"/work/mine.json\"\n{CODEX_USER_LIVE}"),
        None,
    );
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
    let open = |id: &str| {
        let row = state
            .db
            .get_provider_by_id(id, &AppType::Codex)
            .unwrap()
            .unwrap();
        let view = ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
            .expect("view");
        (row, view.settings)
    };
    let save = |mut row: Provider, edited: Value, base: Value| {
        row.settings_config = edited;
        ProviderService::update_from_editor(
            &state,
            AppType::Codex,
            None,
            row,
            Some(crate::services::provider::EditorSave {
                base,
                draft: None,
                on_conflict: Default::default(),
            }),
        )
    };
    let row_config = |id: &str| {
        state
            .db
            .get_provider_by_id(id, &AppType::Codex)
            .unwrap()
            .unwrap()
            .settings_config["config"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let with_pointer = |base: &Value, pointer: &str| {
        let mut edited = base.clone();
        edited["config"] = json!(format!(
            "model_catalog_json = {pointer:?}\n{}",
            base["config"].as_str().unwrap()
        ));
        edited
    };
    let pointer = || {
        codex_doc()
            .get("model_catalog_json")
            .and_then(|value| value.as_str().map(str::to_string))
    };

    let (row, base) = open("a");
    assert!(
        !base["config"]
            .as_str()
            .unwrap()
            .contains("model_catalog_json"),
        "{base}"
    );
    save(row, base.clone(), base).expect("save as is");
    assert!(!row_config("a").contains("model_catalog_json"));
    assert_eq!(pointer(), None, "{}", codex_text());

    // 手写 CC Switch 自己的目录：a 没有模型映射，不生成目录，行里不存、live 里也不写。
    let ours = crate::codex_config::get_codex_model_catalog_path()
        .display()
        .to_string();
    let (row, base) = open("a");
    save(row, with_pointer(&base, &ours), base).expect("save ours");
    assert!(!row_config("a").contains("model_catalog_json"));
    assert_eq!(pointer(), None, "{}", codex_text());

    let (row, base) = open("b");
    save(row, with_pointer(&base, "/work/b.json"), base).expect("save b pointer");
    assert!(
        row_config("b").contains("model_catalog_json = \"/work/b.json\""),
        "{}",
        row_config("b")
    );
    ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
    assert_eq!(pointer().as_deref(), Some("/work/b.json"));
    ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
    assert_eq!(pointer(), None, "{}", codex_text());
}

/// 普通保存入口（不带编辑器底）改了 Stack 里那家的模型目录：合并目录跟着重算。
#[tokio::test]
#[serial]
async fn editing_an_stacked_codex_provider_rewrites_the_catalog() {
    let _home = Home::new();
    seed_codex("", None);
    let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;

    let mut deepseek = state
        .db
        .get_provider_by_id("deepseek", &AppType::Codex)
        .unwrap()
        .unwrap();
    deepseek.settings_config["modelCatalog"] = json!({ "models": [
        { "model": "deepseek-v5" }
    ]});
    ProviderService::update(&state, AppType::Codex, None, deepseek).expect("update deepseek");
    let slugs: Vec<String> = codex_catalog()["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["slug"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(slugs, vec!["gpt-a", "ccs-deepseek/deepseek-v5"]);
}

/// 「已添加」列表拖动排序：名单按 sort_index 重排，合并目录跟着重写；顺序没变时不写。
#[tokio::test]
#[serial]
async fn reordering_providers_reorders_the_codex_stack_catalog() {
    let _home = Home::new();
    seed_codex("", None);
    let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    set_codex_member(&state, "zhipu", true).await;
    assert_eq!(
        catalog_slugs(),
        vec![
            "gpt-a",
            "ccs-deepseek/deepseek-v4-pro",
            "ccs-zhipu/gpt-zhipu"
        ]
    );

    let updates = ["zhipu", "a", "deepseek"]
        .iter()
        .enumerate()
        .map(
            |(index, id)| crate::services::provider::ProviderSortUpdate {
                id: id.to_string(),
                sort_index: index,
            },
        )
        .collect();
    ProviderService::update_sort_order(&state, AppType::Codex, updates).expect("sort");
    sync_stack_order(&state, &AppType::Codex)
        .await
        .expect("sync order");
    assert_eq!(
        stack_state_of(&AppType::Codex).members,
        vec!["zhipu", "a", "deepseek"]
    );
    // 默认那家的行永远排最前，其余按名单顺序。
    assert_eq!(
        catalog_slugs(),
        vec![
            "gpt-a",
            "ccs-zhipu/gpt-zhipu",
            "ccs-deepseek/deepseek-v4-pro"
        ]
    );

    let catalog_path = crate::codex_config::get_codex_model_catalog_path();
    let before = std::fs::metadata(&catalog_path)
        .unwrap()
        .modified()
        .unwrap();
    sync_stack_order(&state, &AppType::Codex)
        .await
        .expect("sync again");
    assert_eq!(
        std::fs::metadata(&catalog_path)
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
}

/// 直连模式下拖动排序只落定名单、不碰客户端文件，下次进入聚合时按新顺序发布。
#[tokio::test]
#[serial]
async fn reordering_providers_in_direct_mode_only_settles_the_stack() {
    let _home = Home::new();
    seed_codex("", None);
    let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
    set_codex_member(&state, "deepseek", true).await;
    set_codex_member(&state, "zhipu", true).await;
    assert_eq!(
        stack_state_of(&AppType::Codex).members,
        vec!["deepseek", "zhipu"]
    );
    let config_path = crate::codex_config::get_codex_config_path();
    let config_before = std::fs::read(&config_path).unwrap();

    let updates = ["zhipu", "a", "deepseek"]
        .iter()
        .enumerate()
        .map(
            |(index, id)| crate::services::provider::ProviderSortUpdate {
                id: id.to_string(),
                sort_index: index,
            },
        )
        .collect();
    ProviderService::update_sort_order(&state, AppType::Codex, updates).expect("sort");
    sync_stack_order(&state, &AppType::Codex)
        .await
        .expect("sync order");
    assert_eq!(
        stack_state_of(&AppType::Codex).members,
        vec!["zhipu", "deepseek"]
    );
    assert_eq!(std::fs::read(&config_path).unwrap(), config_before);

    enter(&state, &AppType::Codex, true).await.expect("enter");
    assert_eq!(
        catalog_slugs(),
        vec![
            "gpt-a",
            "ccs-zhipu/gpt-zhipu",
            "ccs-deepseek/deepseek-v4-pro"
        ]
    );
}

/// 排序只改 sort_index：排序批次里带着刚删掉的供应商也不会把它写回来。
#[tokio::test]
#[serial]
async fn sort_order_update_does_not_resurrect_a_deleted_provider() {
    let _home = Home::new();
    seed_codex("", None);
    let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
    state.db.delete_provider(&AppType::Codex, "zhipu").unwrap();

    let updates = ["zhipu", "deepseek", "a"]
        .iter()
        .enumerate()
        .map(
            |(index, id)| crate::services::provider::ProviderSortUpdate {
                id: id.to_string(),
                sort_index: index,
            },
        )
        .collect();
    ProviderService::update_sort_order(&state, AppType::Codex, updates).expect("sort");
    assert!(state
        .db
        .get_provider_by_id("zhipu", &AppType::Codex)
        .unwrap()
        .is_none());
    let deepseek = state
        .db
        .get_provider_by_id("deepseek", &AppType::Codex)
        .unwrap()
        .unwrap();
    assert_eq!(deepseek.sort_index, Some(1));
}

#[tokio::test]
#[serial]
async fn codex_a_broken_stacked_provider_is_skipped_and_cannot_be_stacked() {
    let _home = Home::new();
    seed_codex("", None);
    let mut rows = codex_stack_rows().to_vec();
    let mut broken = codex_native("broken", "https://b.example/v1", "", None);
    broken.settings_config["config"] = json!("model = [\n");
    rows.push(broken);
    let state = state_with(AppType::Codex, &rows, "a").await;
    enter(&state, &AppType::Codex, true).await.expect("enter");

    // 配置解析不了的行不能加进 Stack，什么都不改。
    let error = set_stack_member(&state, &AppType::Codex, "broken", true)
        .await
        .unwrap_err();
    assert!(!error.partial);
    assert_eq!(stack_state_of(&AppType::Codex).members, vec!["a"]);

    // 加进 Stack 之后才坏掉（比如云同步直接换了行）：只跳过这一家，其余照常发布，
    // 重写和换路由都不受影响。
    set_codex_member(&state, "deepseek", true).await;
    set_codex_member(&state, "zhipu", true).await;
    assert_eq!(
        catalog_slugs(),
        vec![
            "gpt-a",
            "ccs-deepseek/deepseek-v4-pro",
            "ccs-zhipu/gpt-zhipu"
        ]
    );
    let mut zhipu = state
        .db
        .get_provider_by_id("zhipu", &AppType::Codex)
        .unwrap()
        .unwrap();
    zhipu.settings_config["config"] = json!("model = [\n");
    state.db.save_provider(&AppType::Codex, &zhipu).unwrap();
    resync_route(&state, &AppType::Codex).await.expect("resync");
    assert_eq!(
        catalog_slugs(),
        vec!["gpt-a", "ccs-deepseek/deepseek-v4-pro"]
    );
    set_route(&state, &AppType::Codex, "deepseek")
        .await
        .expect("switch route");
    assert_eq!(
        mode(&AppType::Codex).proxy_route.as_deref(),
        Some("deepseek")
    );
}

/// 行里的 TOML 坏在密钥那一行：解析诊断会带上这行原文，失败日志不能把它写进日志文件。
#[tokio::test]
#[serial]
async fn failure_logs_drop_the_config_line_a_broken_row_quotes() {
    let _home = Home::new();
    seed_codex("", None);
    let secret = "sk-review-only-secret";
    let mut rows = codex_stack_rows().to_vec();
    let mut broken = codex_native("broken", "https://b.example/v1", "", None);
    broken.settings_config["config"] =
        json!(format!("experimental_bearer_token = \"{secret}\" !\n"));
    rows.push(broken);
    let state = state_with(AppType::Codex, &rows, "a").await;

    let switch_error = ProviderService::switch(&state, AppType::Codex, "broken")
        .unwrap_err()
        .to_string();
    let enter_error = enter_with_route(&state, &AppType::Codex, false, Some("broken"))
        .await
        .unwrap_err();
    for error in [switch_error, enter_error] {
        assert!(error.contains(secret), "前提：错误里带配置原文 {error}");
        let logged = crate::error_for_log(&error);
        assert!(!logged.contains(secret), "{logged}");
        assert!(logged.contains("line 1"), "{logged}");
    }
}

#[tokio::test]
#[serial]
async fn codex_official_accounts_cannot_be_stacked() {
    let _home = Home::new();
    seed_codex("", None);
    let mut rows = codex_stack_rows().to_vec();
    rows.push(codex_official());
    let state = state_with(AppType::Codex, &rows, "a").await;
    let error = set_stack_member(
        &state,
        &AppType::Codex,
        crate::database::CODEX_OFFICIAL_PROVIDER_ID,
        true,
    )
    .await
    .expect_err("official");
    assert!(!error.partial);
    assert!(stack_state_of(&AppType::Codex).members.is_empty());
}

fn stack_state_of(app: &AppType) -> StackState {
    state::stack(&DeviceStore::for_device(), app.as_str()).unwrap()
}

// ---- Stack 模型：Codex 官方做路由 ----

use crate::services::provider::codex_official_models::testing::{
    login as chatgpt, native_models, Calls, Fake,
};
use crate::services::provider::codex_official_models::{self as official_models, Fetch};
use crate::services::subscription::CodexKeychainLogin;

/// 换上假的官方接口；结束时换回来。
struct FakeModels {
    calls: Calls,
    now: Arc<std::sync::Mutex<u64>>,
    responses: Arc<std::sync::Mutex<Vec<Fetch>>>,
    keychain_reads: Arc<std::sync::atomic::AtomicUsize>,
}

impl Drop for FakeModels {
    fn drop(&mut self) {
        official_models::reset_test_env();
    }
}

const NOW: u64 = 1_800_000_000;

fn fake_models(
    responses: Vec<Fetch>,
    keychain: CodexKeychainLogin,
    on_fetch: Option<Box<dyn Fn() + Send + Sync>>,
) -> FakeModels {
    fake_models_sharing(
        responses,
        Arc::new(std::sync::Mutex::new(keychain)),
        on_fetch,
    )
}

/// 同 [`fake_models`]，钥匙串由调用方持有，可以在拉取时换掉。
fn fake_models_sharing(
    responses: Vec<Fetch>,
    keychain: Arc<std::sync::Mutex<CodexKeychainLogin>>,
    on_fetch: Option<Box<dyn Fn() + Send + Sync>>,
) -> FakeModels {
    let calls: Calls = Arc::default();
    let now = Arc::new(std::sync::Mutex::new(NOW));
    let responses = Arc::new(std::sync::Mutex::new(responses));
    let keychain_reads: Arc<std::sync::atomic::AtomicUsize> = Arc::default();
    Fake {
        version: Some("0.158.0".to_string()),
        responses: responses.clone(),
        bundled: None,
        keychain,
        keychain_reads: keychain_reads.clone(),
        now: now.clone(),
        calls: calls.clone(),
        on_fetch,
    }
    .install();
    FakeModels {
        calls,
        now,
        responses,
        keychain_reads,
    }
}

fn official_list(extra: &[(&str, i64)]) -> Fetch {
    let mut slugs = vec![("gpt-6-sol", 4), ("gpt-5.5", 12)];
    slugs.extend_from_slice(extra);
    Fetch::Models {
        models: native_models(&slugs),
        etag: Some("\"e1\"".to_string()),
    }
}

fn catalog_slugs() -> Vec<String> {
    codex_catalog()["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["slug"].as_str().unwrap().to_string())
        .collect()
}

fn codex_official_stack_rows() -> Vec<Provider> {
    let mut rows = vec![codex_official()];
    rows.extend(codex_stack_rows());
    rows
}

/// 写目录那一刻钥匙串没读出来，聚合的模型暂不可用；钥匙串又读得出时缓存里的条目还新，
/// 后台检查不联网、直接按它重写（不等条目过了新鲜期）。
#[tokio::test]
#[serial]
async fn codex_a_fallback_recovers_from_a_fresh_entry_once_the_login_is_back() {
    let _home = Home::new();
    seed_codex(
        "cli_auth_credentials_store = \"auto\"\n",
        Some(&chatgpt("ws", "alice")),
    );
    let keychain = Arc::new(std::sync::Mutex::new(CodexKeychainLogin::Missing));
    let fake = fake_models_sharing(vec![official_list(&[])], keychain.clone(), None);
    let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert!(catalog_slugs().contains(&"gpt-6-sol".to_string()));

    *keychain.lock().unwrap() = CodexKeychainLogin::Unknown;
    resync_route(&state, &AppType::Codex).await.expect("resync");
    let view = stack_views(&state, &AppType::Codex).unwrap();
    assert_eq!(view.notice, Some("officialModelsUnavailable"));
    assert_eq!(view.notice_reasons, vec!["noLogin", "noCli"]);

    *keychain.lock().unwrap() = CodexKeychainLogin::Missing;
    *fake.now.lock().unwrap() = NOW + 1800;
    check_codex_official_models(&state).await;
    assert_eq!(fake.calls.lock().unwrap().len(), 1, "no new fetch");
    assert!(catalog_slugs().contains(&"gpt-6-sol".to_string()));
    assert!(stack_views(&state, &AppType::Codex)
        .unwrap()
        .notice
        .is_none());
}

#[tokio::test]
#[serial]
async fn codex_official_route_lists_every_official_model_before_the_stacked_ones() {
    let _home = Home::new();
    let alice = chatgpt("ws", "alice");
    seed_codex("", Some(&alice));
    let auth_bytes = fs::read(codex_auth_path()).unwrap();
    let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
    let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    assert!(
        fake.calls.lock().unwrap().is_empty(),
        "no stacked models, no fetch"
    );

    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(
        catalog_slugs(),
        vec!["gpt-6-sol", "gpt-5.5", "ccs-deepseek/deepseek-v4-pro"]
    );
    let catalog = codex_catalog();
    let sol = &catalog["models"][0];
    // 原生字段保留，旧的指令字段补上。
    assert_eq!(sol["comp_hash"], "3000");
    assert_eq!(sol["base_instructions"], "T");
    // 取列表用的是 Codex 实际会用的登录；整个过程不写 auth.json。
    assert_eq!(
        fake.calls.lock().unwrap().clone(),
        vec![("ws|sub:alice".to_string(), "0.158.0".to_string(), None)]
    );
    assert_eq!(fs::read(codex_auth_path()).unwrap(), auth_bytes);
    assert!(stack_views(&state, &AppType::Codex)
        .unwrap()
        .notice
        .is_none());

    // 后台检查：半小时后是新的，不联网；七小时后刷新，列表变了就重写目录。
    *fake.now.lock().unwrap() = NOW + 1800;
    check_codex_official_models(&state).await;
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
    *fake.now.lock().unwrap() = NOW + 7 * 3600;
    fake.responses
        .lock()
        .unwrap()
        .push(official_list(&[("gpt-6-luna", 5)]));
    check_codex_official_models(&state).await;
    assert_eq!(
        fake.calls.lock().unwrap().last().unwrap().2.as_deref(),
        Some("\"e1\""),
        "the refresh revalidates with the etag"
    );
    assert!(catalog_slugs().contains(&"gpt-6-luna".to_string()));

    // 名单清空：不再写目录。
    set_codex_member(&state, "deepseek", false).await;
    assert!(codex_doc().get("model_catalog_json").is_none());
    exit(&state, &AppType::Codex).await.expect("exit");
    assert_eq!(fs::read(codex_auth_path()).unwrap(), auth_bytes);
}

/// #8014：本机 Codex 太旧时服务端只回隐藏条目。照写的话官方模型在选择器里一个都
/// 看不到、只剩 Stack 模型；不写这种目录，并提示升级。
#[tokio::test]
#[serial]
async fn codex_official_list_without_a_listed_model_is_not_published() {
    let _home = Home::new();
    seed_codex("", Some(&chatgpt("ws", "alice")));
    let mut models = native_models(&[("gpt-5.5", 1), ("codex-auto-review", 2)]);
    for model in &mut models {
        model["visibility"] = serde_json::json!("hide");
    }
    let _fake = fake_models(
        vec![Fetch::Models { models, etag: None }],
        CodexKeychainLogin::Missing,
        None,
    );
    let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert!(codex_doc().get("model_catalog_json").is_none());
    assert_eq!(
        stack_views(&state, &AppType::Codex).unwrap().notice,
        Some("officialModelsOutdated")
    );
}

/// 列表变了而重写失败（这里是 config.toml 恰好解析不了）：缓存已经是新的，下一次检查
/// 不再联网、拿到的也不会是「列表变了」，但仍按缓存补写客户端文件。
#[tokio::test]
#[serial]
async fn codex_a_failed_rewrite_after_a_refresh_is_retried_at_the_next_check() {
    let _home = Home::new();
    seed_codex("", Some(&chatgpt("ws", "alice")));
    let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let break_config: Box<dyn Fn() + Send + Sync> = {
        let fetches = fetches.clone();
        Box::new(move || {
            // 第二次拉取（后台刷新）时 config.toml 恰好坏了。
            if fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                fs::write(codex_config_path(), "model = [\n").unwrap();
            }
        })
    };
    let fake = fake_models(
        vec![official_list(&[]), official_list(&[("gpt-6-luna", 5)])],
        CodexKeychainLogin::Missing,
        Some(break_config),
    );
    let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    let good = codex_text();

    *fake.now.lock().unwrap() = NOW + 7 * 3600;
    check_codex_official_models(&state).await;
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
    assert!(!catalog_slugs().contains(&"gpt-6-luna".to_string()));

    fs::write(codex_config_path(), &good).unwrap();
    check_codex_official_models(&state).await;
    assert_eq!(fake.calls.lock().unwrap().len(), 2, "the cache is fresh");
    assert!(catalog_slugs().contains(&"gpt-6-luna".to_string()));

    // 补写成功后标记清掉：再检查什么都不做。
    let written = codex_text();
    check_codex_official_models(&state).await;
    assert_eq!(codex_text(), written);
}

#[tokio::test]
#[serial]
async fn codex_a_login_that_changes_before_the_write_stops_it() {
    let _home = Home::new();
    seed_codex("", Some(&chatgpt("ws", "alice")));
    // 拿锁前按 alice 取了列表，之后 Codex 恰好换成同一工作区的 bob 登录。
    let _fake = fake_models(
        vec![official_list(&[])],
        CodexKeychainLogin::Missing,
        Some(Box::new(|| {
            fs::write(codex_auth_path(), chatgpt("ws", "bob").to_string()).unwrap();
        })),
    );
    let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    let before = codex_text();

    let error = set_stack_member(&state, &AppType::Codex, "deepseek", true)
        .await
        .expect_err("login changed");
    assert!(!error.partial, "{}", error.message);
    assert_eq!(codex_text(), before);
    assert!(!crate::codex_config::get_codex_model_catalog_path().exists());
    assert!(stack_state_of(&AppType::Codex).members.is_empty());
}

/// 钥匙串不归写锁管：按 alice 取列表的时候 Codex 在钥匙串里换成了 bob，不能把 alice
/// 的列表写给 bob。
#[tokio::test]
#[serial]
async fn codex_a_keychain_login_that_changes_during_the_fetch_stops_the_write() {
    let _home = Home::new();
    seed_codex("cli_auth_credentials_store = \"keyring\"\n", None);
    let keychain = Arc::new(std::sync::Mutex::new(CodexKeychainLogin::Found(chatgpt(
        "ws", "alice",
    ))));
    let switched = keychain.clone();
    let fake = fake_models_sharing(
        vec![official_list(&[])],
        keychain,
        Some(Box::new(move || {
            *switched.lock().unwrap() = CodexKeychainLogin::Found(chatgpt("ws", "bob"));
        })),
    );
    let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    let before = codex_text();

    let error = set_stack_member(&state, &AppType::Codex, "deepseek", true)
        .await
        .expect_err("login changed");
    assert!(!error.partial, "{}", error.message);
    assert_eq!(fake.calls.lock().unwrap()[0].0, "ws|sub:alice");
    assert_eq!(codex_text(), before);
    assert!(!crate::codex_config::get_codex_model_catalog_path().exists());
    assert!(stack_state_of(&AppType::Codex).members.is_empty());

    // 重试按 bob 重新取。
    *fake.responses.lock().unwrap() = vec![official_list(&[])];
    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(fake.calls.lock().unwrap()[1].0, "ws|sub:bob");
    assert!(catalog_slugs().contains(&"gpt-6-sol".to_string()));
}

#[tokio::test]
#[serial]
async fn codex_the_login_comes_from_where_codex_keeps_it() {
    let _home = Home::new();
    let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;

    // keyring：auth.json 里残留 alice，Codex 用的是钥匙串里的 bob。
    seed_codex(
        "cli_auth_credentials_store = \"keyring\"\n",
        Some(&chatgpt("ws", "alice")),
    );
    let fake = fake_models(
        vec![official_list(&[])],
        CodexKeychainLogin::Found(chatgpt("ws", "bob")),
        None,
    );
    let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(fake.calls.lock().unwrap()[0].0, "ws|sub:bob");
    // 预测登录读一次，取完列表再读一次给拿锁后的核对用；写锁里不读。
    assert_eq!(
        fake.keychain_reads
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    exit(&state, &AppType::Codex).await.expect("exit");
    set_codex_member(&state, "deepseek", false).await;
    drop(fake);

    // auto：钥匙串读不出来（Windows、Linux，或访问被拒）时不知道 Codex 用的是谁，
    // 不拿 auth.json 里的 alice 顶替。
    seed_codex(
        "cli_auth_credentials_store = \"auto\"\n",
        Some(&chatgpt("ws", "alice")),
    );
    let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Unknown, None);
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert!(fake.calls.lock().unwrap().is_empty());
    let view = stack_views(&state, &AppType::Codex).unwrap();
    assert_eq!(view.notice, Some("officialModelsUnavailable"));
    assert_eq!(view.notice_reasons, vec!["noLogin", "noCli"]);
    exit(&state, &AppType::Codex).await.expect("exit");
    set_codex_member(&state, "deepseek", false).await;
    drop(fake);

    // auto：钥匙串里确定没有，Codex 退回 auth.json。
    seed_codex(
        "cli_auth_credentials_store = \"auto\"\n",
        Some(&chatgpt("ws", "alice")),
    );
    let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert_eq!(fake.calls.lock().unwrap()[0].0, "ws|sub:alice");
    exit(&state, &AppType::Codex).await.expect("exit");
    set_codex_member(&state, "deepseek", false).await;
    drop(fake);

    // ephemeral：登录从不落盘，不拉取；没有自带列表时 Stack 模型暂不可用，也不写目录。
    seed_codex(
        "cli_auth_credentials_store = \"ephemeral\"\n",
        Some(&chatgpt("ws", "alice")),
    );
    let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert!(fake.calls.lock().unwrap().is_empty());
    assert!(codex_doc().get("model_catalog_json").is_none());
    assert_eq!(
        stack_views(&state, &AppType::Codex).unwrap().notice,
        Some("officialModelsUnavailable")
    );
    exit(&state, &AppType::Codex).await.expect("exit");
    set_codex_member(&state, "deepseek", false).await;
    drop(fake);

    // 登录超过 8 天没刷新：不拉取，提示里说清是登录过期，而不是笼统的「读取不到」。
    let mut stale = chatgpt("ws", "alice");
    stale["last_refresh"] = serde_json::json!("2026-01-01T00:00:00Z");
    seed_codex("", Some(&stale));
    let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
    enter(&state, &AppType::Codex, true).await.expect("enter");
    set_codex_member(&state, "deepseek", true).await;
    assert!(fake.calls.lock().unwrap().is_empty());
    let view = stack_views(&state, &AppType::Codex).unwrap();
    assert_eq!(view.notice, Some("officialModelsUnavailable"));
    assert_eq!(view.notice_reasons, vec!["loginStale", "noCli"]);
}

#[tokio::test]
#[serial]
async fn codex_switching_back_to_official_uses_the_login_it_will_restore() {
    let _home = Home::new();
    set_preservation(false);
    seed_codex("", Some(&chatgpt("ws", "alice")));
    let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
    let state = state_with(AppType::Codex, &codex_official_stack_rows(), "a").await;
    // 直连切到第三方：登录存进暂存，auth.json 删掉。
    ProviderService::switch(&state, AppType::Codex, "a").expect("direct a");
    assert!(!codex_auth_path().exists());
    set_codex_member(&state, "deepseek", true).await;
    enter(&state, &AppType::Codex, true).await.expect("enter");
    assert!(fake.calls.lock().unwrap().is_empty(), "third-party route");

    // 换路由到官方卡：auth.json 会从暂存还回 alice，列表按 alice 取。
    let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
    ProviderService::switch(&state, AppType::Codex, official).expect("route to official");
    assert_eq!(fake.calls.lock().unwrap()[0].0, "ws|sub:alice");
    assert!(catalog_slugs().contains(&"gpt-6-sol".to_string()));
    let restored: Value = crate::config::read_json_file(&codex_auth_path()).unwrap();
    assert_eq!(restored["tokens"]["account_id"], "ws");
}

/// `src/config/codexTemplates.ts` 的 `getCodexCustomTemplate()`：Key 为空、
/// `requires_openai_auth = true`。新增对话框一打开就投影它。
fn codex_keyless_template() -> Value {
    json!({
        "auth": { "OPENAI_API_KEY": "" },
        "config": "model_provider = \"custom\"\nmodel = \"gpt-5.6-sol\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
    })
}

/// 没填 Key 的草稿：显示「切过去之后的样子」和只存行都不拿切换的安全闸拒绝，要进 live
/// 时（路由到它、直连切到它）照样拒绝，否则会把 ChatGPT 登录发给第三方。
#[tokio::test]
#[serial]
async fn codex_keyless_draft_is_shown_and_saved_but_never_written_to_live() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex("model = \"gpt-5.4\"\n", Some(&chatgpt_login("acct")));
    let [a, b] = codex_a_b();
    let official = codex_official();
    let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;
    ProviderService::switch(&state, AppType::Codex, &official.id).expect("official");
    let template = codex_keyless_template();
    let view = |state: &AppState, label: &str| -> Value {
        let view = ProviderService::editor_view(state, AppType::Codex, &template, None)
            .unwrap_or_else(|err| panic!("{label}: cannot show the template: {err}"));
        let shown = view.settings["config"].as_str().unwrap().to_string();
        assert!(!shown.contains("pending-key"), "{label}: {shown}");
        let doc: toml::Table = toml::from_str(&shown).unwrap();
        assert_eq!(
            doc["model_provider"].as_str(),
            Some("custom"),
            "{label}: {shown}"
        );
        assert!(doc.get("openai_base_url").is_none(), "{label}: {shown}");
        let route = &doc["model_providers"]["custom"];
        assert_eq!(
            route["requires_openai_auth"].as_bool(),
            Some(true),
            "{label}: {shown}"
        );
        assert!(
            route.get("experimental_bearer_token").is_none(),
            "{label}: {shown}"
        );
        view.settings
    };

    // 直连（当前是官方卡）。
    view(&state, "direct");
    // 代理的官方路由：live 顶层是 openai_base_url。
    enter(&state, &AppType::Codex, false).await.expect("enter");
    let routed = codex_text();
    assert!(routed.contains("openai_base_url"), "{routed}");
    let shown = view(&state, "official proxy route");

    // 表单确认过「不填 Key 也保存」：只存行，live 不动，占位 Key 不进行。
    let draft = Provider::with_id("c".into(), "C".into(), template.clone(), None);
    add_from_editor(&state, AppType::Codex, draft, shown.clone(), shown)
        .expect("a keyless row can be saved");
    assert_eq!(codex_text(), routed);
    let c = state
        .db
        .get_provider_by_id("c", &AppType::Codex)
        .unwrap()
        .unwrap();
    let c_config = c.settings_config["config"].as_str().unwrap();
    assert!(!c_config.contains("pending-key"), "{c_config}");
    assert!(
        !c_config.contains("experimental_bearer_token"),
        "{c_config}"
    );
    assert!(
        c_config.contains("requires_openai_auth = true"),
        "{c_config}"
    );

    // 路由到它、直连切到它都拒绝，live 不动。
    let err = ProviderService::switch(&state, AppType::Codex, "c").expect_err("route to c");
    assert!(err.to_string().contains("requires_openai_auth"), "{err}");
    assert_eq!(codex_text(), routed);
    exit(&state, &AppType::Codex).await.expect("exit");
    let direct_live = codex_text();
    let err = ProviderService::switch(&state, AppType::Codex, "c").expect_err("switch to c");
    assert!(err.to_string().contains("requires_openai_auth"), "{err}");
    assert_eq!(codex_text(), direct_live);
    assert_eq!(
        direct(&state, &AppType::Codex).as_deref(),
        Some(official.id.as_str())
    );
}

/// 直连模式下编辑当前供应商：保存会把关键字段一起换进 live，这时去掉 Key 照样拒绝，行
/// 撤回、live 不动。
#[tokio::test]
#[serial]
async fn codex_editing_the_current_provider_into_a_keyless_row_is_refused() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
    let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
    ProviderService::switch(&state, AppType::Codex, "a").expect("a");
    let before_live = codex_text();
    let mut row = state
        .db
        .get_provider_by_id("a", &AppType::Codex)
        .unwrap()
        .unwrap();
    let before_row = row.settings_config.clone();

    let base = ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
        .expect("view")
        .settings;
    let mut doc: toml_edit::DocumentMut = base["config"].as_str().unwrap().parse().unwrap();
    doc["model_providers"]["custom"]["requires_openai_auth"] = toml_edit::value(true);
    let mut edited = base.clone();
    edited["config"] = json!(doc.to_string());
    edited["auth"]["OPENAI_API_KEY"] = json!("");
    row.settings_config = edited;
    let err = ProviderService::update_from_editor(
        &state,
        AppType::Codex,
        None,
        row,
        Some(crate::services::provider::EditorSave {
            base,
            draft: None,
            on_conflict: Default::default(),
        }),
    )
    .expect_err("the current provider cannot lose its key");
    assert!(err.to_string().contains("requires_openai_auth"), "{err}");
    assert_eq!(codex_text(), before_live);
    assert_eq!(
        state
            .db
            .get_provider_by_id("a", &AppType::Codex)
            .unwrap()
            .unwrap()
            .settings_config,
        before_row
    );
}

/// 新增第一个供应商会同时写进 live：没填 Key 照样拒绝，行不留。
#[tokio::test]
#[serial]
async fn codex_adding_a_keyless_first_provider_is_refused() {
    let _home = Home::new();
    set_preservation(true);
    seed_codex("model = \"gpt-5.4\"\n", Some(&chatgpt_login("acct")));
    let state = state_without_providers().await;
    let before = codex_text();
    let template = codex_keyless_template();
    let base = ProviderService::editor_view(&state, AppType::Codex, &template, None)
        .expect("view")
        .settings;
    let draft = Provider::with_id("c".into(), "C".into(), template, None);
    let err = add_from_editor(&state, AppType::Codex, draft, base.clone(), base)
        .expect_err("a keyless first provider would go live");
    assert!(err.to_string().contains("requires_openai_auth"), "{err}");
    assert!(state
        .db
        .get_provider_by_id("c", &AppType::Codex)
        .unwrap()
        .is_none());
    assert_eq!(codex_text(), before);
}

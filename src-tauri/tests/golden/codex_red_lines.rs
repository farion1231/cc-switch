//! ② Codex 直连切换的红线。
//!
//! 这些红线先在整份写入的旧实现上写成，Codex 改成只替换关键字段之后照样通过：这里只锁
//! 结果，不锁机制，断言写成「Codex 0.149 能加载」「官方登录不会被带到第三方地址」「被拒的
//! 切换没有副作用」这类性质，不断言表叫什么 id、归一化走的哪条分支。
//!
//! 红线索引（CX 编号；「已有」指 `tests/provider_service.rs` 等处已有的黑盒测试，
//! 「crate 内」指经公开服务入口、但依赖 crate 内测试钩子而留在 src 里的测试）：
//!
//! | CX | 红线 | 锁在哪 |
//! |---|---|---|
//! | 01 | 第三方 Key 只落在活跃路由表里：不进 auth.json，不在顶层 | 本文件 |
//! | 02 | 清除登录一律删 auth.json，永不写 `{}`（Codex 把 `{}` 当无 token 的 ChatGPT 模式，启动即报错） | 本文件；已有 `..._default_removes_auth_json_when_preservation_off` |
//! | 03 | 保留登录开关打开时，第三方切换不动 auth.json 的字节 | 本文件 |
//! | 04 | 路由表已声明自己的鉴权（env_key / auth / Authorization 头）时不注入 Key | 本文件 |
//! | 05 | `requires_openai_auth = true` 不妨碍注入 Key（否则 Codex 回退读官方 OAuth 发给第三方） | 本文件 |
//! | 06 | 带 Key 但没有落点、或无 Key 却会回退官方登录的配置，拒绝切换 | 本文件；已有 `..._rejects_keyless_official_auth_fallback`、`..._rejects_empty_third_party_config` |
//! | 07 | 被拒的切换没有副作用：live 字节与 mtime、当前供应商、被拒卡的行都不变 | 本文件 |
//! | 08 | 只有 model / MCP、没有路由的卡在保留开关打开时放行（有意保留的边界） | 已有 `provider_service_switch_codex_updates_live_and_config` |
//! | 09 | 旧形态 `openai_base_url` 改道不进 live：Key 进非保留的路由表，顶层不留 `openai_base_url` | 本文件；已有三条 `..._normalizes_legacy_reroute_config` |
//! | 10 | 归一化不覆盖用户自己的同名表 | 本文件 |
//! | 11 | inline 写法的 `model_providers = { … }`：Key 落进 inline 表 | 本文件 |
//! | 12 | 保留 id 大小写精确：`OpenAI`、`oss` 是合法自定义 id；不产出 `[model_providers.openai]` 这类保留表 | 本文件 |
//! | 13 | 存量的保留表（`[model_providers.openai]` 等）不进 live，带 Key 时路由落到非保留表 | 本文件 |
//! | 14 | 直连时 `requires_openai_auth`：开关关→活跃带 Key 表为 false；开关开且已登录→true；无 Key 的头鉴权表从不为 true | 本文件 |
//! | 15 | 代理注入型 OAuth 卡（xai_oauth 等）直连不会带出官方登录 | 本文件 |
//! | 16 | 官方卡：带登录材料整份写 auth.json；无材料只写 config、不动现有登录 | 已有 `..._official_accounts_write_auth_json`、`..._supports_official_login_provider_without_auth_write` |
//! | 17 | 切到无材料官方卡时删掉第三方残留的 auth.json，真实登录不删；重选当前卡不删 | 已有 `..._official_clears_stale_third_party_auth`、`provider_service_reswitch_current_official_keeps_live_auth` |
//! | 18 | 元数据（last_refresh、account_id）不算登录（#6277） | crate 内：`codex_config.rs` 的 `credential_login_material_only_counts_real_credentials` |
//! | 19 | 托管账号切走：先采纳 CLI 轮换过的 refresh token，按 marker 精确删；代际无法排序时拒绝 | crate 内：`services/provider/mod.rs` 的托管账号测试 |
//! | 20 | 进入代理不写 auth.json；第三方路由契约用字面值 `PROXY_MANAGED` | 已有 `codex_official_to_deepseek_then_takeover_...` |
//! | 21 | 代理下官方路由不写占位凭据，客户端带自己的真实登录 | crate 内：`mode::controller` 的 `codex_routes_between_official_and_third_party_contracts` |
//! | 22 | 退出代理不覆盖用户此刻的登录状态（期间登出就保持登出，重新登录就保留新登录） | 本文件 |
//! | 23 | `model_catalog_json` 只认领 `cc-switch-model-catalog.json`；用户自己的指针不认领、不删除 | 本文件 |
//! | 24 | `web_search = "disabled"` 只删 CC Switch 写的哨兵值，用户的其他值保留 | 本文件 |
//! | 25 | auth.json 删不掉时切换照常成功，返回 `codex_auth_cleanup_failed` 警告 | 本文件 |
//!
//! 只替换关键字段之后新增的性质锁在 crate 内 `mode::controller` 的 `codex_*` 测试里：其余
//! 字节不动、独有字段只删上一家的值、切回官方留下休眠表、生效的 profile 覆盖选路时拒绝
//! 写入、只清能证明是 CC Switch 写的旧表、保留登录关闭时删掉的登录切回官方时还回来、
//! 切换中途 CLI 刷新了登录就停下、契约相同时不碰客户端文件、编辑器的全局改动。
//!
//! 已删除、不再锁的机制：统一会话桶的注入与剥离、回填（token 提回 auth、剥 MCP、保留
//! modelCatalog）、给用户表补 name / wire_api、接管的备份与恢复。

use std::path::PathBuf;
use std::time::SystemTime;

use serde_json::{json, Value};
use toml::Table;

use cc_switch_lib::{
    get_codex_auth_path, get_codex_config_path, update_settings, AppError, AppSettings, AppState,
    AppType, Provider, ProviderMeta, ProviderService,
};

use crate::support::{create_test_state, reset_test_fs, test_mutex};
use crate::util::{official, provider, seed_providers, write_home_file};

const RESERVED_IDS: &[&str] = &["openai", "ollama", "lmstudio"];
const BEDROCK_IDS: &[&str] = &["amazon-bedrock", "amazon-bedrock-runtime"];

const OAUTH_LOGIN: &str = r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"id-token","access_token":"access-token","refresh_token":"refresh-token","account_id":"account-1"},"last_refresh":"2026-09-01T00:00:00Z"}"#;

fn set_login_preservation(on: bool) {
    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: on,
        ..Default::default()
    })
    .expect("update settings");
}

fn codex(id: &str, key: Option<&str>, config: &str) -> Provider {
    let auth = match key {
        Some(key) => json!({ "OPENAI_API_KEY": key }),
        None => json!({}),
    };
    provider(id, json!({ "auth": auth, "config": config }), None)
}

fn with_meta(mut provider: Provider, meta: ProviderMeta) -> Provider {
    provider.meta = Some(meta);
    provider
}

fn relay_config(id: &str, base_url: &str) -> String {
    format!(
        "model_provider = \"{id}\"\nmodel = \"gpt-5\"\n\n[model_providers.{id}]\nname = \"Relay\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
    )
}

/// 当前是官方卡（跟随 live 登录），live 里有一份 ChatGPT 登录；再加上要切过去的卡。
fn setup(targets: Vec<Provider>, logged_in: bool) -> AppState {
    let state = create_test_state().expect("create test state");
    let mut providers = vec![official(
        "codex-official",
        json!({ "auth": {}, "config": "" }),
    )];
    providers.extend(targets);
    seed_providers(&state, &AppType::Codex, &providers, "codex-official");
    write_home_file(".codex/config.toml", "");
    if logged_in {
        write_home_file(".codex/auth.json", OAUTH_LOGIN);
    }
    state
}

fn live_config_text() -> String {
    std::fs::read_to_string(get_codex_config_path()).expect("read config.toml")
}

fn live_config() -> Table {
    toml::from_str(&live_config_text()).expect("config.toml parses")
}

fn live_auth() -> Option<String> {
    let path = get_codex_auth_path();
    path.is_file()
        .then(|| std::fs::read_to_string(path).expect("read auth.json"))
}

fn table<'a>(value: &'a toml::Value, what: &str) -> &'a Table {
    value
        .as_table()
        .unwrap_or_else(|| panic!("{what} is not a table"))
}

/// 活跃路由表：`model_provider` 指向的 `[model_providers.<id>]`。
fn route(doc: &Table) -> Option<(&str, &Table)> {
    let id = doc.get("model_provider")?.as_str()?;
    let providers = table(doc.get("model_providers")?, "model_providers");
    Some((id, table(providers.get(id)?, id)))
}

fn flag(table: &Table, key: &str) -> Option<bool> {
    table.get(key).and_then(toml::Value::as_bool)
}

/// Codex 0.149 加载时会逐张校验 provider 表，这些组合会让整份配置被拒。
fn assert_loadable(doc: &Table) {
    let Some(providers) = doc.get("model_providers") else {
        return;
    };
    for (id, value) in table(providers, "model_providers") {
        assert!(
            !RESERVED_IDS.contains(&id.as_str()),
            "reserved table [model_providers.{id}] makes Codex refuse the config:\n{doc:#?}"
        );
        let t = table(value, id);
        if !BEDROCK_IDS.contains(&id.as_str()) {
            assert!(t.get("aws").is_none(), "aws on a non-bedrock table {id}");
        }
        if t.contains_key("auth") {
            assert!(
                t.get("experimental_bearer_token").is_none()
                    && t.get("env_key").is_none()
                    && flag(t, "requires_openai_auth") != Some(true),
                "table {id} combines `auth` with another credential source:\n{t:#?}"
            );
        }
    }
}

/// Key 落在活跃路由表里，路由指向 `base_url`；顶层没有 token，也没有改道内置 openai 的地址。
fn assert_key_in_route(doc: &Table, base_url: &str, key: &str) {
    let (id, route) = route(doc).unwrap_or_else(|| panic!("no active route table:\n{doc:#?}"));
    assert!(!RESERVED_IDS.contains(&id), "route uses reserved id {id}");
    assert_eq!(
        route.get("base_url").and_then(toml::Value::as_str),
        Some(base_url),
        "route {id}: {route:#?}"
    );
    assert_eq!(
        route
            .get("experimental_bearer_token")
            .and_then(toml::Value::as_str),
        Some(key),
        "the key must travel in the active route table {id}: {route:#?}"
    );
    assert!(doc.get("experimental_bearer_token").is_none());
    assert!(doc.get("openai_base_url").is_none());
    assert_loadable(doc);
}

fn switch(state: &AppState, id: &str) -> Result<Vec<String>, AppError> {
    ProviderService::switch(state, AppType::Codex, id).map(|result| result.warnings)
}

/// 文件内容与 mtime；文件不存在时为 None。
type FileState = Option<(Vec<u8>, SystemTime)>;

#[derive(Debug, PartialEq)]
struct LiveSnapshot {
    files: Vec<(PathBuf, FileState)>,
    current: String,
    target_row: Value,
}

fn snapshot(state: &AppState, target: &str) -> LiveSnapshot {
    let files = [get_codex_config_path(), get_codex_auth_path()]
        .into_iter()
        .map(|path| {
            let content = std::fs::read(&path).ok().map(|bytes| {
                let mtime = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .expect("mtime");
                (bytes, mtime)
            });
            (path, content)
        })
        .collect();
    let target_row = state
        .db
        .get_provider_by_id(target, AppType::Codex.as_str())
        .expect("query provider")
        .expect("target exists")
        .settings_config;
    LiveSnapshot {
        files,
        current: ProviderService::current(state, AppType::Codex).expect("current"),
        target_row,
    }
}

/// CX-01 / 02 / 03
#[test]
fn third_party_key_lands_in_the_active_route_table() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for preserve in [false, true] {
        reset_test_fs();
        set_login_preservation(preserve);
        let state = setup(
            vec![codex(
                "relay",
                Some("sk-relay"),
                &relay_config("relay", "https://relay.example/v1"),
            )],
            true,
        );

        switch(&state, "relay").expect("switch to relay");

        assert_key_in_route(&live_config(), "https://relay.example/v1", "sk-relay");
        match live_auth() {
            Some(auth) => {
                assert!(preserve, "auth.json must be deleted, not rewritten: {auth}");
                assert_eq!(
                    auth, OAUTH_LOGIN,
                    "the preserved login must stay byte-identical"
                );
            }
            None => assert!(!preserve, "the preserved login was deleted"),
        }
    }
}

/// CX-04
#[test]
fn tables_declaring_their_own_auth_get_no_injected_key() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for preserve in [false, true] {
        reset_test_fs();
        set_login_preservation(preserve);
        let own_auth = |id: &str, extra: &str| {
            codex(
                id,
                Some("sk-stored"),
                &format!(
                    "model_provider = \"{id}\"\n\n[model_providers.{id}]\nname = \"Own\"\nbase_url = \"https://{id}.example/v1\"\nwire_api = \"responses\"\n{extra}"
                ),
            )
        };
        let state = setup(
            vec![
                own_auth("envkey", "env_key = \"RELAY_API_KEY\"\n"),
                own_auth(
                    "header",
                    "http_headers = { authorization = \"Bearer header-token\" }\n",
                ),
                own_auth(
                    "helper",
                    "\n[model_providers.helper.auth]\ncommand = \"print-token\"\n",
                ),
            ],
            false,
        );

        for id in ["envkey", "header", "helper"] {
            switch(&state, id).unwrap_or_else(|e| panic!("switch to {id}: {e:?}"));
            let text = live_config_text();
            assert!(
                !text.contains("sk-stored"),
                "{id} declares its own auth; the stored key must not be injected:\n{text}"
            );
            let doc = live_config();
            assert_loadable(&doc);
            let (_, route) = route(&doc).expect("route");
            assert_eq!(
                route.get("base_url").and_then(toml::Value::as_str),
                Some(format!("https://{id}.example/v1").as_str())
            );
            if id == "header" {
                assert_eq!(
                    route["http_headers"]["authorization"].as_str(),
                    Some("Bearer header-token"),
                    "the provider's own Authorization header is kept"
                );
            }
        }
    }
}

/// CX-05
#[test]
fn requires_openai_auth_true_still_gets_the_key() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    set_login_preservation(true);
    let state = setup(
        vec![codex(
            "bridge",
            Some("sk-bridge"),
            "model_provider = \"bridge\"\n\n[model_providers.bridge]\nname = \"Bridge\"\nbase_url = \"https://bridge.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nhttp_headers = { Authorization = \"Bearer header-token\" }\n",
        )],
        true,
    );

    switch(&state, "bridge").expect("switch to bridge");

    assert_key_in_route(&live_config(), "https://bridge.example/v1", "sk-bridge");
    assert_eq!(live_auth().as_deref(), Some(OAUTH_LOGIN));
}

/// CX-06 / 07
#[test]
fn refused_switches_have_no_side_effects() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let cases: [(&str, bool, Option<&str>, &str); 5] = [
        // 带 Key 但配置为空：Key 没有地方放。
        ("empty-config-with-key", false, Some("sk-x"), ""),
        ("empty-config-with-key", true, Some("sk-x"), ""),
        // 无 Key，顶层 openai_base_url 会把官方登录改道发到第三方。
        (
            "keyless-reroute",
            false,
            None,
            "model = \"gpt-5\"\nopenai_base_url = \"https://relay.example/v1\"\n",
        ),
        (
            "keyless-reroute",
            true,
            None,
            "model = \"gpt-5\"\nopenai_base_url = \"https://relay.example/v1\"\n",
        ),
        // 无 Key，requires_openai_auth = true 会让 Codex 用官方登录访问第三方。
        (
            "keyless-requires",
            true,
            None,
            "model_provider = \"relay\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nhttp_headers = { Authorization = \"Bearer h\" }\n",
        ),
    ];
    for (id, preserve, key, config) in cases {
        reset_test_fs();
        set_login_preservation(preserve);
        let state = setup(
            vec![
                codex(
                    "good",
                    Some("sk-good"),
                    &relay_config("good", "https://good.example/v1"),
                ),
                codex(id, key, config),
            ],
            true,
        );
        switch(&state, "good").expect("switch to good");
        let before = snapshot(&state, id);

        let result = switch(&state, id);

        assert!(
            result.is_err(),
            "{id} (preserve={preserve}) must be refused"
        );
        assert_eq!(
            before,
            snapshot(&state, id),
            "{id} (preserve={preserve}): a refused switch must not touch live, current or the row"
        );
    }
}

/// CX-09
#[test]
fn legacy_openai_base_url_reroute_never_reaches_live() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for preserve in [false, true] {
        reset_test_fs();
        set_login_preservation(preserve);
        let state = setup(
            vec![codex(
                "legacy",
                Some("sk-legacy"),
                "model = \"gpt-5\"\nopenai_base_url = \"https://relay.example/v1\"\n",
            )],
            true,
        );

        switch(&state, "legacy").expect("switch to legacy reroute card");

        let doc = live_config();
        assert_key_in_route(&doc, "https://relay.example/v1", "sk-legacy");
        assert_ne!(
            route(&doc)
                .expect("route")
                .1
                .get("name")
                .and_then(toml::Value::as_str),
            Some("OpenAI"),
            "a third-party table named OpenAI turns on Codex's OpenAI-only features"
        );
    }
}

/// CX-10
#[test]
fn normalization_never_overwrites_a_user_table() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    set_login_preservation(false);
    let user_table = "[model_providers.cc-switch]\nname = \"Mine\"\nbase_url = \"https://mine.example/v1\"\nwire_api = \"responses\"\nhttp_headers = { X-Tenant = \"t1\" }\n";
    let state = setup(
        vec![codex(
            "legacy",
            Some("sk-legacy"),
            &format!(
                "model = \"gpt-5\"\nopenai_base_url = \"https://relay.example/v1\"\n\n{user_table}"
            ),
        )],
        false,
    );
    // 用户的表同时在 live 里（重构后切换以 live 为底）。
    write_home_file(".codex/config.toml", user_table);

    switch(&state, "legacy").expect("switch to legacy reroute card");

    let doc = live_config();
    assert_key_in_route(&doc, "https://relay.example/v1", "sk-legacy");
    let mine = table(&doc["model_providers"]["cc-switch"], "cc-switch");
    let expected: Table = toml::from_str::<Table>(user_table).expect("parse user table")
        ["model_providers"]["cc-switch"]
        .as_table()
        .expect("user table")
        .clone();
    assert_eq!(
        mine, &expected,
        "the user's own table must survive untouched"
    );
}

/// CX-11
#[test]
fn inline_model_providers_keep_the_key_inside() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    set_login_preservation(false);
    let state = setup(
        vec![codex(
            "inline",
            Some("sk-inline"),
            "model_provider = \"custom\"\nmodel = \"gpt-5\"\nmodel_providers = { custom = { name = \"Inline\", base_url = \"https://inline.example/v1\", wire_api = \"responses\" } }\n",
        )],
        false,
    );

    switch(&state, "inline").expect("switch to inline card");

    assert_key_in_route(&live_config(), "https://inline.example/v1", "sk-inline");
}

/// CX-12
#[test]
fn reserved_ids_match_case_sensitively() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for id in ["OpenAI", "Ollama", "oss"] {
        reset_test_fs();
        set_login_preservation(false);
        let state = setup(
            vec![codex(
                "variant",
                Some("sk-variant"),
                &relay_config(id, "https://variant.example/v1"),
            )],
            false,
        );

        switch(&state, "variant").unwrap_or_else(|e| panic!("switch to {id}: {e:?}"));

        assert_key_in_route(&live_config(), "https://variant.example/v1", "sk-variant");
    }
}

/// CX-13
#[test]
fn stale_reserved_tables_never_reach_live() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for preserve in [false, true] {
        reset_test_fs();
        set_login_preservation(preserve);
        let state = setup(
            vec![codex(
                "stale",
                Some("sk-stale"),
                "model_provider = \"openai\"\nmodel = \"gpt-5\"\n\n[model_providers.openai]\nname = \"Stale\"\nbase_url = \"https://stale.example/v1\"\nwire_api = \"chat\"\n",
            )],
            true,
        );

        switch(&state, "stale").expect("switch to a card carrying a stale reserved table");

        assert_key_in_route(&live_config(), "https://stale.example/v1", "sk-stale");
        if preserve {
            assert_eq!(live_auth().as_deref(), Some(OAUTH_LOGIN));
        }
    }
}

/// CX-14
#[test]
fn requires_openai_auth_follows_login_preservation() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let header_card = codex(
        "header",
        None,
        "model_provider = \"header\"\n\n[model_providers.header]\nname = \"Header\"\nbase_url = \"https://header.example/v1\"\nwire_api = \"responses\"\nhttp_headers = { Authorization = \"Bearer h\" }\n",
    );

    // 开关关：登录被删，活跃带 Key 的表不能再要求官方登录（否则 TUI 卡在登录屏）。
    reset_test_fs();
    set_login_preservation(false);
    let state = setup(
        vec![
            codex(
                "claims-login",
                Some("sk-a"),
                "model_provider = \"relay\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
            ),
            header_card.clone(),
        ],
        true,
    );
    switch(&state, "claims-login").expect("switch");
    let doc = live_config();
    assert_eq!(
        flag(route(&doc).expect("route").1, "requires_openai_auth"),
        Some(false)
    );
    assert!(live_auth().is_none());
    switch(&state, "header").expect("switch to header card");
    let doc = live_config();
    assert_ne!(
        flag(route(&doc).expect("route").1, "requires_openai_auth"),
        Some(true)
    );

    // 开关开且已登录：带 Key 的表要显示账号状态、让 Codex 继续刷新 token。
    reset_test_fs();
    set_login_preservation(true);
    let state = setup(
        vec![
            codex(
                "silent",
                Some("sk-b"),
                &relay_config("relay", "https://relay.example/v1"),
            ),
            header_card,
        ],
        true,
    );
    switch(&state, "silent").expect("switch");
    let doc = live_config();
    assert_eq!(
        flag(route(&doc).expect("route").1, "requires_openai_auth"),
        Some(true)
    );
    switch(&state, "header").expect("switch to header card");
    let doc = live_config();
    assert_ne!(
        flag(route(&doc).expect("route").1, "requires_openai_auth"),
        Some(true)
    );
}

/// CX-15：结果可以是拒绝，也可以是放行，但放行时路由表绝不要求官方登录。
#[test]
fn proxy_injected_oauth_cards_never_carry_the_official_login() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    for provider_type in ["xai_oauth", "github_copilot"] {
        reset_test_fs();
        set_login_preservation(true);
        let card = with_meta(
            codex(
                "oauth-card",
                Some(""),
                "model_provider = \"custom\"\nmodel = \"grok-4.5\"\n\n[model_providers.custom]\nname = \"xAI\"\nbase_url = \"https://api.x.ai/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
            ),
            ProviderMeta {
                provider_type: Some(provider_type.to_string()),
                ..Default::default()
            },
        );
        let state = setup(vec![card], true);
        let before = snapshot(&state, "oauth-card");

        match switch(&state, "oauth-card") {
            Ok(_) => {
                let doc = live_config();
                let (_, route) = route(&doc).expect("route");
                assert_ne!(
                    flag(route, "requires_openai_auth"),
                    Some(true),
                    "{provider_type}: the official ChatGPT login would be sent to api.x.ai"
                );
                assert_loadable(&doc);
            }
            Err(_) => assert_eq!(before, snapshot(&state, "oauth-card")),
        }
    }
}

/// CX-23
#[test]
fn model_catalog_pointer_ownership() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    set_login_preservation(false);
    let catalog = json!({ "models": [{ "model": "relay-model" }] });
    let with_catalog = |id: &str, extra: &str| {
        let mut card = codex(
            id,
            Some("sk-c"),
            &format!(
                "{extra}{}",
                relay_config("relay", "https://relay.example/v1")
            ),
        );
        card.settings_config["modelCatalog"] = catalog.clone();
        card
    };
    let state = setup(
        vec![
            with_catalog("owned", ""),
            with_catalog(
                "user-pointer",
                "model_catalog_json = \"/Users/me/my-catalog.json\"\n",
            ),
            codex(
                "plain",
                Some("sk-p"),
                &relay_config("relay", "https://relay.example/v1"),
            ),
        ],
        false,
    );

    switch(&state, "owned").expect("switch to owned");
    assert_eq!(
        live_config()["model_catalog_json"].as_str(),
        Some("cc-switch-model-catalog.json")
    );
    assert!(get_codex_config_path()
        .with_file_name("cc-switch-model-catalog.json")
        .exists());

    switch(&state, "plain").expect("switch to plain");
    assert!(
        live_config().get("model_catalog_json").is_none(),
        "CC Switch removes its own pointer when the target has no catalog"
    );

    switch(&state, "user-pointer").expect("switch to user pointer");
    assert_eq!(
        live_config()["model_catalog_json"].as_str(),
        Some("/Users/me/my-catalog.json"),
        "a user-managed catalog pointer is never claimed or removed"
    );
}

/// CX-24
#[test]
fn web_search_sentinel_is_the_only_value_removed() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    set_login_preservation(false);
    let anthropic = with_meta(
        codex(
            "anthropic",
            Some("sk-a"),
            &relay_config("relay", "https://relay.example/v1"),
        ),
        ProviderMeta {
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        },
    );
    let state = setup(
        vec![
            anthropic,
            codex(
                "plain",
                Some("sk-p"),
                &relay_config("relay", "https://relay.example/v1"),
            ),
            codex(
                "user-value",
                Some("sk-u"),
                &format!(
                    "web_search = \"live\"\n{}",
                    relay_config("relay", "https://relay.example/v1")
                ),
            ),
        ],
        false,
    );

    switch(&state, "anthropic").expect("switch to anthropic");
    assert_eq!(live_config()["web_search"].as_str(), Some("disabled"));

    switch(&state, "plain").expect("switch to plain");
    assert!(live_config().get("web_search").is_none());

    switch(&state, "user-value").expect("switch to user value");
    assert_eq!(live_config()["web_search"].as_str(), Some("live"));
}

/// CX-25
#[test]
fn auth_cleanup_failure_is_a_warning() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    set_login_preservation(false);
    let state = setup(
        vec![codex(
            "relay",
            Some("sk-relay"),
            &relay_config("relay", "https://relay.example/v1"),
        )],
        false,
    );
    // auth.json 是个目录：删除必然失败。
    std::fs::create_dir_all(get_codex_auth_path()).expect("make auth.json a directory");

    let warnings = switch(&state, "relay").expect("the switch itself succeeds");

    assert!(
        warnings
            .iter()
            .any(|w| w.contains("codex_auth_cleanup_failed")),
        "warnings: {warnings:?}"
    );
    assert_key_in_route(&live_config(), "https://relay.example/v1", "sk-relay");
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).expect("current"),
        "relay"
    );
}

async fn use_ephemeral_proxy_port(state: &AppState) {
    let mut config = state.db.get_proxy_config().await.expect("proxy config");
    config.listen_port = 0;
    state
        .db
        .update_proxy_config(config)
        .await
        .expect("use ephemeral proxy port");
}

const NEW_LOGIN: &str = r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"id-2","access_token":"access-2","refresh_token":"refresh-2","account_id":"account-2"},"last_refresh":"2026-09-20T00:00:00Z"}"#;

/// 当前是 `current` 时进入代理，用户在 Codex 里改成 `after_login`（`None` 为登出），
/// 退出代理后 auth.json 应当仍是用户此刻的状态。
async fn check_proxy_exit_keeps_login(current: &str, after_login: Option<&str>) {
    reset_test_fs();
    set_login_preservation(true);
    let state = setup(
        vec![codex(
            "relay",
            Some("sk-relay"),
            &relay_config("relay", "https://relay.example/v1"),
        )],
        true,
    );
    if current == "relay" {
        switch(&state, "relay").expect("switch to relay");
    }
    use_ephemeral_proxy_port(&state).await;
    cc_switch_lib::mode::controller::enter(&state, &AppType::Codex)
        .await
        .expect("enter proxy");
    if current == "codex-official" {
        // 进入代理不回填官方卡（直连切走时的回填是另一回事，随 Codex 只写关键字段那一步去掉）。
        let official = state
            .db
            .get_provider_by_id(current, AppType::Codex.as_str())
            .expect("read official row")
            .expect("official row");
        assert!(
            !official
                .settings_config
                .to_string()
                .contains("refresh-token"),
            "entering the proxy must not store the ChatGPT login in the official row"
        );
    }

    match after_login {
        None => std::fs::remove_file(get_codex_auth_path()).expect("codex logout"),
        Some(login) => std::fs::write(get_codex_auth_path(), login).expect("codex login"),
    }

    cc_switch_lib::mode::controller::exit(&state, &AppType::Codex)
        .await
        .expect("exit proxy");

    assert_eq!(
        live_auth().as_deref(),
        after_login,
        "current={current}: exiting the proxy must keep the login state the user has now"
    );
}

/// CX-22：进入代理后用户在 Codex 里登出或换了账号，退出代理时保持用户此刻的状态。
#[tokio::test(flavor = "current_thread")]
#[allow(
    clippy::await_holding_lock,
    reason = "the test HOME and settings are process-global; the guard must span the async takeover calls"
)]
async fn exiting_proxy_keeps_the_login_the_user_has_now() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    check_proxy_exit_keeps_login("codex-official", None).await;
    check_proxy_exit_keeps_login("codex-official", Some(NEW_LOGIN)).await;
    check_proxy_exit_keeps_login("relay", Some(NEW_LOGIN)).await;
}

/// CX-22：当前是第三方卡、保留登录开关打开时，代理期间登出，退出代理后仍是登出状态（退出
/// 代理按直连供应商写回，不回放进入时的快照）。
#[tokio::test(flavor = "current_thread")]
#[allow(
    clippy::await_holding_lock,
    reason = "the test HOME and settings are process-global; the guard must span the async takeover calls"
)]
async fn exiting_proxy_after_logout_on_a_third_party_route_stays_logged_out() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    check_proxy_exit_keeps_login("relay", None).await;
}

//! 用各家 CLI 的真实输出跑解析（样例和抓取方法见 `fixtures/README.md`）。
//!
//! 插件页靠这些 `--json` 输出工作，CLI 升级改了格式，这里先报出来。断言只挑解析依赖的
//! 字段：改名、换结构、去掉字段都会让它们失败。升级 CLI 后按 README 重新抓一份放进新的
//! `<cli>-<版本>/` 目录，把下面的路径指过去。

use serde_json::Value;

use super::*;

macro_rules! fixture {
    ($path:literal) => {
        include_str!(concat!("fixtures/", $path))
    };
}

fn json(text: &str) -> Value {
    parse_json_payload(text).expect("fixture is not JSON")
}

fn ok(stdout: &str) -> CliOutput {
    CliOutput {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn failed(stdout: &str, stderr: &str) -> CliOutput {
    CliOutput {
        success: false,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

fn ids(entries: &[PluginEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.id.as_str()).collect()
}

// ───────────────────────────── Claude Code 2.1.318 ─────────────────────────────

#[test]
fn claude_installed_list() {
    let listing = parse_claude_listing(&json(fixture!("claude-2.1.318/plugin-list.json")));
    assert_eq!(ids(&listing.installed), ["hello@ccs-test"]);
    let hello = &listing.installed[0];
    assert_eq!(hello.name, "hello");
    assert_eq!(hello.marketplace, "ccs-test");
    assert_eq!(hello.version.as_deref(), Some("0.1.0"));
    assert_eq!(hello.scope.as_deref(), Some("user"));
    assert!(hello.installed && !hello.enabled && hello.manageable && hello.removable);
    assert!(hello.install_path.is_some());
    assert!(hello.updated_at.is_some());
}

#[test]
fn claude_catalog_list() {
    let listing =
        parse_claude_listing(&json(fixture!("claude-2.1.318/plugin-list-available.json")));
    assert_eq!(ids(&listing.installed), ["hello@ccs-test"]);
    assert!(!listing.installed[0].enabled);
    assert_eq!(
        ids(&listing.available),
        [
            "cmdplug@ccs-test",
            "superpowers@anthropic-plugin-directory",
            "frontend-design@anthropic-plugin-directory",
            "design@anthropic-plugin-directory",
        ]
    );
    let superpowers = &listing.available[1];
    assert_eq!(superpowers.name, "superpowers");
    assert_eq!(superpowers.marketplace, "anthropic-plugin-directory");
    assert_eq!(superpowers.version.as_deref(), Some("6.4.2"));
    assert!(superpowers.description.is_some());
    assert!(listing
        .available
        .iter()
        .all(|e| e.installable && !e.installed));
}

#[test]
fn claude_marketplace_list() {
    let markets =
        parse_claude_marketplaces(&json(fixture!("claude-2.1.318/marketplace-list.json")));
    assert_eq!(markets.len(), 1);
    assert_eq!(markets[0].name, "ccs-test");
    assert_eq!(markets[0].source.as_deref(), Some(r"C:\fixture\mkt"));
    assert_eq!(markets[0].location.as_deref(), Some(r"C:\fixture\mkt"));
    assert!(markets[0].removable);
}

#[test]
fn claude_action_results() {
    for stdout in [
        fixture!("claude-2.1.318/install-ok.json"),
        fixture!("claude-2.1.318/disable.json"),
        fixture!("claude-2.1.318/marketplace-update.json"),
    ] {
        let value = claude_action_result(&ok(stdout)).expect(stdout);
        assert!(ok_outcome(&value).message.is_some(), "{stdout}");
    }

    let update = claude_action_result(&ok(fixture!("claude-2.1.318/update-up-to-date.json")))
        .map(|v| claude_update_outcome(&v))
        .unwrap();
    assert!(!update.changed);

    let not_found = claude_action_result(&failed(
        fixture!("claude-2.1.318/install-not-found.json"),
        "",
    ))
    .unwrap_err();
    assert!(
        not_found.starts_with("Plugin \"nope\" not found"),
        "{not_found}"
    );
}

/// 不传 `-y`、stdin 不是终端时，要确认命令的插件立刻失败，换成前端认得的错误码。
#[test]
fn claude_install_that_needs_confirmation_fails_fast() {
    let err = claude_action_result(&failed(
        fixture!("claude-2.1.318/install-needs-confirm.stdout"),
        "",
    ))
    .unwrap_err();
    assert_eq!(err, NEEDS_TERMINAL_CONFIRM);
}

// ───────────────────────────── Codex 0.167.0 ─────────────────────────────

#[test]
fn codex_installed_list() {
    let listing = parse_codex_listing(&json(fixture!("codex-0.167.0/plugin-list.json")));
    assert_eq!(ids(&listing.installed), ["hello@ccs-test"]);
    let hello = &listing.installed[0];
    assert_eq!(hello.name, "hello");
    assert_eq!(hello.marketplace, "ccs-test");
    assert_eq!(hello.version.as_deref(), Some("0.1.0"));
    assert!(hello.installed && !hello.enabled && hello.installable);
    assert!(listing.available.is_empty());

    // 启停状态和 config.toml 里的开关是同一个
    let config = fixture!("codex-0.167.0/config.toml");
    assert_eq!(
        codex_plugin_enabled_in_text(config, "hello@ccs-test"),
        Some(false)
    );
}

#[test]
fn codex_catalog_list() {
    let listing = parse_codex_listing(&json(fixture!("codex-0.167.0/plugin-list-available.json")));
    assert_eq!(ids(&listing.installed), ["hello@ccs-test"]);
    assert_eq!(ids(&listing.available), ["locked@ccs-test"]);
    assert!(
        !listing.available[0].installable,
        "installPolicy NOT_AVAILABLE"
    );
    assert!(!listing.available[0].installed);
}

#[test]
fn codex_marketplace_list_and_config() {
    let config = fixture!("codex-0.167.0/config.toml");
    let markets = parse_codex_marketplaces(
        &json(fixture!("codex-0.167.0/marketplace-list.json")),
        config,
    );
    assert_eq!(markets.len(), 1);
    assert_eq!(markets[0].name, "ccs-test");
    assert!(markets[0].removable);
    assert_eq!(markets[0].source.as_deref(), Some(r"C:\fixture\cmkt"));
    assert_eq!(markets[0].location.as_deref(), Some(r"C:\fixture\cmkt"));
    assert!(!codex_marketplace_needs_upgrade(config, "ccs-test").unwrap());
}

#[test]
fn codex_action_results() {
    let added = codex_action_result(&ok(fixture!("codex-0.167.0/marketplace-add.json")))
        .map(|v| codex_marketplace_add_outcome(&v))
        .unwrap();
    assert!(added.changed);
    assert_eq!(added.message.as_deref(), Some("ccs-test"));

    let again = codex_action_result(&ok(fixture!("codex-0.167.0/marketplace-add-again.json")))
        .map(|v| codex_marketplace_add_outcome(&v))
        .unwrap();
    assert!(!again.changed);

    for stdout in [
        fixture!("codex-0.167.0/add.json"),
        fixture!("codex-0.167.0/remove.json"),
        fixture!("codex-0.167.0/marketplace-upgrade.json"),
    ] {
        assert!(codex_action_result(&ok(stdout)).unwrap().is_object());
    }

    let err = codex_action_result(&failed("", fixture!("codex-0.167.0/add-not-found.stderr")))
        .unwrap_err();
    assert_eq!(err, "plugin `nope` was not found in marketplace `ccs-test`");
}

// ───────────────────────────── Grok Build 1.0.70 ─────────────────────────────

#[test]
fn grok_lists_read_status_and_config_toggle() {
    let config = fixture!("grok-1.0.70/config.toml");
    for text in [
        fixture!("grok-1.0.70/plugin-list.json"),
        fixture!("grok-1.0.70/plugin-list-available.json"),
    ] {
        let listing = grok::parse_listing(&json(text), config);
        assert_eq!(ids(&listing.installed), ["hello"]);
        let hello = &listing.installed[0];
        assert_eq!(hello.marketplace, "gmkt");
        assert_eq!(hello.version.as_deref(), Some("0.1.0"));
        assert!(hello.installed && !hello.enabled, "disabled in config.toml");
        assert!(hello.install_path.is_some());
        assert!(listing.available.is_empty());
    }
}

#[test]
fn grok_marketplace_list() {
    let markets = grok::parse_marketplaces(&json(fixture!("grok-1.0.70/marketplace-list.json")));
    assert_eq!(markets.len(), 1);
    assert_eq!(markets[0].name, "gmkt");
    assert_eq!(markets[0].source.as_deref(), Some(r"C:\fixture\gmkt"));
    assert_eq!(markets[0].location.as_deref(), Some(r"C:\fixture\gmkt"));
}

#[test]
fn grok_update_output() {
    assert!(!grok::update_changed(fixture!(
        "grok-1.0.70/update-up-to-date.txt"
    )));
    assert!(grok::update_changed(fixture!(
        "grok-1.0.70/update-changed.txt"
    )));
}

// ───────────────────────────── MiniMax Code 0.7.4 ─────────────────────────────

#[test]
fn mcode_lists() {
    let installed = mcode::parse_listing(&json(fixture!("mcode-0.7.4/plugin-list.json")));
    assert!(installed.installed.is_empty() && installed.available.is_empty());

    let catalog = mcode::parse_listing(&json(fixture!("mcode-0.7.4/plugin-list-available.json")));
    assert!(catalog.installed.is_empty());
    assert_eq!(
        ids(&catalog.available),
        [
            "computer-use@official",
            "browser-use@official",
            "minimax-bot@official",
            "email-workbench@official",
        ]
    );
    let computer = &catalog.available[0];
    assert_eq!(computer.name, "computer-use");
    assert_eq!(computer.marketplace, "official");
    assert_eq!(computer.display_name.as_deref(), Some("电脑操控"));
    assert_eq!(computer.version.as_deref(), Some("1.1.1"));
    assert!(computer.description.is_some());
}

#[test]
fn mcode_failures() {
    assert_eq!(
        failure_detail(&failed(
            "",
            fixture!("mcode-0.7.4/add-auth-required.stderr")
        )),
        "PLUGIN_AUTH_REQUIRED"
    );
    assert_eq!(
        failure_detail(&failed("", fixture!("mcode-0.7.4/add-not-found.stderr"))),
        "Plugin not found: nope@official"
    );
}

// ───────────────────────────── OpenClaw 2026.11.1 ─────────────────────────────

#[test]
fn openclaw_installed_list() {
    let installed =
        openclaw::parse_installed(&json(fixture!("openclaw-2026.11.1/plugins-list.json")));
    assert_eq!(ids(&installed), ["a2a", "anthropic", "airtable"]);
    let [a2a, anthropic, airtable] = installed.as_slice() else {
        unreachable!()
    };
    assert!(!a2a.enabled && !a2a.removable, "bundled is toggle-only");
    assert_eq!(a2a.display_name.as_deref(), Some("A2A"));
    assert!(anthropic.enabled && !anthropic.removable);
    assert!(airtable.enabled && airtable.removable, "origin global");
    assert_eq!(airtable.version.as_deref(), Some("1.0.1"));
    assert!(airtable.install_path.is_some());
}

#[test]
fn openclaw_catalog_and_refresh() {
    let catalog = openclaw::parse_catalog(&json(fixture!(
        "openclaw-2026.11.1/marketplace-entries.json"
    )));
    assert_eq!(catalog.len(), 4);
    let airtable = catalog
        .iter()
        .find(|e| e.id == "@openclaw/airtable")
        .unwrap();
    assert_eq!(airtable.display_name.as_deref(), Some("Airtable"));
    assert_eq!(airtable.version.as_deref(), Some("1.0.1"));
    assert_eq!(airtable.marketplace, "clawhub");
    // 目录里的包名装好后对应已安装列表里的 `airtable`
    assert_eq!(airtable.installed_id.as_deref(), Some("airtable"));

    assert!(json(fixture!("openclaw-2026.11.1/marketplace-refresh.json")).is_object());
}

#[test]
fn openclaw_update_output() {
    assert!(!openclaw::update_changed(fixture!(
        "openclaw-2026.11.1/update-up-to-date.txt"
    )));
}

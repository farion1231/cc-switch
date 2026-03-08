use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cc_switch_core::{AppSettings, AppState, Database, InstalledSkill, SkillApps};
use rusqlite::{params, Connection};
use serde_json::Value;
use serial_test::serial;
use tempfile::tempdir;

fn run_cli(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cc-switch"))
        .args(args)
        .env("CC_SWITCH_TEST_HOME", home)
        .output()
        .expect("cli command should run")
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be utf-8")
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be utf-8")
}

fn database_path(home: &Path) -> PathBuf {
    home.join(".cc-switch").join("cc-switch.db")
}

fn claude_settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

fn claude_prompt_path(home: &Path) -> PathBuf {
    home.join(".claude").join("CLAUDE.md")
}

fn claude_mcp_path(home: &Path) -> PathBuf {
    home.join(".claude.json")
}

fn codex_config_path(home: &Path) -> PathBuf {
    home.join(".codex").join("config.toml")
}

fn opencode_config_path(home: &Path) -> PathBuf {
    home.join(".config").join("opencode").join("opencode.json")
}

fn skill_ssot_dir(home: &Path, directory: &str) -> PathBuf {
    home.join(".cc-switch").join("skills").join(directory)
}

fn claude_skill_dir(home: &Path, directory: &str) -> PathBuf {
    home.join(".claude").join("skills").join(directory)
}

fn exists_or_symlink(path: &Path) -> bool {
    path.exists()
        || path
            .symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink())
}

fn with_seeded_state<T>(home: &Path, f: impl FnOnce(&AppState) -> T) -> T {
    let previous = env::var("CC_SWITCH_TEST_HOME").ok();
    env::set_var("CC_SWITCH_TEST_HOME", home);
    cc_switch_core::settings::update_settings(AppSettings::default()).expect("default settings");
    let state = AppState::new(Database::new().expect("file database"));
    let result = f(&state);
    match previous {
        Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
        None => env::remove_var("CC_SWITCH_TEST_HOME"),
    }
    result
}

fn ensure_persisted_state(home: &Path) {
    with_seeded_state(home, |_state| ());
}

fn seed_installed_skill(home: &Path, id: &str, directory: &str) {
    with_seeded_state(home, |state| {
        let ssot_dir = skill_ssot_dir(home, directory);
        fs::create_dir_all(&ssot_dir).expect("ssot skill dir");
        fs::write(
            ssot_dir.join("SKILL.md"),
            "---\nname: Demo Skill\ndescription: seeded skill\n---\n",
        )
        .expect("write skill");
        state
            .db
            .save_skill(&InstalledSkill {
                id: id.to_string(),
                name: "Demo Skill".to_string(),
                description: Some("seeded skill".to_string()),
                directory: directory.to_string(),
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps: SkillApps::default(),
                installed_at: 1,
            })
            .expect("save skill");
    });
}

fn insert_usage_log(
    home: &Path,
    request_id: &str,
    app_type: &str,
    provider_id: &str,
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
    total_cost: &str,
    created_at: i64,
) {
    ensure_persisted_state(home);
    let conn = Connection::open(database_path(home)).expect("open database");
    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model,
            input_tokens, output_tokens, total_cost_usd,
            latency_ms, status_code, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            request_id,
            provider_id,
            app_type,
            model,
            input_tokens,
            output_tokens,
            total_cost,
            120i64,
            200i64,
            created_at,
        ],
    )
    .expect("insert usage log");
}

#[test]
#[serial]
fn quiet_mode_suppresses_success_output_and_config_get_returns_json() {
    let temp = tempdir().expect("tempdir");

    let set_output = run_cli(temp.path(), &["--quiet", "config", "set", "language", "zh"]);
    assert!(
        set_output.status.success(),
        "stderr: {}",
        stderr_text(&set_output)
    );
    assert!(stdout_text(&set_output).trim().is_empty());

    let get_output = run_cli(
        temp.path(),
        &["--format", "json", "config", "get", "language"],
    );
    assert!(
        get_output.status.success(),
        "stderr: {}",
        stderr_text(&get_output)
    );

    let value: Value =
        serde_json::from_slice(&get_output.stdout).expect("config get should return json");
    assert_eq!(value.get("language").and_then(Value::as_str), Some("zh"));
}

#[test]
#[serial]
fn quiet_mode_overrides_verbose_output_for_value_commands() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(
        temp.path(),
        &["--quiet", "--verbose", "--format", "json", "config", "path"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));
    assert!(stdout_text(&output).trim().is_empty());
    assert!(stderr_text(&output).trim().is_empty());
}

#[test]
#[serial]
fn prompt_lifecycle_round_trips_through_cli_and_live_file() {
    let temp = tempdir().expect("tempdir");
    let review_file = temp.path().join("review.txt");
    let draft_file = temp.path().join("draft.txt");
    fs::write(&review_file, "Review the diff carefully.\n").expect("write review prompt");
    fs::write(&draft_file, "Draft prompt.\n").expect("write draft prompt");

    let add_review = run_cli(
        temp.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "review",
            "--file",
            review_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_review.status.success(),
        "stderr: {}",
        stderr_text(&add_review)
    );

    let add_draft = run_cli(
        temp.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "draft",
            "--file",
            draft_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_draft.status.success(),
        "stderr: {}",
        stderr_text(&add_draft)
    );

    let enable_output = run_cli(
        temp.path(),
        &["prompt", "enable", "review", "--app", "claude"],
    );
    assert!(
        enable_output.status.success(),
        "stderr: {}",
        stderr_text(&enable_output)
    );
    let live_prompt = fs::read_to_string(claude_prompt_path(temp.path())).expect("live prompt");
    assert_eq!(live_prompt, "Review the diff carefully.\n");

    let delete_without_yes = run_cli(
        temp.path(),
        &["prompt", "delete", "draft", "--app", "claude"],
    );
    assert!(!delete_without_yes.status.success(), "delete should fail");
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_with_yes = run_cli(
        temp.path(),
        &["prompt", "delete", "draft", "--app", "claude", "--yes"],
    );
    assert!(
        delete_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&delete_with_yes)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "prompt", "list", "--app", "claude"],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let prompts: Value =
        serde_json::from_slice(&list_output.stdout).expect("prompt list should return json");
    assert!(prompts.get("review").is_some());
    assert!(prompts.get("draft").is_none());
}

#[test]
#[serial]
fn prompt_import_reads_live_file_on_first_launch() {
    let temp = tempdir().expect("tempdir");
    let live_path = claude_prompt_path(temp.path());
    fs::create_dir_all(live_path.parent().expect("claude dir")).expect("claude dir");
    fs::write(&live_path, "Seed live prompt.\n").expect("write live prompt");

    let import_output = run_cli(temp.path(), &["prompt", "import", "--app", "claude"]);
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "prompt", "list", "--app", "claude"],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let prompts: Value =
        serde_json::from_slice(&list_output.stdout).expect("prompt list should return json");
    let prompt = prompts
        .as_object()
        .and_then(|items| items.values().next())
        .expect("imported prompt should exist");
    assert_eq!(
        prompt.get("content").and_then(Value::as_str),
        Some("Seed live prompt.\n")
    );
    assert_eq!(prompt.get("enabled").and_then(Value::as_bool), Some(true));
}

#[test]
#[serial]
fn mcp_add_from_json_edit_and_delete_round_trip_with_live_sync() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".claude")).expect("claude dir");
    fs::create_dir_all(temp.path().join(".codex")).expect("codex dir");
    let mcp_file = temp.path().join("mcp.json");
    fs::write(
        &mcp_file,
        r#"{"type":"stdio","command":"npx","args":["foo","bar"]}"#,
    )
    .expect("write mcp json");

    let add_output = run_cli(
        temp.path(),
        &[
            "--quiet",
            "mcp",
            "add",
            "--id",
            "demo",
            "--from-json",
            mcp_file.to_str().expect("utf-8 path"),
            "--apps",
            "claude,codex",
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );
    assert!(stdout_text(&add_output).trim().is_empty());

    let claude_mcp = fs::read_to_string(claude_mcp_path(temp.path())).expect("claude mcp");
    assert!(claude_mcp.contains("\"demo\""));
    let codex_config = fs::read_to_string(codex_config_path(temp.path())).expect("codex config");
    assert!(codex_config.contains("demo"));

    let edit_output = run_cli(
        temp.path(),
        &["mcp", "edit", "demo", "--disable-app", "codex"],
    );
    assert!(
        edit_output.status.success(),
        "stderr: {}",
        stderr_text(&edit_output)
    );

    let list_output = run_cli(temp.path(), &["--format", "json", "mcp", "list"]);
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("mcp list should return json");
    let apps = value
        .get("demo")
        .and_then(|item| item.get("apps"))
        .expect("apps should exist");
    assert_eq!(apps.get("claude").and_then(Value::as_bool), Some(true));
    assert_eq!(apps.get("codex").and_then(Value::as_bool), Some(false));

    let codex_after_edit =
        fs::read_to_string(codex_config_path(temp.path())).expect("codex config");
    assert!(!codex_after_edit.contains("demo"));

    let delete_without_yes = run_cli(temp.path(), &["mcp", "delete", "demo"]);
    assert!(!delete_without_yes.status.success(), "delete should fail");
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_with_yes = run_cli(temp.path(), &["mcp", "delete", "demo", "--yes"]);
    assert!(
        delete_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&delete_with_yes)
    );

    let list_after_delete = run_cli(temp.path(), &["--format", "json", "mcp", "list"]);
    let servers: Value =
        serde_json::from_slice(&list_after_delete.stdout).expect("mcp list should return json");
    assert!(servers.get("demo").is_none());
    let claude_mcp_after_delete =
        fs::read_to_string(claude_mcp_path(temp.path())).expect("claude mcp");
    assert!(!claude_mcp_after_delete.contains("\"demo\""));
}

#[test]
#[serial]
fn mcp_import_reads_existing_live_configs() {
    let temp = tempdir().expect("tempdir");
    fs::write(
        claude_mcp_path(temp.path()),
        r#"{"mcpServers":{"from-claude":{"type":"stdio","command":"npx","args":["claude"]}}}"#,
    )
    .expect("write claude mcp");
    fs::create_dir_all(temp.path().join(".codex")).expect("codex dir");
    fs::write(
        codex_config_path(temp.path()),
        r#"[mcp_servers.from_codex]
type = "stdio"
command = "npx"
args = ["codex"]
"#,
    )
    .expect("write codex config");

    let import_output = run_cli(temp.path(), &["mcp", "import"]);
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let list_output = run_cli(temp.path(), &["--format", "json", "mcp", "list"]);
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("mcp list should return json");
    assert_eq!(
        value
            .get("from-claude")
            .and_then(|item| item.get("apps"))
            .and_then(|apps| apps.get("claude"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .get("from_codex")
            .and_then(|item| item.get("apps"))
            .and_then(|apps| apps.get("codex"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
#[serial]
fn provider_add_from_json_switch_and_delete_round_trip() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://old.example","ANTHROPIC_AUTH_TOKEN":"sk-old"}}"#,
    )
    .expect("write provider json");

    let import_output = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Imported Router",
            "--base-url",
            "https://new.example",
            "--api-key",
            "sk-new",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let show_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "show",
            "imported-router",
            "--app",
            "claude",
        ],
    );
    assert!(
        show_output.status.success(),
        "stderr: {}",
        stderr_text(&show_output)
    );
    let provider: Value =
        serde_json::from_slice(&show_output.stdout).expect("provider show should return json");
    assert_eq!(
        provider
            .get("settingsConfig")
            .and_then(|config| config.get("env"))
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(Value::as_str),
        Some("https://new.example")
    );
    assert_eq!(
        provider
            .get("settingsConfig")
            .and_then(|config| config.get("env"))
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(Value::as_str),
        Some("sk-new")
    );

    let switch_output = run_cli(
        temp.path(),
        &["provider", "switch", "imported-router", "--app", "claude"],
    );
    assert!(
        switch_output.status.success(),
        "stderr: {}",
        stderr_text(&switch_output)
    );
    let live_settings =
        fs::read_to_string(claude_settings_path(temp.path())).expect("claude settings");
    assert!(live_settings.contains("https://new.example"));
    assert!(live_settings.contains("sk-new"));

    let add_delete_target = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Delete Me",
            "--base-url",
            "https://delete.example",
            "--api-key",
            "sk-delete",
        ],
    );
    assert!(
        add_delete_target.status.success(),
        "stderr: {}",
        stderr_text(&add_delete_target)
    );

    let delete_without_yes = run_cli(
        temp.path(),
        &["provider", "delete", "delete-me", "--app", "claude"],
    );
    assert!(!delete_without_yes.status.success(), "delete should fail");
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_with_yes = run_cli(
        temp.path(),
        &[
            "provider",
            "delete",
            "delete-me",
            "--app",
            "claude",
            "--yes",
        ],
    );
    assert!(
        delete_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&delete_with_yes)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    let providers: Value =
        serde_json::from_slice(&list_output.stdout).expect("provider list should return json");
    assert!(providers.get("imported-router").is_some());
    assert!(providers.get("delete-me").is_none());
}

#[test]
#[serial]
fn provider_usage_without_script_falls_back_to_local_summary() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://usage.example","ANTHROPIC_AUTH_TOKEN":"sk-usage"}}"#,
    )
    .expect("write provider json");

    let add_output = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Local Usage Provider",
            "--base-url",
            "https://usage.example",
            "--api-key",
            "sk-usage",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );

    insert_usage_log(
        temp.path(),
        "req-provider-usage",
        "claude",
        "local-usage-provider",
        "claude-haiku",
        12,
        8,
        "0.0015",
        chrono::Utc::now().timestamp(),
    );

    let usage_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "usage",
            "local-usage-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        usage_output.status.success(),
        "stderr: {}",
        stderr_text(&usage_output)
    );
    assert!(stderr_text(&usage_output).contains("local proxy usage"));

    let value: Value =
        serde_json::from_slice(&usage_output.stdout).expect("provider usage should return json");
    assert_eq!(value.get("totalRequests").and_then(Value::as_u64), Some(1));
    assert_eq!(value.get("totalTokens").and_then(Value::as_u64), Some(20));
    assert_eq!(
        value
            .get("requestsByModel")
            .and_then(|items| items.get("claude-haiku"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[test]
#[serial]
fn provider_duplicate_sort_order_and_read_live_round_trip() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://dup.example","ANTHROPIC_AUTH_TOKEN":"sk-dup"}}"#,
    )
    .expect("write provider json");

    let add_output = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Primary Provider",
            "--base-url",
            "https://dup.example",
            "--api-key",
            "sk-dup",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );

    let duplicate_output = run_cli(
        temp.path(),
        &[
            "provider",
            "duplicate",
            "primary-provider",
            "--app",
            "claude",
            "--name",
            "Primary Provider Backup",
        ],
    );
    assert!(
        duplicate_output.status.success(),
        "stderr: {}",
        stderr_text(&duplicate_output)
    );

    let sort_output = run_cli(
        temp.path(),
        &[
            "provider",
            "sort-order",
            "primary-provider-backup",
            "--app",
            "claude",
            "--index",
            "7",
        ],
    );
    assert!(
        sort_output.status.success(),
        "stderr: {}",
        stderr_text(&sort_output)
    );

    let providers_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    assert!(
        providers_output.status.success(),
        "stderr: {}",
        stderr_text(&providers_output)
    );
    let providers: Value =
        serde_json::from_slice(&providers_output.stdout).expect("provider list should return json");
    assert!(providers.get("primary-provider").is_some());
    assert_eq!(
        providers
            .get("primary-provider-backup")
            .and_then(|item| item.get("sortIndex"))
            .and_then(Value::as_u64),
        Some(7)
    );

    let switch_output = run_cli(
        temp.path(),
        &[
            "provider",
            "switch",
            "primary-provider-backup",
            "--app",
            "claude",
        ],
    );
    assert!(
        switch_output.status.success(),
        "stderr: {}",
        stderr_text(&switch_output)
    );

    let read_live_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "read-live",
            "--app",
            "claude",
        ],
    );
    assert!(
        read_live_output.status.success(),
        "stderr: {}",
        stderr_text(&read_live_output)
    );
    let live: Value =
        serde_json::from_slice(&read_live_output.stdout).expect("read-live should return json");
    assert_eq!(
        live.get("env")
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(Value::as_str),
        Some("https://dup.example")
    );
}

#[test]
#[serial]
fn provider_import_live_and_remove_from_live_round_trip() {
    let temp = tempdir().expect("tempdir");

    let claude_live = claude_settings_path(temp.path());
    fs::create_dir_all(claude_live.parent().expect("parent")).expect("claude dir");
    fs::write(
        &claude_live,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://live.example","ANTHROPIC_AUTH_TOKEN":"sk-live"}}"#,
    )
    .expect("write claude live config");

    let import_claude = run_cli(temp.path(), &["provider", "import-live", "--app", "claude"]);
    assert!(
        import_claude.status.success(),
        "stderr: {}",
        stderr_text(&import_claude)
    );

    let show_default = run_cli(
        temp.path(),
        &[
            "--format", "json", "provider", "show", "default", "--app", "claude",
        ],
    );
    assert!(
        show_default.status.success(),
        "stderr: {}",
        stderr_text(&show_default)
    );
    let default_provider: Value =
        serde_json::from_slice(&show_default.stdout).expect("provider show should return json");
    assert_eq!(
        default_provider
            .get("settingsConfig")
            .and_then(|config| config.get("env"))
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(Value::as_str),
        Some("sk-live")
    );

    let opencode_file = temp.path().join("opencode-provider.json");
    fs::write(
        &opencode_file,
        r#"{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://open.live","apiKey":"sk-open"},"models":{}}"#,
    )
    .expect("write opencode provider");

    let add_opencode = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "opencode",
            "--name",
            "Open Live",
            "--from-json",
            opencode_file.to_str().expect("utf-8 path"),
            "--base-url",
            "https://open.live",
            "--api-key",
            "sk-open",
        ],
    );
    assert!(
        add_opencode.status.success(),
        "stderr: {}",
        stderr_text(&add_opencode)
    );

    let opencode_live_before: Value = serde_json::from_str(
        &fs::read_to_string(opencode_config_path(temp.path())).expect("opencode config"),
    )
    .expect("opencode json");
    assert!(opencode_live_before
        .get("provider")
        .and_then(|providers| providers.get("open-live"))
        .is_some());

    let remove_live = run_cli(
        temp.path(),
        &[
            "provider",
            "remove-from-live",
            "open-live",
            "--app",
            "opencode",
        ],
    );
    assert!(
        remove_live.status.success(),
        "stderr: {}",
        stderr_text(&remove_live)
    );

    let providers_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "opencode"],
    );
    let providers: Value =
        serde_json::from_slice(&providers_output.stdout).expect("provider list should return json");
    assert!(
        providers.get("open-live").is_some(),
        "db record should remain"
    );

    let opencode_live_after: Value = serde_json::from_str(
        &fs::read_to_string(opencode_config_path(temp.path())).expect("opencode config"),
    )
    .expect("opencode json");
    assert!(
        opencode_live_after
            .get("provider")
            .and_then(|providers| providers.get("open-live"))
            .is_none(),
        "live config should no longer contain removed provider"
    );
}

#[test]
#[serial]
fn provider_sort_order_and_remove_from_live_require_existing_provider() {
    let temp = tempdir().expect("tempdir");

    let sort_output = run_cli(
        temp.path(),
        &[
            "provider",
            "sort-order",
            "missing-provider",
            "--app",
            "claude",
            "--index",
            "1",
        ],
    );
    assert!(!sort_output.status.success(), "sort-order should fail");
    assert!(stderr_text(&sort_output).contains("Provider not found"));

    let remove_output = run_cli(
        temp.path(),
        &[
            "provider",
            "remove-from-live",
            "missing-provider",
            "--app",
            "opencode",
        ],
    );
    assert!(
        !remove_output.status.success(),
        "remove-from-live should fail"
    );
    assert!(stderr_text(&remove_output).contains("Provider not found"));
}

#[test]
#[serial]
fn provider_endpoint_lifecycle_and_speedtest_round_trip() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:9/v1","ANTHROPIC_AUTH_TOKEN":"sk-endpoint"}}"#,
    )
    .expect("write provider json");

    let add_provider = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Endpoint Provider",
            "--base-url",
            "http://127.0.0.1:9/v1",
            "--api-key",
            "sk-endpoint",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_provider.status.success(),
        "stderr: {}",
        stderr_text(&add_provider)
    );

    let add_invalid = run_cli(
        temp.path(),
        &[
            "provider",
            "endpoint",
            "add",
            "endpoint-provider",
            "--app",
            "claude",
            "--url",
            "not-a-url",
        ],
    );
    assert!(
        add_invalid.status.success(),
        "stderr: {}",
        stderr_text(&add_invalid)
    );

    let add_secondary = run_cli(
        temp.path(),
        &[
            "provider",
            "endpoint",
            "add",
            "endpoint-provider",
            "--app",
            "claude",
            "--url",
            "http://127.0.0.1:9/secondary/",
        ],
    );
    assert!(
        add_secondary.status.success(),
        "stderr: {}",
        stderr_text(&add_secondary)
    );

    let mark_used = run_cli(
        temp.path(),
        &[
            "provider",
            "endpoint",
            "mark-used",
            "endpoint-provider",
            "--app",
            "claude",
            "--url",
            "http://127.0.0.1:9/secondary",
        ],
    );
    assert!(
        mark_used.status.success(),
        "stderr: {}",
        stderr_text(&mark_used)
    );

    let list_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "endpoint",
            "list",
            "endpoint-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let endpoints: Value =
        serde_json::from_slice(&list_output.stdout).expect("endpoint list should return json");
    assert_eq!(endpoints.as_array().map(Vec::len), Some(2));
    assert!(endpoints
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("url") == Some(&Value::String("http://127.0.0.1:9/secondary".to_string()))
            })
        })
        .and_then(|item| item.get("lastUsed"))
        .and_then(Value::as_i64)
        .is_some());

    let speedtest_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "endpoint",
            "speedtest",
            "endpoint-provider",
            "--app",
            "claude",
            "--timeout",
            "2",
        ],
    );
    assert!(
        speedtest_output.status.success(),
        "stderr: {}",
        stderr_text(&speedtest_output)
    );
    let speedtest: Value =
        serde_json::from_slice(&speedtest_output.stdout).expect("speedtest should return json");
    assert!(speedtest
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item.get("url")
            == Some(&Value::String("not-a-url".to_string()))
            && item
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|text| text.starts_with("URL 无效")))));

    let remove_endpoint = run_cli(
        temp.path(),
        &[
            "provider",
            "endpoint",
            "remove",
            "endpoint-provider",
            "--app",
            "claude",
            "--url",
            "not-a-url",
        ],
    );
    assert!(
        remove_endpoint.status.success(),
        "stderr: {}",
        stderr_text(&remove_endpoint)
    );

    let list_after_remove = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "endpoint",
            "list",
            "endpoint-provider",
            "--app",
            "claude",
        ],
    );
    let endpoints_after_remove: Value = serde_json::from_slice(&list_after_remove.stdout)
        .expect("endpoint list should return json");
    assert_eq!(endpoints_after_remove.as_array().map(Vec::len), Some(1));
    assert!(endpoints_after_remove.as_array().is_some_and(|items| items
        .iter()
        .all(|item| item.get("url") != Some(&Value::String("not-a-url".to_string())))));
}

#[test]
#[serial]
fn universal_provider_sync_adds_target_app_providers() {
    let temp = tempdir().expect("tempdir");

    let add_output = run_cli(
        temp.path(),
        &[
            "provider",
            "universal",
            "add",
            "--name",
            "Omni",
            "--apps",
            "claude,codex",
            "--base-url",
            "https://api.example.com",
            "--api-key",
            "sk-omni",
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );

    let sync_output = run_cli(temp.path(), &["provider", "universal", "sync", "omni"]);
    assert!(
        sync_output.status.success(),
        "stderr: {}",
        stderr_text(&sync_output)
    );

    let claude_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    let claude_providers: Value =
        serde_json::from_slice(&claude_output.stdout).expect("provider list should return json");
    assert!(claude_providers.get("universal-claude-omni").is_some());

    let codex_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "codex"],
    );
    let codex_providers: Value =
        serde_json::from_slice(&codex_output.stdout).expect("provider list should return json");
    assert!(codex_providers.get("universal-codex-omni").is_some());

    let delete_without_yes = run_cli(temp.path(), &["provider", "universal", "delete", "omni"]);
    assert!(!delete_without_yes.status.success(), "delete should fail");
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_with_yes = run_cli(
        temp.path(),
        &["provider", "universal", "delete", "omni", "--yes"],
    );
    assert!(
        delete_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&delete_with_yes)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "universal", "list"],
    );
    let providers: Value = serde_json::from_slice(&list_output.stdout)
        .expect("universal provider list should return json");
    assert_eq!(providers, Value::Object(Default::default()));
}

#[test]
#[serial]
fn proxy_config_set_then_show_round_trips_through_cli() {
    let temp = tempdir().expect("tempdir");

    let set_output = run_cli(
        temp.path(),
        &[
            "--quiet",
            "proxy",
            "config",
            "set",
            "--port",
            "9999",
            "--host",
            "127.0.0.2",
        ],
    );
    assert!(
        set_output.status.success(),
        "stderr: {}",
        stderr_text(&set_output)
    );

    let show_output = run_cli(
        temp.path(),
        &["--format", "json", "proxy", "config", "show"],
    );
    assert!(
        show_output.status.success(),
        "stderr: {}",
        stderr_text(&show_output)
    );

    let value: Value =
        serde_json::from_slice(&show_output.stdout).expect("proxy config should return json");
    assert_eq!(value.get("listen_port").and_then(Value::as_u64), Some(9999));
    assert_eq!(
        value.get("listen_address").and_then(Value::as_str),
        Some("127.0.0.2")
    );
}

#[test]
#[serial]
fn proxy_failover_queue_switch_and_priority_round_trip() {
    let temp = tempdir().expect("tempdir");

    for (name, base_url, api_key) in [
        ("Alpha", "https://alpha.example", "sk-alpha"),
        ("Beta", "https://beta.example", "sk-beta"),
    ] {
        let output = run_cli(
            temp.path(),
            &[
                "provider",
                "add",
                "--app",
                "claude",
                "--name",
                name,
                "--base-url",
                base_url,
                "--api-key",
                api_key,
            ],
        );
        assert!(output.status.success(), "stderr: {}", stderr_text(&output));
    }

    let add_alpha = run_cli(
        temp.path(),
        &[
            "proxy",
            "failover",
            "add",
            "alpha",
            "--app",
            "claude",
            "--priority",
            "5",
        ],
    );
    assert!(
        add_alpha.status.success(),
        "stderr: {}",
        stderr_text(&add_alpha)
    );

    let add_beta = run_cli(
        temp.path(),
        &[
            "proxy",
            "failover",
            "add",
            "beta",
            "--app",
            "claude",
            "--priority",
            "1",
        ],
    );
    assert!(
        add_beta.status.success(),
        "stderr: {}",
        stderr_text(&add_beta)
    );

    let queue_output = run_cli(
        temp.path(),
        &[
            "--format", "json", "proxy", "failover", "queue", "--app", "claude",
        ],
    );
    assert!(
        queue_output.status.success(),
        "stderr: {}",
        stderr_text(&queue_output)
    );
    let queue: Value =
        serde_json::from_slice(&queue_output.stdout).expect("failover queue should return json");
    let items = queue.as_array().expect("queue should be an array");
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].get("providerId").and_then(Value::as_str),
        Some("beta")
    );
    assert_eq!(items[0].get("priority").and_then(Value::as_u64), Some(1));
    assert_eq!(
        items[1].get("providerId").and_then(Value::as_str),
        Some("alpha")
    );
    assert_eq!(items[1].get("priority").and_then(Value::as_u64), Some(5));

    let switch_output = run_cli(
        temp.path(),
        &["proxy", "failover", "switch", "beta", "--app", "claude"],
    );
    assert!(
        switch_output.status.success(),
        "stderr: {}",
        stderr_text(&switch_output)
    );

    let current_output = run_cli(
        temp.path(),
        &["--format", "json", "config", "get", "currentProviderClaude"],
    );
    assert!(
        current_output.status.success(),
        "stderr: {}",
        stderr_text(&current_output)
    );
    let current: Value =
        serde_json::from_slice(&current_output.stdout).expect("current provider should be json");
    assert_eq!(
        current.get("currentProviderClaude").and_then(Value::as_str),
        Some("beta")
    );

    let remove_output = run_cli(
        temp.path(),
        &["proxy", "failover", "remove", "alpha", "--app", "claude"],
    );
    assert!(
        remove_output.status.success(),
        "stderr: {}",
        stderr_text(&remove_output)
    );

    let queue_after_remove = run_cli(
        temp.path(),
        &[
            "--format", "json", "proxy", "failover", "queue", "--app", "claude",
        ],
    );
    let items_after_remove: Value =
        serde_json::from_slice(&queue_after_remove.stdout).expect("queue should return json");
    assert_eq!(items_after_remove.as_array().map(Vec::len), Some(1));
}

#[test]
#[serial]
fn proxy_commands_reject_missing_and_unsupported_providers() {
    let temp = tempdir().expect("tempdir");

    let unsupported_output = run_cli(
        temp.path(),
        &["proxy", "takeover", "enable", "--app", "openclaw"],
    );
    assert!(!unsupported_output.status.success(), "command should fail");
    assert!(stderr_text(&unsupported_output).contains("claude, codex, gemini"));

    let missing_output = run_cli(
        temp.path(),
        &["proxy", "failover", "add", "missing", "--app", "claude"],
    );
    assert!(!missing_output.status.success(), "command should fail");
    assert!(stderr_text(&missing_output).contains("Provider 'missing' not found"));

    let negative_priority = run_cli(
        temp.path(),
        &[
            "proxy",
            "failover",
            "add",
            "missing",
            "--app",
            "claude",
            "--priority=-1",
        ],
    );
    assert!(!negative_priority.status.success(), "command should fail");
    assert!(stderr_text(&negative_priority).contains("zero or greater"));
}

#[test]
#[serial]
fn proxy_circuit_config_rejects_half_open_requests() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(
        temp.path(),
        &[
            "proxy",
            "circuit",
            "config",
            "set",
            "--half-open-requests",
            "2",
        ],
    );
    assert!(!output.status.success(), "command should fail");
    assert!(stderr_text(&output).contains("not supported"));
}

#[test]
#[serial]
fn usage_summary_defaults_to_all_history_and_days_filters_when_requested() {
    let temp = tempdir().expect("tempdir");

    insert_usage_log(
        temp.path(),
        "req-claude-history",
        "claude",
        "claude-provider",
        "claude-haiku",
        120,
        30,
        "0.0125",
        chrono::NaiveDate::from_ymd_opt(2026, 2, 14)
            .expect("date")
            .and_hms_opt(2, 1, 3)
            .expect("time")
            .and_utc()
            .timestamp(),
    );

    let all_history_output = run_cli(
        temp.path(),
        &["--format", "json", "usage", "summary", "--app", "claude"],
    );
    assert!(
        all_history_output.status.success(),
        "stderr: {}",
        stderr_text(&all_history_output)
    );
    let all_history: Value = serde_json::from_slice(&all_history_output.stdout)
        .expect("usage summary should return json");
    assert_eq!(
        all_history.get("totalRequests").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        all_history.get("totalTokens").and_then(Value::as_u64),
        Some(150)
    );

    let recent_only_output = run_cli(
        temp.path(),
        &[
            "--format", "json", "usage", "summary", "--app", "claude", "--days", "7",
        ],
    );
    assert!(
        recent_only_output.status.success(),
        "stderr: {}",
        stderr_text(&recent_only_output)
    );
    let recent_only: Value = serde_json::from_slice(&recent_only_output.stdout)
        .expect("usage summary should return json");
    assert_eq!(
        recent_only.get("totalRequests").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        recent_only.get("totalTokens").and_then(Value::as_u64),
        Some(0)
    );
}

#[test]
#[serial]
fn usage_logs_export_and_invalid_date_paths_work() {
    let temp = tempdir().expect("tempdir");
    let export_file = temp.path().join("usage.csv");

    insert_usage_log(
        temp.path(),
        "req-claude-a",
        "claude",
        "claude-provider",
        "claude-sonnet",
        100,
        50,
        "0.01",
        chrono::NaiveDate::from_ymd_opt(2026, 3, 5)
            .expect("date")
            .and_hms_opt(8, 0, 0)
            .expect("time")
            .and_utc()
            .timestamp_millis(),
    );
    insert_usage_log(
        temp.path(),
        "req-claude-b",
        "claude",
        "claude-provider",
        "claude-haiku",
        40,
        10,
        "0.005",
        chrono::NaiveDate::from_ymd_opt(2026, 3, 6)
            .expect("date")
            .and_hms_opt(9, 0, 0)
            .expect("time")
            .and_utc()
            .timestamp_millis(),
    );
    insert_usage_log(
        temp.path(),
        "req-codex-a",
        "codex",
        "codex-provider",
        "gpt-5",
        200,
        100,
        "0.02",
        chrono::NaiveDate::from_ymd_opt(2026, 3, 5)
            .expect("date")
            .and_hms_opt(10, 0, 0)
            .expect("time")
            .and_utc()
            .timestamp_millis(),
    );

    let logs_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "usage",
            "logs",
            "--app",
            "claude",
            "--from",
            "2026-03-05",
            "--to",
            "2026-03-05",
        ],
    );
    assert!(
        logs_output.status.success(),
        "stderr: {}",
        stderr_text(&logs_output)
    );
    let logs: Value =
        serde_json::from_slice(&logs_output.stdout).expect("usage logs should return json");
    let items = logs.as_array().expect("logs should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("model").and_then(Value::as_str),
        Some("claude-sonnet")
    );

    let export_output = run_cli(
        temp.path(),
        &[
            "usage",
            "export",
            "--app",
            "claude",
            "--output",
            export_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        export_output.status.success(),
        "stderr: {}",
        stderr_text(&export_output)
    );
    let csv = fs::read_to_string(&export_file).expect("read csv");
    assert!(csv.contains("claude-sonnet"));
    assert!(csv.contains("claude-haiku"));
    assert!(!csv.contains("gpt-5"));

    let invalid_date = run_cli(
        temp.path(),
        &["usage", "logs", "--app", "claude", "--from", "bad-date"],
    );
    assert!(!invalid_date.status.success(), "command should fail");
    assert!(stderr_text(&invalid_date).contains("Expected format: YYYY-MM-DD"));
}

#[test]
#[serial]
fn skill_enable_disable_and_uninstall_round_trip() {
    let temp = tempdir().expect("tempdir");
    seed_installed_skill(temp.path(), "local:demo-skill", "demo-skill");

    let list_output = run_cli(temp.path(), &["--format", "json", "skill", "list"]);
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let skills: Value =
        serde_json::from_slice(&list_output.stdout).expect("skill list should return json");
    assert_eq!(skills.as_array().map(Vec::len), Some(1));

    let enable_output = run_cli(
        temp.path(),
        &["skill", "enable", "local:demo-skill", "--app", "claude"],
    );
    assert!(
        enable_output.status.success(),
        "stderr: {}",
        stderr_text(&enable_output)
    );
    assert!(exists_or_symlink(&claude_skill_dir(
        temp.path(),
        "demo-skill"
    )));

    let disable_output = run_cli(
        temp.path(),
        &["skill", "disable", "local:demo-skill", "--app", "claude"],
    );
    assert!(
        disable_output.status.success(),
        "stderr: {}",
        stderr_text(&disable_output)
    );
    assert!(!exists_or_symlink(&claude_skill_dir(
        temp.path(),
        "demo-skill"
    )));

    let uninstall_without_yes = run_cli(temp.path(), &["skill", "uninstall", "local:demo-skill"]);
    assert!(
        !uninstall_without_yes.status.success(),
        "command should fail"
    );
    assert!(stderr_text(&uninstall_without_yes).contains("Re-run with --yes"));

    let uninstall_with_yes = run_cli(
        temp.path(),
        &["skill", "uninstall", "local:demo-skill", "--yes"],
    );
    assert!(
        uninstall_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&uninstall_with_yes)
    );

    let list_after_uninstall = run_cli(temp.path(), &["--format", "json", "skill", "list"]);
    let skills_after_uninstall: Value = serde_json::from_slice(&list_after_uninstall.stdout)
        .expect("skill list should return json");
    assert_eq!(skills_after_uninstall, Value::Array(vec![]));
}

#[test]
#[serial]
fn import_deeplink_provider_populates_provider_list() {
    let temp = tempdir().expect("tempdir");
    let deeplink =
        "ccswitch://provider?name=Router&baseUrl=https%3A%2F%2Fapi.example.com&apiKey=sk-demo&app=claude";

    let import_output = run_cli(temp.path(), &["import-deeplink", deeplink]);
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );

    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("provider list should return json");
    let has_router = value
        .as_object()
        .expect("provider list should be an object")
        .values()
        .any(|provider| provider.get("name").and_then(Value::as_str) == Some("Router"));
    assert!(
        has_router,
        "imported provider should exist in provider list"
    );
}

#[test]
#[serial]
fn export_import_merge_preserves_existing_data() {
    let source = tempdir().expect("tempdir");
    let target = tempdir().expect("tempdir");
    let export_file = source.path().join("backup.json");
    let source_prompt = source.path().join("source-prompt.txt");
    let target_prompt = target.path().join("target-prompt.txt");
    fs::write(&source_prompt, "Keep it sharp.\n").expect("write source prompt");
    fs::write(&target_prompt, "Stay local.\n").expect("write target prompt");

    let source_set = run_cli(source.path(), &["config", "set", "language", "zh"]);
    assert!(
        source_set.status.success(),
        "stderr: {}",
        stderr_text(&source_set)
    );
    let source_add_prompt = run_cli(
        source.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "sharp",
            "--file",
            source_prompt.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        source_add_prompt.status.success(),
        "stderr: {}",
        stderr_text(&source_add_prompt)
    );

    let export_output = run_cli(
        source.path(),
        &[
            "export",
            "--output",
            export_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        export_output.status.success(),
        "stderr: {}",
        stderr_text(&export_output)
    );

    let target_set = run_cli(target.path(), &["config", "set", "language", "ja"]);
    assert!(
        target_set.status.success(),
        "stderr: {}",
        stderr_text(&target_set)
    );
    let target_add_prompt = run_cli(
        target.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "local",
            "--file",
            target_prompt.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        target_add_prompt.status.success(),
        "stderr: {}",
        stderr_text(&target_add_prompt)
    );

    let import_output = run_cli(
        target.path(),
        &[
            "import",
            "--input",
            export_file.to_str().expect("utf-8 path"),
            "--merge",
        ],
    );
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let get_output = run_cli(
        target.path(),
        &["--format", "json", "config", "get", "language"],
    );
    assert!(
        get_output.status.success(),
        "stderr: {}",
        stderr_text(&get_output)
    );
    let language: Value =
        serde_json::from_slice(&get_output.stdout).expect("config get should return json");
    assert_eq!(language.get("language").and_then(Value::as_str), Some("zh"));

    let prompt_output = run_cli(
        target.path(),
        &["--format", "json", "prompt", "list", "--app", "claude"],
    );
    assert!(
        prompt_output.status.success(),
        "stderr: {}",
        stderr_text(&prompt_output)
    );
    let prompts: Value =
        serde_json::from_slice(&prompt_output.stdout).expect("prompt list should return json");
    assert_eq!(
        prompts
            .get("sharp")
            .and_then(|item| item.get("content"))
            .and_then(Value::as_str),
        Some("Keep it sharp.\n")
    );
    assert_eq!(
        prompts
            .get("local")
            .and_then(|item| item.get("content"))
            .and_then(Value::as_str),
        Some("Stay local.\n")
    );
}

#[test]
#[serial]
fn invalid_app_error_is_consistent_across_commands() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(temp.path(), &["provider", "list", "--app", "bad-app"]);
    assert!(!output.status.success(), "command should fail");

    let stderr = stderr_text(&output);
    assert!(stderr.contains("Invalid app type: bad-app"));
    assert!(stderr.contains("claude, codex, gemini, opencode, openclaw"));
}

#[test]
#[serial]
fn proxy_commands_reuse_the_same_invalid_app_error() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(
        temp.path(),
        &["proxy", "takeover", "enable", "--app", "bad-app"],
    );
    assert!(!output.status.success(), "command should fail");

    let stderr = stderr_text(&output);
    assert!(stderr.contains("Invalid app type: bad-app"));
    assert!(stderr.contains("claude, codex, gemini, opencode, openclaw"));
}

#[test]
#[serial]
fn verbose_mode_emits_command_context_on_stderr() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(
        temp.path(),
        &["--verbose", "--format", "json", "config", "path"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));

    let stderr = stderr_text(&output);
    assert!(stderr.contains("Executing config command"));

    let value: Value =
        serde_json::from_slice(&output.stdout).expect("config path should still return json");
    assert!(value.get("configDir").is_some());
}

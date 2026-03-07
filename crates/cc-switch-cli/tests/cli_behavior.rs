use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
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

#[test]
fn quiet_mode_suppresses_success_output_and_config_get_returns_json() {
    let temp = tempdir().expect("tempdir");

    let set_output = run_cli(temp.path(), &["--quiet", "config", "set", "language", "zh"]);
    assert!(set_output.status.success(), "stderr: {}", stderr_text(&set_output));
    assert!(stdout_text(&set_output).trim().is_empty());

    let get_output = run_cli(temp.path(), &["--format", "json", "config", "get", "language"]);
    assert!(get_output.status.success(), "stderr: {}", stderr_text(&get_output));

    let value: Value =
        serde_json::from_slice(&get_output.stdout).expect("config get should return json");
    assert_eq!(value.get("language").and_then(Value::as_str), Some("zh"));
}

#[test]
fn prompt_add_then_list_round_trips_through_cli() {
    let temp = tempdir().expect("tempdir");
    let prompt_file = temp.path().join("prompt.txt");
    fs::write(&prompt_file, "Review the diff carefully.\n").expect("write prompt");

    let add_output = run_cli(
        temp.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "review",
            "--file",
            prompt_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(add_output.status.success(), "stderr: {}", stderr_text(&add_output));

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "prompt", "list", "--app", "claude"],
    );
    assert!(list_output.status.success(), "stderr: {}", stderr_text(&list_output));

    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("prompt list should return json");
    assert_eq!(
        value
            .get("review")
            .and_then(|item| item.get("content"))
            .and_then(Value::as_str),
        Some("Review the diff carefully.\n")
    );
}

#[test]
fn mcp_add_then_list_round_trips_through_cli() {
    let temp = tempdir().expect("tempdir");

    let add_output = run_cli(
        temp.path(),
        &[
            "--quiet",
            "mcp",
            "add",
            "--id",
            "demo",
            "--command",
            "npx",
            "--args",
            "foo,bar",
            "--apps",
            "claude,codex",
        ],
    );
    assert!(add_output.status.success(), "stderr: {}", stderr_text(&add_output));
    assert!(stdout_text(&add_output).trim().is_empty());

    let list_output = run_cli(temp.path(), &["--format", "json", "mcp", "list"]);
    assert!(list_output.status.success(), "stderr: {}", stderr_text(&list_output));

    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("mcp list should return json");
    let apps = value
        .get("demo")
        .and_then(|item| item.get("apps"))
        .cloned()
        .expect("mcp server apps should exist");
    assert_eq!(apps.get("claude").and_then(Value::as_bool), Some(true));
    assert_eq!(apps.get("codex").and_then(Value::as_bool), Some(true));
    assert_eq!(apps.get("gemini").and_then(Value::as_bool), Some(false));
}

#[test]
fn provider_add_then_list_round_trips_through_cli() {
    let temp = tempdir().expect("tempdir");

    let add_output = run_cli(
        temp.path(),
        &[
            "--quiet",
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Demo Provider",
            "--base-url",
            "https://api.example.com",
            "--api-key",
            "sk-demo",
        ],
    );
    assert!(add_output.status.success(), "stderr: {}", stderr_text(&add_output));

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    assert!(list_output.status.success(), "stderr: {}", stderr_text(&list_output));

    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("provider list should return json");
    assert_eq!(
        value
            .get("demo-provider")
            .and_then(|item| item.get("name"))
            .and_then(Value::as_str),
        Some("Demo Provider")
    );
}

#[test]
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
    assert!(set_output.status.success(), "stderr: {}", stderr_text(&set_output));

    let show_output = run_cli(temp.path(), &["--format", "json", "proxy", "config", "show"]);
    assert!(show_output.status.success(), "stderr: {}", stderr_text(&show_output));

    let value: Value =
        serde_json::from_slice(&show_output.stdout).expect("proxy config should return json");
    assert_eq!(value.get("listen_port").and_then(Value::as_u64), Some(9999));
    assert_eq!(
        value.get("listen_address").and_then(Value::as_str),
        Some("127.0.0.2")
    );
}

#[test]
fn usage_summary_and_skill_list_work_on_empty_state() {
    let temp = tempdir().expect("tempdir");

    let usage_output = run_cli(
        temp.path(),
        &["--format", "json", "usage", "summary", "--app", "claude"],
    );
    assert!(usage_output.status.success(), "stderr: {}", stderr_text(&usage_output));

    let usage_value: Value =
        serde_json::from_slice(&usage_output.stdout).expect("usage summary should return json");
    assert_eq!(
        usage_value.get("totalRequests").and_then(Value::as_u64),
        Some(0)
    );

    let skill_output = run_cli(temp.path(), &["--format", "json", "skill", "list"]);
    assert!(skill_output.status.success(), "stderr: {}", stderr_text(&skill_output));

    let skill_value: Value =
        serde_json::from_slice(&skill_output.stdout).expect("skill list should return json");
    assert_eq!(skill_value, Value::Array(vec![]));
}

#[test]
fn import_deeplink_provider_populates_provider_list() {
    let temp = tempdir().expect("tempdir");
    let deeplink = "ccswitch://provider?name=Router&baseUrl=https%3A%2F%2Fapi.example.com&apiKey=sk-demo&app=claude";

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
    assert!(list_output.status.success(), "stderr: {}", stderr_text(&list_output));

    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("provider list should return json");
    let has_router = value
        .as_object()
        .expect("provider list should be an object")
        .values()
        .any(|provider| provider.get("name").and_then(Value::as_str) == Some("Router"));
    assert!(has_router, "imported provider should exist in provider list");
}

#[test]
fn export_then_import_restores_config_and_prompts() {
    let source = tempdir().expect("tempdir");
    let export_file = source.path().join("backup.json");
    let prompt_file = source.path().join("prompt.txt");
    fs::write(&prompt_file, "Keep it sharp.\n").expect("write prompt");

    let set_output = run_cli(source.path(), &["config", "set", "language", "zh"]);
    assert!(set_output.status.success(), "stderr: {}", stderr_text(&set_output));

    let add_prompt = run_cli(
        source.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "sharp",
            "--file",
            prompt_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(add_prompt.status.success(), "stderr: {}", stderr_text(&add_prompt));

    let export_output = run_cli(
        source.path(),
        &[
            "export",
            "--output",
            export_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(export_output.status.success(), "stderr: {}", stderr_text(&export_output));

    let target = tempdir().expect("tempdir");
    let import_output = run_cli(
        target.path(),
        &[
            "import",
            "--input",
            export_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(import_output.status.success(), "stderr: {}", stderr_text(&import_output));

    let get_output = run_cli(target.path(), &["--format", "json", "config", "get", "language"]);
    assert!(get_output.status.success(), "stderr: {}", stderr_text(&get_output));
    let language: Value =
        serde_json::from_slice(&get_output.stdout).expect("config get should return json");
    assert_eq!(language.get("language").and_then(Value::as_str), Some("zh"));

    let prompt_output = run_cli(
        target.path(),
        &["--format", "json", "prompt", "list", "--app", "claude"],
    );
    assert!(prompt_output.status.success(), "stderr: {}", stderr_text(&prompt_output));
    let prompts: Value =
        serde_json::from_slice(&prompt_output.stdout).expect("prompt list should return json");
    assert_eq!(
        prompts
            .get("sharp")
            .and_then(|item| item.get("content"))
            .and_then(Value::as_str),
        Some("Keep it sharp.\n")
    );
}

#[test]
fn invalid_app_error_is_consistent_across_commands() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(temp.path(), &["provider", "list", "--app", "bad-app"]);
    assert!(!output.status.success(), "command should fail");

    let stderr = stderr_text(&output);
    assert!(stderr.contains("Invalid app type: bad-app"));
    assert!(stderr.contains("claude, codex, gemini, opencode, openclaw"));
}

#[test]
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
fn verbose_mode_emits_command_context_on_stderr() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(temp.path(), &["--verbose", "--format", "json", "config", "path"]);
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));

    let stderr = stderr_text(&output);
    assert!(stderr.contains("Executing config command"));

    let value: Value =
        serde_json::from_slice(&output.stdout).expect("config path should still return json");
    assert!(value.get("configDir").is_some());
}

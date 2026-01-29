use std::process::{Command, Stdio};

use serde_json::json;

use cc_switch_lib::cli::{Cli, CliCommand, CmdAction};
use cc_switch_lib::Provider;
use clap::Parser;

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn cc_switch_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cc-switch"));
    let home = ensure_test_home();
    cmd.env("HOME", home);
    #[cfg(windows)]
    cmd.env("USERPROFILE", home);
    cmd.stdin(Stdio::null());
    cmd
}

fn write_json_file(contents: serde_json::Value) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().expect("create json temp file");
    std::fs::write(
        file.path(),
        serde_json::to_string_pretty(&contents).expect("serialize json"),
    )
    .expect("write json temp file");
    file
}

#[test]
fn cli_cmd_subcommand_parsing() {
    let cli = Cli::try_parse_from(["cc-switch", "cmd"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd { action: None })
    ));
}

#[test]
fn cli_cmd_status_parsing() {
    let cli = Cli::try_parse_from(["cc-switch", "cmd", "status"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::Status)
        })
    ));
}

#[test]
fn cli_cmd_list_parsing() {
    let cli = Cli::try_parse_from(["cc-switch", "cmd", "list", "claude"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::List { tool, ids })
        }) if tool == "claude" && !ids
    ));
}

#[test]
fn cli_cmd_list_parsing_with_ids_flag() {
    let cli = Cli::try_parse_from(["cc-switch", "cmd", "list", "claude", "--ids"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::List { tool, ids })
        }) if tool == "claude" && ids
    ));
}

#[test]
fn cli_cmd_switch_parsing() {
    let cli = Cli::try_parse_from(["cc-switch", "cmd", "switch", "claude", "My Provider"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::Switch { tool, provider })
        }) if tool == "claude" && provider == "My Provider"
    ));
}

#[test]
fn cli_cmd_add_parsing_with_json_flag() {
    let cli = Cli::try_parse_from([
        "cc-switch",
        "cmd",
        "add",
        "claude",
        "--json",
        "provider.json",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::Add { tool, json })
        }) if tool == "claude" && json.as_deref() == Some("provider.json")
    ));
}

#[test]
fn cli_cmd_edit_parsing() {
    let cli = Cli::try_parse_from(["cc-switch", "cmd", "edit", "claude", "My Provider"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::Edit { tool, provider })
        }) if tool == "claude" && provider == "My Provider"
    ));
}

#[test]
fn cli_cmd_delete_parsing_force() {
    let cli = Cli::try_parse_from([
        "cc-switch",
        "cmd",
        "delete",
        "claude",
        "My Provider",
        "--force",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::Delete { tool, provider, force })
        }) if tool == "claude" && provider == "My Provider" && force
    ));
}

#[test]
fn cli_cmd_show_parsing_json_flag() {
    let cli = Cli::try_parse_from([
        "cc-switch",
        "cmd",
        "show",
        "claude",
        "My Provider",
        "--json",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::Show { tool, provider, json })
        }) if tool == "claude" && provider == "My Provider" && json
    ));
}

#[test]
fn cli_cmd_help_parsing() {
    let cli = Cli::try_parse_from(["cc-switch", "cmd", "help"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(CliCommand::Cmd {
            action: Some(CmdAction::Help)
        })
    ));
}

#[test]
fn cli_status_empty_shows_not_configured() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let output = cc_switch_cmd()
        .args(["cmd", "status"])
        .output()
        .expect("run cc-switch cmd status");

    assert!(output.status.success(), "status should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Not configured"),
        "status output should mention Not configured, got: {stdout}"
    );
}

#[test]
fn cli_help_exits_success() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let output = cc_switch_cmd()
        .args(["cmd", "help"])
        .output()
        .expect("run cc-switch cmd help");

    assert!(output.status.success(), "help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("CC-Switch CLI"),
        "help output should contain header, got: {stdout}"
    );
    assert!(
        stdout.contains("JSON IMPORT FORMAT") && stdout.contains("settingsConfig"),
        "help should include JSON import guidance, got: {stdout}"
    );
}

#[test]
fn cli_list_empty_returns_success() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let output = cc_switch_cmd()
        .args(["cmd", "list", "claude"])
        .output()
        .expect("run cc-switch cmd list claude");

    assert!(output.status.success(), "list on empty db should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No providers configured"),
        "empty list output should be informative, got: {stdout}"
    );
}

#[test]
fn cli_list_invalid_tool_exits_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let output = cc_switch_cmd()
        .args(["cmd", "list", "invalid"])
        .output()
        .expect("run cc-switch cmd list invalid");

    assert!(!output.status.success(), "list invalid should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid tool name") || stderr.contains("Valid options"),
        "stderr should mention valid tool names, got: {stderr}"
    );
}

#[test]
fn cli_add_invalid_json_exits_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let bad = tempfile::NamedTempFile::new().expect("create bad json file");
    std::fs::write(bad.path(), "{").expect("write invalid json");

    let output = cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(bad.path())
        .output()
        .expect("add invalid json");

    assert!(!output.status.success(), "invalid json add should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid JSON") || stderr.contains("Failed to read"),
        "stderr should mention invalid JSON, got: {stderr}"
    );
}

#[test]
fn cli_add_invalid_config_exits_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let invalid = write_json_file(json!({
        "name": "Bad Provider",
        "settingsConfig": "not-an-object"
    }));

    let output = cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(invalid.path())
        .output()
        .expect("add invalid config");

    assert!(!output.status.success(), "invalid config add should fail");
}

#[test]
fn cli_add_missing_required_fields_exits_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let missing = write_json_file(json!({
        "name": "Missing Fields",
        "settingsConfig": {
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-test-1234567890"
            }
        }
    }));

    let output = cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(missing.path())
        .output()
        .expect("add missing required fields");

    assert!(
        !output.status.success(),
        "missing required fields should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ANTHROPIC_BASE_URL") && stderr.contains("cc-switch cmd help"),
        "stderr should mention missing field and hint, got: {stderr}"
    );
}

#[test]
fn cli_add_duplicate_name_exits_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let p1 = write_json_file(json!({
        "name": "Dup",
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-test-a-1234567890",
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
        }
    }));
    let p2 = write_json_file(json!({
        "name": "Dup",
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-test-b-1234567890",
            "ANTHROPIC_BASE_URL": "https://openrouter.ai/api"
        }
    }));

    let out1 = cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(p1.path())
        .output()
        .expect("add first dup");
    assert!(out1.status.success(), "first add should succeed");

    let out2 = cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(p2.path())
        .output()
        .expect("add second dup");
    assert!(!out2.status.success(), "duplicate add should fail");
}

#[test]
fn cli_switch_provider_not_found_exits_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let provider = write_json_file(json!({
        "name": "Test Provider",
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-test-1234567890",
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
        }
    }));
    let out_add = cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(provider.path())
        .output()
        .expect("add provider");
    assert!(out_add.status.success(), "seed provider should succeed");

    let out_switch = cc_switch_cmd()
        .args(["cmd", "switch", "claude", "does-not-exist"])
        .output()
        .expect("switch missing provider");
    assert!(
        !out_switch.status.success(),
        "switch to missing provider should fail"
    );
}

#[test]
fn cli_switch_provider_ambiguous_match_exits_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let p1 = write_json_file(json!({
        "name": "OpenRouter A",
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-test-aaaaaaaa",
            "ANTHROPIC_BASE_URL": "https://openrouter.ai/api"
        }
    }));
    let p2 = write_json_file(json!({
        "name": "OpenRouter B",
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-test-bbbbbbbb",
            "ANTHROPIC_BASE_URL": "https://openrouter.ai/api"
        }
    }));

    assert!(cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(p1.path())
        .output()
        .expect("add p1")
        .status
        .success());
    assert!(cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(p2.path())
        .output()
        .expect("add p2")
        .status
        .success());

    let out_switch = cc_switch_cmd()
        .args(["cmd", "switch", "claude", "openrouter"])
        .output()
        .expect("switch ambiguous");
    assert!(!out_switch.status.success(), "ambiguous switch should fail");
    let stderr = String::from_utf8_lossy(&out_switch.stderr);
    assert!(
        stderr.contains("Multiple providers match") || stderr.contains("match"),
        "stderr should mention ambiguity, got: {stderr}"
    );
}

#[test]
fn cli_duplicate_provider_names_require_id_and_can_select_by_id() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    // Seed duplicate provider names directly via database (CLI "add" rejects duplicates).
    let state = create_test_state().expect("create test state");
    let p1 = Provider::with_id(
        "dup-1".to_string(),
        "Dup Provider".to_string(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-test-dup-1111111111",
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }),
        None,
    );
    let p2 = Provider::with_id(
        "dup-2".to_string(),
        "Dup Provider".to_string(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-test-dup-2222222222",
                "ANTHROPIC_BASE_URL": "https://openrouter.ai/api"
            }
        }),
        None,
    );
    state.db.save_provider("claude", &p1).expect("save p1");
    state.db.save_provider("claude", &p2).expect("save p2");
    state
        .db
        .set_current_provider("claude", "dup-1")
        .expect("set current provider");

    let out_switch_by_name = cc_switch_cmd()
        .args(["cmd", "switch", "claude", "Dup Provider"])
        .output()
        .expect("switch by duplicate name");
    assert!(
        !out_switch_by_name.status.success(),
        "switch by duplicate name should fail"
    );
    let name_err = String::from_utf8_lossy(&out_switch_by_name.stderr);
    assert!(
        name_err.contains("Please use a provider ID") && name_err.contains("id:"),
        "error should include IDs, got: {name_err}"
    );

    let out_list = cc_switch_cmd()
        .args(["cmd", "list", "claude", "--ids"])
        .output()
        .expect("list with ids");
    assert!(out_list.status.success(), "list --ids should succeed");
    let list_stdout = String::from_utf8_lossy(&out_list.stdout);

    let mut ids = Vec::new();
    for line in list_stdout.lines() {
        let Some(start) = line.find("(id: ") else {
            continue;
        };
        let rest = &line[start + "(id: ".len()..];
        let Some(end) = rest.find(')') else {
            continue;
        };
        let id = rest[..end].trim();
        if !id.is_empty() {
            ids.push(id.to_string());
        }
    }
    ids.sort();
    ids.dedup();
    assert!(
        ids.len() >= 2,
        "expected at least 2 provider IDs, got output: {list_stdout}"
    );

    let target_id = "dup-2".to_string();
    let out_switch_by_id = cc_switch_cmd()
        .args(["cmd", "switch", "claude"])
        .arg(&target_id)
        .output()
        .expect("switch by id");
    assert!(
        out_switch_by_id.status.success(),
        "switch by id should succeed"
    );

    let out_show = cc_switch_cmd()
        .args(["cmd", "show", "claude", "--json"])
        .arg(&target_id)
        .output()
        .expect("show by id json");
    assert!(out_show.status.success(), "show by id should succeed");
    let show_stdout = String::from_utf8_lossy(&out_show.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(show_stdout.trim()).expect("show output is valid JSON");
    assert_eq!(
        parsed.pointer("/id").and_then(|v| v.as_str()),
        Some(target_id.as_str())
    );
    assert_eq!(
        parsed.pointer("/active").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn cli_workflow_add_list_switch_show_delete() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let provider_a = write_json_file(json!({
        "name": "Provider A",
        "settingsConfig": {
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-test-a-1234567890",
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }
    }));

    let provider_b = write_json_file(json!({
        "name": "Provider B",
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-test-b-1234567890",
            "ANTHROPIC_BASE_URL": "https://openrouter.ai/api"
        }
    }));

    let out_add_a = cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(provider_a.path())
        .output()
        .expect("add provider a");
    assert!(out_add_a.status.success(), "add provider a should succeed");

    let out_add_b = cc_switch_cmd()
        .args(["cmd", "add", "claude", "--json"])
        .arg(provider_b.path())
        .output()
        .expect("add provider b");
    assert!(out_add_b.status.success(), "add provider b should succeed");

    let out_list = cc_switch_cmd()
        .args(["cmd", "list", "claude"])
        .output()
        .expect("list claude");
    assert!(out_list.status.success(), "list should succeed");
    let list_stdout = String::from_utf8_lossy(&out_list.stdout);
    assert!(
        list_stdout.contains("Provider A"),
        "list should include Provider A"
    );
    assert!(
        list_stdout.contains("Provider B"),
        "list should include Provider B"
    );

    let out_switch = cc_switch_cmd()
        .args(["cmd", "switch", "claude", "Provider B"])
        .output()
        .expect("switch to provider b");
    assert!(out_switch.status.success(), "switch should succeed");

    let out_list_after = cc_switch_cmd()
        .args(["cmd", "list", "claude"])
        .output()
        .expect("list claude after switch");
    assert!(
        out_list_after.status.success(),
        "list after switch should succeed"
    );
    let list_after_stdout = String::from_utf8_lossy(&out_list_after.stdout);
    assert!(
        list_after_stdout.contains("[Active]") && list_after_stdout.contains("Provider B"),
        "active provider should be marked, got: {list_after_stdout}"
    );

    let out_show = cc_switch_cmd()
        .args(["cmd", "show", "claude", "Provider B", "--json"])
        .output()
        .expect("show provider b json");
    assert!(out_show.status.success(), "show --json should succeed");
    let show_stdout = String::from_utf8_lossy(&out_show.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(show_stdout.trim()).expect("show output is valid JSON");

    let redacted = parsed
        .pointer("/settingsConfig/env/ANTHROPIC_AUTH_TOKEN")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_ne!(redacted, "sk-test-b-1234567890");
    assert!(
        redacted.contains("****"),
        "token should be redacted, got: {redacted}"
    );
    assert!(
        parsed.pointer("/settingsConfig/name").is_none(),
        "name must not be stored inside settingsConfig"
    );

    let out_show_formatted = cc_switch_cmd()
        .args(["cmd", "show", "claude", "Provider B"])
        .output()
        .expect("show provider b formatted");
    assert!(out_show_formatted.status.success(), "show should succeed");
    let formatted = String::from_utf8_lossy(&out_show_formatted.stdout);
    assert!(
        formatted.contains("Provider Details") && formatted.contains("Provider B"),
        "formatted show should include details, got: {formatted}"
    );

    let out_delete_a = cc_switch_cmd()
        .args(["cmd", "delete", "claude", "Provider A", "--force"])
        .output()
        .expect("delete provider a");
    assert!(
        out_delete_a.status.success(),
        "delete provider a should succeed"
    );

    let out_delete_active = cc_switch_cmd()
        .args(["cmd", "delete", "claude", "Provider B", "--force"])
        .output()
        .expect("delete active provider b");
    assert!(
        !out_delete_active.status.success(),
        "deleting active provider should fail"
    );
}

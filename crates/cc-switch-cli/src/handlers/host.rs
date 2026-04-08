use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};
use clap::CommandFactory;
use clap_complete::Generator;
use serde::Serialize;
use serde_json::json;

use crate::cli::{AutoLaunchCommands, Cli, CompletionShell};
use crate::output::Printer;
use cc_switch_core::{AutoLaunchService, HostService, RuntimeService, WslShellPreference};

const VALID_TOOLS: [&str; 4] = ["claude", "codex", "gemini", "opencode"];
const CARGO_GIT_INSTALL_TEMPLATE: &str =
    "cargo install --git {repo} cc-switch-cli --bin cc-switch --locked --force";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallGuide {
    executable_path: String,
    repository: String,
    releases_url: String,
    platform: String,
    arch: String,
    recommended_method: String,
    methods: Vec<GuideMethod>,
    completion_hints: Vec<CompletionHint>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuideMethod {
    id: String,
    label: String,
    commands: Vec<String>,
    note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletionHint {
    shell: String,
    target_path: String,
    install_command: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledCompletion {
    shell: String,
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateGuide {
    executable_path: String,
    install_source: String,
    current_version: String,
    latest_version: Option<String>,
    has_update: bool,
    releases_url: String,
    release_notes_url: String,
    latest_release_url: String,
    error: Option<String>,
    steps: Vec<String>,
    notes: Vec<String>,
}

pub async fn handle_completions(shell: CompletionShell, printer: &Printer) -> anyhow::Result<()> {
    printer.print_text(generate_completion_script(shell)?)
}

pub async fn handle_install_guide(
    shell: Option<CompletionShell>,
    printer: &Printer,
) -> anyhow::Result<()> {
    let about = RuntimeService::about()?;
    let executable_path = RuntimeService::current_executable_path()?;
    let source_root = detect_source_checkout_root(&executable_path);
    let cargo_git_install = cargo_git_install_command(&about.repository);
    let mut methods = vec![GuideMethod {
        id: "cargo-git".to_string(),
        label: "Cargo install from git".to_string(),
        commands: vec![cargo_git_install],
        note: Some(
            "Best fit when you want one reproducible CLI install without cloning the repo."
                .to_string(),
        ),
    }];

    if let Some(root) = source_root {
        methods.push(GuideMethod {
            id: "source-checkout".to_string(),
            label: "Build from local checkout".to_string(),
            commands: vec![
                format!("git -C {} pull --ff-only", root.display()),
                format!(
                    "cargo build -p cc-switch-cli --bin cc-switch --manifest-path {}/Cargo.toml",
                    root.display()
                ),
            ],
            note: Some("Useful when you already keep a local source checkout.".to_string()),
        });
    } else {
        methods.push(GuideMethod {
            id: "source-checkout".to_string(),
            label: "Build from source checkout".to_string(),
            commands: vec![
                format!("git clone {} ~/cc-switch", about.repository),
                "cd ~/cc-switch".to_string(),
                "cargo build -p cc-switch-cli --bin cc-switch".to_string(),
            ],
            note: Some(
                "Current releases focus on the desktop bundles, so source install is the safest CLI path."
                    .to_string(),
            ),
        });
    }

    let completion_hints = match shell {
        Some(shell) => vec![completion_hint(shell)],
        None => vec![
            completion_hint(CompletionShell::Bash),
            completion_hint(CompletionShell::Zsh),
            completion_hint(CompletionShell::Fish),
        ],
    };

    printer.print_value(&InstallGuide {
        executable_path: executable_path.display().to_string(),
        repository: about.repository,
        releases_url: about.releases_url,
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        recommended_method: "cargo-git".to_string(),
        methods,
        completion_hints,
        notes: vec![
            "Shell completions can be generated with `cc-switch completions <shell>`.".to_string(),
            "The install helper only writes completion files; it does not modify your shell rc automatically.".to_string(),
        ],
    })
}

pub async fn handle_install_completions(
    shell: CompletionShell,
    dir: Option<&str>,
    printer: &Printer,
) -> anyhow::Result<()> {
    let target_dir = dir
        .map(PathBuf::from)
        .unwrap_or_else(|| default_completion_dir(shell));
    std::fs::create_dir_all(&target_dir)?;
    let path = target_dir.join(completion_file_name(shell));
    std::fs::write(&path, generate_completion_script(shell)?)?;
    printer.print_value(&InstalledCompletion {
        shell: completion_shell_name(shell).to_string(),
        path: path.display().to_string(),
    })
}

pub async fn handle_auto_launch(cmd: AutoLaunchCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        AutoLaunchCommands::Status => {
            printer.print_value(&auto_launch_payload(AutoLaunchService::is_enabled()?)?)?;
        }
        AutoLaunchCommands::Enable => {
            AutoLaunchService::enable()?;
            HostService::set_launch_on_startup(true)?;
            printer.print_value(&auto_launch_payload(true)?)?;
        }
        AutoLaunchCommands::Disable => {
            AutoLaunchService::disable()?;
            HostService::set_launch_on_startup(false)?;
            printer.print_value(&auto_launch_payload(false)?)?;
        }
    }

    Ok(())
}

pub async fn handle_portable_mode(printer: &Printer) -> anyhow::Result<()> {
    printer.print_value(&json!({
        "portableMode": RuntimeService::is_portable_mode()?,
    }))?;
    Ok(())
}

pub async fn handle_tool_versions(
    tools: Vec<String>,
    latest: bool,
    wsl_shell: Vec<String>,
    wsl_shell_flag: Vec<String>,
    printer: &Printer,
) -> anyhow::Result<()> {
    let tools = if tools.is_empty() { None } else { Some(tools) };
    let prefs = merge_wsl_preferences(wsl_shell, wsl_shell_flag)?;
    let prefs = if prefs.is_empty() { None } else { Some(prefs) };
    let versions = RuntimeService::get_tool_versions(tools, prefs, latest).await?;
    printer.print_value(&versions)?;
    Ok(())
}

pub async fn handle_about(printer: &Printer) -> anyhow::Result<()> {
    printer.print_value(&RuntimeService::about()?)?;
    Ok(())
}

pub async fn handle_update_check(printer: &Printer) -> anyhow::Result<()> {
    printer.print_value(&RuntimeService::check_for_updates().await?)?;
    Ok(())
}

pub async fn handle_update_guide(printer: &Printer) -> anyhow::Result<()> {
    let about = RuntimeService::about()?;
    let info = RuntimeService::check_for_updates().await?;
    let executable_path = RuntimeService::current_executable_path()?;
    let source_root = detect_source_checkout_root(&executable_path);
    let install_source = if source_root.is_some() {
        "source-checkout"
    } else {
        "cargo-git-compatible"
    };

    let mut steps = if let Some(root) = source_root {
        vec![
            format!("git -C {} pull --ff-only", root.display()),
            format!(
                "cargo build -p cc-switch-cli --bin cc-switch --manifest-path {}/Cargo.toml",
                root.display()
            ),
        ]
    } else {
        vec![cargo_git_install_command(&about.repository)]
    };

    if info.has_update {
        steps.push(format!("Open release notes: {}", info.release_notes_url));
    }

    let mut notes = vec![
        "If you installed the CLI in another way, use the equivalent package manager command instead.".to_string(),
    ];
    if info.latest_version.is_none() {
        notes.push(
            "Latest release metadata could not be fetched just now, so the guide falls back to stable update commands."
                .to_string(),
        );
    }

    printer.print_value(&UpdateGuide {
        executable_path: executable_path.display().to_string(),
        install_source: install_source.to_string(),
        current_version: info.current_version,
        latest_version: info.latest_version,
        has_update: info.has_update,
        releases_url: info.releases_url,
        release_notes_url: info.release_notes_url,
        latest_release_url: info.latest_release_url,
        error: info.error,
        steps,
        notes,
    })
}

pub async fn handle_release_notes(latest: bool, printer: &Printer) -> anyhow::Result<()> {
    let about = RuntimeService::about()?;
    let url = if latest {
        about.latest_release_url
    } else {
        about.current_release_notes_url
    };

    printer.print_value(&json!({ "url": url }))?;
    Ok(())
}

fn generate_completion_script(shell: CompletionShell) -> anyhow::Result<String> {
    let mut command = Cli::command();
    stabilize_command_tree(&mut command, "cc-switch");
    let mut buffer = Vec::new();

    match shell {
        CompletionShell::Bash => {
            return Ok(generate_bash_completion_script(&command));
        }
        CompletionShell::Zsh => clap_complete::Shell::Zsh.generate(&command, &mut buffer),
        CompletionShell::Fish => clap_complete::Shell::Fish.generate(&command, &mut buffer),
    }

    String::from_utf8(buffer)
        .map_err(|error| anyhow!("completion script must be valid UTF-8: {error}"))
}

fn generate_bash_completion_script(command: &clap::Command) -> String {
    let transitions = build_bash_path_transitions(command);
    let cases = build_bash_suggestion_cases(command);

    format!(
        r#"_cc_switch()
{{
    local cur path word idx
    COMPREPLY=()
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    path=""

    for (( idx=1; idx<COMP_CWORD; idx++ )); do
        word="${{COMP_WORDS[idx]}}"
        case "$word" in
            -*) continue ;;
        esac
{transitions}
    done

    local suggestions=""
    case "$path" in
{cases}
    esac

    COMPREPLY=( $(compgen -W "$suggestions" -- "$cur") )
}}

complete -F _cc_switch cc-switch
"#
    )
}

fn build_bash_path_transitions(command: &clap::Command) -> String {
    let mut lines = Vec::new();
    collect_bash_path_transitions(command, "", &mut lines);
    lines.join("\n")
}

fn collect_bash_path_transitions(command: &clap::Command, path: &str, lines: &mut Vec<String>) {
    let subcommands: Vec<_> = command
        .get_subcommands()
        .map(|item| item.get_name().to_string())
        .collect();
    if !subcommands.is_empty() {
        let matchers = subcommands
            .iter()
            .map(|name| shell_case_literal(name))
            .collect::<Vec<_>>()
            .join("|");
        lines.push(format!(
            "        if [[ \"$path\" == {path_label} ]]; then",
            path_label = shell_case_literal(path)
        ));
        lines.push("            case \"$word\" in".to_string());
        lines.push(format!("                {matchers})"));
        lines.push("                    path=\"${path:+$path }$word\"".to_string());
        lines.push("                    ;;".to_string());
        lines.push("            esac".to_string());
        lines.push("        fi".to_string());
    }

    for subcommand in command.get_subcommands() {
        let child_path = if path.is_empty() {
            subcommand.get_name().to_string()
        } else {
            format!("{path} {}", subcommand.get_name())
        };
        collect_bash_path_transitions(subcommand, &child_path, lines);
    }
}

fn build_bash_suggestion_cases(command: &clap::Command) -> String {
    let mut cases = Vec::new();
    collect_bash_suggestion_cases(command, "", &mut cases);
    cases.join("\n")
}

fn collect_bash_suggestion_cases(command: &clap::Command, path: &str, cases: &mut Vec<String>) {
    let mut suggestions = BTreeSet::new();
    for subcommand in command.get_subcommands() {
        suggestions.insert(subcommand.get_name().to_string());
    }
    for argument in command.get_arguments() {
        if argument.is_positional() {
            continue;
        }
        if let Some(short) = argument.get_short() {
            suggestions.insert(format!("-{short}"));
        }
        if let Some(long) = argument.get_long() {
            suggestions.insert(format!("--{long}"));
        }
    }

    let label = shell_case_literal(path);
    let joined = suggestions.into_iter().collect::<Vec<_>>().join(" ");
    cases.push(format!("        {label}) suggestions=\"{joined}\" ;;"));

    for subcommand in command.get_subcommands() {
        let child_path = if path.is_empty() {
            subcommand.get_name().to_string()
        } else {
            format!("{path} {}", subcommand.get_name())
        };
        collect_bash_suggestion_cases(subcommand, &child_path, cases);
    }
}

fn shell_case_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn cargo_git_install_command(repo: &str) -> String {
    CARGO_GIT_INSTALL_TEMPLATE.replace("{repo}", repo)
}

fn completion_hint(shell: CompletionShell) -> CompletionHint {
    let target_path = default_completion_dir(shell).join(completion_file_name(shell));
    CompletionHint {
        shell: completion_shell_name(shell).to_string(),
        target_path: target_path.display().to_string(),
        install_command: format!(
            "cc-switch install completions {} --dir {}",
            completion_shell_name(shell),
            target_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .display()
        ),
    }
}

fn default_completion_dir(shell: CompletionShell) -> PathBuf {
    let home = cc_switch_core::config::get_home_dir();
    match shell {
        CompletionShell::Bash => home.join(".local/share/bash-completion/completions"),
        CompletionShell::Zsh => home.join(".zfunc"),
        CompletionShell::Fish => home.join(".config/fish/completions"),
    }
}

fn completion_file_name(shell: CompletionShell) -> &'static str {
    match shell {
        CompletionShell::Bash => "cc-switch",
        CompletionShell::Zsh => "_cc-switch",
        CompletionShell::Fish => "cc-switch.fish",
    }
}

fn completion_shell_name(shell: CompletionShell) -> &'static str {
    match shell {
        CompletionShell::Bash => "bash",
        CompletionShell::Zsh => "zsh",
        CompletionShell::Fish => "fish",
    }
}

fn detect_source_checkout_root(executable_path: &Path) -> Option<PathBuf> {
    executable_path.ancestors().find_map(|ancestor| {
        if ancestor.join("crates/cc-switch-cli/Cargo.toml").is_file()
            && ancestor.join("Cargo.toml").is_file()
        {
            Some(ancestor.to_path_buf())
        } else {
            None
        }
    })
}

fn stabilize_command_tree(command: &mut clap::Command, bin_name: &str) {
    command.set_bin_name(bin_name);
    command.build();
    for subcommand in command.get_subcommands_mut() {
        let child_bin_name = subcommand
            .get_bin_name()
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("{bin_name} {}", subcommand.get_name()));
        stabilize_command_tree(subcommand, &child_bin_name);
    }
}

fn auto_launch_payload(enabled: bool) -> anyhow::Result<serde_json::Value> {
    let preferences = HostService::get_preferences()?;
    Ok(json!({
        "enabled": enabled,
        "launchOnStartup": preferences.launch_on_startup,
    }))
}

fn merge_wsl_preferences(
    wsl_shell: Vec<String>,
    wsl_shell_flag: Vec<String>,
) -> anyhow::Result<HashMap<String, WslShellPreference>> {
    let mut map = HashMap::new();

    for entry in wsl_shell {
        let (tool, value) = parse_tool_assignment(&entry, "wsl shell override")?;
        map.entry(tool)
            .or_insert_with(WslShellPreference::default)
            .wsl_shell = Some(value);
    }

    for entry in wsl_shell_flag {
        let (tool, value) = parse_tool_assignment(&entry, "wsl shell flag override")?;
        map.entry(tool)
            .or_insert_with(WslShellPreference::default)
            .wsl_shell_flag = Some(value);
    }

    Ok(map)
}

fn parse_tool_assignment(raw: &str, label: &str) -> anyhow::Result<(String, String)> {
    let (tool, value) = raw
        .split_once('=')
        .ok_or_else(|| anyhow!("{label} must look like <tool>=<value>"))?;
    let tool = tool.trim().to_lowercase();
    let value = value.trim().to_string();

    if !VALID_TOOLS.contains(&tool.as_str()) {
        bail!(
            "{label} uses unsupported tool '{}', expected one of: {}",
            tool,
            VALID_TOOLS.join(", ")
        );
    }

    if value.is_empty() {
        bail!("{label} value cannot be empty");
    }

    Ok((tool, value))
}

#[cfg(test)]
mod tests {
    use super::{
        cargo_git_install_command, completion_file_name, completion_shell_name,
        default_completion_dir, detect_source_checkout_root, merge_wsl_preferences,
        parse_tool_assignment, stabilize_command_tree,
    };
    use crate::cli::CompletionShell;
    use clap::CommandFactory;
    use clap_complete::generator::utils::all_subcommands;
    use tempfile::tempdir;

    #[test]
    fn parse_tool_assignment_accepts_valid_entries() {
        let (tool, value) =
            parse_tool_assignment("claude=bash", "wsl shell override").expect("valid entry");
        assert_eq!(tool, "claude");
        assert_eq!(value, "bash");
    }

    #[test]
    fn parse_tool_assignment_rejects_invalid_entries() {
        let error =
            parse_tool_assignment("oops", "wsl shell override").expect_err("expected error");
        assert!(error.to_string().contains("<tool>=<value>"));
    }

    #[test]
    fn merge_wsl_preferences_combines_shell_and_flag() {
        let prefs = merge_wsl_preferences(
            vec!["claude=bash".to_string()],
            vec!["claude=-lc".to_string()],
        )
        .expect("valid prefs");
        let claude = prefs.get("claude").expect("claude pref");
        assert_eq!(claude.wsl_shell.as_deref(), Some("bash"));
        assert_eq!(claude.wsl_shell_flag.as_deref(), Some("-lc"));
    }

    #[test]
    fn cargo_git_install_command_targets_repo_package() {
        let command = cargo_git_install_command("https://github.com/farion1231/cc-switch");
        assert!(command.contains("cargo install --git https://github.com/farion1231/cc-switch"));
        assert!(command.contains("cc-switch-cli"));
        assert!(command.contains("--bin cc-switch"));
    }

    #[test]
    fn completion_file_names_match_shell_conventions() {
        assert_eq!(completion_file_name(CompletionShell::Bash), "cc-switch");
        assert_eq!(completion_file_name(CompletionShell::Zsh), "_cc-switch");
        assert_eq!(
            completion_file_name(CompletionShell::Fish),
            "cc-switch.fish"
        );
        assert_eq!(completion_shell_name(CompletionShell::Fish), "fish");
    }

    #[test]
    fn detect_source_checkout_root_finds_workspace_root() {
        let temp = tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("crates/cc-switch-cli"))
            .expect("create cli crate dir");
        std::fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").expect("write workspace");
        std::fs::write(
            temp.path().join("crates/cc-switch-cli/Cargo.toml"),
            "[package]\nname = \"cc-switch-cli\"\nversion = \"0.0.0\"\n",
        )
        .expect("write cli cargo");
        let exe = temp.path().join("target/debug/cc-switch");
        std::fs::create_dir_all(exe.parent().expect("target dir")).expect("create target dir");
        std::fs::write(&exe, "").expect("write fake exe");

        let root = detect_source_checkout_root(&exe).expect("workspace root should be detected");
        assert_eq!(root, temp.path());
    }

    #[test]
    fn default_completion_dir_uses_test_home() {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let path = default_completion_dir(CompletionShell::Zsh);
        assert!(path.ends_with(".zfunc"));
    }

    #[test]
    fn stabilize_command_tree_populates_full_command_tree() {
        let mut command = crate::cli::Cli::command();
        stabilize_command_tree(&mut command, "cc-switch");

        fn assert_tree(command: &clap::Command) {
            for subcommand in command.get_subcommands() {
                assert!(
                    subcommand.get_bin_name().is_some(),
                    "missing bin_name for subcommand '{}'",
                    subcommand.get_name()
                );
                assert_tree(subcommand);
            }
        }

        assert_tree(&command);
    }

    #[test]
    fn clap_complete_utils_accept_the_command_tree() {
        let mut command = crate::cli::Cli::command();
        stabilize_command_tree(&mut command, "cc-switch");

        let subcommands = all_subcommands(&command);
        assert!(!subcommands.is_empty());
    }

    #[test]
    fn bash_completion_generation_does_not_panic() {
        let script = super::generate_completion_script(CompletionShell::Bash).expect("bash script");
        assert!(script.contains("cc-switch"));
        assert!(script.contains("provider"));
    }
}

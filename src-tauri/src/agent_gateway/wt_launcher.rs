use crate::agent_gateway::launcher_security::{
    build_agent_window_title, build_encoded_command, build_powershell_script, validate_cwd,
    validate_optional_shell_value, validate_shell_value,
};
use crate::agent_gateway::models::{
    AgentPermissionMode, LaunchAgentRequest, LaunchCommandPreview, LaunchStrategy,
};
use crate::claude_desktop_config::is_claude_safe_model_id;
use crate::error::AppError;
use serde_json::Value;
#[cfg(windows)]
use std::env;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

pub const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";
const CLAUDE_MODEL_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
];
const CLAUDE_CODE_RUNTIME_ENV_KEYS: &[&str] = &[
    "ENABLE_TOOL_SEARCH",
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
    "API_TIMEOUT_MS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
];

#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    pub strategy: LaunchStrategy,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub window_title: String,
    pub env: Vec<(String, String)>,
    pub settings_file: PathBuf,
}

impl PreparedLaunch {
    pub fn preview(&self) -> LaunchCommandPreview {
        LaunchCommandPreview {
            strategy: self.strategy.clone(),
            program: self.program.clone(),
            args_redacted: redact_launch_args(&self.args),
            cwd: self.cwd.as_ref().map(|path| path.display().to_string()),
            window_title: self.window_title.clone(),
            env_keys: self.env.iter().map(|(key, _)| key.clone()).collect(),
        }
    }

    /// Clean up temporary settings file
    pub fn cleanup_settings_file(&self) {
        let _ = std::fs::remove_file(&self.settings_file);
        if let Some(parent) = self.settings_file.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

fn redact_launch_args(args: &[String]) -> Vec<String> {
    let mut result = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            result.push("<redacted>".to_string());
            redact_next = false;
            continue;
        }
        result.push(arg.clone());
        if matches!(arg.as_str(), "-Command" | "-EncodedCommand") {
            redact_next = true;
        }
    }
    result
}

pub fn prepare_launch(
    req: &LaunchAgentRequest,
    agent_id: &str,
    port: u16,
    strategy: LaunchStrategy,
    provider_name: &str,
    upstream_model: &str,
    provider_settings_config: Option<&Value>,
) -> Result<PreparedLaunch, AppError> {
    validate_shell_value("agent_id", agent_id)?;
    validate_shell_value("agent name", &req.name)?;
    validate_shell_value("provider_id", &req.provider_id)?;
    validate_optional_shell_value("session_id", req.session_id.as_deref())?;
    validate_optional_shell_value("model", req.model.as_deref())?;
    validate_optional_shell_value("claude_entry_model", req.claude_entry_model.as_deref())?;
    validate_optional_shell_value(
        "upstream_provider_model",
        req.upstream_provider_model.as_deref(),
    )?;

    let cwd = validate_cwd(req.cwd.as_deref())?;
    let window_title = build_agent_window_title(agent_id, Some(provider_name), cwd.as_deref())?;

    // Create temp dir for the --settings file
    let settings_temp_dir = create_settings_temp_dir(agent_id)?;

    // Create isolated --settings file that overrides ~/.claude/settings.json.
    // Process env vars alone are NOT enough — Claude Code always reads
    // ~/.claude/settings.json and its `env` section overrides OS env vars.
    // By writing the proxy URL into an isolated settings file passed via
    // --settings we guarantee Claude Code sends requests to the local proxy.
    let settings_file =
        create_agent_settings_file(agent_id, provider_settings_config, &settings_temp_dir, port)?;

    let mut claude_args = Vec::new();
    claude_args.push("--settings".to_string());
    claude_args.push(settings_file.to_string_lossy().to_string());

    let cli_model = req
        .claude_entry_model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| {
            req.model
                .as_deref()
                .map(str::trim)
                .filter(|model| is_claude_safe_model_id(model))
        });
    if let Some(model) = cli_model {
        claude_args.push("--model".to_string());
        claude_args.push(model.to_string());
    }
    if let Some(session_id) = req
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        claude_args.push("--resume".to_string());
        claude_args.push(session_id.to_string());
    }
    claude_args.extend(
        req.permission_mode
            .clone()
            .unwrap_or(AgentPermissionMode::Default)
            .claude_args(),
    );

    let env = vec![
        (
            "ANTHROPIC_BASE_URL".to_string(),
            format!("http://127.0.0.1:{port}"),
        ),
        (
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            PROXY_TOKEN_PLACEHOLDER.to_string(),
        ),
    ];

    let (program, args) = match strategy {
        LaunchStrategy::WindowsTerminal => {
            let script = build_powershell_script(
                &env,
                "claude",
                &claude_args,
                agent_id,
                provider_name,
                upstream_model,
            )?;
            let encoded = build_encoded_command(&script);
            (
                resolve_command_program("wt.exe"),
                vec![
                    "-w".to_string(),
                    "0".to_string(),
                    "new-tab".to_string(),
                    "--title".to_string(),
                    window_title.clone(),
                    "powershell.exe".to_string(),
                    "-NoExit".to_string(),
                    "-ExecutionPolicy".to_string(),
                    "Bypass".to_string(),
                    "-EncodedCommand".to_string(),
                    encoded,
                ],
            )
        }
        LaunchStrategy::PowerShellWindow => {
            let script = build_powershell_script(
                &env,
                "claude",
                &claude_args,
                agent_id,
                provider_name,
                upstream_model,
            )?;
            let encoded = build_encoded_command(&script);
            (
                "powershell.exe".to_string(),
                vec![
                    "-NoExit".to_string(),
                    "-ExecutionPolicy".to_string(),
                    "Bypass".to_string(),
                    "-EncodedCommand".to_string(),
                    encoded,
                ],
            )
        }
        LaunchStrategy::BackgroundProcess => ("claude".to_string(), claude_args),
    };

    Ok(PreparedLaunch {
        strategy,
        program,
        args,
        cwd,
        window_title,
        env,
        settings_file,
    })
}

/// Create a temporary settings file for provider isolation
/// This file contains the provider's env vars and is passed to Claude via --settings
/// Create a temp directory for the isolated --settings file.
fn create_settings_temp_dir(agent_id: &str) -> Result<PathBuf, AppError> {
    let safe_id = agent_id.replace(['/', '\\', ':'], "_");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("ccs_agent_{}_{}_settings_tmp", safe_id, nonce));
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Message(format!("Failed to create settings temp dir: {e}")))?;
    Ok(dir)
}

/// Write an isolated `--settings` file that:
/// - Preserves the user's installed plugins (enabledPlugins, permissions, etc.)
/// - Overrides the proxy URL (ANTHROPIC_BASE_URL + AUTH_TOKEN) so requests
///   go through the local proxy regardless of ~/.claude/settings.json
/// - Adds provider model aliases from the CC Switch provider config
/// - Filters upstream route/auth secrets so they never reach Claude Code
///
/// Sessions are stored in Claude Code's default location (~/.claude/projects/
/// or cwd/.claude/projects/), so `--resume` works across agent restarts.
fn create_agent_settings_file(
    agent_id: &str,
    provider_settings_config: Option<&Value>,
    temp_dir: &Path,
    port: u16,
) -> Result<PathBuf, AppError> {
    // 1. Start from the user's ~/.claude/settings.json
    let user_settings_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("settings.json");
    let mut settings: serde_json::Value = match std::fs::read_to_string(&user_settings_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| {
            log::warn!("[Agent] Failed to parse user settings, using empty base");
            serde_json::json!({})
        }),
        Err(_) => {
            log::debug!("[Agent] No user settings found, starting fresh");
            serde_json::json!({})
        }
    };

    // 2. Build the env section:
    //    - Start with any existing user env keys (except route/auth)
    //    - Override proxy URL + auth token
    //    - Add provider model env keys
    let mut env_obj = settings
        .get("env")
        .and_then(|e| e.as_object())
        .cloned()
        .unwrap_or_default();

    // Remove stale route/auth keys from user settings (agent owns routing)
    env_obj.remove("ANTHROPIC_BASE_URL");
    env_obj.remove("ANTHROPIC_AUTH_TOKEN");
    env_obj.remove("ANTHROPIC_API_KEY");

    // Inject local proxy URL
    env_obj.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        Value::String(format!("http://127.0.0.1:{port}")),
    );
    env_obj.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        Value::String(PROXY_TOKEN_PLACEHOLDER.to_string()),
    );

    // Forward provider model aliases and runtime tuning keys.
    // Upstream route/auth keys are filtered — the local proxy owns routing.
    if let Some(config) = provider_settings_config {
        if let Some(env_vars) = config.get("env").and_then(|e| e.as_object()) {
            for (key, value) in env_vars {
                if is_claude_model_env_key(key) || is_claude_code_runtime_env_key(key) {
                    env_obj.insert(key.clone(), value.clone());
                }
            }
        }
    }

    // 3. Preserve user settings keys that matter for Claude Code behaviour
    //    (plugins, permissions, etc.) while overriding env.
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("env".to_string(), Value::Object(env_obj));
        // Remove keys that should NOT be in the agent settings file
        obj.remove("model"); // Agent uses --model flag, not settings file
        obj.remove("skipAutoPermissionPrompt"); // Managed by --permission-mode
        obj.remove("skipDangerousModePermissionPrompt");
    }

    let settings_json = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppError::Message(format!("Failed to serialize settings: {e}")))?;

    let settings_file = temp_dir.join(format!("ccs_agent_{}_settings.json", agent_id));
    std::fs::write(&settings_file, settings_json)
        .map_err(|e| AppError::Message(format!("Failed to write settings file: {e}")))?;

    log::info!(
        "[Agent] Created agent settings file: {} (port={port})",
        settings_file.display()
    );

    Ok(settings_file)
}

fn is_claude_model_env_key(key: &str) -> bool {
    CLAUDE_MODEL_ENV_KEYS.contains(&key)
}

fn is_claude_code_runtime_env_key(key: &str) -> bool {
    CLAUDE_CODE_RUNTIME_ENV_KEYS.contains(&key)
}

pub fn command_exists(program: &str) -> bool {
    resolve_command_path(program).is_some()
}

fn resolve_command_program(program: &str) -> String {
    resolve_command_path(program)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| program.to_string())
}

fn resolve_command_path(program: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut command = Command::new("where.exe");
        command.arg(program);
        command.creation_flags(CREATE_NO_WINDOW);
        if let Ok(output) = command.output() {
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
                    if let Some(first_path) =
                        stdout.lines().map(str::trim).find(|line| !line.is_empty())
                    {
                        return Some(PathBuf::from(first_path));
                    }
                }
                return Some(PathBuf::from(program));
            }
        }

        if program.eq_ignore_ascii_case("wt.exe") || program.eq_ignore_ascii_case("wt") {
            if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
                let wt_path = PathBuf::from(local_app_data)
                    .join("Microsoft")
                    .join("WindowsApps")
                    .join("wt.exe");
                if wt_path.exists() {
                    return Some(wt_path);
                }
            }
        }

        None
    }

    #[cfg(not(windows))]
    {
        Command::new("which")
            .arg(program)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|stdout| {
                stdout
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .map(PathBuf::from)
            })
    }
}

pub fn strategy_available(strategy: &LaunchStrategy) -> bool {
    match strategy {
        LaunchStrategy::WindowsTerminal => command_exists("wt.exe"),
        LaunchStrategy::PowerShellWindow => command_exists("powershell.exe"),
        LaunchStrategy::BackgroundProcess => command_exists("claude"),
    }
}

pub fn launch_with_strategy(prepared: &PreparedLaunch) -> Result<u32, AppError> {
    let mut command = Command::new(&prepared.program);
    command.args(&prepared.args);
    command.envs(prepared.env.iter().map(|(key, value)| (key, value)));
    if let Some(cwd) = &prepared.cwd {
        command.current_dir(cwd);
    }
    #[cfg(windows)]
    if prepared.strategy == LaunchStrategy::BackgroundProcess {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| AppError::Message(format!("LAUNCH_STRATEGY_FAILED: {e}")))?;
    let pid = child.id();

    if prepared.strategy == LaunchStrategy::BackgroundProcess {
        let settings_file = prepared.settings_file.clone();
        std::thread::spawn(move || {
            let _ = child.wait();
            if let Some(parent) = settings_file.parent() {
                let _ = std::fs::remove_file(&settings_file);
                let _ = std::fs::remove_dir(parent);
            }
        });
    }

    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::prepare_launch;
    use crate::agent_gateway::models::{
        AgentPermissionMode, AgentRuntimeKind, LaunchAgentRequest, LaunchStrategy,
    };
    use serde_json::json;

    fn request() -> LaunchAgentRequest {
        LaunchAgentRequest {
            name: "Agent".to_string(),
            runtime: AgentRuntimeKind::ClaudeCode,
            provider_id: "provider-1".to_string(),
            provider_mode: None,
            model: None,
            claude_entry_model: None,
            upstream_provider_model: None,
            run_profile_id: Some("safe".to_string()),
            cwd: None,
            session_id: None,
            launch_strategy: None,
            permission_mode: None,
        }
    }

    #[test]
    fn launch_preview_injects_only_native_proxy_env() {
        let prepared = prepare_launch(
            &request(),
            "agent-1",
            15722,
            LaunchStrategy::PowerShellWindow,
            "TestProvider",
            "test-model",
            None,
        )
        .expect("prepare");
        let preview = prepared.preview();
        assert_eq!(preview.window_title, "CCSA:agent-1/TestProvider");
        assert!(preview
            .env_keys
            .iter()
            .any(|key| key == "ANTHROPIC_BASE_URL"));
        assert!(preview
            .env_keys
            .iter()
            .any(|key| key == "ANTHROPIC_AUTH_TOKEN"));
        // CLAUDE_CONFIG_DIR was removed — it is NOT a recognized Claude Code env var
        assert!(!preview
            .env_keys
            .iter()
            .any(|key| key == "CLAUDE_CONFIG_DIR"));
        assert!(preview.args_redacted.iter().any(|arg| arg == "<redacted>"));
        prepared.cleanup_settings_file();
    }

    #[test]
    fn launch_preview_rejects_bad_session_id() {
        let mut req = request();
        req.session_id = Some("abc;def".to_string());
        assert!(prepare_launch(
            &req,
            "agent-1",
            15722,
            LaunchStrategy::PowerShellWindow,
            "TestProvider",
            "test-model",
            None
        )
        .is_err());
    }

    #[test]
    fn launch_preview_includes_trimmed_resume_session_id() {
        let mut req = request();
        req.session_id = Some("  session-123  ".to_string());
        let prepared = prepare_launch(
            &req,
            "agent-1",
            15722,
            LaunchStrategy::BackgroundProcess,
            "TestProvider",
            "test-model",
            None,
        )
        .expect("prepare");

        assert!(prepared
            .args
            .windows(2)
            .any(|pair| pair == ["--resume", "session-123"]));
        prepared.cleanup_settings_file();
    }

    #[test]
    fn launch_preview_includes_explicit_permission_mode() {
        let mut req = request();
        req.permission_mode = Some(AgentPermissionMode::BypassPermissions);
        let prepared = prepare_launch(
            &req,
            "agent-1",
            15722,
            LaunchStrategy::BackgroundProcess,
            "TestProvider",
            "test-model",
            None,
        )
        .expect("prepare");
        assert!(prepared
            .args
            .iter()
            .any(|arg| arg.contains("--dangerously-skip-permissions")));
        prepared.cleanup_settings_file();
    }

    #[test]
    fn launch_preview_includes_safe_explicit_model() {
        let mut req = request();
        req.claude_entry_model = Some("claude-sonnet-4-6".to_string());
        let prepared = prepare_launch(
            &req,
            "agent-1",
            15722,
            LaunchStrategy::BackgroundProcess,
            "TestProvider",
            "test-model",
            None,
        )
        .expect("prepare");
        assert_eq!(prepared.program, "claude");
        assert!(prepared
            .args
            .windows(2)
            .any(|pair| pair == ["--model", "claude-sonnet-4-6"]));
        prepared.cleanup_settings_file();
    }

    #[test]
    fn upstream_provider_model_is_not_passed_to_claude_model_arg() {
        let mut req = request();
        req.model = Some("deepseek-v4-pro[1M]".to_string());
        req.upstream_provider_model = Some("deepseek-v4-pro[1M]".to_string());
        let prepared = prepare_launch(
            &req,
            "agent-1",
            15722,
            LaunchStrategy::BackgroundProcess,
            "TestProvider",
            "test-model",
            None,
        )
        .expect("prepare");
        assert!(!prepared.args.iter().any(|arg| arg == "--model"));
        assert!(!prepared.args.iter().any(|arg| arg == "deepseek-v4-pro[1M]"));
        prepared.cleanup_settings_file();
    }

    #[test]
    fn provider_settings_filters_routes_and_secrets_but_preserves_models() {
        let config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                "ANTHROPIC_AUTH_TOKEN": "sk-secret",
                "ANTHROPIC_API_KEY": "sk-other-secret",
                "OPENAI_API_KEY": "sk-openai-secret",
                "ANTHROPIC_MODEL": "kimi-k2.6",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-5.1",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro[1M]",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro",
                "ANTHROPIC_SMALL_FAST_MODEL": "mimo-v2.5",
                "ENABLE_TOOL_SEARCH": "true",
                "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1",
                "API_TIMEOUT_MS": "3000000",
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": 1
            }
        });

        let prepared = prepare_launch(
            &request(),
            "agent-settings-filter",
            15722,
            LaunchStrategy::BackgroundProcess,
            "OpenCode Go",
            "kimi-k2.6",
            Some(&config),
        )
        .expect("prepare");

        let path = &prepared.settings_file;
        let encoded = std::fs::read_to_string(path).expect("read settings");
        let settings: serde_json::Value = serde_json::from_str(&encoded).expect("parse settings");
        let env = settings["env"].as_object().expect("settings env");

        assert_eq!(env.get("ENABLE_TOOL_SEARCH"), Some(&json!("true")));
        assert_eq!(
            env.get("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
            Some(&json!("1"))
        );
        assert_eq!(env.get("API_TIMEOUT_MS"), Some(&json!("3000000")));
        assert_eq!(
            env.get("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"),
            Some(&json!(1))
        );
        assert_eq!(env.get("ANTHROPIC_MODEL"), Some(&json!("kimi-k2.6")));
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            Some(&json!("glm-5.1"))
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_SONNET_MODEL"),
            Some(&json!("deepseek-v4-pro[1M]"))
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_OPUS_MODEL"),
            Some(&json!("deepseek-v4-pro"))
        );
        assert_eq!(
            env.get("ANTHROPIC_SMALL_FAST_MODEL"),
            Some(&json!("mimo-v2.5"))
        );
        // Upstream route/auth keys MUST be filtered (the local proxy owns routing).
        for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"] {
            assert!(
                !env.contains_key(key),
                "{key} from upstream should be filtered"
            );
        }
        // ANTHROPIC_BASE_URL is injected as the LOCAL proxy address (not the
        // upstream URL), so it IS present — but it must point to localhost.
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL"),
            Some(&json!("http://127.0.0.1:15722")),
            "ANTHROPIC_BASE_URL must be the local proxy"
        );
        // ANTHROPIC_AUTH_TOKEN is set to the dummy placeholder value.
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN"),
            Some(&json!("PROXY_MANAGED")),
            "ANTHROPIC_AUTH_TOKEN must be the proxy placeholder"
        );
        // Upstream secrets must NOT leak into the settings file.
        for forbidden in [
            "sk-secret",
            "sk-other-secret",
            "sk-openai-secret",
            "opencode.ai",
        ] {
            assert!(!encoded.contains(forbidden), "{forbidden} leaked");
        }

        prepared.cleanup_settings_file();
    }

    #[test]
    fn provider_settings_preserves_all_model_env_keys() {
        let config = json!({
            "env": {
                "ANTHROPIC_MODEL": "claude-sonnet-4-6",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "anthropic/claude-haiku-4-5",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro",
                "ENABLE_TOOL_SEARCH": "true"
            }
        });

        let prepared = prepare_launch(
            &request(),
            "agent-settings-safe-model",
            15722,
            LaunchStrategy::BackgroundProcess,
            "Claude Safe",
            "claude-sonnet-4-6",
            Some(&config),
        )
        .expect("prepare");
        let path = &prepared.settings_file;
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read settings"))
                .expect("parse settings");
        let env = settings["env"].as_object().expect("settings env");

        assert_eq!(
            env.get("ANTHROPIC_MODEL"),
            Some(&json!("claude-sonnet-4-6"))
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            Some(&json!("anthropic/claude-haiku-4-5"))
        );
        assert_eq!(
            env.get("ANTHROPIC_DEFAULT_SONNET_MODEL"),
            Some(&json!("deepseek-v4-pro"))
        );
        assert_eq!(env.get("ENABLE_TOOL_SEARCH"), Some(&json!("true")));

        prepared.cleanup_settings_file();
    }

    #[test]
    fn provider_settings_keeps_file_when_model_env_is_present() {
        let config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://opencode.ai/zen/go/v1/chat/completions",
                "ANTHROPIC_AUTH_TOKEN": "sk-secret",
                "ANTHROPIC_MODEL": "kimi-k2.6"
            }
        });

        let prepared = prepare_launch(
            &request(),
            "agent-settings-empty",
            15722,
            LaunchStrategy::BackgroundProcess,
            "OpenCode Go",
            "kimi-k2.6",
            Some(&config),
        )
        .expect("prepare");

        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&prepared.settings_file).expect("read settings"),
        )
        .expect("parse settings");
        let env = settings["env"].as_object().expect("settings env");
        assert_eq!(env.get("ANTHROPIC_MODEL"), Some(&json!("kimi-k2.6")));
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL"),
            Some(&json!("http://127.0.0.1:15722"))
        );
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN"),
            Some(&serde_json::json!("PROXY_MANAGED")),
            "auth token must be the proxy placeholder"
        );
        // CLAUDE_CONFIG_DIR was removed — it is NOT a recognized Claude Code env var
        assert!(!prepared
            .env
            .iter()
            .any(|(key, _)| key == "CLAUDE_CONFIG_DIR"));
        prepared.cleanup_settings_file();
    }

    #[test]
    fn windows_terminal_uses_encoded_command() {
        let prepared = prepare_launch(
            &request(),
            "agent-1",
            15722,
            LaunchStrategy::WindowsTerminal,
            "TestProvider",
            "test-model",
            None,
        )
        .expect("prepare");
        assert!(prepared.args.iter().any(|arg| arg == "powershell.exe"));
        assert!(prepared.args.iter().any(|arg| arg == "-EncodedCommand"));
        assert!(!prepared.args.iter().any(|arg| arg == "-Command"));
        let encoded_index = prepared
            .args
            .iter()
            .position(|arg| arg == "-EncodedCommand")
            .expect("encoded command arg");
        assert!(!prepared.args[encoded_index + 1].contains(';'));
        prepared.cleanup_settings_file();
    }

    #[test]
    fn powershell_window_fallback_uses_encoded_command() {
        let prepared = prepare_launch(
            &request(),
            "agent-1",
            15722,
            LaunchStrategy::PowerShellWindow,
            "TestProvider",
            "test-model",
            None,
        )
        .expect("prepare");
        assert_eq!(prepared.program, "powershell.exe");
        assert!(prepared.args.iter().any(|arg| arg == "-EncodedCommand"));
        prepared.cleanup_settings_file();
    }

    #[test]
    fn background_process_still_launches_claude_directly() {
        let prepared = prepare_launch(
            &request(),
            "agent-1",
            15722,
            LaunchStrategy::BackgroundProcess,
            "TestProvider",
            "test-model",
            None,
        )
        .expect("prepare");
        assert_eq!(prepared.program, "claude");
        assert!(!prepared.args.iter().any(|arg| arg == "-EncodedCommand"));
        // CLAUDE_CONFIG_DIR was removed — it is NOT a recognized Claude Code env var
        assert!(!prepared
            .env
            .iter()
            .any(|(key, _)| key == "CLAUDE_CONFIG_DIR"));
        prepared.cleanup_settings_file();
    }
}

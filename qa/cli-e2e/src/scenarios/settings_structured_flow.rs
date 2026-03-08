use anyhow::Result;

use crate::asserts::{ensure, read_text, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::args;

const NAME: &str = "settings_structured_flow";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description:
            "structured settings commands update language, host prefs, plugin and onboarding state",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        let language_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "settings",
                "language",
                "set",
                "zh",
            ]))
            .await?;
        let language = stdout_json(&language_output)?;
        ensure(language["language"] == "zh", "settings language set should persist zh")?;

        let visible_apps_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "settings",
                "visible-apps",
                "set",
                "--codex",
                "false",
                "--openclaw",
                "false",
            ]))
            .await?;
        let visible_apps = stdout_json(&visible_apps_output)?;
        ensure(
            visible_apps["visibleApps"]["codex"] == false
                && visible_apps["visibleApps"]["openclaw"] == false,
            "settings visible-apps set should update selected flags",
        )?;

        let terminal_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "settings",
                "terminal",
                "set",
                "wezterm",
            ]))
            .await?;
        let terminal = stdout_json(&terminal_output)?;
        ensure(
            terminal["preferredTerminal"] == "wezterm",
            "settings terminal set should persist preferred terminal",
        )?;

        let startup_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "settings",
                "startup",
                "set",
                "--show-in-tray",
                "false",
                "--launch-on-startup",
                "true",
                "--silent-startup",
                "true",
            ]))
            .await?;
        let startup = stdout_json(&startup_output)?;
        ensure(
            startup["showInTray"] == false
                && startup["launchOnStartup"] == true
                && startup["silentStartup"] == true,
            "settings startup set should persist startup-related flags",
        )?;

        let plugin_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "settings",
                "plugin",
                "enable",
            ]))
            .await?;
        let plugin = stdout_json(&plugin_output)?;
        ensure(
            plugin["enabledInSettings"] == true && plugin["applied"] == true,
            "settings plugin enable should sync Claude plugin config",
        )?;
        ensure(
            sandbox.home_path(".claude/config.json").exists(),
            "Claude plugin config file should exist after enable",
        )?;

        let onboarding_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "settings",
                "onboarding",
                "skip",
            ]))
            .await?;
        let onboarding = stdout_json(&onboarding_output)?;
        ensure(
            onboarding["skipInSettings"] == true && onboarding["applied"] == true,
            "settings onboarding skip should persist and apply the marker",
        )?;
        let onboarding_file = read_text(&sandbox.home_path(".claude.json"))?;
        ensure(
            onboarding_file.contains("hasCompletedOnboarding"),
            "Claude onboarding file should contain the completion marker",
        )?;

        let show_output = sandbox
            .run_ok(&args(&["--format", "json", "settings", "show"]))
            .await?;
        let shown = stdout_json(&show_output)?;
        ensure(
            shown["language"] == "zh"
                && shown["preferredTerminal"] == "wezterm"
                && shown["launchOnStartup"] == true,
            "settings show should reflect the saved structured values",
        )?;

        Ok(())
    }
    .await;

    sandbox.finalize(result.is_ok(), None)?;
    result
}

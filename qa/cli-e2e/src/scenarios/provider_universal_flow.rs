use anyhow::Result;

use crate::asserts::{assert_contains, ensure, read_json, read_text, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "provider_universal_flow";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "provider universal show/edit/save-and-sync keeps CLI and live config in sync",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;

        let add_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
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
            ]))
            .await?;
        let added = stdout_json(&add_output)?;
        ensure(added["id"] == "omni", "universal add did not return the saved provider")?;

        let show_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "universal",
                "show",
                "omni",
            ]))
            .await?;
        let shown = stdout_json(&show_output)?;
        ensure(shown["name"] == "Omni", "universal show returned unexpected name")?;

        let edit_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "universal",
                "edit",
                "omni",
                "--set-name",
                "Omni Prime",
                "--set-apps",
                "claude,codex,gemini",
                "--set-base-url",
                "https://api2.example.com",
                "--set-api-key",
                "sk-prime",
            ]))
            .await?;
        let edited = stdout_json(&edit_output)?;
        ensure(
            edited["baseUrl"] == "https://api2.example.com",
            "universal edit did not persist the new base URL",
        )?;
        ensure(
            edited["apps"]["gemini"] == true,
            "universal edit did not persist the updated app list",
        )?;

        let sync_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "universal",
                "sync",
                "omni",
            ]))
            .await?;
        let sync_json = stdout_json(&sync_output)?;
        ensure(
            sync_json["syncedApps"]
                .as_array()
                .is_some_and(|items| items.len() == 3),
            "universal sync should report all enabled target apps",
        )?;

        let claude = read_json(&sandbox.home_path(".claude/settings.json"))?;
        ensure(
            claude["env"]["ANTHROPIC_BASE_URL"] == "https://api2.example.com",
            "universal sync did not update Claude live config",
        )?;
        let codex_toml = read_text(&sandbox.home_path(".codex/config.toml"))?;
        assert_contains(
            &codex_toml,
            "https://api2.example.com",
            "codex config.toml after universal sync",
        )?;
        let gemini_env = read_text(&sandbox.home_path(".gemini/.env"))?;
        assert_contains(
            &gemini_env,
            "GOOGLE_GEMINI_BASE_URL=https://api2.example.com",
            "gemini .env after universal sync",
        )?;

        let save_and_sync_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "universal",
                "save-and-sync",
                "--name",
                "Nova",
                "--apps",
                "claude,gemini",
                "--base-url",
                "https://nova.example.com",
                "--api-key",
                "sk-nova",
            ]))
            .await?;
        let save_and_sync = stdout_json(&save_and_sync_output)?;
        ensure(
            save_and_sync["provider"]["id"] == "nova",
            "save-and-sync did not return the saved universal provider",
        )?;

        let provider_list = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "list",
                "--app",
                "claude",
            ]))
            .await?;
        let provider_list_json = stdout_json(&provider_list)?;
        ensure(
            provider_list_json.get("universal-claude-nova").is_some(),
            "save-and-sync did not push the new universal provider into Claude providers",
        )?;

        sandbox
            .run_ok(&args(&["provider", "universal", "delete", "omni", "--yes"]))
            .await?;
        sandbox
            .run_ok(&args(&["provider", "universal", "delete", "nova", "--yes"]))
            .await?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}

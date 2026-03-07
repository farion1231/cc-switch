use anyhow::Result;

use crate::asserts::{assert_contains, ensure, read_json, read_text, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::finalize;

const NAME: &str = "provider_live_switch";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "provider add/switch writes live configs for all supported apps",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;

        let apps = vec![
            (
                "claude",
                "providers/claude-settings.json",
                "Claude Alpha",
                "Claude Beta",
            ),
            (
                "codex",
                "providers/codex-settings.json",
                "Codex Alpha",
                "Codex Beta",
            ),
            (
                "gemini",
                "providers/gemini-settings.json",
                "Gemini Alpha",
                "Gemini Beta",
            ),
            (
                "opencode",
                "providers/opencode-settings.json",
                "OpenCode Alpha",
                "OpenCode Beta",
            ),
            (
                "openclaw",
                "providers/openclaw-settings.json",
                "OpenClaw Alpha",
                "OpenClaw Beta",
            ),
        ];

        for (app, fixture, alpha, beta) in &apps {
            let fixture_path = sandbox.fixture_path(fixture);
            sandbox
                .run_ok(&vec![
                    "provider".to_string(),
                    "add".to_string(),
                    "--app".to_string(),
                    (*app).to_string(),
                    "--from-json".to_string(),
                    fixture_path.display().to_string(),
                    "--name".to_string(),
                    (*alpha).to_string(),
                    "--base-url".to_string(),
                    format!("https://alpha.{}.mock/v1", app),
                    "--api-key".to_string(),
                    format!("alpha-{}-key", app),
                ])
                .await?;

            sandbox
                .run_ok(&vec![
                    "provider".to_string(),
                    "add".to_string(),
                    "--app".to_string(),
                    (*app).to_string(),
                    "--from-json".to_string(),
                    fixture_path.display().to_string(),
                    "--name".to_string(),
                    (*beta).to_string(),
                    "--base-url".to_string(),
                    format!("https://beta.{}.mock/v1", app),
                    "--api-key".to_string(),
                    format!("beta-{}-key", app),
                ])
                .await?;

            let beta_id = beta.to_lowercase().replace(' ', "-");
            sandbox
                .run_ok(&vec![
                    "provider".to_string(),
                    "switch".to_string(),
                    beta_id.clone(),
                    "--app".to_string(),
                    (*app).to_string(),
                ])
                .await?;
        }

        let claude = read_json(&sandbox.home_path(".claude/settings.json"))?;
        ensure(
            claude["env"]["ANTHROPIC_BASE_URL"] == "https://beta.claude.mock/v1",
            "claude live base url mismatch",
        )?;
        ensure(
            claude["env"]["ANTHROPIC_AUTH_TOKEN"] == "beta-claude-key",
            "claude live api key mismatch",
        )?;

        let codex_auth = read_json(&sandbox.home_path(".codex/auth.json"))?;
        ensure(
            codex_auth["OPENAI_API_KEY"] == "beta-codex-key",
            "codex auth.json mismatch",
        )?;
        let codex_toml = read_text(&sandbox.home_path(".codex/config.toml"))?;
        assert_contains(
            &codex_toml,
            "https://beta.codex.mock/v1",
            "codex config.toml",
        )?;

        let gemini_env = read_text(&sandbox.home_path(".gemini/.env"))?;
        assert_contains(
            &gemini_env,
            "GOOGLE_GEMINI_BASE_URL=https://beta.gemini.mock/v1",
            "gemini .env",
        )?;
        assert_contains(&gemini_env, "GEMINI_API_KEY=beta-gemini-key", "gemini .env")?;

        let opencode = read_json(&sandbox.home_path(".config/opencode/opencode.json"))?;
        ensure(
            opencode["provider"]["opencode-beta"]["options"]["baseURL"]
                == "https://beta.opencode.mock/v1",
            "opencode live base url mismatch",
        )?;
        ensure(
            opencode["provider"]["opencode-beta"]["options"]["apiKey"] == "beta-opencode-key",
            "opencode live api key mismatch",
        )?;

        let openclaw = read_json(&sandbox.home_path(".openclaw/openclaw.json"))?;
        ensure(
            openclaw["models"]["providers"]["openclaw-beta"]["baseUrl"]
                == "https://beta.openclaw.mock/v1",
            "openclaw live base url mismatch",
        )?;
        ensure(
            openclaw["models"]["providers"]["openclaw-beta"]["apiKey"] == "beta-openclaw-key",
            "openclaw live api key mismatch",
        )?;

        for (key, expected) in [
            ("currentProviderClaude", "claude-beta"),
            ("currentProviderCodex", "codex-beta"),
            ("currentProviderGemini", "gemini-beta"),
        ] {
            let output = sandbox
                .run_ok(&vec![
                    "config".to_string(),
                    "get".to_string(),
                    key.to_string(),
                    "--format".to_string(),
                    "json".to_string(),
                ])
                .await?;
            let json = stdout_json(&output)?;
            ensure(
                json[key] == expected,
                format!("{key} did not point to switched provider"),
            )?;
        }

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}

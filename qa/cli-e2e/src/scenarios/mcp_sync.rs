use anyhow::Result;

use crate::asserts::{
    assert_contains, assert_not_contains, ensure, read_json, read_text, stdout_json,
};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "mcp_sync";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "mcp add/edit/delete/import syncs live configs across apps",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;

        let full_server = sandbox.fixture_path("mcp/full-server.json");
        sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "mcp".to_string(),
                "add".to_string(),
                "--from-json".to_string(),
                full_server.display().to_string(),
            ])
            .await?;

        let claude_mcp = read_json(&sandbox.home_path(".claude.json"))?;
        ensure(
            claude_mcp["mcpServers"].get("demo-mcp").is_some(),
            "claude MCP file missing demo-mcp",
        )?;
        assert_contains(
            &read_text(&sandbox.home_path(".codex/config.toml"))?,
            "demo-mcp",
            "codex MCP config",
        )?;
        ensure(
            read_json(&sandbox.home_path(".gemini/settings.json"))?["mcpServers"]
                .get("demo-mcp")
                .is_some(),
            "gemini MCP settings missing demo-mcp",
        )?;
        assert_contains(
            &read_text(&sandbox.home_path(".config/opencode/opencode.json"))?,
            "demo-mcp",
            "opencode MCP config",
        )?;

        let edit_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "mcp",
                "edit",
                "demo-mcp",
                "--disable-app",
                "gemini",
            ]))
            .await?;
        let edit_json = stdout_json(&edit_output)?;
        ensure(
            edit_json["apps"]["gemini"] == false,
            "mcp edit did not return the updated app toggles",
        )?;
        let gemini_settings = read_json(&sandbox.home_path(".gemini/settings.json"))?;
        ensure(
            gemini_settings["mcpServers"].get("demo-mcp").is_none(),
            "gemini MCP entry should be removed after disable-app",
        )?;

        let validate_output = sandbox
            .run_ok(&args(&["--format", "json", "mcp", "validate", "demo-mcp"]))
            .await?;
        let validate_json = stdout_json(&validate_output)?;
        ensure(
            validate_json["valid"] == true,
            "mcp validate should mark demo-mcp as valid",
        )?;

        let docs_output = sandbox
            .run_ok(&args(&["--format", "json", "mcp", "docs-link", "demo-mcp"]))
            .await?;
        let docs_json = stdout_json(&docs_output)?;
        ensure(
            docs_json["homepage"] == "https://example.com/mcp",
            "mcp docs-link should expose homepage",
        )?;
        ensure(
            docs_json["docs"] == "https://example.com/mcp/docs",
            "mcp docs-link should expose docs url",
        )?;

        let delete_without_yes = sandbox.run(&args(&["mcp", "delete", "demo-mcp"])).await?;
        ensure(
            !delete_without_yes.success(),
            "mcp delete without --yes should fail",
        )?;
        sandbox
            .run_ok(&args(&["mcp", "delete", "demo-mcp", "--yes"]))
            .await?;
        assert_not_contains(
            &read_text(&sandbox.home_path(".claude.json"))?,
            "demo-mcp",
            "claude MCP config after delete",
        )?;

        sandbox
            .stage_fixture_to_path("mcp/claude-import.json", &sandbox.home_path(".claude.json"))?;
        sandbox.run_ok(&args(&["mcp", "import"])).await?;
        let imported = sandbox
            .run_ok(&args(&["mcp", "list", "--format", "json"]))
            .await?;
        let imported_json = stdout_json(&imported)?;
        ensure(
            imported_json.get("imported-mcp").is_some(),
            "mcp import did not bring imported-mcp into SSOT",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}

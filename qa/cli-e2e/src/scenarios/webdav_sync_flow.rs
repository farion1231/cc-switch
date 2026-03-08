use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::mock::WebDavServer;
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::args;

const NAME: &str = "webdav_sync_flow";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "webdav save/test/upload/download/remote-info round-trips database state",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let mock = WebDavServer::start().await?;
    let result = async {
        let save_output = sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "webdav".to_string(),
                "save".to_string(),
                "--base-url".to_string(),
                mock.base_url(),
                "--username".to_string(),
                "demo".to_string(),
                "--password".to_string(),
                "secret".to_string(),
                "--remote-root".to_string(),
                "sync-root".to_string(),
                "--profile".to_string(),
                "qa".to_string(),
                "--enable".to_string(),
            ])
            .await?;
        let saved = stdout_json(&save_output)?;
        ensure(saved["success"] == true, "webdav save should succeed")?;
        ensure(
            saved["settings"]["passwordConfigured"] == true,
            "webdav save should record that a password is configured",
        )?;

        let show_output = sandbox
            .run_ok(&args(&["--format", "json", "webdav", "show"]))
            .await?;
        let shown = stdout_json(&show_output)?;
        ensure(shown["configured"] == true, "webdav show should report configured")?;

        let test_output = sandbox
            .run_ok(&args(&["--format", "json", "webdav", "test"]))
            .await?;
        let tested = stdout_json(&test_output)?;
        ensure(tested["success"] == true, "webdav test should succeed")?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "add",
                "--app",
                "claude",
                "--name",
                "before-webdav",
                "--base-url",
                "https://before.example.com",
                "--api-key",
                "sk-before",
            ]))
            .await?;

        let upload_output = sandbox
            .run_ok(&args(&["--format", "json", "webdav", "upload"]))
            .await?;
        let uploaded = stdout_json(&upload_output)?;
        ensure(
            uploaded["status"] == "uploaded",
            "webdav upload should report uploaded status",
        )?;

        let info_output = sandbox
            .run_ok(&args(&["--format", "json", "webdav", "remote-info"]))
            .await?;
        let info = stdout_json(&info_output)?;
        ensure(
            info["compatible"] == true && info["version"] == 2,
            "webdav remote-info should expose a compatible v2 snapshot",
        )?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "add",
                "--app",
                "claude",
                "--name",
                "after-webdav",
                "--base-url",
                "https://after.example.com",
                "--api-key",
                "sk-after",
            ]))
            .await?;

        let download_output = sandbox
            .run_ok(&args(&["--format", "json", "webdav", "download"]))
            .await?;
        let downloaded = stdout_json(&download_output)?;
        ensure(
            downloaded["status"] == "downloaded",
            "webdav download should report downloaded status",
        )?;

        let providers_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "list",
                "--app",
                "claude",
            ]))
            .await?;
        let providers = stdout_json(&providers_output)?;
        ensure(
            providers.as_object().is_some_and(|items| {
                items.len() == 1
                    && items
                        .values()
                        .any(|provider| provider["name"] == "before-webdav")
            }),
            "webdav download should restore the uploaded provider snapshot",
        )?;

        Ok(())
    }
    .await;

    let mock_requests = Some(mock.requests_json().await?);
    sandbox.finalize(result.is_ok(), mock_requests)?;
    mock.shutdown().await;
    result
}

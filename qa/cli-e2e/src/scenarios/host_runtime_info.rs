use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "host_runtime_info";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "auto-launch, portable-mode, tool-versions and about/update info work in sandbox",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        let portable_root = sandbox.work_path("portable-app");
        fs::create_dir_all(&portable_root)?;
        fs::write(portable_root.join("portable.ini"), "")?;
        fs::write(portable_root.join("cc-switch"), "")?;

        let bin_dir = sandbox.work_path("bin");
        fs::create_dir_all(&bin_dir)?;
        #[cfg(unix)]
        write_executable(&bin_dir.join("claude"), "#!/bin/sh\necho 'claude 1.2.3'\n")?;

        let npm_server = spawn_json_server(HashMap::from([(
            "/@anthropic-ai/claude-code".to_string(),
            r#"{"dist-tags":{"latest":"9.9.9"}}"#.to_string(),
        )]))?;
        let github_server = spawn_json_server(HashMap::from([(
            "/repos/farion1231/cc-switch/releases/latest".to_string(),
            r#"{"tag_name":"v99.1.0"}"#.to_string(),
        )]))?;

        let fake_exe = portable_root.join("cc-switch");
        let state_file = sandbox.work_path("auto-launch.state");
        let path_value = prepend_path(&bin_dir)?;
        let fake_exe_value = fake_exe.display().to_string();
        let state_file_value = state_file.display().to_string();
        let common_env = [
            ("CC_SWITCH_TEST_CURRENT_EXE", fake_exe_value.as_str()),
            (
                "CC_SWITCH_TEST_AUTO_LAUNCH_STATE_FILE",
                state_file_value.as_str(),
            ),
            ("PATH", path_value.as_str()),
            (
                "CC_SWITCH_TEST_NPM_REGISTRY_BASE_URL",
                npm_server.as_str(),
            ),
            (
                "CC_SWITCH_TEST_GITHUB_API_BASE_URL",
                github_server.as_str(),
            ),
        ];

        let portable_output = sandbox
            .run_ok_with_env(&args(&["--format", "json", "portable-mode"]), &common_env)
            .await?;
        let portable_json = stdout_json(&portable_output)?;
        ensure(
            portable_json["portableMode"] == true,
            "portable-mode should report true when marker exists next to fake executable",
        )?;

        let about_output = sandbox
            .run_ok_with_env(&args(&["--format", "json", "about"]), &common_env)
            .await?;
        let about_json = stdout_json(&about_output)?;
        ensure(about_json["portableMode"] == true, "about should surface portable mode")?;
        ensure(
            about_json["currentReleaseNotesUrl"]
                .as_str()
                .is_some_and(|value| value.contains("/releases/tag/v")),
            "about should include the current release notes URL",
        )?;

        let status_output = sandbox
            .run_ok_with_env(&args(&["--format", "json", "auto-launch", "status"]), &common_env)
            .await?;
        let status_json = stdout_json(&status_output)?;
        ensure(
            status_json["enabled"] == false && status_json["launchOnStartup"] == false,
            "auto-launch status should start disabled in test state file mode",
        )?;

        let enable_output = sandbox
            .run_ok_with_env(&args(&["--format", "json", "auto-launch", "enable"]), &common_env)
            .await?;
        let enable_json = stdout_json(&enable_output)?;
        ensure(
            enable_json["enabled"] == true && enable_json["launchOnStartup"] == true,
            "auto-launch enable should flip both runtime and persisted preference",
        )?;

        let tool_output = sandbox
            .run_ok_with_env(
                &args(&[
                    "--format",
                    "json",
                    "tool-versions",
                    "--tool",
                    "claude",
                    "--latest",
                ]),
                &common_env,
            )
            .await?;
        let tool_json = stdout_json(&tool_output)?;
        let tool = tool_json
            .as_array()
            .and_then(|items| items.first())
            .ok_or_else(|| anyhow::anyhow!("tool-versions should return one row"))?;
        ensure(
            tool["version"] == "1.2.3" && tool["latestVersion"] == "9.9.9",
            "tool-versions should report both local and mocked latest versions",
        )?;

        let update_output = sandbox
            .run_ok_with_env(&args(&["--format", "json", "update", "check"]), &common_env)
            .await?;
        let update_json = stdout_json(&update_output)?;
        ensure(
            update_json["latestVersion"] == "99.1.0" && update_json["hasUpdate"] == true,
            "update check should surface mocked latest release info",
        )?;

        let release_notes_output = sandbox
            .run_ok_with_env(&args(&["--format", "json", "release-notes"]), &common_env)
            .await?;
        let release_notes_json = stdout_json(&release_notes_output)?;
        ensure(
            release_notes_json["url"]
                .as_str()
                .is_some_and(|value| value.contains("/releases/tag/v")),
            "release-notes should point to the current version page",
        )?;

        let latest_release_notes_output = sandbox
            .run_ok_with_env(
                &args(&["--format", "json", "release-notes", "--latest"]),
                &common_env,
            )
            .await?;
        let latest_release_notes_json = stdout_json(&latest_release_notes_output)?;
        ensure(
            latest_release_notes_json["url"]
                .as_str()
                .is_some_and(|value| value.ends_with("/releases/latest")),
            "release-notes --latest should point to the latest release page",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}

#[cfg(unix)]
fn write_executable(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

fn prepend_path(dir: &Path) -> Result<String> {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    Ok(std::env::join_paths(paths)?.to_string_lossy().into_owned())
}

fn spawn_json_server(routes: HashMap<String, String>) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;

    std::thread::spawn(move || {
        for stream in listener.incoming().take(routes.len()) {
            let Ok(mut stream) = stream else {
                continue;
            };
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer);
            let request = String::from_utf8_lossy(&buffer);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");
            let body = routes
                .get(path)
                .cloned()
                .unwrap_or_else(|| "{}".to_string());
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    Ok(format!("http://{addr}"))
}

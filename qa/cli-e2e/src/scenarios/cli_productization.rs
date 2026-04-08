use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;

use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "cli_productization";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "completions, install/update guide, and doctor expose actionable productized CLI flows",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        let completions_output = sandbox.run_ok(&args(&["completions", "bash"])).await?;
        let completions_stdout = completions_output.stdout.as_str();
        ensure(
            completions_stdout.contains("cc-switch")
                && completions_stdout.contains("provider"),
            "bash completions should contain the command name and subcommands",
        )?;

        let install_dir = sandbox.work_path("completions");
        let install_dir_value = install_dir.display().to_string();
        let install_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "install",
                "completions",
                "fish",
                "--dir",
                install_dir_value.as_str(),
            ]))
            .await?;
        let install_json = stdout_json(&install_output)?;
        let installed_path = install_dir.join("cc-switch.fish");
        ensure(
            install_json["path"] == installed_path.display().to_string(),
            "install completions should report the written target path",
        )?;
        ensure(installed_path.is_file(), "installed completion file should exist")?;

        let guide_output = sandbox
            .run_ok(&args(&["--format", "json", "install", "guide"]))
            .await?;
        let guide_json = stdout_json(&guide_output)?;
        ensure(
            guide_json["recommendedMethod"] == "cargo-git",
            "install guide should recommend the cargo-git path",
        )?;

        let github_server = spawn_json_server(HashMap::from([(
            "/repos/farion1231/cc-switch/releases/latest".to_string(),
            r#"{"tag_name":"v99.1.0"}"#.to_string(),
        )]))?;
        let update_output = sandbox
            .run_ok_with_env(
                &args(&["--format", "json", "update", "guide"]),
                &[(
                    "CC_SWITCH_TEST_GITHUB_API_BASE_URL",
                    github_server.as_str(),
                )],
            )
            .await?;
        let update_json = stdout_json(&update_output)?;
        ensure(
            update_json["latestVersion"] == "99.1.0" && update_json["hasUpdate"] == true,
            "update guide should surface mocked release metadata",
        )?;

        fs::create_dir_all(sandbox.home_path(".claude"))?;
        let doctor_output = sandbox
            .run_ok(&args(&["--format", "json", "doctor", "--app", "claude"]))
            .await?;
        let doctor_json = stdout_json(&doctor_output)?;
        ensure(
            doctor_json["runtime"]["databasePath"]
                .as_str()
                .is_some_and(|value| value.ends_with(".cc-switch/cc-switch.db")),
            "doctor should surface the sandbox database path",
        )?;
        ensure(
            doctor_json["apps"][0]["app"] == "claude",
            "doctor should scope the report to the requested app",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
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

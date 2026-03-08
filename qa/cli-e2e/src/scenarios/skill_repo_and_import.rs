use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "skill_repo_and_import";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "skill unmanaged scan/import, repo management, and zip install work in isolation",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;
        write_unmanaged_skill(
            &sandbox.home_path(".codex/skills/unmanaged-skill"),
            "Unmanaged Skill",
            "from codex live dir",
        )?;

        let scan_output = sandbox
            .run_ok(&args(&["skill", "unmanaged", "scan", "--format", "json"]))
            .await?;
        let scan_json = stdout_json(&scan_output)?;
        ensure(
            scan_json.as_array().is_some_and(|items| items.iter().any(|item| {
                item["directory"] == "unmanaged-skill"
                    && item["foundIn"]
                        .as_array()
                        .is_some_and(|labels| labels.iter().any(|label| label == "codex"))
            })),
            "skill unmanaged scan did not report the staged codex skill",
        )?;

        sandbox
            .run_ok(&args(&[
                "skill",
                "unmanaged",
                "import",
                "unmanaged-skill",
                "--format",
                "json",
            ]))
            .await?;
        ensure(
            sandbox
                .home_path(".cc-switch/skills/unmanaged-skill/SKILL.md")
                .exists(),
            "skill unmanaged import did not copy into SSOT",
        )?;

        let repos_output = sandbox
            .run_ok(&args(&["skill", "repo", "list", "--format", "json"]))
            .await?;
        let repos_json = stdout_json(&repos_output)?;
        ensure(
            repos_json.as_array().is_some_and(|items| !items.is_empty()),
            "skill repo list should expose default repos",
        )?;

        sandbox
            .run_ok(&args(&[
                "skill",
                "repo",
                "add",
                "example/demo",
                "--branch",
                "develop",
                "--format",
                "json",
            ]))
            .await?;
        let repos_after_add = sandbox
            .run_ok(&args(&["skill", "repo", "list", "--format", "json"]))
            .await?;
        let repos_after_add_json = stdout_json(&repos_after_add)?;
        ensure(
            repos_after_add_json.as_array().is_some_and(|items| items.iter().any(|item| {
                item["owner"] == "example"
                    && item["name"] == "demo"
                    && item["branch"] == "develop"
            })),
            "skill repo add did not persist the custom repo",
        )?;

        sandbox
            .run_ok(&args(&["skill", "repo", "remove", "example/demo"]))
            .await?;
        let repos_after_remove = sandbox
            .run_ok(&args(&["skill", "repo", "list", "--format", "json"]))
            .await?;
        let repos_after_remove_json = stdout_json(&repos_after_remove)?;
        ensure(
            repos_after_remove_json
                .as_array()
                .is_some_and(|items| items.iter().all(|item| {
                    item["owner"] != "example" || item["name"] != "demo"
                })),
            "skill repo remove left the repo in the persisted list",
        )?;

        let zip_path = sandbox.work_path("zip-skill.zip");
        create_skill_zip(&zip_path, "zip-skill", "Zip Skill", "from zip archive")?;
        let zip_output = sandbox
            .run_ok(&vec![
                "skill".to_string(),
                "zip-install".to_string(),
                "--file".to_string(),
                zip_path.display().to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        let zip_json = stdout_json(&zip_output)?;
        ensure(
            zip_json.as_array().is_some_and(|items| items.len() == 1),
            "skill zip-install should install exactly one skill from the archive",
        )?;
        ensure(
            sandbox.home_path(".cc-switch/skills/zip-skill/SKILL.md").exists(),
            "skill zip-install did not copy the skill into SSOT",
        )?;
        ensure(
            sandbox.home_path(".claude/skills/zip-skill/SKILL.md").exists(),
            "skill zip-install did not sync the skill into Claude live dir",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}

fn write_unmanaged_skill(dir: &Path, name: &str, description: &str) -> Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )?;
    Ok(())
}

fn create_skill_zip(zip_path: &Path, root_dir: &str, name: &str, description: &str) -> Result<()> {
    let file = fs::File::create(zip_path)?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    writer.add_directory(root_dir, options)?;
    writer.start_file(format!("{root_dir}/SKILL.md"), options)?;
    writer.write_all(
        format!("---\nname: {name}\ndescription: {description}\n---\n").as_bytes(),
    )?;
    writer.start_file(format!("{root_dir}/notes.txt"), options)?;
    writer.write_all(b"zip body")?;
    writer.finish()?;
    Ok(())
}

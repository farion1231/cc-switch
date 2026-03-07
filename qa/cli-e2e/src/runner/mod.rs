use anyhow::{bail, Context, Result};
use std::future::Future;
use std::net::TcpListener;
use std::path::PathBuf;
use std::pin::Pin;

use crate::scenarios;

pub type ScenarioFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

#[derive(Clone)]
pub struct HarnessEnv {
    pub repo_root: PathBuf,
    pub fixtures_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub bin_path: PathBuf,
    pub keep_artifacts: bool,
    pub filter: Option<String>,
}

#[derive(Clone, Copy)]
pub struct Scenario {
    pub name: &'static str,
    pub description: &'static str,
    pub run: fn(HarnessEnv) -> ScenarioFuture,
}

impl HarnessEnv {
    pub fn detect() -> Result<Self> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir
            .parent()
            .and_then(|path| path.parent())
            .map(PathBuf::from)
            .context("failed to resolve repository root from qa/cli-e2e manifest dir")?;
        let fixtures_dir = manifest_dir.join("fixtures");
        let artifacts_dir = manifest_dir.join(".artifacts");

        let bin_path = std::env::var("CC_SWITCH_E2E_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("target").join("debug").join("cc-switch"));

        Ok(Self {
            repo_root,
            fixtures_dir,
            artifacts_dir,
            bin_path,
            keep_artifacts: matches!(
                std::env::var("CC_SWITCH_E2E_KEEP_ARTIFACTS").as_deref(),
                Ok("1" | "true" | "TRUE" | "yes" | "YES")
            ),
            filter: std::env::var("CC_SWITCH_E2E_FILTER")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        })
    }

    pub fn matches_filter(&self, scenario_name: &str) -> bool {
        match &self.filter {
            Some(filter) => scenario_name.contains(filter),
            None => true,
        }
    }
}

pub fn list_scenarios(env: &HarnessEnv) -> Result<()> {
    let mut any = false;
    for scenario in catalog()
        .into_iter()
        .filter(|item| env.matches_filter(item.name))
    {
        any = true;
        println!("{:<28} {}", scenario.name, scenario.description);
    }

    if !any {
        bail!("no scenarios matched the current filter");
    }

    Ok(())
}

pub fn doctor(env: &HarnessEnv) -> Result<()> {
    std::fs::create_dir_all(&env.artifacts_dir)
        .with_context(|| format!("failed to create {}", env.artifacts_dir.display()))?;

    if !env.bin_path.exists() {
        bail!(
            "cc-switch binary not found: {}. Run qa/cli-e2e/scripts/build-cli.sh first or set CC_SWITCH_E2E_BIN",
            env.bin_path.display()
        );
    }

    let probe_home = env.artifacts_dir.join("doctor-home");
    std::fs::create_dir_all(&probe_home)
        .with_context(|| format!("failed to create {}", probe_home.display()))?;
    std::fs::write(probe_home.join("write-probe.txt"), "ok")
        .with_context(|| format!("failed to write probe file in {}", probe_home.display()))?;
    std::fs::remove_file(probe_home.join("write-probe.txt")).ok();
    std::fs::remove_dir_all(&probe_home).ok();

    let listener = TcpListener::bind("127.0.0.1:0").context("failed to bind loopback port")?;
    let addr = listener
        .local_addr()
        .context("failed to inspect loopback port")?;
    drop(listener);

    println!("binary:    {}", env.bin_path.display());
    println!("fixtures:  {}", env.fixtures_dir.display());
    println!("artifacts: {}", env.artifacts_dir.display());
    println!("loopback:  {}", addr);
    println!("doctor:    ok");
    Ok(())
}

pub async fn run_named(env: HarnessEnv, scenario_name: &str) -> Result<()> {
    let scenario = catalog()
        .into_iter()
        .find(|item| item.name == scenario_name)
        .with_context(|| format!("unknown scenario: {scenario_name}"))?;

    println!("running {}", scenario.name);
    (scenario.run)(env).await
}

pub async fn run_all(env: HarnessEnv) -> Result<()> {
    let scenarios: Vec<_> = catalog()
        .into_iter()
        .filter(|item| env.matches_filter(item.name))
        .collect();

    if scenarios.is_empty() {
        bail!("no scenarios matched the current filter");
    }

    let mut failures = Vec::new();
    for scenario in scenarios {
        println!("running {}", scenario.name);
        if let Err(err) = (scenario.run)(env.clone()).await {
            failures.push((scenario.name, err.to_string()));
        }
    }

    if failures.is_empty() {
        println!("all scenarios passed");
        Ok(())
    } else {
        for (name, error) in &failures {
            eprintln!("FAILED {name}: {error}");
        }
        bail!("{} scenario(s) failed", failures.len());
    }
}

pub fn catalog() -> Vec<Scenario> {
    scenarios::all()
}

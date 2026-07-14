use std::env;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

use super::ZedRemoteError;

pub fn candidate_zed_app_paths() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("/Applications/Zed.app"),
        PathBuf::from("/Applications/Zed Preview.app"),
        PathBuf::from("/Applications/Zed Nightly.app"),
    ];
    if let Some(home) = home_dir() {
        paths.push(home.join("Applications/Zed.app"));
        paths.push(home.join("Applications/Zed Preview.app"));
        paths.push(home.join("Applications/Zed Nightly.app"));
    }
    paths
}

pub fn find_zed_app_path() -> Option<PathBuf> {
    candidate_zed_app_paths()
        .into_iter()
        .find(|path| path.exists())
}

pub fn find_zed_cli_path() -> String {
    find_executable_on_path("zed")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn zed_remote_status() -> Value {
    let app_path = find_zed_app_path();
    let cli_path = find_zed_cli_path();
    let platform_supported =
        cfg!(target_os = "macos") || cfg!(target_os = "windows") || cfg!(target_os = "linux");
    json!({
        "status": if platform_supported { "ok" } else { "failed" },
        "platformSupported": platform_supported,
        "zedAppFound": app_path.is_some(),
        "zedCliFound": !cli_path.is_empty(),
        "zedAppPath": app_path.map(|path| path.to_string_lossy().into_owned()).unwrap_or_default(),
        "zedCliPath": cli_path,
    })
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn launch_zed_url(url: &str) -> Result<(), ZedRemoteError> {
    let app_path = find_zed_app_path();
    let cli_path = find_zed_cli_path();
    if cfg!(target_os = "macos") {
        if let Some(app_path) = app_path {
            Command::new("open")
                .arg("-a")
                .arg(app_path)
                .arg(url)
                .spawn()
                .map_err(ZedRemoteError::Launch)?;
            return Ok(());
        }
    }
    if !cli_path.is_empty() {
        Command::new(cli_path)
            .arg(url)
            .spawn()
            .map_err(ZedRemoteError::Launch)?;
        return Ok(());
    }
    Err(ZedRemoteError::Validation(
        "Zed is not installed or not available on PATH",
    ))
}

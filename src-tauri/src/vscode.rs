use std::path::PathBuf;

/// 枚举可能的 VS Code 发行版配置目录名称
fn vscode_product_dirs() -> Vec<&'static str> {
    vec![
        "Code",            // VS Code Stable
        "Code - Insiders", // VS Code Insiders
        "VSCodium",        // VSCodium
        "Code - OSS",      // OSS 发行版
    ]
}

/// 获取 VS Code 用户 settings.json 的候选路径列表（按优先级排序）
pub fn candidate_settings_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            for prod in vscode_product_dirs() {
                paths.push(
                    home.join("Library")
                        .join("Application Support")
                        .join(prod)
                        .join("User")
                        .join("settings.json"),
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: %APPDATA%\Code\User\settings.json
        if let Some(roaming) = dirs::config_dir() {
            for prod in vscode_product_dirs() {
                paths.push(roaming.join(prod).join("User").join("settings.json"));
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Linux: ~/.config/Code/User/settings.json
        if let Some(config) = dirs::config_dir() {
            for prod in vscode_product_dirs() {
                paths.push(config.join(prod).join("User").join("settings.json"));
            }
        }
    }

    paths
}

/// 返回第一个存在的 settings.json 路径
pub fn find_existing_settings() -> Option<PathBuf> {
    for p in candidate_settings_paths() {
        if let Ok(meta) = std::fs::metadata(&p) {
            if meta.is_file() {
                return Some(p);
            }
        }
    }
    None
}

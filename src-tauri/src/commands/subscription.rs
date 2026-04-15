use crate::services::subscription::{CredentialScanStatus, SubscriptionQuota};
use std::path::PathBuf;
use std::process::Command;

/// Windows 上 gemini 可能的安装位置
#[cfg(target_os = "windows")]
fn find_gemini_exe() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    // 常见安装位置
    let candidates = vec![
        // npm 全局安装
        home.join("AppData/Roaming/npm/gemini.cmd"),
        home.join("AppData/Roaming/npm/gemini"),
        // bun 全局安装
        home.join(".bun/bin/gemini"),
        home.join(".bun/bin/gemini.cmd"),
        // pnpm 全局安装
        home.join("AppData/Local/pnpm/gemini.cmd"),
        // 直接安装在 PATH 中
        PathBuf::from("gemini.cmd"),
        PathBuf::from("gemini.exe"),
        PathBuf::from("gemini"),
    ];

    for path in candidates {
        if path.exists() {
            return Some(path);
        }
    }

    // 尝试在 PATH 中查找
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_env) {
            let gemini_cmd = dir.join("gemini.cmd");
            let gemini_exe = dir.join("gemini.exe");
            let gemini = dir.join("gemini");

            for candidate in &[gemini_cmd, gemini_exe, gemini] {
                if candidate.exists() {
                    return Some(candidate.clone());
                }
            }
        }
    }

    None
}

/// 检测是否已安装 gemini，如果已安装返回可执行文件路径
#[cfg(target_os = "windows")]
fn detect_gemini() -> Result<PathBuf, String> {
    find_gemini_exe().ok_or_else(|| "未检测到 Gemini CLI。".to_string())
}

/// 使用 npm 安装 gemini
#[cfg(target_os = "windows")]
fn install_gemini_npm() -> Result<PathBuf, String> {
    log::info!("正在通过 npm 安装 Gemini CLI...");

    let output = Command::new("cmd")
        .args(["/C", "npm", "install", "-g", "@google/gemini-cli"])
        .output()
        .map_err(|e| format!("npm 安装失败：{}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("npm 安装失败：{}", stderr));
    }

    log::info!("Gemini CLI 安装成功");

    // 安装后查找可执行文件
    find_gemini_exe()
        .ok_or_else(|| "安装完成但未找到 gemini 可执行文件，可能需要重启终端".to_string())
}

/// 使用 bun 安装 gemini
#[cfg(target_os = "windows")]
fn install_gemini_bun() -> Result<PathBuf, String> {
    log::info!("正在通过 bun 安装 Gemini CLI...");

    let output = Command::new("cmd")
        .args(["/C", "bun", "add", "-g", "@google/gemini-cli"])
        .output()
        .map_err(|e| format!("bun 安装失败：{}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("bun 安装失败：{}", stderr));
    }

    log::info!("Gemini CLI 安装成功");

    let home = dirs::home_dir().ok_or("无法获取 home 目录")?;
    let gemini_path = home.join(".bun/bin/gemini.cmd");

    if gemini_path.exists() {
        Ok(gemini_path)
    } else {
        Err("安装完成但未找到 gemini 可执行文件".to_string())
    }
}

#[tauri::command]
pub async fn launch_gemini_oauth_login() -> Result<bool, String> {
    // 1. 设置 Gemini CLI 使用 OAuth 模式
    crate::gemini_config::write_google_oauth_settings().map_err(|e| e.to_string())?;

    // 2. 跨平台启动 gemini CLI
    #[cfg(target_os = "windows")]
    {
        // 先尝试查找已安装的 gemini
        match detect_gemini() {
            Ok(gemini_path) => {
                // 已安装，直接启动
                Command::new("cmd")
                    .args(["/C", "start"])
                    .arg("Gemini CLI")
                    .arg(&gemini_path)
                    .spawn()
                    .map_err(|e| format!("启动 Gemini CLI 失败：{}", e))?;
                return Ok(true);
            }
            Err(_) => {
                // 未安装，返回错误让前端显示安装提示
                return Err("Gemini CLI 未安装。请点击安装按钮进行安装。".to_string());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: 使用 osascript 打开新终端窗口运行 gemini
        let script = r#"tell application "Terminal"
    activate
    do script "gemini"
end tell"#;

        Command::new("osascript")
            .arg("-e")
            .arg(script)
            .spawn()
            .map_err(|e| format!("启动 Terminal 失败：{}", e))?;

        Ok(true)
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: 尝试常见的终端模拟器
        let terminals = [
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "kitty",
            "alacritty",
            "ghostty",
        ];

        for terminal in &terminals {
            match Command::new(terminal).args(["-e", "gemini"]).spawn() {
                Ok(_) => return Ok(true),
                Err(_) => continue,
            }
        }

        // 都失败了，尝试直接运行
        Command::new("gemini")
            .spawn()
            .map_err(|e| format!("启动 Gemini CLI 失败：{}。请确认已安装 Gemini CLI。", e))?;
        Ok(true)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err("不支持的操作系统".to_string())
    }
}

/// 安装 Gemini CLI
#[tauri::command]
pub async fn install_gemini_cli(use_bun: bool) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let result = if use_bun {
            install_gemini_bun()
        } else {
            install_gemini_npm()
        };

        match result {
            Ok(_) => Ok(true),
            Err(e) => Err(e),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("当前仅支持 Windows 自动安装".to_string())
    }
}

/// 查询官方订阅额度
///
/// 读取 CLI 工具已有的 OAuth 凭据并调用官方 API 获取使用额度。
/// 不需要 AppState（不访问数据库），直接读文件 + 发 HTTP。
#[tauri::command]
pub async fn get_subscription_quota(tool: String) -> Result<SubscriptionQuota, String> {
    crate::services::subscription::get_subscription_quota(&tool).await
}

#[tauri::command]
pub fn get_credential_scan_status(tool: String) -> Result<CredentialScanStatus, String> {
    crate::services::subscription::get_credential_scan_status(&tool)
}

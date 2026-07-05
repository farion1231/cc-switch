use serde::{Deserialize, Serialize};

/// WSL 发行版中单个工具的安装状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WslToolStatus {
    /// 工具名称: claude, codex, gemini, opencode, openclaw, hermes
    pub name: String,
    /// 工具是否在 WSL 中安装
    pub installed: bool,
    /// 工具的配置目录是否存在于 WSL 中
    pub config_exists: bool,
    /// WSL 中的配置目录路径（UNC 格式），如 \\wsl$\Ubuntu\home\user\.claude
    pub config_path: Option<String>,
    /// 当前是否已配置目录覆盖指向 WSL
    pub is_currently_overridden: bool,
}

/// 将 UTF-16LE 字节转换为 String
#[cfg(target_os = "windows")]
fn decode_utf16le(bytes: &[u8]) -> String {
    let u16_vec: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16_lossy(&u16_vec)
}

/// 检测已安装的 WSL 发行版列表
///
/// 仅在 Windows 上可用，其他平台返回空数组。
#[tauri::command]
pub async fn detect_wsl_distros() -> Result<Vec<String>, String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Ok(vec![]);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        // CREATE_NO_WINDOW 避免弹出控制台窗口
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let output = std::process::Command::new("wsl.exe")
            .args(["--list", "--quiet"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("运行 wsl.exe 失败: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // wsl.exe 未安装或无发行版时返回非零退出码
            if stderr.contains("没有安装") || stderr.contains("not installed") {
                return Ok(vec![]);
            }
            return Err(format!("wsl.exe 返回错误: {}", stderr));
        }

        // wsl.exe --list --quiet 输出 UTF-16LE 编码
        let decoded = decode_utf16le(&output.stdout);
        let distros: Vec<String> = decoded
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        Ok(distros)
    }
}

/// 检测指定 WSL 发行版中已安装的工具
///
/// 通过 `which` 命令检测工具是否安装，通过检查配置目录是否存在来判断配置状态。
#[tauri::command]
pub async fn detect_wsl_tools(distro: String) -> Result<Vec<WslToolStatus>, String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Ok(vec![]);
    }

    #[cfg(target_os = "windows")]
    {
        if distro.is_empty() {
            return Err("发行版名称不能为空".to_string());
        }

        // 获取 WSL 用户名
        let user = get_wsl_user(&distro).await?;

        let tools = [
            ("claude", ".claude"),
            ("codex", ".codex"),
            ("gemini", ".gemini"),
            ("opencode", ".config/opencode"),
            ("openclaw", ".openclaw"),
            ("hermes", ".hermes"),
        ];

        let mut results = Vec::new();

        for (tool_name, config_dir) in &tools {
            // 检测工具是否安装
            let installed = check_wsl_tool_installed(&distro, tool_name).await;

            // 检测配置目录是否存在
            let config_path = format!(
                "\\\\wsl$\\{}\\home\\{}\\{}",
                distro, user, config_dir
            );
            let config_exists = check_wsl_path_exists(&distro, &format!(
                "/home/{}/{}",
                user, config_dir
            ))
            .await;

            // 检查当前是否已配置目录覆盖
            let is_currently_overridden =
                is_directory_overridden_to_wsl(tool_name, &distro);

            results.push(WslToolStatus {
                name: tool_name.to_string(),
                installed,
                config_exists,
                config_path: if config_exists {
                    Some(config_path)
                } else {
                    None
                },
                is_currently_overridden,
            });
        }

        Ok(results)
    }
}

/// 为指定的 WSL 工具一键配置目录覆盖
///
/// 根据发行版名称和 WSL 用户名构建 UNC 路径，更新设置中的目录覆盖。
#[tauri::command]
pub async fn apply_wsl_directory_overrides(
    distro: String,
    tools: Vec<String>,
) -> Result<Vec<String>, String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Err("WSL 集成仅在 Windows 上可用".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        if distro.is_empty() {
            return Err("发行版名称不能为空".to_string());
        }
        if tools.is_empty() {
            return Ok(vec![]);
        }

        let user = get_wsl_user(&distro).await?;

        let tool_dir_map: std::collections::HashMap<&str, &str> = [
            ("claude", ".claude"),
            ("codex", ".codex"),
            ("gemini", ".gemini"),
            ("opencode", ".config/opencode"),
            ("openclaw", ".openclaw"),
            ("hermes", ".hermes"),
        ]
        .iter()
        .cloned()
        .collect();

        let mut configured = Vec::new();

        for tool in &tools {
            let config_dir = match tool_dir_map.get(tool.as_str()) {
                Some(dir) => dir,
                None => continue,
            };

            let unc_path = format!(
                "\\\\wsl$\\{}\\home\\{}\\{}",
                distro, user, config_dir
            );

            // 保存目录覆盖到设置
            set_directory_override(tool, &unc_path)?;
            configured.push(tool.clone());
        }

        Ok(configured)
    }
}

/// 重置指定工具的目录覆盖为默认值
#[tauri::command]
pub async fn reset_wsl_directory_overrides(
    tools: Vec<String>,
) -> Result<Vec<String>, String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Err("WSL 集成仅在 Windows 上可用".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let mut reset = Vec::new();

        for tool in &tools {
            set_directory_override(tool, "")?;
            reset.push(tool.clone());
        }

        Ok(reset)
    }
}

// ===== 辅助函数 =====

/// 获取 WSL 发行版中的当前用户名
#[cfg(target_os = "windows")]
async fn get_wsl_user(distro: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let output = std::process::Command::new("wsl.exe")
        .args(["-d", distro, "--", "whoami"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("获取 WSL 用户名失败: {e}"))?;

    if !output.status.success() {
        return Err("无法获取 WSL 用户名".to_string());
    }

    // whoami 输出可能是 UTF-16LE 或 UTF-8，尝试两种
    let user = if output.stdout.len() >= 2 && output.stdout[0] == 0 && output.stdout[1] != 0 {
        // 看起来像 UTF-16LE（第一个字节是 0）
        decode_utf16le(&output.stdout)
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    let user = user.trim().to_string();

    if user.is_empty() {
        return Err("WSL 用户名为空".to_string());
    }

    // whoami 可能返回 DOMAIN\user 格式，取最后一个反斜杠后面的部分
    let user = user.rsplit('\\').next().unwrap_or(&user).to_string();

    Ok(user)
}

/// 检测 WSL 中是否安装了指定工具
#[cfg(target_os = "windows")]
async fn check_wsl_tool_installed(distro: &str, tool: &str) -> bool {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let check_cmd = match tool {
        "claude" => "which claude 2>/dev/null",
        "codex" => "which codex 2>/dev/null",
        "gemini" => "which gemini 2>/dev/null",
        "opencode" => "which opencode 2>/dev/null",
        "openclaw" => "which openclaw 2>/dev/null",
        "hermes" => "which hermes 2>/dev/null",
        _ => return false,
    };

    let output = std::process::Command::new("wsl.exe")
        .args(["-d", distro, "--", "sh", "-c", check_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    match output {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// 检测 WSL 中指定路径是否存在
#[cfg(target_os = "windows")]
async fn check_wsl_path_exists(distro: &str, path: &str) -> bool {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let check_cmd = format!("test -d {} && echo exists", path);

    let output = std::process::Command::new("wsl.exe")
        .args(["-d", distro, "--", "sh", "-c", &check_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    match output {
        Ok(out) => {
            out.status.success()
                && String::from_utf8_lossy(&out.stdout).contains("exists")
        }
        Err(_) => false,
    }
}

/// 检查指定工具的目录覆盖是否已指向 WSL 路径
#[cfg(target_os = "windows")]
fn is_directory_overridden_to_wsl(tool: &str, distro: &str) -> bool {
    let override_dir = match tool {
        "claude" => crate::settings::get_claude_override_dir(),
        "codex" => crate::settings::get_codex_override_dir(),
        "gemini" => crate::settings::get_gemini_override_dir(),
        "opencode" => crate::settings::get_opencode_override_dir(),
        "openclaw" => crate::settings::get_openclaw_override_dir(),
        "hermes" => crate::settings::get_hermes_override_dir(),
        _ => return false,
    };

    match override_dir {
        Some(path) => {
            let path_str = path.to_string_lossy().to_lowercase();
            let distro_lower = distro.to_lowercase();
            (path_str.contains("\\wsl$\\") || path_str.contains("\\wsl.localhost\\"))
                && path_str.contains(&distro_lower)
        }
        None => false,
    }
}

/// 设置指定工具的目录覆盖
///
/// 传入空字符串表示清除覆盖（恢复默认）。
#[cfg(target_os = "windows")]
fn set_directory_override(tool: &str, path: &str) -> Result<(), String> {
    use crate::settings::mutate_settings;

    let value = if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    };

    mutate_settings(|settings| match tool {
        "claude" => settings.claude_config_dir = value.clone(),
        "codex" => settings.codex_config_dir = value.clone(),
        "gemini" => settings.gemini_config_dir = value.clone(),
        "opencode" => settings.opencode_config_dir = value.clone(),
        "openclaw" => settings.openclaw_config_dir = value.clone(),
        "hermes" => settings.hermes_config_dir = value.clone(),
        _ => {}
    })
    .map_err(|e| format!("保存设置失败: {e}"))
}

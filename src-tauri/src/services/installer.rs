/// 工具安装服务
///
/// 提供自动化安装 Claude Code、Codex、Gemini CLI、OpenCode 及其依赖（Node.js）的能力。
/// 所有安装操作都通过 shell 命令执行，支持 macOS、Linux 平台。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::process::Command;

/// 安装结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    /// 是否安装成功
    pub success: bool,
    /// 安装结果消息
    pub message: String,
    /// 安装后的版本号（如果成功）
    pub installed_version: Option<String>,
}

/// 检查 Node.js 是否已安装
///
/// # Returns
/// - `Ok(true)` - Node.js 已安装
/// - `Ok(false)` - Node.js 未安装
/// - `Err(String)` - 检测失败
pub fn check_nodejs_installed() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("cmd")
            .args(["/C", "node --version"])
            .output()
            .map_err(|e| format!("执行 node 命令失败: {}", e))?;

        Ok(output.status.success())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg("node --version")
            .output()
            .map_err(|e| format!("执行 node 命令失败: {}", e))?;

        Ok(output.status.success())
    }
}

/// 检查 Node.js 版本是否满足要求（>= 18.0.0）
///
/// # Returns
/// - `Ok(true)` - 版本满足要求
/// - `Ok(false)` - 版本不满足要求或未安装
/// - `Err(String)` - 检测失败
pub fn check_nodejs_version_sufficient() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("cmd")
            .args(["/C", "node --version"])
            .output()
            .map_err(|e| format!("执行 node 命令失败: {}", e))?;

        if !output.status.success() {
            return Ok(false);
        }

        let version_str = String::from_utf8_lossy(&output.stdout);
        parse_and_check_node_version(&version_str)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg("node --version")
            .output()
            .map_err(|e| format!("执行 node 命令失败: {}", e))?;

        if !output.status.success() {
            return Ok(false);
        }

        let version_str = String::from_utf8_lossy(&output.stdout);
        parse_and_check_node_version(&version_str)
    }
}

/// 解析并检查 Node.js 版本是否 >= 18.0.0
fn parse_and_check_node_version(version_str: &str) -> Result<bool, String> {
    // 版本格式: v18.20.0 或 18.20.0
    let version = version_str.trim().trim_start_matches('v');
    let parts: Vec<&str> = version.split('.').collect();

    if parts.is_empty() {
        return Err(format!("无法解析 Node.js 版本: {}", version_str));
    }

    let major = parts[0]
        .parse::<u32>()
        .map_err(|_| format!("无法解析主版本号: {}", parts[0]))?;

    Ok(major >= 18)
}

/// 检查 Homebrew 是否已安装（仅 macOS）
#[cfg(target_os = "macos")]
fn check_homebrew_installed() -> Result<bool, String> {
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg("which brew")
        .output()
        .map_err(|e| format!("检测 Homebrew 失败: {}", e))?;

    Ok(output.status.success())
}

/// 通过 Homebrew 安装 Node.js（仅 macOS）
///
/// # Returns
/// - `Ok(InstallResult)` - 安装结果
/// - `Err(String)` - 安装失败
#[cfg(target_os = "macos")]
pub fn install_nodejs() -> Result<InstallResult, String> {
    // 检查 Homebrew 是否可用
    if !check_homebrew_installed()? {
        return Ok(InstallResult {
            success: false,
            message: "Homebrew 未安装，请先安装 Homebrew: https://brew.sh".to_string(),
            installed_version: None,
        });
    }

    // 执行安装命令
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg("brew install node")
        .output()
        .map_err(|e| format!("执行 brew install node 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(InstallResult {
            success: false,
            message: format!("安装 Node.js 失败: {}", stderr),
            installed_version: None,
        });
    }

    // 验证安装结果
    let version_output = Command::new("/bin/bash")
        .arg("-c")
        .arg("node --version")
        .output()
        .map_err(|e| format!("验证 Node.js 安装失败: {}", e))?;

    let version = if version_output.status.success() {
        Some(String::from_utf8_lossy(&version_output.stdout).trim().to_string())
    } else {
        None
    };

    Ok(InstallResult {
        success: true,
        message: "Node.js 安装成功".to_string(),
        installed_version: version,
    })
}

/// Linux 平台安装 Node.js（提示用户手动安装）
#[cfg(target_os = "linux")]
pub fn install_nodejs() -> Result<InstallResult, String> {
    Ok(InstallResult {
        success: false,
        message: "Linux 平台请使用包管理器手动安装 Node.js:\n\
                  - Ubuntu/Debian: sudo apt install nodejs npm\n\
                  - Fedora: sudo dnf install nodejs\n\
                  - Arch: sudo pacman -S nodejs npm"
            .to_string(),
        installed_version: None,
    })
}

/// Windows 平台安装 Node.js（提示用户手动安装）
#[cfg(target_os = "windows")]
pub fn install_nodejs() -> Result<InstallResult, String> {
    Ok(InstallResult {
        success: false,
        message: "Windows 平台请从官网下载安装 Node.js: https://nodejs.org".to_string(),
        installed_version: None,
    })
}

/// 安装 Claude Code
///
/// 执行官方安装脚本: `curl -fsSL https://claude.ai/install.sh | bash`
///
/// # Returns
/// - `Ok(InstallResult)` - 安装结果
/// - `Err(String)` - 安装失败
#[cfg(not(target_os = "windows"))]
pub fn install_claude_code() -> Result<InstallResult, String> {
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg("curl -fsSL https://claude.ai/install.sh | bash")
        .output()
        .map_err(|e| format!("执行 Claude Code 安装脚本失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(InstallResult {
            success: false,
            message: format!("安装 Claude Code 失败: {}", stderr),
            installed_version: None,
        });
    }

    // 验证安装结果
    let version_output = Command::new("/bin/bash")
        .arg("-c")
        .arg("claude --version")
        .output()
        .map_err(|e| format!("验证 Claude Code 安装失败: {}", e))?;

    let version = if version_output.status.success() {
        let raw = String::from_utf8_lossy(&version_output.stdout);
        Some(extract_version(&raw))
    } else {
        None
    };

    Ok(InstallResult {
        success: true,
        message: "Claude Code 安装成功".to_string(),
        installed_version: version,
    })
}

/// Windows 平台安装 Claude Code（提示用户手动安装）
#[cfg(target_os = "windows")]
pub fn install_claude_code() -> Result<InstallResult, String> {
    Ok(InstallResult {
        success: false,
        message: "Windows 平台请从官网下载安装 Claude Code: https://claude.ai/download"
            .to_string(),
        installed_version: None,
    })
}

/// 安装 Codex
///
/// 执行命令: `npm install -g @openai/codex@latest`
///
/// # Returns
/// - `Ok(InstallResult)` - 安装结果
/// - `Err(String)` - 安装失败
pub fn install_codex() -> Result<InstallResult, String> {
    // 检查 Node.js 是否已安装
    if !check_nodejs_installed()? {
        return Ok(InstallResult {
            success: false,
            message: "请先安装 Node.js".to_string(),
            installed_version: None,
        });
    }

    #[cfg(target_os = "windows")]
    {
        let output = Command::new("cmd")
            .args(["/C", "npm install -g @openai/codex@latest"])
            .output()
            .map_err(|e| format!("执行 npm install 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(InstallResult {
                success: false,
                message: format!("安装 Codex 失败: {}", stderr),
                installed_version: None,
            });
        }

        // 验证安装结果
        let version_output = Command::new("cmd")
            .args(["/C", "codex --version"])
            .output()
            .map_err(|e| format!("验证 Codex 安装失败: {}", e))?;

        let version = if version_output.status.success() {
            let raw = String::from_utf8_lossy(&version_output.stdout);
            Some(extract_version(&raw))
        } else {
            None
        };

        Ok(InstallResult {
            success: true,
            message: "Codex 安装成功".to_string(),
            installed_version: version,
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg("npm install -g @openai/codex@latest")
            .output()
            .map_err(|e| format!("执行 npm install 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(InstallResult {
                success: false,
                message: format!("安装 Codex 失败: {}", stderr),
                installed_version: None,
            });
        }

        // 验证安装结果
        let version_output = Command::new("/bin/bash")
            .arg("-c")
            .arg("codex --version")
            .output()
            .map_err(|e| format!("验证 Codex 安装失败: {}", e))?;

        let version = if version_output.status.success() {
            let raw = String::from_utf8_lossy(&version_output.stdout);
            Some(extract_version(&raw))
        } else {
            None
        };

        Ok(InstallResult {
            success: true,
            message: "Codex 安装成功".to_string(),
            installed_version: version,
        })
    }
}

/// 安装 Gemini CLI
///
/// 执行命令: `npm install -g @google/gemini-cli@latest`
///
/// # Returns
/// - `Ok(InstallResult)` - 安装结果
/// - `Err(String)` - 安装失败
pub fn install_gemini_cli() -> Result<InstallResult, String> {
    // 检查 Node.js 是否已安装
    if !check_nodejs_installed()? {
        return Ok(InstallResult {
            success: false,
            message: "请先安装 Node.js".to_string(),
            installed_version: None,
        });
    }

    #[cfg(target_os = "windows")]
    {
        let output = Command::new("cmd")
            .args(["/C", "npm install -g @google/gemini-cli@latest"])
            .output()
            .map_err(|e| format!("执行 npm install 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(InstallResult {
                success: false,
                message: format!("安装 Gemini CLI 失败: {}", stderr),
                installed_version: None,
            });
        }

        // 验证安装结果
        let version_output = Command::new("cmd")
            .args(["/C", "gemini --version"])
            .output()
            .map_err(|e| format!("验证 Gemini CLI 安装失败: {}", e))?;

        let version = if version_output.status.success() {
            let raw = String::from_utf8_lossy(&version_output.stdout);
            Some(extract_version(&raw))
        } else {
            None
        };

        Ok(InstallResult {
            success: true,
            message: "Gemini CLI 安装成功".to_string(),
            installed_version: version,
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg("npm install -g @google/gemini-cli@latest")
            .output()
            .map_err(|e| format!("执行 npm install 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(InstallResult {
                success: false,
                message: format!("安装 Gemini CLI 失败: {}", stderr),
                installed_version: None,
            });
        }

        // 验证安装结果
        let version_output = Command::new("/bin/bash")
            .arg("-c")
            .arg("gemini --version")
            .output()
            .map_err(|e| format!("验证 Gemini CLI 安装失败: {}", e))?;

        let version = if version_output.status.success() {
            let raw = String::from_utf8_lossy(&version_output.stdout);
            Some(extract_version(&raw))
        } else {
            None
        };

        Ok(InstallResult {
            success: true,
            message: "Gemini CLI 安装成功".to_string(),
            installed_version: version,
        })
    }
}

/// 安装 OpenCode
///
/// 执行官方安装脚本: `curl -fsSL https://opencode.ai/install | bash`
///
/// # Returns
/// - `Ok(InstallResult)` - 安装结果
/// - `Err(String)` - 安装失败
#[cfg(not(target_os = "windows"))]
pub fn install_opencode() -> Result<InstallResult, String> {
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg("curl -fsSL https://opencode.ai/install | bash")
        .output()
        .map_err(|e| format!("执行 OpenCode 安装脚本失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(InstallResult {
            success: false,
            message: format!("安装 OpenCode 失败: {}", stderr),
            installed_version: None,
        });
    }

    // 验证安装结果
    let version_output = Command::new("/bin/bash")
        .arg("-c")
        .arg("opencode --version")
        .output()
        .map_err(|e| format!("验证 OpenCode 安装失败: {}", e))?;

    let version = if version_output.status.success() {
        let raw = String::from_utf8_lossy(&version_output.stdout);
        Some(extract_version(&raw))
    } else {
        None
    };

    Ok(InstallResult {
        success: true,
        message: "OpenCode 安装成功".to_string(),
        installed_version: version,
    })
}

/// Windows 平台安装 OpenCode（提示用户手动安装）
#[cfg(target_os = "windows")]
pub fn install_opencode() -> Result<InstallResult, String> {
    Ok(InstallResult {
        success: false,
        message: "Windows 平台请从官网下载安装 OpenCode: https://opencode.ai".to_string(),
        installed_version: None,
    })
}

/// 从版本输出中提取纯版本号
///
/// 例如: "claude 1.0.20" -> "1.0.20"
fn extract_version(raw: &str) -> String {
    let re = Regex::new(r"\d+\.\d+\.\d+(-[\w.]+)?").expect("Invalid version regex");
    re.find(raw)
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| raw.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_check_node_version() {
        assert!(parse_and_check_node_version("v18.20.0").unwrap());
        assert!(parse_and_check_node_version("18.20.0").unwrap());
        assert!(parse_and_check_node_version("v20.0.0").unwrap());
        assert!(!parse_and_check_node_version("v16.0.0").unwrap());
        assert!(!parse_and_check_node_version("v14.21.3").unwrap());
    }

    #[test]
    fn test_extract_version() {
        assert_eq!(extract_version("claude 1.0.20"), "1.0.20");
        assert_eq!(extract_version("v2.3.4-beta.1"), "2.3.4-beta.1");
        assert_eq!(extract_version("18.20.0"), "18.20.0");
    }
}

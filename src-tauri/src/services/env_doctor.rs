use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

/// 诊断结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisResult {
    /// 整体健康状态
    pub overall_status: HealthStatus,
    /// 诊断发现的问题列表
    pub issues: Vec<DiagnosisIssue>,
    /// 各工具的状态
    pub tools_status: HashMap<String, ToolStatus>,
}

/// 健康状态枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum HealthStatus {
    /// 一切正常
    Healthy,
    /// 需要安装
    NeedsInstall,
    /// 需要修复
    NeedsRepair,
    /// 部分工具有问题
    PartiallyHealthy,
}

/// 诊断问题
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisIssue {
    /// 问题唯一标识
    pub id: String,
    /// 严重程度
    pub severity: IssueSeverity,
    /// 问题类别
    pub category: IssueCategory,
    /// 问题标题
    pub title: String,
    /// 问题描述
    pub description: String,
    /// 是否可自动修复
    pub auto_fixable: bool,
    /// 修复动作（如果可修复）
    pub fix_action: Option<FixAction>,
}

/// 问题严重程度
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "PascalCase")]
pub enum IssueSeverity {
    /// 阻塞使用
    Critical,
    /// 严重影响
    High,
    /// 中等影响
    Medium,
    /// 轻微影响
    Low,
}

/// 问题类别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum IssueCategory {
    /// 工具未安装
    NotInstalled,
    /// 环境变量冲突
    EnvConflict,
    /// 配置文件损坏
    ConfigCorrupted,
    /// 权限不足
    PermissionDenied,
    /// 版本过期
    VersionOutdated,
    /// Node.js 缺失或版本过低
    NodeJsMissing,
}

/// 修复动作
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum FixAction {
    /// 安装工具
    InstallTool { tool: String },
    /// 安装 Node.js
    InstallNodeJs,
    /// 移除环境变量
    RemoveEnvVar { var_name: String, source: String },
    /// 修复配置文件
    RepairConfig { path: String },
    /// 修复权限
    FixPermission { path: String },
    /// 更新工具
    UpdateTool {
        tool: String,
        current: String,
        latest: String,
    },
}

/// 工具状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    /// 是否已安装
    pub installed: bool,
    /// 当前版本
    pub version: Option<String>,
    /// 最新版本
    pub latest_version: Option<String>,
    /// 问题列表
    pub issues: Vec<String>,
}

/// 执行环境诊断
///
/// 检测项包括：
/// - 工具安装状态（Claude Code）
/// - Node.js 版本（需要 >= 18.0.0）
/// - 环境变量冲突
/// - 配置文件完整性（~/.claude/settings.json）
pub async fn diagnose_environment() -> Result<DiagnosisResult, String> {
    let mut issues = Vec::new();
    let mut tools_status = HashMap::new();

    // 1. 检测工具安装状态
    diagnose_tools(&mut issues, &mut tools_status).await;

    // 2. 检测 Node.js 环境
    diagnose_nodejs(&mut issues).await;

    // 3. 检测环境变量冲突
    diagnose_env_conflicts(&mut issues).await;

    // 4. 检测配置文件完整性
    diagnose_config_file(&mut issues).await;

    // 5. 根据问题列表确定整体健康状态
    let overall_status = determine_overall_status(&issues, &tools_status);

    Ok(DiagnosisResult {
        overall_status,
        issues,
        tools_status,
    })
}

/// 诊断工具安装状态
async fn diagnose_tools(
    issues: &mut Vec<DiagnosisIssue>,
    tools_status: &mut HashMap<String, ToolStatus>,
) {
    // 只检测 Claude Code
    let tool = "claude";

    // 使用内部实现检测版本
    let (version, error) = check_tool_version(tool);

    let installed = version.is_some();
    let latest_version = None; // 暂不获取最新版本，避免网络请求延迟

    let mut tool_issues = Vec::new();

    if !installed {
        // 工具未安装
        let issue_id = format!("{}_not_installed", tool);
        issues.push(DiagnosisIssue {
            id: issue_id.clone(),
            severity: IssueSeverity::Critical,
            category: IssueCategory::NotInstalled,
            title: format!("{} 未安装", tool_display_name(tool)),
            description: format!(
                "{} 未安装或未在 PATH 中找到。",
                tool_display_name(tool)
            ),
            auto_fixable: true,
            fix_action: Some(FixAction::InstallTool {
                tool: tool.to_string(),
            }),
        });
        tool_issues.push(issue_id);
    } else if let Some(err) = error {
        // 工具已安装但有错误
        tool_issues.push(format!("检测错误: {}", err));
    }

    tools_status.insert(
        tool.to_string(),
        ToolStatus {
            installed,
            version,
            latest_version,
            issues: tool_issues,
        },
    );
}

/// 诊断 Node.js 环境
async fn diagnose_nodejs(issues: &mut Vec<DiagnosisIssue>) {
    let (version, error) = check_nodejs_version();

    if let Some(err) = error {
        // Node.js 未安装或版本过低
        issues.push(DiagnosisIssue {
            id: "nodejs_missing".to_string(),
            severity: IssueSeverity::Critical,
            category: IssueCategory::NodeJsMissing,
            title: "Node.js 环境问题".to_string(),
            description: err,
            auto_fixable: true,
            fix_action: Some(FixAction::InstallNodeJs),
        });
    } else if let Some(ver) = version {
        // 检查版本是否 >= 18.0.0
        if !is_nodejs_version_sufficient(&ver) {
            issues.push(DiagnosisIssue {
                id: "nodejs_version_low".to_string(),
                severity: IssueSeverity::Critical,
                category: IssueCategory::NodeJsMissing,
                title: "Node.js 版本过低".to_string(),
                description: format!(
                    "当前 Node.js 版本为 {}，需要 >= 18.0.0",
                    ver
                ),
                auto_fixable: true,
                fix_action: Some(FixAction::InstallNodeJs),
            });
        }
    }
}

/// 诊断环境变量冲突
async fn diagnose_env_conflicts(issues: &mut Vec<DiagnosisIssue>) {
    // 复用现有的环境变量冲突检测逻辑
    let conflicts = match super::env_checker::check_env_conflicts("claude") {
        Ok(conflicts) => conflicts,
        Err(_) => return, // 检测失败，跳过
    };

    for conflict in conflicts {
        let issue_id = format!("env_conflict_{}", conflict.var_name);
        issues.push(DiagnosisIssue {
            id: issue_id,
            severity: IssueSeverity::High,
            category: IssueCategory::EnvConflict,
            title: format!("环境变量冲突: {}", conflict.var_name),
            description: format!(
                "检测到环境变量 {} 可能与官方登录冲突。来源: {}",
                conflict.var_name, conflict.source_path
            ),
            auto_fixable: true,
            fix_action: Some(FixAction::RemoveEnvVar {
                var_name: conflict.var_name,
                source: conflict.source_path,
            }),
        });
    }
}

/// 诊断配置文件完整性
async fn diagnose_config_file(issues: &mut Vec<DiagnosisIssue>) {
    let config_path = get_claude_config_path();

    // 检查文件是否存在
    if !config_path.exists() {
        issues.push(DiagnosisIssue {
            id: "config_missing".to_string(),
            severity: IssueSeverity::High,
            category: IssueCategory::ConfigCorrupted,
            title: "配置文件缺失".to_string(),
            description: format!("配置文件 {} 不存在", config_path.display()),
            auto_fixable: true,
            fix_action: Some(FixAction::RepairConfig {
                path: config_path.to_string_lossy().to_string(),
            }),
        });
        return;
    }

    // 检查文件是否可读
    match std::fs::read_to_string(&config_path) {
        Ok(content) => {
            // 检查 JSON 格式是否正确
            if serde_json::from_str::<serde_json::Value>(&content).is_err() {
                issues.push(DiagnosisIssue {
                    id: "config_corrupted".to_string(),
                    severity: IssueSeverity::High,
                    category: IssueCategory::ConfigCorrupted,
                    title: "配置文件格式错误".to_string(),
                    description: format!("配置文件 {} 不是有效的 JSON 格式", config_path.display()),
                    auto_fixable: true,
                    fix_action: Some(FixAction::RepairConfig {
                        path: config_path.to_string_lossy().to_string(),
                    }),
                });
            }
        }
        Err(_) => {
            // 文件不可读，可能是权限问题
            issues.push(DiagnosisIssue {
                id: "config_permission".to_string(),
                severity: IssueSeverity::Medium,
                category: IssueCategory::PermissionDenied,
                title: "配置文件权限不足".to_string(),
                description: format!("无法读取配置文件 {}", config_path.display()),
                auto_fixable: true,
                fix_action: Some(FixAction::FixPermission {
                    path: config_path.to_string_lossy().to_string(),
                }),
            });
        }
    }
}

/// 根据问题列表确定整体健康状态
fn determine_overall_status(
    issues: &[DiagnosisIssue],
    tools_status: &HashMap<String, ToolStatus>,
) -> HealthStatus {
    if issues.is_empty() {
        return HealthStatus::Healthy;
    }

    // 检查是否有 Critical 级别的未安装问题
    let has_critical_not_installed = issues.iter().any(|issue| {
        issue.severity == IssueSeverity::Critical
            && issue.category == IssueCategory::NotInstalled
    });

    if has_critical_not_installed {
        return HealthStatus::NeedsInstall;
    }

    // 检查是否有 Critical 或 High 级别的问题
    let has_critical_or_high = issues
        .iter()
        .any(|issue| matches!(issue.severity, IssueSeverity::Critical | IssueSeverity::High));

    if has_critical_or_high {
        return HealthStatus::NeedsRepair;
    }

    // 检查是否有部分工具未安装
    let installed_count = tools_status.values().filter(|s| s.installed).count();
    let total_count = tools_status.len();

    if installed_count > 0 && installed_count < total_count {
        return HealthStatus::PartiallyHealthy;
    }

    // 其他情况视为部分健康
    HealthStatus::PartiallyHealthy
}

/// 检查 Node.js 版本
///
/// 返回 (版本号, 错误信息)
fn check_nodejs_version() -> (Option<String>, Option<String>) {
    use std::process::Command;

    let output = match Command::new("node").arg("--version").output() {
        Ok(output) => output,
        Err(_) => {
            return (
                None,
                Some("Node.js 未安装或未在 PATH 中找到".to_string()),
            );
        }
    };

    if !output.status.success() {
        return (None, Some("无法获取 Node.js 版本".to_string()));
    }

    let version_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // 移除 'v' 前缀
    let version = version_str.strip_prefix('v').unwrap_or(&version_str);

    (Some(version.to_string()), None)
}

/// 检查 Node.js 版本是否满足要求（>= 18.0.0）
fn is_nodejs_version_sufficient(version: &str) -> bool {
    // 解析版本号
    let parts: Vec<&str> = version.split('.').collect();
    if parts.is_empty() {
        return false;
    }

    // 获取主版本号
    if let Ok(major) = parts[0].parse::<u32>() {
        major >= 18
    } else {
        false
    }
}

/// 获取 Claude 配置文件路径
fn get_claude_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".claude").join("settings.json")
}

/// 获取工具的显示名称
fn tool_display_name(tool: &str) -> &str {
    match tool {
        "claude" => "Claude Code",
        _ => tool,
    }
}

/// 检查工具版本
///
/// 返回 (版本号, 错误信息)
fn check_tool_version(tool: &str) -> (Option<String>, Option<String>) {
    use std::process::Command;

    let output = {
        let shell = std::env::var("SHELL")
            .ok()
            .unwrap_or_else(|| "sh".to_string());
        Command::new(shell)
            .arg("-c")
            .arg(format!("{} --version", tool))
            .output()
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if out.status.success() {
                let raw = if stdout.is_empty() { &stderr } else { &stdout };
                if raw.is_empty() {
                    (None, Some("未安装或不可执行".to_string()))
                } else {
                    (Some(extract_version(raw)), None)
                }
            } else {
                let err = if stderr.is_empty() { stdout } else { stderr };
                (
                    None,
                    Some(if err.is_empty() {
                        "未安装或不可执行".to_string()
                    } else {
                        err
                    }),
                )
            }
        }
        Err(e) => (None, Some(e.to_string())),
    }
}

/// 从版本输出中提取纯版本号
fn extract_version(raw: &str) -> String {
    use regex::Regex;

    let version_re = Regex::new(r"\d+\.\d+\.\d+(-[\w.]+)?").expect("Invalid version regex");
    version_re
        .find(raw)
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| raw.to_string())
}

// ============================================================================
// 环境修复功能
// ============================================================================

/// 修复结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixResult {
    /// 成功修复的问题 ID 列表
    pub fixed: Vec<String>,
    /// 修复失败的问题列表 (问题 ID, 错误信息)
    pub failed: Vec<(String, String)>,
}

/// 批量修复环境问题
///
/// 只修复 `auto_fixable = true` 的问题。
/// 每个修复操作前会先备份相关数据。
///
/// # 参数
/// - `issues`: 待修复的问题列表
///
/// # 返回
/// - `Ok(FixResult)`: 修复结果，包含成功和失败的问题列表
/// - `Err(String)`: 修复过程中的致命错误
pub async fn fix_environment(issues: Vec<DiagnosisIssue>) -> Result<FixResult, String> {
    let mut fixed = Vec::new();
    let mut failed = Vec::new();

    for issue in issues {
        // 只修复可自动修复的问题
        if !issue.auto_fixable {
            continue;
        }

        let fix_action = match issue.fix_action {
            Some(action) => action,
            None => continue,
        };

        // 根据修复动作类型执行对应的修复操作
        let result = match fix_action {
            FixAction::RemoveEnvVar { var_name, source } => {
                fix_env_conflict(var_name, source).await
            }
            FixAction::RepairConfig { path } => repair_config_file(path).await,
            FixAction::FixPermission { path } => fix_permission(path).await,
            // 安装和更新操作不在此处理，需要用户明确触发
            FixAction::InstallTool { .. } | FixAction::InstallNodeJs | FixAction::UpdateTool { .. } => {
                continue;
            }
        };

        match result {
            Ok(_) => fixed.push(issue.id),
            Err(e) => failed.push((issue.id, e)),
        }
    }

    Ok(FixResult { fixed, failed })
}

/// 修复环境变量冲突
///
/// 通过复用 `env_manager::delete_env_vars` 功能删除冲突的环境变量。
/// 删除前会自动创建备份。
///
/// # 参数
/// - `var_name`: 环境变量名称
/// - `source`: 环境变量来源路径
///
/// # 平台支持
/// - macOS: 从 shell 配置文件中删除（.zshrc, .bashrc 等）
/// - Linux: 从 shell 配置文件中删除
/// - Windows: 暂不支持（需要管理员权限操作注册表）
async fn fix_env_conflict(var_name: String, source: String) -> Result<(), String> {
    // 构造 EnvConflict 对象
    let conflict = super::env_checker::EnvConflict {
        var_name: var_name.clone(),
        var_value: std::env::var(&var_name).unwrap_or_default(),
        source_type: if source.contains("HKEY") {
            "system".to_string()
        } else {
            "file".to_string()
        },
        source_path: source,
    };

    // 复用 env_manager 的删除功能（会自动备份）
    super::env_manager::delete_env_vars(vec![conflict])
        .map(|_| ())
        .map_err(|e| format!("删除环境变量失败: {}", e))
}

/// 修复配置文件
///
/// 修复策略：
/// 1. 检查是否存在 `.backup` 文件，如果有则从备份恢复
/// 2. 如果没有备份，生成默认配置文件
///
/// # 参数
/// - `path`: 配置文件路径
///
/// # 平台支持
/// - macOS: 完全支持
/// - Linux: 完全支持
/// - Windows: 完全支持
async fn repair_config_file(path: String) -> Result<(), String> {
    let config_path = PathBuf::from(&path);

    // 1. 尝试从备份恢复
    let backup_path = config_path.with_extension("json.backup");
    if backup_path.exists() {
        fs::copy(&backup_path, &config_path)
            .map_err(|e| format!("从备份恢复配置文件失败: {}", e))?;
        return Ok(());
    }

    // 2. 生成默认配置
    let default_config = generate_default_config();

    // 确保父目录存在
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建配置目录失败: {}", e))?;
    }

    // 写入默认配置
    fs::write(&config_path, default_config)
        .map_err(|e| format!("写入默认配置失败: {}", e))?;

    Ok(())
}

/// 修复权限问题
///
/// 修复策略：
/// - macOS/Linux: 使用 chmod 755 修复目录权限
/// - Windows: 暂不支持（需要管理员权限）
///
/// # 参数
/// - `path`: 需要修复权限的路径
///
/// # 平台支持
/// - macOS: 完全支持
/// - Linux: 完全支持
/// - Windows: 暂不支持
#[cfg(not(target_os = "windows"))]
async fn fix_permission(path: String) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let target_path = PathBuf::from(&path);

    // 确保路径存在
    if !target_path.exists() {
        return Err(format!("路径不存在: {}", path));
    }

    // 设置权限为 755 (rwxr-xr-x)
    let permissions = fs::Permissions::from_mode(0o755);
    fs::set_permissions(&target_path, permissions)
        .map_err(|e| format!("修复权限失败: {}", e))?;

    Ok(())
}

#[cfg(target_os = "windows")]
async fn fix_permission(_path: String) -> Result<(), String> {
    // Windows 权限修复需要管理员权限，暂不支持
    Err("Windows 平台暂不支持自动修复权限".to_string())
}

/// 生成默认配置文件内容
///
/// 生成一个最小化的 Claude Code settings.json 配置
fn generate_default_config() -> String {
    serde_json::json!({
        "model": "claude-opus-4-6",
        "language": "zh-CN",
        "env": {},
        "plugins": {
            "enabled": []
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_nodejs_version_sufficient() {
        assert!(is_nodejs_version_sufficient("18.0.0"));
        assert!(is_nodejs_version_sufficient("18.12.1"));
        assert!(is_nodejs_version_sufficient("20.0.0"));
        assert!(!is_nodejs_version_sufficient("16.0.0"));
        assert!(!is_nodejs_version_sufficient("17.9.1"));
    }

    #[test]
    fn test_determine_overall_status_healthy() {
        let issues = vec![];
        let tools_status = HashMap::new();
        assert_eq!(
            determine_overall_status(&issues, &tools_status),
            HealthStatus::Healthy
        );
    }

    #[test]
    fn test_determine_overall_status_needs_install() {
        let issues = vec![DiagnosisIssue {
            id: "test".to_string(),
            severity: IssueSeverity::Critical,
            category: IssueCategory::NotInstalled,
            title: "Test".to_string(),
            description: "Test".to_string(),
            auto_fixable: true,
            fix_action: None,
        }];
        let tools_status = HashMap::new();
        assert_eq!(
            determine_overall_status(&issues, &tools_status),
            HealthStatus::NeedsInstall
        );
    }

    #[test]
    fn test_determine_overall_status_needs_repair() {
        let issues = vec![DiagnosisIssue {
            id: "test".to_string(),
            severity: IssueSeverity::High,
            category: IssueCategory::EnvConflict,
            title: "Test".to_string(),
            description: "Test".to_string(),
            auto_fixable: true,
            fix_action: None,
        }];
        let tools_status = HashMap::new();
        assert_eq!(
            determine_overall_status(&issues, &tools_status),
            HealthStatus::NeedsRepair
        );
    }
}

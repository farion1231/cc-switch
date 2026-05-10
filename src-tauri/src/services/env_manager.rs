use super::env_checker::EnvConflict;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

/// 错误信息前缀，标记此次失败是因为缺少管理员权限。
///
/// 上层（`env_doctor::fix_environment`）会识别这个前缀，把它归一化成
/// `error_code: "requires_admin"`，让前端弹"请以管理员身份重启"的
/// 友好 toast。改名时务必同步 `env_doctor` 的归一化逻辑及其测试。
pub const REQUIRES_ADMIN_SENTINEL: &str = "[REQUIRES_ADMIN] ";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub backup_path: String,
    pub timestamp: String,
    pub conflicts: Vec<EnvConflict>,
}

/// Delete environment variables with automatic backup
pub fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, String> {
    // Step 1: Create backup
    let backup_info = create_backup(&conflicts)?;

    // Step 2: Delete variables
    for conflict in &conflicts {
        match delete_single_env(conflict) {
            Ok(_) => {}
            Err(e) => {
                // If deletion fails, we keep the backup but return error
                return Err(format!(
                    "删除环境变量失败: {}. 备份已保存到: {}",
                    e, backup_info.backup_path
                ));
            }
        }
    }

    // Step 3: Tell other processes to refresh their environment block.
    // 等价于 setx 的行为；新开终端无需 cc-doctor 重启即可看到变化。
    broadcast_environment_change();

    Ok(backup_info)
}

/// Create backup file before deletion
fn create_backup(conflicts: &[EnvConflict]) -> Result<BackupInfo, String> {
    // Get backup directory
    let backup_dir = get_backup_dir()?;
    fs::create_dir_all(&backup_dir).map_err(|e| format!("创建备份目录失败: {e}"))?;

    // Generate backup file name with timestamp
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_file = backup_dir.join(format!("env-backup-{timestamp}.json"));

    // Create backup data
    let backup_info = BackupInfo {
        backup_path: backup_file.to_string_lossy().to_string(),
        timestamp: timestamp.clone(),
        conflicts: conflicts.to_vec(),
    };

    // Write backup file
    let json = serde_json::to_string_pretty(&backup_info)
        .map_err(|e| format!("序列化备份数据失败: {e}"))?;

    fs::write(&backup_file, json).map_err(|e| format!("写入备份文件失败: {e}"))?;

    Ok(backup_info)
}

/// Get backup directory path
fn get_backup_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("无法获取用户主目录")?;
    Ok(home.join(".cc-doctor").join("backups"))
}

/// Delete a single environment variable
#[cfg(target_os = "windows")]
fn delete_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "system" => {
            if conflict.source_path.contains("HKEY_CURRENT_USER") {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER)
                    .open_subkey_with_flags("Environment", KEY_ALL_ACCESS)
                    .map_err(|e| format!("打开注册表失败: {}", e))?;

                hkcu.delete_value(&conflict.var_name)
                    .map_err(|e| format!("删除注册表项失败: {}", e))?;
            } else if conflict.source_path.contains("HKEY_LOCAL_MACHINE") {
                let hklm = RegKey::predef(HKEY_LOCAL_MACHINE)
                    .open_subkey_with_flags(
                        "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                        KEY_ALL_ACCESS,
                    )
                    .map_err(|e| {
                        format!(
                            "{}打开系统注册表失败 (需要以管理员身份重启 cc-doctor): {}",
                            REQUIRES_ADMIN_SENTINEL, e
                        )
                    })?;

                hklm.delete_value(&conflict.var_name).map_err(|e| {
                    format!(
                        "{}删除系统注册表项失败 (需要以管理员身份重启 cc-doctor): {}",
                        REQUIRES_ADMIN_SENTINEL, e
                    )
                })?;
            }
            Ok(())
        }
        "file" => Err("Windows 系统不应该有文件类型的环境变量".to_string()),
        _ => Err(format!("未知的环境变量来源类型: {}", conflict.source_type)),
    }
}

#[cfg(not(target_os = "windows"))]
fn delete_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "file" => {
            // Parse file path and line number from source_path (format: "path:line")
            let parts: Vec<&str> = conflict.source_path.split(':').collect();
            if parts.len() < 2 {
                return Err("无效的文件路径格式".to_string());
            }

            let file_path = parts[0];

            // Read file content
            let content = fs::read_to_string(file_path)
                .map_err(|e| format!("读取文件失败 {file_path}: {e}"))?;

            // Filter out the line containing the environment variable
            let new_content: Vec<String> = content
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    let export_line = trimmed.strip_prefix("export ").unwrap_or(trimmed);

                    // Check if this line sets the target variable
                    if let Some(eq_pos) = export_line.find('=') {
                        let var_name = export_line[..eq_pos].trim();
                        var_name != conflict.var_name
                    } else {
                        true
                    }
                })
                .map(|s| s.to_string())
                .collect();

            // Write back to file
            fs::write(file_path, new_content.join("\n"))
                .map_err(|e| format!("写入文件失败 {file_path}: {e}"))?;

            Ok(())
        }
        "system" => {
            // On Unix, we can't directly delete process environment variables
            Ok(())
        }
        _ => Err(format!("未知的环境变量来源类型: {}", conflict.source_type)),
    }
}

/// Restore environment variables from backup
pub fn restore_from_backup(backup_path: String) -> Result<(), String> {
    // Read backup file
    let content = fs::read_to_string(&backup_path).map_err(|e| format!("读取备份文件失败: {e}"))?;

    let backup_info: BackupInfo =
        serde_json::from_str(&content).map_err(|e| format!("解析备份文件失败: {e}"))?;

    // Restore each variable
    for conflict in &backup_info.conflicts {
        restore_single_env(conflict)?;
    }

    broadcast_environment_change();

    Ok(())
}

/// 广播 WM_SETTINGCHANGE，告诉其他进程环境变量已变。
///
/// Windows 上 setx 之类的工具默认会做这件事——其他进程（新开的 cmd /
/// PowerShell / Explorer）只有收到广播才会刷新环境块。winreg 的
/// `delete_value` / `set_value` 不会自动广播，所以这里手工补一刀。
///
/// 用 SMTO_ABORTIFHUNG + 5s 超时，避免被卡死的目标窗口拖住主进程。
/// 失败也不返回错误：广播本身是 best-effort 的优化，缺它也不影响数据
/// 正确性。
#[cfg(target_os = "windows")]
fn broadcast_environment_change() {
    use winapi::shared::basetsd::DWORD_PTR;
    use winapi::shared::minwindef::{LPARAM, WPARAM};
    use winapi::um::winuser::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    let environment: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result: DWORD_PTR = 0;

    // SAFETY: 所有指针指向函数内有效的栈/堆分配（environment 在调用期间持
    // 有引用，result 是栈变量）；HWND_BROADCAST 是 Windows API 公认的合
    // 法广播句柄；SendMessageTimeoutW 不会写超过 result 大小的字节。
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0 as WPARAM,
            environment.as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            5000_u32,
            &mut result,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn broadcast_environment_change() {
    // 非 Windows 平台无操作；保留同名空函数让调用点不需要 cfg。
}

/// Restore a single environment variable
#[cfg(target_os = "windows")]
fn restore_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "system" => {
            if conflict.source_path.contains("HKEY_CURRENT_USER") {
                let (hkcu, _) = RegKey::predef(HKEY_CURRENT_USER)
                    .create_subkey("Environment")
                    .map_err(|e| format!("打开注册表失败: {}", e))?;

                hkcu.set_value(&conflict.var_name, &conflict.var_value)
                    .map_err(|e| format!("恢复注册表项失败: {}", e))?;
            } else if conflict.source_path.contains("HKEY_LOCAL_MACHINE") {
                let (hklm, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
                    .create_subkey(
                        "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                    )
                    .map_err(|e| {
                        format!(
                            "{}打开系统注册表失败 (需要以管理员身份重启 cc-doctor): {}",
                            REQUIRES_ADMIN_SENTINEL, e
                        )
                    })?;

                hklm.set_value(&conflict.var_name, &conflict.var_value)
                    .map_err(|e| {
                        format!(
                            "{}恢复系统注册表项失败 (需要以管理员身份重启 cc-doctor): {}",
                            REQUIRES_ADMIN_SENTINEL, e
                        )
                    })?;
            }
            Ok(())
        }
        _ => Err(format!(
            "无法恢复类型为 {} 的环境变量",
            conflict.source_type
        )),
    }
}

#[cfg(not(target_os = "windows"))]
fn restore_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "file" => {
            // Parse file path from source_path
            let parts: Vec<&str> = conflict.source_path.split(':').collect();
            if parts.is_empty() {
                return Err("无效的文件路径格式".to_string());
            }

            let file_path = parts[0];

            // Read file content
            let mut content = fs::read_to_string(file_path)
                .map_err(|e| format!("读取文件失败 {file_path}: {e}"))?;

            // Append the environment variable line
            let export_line = format!("\nexport {}={}", conflict.var_name, conflict.var_value);
            content.push_str(&export_line);

            // Write back to file
            fs::write(file_path, content).map_err(|e| format!("写入文件失败 {file_path}: {e}"))?;

            Ok(())
        }
        _ => Err(format!(
            "无法恢复类型为 {} 的环境变量",
            conflict.source_type
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_dir_creation() {
        let backup_dir = get_backup_dir();
        assert!(backup_dir.is_ok());
    }

    #[test]
    fn broadcast_environment_change_does_not_panic_on_any_platform() {
        // 非 Windows 是 no-op；Windows 上 SendMessageTimeoutW 失败也只
        // 是无声返回——保证调用方可以无脑 fire-and-forget。这个回归
        // 保护防止有人意外把它改成 Result 或 panic。
        broadcast_environment_change();
    }

    #[test]
    fn requires_admin_sentinel_is_stable_machine_readable_marker() {
        // env_doctor::classify_fix_error 用 contains 匹配这个常量；
        // 改名/改值前必须同步那边的归一化逻辑及其单测。
        assert_eq!(REQUIRES_ADMIN_SENTINEL, "[REQUIRES_ADMIN] ");
    }
}

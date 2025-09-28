use serde::{Deserialize, Serialize};
// unused import removed
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 获取 Claude Code 配置目录路径
pub fn get_claude_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_claude_override_dir() {
        return custom;
    }

    dirs::home_dir()
        .expect("无法获取用户主目录")
        .join(".claude")
}

/// 获取 Claude Code 主配置文件路径
pub fn get_claude_settings_path() -> PathBuf {
    let dir = get_claude_config_dir();
    let settings = dir.join("settings.json");
    if settings.exists() {
        return settings;
    }
    // 兼容旧版命名：若存在旧文件则继续使用
    let legacy = dir.join("claude.json");
    if legacy.exists() {
        return legacy;
    }
    // 默认新建：回落到标准文件名 settings.json（不再生成 claude.json）
    settings
}

/// 获取应用配置目录路径 (~/.cc-switch)
pub fn get_app_config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("无法获取用户主目录")
        .join(".cc-switch")
}

/// 获取应用配置文件路径
pub fn get_app_config_path() -> PathBuf {
    get_app_config_dir().join("config.json")
}

/// 归档根目录 ~/.cc-switch/archive
pub fn get_archive_root() -> PathBuf {
    get_app_config_dir().join("archive")
}

fn ensure_unique_path(dest: PathBuf) -> PathBuf {
    if !dest.exists() {
        return dest;
    }
    let file_name = dest
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = dest
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    let parent = dest.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    for i in 2..1000 {
        let mut candidate = parent.clone();
        candidate.push(format!("{}-{}{}", file_name, i, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    dest
}

/// 将现有文件归档到 `~/.cc-switch/archive/<ts>/<category>/` 下，返回归档路径
pub fn archive_file(ts: u64, category: &str, src: &Path) -> Result<Option<PathBuf>, String> {
    if !src.exists() {
        return Ok(None);
    }
    let mut dest_dir = get_archive_root();
    dest_dir.push(ts.to_string());
    dest_dir.push(category);
    fs::create_dir_all(&dest_dir).map_err(|e| format!("创建归档目录失败: {}", e))?;

    let file_name = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let mut dest = dest_dir.join(file_name);
    dest = ensure_unique_path(dest);

    copy_file(src, &dest)?;
    Ok(Some(dest))
}

/// 清理供应商名称，确保文件名安全
pub fn sanitize_provider_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// 获取供应商配置文件路径
pub fn get_provider_config_path(provider_id: &str, provider_name: Option<&str>) -> PathBuf {
    let base_name = provider_name
        .map(|name| sanitize_provider_name(name))
        .unwrap_or_else(|| sanitize_provider_name(provider_id));

    get_claude_config_dir().join(format!("settings-{}.json", base_name))
}

/// 读取 JSON 配置文件
pub fn read_json_file<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    if !path.exists() {
        return Err(format!("文件不存在: {}", path.display()));
    }

    let content = fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?;

    serde_json::from_str(&content).map_err(|e| format!("解析 JSON 失败: {}", e))
}

/// 写入 JSON 配置文件
pub fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    // 确保目录存在
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    let json =
        serde_json::to_string_pretty(data).map_err(|e| format!("序列化 JSON 失败: {}", e))?;

    atomic_write(path, json.as_bytes())
}

/// 原子写入文本文件（用于 TOML/纯文本）
pub fn write_text_file(path: &Path, data: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    atomic_write(path, data.as_bytes())
}

/// 原子写入：写入临时文件后 rename 替换，避免半写状态
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    let parent = path.parent().ok_or_else(|| "无效的路径".to_string())?;
    let mut tmp = parent.to_path_buf();
    let file_name = path
        .file_name()
        .ok_or_else(|| "无效的文件名".to_string())?
        .to_string_lossy()
        .to_string();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    tmp.push(format!("{}.tmp.{}", file_name, ts));

    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {}", e))?;
        f.write_all(data)
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        f.flush().map_err(|e| format!("刷新临时文件失败: {}", e))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            let perm = meta.permissions().mode();
            let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(perm));
        }
    }

    #[cfg(windows)]
    {
        // Windows 上 rename 目标存在会失败，先移除再重命名（尽量接近原子性）
        if path.exists() {
            let _ = fs::remove_file(path);
        }
        fs::rename(&tmp, path).map_err(|e| format!("原子替换失败: {}", e))?;
    }

    #[cfg(not(windows))]
    {
        fs::rename(&tmp, path).map_err(|e| format!("原子替换失败: {}", e))?;
    }
    Ok(())
}

/// 复制文件
pub fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to).map_err(|e| format!("复制文件失败: {}", e))?;
    Ok(())
}

/// 删除文件
pub fn delete_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("删除文件失败: {}", e))?;
    }
    Ok(())
}

/// 检查 Claude Code 配置状态
#[derive(Serialize, Deserialize)]
pub struct ConfigStatus {
    pub exists: bool,
    pub path: String,
}

/// 获取 Claude Code 配置状态
pub fn get_claude_config_status() -> ConfigStatus {
    let path = get_claude_settings_path();
    ConfigStatus {
        exists: path.exists(),
        path: path.to_string_lossy().to_string(),
    }
}

//（移除未使用的备份/导入函数，避免 dead_code 告警）

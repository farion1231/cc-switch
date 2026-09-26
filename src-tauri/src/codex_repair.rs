//! Codex 修复工具：插件商店（官方 curated 插件市场）与 `auth.json` 的诊断 / 修复。
//!
//! 设计参考 [CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus) 的
//! `plugin_marketplace` 修复思路，并按 openai/codex 当前实现逐条核实：
//!
//! - Codex 启动时会把 `openai/plugins` 仓库同步到 `<codex_home>/.tmp/plugins`
//!   （`core-plugins/src/startup_sync.rs`：`CURATED_PLUGINS_RELATIVE_DIR =
//!   ".tmp/plugins"`），插件商店（curated store）从该目录加载。
//! - `config.toml` 里 `[marketplaces]` 名为 `openai-curated` /
//!   `openai-api-curated` 等保留名、且 `source` 不是 Codex 托管路径的条目，
//!   会被 `marketplace_policy` 静默过滤（`allowed_configured_marketplace_names_
//!   _with_policy` 对保留名只放行托管路径）——表现为插件商店空白、装不了插件。
//!
//! 因此“商店修复”做两件事：
//! 1. 商店目录缺失 / 损坏时，从 GitHub 重新下载 `openai/plugins` 并原子替换；
//! 2. 清理 config.toml 中会被 Codex 忽略的保留名注册条目（用户自己的非保留名
//!   市场与指向托管路径的条目一律保留）。
//!
//! 另外附带 `auth.json` 修复：历史遗留的 0 字节 / 损坏 JSON 会让 Codex 一直
//! 停在登录页（CodexPlusPlus issue #1604 同款问题），修复为合法 JSON 前先备份。

use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::config::atomic_write;
use crate::error::AppError;

/// Codex 官方 curated 插件市场的保留名（openai/codex `core-plugins/src/lib.rs`）。
const OPENAI_CURATED_MARKETPLACE_NAME: &str = "openai-curated";
const OPENAI_API_CURATED_MARKETPLACE_NAME: &str = "openai-api-curated";
/// Codex 启动同步 curated 商店的相对目录（相对 codex home）。
const CURATED_STORE_RELATIVE_DIR: &str = ".tmp/plugins";
/// 下载 openai/plugins 的 zip 地址（GitHub codeload）。
const OPENAI_PLUGINS_ZIP_URL: &str =
    "https://codeload.github.com/openai/plugins/zip/refs/heads/main";
/// 下载大小上限（CodexPlusPlus 同款 128 MiB）。
const OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES: usize = 128 * 1024 * 1024;
/// zip 条目数上限，防止异常包把磁盘写爆。
const MAX_ZIP_ENTRIES: usize = 5000;
/// 单个 zip 条目解压大小上限。
const MAX_ZIP_ENTRY_BYTES: usize = 64 * 1024 * 1024;
/// 替换商店目录时旧目录的备份名（修复成功后删除）。
const STORE_BACKUP_DIR_NAME: &str = "plugins.previous-cc-switch-repair";
/// 修复后 `auth.json` 的最小合法形状（未登录）。
const EMPTY_AUTH_JSON: &str = "{\"OPENAI_API_KEY\":\"\"}";

/// 商店（curated 插件市场）状态。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CuratedStoreStatus {
    /// `~/.codex/.tmp/plugins` 是否存在
    pub exists: bool,
    /// 目录结构是否有效（`marketplace.json` 可解析、name 正确、有插件、有 plugins 目录）
    pub valid: bool,
    /// marketplace.json 里声明的插件数
    pub plugin_count: usize,
    pub needs_repair: bool,
}

/// config.toml 中被 Codex 策略过滤的保留名市场条目。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlockedMarketplaceEntry {
    pub name: String,
    pub source: String,
    /// 该名字唯一允许的 Codex 托管路径
    pub managed_path: String,
}

/// config.toml 诊断结果。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDiagnosis {
    /// config.toml 无法解析时的错误信息（不阻塞其它修复项）
    pub parse_error: Option<String>,
    /// 会被 Codex 静默忽略的保留名注册条目
    pub blocked_marketplaces: Vec<BlockedMarketplaceEntry>,
}

/// `auth.json` 诊断结果。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthDiagnosis {
    pub exists: bool,
    pub valid_json: bool,
    pub needs_repair: bool,
}

/// 整体诊断结果（前端“Codex 修复工具”面板展示用）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRepairStatus {
    pub codex_home: String,
    pub curated_store: CuratedStoreStatus,
    pub config: ConfigDiagnosis,
    pub auth: AuthDiagnosis,
    pub needs_repair: bool,
}

/// 商店修复结果。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexStoreRepairResult {
    /// 商店目录缺失 / 损坏，本次重新下载并安装
    pub initialized: bool,
    /// config.toml 中被 Codex 忽略的保留名条目被清理
    pub config_cleaned: bool,
    pub plugin_count: usize,
    /// 修复后是否仍需要修复
    pub needs_repair: bool,
    /// 给用户看的摘要
    pub message: String,
}

/// `auth.json` 修复结果。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexAuthRepairResult {
    pub repaired: bool,
    pub backup_path: Option<String>,
    pub message: String,
}

/// 执行整体诊断（不修改任何文件）。
pub fn codex_repair_status() -> Result<CodexRepairStatus, AppError> {
    let home = crate::codex_config::get_codex_config_dir();
    let curated_store = curated_store_status(&home);
    let config = config_diagnosis(&home);
    let auth = auth_diagnosis(&home);
    let needs_repair =
        curated_store.needs_repair || !config.blocked_marketplaces.is_empty() || auth.needs_repair;
    Ok(CodexRepairStatus {
        codex_home: home.to_string_lossy().to_string(),
        curated_store,
        config,
        auth,
        needs_repair,
    })
}

/// 修复 Codex 插件商店：
/// 1. 商店目录缺失 / 损坏 → 从 GitHub 下载 `openai/plugins` 并原子替换；
/// 2. 清理 config.toml 中会被 Codex 静默忽略的保留名市场条目。
pub async fn repair_codex_plugin_store() -> Result<CodexStoreRepairResult, AppError> {
    let home = crate::codex_config::get_codex_config_dir();
    let mut initialized = false;
    let store_status = curated_store_status(&home);
    if !store_status.valid {
        let client = crate::proxy::http_client::get();
        let bytes = download_openai_plugins_zip(&client).await?;
        install_openai_plugins_zip(&home, &bytes)?;
        initialized = true;
    }
    let config_cleaned = cleanup_blocked_marketplace_configs(&home)?;
    let fresh = curated_store_status(&home);
    Ok(CodexStoreRepairResult {
        initialized,
        config_cleaned,
        plugin_count: fresh.plugin_count,
        needs_repair: fresh.needs_repair,
        message: repair_store_message(initialized, config_cleaned, &fresh),
    })
}

/// 修复 0 字节 / 损坏的 `auth.json`（修复前先备份）。
pub fn repair_codex_auth_json() -> Result<CodexAuthRepairResult, AppError> {
    repair_auth_json_at(&crate::codex_config::get_codex_auth_path())
}

fn repair_auth_json_at(auth_path: &Path) -> Result<CodexAuthRepairResult, AppError> {
    let diagnosis = auth_diagnosis_for(auth_path);
    if !diagnosis.needs_repair {
        let message = if diagnosis.exists {
            "auth.json 状态正常，无需修复。".to_string()
        } else {
            "auth.json 不存在，Codex 会按未登录处理，无需修复。".to_string()
        };
        return Ok(CodexAuthRepairResult {
            repaired: false,
            backup_path: None,
            message,
        });
    }

    // 备份损坏文件，再写入最小合法 JSON。
    let backup_path = backup_broken_auth_json(auth_path)?;
    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write(auth_path, EMPTY_AUTH_JSON.as_bytes())?;
    Ok(CodexAuthRepairResult {
        repaired: true,
        backup_path: Some(backup_path.to_string_lossy().to_string()),
        message: format!(
            "已把损坏的 auth.json 修复为合法 JSON，原文件已备份到 {}",
            backup_path.display()
        ),
    })
}

fn repair_store_message(
    initialized: bool,
    config_cleaned: bool,
    fresh: &CuratedStoreStatus,
) -> String {
    let mut parts = Vec::new();
    if initialized {
        parts.push("已从 openai/plugins 重新下载并安装插件商店".to_string());
    }
    if config_cleaned {
        parts.push("已清理 config.toml 中被 Codex 忽略的保留名市场条目".to_string());
    }
    if parts.is_empty() {
        parts.push(if fresh.needs_repair {
            "商店仍需要修复".to_string()
        } else {
            "插件商店状态正常，无需修复".to_string()
        });
    }
    if fresh.needs_repair {
        parts.push("修复后商店仍异常，请确认已完全退出 Codex 后重试".to_string());
    } else {
        parts.push(format!("当前商店包含 {} 个插件", fresh.plugin_count));
    }
    parts.join("；")
}

// ===== 商店目录诊断 =====

fn curated_store_root(home: &Path) -> PathBuf {
    home.join(CURATED_STORE_RELATIVE_DIR)
}

fn curated_store_status(home: &Path) -> CuratedStoreStatus {
    let root = curated_store_root(home);
    let exists = root.is_dir();
    if !exists {
        return CuratedStoreStatus {
            exists,
            valid: false,
            plugin_count: 0,
            needs_repair: true,
        };
    }
    match read_marketplace_plugin_count(&root) {
        Ok(plugin_count) => {
            let valid = plugin_count > 0 && root.join("plugins").is_dir();
            CuratedStoreStatus {
                exists,
                valid,
                plugin_count,
                needs_repair: !valid,
            }
        }
        Err(_) => CuratedStoreStatus {
            exists,
            valid: false,
            plugin_count: 0,
            needs_repair: true,
        },
    }
}

/// 读取并校验 `root/.agents/plugins/marketplace.json`，返回插件数。
fn read_marketplace_plugin_count(root: &Path) -> Result<usize, AppError> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    let text = std::fs::read_to_string(&marketplace_path)
        .map_err(|e| AppError::io(&marketplace_path, e))?;
    let marketplace: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| AppError::json(&marketplace_path, e))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_MARKETPLACE_NAME)
    {
        return Err(AppError::Message(format!(
            "{} 不是 openai-curated 市场（name={:?}）",
            marketplace_path.display(),
            marketplace.get("name")
        )));
    }
    let plugin_count = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| plugins.len())
        .unwrap_or(0);
    Ok(plugin_count)
}

// ===== config.toml 诊断与清理 =====

/// 保留名市场唯一允许的 Codex 托管路径（openai/codex `marketplace_policy.rs`）。
fn managed_marketplace_path(home: &Path, name: &str) -> Option<PathBuf> {
    match name {
        OPENAI_CURATED_MARKETPLACE_NAME => Some(curated_store_root(home)),
        OPENAI_API_CURATED_MARKETPLACE_NAME => Some(curated_store_root(home)),
        _ => None,
    }
}

fn is_curated_reserved_marketplace_name(name: &str) -> bool {
    matches!(
        name,
        OPENAI_CURATED_MARKETPLACE_NAME | OPENAI_API_CURATED_MARKETPLACE_NAME
    )
}

fn config_diagnosis(home: &Path) -> ConfigDiagnosis {
    let config_path = home.join("config.toml");
    let text = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ConfigDiagnosis {
                parse_error: None,
                blocked_marketplaces: Vec::new(),
            }
        }
        Err(error) => {
            return ConfigDiagnosis {
                parse_error: Some(format!("读取 config.toml 失败: {error}")),
                blocked_marketplaces: Vec::new(),
            }
        }
    };
    let text = text.trim_start_matches('\u{feff}');
    let doc = match text.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(error) => {
            return ConfigDiagnosis {
                parse_error: Some(format!("config.toml 无法解析: {error}")),
                blocked_marketplaces: Vec::new(),
            }
        }
    };
    let blocked = blocked_marketplace_entries(home, &doc);
    ConfigDiagnosis {
        parse_error: None,
        blocked_marketplaces: blocked,
    }
}

/// 收集会被 Codex 策略静默过滤的保留名市场条目。
fn blocked_marketplace_entries(
    home: &Path,
    doc: &toml_edit::DocumentMut,
) -> Vec<BlockedMarketplaceEntry> {
    let Some(marketplaces) = doc.get("marketplaces").and_then(toml_edit::Item::as_table) else {
        return Vec::new();
    };
    let mut blocked = Vec::new();
    for (name, item) in marketplaces.iter() {
        if !is_curated_reserved_marketplace_name(name) {
            continue;
        }
        let Some(managed_path) = managed_marketplace_path(home, name) else {
            continue;
        };
        let Some(table) = item.as_table() else {
            continue;
        };
        let source_type = table
            .get("source_type")
            .and_then(toml_edit::Item::as_str)
            .unwrap_or_default();
        let source = table
            .get("source")
            .and_then(toml_edit::Item::as_str)
            .unwrap_or_default()
            .to_string();
        // 只有 source_type=local 且 source 指向托管路径的保留名条目才会被 Codex 加载。
        if source_type != "local" || !managed_marketplace_path_matches(&source, &managed_path) {
            blocked.push(BlockedMarketplaceEntry {
                name: name.to_string(),
                source,
                managed_path: managed_path.to_string_lossy().to_string(),
            });
        }
    }
    blocked
}

/// Windows 长路径前缀归一化后比较。
fn managed_marketplace_path_matches(value: &str, path: &Path) -> bool {
    let normalized = path.to_string_lossy();
    let value = value.trim();
    value == normalized || value.strip_prefix(r"\\?\") == Some(normalized.as_ref())
}

/// 从 config.toml 移除会被 Codex 忽略的保留名市场条目，返回是否发生修改。
fn cleanup_blocked_marketplace_configs(home: &Path) -> Result<bool, AppError> {
    let config_path = home.join("config.toml");
    let existing = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(AppError::io(&config_path, error)),
    };
    let text = existing.trim_start_matches('\u{feff}');
    let mut doc = match text.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(error) => return Err(AppError::Message(format!("config.toml 无法解析: {error}"))),
    };
    let blocked_names = blocked_marketplace_entries(home, &doc)
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    if blocked_names.is_empty() {
        return Ok(false);
    }
    let mut changed = false;
    if let Some(marketplaces) = doc
        .get_mut("marketplaces")
        .and_then(toml_edit::Item::as_table_mut)
    {
        for name in blocked_names {
            marketplaces.remove(&name);
            changed = true;
        }
        if marketplaces.is_empty() {
            doc.as_table_mut().remove("marketplaces");
        }
    }
    if !changed {
        return Ok(false);
    }
    let mut output = doc.to_string();
    if !output.ends_with('\n') {
        output.push('\n');
    }
    atomic_write(&config_path, output.as_bytes())?;
    Ok(true)
}

// ===== 下载与安装 =====

async fn download_openai_plugins_zip(client: &reqwest::Client) -> Result<Vec<u8>, AppError> {
    let response = client
        .get(OPENAI_PLUGINS_ZIP_URL)
        .header(reqwest::header::ACCEPT, "application/zip")
        .send()
        .await
        .map_err(|e| AppError::Message(format!("下载 openai/plugins 失败: {e}")))?;
    let response = response
        .error_for_status()
        .map_err(|e| AppError::Message(format!("下载 openai/plugins 失败: {e}")))?;

    // 全局客户端禁用了自动解压，codeload 可能按 content-encoding 压缩，手动处理。
    let mut bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::Message(format!("读取 openai/plugins 下载内容失败: {e}")))?
        .to_vec();
    if bytes.len() > OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES {
        return Err(AppError::Message(format!(
            "openai/plugins 下载内容过大（{} 字节）",
            bytes.len()
        )));
    }
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        // gzip magic
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(Cursor::new(bytes))
            .take(OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES as u64 + 1)
            .read_to_end(&mut decoded)
            .map_err(|e| AppError::Message(format!("解压 openai/plugins 下载内容失败: {e}")))?;
        if decoded.len() > OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES {
            return Err(AppError::Message(format!(
                "openai/plugins 解压后过大（{} 字节）",
                decoded.len()
            )));
        }
        bytes = decoded;
    }
    Ok(bytes)
}

/// 把下载的 openai/plugins zip 安装到 `<home>/.tmp/plugins`。
///
/// 安装分阶段进行：先解压到临时目录并校验，再把旧目录改名备份后原子替换；
/// 任一步失败都会清理临时目录并回滚备份，不会留下半成品。
fn install_openai_plugins_zip(home: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let destination = curated_store_root(home);
    let tmp_parent = home.join(".tmp");
    std::fs::create_dir_all(&tmp_parent).map_err(|e| AppError::io(&tmp_parent, e))?;

    let staging = tmp_parent.join(format!(
        "plugins-repair-staging-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| AppError::io(&staging, e))?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| AppError::io(&staging, e))?;

    let result = extract_openai_plugins_zip(bytes, &staging)
        .and_then(|_| validate_curated_store_root(&staging))
        .and_then(|_| replace_directory(&staging, &destination));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// 校验解压结果是一个可用的 openai-curated 商店根目录。
fn validate_curated_store_root(root: &Path) -> Result<(), AppError> {
    let plugin_count = read_marketplace_plugin_count(root)?;
    if plugin_count == 0 {
        return Err(AppError::Message(
            "下载的 openai/plugins 市场没有声明任何插件".to_string(),
        ));
    }
    if !root.join("plugins").is_dir() {
        return Err(AppError::Message(
            "下载的 openai/plugins 缺少 plugins 目录".to_string(),
        ));
    }
    Ok(())
}

/// 解压 GitHub zip：跳过顶层包装目录（`<repo>-<branch>/`），拒绝逃逸路径。
fn extract_openai_plugins_zip(bytes: &[u8], destination: &Path) -> Result<(), AppError> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| AppError::Message(format!("无法读取 openai/plugins zip: {e}")))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(AppError::Message(format!(
            "openai/plugins zip 条目过多（{}）",
            archive.len()
        )));
    }
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|e| AppError::Message(format!("读取 zip 条目失败: {e}")))?;
        let entry_name = file.name().to_string();
        let Some(relative_path) = zip_entry_relative_path(&entry_name) else {
            continue;
        };
        let output_path = destination.join(&relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path).map_err(|e| AppError::io(&output_path, e))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let mut contents = Vec::new();
        file.by_ref()
            .take(MAX_ZIP_ENTRY_BYTES as u64 + 1)
            .read_to_end(&mut contents)
            .map_err(|e| AppError::Message(format!("读取 zip 条目 {entry_name} 失败: {e}")))?;
        if contents.len() > MAX_ZIP_ENTRY_BYTES {
            return Err(AppError::Message(format!("zip 条目 {entry_name} 过大")));
        }
        std::fs::write(&output_path, contents).map_err(|e| AppError::io(&output_path, e))?;
    }
    Ok(())
}

/// 去掉 GitHub zip 的顶层包装目录，返回相对路径；异常条目返回 None 跳过。
fn zip_entry_relative_path(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    let mut components = path.components();
    match components.next()? {
        Component::Normal(_) => {}
        _ => return None,
    }
    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

/// 用备份名原子替换目录：旧目录先改名备份，替换失败时回滚。
fn replace_directory(source: &Path, destination: &Path) -> Result<(), AppError> {
    let backup = destination.with_file_name(STORE_BACKUP_DIR_NAME);
    if backup.exists() {
        std::fs::remove_dir_all(&backup).map_err(|e| AppError::io(&backup, e))?;
    }
    if destination.exists() {
        std::fs::rename(destination, &backup).map_err(|e| {
            AppError::Message(format!(
                "无法移动旧商店目录 {} → {}：{e}（请先完全退出 Codex 再重试）",
                destination.display(),
                backup.display()
            ))
        })?;
    }
    match std::fs::rename(source, destination) {
        Ok(()) => {
            if backup.exists() {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(error) => {
            if backup.exists() {
                let _ = std::fs::rename(&backup, destination);
            }
            Err(AppError::Message(format!("安装新商店目录失败: {error}")))
        }
    }
}

// ===== auth.json 诊断与备份 =====

fn auth_diagnosis(home: &Path) -> AuthDiagnosis {
    auth_diagnosis_for(&home.join("auth.json"))
}

fn auth_diagnosis_for(auth_path: &Path) -> AuthDiagnosis {
    let bytes = match std::fs::read(auth_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return AuthDiagnosis {
                exists: false,
                valid_json: false,
                needs_repair: false,
            }
        }
        Err(_) => {
            return AuthDiagnosis {
                exists: true,
                valid_json: false,
                needs_repair: true,
            }
        }
    };
    let valid_json = serde_json::from_slice::<serde_json::Value>(&bytes).is_ok();
    AuthDiagnosis {
        exists: true,
        valid_json,
        needs_repair: !valid_json,
    }
}

/// 把损坏的 auth.json 改名为带时间戳的备份，返回备份路径。
fn backup_broken_auth_json(auth_path: &Path) -> Result<PathBuf, AppError> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup_path = auth_path.with_file_name(format!("auth.json.bak-{timestamp}"));
    std::fs::rename(auth_path, &backup_path).map_err(|e| AppError::io(auth_path, e))?;
    Ok(backup_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 构造一个仿 GitHub 结构的 openai/plugins zip（带顶层包装目录）。
    /// 条目名以 `/` 结尾视为目录，其余视为文件（空内容也是文件）。
    fn build_plugins_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        for (name, contents) in entries {
            let full_name = format!("plugins-main/{name}");
            if name.ends_with('/') {
                writer
                    .add_directory(full_name, options)
                    .expect("add directory entry");
            } else {
                writer
                    .start_file(full_name, options)
                    .expect("start file entry");
                writer.write_all(contents).expect("write entry contents");
            }
        }
        writer.finish().expect("finish zip").into_inner()
    }

    fn marketplace_json(plugins: &[&str]) -> Vec<u8> {
        let plugins_json = plugins
            .iter()
            .map(|name| format!(r#"{{"name":"{name}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"name":"openai-curated","interface":{{"displayName":"Codex official"}},"plugins":[{plugins_json}]}}"#
        )
        .into_bytes()
    }

    fn write_curated_store(home: &Path) {
        let root = home.join(".tmp").join("plugins");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        std::fs::create_dir_all(root.join("plugins").join("gmail")).unwrap();
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            marketplace_json(&["gmail", "slack"]),
        )
        .unwrap();
    }

    #[test]
    fn store_status_detects_missing_and_valid_stores() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();

        let missing = curated_store_status(home);
        assert!(!missing.exists);
        assert!(!missing.valid);
        assert!(missing.needs_repair);

        write_curated_store(home);
        let valid = curated_store_status(home);
        assert!(valid.exists);
        assert!(valid.valid);
        assert!(!valid.needs_repair);
        assert_eq!(valid.plugin_count, 2);
    }

    #[test]
    fn store_status_detects_corrupt_marketplace_json() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let root = home.join(".tmp").join("plugins");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            "{not json",
        )
        .unwrap();

        let status = curated_store_status(home);
        assert!(status.exists);
        assert!(!status.valid);
        assert!(status.needs_repair);
    }

    #[test]
    fn store_status_ignores_wrong_marketplace_name() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let root = home.join(".tmp").join("plugins");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            r#"{"name":"other-market","plugins":[{"name":"x"}]}"#,
        )
        .unwrap();

        let status = curated_store_status(home);
        assert!(status.exists);
        assert!(!status.valid);
        assert!(status.needs_repair);
    }

    #[test]
    fn blocked_entries_only_target_reserved_names_off_managed_path() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_curated_store(home);
        let config_path = home.join("config.toml");
        std::fs::write(
            &config_path,
            r#"
[marketplaces.openai-curated]
source_type = "local"
source = "/tmp/wrong-path"

[marketplaces.openai-api-curated]
source_type = "remote"
source = "https://example.com/plugins"

[marketplaces.my-marketplace]
source_type = "local"
source = "/tmp/my-marketplace"

[model_providers.custom]
name = "custom"
"#,
        )
        .unwrap();

        let diagnosis = config_diagnosis(home);
        assert!(diagnosis.parse_error.is_none());
        let names = diagnosis
            .blocked_marketplaces
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["openai-curated", "openai-api-curated"]);
    }

    #[test]
    fn managed_path_entries_are_not_blocked() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_curated_store(home);
        let managed = curated_store_root(home).to_string_lossy().to_string();
        std::fs::write(
            home.join("config.toml"),
            format!(
                r#"
[marketplaces.openai-curated]
source_type = "local"
source = "{managed}"
"#
            ),
        )
        .unwrap();

        let diagnosis = config_diagnosis(home);
        assert!(diagnosis.blocked_marketplaces.is_empty());
    }

    #[test]
    fn cleanup_removes_only_blocked_entries_and_preserves_user_marketplaces() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_curated_store(home);
        let config_path = home.join("config.toml");
        std::fs::write(
            &config_path,
            r#"
[marketplaces.openai-curated]
source_type = "local"
source = "/tmp/wrong-path"

[marketplaces.my-marketplace]
source_type = "local"
source = "/tmp/my-marketplace"
"#,
        )
        .unwrap();

        let changed = cleanup_blocked_marketplace_configs(home).unwrap();
        assert!(changed);

        let text = std::fs::read_to_string(&config_path).unwrap();
        let doc = text.parse::<toml_edit::DocumentMut>().unwrap();
        assert!(doc
            .get("marketplaces")
            .unwrap()
            .as_table()
            .unwrap()
            .get("openai-curated")
            .is_none());
        assert_eq!(
            doc["marketplaces"]["my-marketplace"]["source"].as_str(),
            Some("/tmp/my-marketplace")
        );
    }

    #[test]
    fn cleanup_removes_empty_marketplaces_table() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        std::fs::write(
            home.join("config.toml"),
            r#"
[marketplaces.openai-curated]
source_type = "local"
source = "/tmp/wrong-path"
"#,
        )
        .unwrap();

        let changed = cleanup_blocked_marketplace_configs(home).unwrap();
        assert!(changed);
        let text = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let doc = text.parse::<toml_edit::DocumentMut>().unwrap();
        assert!(doc.get("marketplaces").is_none());
    }

    #[test]
    fn install_replaces_missing_store_with_valid_zip() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let zip_bytes = build_plugins_zip(&[
            (
                ".agents/plugins/marketplace.json",
                &marketplace_json(&["gmail", "slack"]),
            ),
            ("plugins/gmail/", b""),
            ("plugins/gmail/.gitkeep", b""),
            ("plugins/slack/", b""),
            ("plugins/slack/.gitkeep", b""),
        ]);

        install_openai_plugins_zip(home, &zip_bytes).unwrap();

        let status = curated_store_status(home);
        assert!(status.valid);
        assert_eq!(status.plugin_count, 2);
        assert!(home
            .join(".tmp")
            .join("plugins")
            .join("plugins/gmail")
            .is_dir());
    }

    #[test]
    fn install_rejects_zip_escaping_destination() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        // 逃逸条目被跳过后 marketplace.json 缺失，安装必须失败且不留半成品。
        let zip_bytes = build_plugins_zip(&[
            (
                "../.agents/plugins/marketplace.json",
                &marketplace_json(&["gmail"]),
            ),
            ("../escape.txt", b"boom"),
            ("plugins/gmail/", b""),
        ]);

        let result = install_openai_plugins_zip(home, &zip_bytes);
        assert!(result.is_err());
        assert!(!home.join(".tmp").join("plugins").exists());
        // 逃逸文件不得落在商店目录之外
        assert!(!home.join("escape.txt").exists());
        assert!(!home.join(".agents").exists());
    }

    #[test]
    fn auth_repair_fixes_zero_byte_file_with_backup() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        std::fs::create_dir_all(home).unwrap();
        let auth_path = home.join("auth.json");
        std::fs::write(&auth_path, b"").unwrap();

        let result = repair_auth_json_at(&auth_path).unwrap();
        assert!(result.repaired);
        assert!(result.backup_path.is_some());

        let repaired = std::fs::read_to_string(&auth_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(parsed["OPENAI_API_KEY"].as_str(), Some(""));
        assert!(std::fs::read(result.backup_path.unwrap())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn auth_repair_leaves_valid_file_alone() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        std::fs::create_dir_all(home).unwrap();
        let auth_path = home.join("auth.json");
        std::fs::write(&auth_path, r#"{"OPENAI_API_KEY":"sk-test"}"#).unwrap();

        let result = repair_auth_json_at(&auth_path).unwrap();
        assert!(!result.repaired);
        assert!(result.backup_path.is_none());
        assert_eq!(
            std::fs::read_to_string(&auth_path).unwrap(),
            r#"{"OPENAI_API_KEY":"sk-test"}"#
        );
    }
}

#![cfg_attr(not(windows), allow(dead_code))]

use std::path::{Path, PathBuf};

use anyhow::Context;
use toml_edit::{Array, DocumentMut, Item, Table};

const BUNDLED_MARKETPLACE: &str = "openai-bundled";
const BUNDLED_MARKETPLACE_PLUGINS: &[&str] = &["browser", "chrome", "computer-use", "latex"];
const COMPUTER_USE_PLUGINS: &[&str] = &[
    "browser@openai-bundled",
    "chrome@openai-bundled",
    "computer-use@openai-bundled",
];
const COMPUTER_USE_EXE: &str = "codex-computer-use.exe";
const COMPUTER_USE_CLIENT_SCRIPT: &str = "computer-use-client.mjs";
const SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT: &str =
    "./dist/project/cua/sky_js/src/targets/windows/internal/computer_use_client_base.js";
const SKY_INTERNAL_COMPUTER_USE_CLIENT_IMPORT: &str =
    "@oai/sky/dist/project/cua/sky_js/src/targets/windows/internal/computer_use_client_base.js";
const SKY_PACKAGE_EXPORTS_BACKUP: &str = "package.json.bak-codexpp-runtime-exports";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardResult {
    pub changed: bool,
    pub notify_exe: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardArtifacts {
    pub notify_exe: Option<PathBuf>,
    pub marketplace_path: Option<PathBuf>,
    pub sky_package_json: Option<PathBuf>,
    pub runtime_exports_needed: bool,
}

pub(crate) fn resolve_computer_use_guard_artifacts(home: &Path) -> anyhow::Result<GuardArtifacts> {
    #[cfg(windows)]
    {
        let notify_exe = find_computer_use_notify_exe(home);
        let runtime_exports_needed = computer_use_client_needs_sky_internal_export(home)?;
        Ok(GuardArtifacts {
            sky_package_json: find_sky_package_json_for_notify_exe(notify_exe.as_deref())
                .or_else(find_latest_sky_package_json),
            notify_exe,
            marketplace_path: ensure_openai_bundled_marketplace(home)?,
            runtime_exports_needed,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = home;
        Ok(GuardArtifacts {
            notify_exe: None,
            marketplace_path: None,
            sky_package_json: None,
            runtime_exports_needed: false,
        })
    }
}

pub(crate) fn ensure_computer_use_config_with_artifacts(
    home: &Path,
    artifacts: &GuardArtifacts,
) -> anyhow::Result<GuardResult> {
    #[cfg(windows)]
    {
        ensure_computer_use_config_with_artifacts_windows(home, artifacts)
    }
    #[cfg(not(windows))]
    {
        let _ = (home, artifacts);
        Ok(GuardResult {
            changed: false,
            notify_exe: None,
        })
    }
}

#[cfg(windows)]
fn ensure_computer_use_config_with_artifacts_windows(
    home: &Path,
    artifacts: &GuardArtifacts,
) -> anyhow::Result<GuardResult> {
    let config_path = home.join("config.toml");
    let existing = match std::fs::read(&config_path) {
        Ok(bytes) => String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 {}", config_path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let updated = if let Some(marketplace_path) = artifacts.marketplace_path.as_deref() {
        guard_config_text_with_marketplace(
            &existing,
            artifacts.notify_exe.as_deref(),
            Some(marketplace_path),
        )?
    } else {
        guard_config_text(&existing, artifacts.notify_exe.as_deref())?
    };
    let changed = updated.as_bytes() != existing.as_bytes();
    if changed {
        crate::settings::atomic_write(&config_path, updated.as_bytes())?;
    }
    let runtime_compat = ensure_computer_use_runtime_exports_compat_windows(
        home,
        artifacts.sky_package_json.as_deref(),
    )?;
    Ok(GuardResult {
        changed: changed || runtime_compat.changed,
        notify_exe: artifacts.notify_exe.clone(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCompatResult {
    pub changed: bool,
    pub package_json: Option<PathBuf>,
    pub backup_path: Option<PathBuf>,
}

#[cfg(not(windows))]
pub(crate) fn ensure_computer_use_runtime_exports_compat(
    home: &Path,
) -> anyhow::Result<RuntimeCompatResult> {
    let _ = home;
    Ok(RuntimeCompatResult {
        changed: false,
        package_json: None,
        backup_path: None,
    })
}

#[cfg(windows)]
#[allow(dead_code)]
pub(crate) fn ensure_computer_use_runtime_exports_compat(
    home: &Path,
) -> anyhow::Result<RuntimeCompatResult> {
    ensure_computer_use_runtime_exports_compat_windows(
        home,
        find_latest_sky_package_json().as_deref(),
    )
}

#[cfg(windows)]
fn ensure_computer_use_runtime_exports_compat_windows(
    home: &Path,
    sky_package_json: Option<&Path>,
) -> anyhow::Result<RuntimeCompatResult> {
    if !computer_use_client_needs_sky_internal_export(home)? {
        return Ok(RuntimeCompatResult {
            changed: false,
            package_json: sky_package_json.map(Path::to_path_buf),
            backup_path: None,
        });
    }
    let Some(package_json) = sky_package_json else {
        return Ok(RuntimeCompatResult {
            changed: false,
            package_json: None,
            backup_path: None,
        });
    };
    if !sky_internal_computer_use_client_file_exists(package_json) {
        return Ok(RuntimeCompatResult {
            changed: false,
            package_json: Some(package_json.to_path_buf()),
            backup_path: None,
        });
    }

    let existing = std::fs::read_to_string(package_json)
        .with_context(|| format!("failed to read {}", package_json.display()))?;
    let Some(updated) = add_sky_internal_computer_use_export(&existing)? else {
        return Ok(RuntimeCompatResult {
            changed: false,
            package_json: Some(package_json.to_path_buf()),
            backup_path: None,
        });
    };

    let backup_path = package_json
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid @oai/sky package.json path"))?
        .join(SKY_PACKAGE_EXPORTS_BACKUP);
    if !backup_path.exists() {
        std::fs::copy(package_json, &backup_path).with_context(|| {
            format!(
                "failed to back up {} to {}",
                package_json.display(),
                backup_path.display()
            )
        })?;
    }
    atomic_write_runtime_file(package_json, updated.as_bytes())?;
    Ok(RuntimeCompatResult {
        changed: true,
        package_json: Some(package_json.to_path_buf()),
        backup_path: Some(backup_path),
    })
}

pub(crate) fn guard_config_text(
    config_text: &str,
    notify_exe: Option<&Path>,
) -> anyhow::Result<String> {
    guard_config_text_with_marketplace(config_text, notify_exe, None)
}

pub(crate) fn guard_config_text_with_marketplace(
    config_text: &str,
    notify_exe: Option<&Path>,
    marketplace_path: Option<&Path>,
) -> anyhow::Result<String> {
    let without_bom = config_text.trim_start_matches('\u{feff}');
    let mut doc = parse_toml_document(without_bom)?;

    let features = table_mut_or_insert(&mut doc, "features")?;
    features["js_repl"] = toml_edit::value(true);

    for plugin_id in COMPUTER_USE_PLUGINS {
        ensure_plugin_enabled(&mut doc, plugin_id)?;
    }

    if let Some(notify_exe) = notify_exe {
        let mut notify = Array::default();
        notify.push(notify_exe.to_string_lossy().as_ref());
        notify.push("turn-ended");
        doc["notify"] = toml_edit::value(notify);
    }

    if let Some(marketplace_path) = marketplace_path {
        ensure_openai_bundled_marketplace_config(&mut doc, marketplace_path)?;
    }

    Ok(ensure_trailing_newline(doc.to_string()))
}

pub(crate) fn find_computer_use_notify_exe(home: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        find_computer_use_notify_exe_windows(home)
    }
    #[cfg(not(windows))]
    {
        let _ = home;
        None
    }
}

#[cfg(windows)]
fn find_computer_use_notify_exe_windows(home: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        collect_named_files(
            &PathBuf::from(local_app_data)
                .join("OpenAI")
                .join("Codex")
                .join("runtimes")
                .join("cua_node"),
            COMPUTER_USE_EXE,
            12,
            &mut candidates,
        );
    }
    if candidates.is_empty() {
        collect_named_files(
            &home
                .join("plugins")
                .join("cache")
                .join("openai-bundled")
                .join("computer-use"),
            COMPUTER_USE_EXE,
            12,
            &mut candidates,
        );
    }
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn collect_named_files(root: &Path, file_name: &str, depth: usize, output: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
            {
                output.push(path);
            }
        } else if path.is_dir() {
            collect_named_files(&path, file_name, depth - 1, output);
        }
    }
}

#[cfg(windows)]
fn modified_millis(path: &Path) -> u128 {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(windows)]
fn computer_use_client_needs_sky_internal_export(home: &Path) -> anyhow::Result<bool> {
    let mut candidates = Vec::new();
    collect_named_files(
        &home
            .join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use"),
        COMPUTER_USE_CLIENT_SCRIPT,
        8,
        &mut candidates,
    );
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    for candidate in candidates {
        let contents = std::fs::read_to_string(&candidate)
            .with_context(|| format!("failed to read {}", candidate.display()))?;
        if contents.contains(SKY_INTERNAL_COMPUTER_USE_CLIENT_IMPORT) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(windows)]
fn find_sky_package_json_for_notify_exe(notify_exe: Option<&Path>) -> Option<PathBuf> {
    let notify_exe = notify_exe?;
    for ancestor in notify_exe.ancestors() {
        if ancestor
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("sky"))
            && ancestor
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("@oai"))
        {
            let package_json = ancestor.join("package.json");
            if package_json.is_file() {
                return Some(package_json);
            }
        }
    }
    None
}

#[cfg(windows)]
fn find_latest_sky_package_json() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let runtimes = PathBuf::from(local_app_data)
        .join("OpenAI")
        .join("Codex")
        .join("runtimes")
        .join("cua_node");
    let Ok(entries) = std::fs::read_dir(runtimes) else {
        return None;
    };
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| {
            entry
                .path()
                .join("bin")
                .join("node_modules")
                .join("@oai")
                .join("sky")
                .join("package.json")
        })
        .filter(|path| path.is_file())
        .collect();
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn sky_internal_computer_use_client_file_exists(package_json: &Path) -> bool {
    let Some(package_root) = package_json.parent() else {
        return false;
    };
    package_root
        .join(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.trim_start_matches("./"))
        .is_file()
}

fn add_sky_internal_computer_use_export(contents: &str) -> anyhow::Result<Option<String>> {
    let mut package: serde_json::Value =
        serde_json::from_str(contents).with_context(|| "@oai/sky package.json parse failed")?;
    let Some(exports) = package
        .get_mut("exports")
        .and_then(|value| value.as_object_mut())
    else {
        return Ok(None);
    };
    if exports.contains_key(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT) {
        return Ok(None);
    }
    exports.insert(
        SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.to_string(),
        serde_json::Value::String(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.to_string()),
    );
    let mut updated = serde_json::to_string_pretty(&package)?;
    updated.push('\n');
    Ok(Some(updated))
}

#[cfg(windows)]
fn atomic_write_runtime_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid runtime file path"))?;
    let temp = parent.join(format!(
        ".{}.codexpp-tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("package.json")
    ));
    std::fs::write(&temp, bytes).with_context(|| format!("failed to write {}", temp.display()))?;
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error).with_context(|| format!("failed to replace {}", path.display()))
        }
    }
}

#[cfg(windows)]
pub(crate) fn ensure_openai_bundled_marketplace(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let active = home
        .join(".tmp")
        .join("bundled-marketplaces")
        .join(BUNDLED_MARKETPLACE);
    if is_complete_openai_bundled_marketplace(&active) {
        return Ok(Some(active));
    }
    if let Some(configured) = configured_openai_bundled_marketplace(home) {
        if is_complete_openai_bundled_marketplace(&configured) {
            return Ok(Some(configured));
        }
    }

    let parent = active
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid bundled marketplace path"))?;
    std::fs::create_dir_all(parent)?;

    let staging = parent.join(format!(
        "{BUNDLED_MARKETPLACE}.guard-staging-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }

    if let Some(source) = find_complete_openai_bundled_marketplace(parent, &active) {
        copy_dir_recursive(&source, &staging)?;
    } else if can_build_marketplace_from_cache(home) {
        build_marketplace_from_cache(home, &staging)?;
    } else {
        return Ok(None);
    }

    match replace_active_marketplace(&active, &staging) {
        Ok(()) => Ok(Some(active)),
        Err(_) if is_complete_openai_bundled_marketplace(&staging) => {
            // Windows can keep the active marketplace directory pinned while
            // Codex extension hosts are still alive. Pointing config at the
            // complete staging marketplace still restores plugin discovery.
            Ok(Some(staging))
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to replace active bundled marketplace at {}",
                active.display()
            )
        }),
    }
}

#[cfg(windows)]
fn configured_openai_bundled_marketplace(home: &Path) -> Option<PathBuf> {
    let config = std::fs::read_to_string(home.join("config.toml")).ok()?;
    let without_bom = config.trim_start_matches('\u{feff}');
    let doc = parse_toml_document(without_bom).ok()?;
    let source = doc
        .get("marketplaces")?
        .as_table()?
        .get(BUNDLED_MARKETPLACE)?
        .as_table()?
        .get("source")?
        .as_str()?;
    Some(path_from_configured_marketplace_source(source))
}

#[cfg(windows)]
fn path_from_configured_marketplace_source(source: &str) -> PathBuf {
    source
        .strip_prefix(r"\\?\")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(source))
}

#[cfg(windows)]
fn is_complete_openai_bundled_marketplace(path: &Path) -> bool {
    if !path
        .join(".agents")
        .join("plugins")
        .join("marketplace.json")
        .is_file()
    {
        return false;
    }
    BUNDLED_MARKETPLACE_PLUGINS.iter().all(|plugin| {
        path.join("plugins")
            .join(plugin)
            .join(".codex-plugin")
            .join("plugin.json")
            .is_file()
    })
}

#[cfg(windows)]
fn find_complete_openai_bundled_marketplace(parent: &Path, active: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    let Ok(entries) = std::fs::read_dir(parent) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == active || !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with(BUNDLED_MARKETPLACE) && is_complete_openai_bundled_marketplace(&path) {
            candidates.push(path);
        }
    }
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn cache_plugin_root(home: &Path, plugin: &str) -> PathBuf {
    home.join("plugins")
        .join("cache")
        .join(BUNDLED_MARKETPLACE)
        .join(plugin)
}

#[cfg(windows)]
fn can_build_marketplace_from_cache(home: &Path) -> bool {
    BUNDLED_MARKETPLACE_PLUGINS
        .iter()
        .all(|plugin| latest_cache_plugin_version(home, plugin).is_some())
}

#[cfg(windows)]
fn latest_cache_plugin_version(home: &Path, plugin: &str) -> Option<PathBuf> {
    let root = cache_plugin_root(home, plugin);
    let mut candidates = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.join(".codex-plugin").join("plugin.json").is_file() {
            candidates.push(path);
        }
    }
    candidates.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let right_name = right
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        right_name
            .cmp(left_name)
            .then_with(|| modified_millis(right).cmp(&modified_millis(left)))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn build_marketplace_from_cache(home: &Path, staging: &Path) -> anyhow::Result<()> {
    let plugins_dir = staging.join("plugins");
    std::fs::create_dir_all(staging.join(".agents").join("plugins"))?;
    std::fs::create_dir_all(&plugins_dir)?;
    std::fs::write(
        staging
            .join(".agents")
            .join("plugins")
            .join("marketplace.json"),
        bundled_marketplace_json().as_bytes(),
    )?;
    for plugin in BUNDLED_MARKETPLACE_PLUGINS {
        let Some(source) = latest_cache_plugin_version(home, plugin) else {
            anyhow::bail!("missing cached {plugin} plugin for openai-bundled marketplace");
        };
        copy_dir_recursive(&source, &plugins_dir.join(plugin))?;
    }
    Ok(())
}

#[cfg(windows)]
fn bundled_marketplace_json() -> String {
    let plugins = [
        ("browser", "Engineering"),
        ("chrome", "Productivity"),
        ("computer-use", "Productivity"),
        ("latex", "Research"),
    ]
    .into_iter()
    .map(|(name, category)| {
        serde_json::json!({
            "name": name,
            "source": {
                "source": "local",
                "path": format!("./plugins/{name}")
            },
            "policy": {
                "installation": "AVAILABLE",
                "authentication": "ON_INSTALL"
            },
            "category": category
        })
    })
    .collect::<Vec<_>>();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": BUNDLED_MARKETPLACE,
        "interface": {
            "displayName": "OpenAI Bundled"
        },
        "plugins": plugins
    }))
    .expect("bundled marketplace JSON should serialize")
}

#[cfg(windows)]
fn replace_active_marketplace(active: &Path, staging: &Path) -> anyhow::Result<()> {
    if active.exists() {
        let backup = active.with_file_name(format!(
            "{BUNDLED_MARKETPLACE}.bak-guard-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        std::fs::rename(active, backup)?;
    }
    std::fs::rename(staging, active)?;
    Ok(())
}

#[cfg(windows)]
fn copy_dir_recursive(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            std::fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn ensure_openai_bundled_marketplace_config(
    doc: &mut DocumentMut,
    marketplace_path: &Path,
) -> anyhow::Result<()> {
    let marketplaces = table_mut_or_insert(doc, "marketplaces")?;
    if marketplaces
        .get(BUNDLED_MARKETPLACE)
        .and_then(Item::as_table)
        .is_none()
    {
        marketplaces[BUNDLED_MARKETPLACE] = toml_edit::table();
    }
    marketplaces[BUNDLED_MARKETPLACE]["source_type"] = toml_edit::value("local");
    marketplaces[BUNDLED_MARKETPLACE]["source"] =
        toml_edit::value(windows_extended_path(marketplace_path));
    Ok(())
}

fn windows_extended_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value.starts_with(r"\\?\") {
        value.into_owned()
    } else {
        format!(r"\\?\{value}")
    }
}

fn parse_toml_document(contents: &str) -> anyhow::Result<DocumentMut> {
    if contents.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        contents
            .parse::<DocumentMut>()
            .with_context(|| "config.toml TOML parse failed")
    }
}

fn table_mut_or_insert<'a>(doc: &'a mut DocumentMut, key: &str) -> anyhow::Result<&'a mut Table> {
    if !doc.as_table().contains_key(key) {
        doc[key] = toml_edit::table();
    }
    if doc.get(key).and_then(Item::as_table).is_none() {
        doc[key] = toml_edit::table();
    }
    doc.get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("{key} must be a TOML table"))
}

fn ensure_plugin_enabled(doc: &mut DocumentMut, plugin_id: &str) -> anyhow::Result<()> {
    let plugins = table_mut_or_insert(doc, "plugins")?;
    if !plugins.contains_key(plugin_id) {
        plugins[plugin_id] = toml_edit::table();
    }
    if plugins.get(plugin_id).and_then(Item::as_table).is_none() {
        plugins[plugin_id] = toml_edit::table();
    }
    plugins[plugin_id]["enabled"] = toml_edit::value(true);
    Ok(())
}

fn ensure_trailing_newline(mut contents: String) -> String {
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_config_text_repairs_computer_use_settings() {
        let updated = guard_config_text(
            "\u{feff}notify = [\"old.exe\", \"turn-ended\"]\n\n[features]\njs_repl = false\n\n[plugins.\"computer-use@openai-bundled\"]\nenabled = false\n",
            Some(Path::new(r"C:\tools\codex-computer-use.exe")),
        )
        .unwrap();

        assert!(!updated.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]));
        assert!(updated.contains("js_repl = true"));
        assert!(updated.contains("[plugins.\"browser@openai-bundled\"]"));
        assert!(updated.contains("[plugins.\"chrome@openai-bundled\"]"));
        assert!(updated.contains("[plugins.\"computer-use@openai-bundled\"]"));
        assert!(updated.contains("enabled = true"));
        let parsed = updated.parse::<DocumentMut>().unwrap();
        let notify = parsed["notify"].as_array().unwrap();
        assert_eq!(
            notify.get(0).and_then(|value| value.as_str()),
            Some(r"C:\tools\codex-computer-use.exe")
        );
        assert_eq!(
            notify.get(1).and_then(|value| value.as_str()),
            Some("turn-ended")
        );
        assert!(!updated.contains("old.exe"));
    }

    #[test]
    fn guard_config_text_creates_missing_sections() {
        let updated = guard_config_text("model = \"gpt-5\"\n", None).unwrap();

        assert!(updated.contains("[features]"));
        assert!(updated.contains("js_repl = true"));
        for plugin_id in COMPUTER_USE_PLUGINS {
            assert!(updated.contains(&format!("[plugins.\"{plugin_id}\"]")));
        }
        assert!(!updated.contains("notify ="));
    }

    #[test]
    fn guard_config_text_writes_openai_bundled_marketplace_source() {
        let updated = guard_config_text_with_marketplace(
            "model = \"gpt-5\"\n\n[marketplaces.openai-bundled]\nsource_type = \"remote\"\nsource = \"old\"\n",
            None,
            Some(Path::new(r"C:\Users\me\.codex\.tmp\bundled-marketplaces\openai-bundled")),
        )
        .unwrap();
        let parsed = updated.parse::<DocumentMut>().unwrap();
        assert_eq!(
            parsed["marketplaces"]["openai-bundled"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-bundled"]["source"].as_str(),
            Some(r"\\?\C:\Users\me\.codex\.tmp\bundled-marketplaces\openai-bundled")
        );
    }

    #[test]
    fn add_sky_internal_computer_use_export_adds_exact_subpath() {
        let updated = add_sky_internal_computer_use_export(
            r#"{
  "name": "@oai/sky",
  "exports": {
    ".": "./dist/project/cua/sky_js/src/index.js"
  }
}"#,
        )
        .unwrap()
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&updated).unwrap();

        assert_eq!(
            parsed["exports"][SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT].as_str(),
            Some(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT)
        );
        assert!(updated.ends_with('\n'));
    }

    #[test]
    fn add_sky_internal_computer_use_export_is_idempotent() {
        let updated = add_sky_internal_computer_use_export(&format!(
            r#"{{
  "name": "@oai/sky",
  "exports": {{
    ".": "./dist/project/cua/sky_js/src/index.js",
    "{SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT}": "{SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT}"
  }}
}}"#
        ))
        .unwrap();

        assert!(updated.is_none());
    }

    #[test]
    fn add_sky_internal_computer_use_export_ignores_non_object_exports() {
        let updated =
            add_sky_internal_computer_use_export(r#"{ "name": "@oai/sky", "exports": "." }"#)
                .unwrap();

        assert!(updated.is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn runtime_exports_compat_is_noop_off_windows() {
        let temp = tempfile::tempdir().unwrap();
        let result = ensure_computer_use_runtime_exports_compat(temp.path()).unwrap();

        assert!(!result.changed);
        assert!(result.package_json.is_none());
        assert!(result.backup_path.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn runtime_exports_compat_adds_missing_exact_export() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let script = home
            .join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use")
            .join("26.608.12217")
            .join("scripts")
            .join(COMPUTER_USE_CLIENT_SCRIPT);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(
            &script,
            format!("import {{ x }} from \"{SKY_INTERNAL_COMPUTER_USE_CLIENT_IMPORT}\";\n"),
        )
        .unwrap();

        let package_json = temp.path().join("@oai").join("sky").join("package.json");
        let internal_file = package_json
            .parent()
            .unwrap()
            .join(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.trim_start_matches("./"));
        std::fs::create_dir_all(internal_file.parent().unwrap()).unwrap();
        std::fs::write(
            &internal_file,
            "export class WindowsComputerUseClientBase {}\n",
        )
        .unwrap();
        std::fs::write(
            &package_json,
            r#"{
  "name": "@oai/sky",
  "exports": {
    ".": "./dist/project/cua/sky_js/src/index.js"
  }
}
"#,
        )
        .unwrap();

        let result =
            ensure_computer_use_runtime_exports_compat_windows(&home, Some(&package_json)).unwrap();

        assert!(result.changed);
        assert_eq!(result.package_json.as_deref(), Some(package_json.as_path()));
        assert!(result.backup_path.as_deref().unwrap().is_file());
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&package_json).unwrap()).unwrap();
        assert_eq!(
            parsed["exports"][SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT].as_str(),
            Some(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT)
        );
    }

    #[cfg(windows)]
    #[test]
    fn runtime_exports_compat_skips_when_internal_file_missing() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let script = home
            .join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use")
            .join("26.608.12217")
            .join("scripts")
            .join(COMPUTER_USE_CLIENT_SCRIPT);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(
            &script,
            format!("import {{ x }} from \"{SKY_INTERNAL_COMPUTER_USE_CLIENT_IMPORT}\";\n"),
        )
        .unwrap();
        let package_json = temp.path().join("@oai").join("sky").join("package.json");
        std::fs::create_dir_all(package_json.parent().unwrap()).unwrap();
        std::fs::write(
            &package_json,
            r#"{ "name": "@oai/sky", "exports": { ".": "./index.js" } }"#,
        )
        .unwrap();

        let result =
            ensure_computer_use_runtime_exports_compat_windows(&home, Some(&package_json)).unwrap();

        assert!(!result.changed);
        assert!(
            !package_json
                .parent()
                .unwrap()
                .join(SKY_PACKAGE_EXPORTS_BACKUP)
                .exists()
        );
    }

    #[cfg(windows)]
    #[test]
    fn runtime_exports_compat_skips_when_plugin_script_no_longer_needs_patch() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let script = home
            .join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use")
            .join("26.608.12217")
            .join("scripts")
            .join(COMPUTER_USE_CLIENT_SCRIPT);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "import { sky } from \"@oai/sky\";\n").unwrap();
        let package_json = temp.path().join("@oai").join("sky").join("package.json");
        let internal_file = package_json
            .parent()
            .unwrap()
            .join(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.trim_start_matches("./"));
        std::fs::create_dir_all(internal_file.parent().unwrap()).unwrap();
        std::fs::write(
            &internal_file,
            "export class WindowsComputerUseClientBase {}\n",
        )
        .unwrap();
        std::fs::write(
            &package_json,
            r#"{ "name": "@oai/sky", "exports": { ".": "./index.js" } }"#,
        )
        .unwrap();

        let result =
            ensure_computer_use_runtime_exports_compat_windows(&home, Some(&package_json)).unwrap();

        assert!(!result.changed);
    }

    #[cfg(windows)]
    #[test]
    fn ensure_openai_bundled_marketplace_rebuilds_damaged_active_from_cache() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let active = home
            .join(".tmp")
            .join("bundled-marketplaces")
            .join(BUNDLED_MARKETPLACE);
        std::fs::create_dir_all(active.join("plugins").join("chrome").join(".codex-plugin"))
            .unwrap();
        std::fs::write(
            active
                .join("plugins")
                .join("chrome")
                .join(".codex-plugin")
                .join("plugin.json"),
            "{}",
        )
        .unwrap();

        for plugin in BUNDLED_MARKETPLACE_PLUGINS {
            let root = home
                .join("plugins")
                .join("cache")
                .join(BUNDLED_MARKETPLACE)
                .join(plugin)
                .join("26.608.12217");
            std::fs::create_dir_all(root.join(".codex-plugin")).unwrap();
            std::fs::write(root.join(".codex-plugin").join("plugin.json"), "{}").unwrap();
            std::fs::write(root.join("payload.txt"), plugin).unwrap();
        }

        let repaired = ensure_openai_bundled_marketplace(home).unwrap().unwrap();
        assert_eq!(repaired, active);
        assert!(
            active
                .join(".agents")
                .join("plugins")
                .join("marketplace.json")
                .is_file()
        );
        let marketplace = std::fs::read_to_string(
            active
                .join(".agents")
                .join("plugins")
                .join("marketplace.json"),
        )
        .unwrap();
        assert!(marketplace.contains("\"computer-use\""));
        for plugin in BUNDLED_MARKETPLACE_PLUGINS {
            assert!(
                active
                    .join("plugins")
                    .join(plugin)
                    .join(".codex-plugin")
                    .join("plugin.json")
                    .is_file()
            );
            assert_eq!(
                std::fs::read_to_string(active.join("plugins").join(plugin).join("payload.txt"))
                    .unwrap(),
                *plugin
            );
        }
        let backup_count = std::fs::read_dir(active.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("openai-bundled.bak-guard-")
            })
            .count();
        assert_eq!(backup_count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn ensure_openai_bundled_marketplace_reuses_configured_complete_staging() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let parent = home.join(".tmp").join("bundled-marketplaces");
        let active = parent.join(BUNDLED_MARKETPLACE);
        let configured = parent.join("openai-bundled.guard-staging-existing");
        std::fs::create_dir_all(active.join("plugins")).unwrap();
        std::fs::create_dir_all(configured.join(".agents").join("plugins")).unwrap();
        std::fs::write(
            configured
                .join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            "{}",
        )
        .unwrap();
        for plugin in BUNDLED_MARKETPLACE_PLUGINS {
            let plugin_root = configured
                .join("plugins")
                .join(plugin)
                .join(".codex-plugin");
            std::fs::create_dir_all(&plugin_root).unwrap();
            std::fs::write(plugin_root.join("plugin.json"), "{}").unwrap();
        }
        let source = format!(r"\\?\{}", configured.display());
        std::fs::write(
            home.join("config.toml"),
            format!(
                "[marketplaces.openai-bundled]\nsource_type = \"local\"\nsource = '{}'\n",
                source
            ),
        )
        .unwrap();

        let repaired = ensure_openai_bundled_marketplace(home).unwrap().unwrap();
        assert_eq!(repaired, configured);
        let guard_staging_count = std::fs::read_dir(parent)
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("openai-bundled.guard-staging-")
            })
            .count();
        assert_eq!(guard_staging_count, 1);
    }
}

/// Kill orphaned SkyComputerUseClient processes on macOS.
///
/// On macOS, Codex spawns a `SkyComputerUseClient` subprocess for each
/// Computer Use session via the bundled openai-bundled computer-use plugin.
/// Codex does not reliably clean these up when conversations end — they
/// accumulate and consume significant memory (~20MB RSS each), eventually
/// causing swap pressure and UI freezes.
///
/// This function kills all `SkyComputerUseClient` processes it can find.
/// Codex re-spawns them lazily on the next Computer Use session, so killing
/// them is safe and does not affect active conversations.
///
/// We intentionally leave `node_repl` processes alone — they are lightweight
/// (~1MB RSS) and killing them could disrupt in-flight code execution.
#[cfg(target_os = "macos")]
pub fn kill_orphaned_computer_use_processes() {
    let _ = std::process::Command::new("pkill")
        .arg("-f")
        .arg("SkyComputerUseClient")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

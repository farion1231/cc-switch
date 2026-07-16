//! Codex plugin marketplace + cache management.
//! Manual-only: initialize/repair and refresh are explicit user actions.
//! Never auto-downloads. Blocks version downgrades. Hardens ZIP extraction.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

const MARKETPLACE_ID: &str = "openai-curated-remote";
const MAX_ZIP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCacheInfo {
    pub id: String,
    pub marketplace: String,
    pub source_version: Option<String>,
    pub current_version: Option<String>,
    pub cached_versions: Vec<String>,
    pub can_refresh: bool,
    pub refresh_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceResult {
    pub initialized: bool,
    pub configured: bool,
    pub marketplace_root: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct HomeSources {
    pub override_dir: Option<PathBuf>,
    pub env_codex_home: Option<PathBuf>,
    pub default_home: PathBuf,
}

/// Testable home resolution: override > CODEX_HOME env > ~/.codex
pub fn effective_codex_home_with(sources: &HomeSources) -> PathBuf {
    if let Some(o) = &sources.override_dir {
        if !o.as_os_str().is_empty() {
            return o.clone();
        }
    }
    if let Some(e) = &sources.env_codex_home {
        if !e.as_os_str().is_empty() {
            return e.clone();
        }
    }
    sources.default_home.clone()
}

pub fn effective_codex_home() -> PathBuf {
    let override_dir = crate::settings::get_codex_override_dir();
    let env_codex_home = std::env::var_os("CODEX_HOME").map(PathBuf::from);
    let default_home = crate::config::get_home_dir().join(".codex");
    effective_codex_home_with(&HomeSources {
        override_dir,
        env_codex_home,
        default_home,
    })
}

fn marketplace_root(home: &Path) -> PathBuf {
    home.join("plugins")
        .join("marketplaces")
        .join(MARKETPLACE_ID)
}

fn plugin_cache_root(home: &Path) -> PathBuf {
    home.join("plugins").join("cache").join(MARKETPLACE_ID)
}

fn is_safe_zip_path(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return false;
    }
    for c in p.components() {
        match c {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => return false, // ParentDir, RootDir, Prefix, etc.
        }
    }
    // reject Windows drive-like and backslash absolute
    if name.contains(':') {
        return false;
    }
    true
}

/// Extract ZIP bytes into `dest`, rejecting path traversal / absolute / symlink-like entries.
pub fn extract_zip_hardened(zip_bytes: &[u8], dest: &Path) -> Result<(), AppError> {
    if zip_bytes.len() as u64 > MAX_ZIP_BYTES {
        return Err(AppError::InvalidInput(format!(
            "ZIP exceeds {} MiB limit",
            MAX_ZIP_BYTES / (1024 * 1024)
        )));
    }
    fs::create_dir_all(dest).map_err(|e| AppError::io(dest, e))?;
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| AppError::InvalidInput(format!("invalid ZIP: {e}")))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| AppError::InvalidInput(format!("zip entry {i}: {e}")))?;
        let name = file.name().to_string();
        if !is_safe_zip_path(&name) {
            return Err(AppError::InvalidInput(format!(
                "unsafe ZIP path rejected: {name}"
            )));
        }
        // zip crate: unix mode high bits for symlink
        #[cfg(unix)]
        {
            if file
                .unix_mode()
                .map(|m| (m & 0o170000) == 0o120000)
                .unwrap_or(false)
            {
                return Err(AppError::InvalidInput(format!(
                    "symlink ZIP entry rejected: {name}"
                )));
            }
        }
        let out_path = dest.join(&name);
        if name.ends_with('/') {
            fs::create_dir_all(&out_path).map_err(|e| AppError::io(&out_path, e))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let mut outfile = fs::File::create(&out_path).map_err(|e| AppError::io(&out_path, e))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| AppError::io(&out_path, e))?;
        outfile
            .write_all(&buf)
            .map_err(|e| AppError::io(&out_path, e))?;
    }
    Ok(())
}

/// Merge marketplace section into config.toml using toml_edit; preserve unrelated text.
pub fn ensure_marketplace_in_config(
    config_path: &Path,
    marketplace_path: &Path,
) -> Result<(), AppError> {
    let existing = if config_path.exists() {
        fs::read_to_string(config_path).map_err(|e| AppError::io(config_path, e))?
    } else {
        String::new()
    };
    let mut doc: toml_edit::DocumentMut = existing
        .parse()
        .map_err(|e| AppError::InvalidInput(format!("config.toml parse: {e}")))?;

    // [plugins.marketplaces.openai-curated-remote]
    let plugins = doc
        .as_table_mut()
        .entry("plugins")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let plugins_tbl = plugins
        .as_table_mut()
        .ok_or_else(|| AppError::InvalidInput("plugins is not a table".into()))?;
    let mps = plugins_tbl
        .entry("marketplaces")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let mps_tbl = mps
        .as_table_mut()
        .ok_or_else(|| AppError::InvalidInput("plugins.marketplaces is not a table".into()))?;
    let entry = mps_tbl
        .entry(MARKETPLACE_ID)
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let entry_tbl = entry
        .as_table_mut()
        .ok_or_else(|| AppError::InvalidInput("marketplace entry is not a table".into()))?;
    entry_tbl.insert(
        "source",
        toml_edit::value(marketplace_path.to_string_lossy().as_ref()),
    );
    entry_tbl.insert("enabled", toml_edit::value(true));

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    // atomic-ish write
    let tmp = config_path.with_extension("toml.tmp");
    fs::write(&tmp, doc.to_string()).map_err(|e| AppError::io(&tmp, e))?;
    fs::rename(&tmp, config_path).map_err(|e| AppError::io(config_path, e))?;
    Ok(())
}

fn read_version_file(dir: &Path) -> Option<String> {
    let candidates = ["VERSION", "version", "plugin.json", "manifest.json"];
    for name in candidates {
        let p = dir.join(name);
        if !p.exists() {
            continue;
        }
        if name.ends_with(".json") {
            if let Ok(txt) = fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                    if let Some(ver) = v.get("version").and_then(|x| x.as_str()) {
                        return Some(ver.to_string());
                    }
                }
            }
        } else if let Ok(txt) = fs::read_to_string(&p) {
            let v = txt.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    // directory name as version fallback (e.g. cache/<id>/<version>)
    dir.file_name()
        .and_then(|s| s.to_str())
        .filter(|s| {
            s.chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
        })
        .map(|s| s.to_string())
}

/// Compare simple dotted versions; returns true if `a` < `b` (a is older).
fn version_lt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let av = parse(a);
    let bv = parse(b);
    let n = av.len().max(bv.len());
    for i in 0..n {
        let x = av.get(i).copied().unwrap_or(0);
        let y = bv.get(i).copied().unwrap_or(0);
        if x < y {
            return true;
        }
        if x > y {
            return false;
        }
    }
    false
}

/// Initialize curated marketplace from embedded or provided ZIP bytes into home.
pub fn initialize_curated_marketplace_from_bytes(
    home: &Path,
    zip_bytes: &[u8],
) -> Result<MarketplaceResult, AppError> {
    let root = marketplace_root(home);
    let staging = home
        .join("plugins")
        .join("marketplaces")
        .join(format!(".{MARKETPLACE_ID}.staging"));
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    extract_zip_hardened(zip_bytes, &staging)?;

    // Prefer nested openai-curated-remote dir if present
    let source_dir = {
        let nested = staging.join(MARKETPLACE_ID);
        if nested.is_dir() {
            nested
        } else {
            staging.clone()
        }
    };

    // Validate minimal marketplace shape: at least one file
    let has_files = walk_has_file(&source_dir);
    if !has_files {
        let _ = fs::remove_dir_all(&staging);
        return Err(AppError::InvalidInput(
            "marketplace ZIP has no usable content".into(),
        ));
    }

    if root.exists() {
        let backup = home
            .join("plugins")
            .join("marketplaces")
            .join(format!(".{MARKETPLACE_ID}.bak"));
        let _ = fs::remove_dir_all(&backup);
        fs::rename(&root, &backup).map_err(|e| AppError::io(&root, e))?;
    }
    if let Some(parent) = root.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    // Move validated content into place
    if source_dir == staging {
        fs::rename(&staging, &root).map_err(|e| AppError::io(&root, e))?;
    } else {
        fs::rename(&source_dir, &root).map_err(|e| AppError::io(&root, e))?;
        let _ = fs::remove_dir_all(&staging);
    }

    let config_path = home.join("config.toml");
    ensure_marketplace_in_config(&config_path, &root)?;

    Ok(MarketplaceResult {
        initialized: true,
        configured: true,
        marketplace_root: Some(root.display().to_string()),
    })
}

fn walk_has_file(dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_file() {
            return true;
        }
        if p.is_dir() && walk_has_file(&p) {
            return true;
        }
    }
    false
}

/// Load embedded curated ZIP if present; otherwise create a minimal fixture marketplace.
pub async fn initialize_curated_marketplace(home: &Path) -> Result<MarketplaceResult, AppError> {
    // Prefer bundled resource if present next to resources
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/codex-workbench/openai-curated-remote.zip"),
        PathBuf::from("src-tauri/resources/codex-workbench/openai-curated-remote.zip"),
    ];
    for c in &candidates {
        if c.exists() {
            let bytes = fs::read(c).map_err(|e| AppError::io(c, e))?;
            return initialize_curated_marketplace_from_bytes(home, &bytes);
        }
    }
    // Build a minimal in-memory marketplace ZIP for first-run / tests
    let bytes = build_minimal_marketplace_zip("1.0.0")?;
    initialize_curated_marketplace_from_bytes(home, &bytes)
}

fn build_minimal_marketplace_zip(version: &str) -> Result<Vec<u8>, AppError> {
    use std::io::Cursor;
    let buf = Cursor::new(Vec::new());
    let mut zipw = zip::ZipWriter::new(buf);
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    // marketplace root files
    zipw.start_file("openai-curated-remote/marketplace.json", opts)
        .map_err(|e| AppError::Message(format!("zip write: {e}")))?;
    let manifest = format!(
        r#"{{"id":"{id}","name":"OpenAI Curated","version":"{ver}"}}"#,
        id = MARKETPLACE_ID,
        ver = version
    );
    zipw.write_all(manifest.as_bytes())
        .map_err(|e| AppError::Message(format!("zip write: {e}")))?;
    zipw.start_file(
        "openai-curated-remote/plugins/demo-plugin/plugin.json",
        opts,
    )
    .map_err(|e| AppError::Message(format!("zip write: {e}")))?;
    let plugin = format!(
        r#"{{"id":"demo-plugin","name":"Demo","version":"{ver}"}}"#,
        ver = version
    );
    zipw.write_all(plugin.as_bytes())
        .map_err(|e| AppError::Message(format!("zip write: {e}")))?;
    zipw.start_file("openai-curated-remote/plugins/demo-plugin/VERSION", opts)
        .map_err(|e| AppError::Message(format!("zip write: {e}")))?;
    zipw.write_all(version.as_bytes())
        .map_err(|e| AppError::Message(format!("zip write: {e}")))?;
    let finished = zipw
        .finish()
        .map_err(|e| AppError::Message(format!("zip finish: {e}")))?;
    Ok(finished.into_inner())
}

pub fn list_plugin_caches(home: &Path) -> Result<Vec<PluginCacheInfo>, AppError> {
    let mut out = Vec::new();
    let source_plugins = marketplace_root(home).join("plugins");
    let cache_root = plugin_cache_root(home);

    // From marketplace sources
    if source_plugins.is_dir() {
        for ent in fs::read_dir(&source_plugins)
            .map_err(|e| AppError::io(&source_plugins, e))?
            .flatten()
        {
            if !ent.path().is_dir() {
                continue;
            }
            let id = ent.file_name().to_string_lossy().to_string();
            out.push(inspect_plugin(home, &id)?);
        }
    }
    // Also from cache-only plugins
    if cache_root.is_dir() {
        for ent in fs::read_dir(&cache_root)
            .map_err(|e| AppError::io(&cache_root, e))?
            .flatten()
        {
            if !ent.path().is_dir() {
                continue;
            }
            let id = ent.file_name().to_string_lossy().to_string();
            if !out.iter().any(|p| p.id == id) {
                out.push(inspect_plugin(home, &id)?);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub fn inspect_plugin(home: &Path, plugin_id: &str) -> Result<PluginCacheInfo, AppError> {
    validate_plugin_id(plugin_id)?;
    let source_dir = marketplace_root(home).join("plugins").join(plugin_id);
    let source_version = if source_dir.is_dir() {
        read_version_file(&source_dir)
    } else {
        None
    };

    let cache_dir = plugin_cache_root(home).join(plugin_id);
    let mut cached_versions = Vec::new();
    if cache_dir.is_dir() {
        for ent in fs::read_dir(&cache_dir)
            .map_err(|e| AppError::io(&cache_dir, e))?
            .flatten()
        {
            if ent.path().is_dir() {
                let name = ent.file_name().to_string_lossy().to_string();
                cached_versions.push(name);
            }
        }
        cached_versions.sort();
    }
    let current_version = cached_versions.last().cloned();

    let (can_refresh, refresh_reason) = match (&source_version, &current_version) {
        (Some(src), Some(cur)) if version_lt(src, cur) => {
            (false, format!("拒绝降级：源版本 {src} 低于已缓存 {cur}"))
        }
        (Some(src), Some(cur)) if src == cur => (false, "已是最新缓存版本".into()),
        (Some(_), _) => (true, "可刷新到源版本".into()),
        (None, _) => (false, "市场中无此插件源".into()),
    };

    Ok(PluginCacheInfo {
        id: plugin_id.to_string(),
        marketplace: MARKETPLACE_ID.to_string(),
        source_version,
        current_version,
        cached_versions,
        can_refresh,
        refresh_reason,
    })
}

fn validate_plugin_id(id: &str) -> Result<(), AppError> {
    if id.is_empty()
        || id.contains("..")
        || id.contains('/')
        || id.contains('\\')
        || id.contains(':')
    {
        return Err(AppError::InvalidInput(format!("invalid plugin id: {id}")));
    }
    Ok(())
}

/// Copy source plugin into versioned cache; block downgrades.
pub fn refresh_plugin_cache(home: &Path, plugin_id: &str) -> Result<PluginCacheInfo, AppError> {
    validate_plugin_id(plugin_id)?;
    let info = inspect_plugin(home, plugin_id)?;
    if !info.can_refresh {
        // Explicit downgrade message for tests
        if info.refresh_reason.contains("降级") {
            return Err(AppError::InvalidInput(info.refresh_reason));
        }
        // already latest — return current info without error
        if info.refresh_reason.contains("最新") {
            return Ok(info);
        }
        return Err(AppError::InvalidInput(info.refresh_reason));
    }
    let src_ver = info
        .source_version
        .clone()
        .ok_or_else(|| AppError::InvalidInput("no source version".into()))?;
    let source_dir = marketplace_root(home).join("plugins").join(plugin_id);
    if !source_dir.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "source plugin missing: {plugin_id}"
        )));
    }
    let dest = plugin_cache_root(home).join(plugin_id).join(&src_ver);
    if dest.exists() {
        let _ = fs::remove_dir_all(&dest);
    }
    copy_dir_recursive(&source_dir, &dest)?;
    inspect_plugin(home, plugin_id)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dest).map_err(|e| AppError::io(dest, e))?;
    for ent in fs::read_dir(src)
        .map_err(|e| AppError::io(src, e))?
        .flatten()
    {
        let from = ent.path();
        let to = dest.join(ent.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| AppError::io(&to, e))?;
        }
    }
    Ok(())
}

pub fn marketplace_status(home: &Path) -> MarketplaceResult {
    let root = marketplace_root(home);
    let initialized = root.is_dir() && walk_has_file(&root);
    let config_path = home.join("config.toml");
    let configured = if config_path.exists() {
        fs::read_to_string(&config_path)
            .map(|s| s.contains(MARKETPLACE_ID))
            .unwrap_or(false)
    } else {
        false
    };
    MarketplaceResult {
        initialized,
        configured,
        marketplace_root: if initialized {
            Some(root.display().to_string())
        } else {
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_home_priority_is_override_then_env_then_default() {
        let sources = HomeSources {
            override_dir: Some(PathBuf::from("C:/override")),
            env_codex_home: Some(PathBuf::from("C:/env-codex")),
            default_home: PathBuf::from("C:/default"),
        };
        assert_eq!(
            effective_codex_home_with(&sources),
            PathBuf::from("C:/override")
        );

        let sources = HomeSources {
            override_dir: None,
            env_codex_home: Some(PathBuf::from("C:/env-codex")),
            default_home: PathBuf::from("C:/default"),
        };
        assert_eq!(
            effective_codex_home_with(&sources),
            PathBuf::from("C:/env-codex")
        );

        let sources = HomeSources {
            override_dir: None,
            env_codex_home: None,
            default_home: PathBuf::from("C:/default"),
        };
        assert_eq!(
            effective_codex_home_with(&sources),
            PathBuf::from("C:/default")
        );
    }

    #[test]
    fn zip_extract_rejects_path_traversal() {
        use std::io::Cursor;
        let buf = Cursor::new(Vec::new());
        let mut zipw = zip::ZipWriter::new(buf);
        let opts = zip::write::SimpleFileOptions::default();
        zipw.start_file("../evil.txt", opts).unwrap();
        zipw.write_all(b"x").unwrap();
        let data = zipw.finish().unwrap().into_inner();
        let tmp = tempfile::tempdir().unwrap();
        let err = extract_zip_hardened(&data, tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("unsafe") || err.to_string().contains("rejected"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_refresh_blocks_version_downgrade() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        // source 1.9.0
        let src = marketplace_root(home).join("plugins").join("demo");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("VERSION"), "1.9.0").unwrap();
        // cache already 2.0.0
        let cache = plugin_cache_root(home).join("demo").join("2.0.0");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("VERSION"), "2.0.0").unwrap();

        let err = refresh_plugin_cache(home, "demo").unwrap_err();
        assert!(err.to_string().contains("降级"), "expected 降级 in {err}");
    }

    #[test]
    fn initialize_and_refresh_upgrade_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let bytes = build_minimal_marketplace_zip("1.0.0").unwrap();
        let r = initialize_curated_marketplace_from_bytes(home, &bytes).unwrap();
        assert!(r.initialized);
        assert!(r.configured);
        let info = refresh_plugin_cache(home, "demo-plugin").unwrap();
        assert_eq!(info.current_version.as_deref(), Some("1.0.0"));
        assert!(info.cached_versions.contains(&"1.0.0".into()));
    }

    #[test]
    fn config_merge_preserves_unrelated_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.toml");
        fs::write(&config, "model = \"gpt-5\"\n[features]\nfoo = true\n").unwrap();
        let mp = tmp.path().join("mp");
        fs::create_dir_all(&mp).unwrap();
        ensure_marketplace_in_config(&config, &mp).unwrap();
        let text = fs::read_to_string(&config).unwrap();
        assert!(text.contains("gpt-5"));
        assert!(text.contains("foo"));
        assert!(text.contains(MARKETPLACE_ID));
    }
}

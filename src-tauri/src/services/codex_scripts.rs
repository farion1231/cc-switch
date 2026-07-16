//! Local Codex user-script inventory + explicit market install.
//! Storage: `get_app_config_dir()/codex-workbench/scripts`
//! Never auto-fetches from a timer; all market ops are user-initiated.

use crate::config::{atomic_write, get_app_config_dir};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_SCRIPT_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB
const MARKET_TIMEOUT: Duration = Duration::from_secs(15);
const META_FILE: &str = "scripts_meta.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserScriptInfo {
    pub key: String,
    pub name: String,
    pub source: String, // "user" | "market" | "builtin"
    pub enabled: bool,
    pub version: Option<String>,
    pub sha256: String,
    pub runtime_state: String, // "idle" | "loaded" | "failed" | "disabled"
    pub runtime_error: Option<String>,
    #[serde(default)]
    pub verification: Option<String>, // "ok" | "unavailable" | "mismatch"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptInstallRequest {
    pub market_id: String,
    pub expected_version: String,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ScriptsMeta {
    /// key -> enabled
    #[serde(default)]
    enabled: HashMap<String, bool>,
    /// key -> version
    #[serde(default)]
    versions: HashMap<String, String>,
    /// key -> source
    #[serde(default)]
    sources: HashMap<String, String>,
    /// key -> last runtime error (UI only, not durable requirement)
    #[serde(default)]
    runtime_errors: HashMap<String, String>,
    /// last market index cache
    #[serde(default)]
    market_cache: Option<MarketIndex>,
    #[serde(default)]
    market_cache_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketScriptEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketIndex {
    #[serde(default)]
    pub scripts: Vec<MarketScriptEntry>,
}

fn scripts_root() -> PathBuf {
    get_app_config_dir().join("codex-workbench").join("scripts")
}

fn meta_path() -> PathBuf {
    scripts_root().join(META_FILE)
}

fn ensure_root() -> Result<PathBuf, AppError> {
    let root = scripts_root();
    fs::create_dir_all(&root).map_err(|e| AppError::io(&root, e))?;
    Ok(root)
}

fn load_meta() -> ScriptsMeta {
    let path = meta_path();
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => ScriptsMeta::default(),
    }
}

fn save_meta(meta: &ScriptsMeta) -> Result<(), AppError> {
    ensure_root()?;
    let path = meta_path();
    let bytes = serde_json::to_vec_pretty(meta).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&path, &bytes)
}

/// Reject path traversal and non-simple market ids.
pub fn sanitize_market_id(id: &str) -> Result<String, AppError> {
    let id = id.trim();
    if id.is_empty() {
        return Err(AppError::InvalidInput("script id is empty".into()));
    }
    if id.contains("..")
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
        || id.starts_with('.')
    {
        return Err(AppError::InvalidInput(format!(
            "invalid script id (path traversal rejected): {id}"
        )));
    }
    // Allow alnum, dash, underscore, dot (not leading)
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(AppError::InvalidInput(format!(
            "invalid script id characters: {id}"
        )));
    }
    Ok(id.to_string())
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn is_safe_js_under(root: &Path, path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("js") {
        return false;
    }
    let Ok(root_c) = root.canonicalize() else {
        return false;
    };
    let Ok(path_c) = path.canonicalize() else {
        return false;
    };
    path_c.starts_with(&root_c) && path_c.is_file()
}

fn script_file_path(key: &str) -> Result<PathBuf, AppError> {
    let safe = sanitize_market_id(key)?;
    Ok(scripts_root().join(format!("{safe}.js")))
}

/// List installed scripts under the user scripts root.
pub fn list_scripts() -> Result<Vec<UserScriptInfo>, AppError> {
    let root = ensure_root()?;
    let meta = load_meta();
    let mut out = Vec::new();

    let rd = match fs::read_dir(&root) {
        Ok(rd) => rd,
        Err(e) => return Err(AppError::io(&root, e)),
    };

    for entry in rd.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some(META_FILE) {
            continue;
        }
        if !is_safe_js_under(&root, &path) {
            continue;
        }
        let key = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if sanitize_market_id(&key).is_err() {
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| AppError::io(&path, e))?;
        let hash = sha256_hex(&bytes);
        let enabled = *meta.enabled.get(&key).unwrap_or(&true);
        let source = meta
            .sources
            .get(&key)
            .cloned()
            .unwrap_or_else(|| "user".into());
        let version = meta.versions.get(&key).cloned();
        let runtime_error = meta.runtime_errors.get(&key).cloned();
        let runtime_state = if !enabled {
            "disabled".into()
        } else if runtime_error.is_some() {
            "failed".into()
        } else {
            "idle".into()
        };
        out.push(UserScriptInfo {
            key: key.clone(),
            name: key,
            source,
            enabled,
            version,
            sha256: hash,
            runtime_state,
            runtime_error,
            verification: Some("ok".into()),
        });
    }

    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// Import a local .js file into the scripts root (copy + enable).
pub fn import_local_script(source_path: &Path, key: Option<&str>) -> Result<UserScriptInfo, AppError> {
    ensure_root()?;
    if !source_path.is_file() {
        return Err(AppError::InvalidInput(format!(
            "not a file: {}",
            source_path.display()
        )));
    }
    let stem = key
        .map(|s| s.to_string())
        .or_else(|| {
            source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "imported".into());
    let safe = sanitize_market_id(&stem)?;
    let bytes = fs::read(source_path).map_err(|e| AppError::io(source_path, e))?;
    if bytes.len() as u64 > MAX_SCRIPT_BYTES {
        return Err(AppError::InvalidInput(format!(
            "script exceeds {MAX_SCRIPT_BYTES} bytes"
        )));
    }
    let dest = script_file_path(&safe)?;
    atomic_write(&dest, &bytes)?;
    let mut meta = load_meta();
    meta.enabled.insert(safe.clone(), true);
    meta.sources.insert(safe.clone(), "user".into());
    save_meta(&meta)?;
    let hash = sha256_hex(&bytes);
    Ok(UserScriptInfo {
        key: safe.clone(),
        name: safe,
        source: "user".into(),
        enabled: true,
        version: None,
        sha256: hash,
        runtime_state: "idle".into(),
        runtime_error: None,
        verification: Some("ok".into()),
    })
}

pub fn set_script_enabled(key: &str, enabled: bool) -> Result<(), AppError> {
    let safe = sanitize_market_id(key)?;
    let path = script_file_path(&safe)?;
    if !path.is_file() {
        return Err(AppError::InvalidInput(format!("script not found: {safe}")));
    }
    let mut meta = load_meta();
    meta.enabled.insert(safe, enabled);
    save_meta(&meta)
}

pub fn delete_user_script(key: &str) -> Result<(), AppError> {
    let safe = sanitize_market_id(key)?;
    let mut meta = load_meta();
    let source = meta.sources.get(&safe).map(|s| s.as_str()).unwrap_or("user");
    if source == "builtin" {
        return Err(AppError::InvalidInput(
            "cannot delete builtin scripts".into(),
        ));
    }
    let path = script_file_path(&safe)?;
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| AppError::io(&path, e))?;
    }
    meta.enabled.remove(&safe);
    meta.versions.remove(&safe);
    meta.sources.remove(&safe);
    meta.runtime_errors.remove(&safe);
    save_meta(&meta)
}

pub fn scripts_dir_path() -> Result<PathBuf, AppError> {
    ensure_root()
}

/// Install/update from raw bytes with optional mandatory sha256.
pub fn install_bytes(
    key: &str,
    code: &[u8],
    version: Option<&str>,
    expected_sha256: Option<&str>,
    source: &str,
) -> Result<UserScriptInfo, AppError> {
    let safe = sanitize_market_id(key)?;
    if code.len() as u64 > MAX_SCRIPT_BYTES {
        return Err(AppError::InvalidInput(format!(
            "script exceeds {MAX_SCRIPT_BYTES} bytes"
        )));
    }
    let actual = sha256_hex(code);
    if let Some(expected) = expected_sha256 {
        let exp = expected.trim().to_lowercase();
        if !exp.is_empty() && exp != actual {
            return Err(AppError::Message(format!(
                "SHA-256 mismatch for script {safe}: expected {exp}, got {actual}"
            )));
        }
    }
    ensure_root()?;
    let dest = script_file_path(&safe)?;
    // atomic write preserves old file on failure (temp+rename)
    atomic_write(&dest, code)?;
    let mut meta = load_meta();
    meta.enabled.insert(safe.clone(), true);
    meta.sources.insert(safe.clone(), source.to_string());
    if let Some(v) = version {
        meta.versions.insert(safe.clone(), v.to_string());
    }
    meta.runtime_errors.remove(&safe);
    save_meta(&meta)?;
    let verification = match expected_sha256 {
        Some(s) if !s.trim().is_empty() => Some("ok".into()),
        Some(_) => Some("unavailable".into()),
        None => Some("unavailable".into()),
    };
    Ok(UserScriptInfo {
        key: safe.clone(),
        name: safe,
        source: source.into(),
        enabled: true,
        version: version.map(|s| s.to_string()),
        sha256: actual,
        runtime_state: "idle".into(),
        runtime_error: None,
        verification,
    })
}

/// Refresh market index (user-initiated only).
pub async fn refresh_market(market_url: &str) -> Result<MarketIndex, AppError> {
    if !(market_url.starts_with("https://") || market_url.starts_with("http://127.0.0.1")
        || market_url.starts_with("http://localhost"))
    {
        return Err(AppError::InvalidInput(
            "market URL must be HTTPS (or localhost)".into(),
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(MARKET_TIMEOUT)
        .build()
        .map_err(|e| AppError::Message(format!("http client: {e}")))?;
    let resp = client
        .get(market_url)
        .send()
        .await
        .map_err(|e| AppError::Message(format!("market fetch failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::HttpStatus {
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        });
    }
    let index: MarketIndex = resp
        .json()
        .await
        .map_err(|e| AppError::Message(format!("market JSON parse failed: {e}")))?;
    let mut meta = load_meta();
    meta.market_cache = Some(index.clone());
    meta.market_cache_at = Some(chrono_like_now());
    save_meta(&meta)?;
    Ok(index)
}

fn chrono_like_now() -> String {
    // Avoid chrono dependency; RFC3339-ish via system time is enough for cache stamp.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn cached_market() -> Option<MarketIndex> {
    load_meta().market_cache
}

/// Install a market entry by id (explicit user action).
pub async fn install_from_market(
    market_url: &str,
    req: &ScriptInstallRequest,
) -> Result<UserScriptInfo, AppError> {
    let safe = sanitize_market_id(&req.market_id)?;
    let index = match cached_market() {
        Some(i) => i,
        None => refresh_market(market_url).await?,
    };
    let entry = index
        .scripts
        .iter()
        .find(|s| s.id == safe)
        .ok_or_else(|| AppError::InvalidInput(format!("market script not found: {safe}")))?
        .clone();

    if !req.expected_version.is_empty() && entry.version != req.expected_version {
        return Err(AppError::InvalidInput(format!(
            "version mismatch: market has {}, expected {}",
            entry.version, req.expected_version
        )));
    }

    if !(entry.url.starts_with("https://")
        || entry.url.starts_with("http://127.0.0.1")
        || entry.url.starts_with("http://localhost"))
    {
        return Err(AppError::InvalidInput(
            "script URL must be HTTPS (or localhost)".into(),
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(MARKET_TIMEOUT)
        .build()
        .map_err(|e| AppError::Message(format!("http client: {e}")))?;
    let resp = client
        .get(&entry.url)
        .send()
        .await
        .map_err(|e| AppError::Message(format!("script download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::HttpStatus {
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        });
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Message(format!("script body read failed: {e}")))?;
    if bytes.len() as u64 > MAX_SCRIPT_BYTES {
        return Err(AppError::InvalidInput(format!(
            "script exceeds {MAX_SCRIPT_BYTES} bytes"
        )));
    }

    let expected = req
        .expected_sha256
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if entry.sha256.trim().is_empty() {
                None
            } else {
                Some(entry.sha256.as_str())
            }
        });

    install_bytes(
        &safe,
        &bytes,
        Some(&entry.version),
        expected,
        "market",
    )
}

/// Build JS snippet that wraps each enabled script in try/catch and reports status.
pub fn build_user_scripts_snippet() -> Result<String, AppError> {
    let scripts = list_scripts()?;
    let mut parts = Vec::new();
    parts.push(
        r#"(function(){
  var status = {};
  window.__ccSwitchCodexUserScripts = status;
"#
        .to_string(),
    );

    let root = scripts_root();
    for info in scripts.iter().filter(|s| s.enabled) {
        let path = root.join(format!("{}.js", info.key));
        if !is_safe_js_under(&root, &path) {
            continue;
        }
        let code = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Escape for embedding inside a JS string used only as Function body via try/catch block.
        // We inject as a raw IIFE body with try/catch — do not use eval of untrusted without isolation.
        // Scripts are user/market installed by explicit action.
        let key_js = serde_json::to_string(&info.key).unwrap_or_else(|_| "\"?\"".into());
        parts.push(format!(
            r#"  try {{
    status[{key}] = {{ state: "loaded", error: null }};
    (function(){{
{code}
    }})();
  }} catch (err) {{
    status[{key}] = {{ state: "failed", error: String(err && err.message || err) }};
  }}
"#,
            key = key_js,
            code = code
        ));
    }
    parts.push("})();\n".into());
    Ok(parts.join(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that touch config dir override
    static LOCK: Mutex<()> = Mutex::new(());

    struct ScriptFixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
    }

    impl ScriptFixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().expect("tempdir");
            // Point app config override if available
            crate::app_store::set_app_config_dir_override_for_test(Some(tmp.path().to_path_buf()));
            let root = scripts_root();
            fs::create_dir_all(&root).unwrap();
            Self {
                _tmp: tmp,
                root,
            }
        }

        fn with_installed(key: &str, code: &str) -> Result<Self, AppError> {
            let f = Self::new();
            install_bytes(key, code.as_bytes(), Some("1.0.0"), None, "user")?;
            Ok(f)
        }

        fn install_with_hash(
            &self,
            key: &str,
            code: &str,
            wrong_hash: &str,
        ) -> Result<UserScriptInfo, AppError> {
            install_bytes(key, code.as_bytes(), Some("2.0.0"), Some(wrong_hash), "market")
        }

        fn contents(&self, key: &str) -> Result<String, AppError> {
            let p = self.root.join(format!("{key}.js"));
            fs::read_to_string(&p).map_err(|e| AppError::io(&p, e))
        }
    }

    impl Drop for ScriptFixture {
        fn drop(&mut self) {
            crate::app_store::set_app_config_dir_override_for_test(None);
        }
    }

    #[test]
    fn market_id_cannot_escape_script_root() {
        let _g = LOCK.lock().unwrap();
        assert!(sanitize_market_id("../../outside").is_err());
        assert!(sanitize_market_id("a/b").is_err());
        assert!(sanitize_market_id("ok-script_1").is_ok());
    }

    #[test]
    fn market_script_install_is_atomic_and_preserves_old_version() -> Result<(), AppError> {
        let _g = LOCK.lock().unwrap();
        let fixture = ScriptFixture::with_installed("demo", "old code")?;
        let error = fixture
            .install_with_hash("demo", "new code", "wrong-hash")
            .unwrap_err();
        assert!(
            error.to_string().contains("SHA-256"),
            "error was: {error}"
        );
        assert_eq!(fixture.contents("demo")?, "old code");
        Ok(())
    }

    #[test]
    fn install_and_list_roundtrip() -> Result<(), AppError> {
        let _g = LOCK.lock().unwrap();
        let _f = ScriptFixture::new();
        install_bytes("hello", b"console.log(1)", Some("0.1.0"), None, "user")?;
        let list = list_scripts()?;
        assert!(list.iter().any(|s| s.key == "hello" && s.enabled));
        set_script_enabled("hello", false)?;
        let list = list_scripts()?;
        assert!(list.iter().any(|s| s.key == "hello" && !s.enabled));
        Ok(())
    }
}

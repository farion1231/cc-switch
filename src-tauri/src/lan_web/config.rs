//! Device-local configuration and cryptographic token/session helpers for the
//! opt-in LAN Mobile Web UI.
//!
//! Stored in `~/.cc-switch/lan-web.json` (mode `0600` on Unix) — intentionally
//! separate from SQLite (`cc-switch.db`) so it never touches `SCHEMA_VERSION`
//! and is never carried to other machines by WebDAV/S3 cloud sync or backups.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use subtle::ConstantTimeEq;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

pub const DEFAULT_LAN_WEB_PORT: u16 = 15722;
pub const MIN_LAN_WEB_PORT: u16 = 1024;
pub const SESSION_COOKIE_NAME: &str = "ccs_lan_session";
pub const SESSION_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60; // 30 days
const SESSION_SIGNING_CONTEXT: &[u8] = b"cc-switch-lan-web-session-v1";
const LAN_WEB_CONFIG_FILENAME: &str = "lan-web.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanWebMode {
    #[default]
    Off,
    Local,
    Lan,
}

impl LanWebMode {
    pub fn bind_host(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Local => Some("127.0.0.1"),
            Self::Lan => Some("0.0.0.0"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanWebConfig {
    #[serde(default)]
    pub mode: LanWebMode,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub allow_control: bool,
    #[serde(default)]
    pub remote_control_apps: Vec<String>,
    #[serde(default)]
    pub token: String,
}

fn default_port() -> u16 {
    DEFAULT_LAN_WEB_PORT
}

impl Default for LanWebConfig {
    fn default() -> Self {
        Self {
            mode: LanWebMode::Off,
            port: DEFAULT_LAN_WEB_PORT,
            allow_control: false,
            remote_control_apps: Vec::new(),
            token: String::new(),
        }
    }
}

impl LanWebConfig {
    pub fn effective_port(&self) -> u16 {
        if self.port < MIN_LAN_WEB_PORT {
            DEFAULT_LAN_WEB_PORT
        } else {
            self.port
        }
    }
}

/// Generate a 256-bit cryptographically random hex token using OS entropy via UUIDv4.
pub fn generate_token() -> String {
    let mut hasher = Sha256::new();
    for _ in 0..4 {
        hasher.update(Uuid::new_v4().as_bytes());
    }
    hex_encode(&hasher.finalize())
}

/// Derive the expected `HttpOnly` session cookie value from the current access token.
/// Rotating the access token immediately invalidates all previously issued cookies.
pub fn derive_session_cookie(token: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(token.as_bytes()).expect("HMAC accepts keys of any size");
    mac.update(SESSION_SIGNING_CONTEXT);
    hex_encode(&mac.finalize().into_bytes())
}

/// Constant-time string comparison (returns false if either string is empty).
pub fn constant_time_eq_non_empty(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Verify an incoming access token against the active configured token.
pub fn verify_access_token(candidate: &str, expected_token: &str) -> bool {
    constant_time_eq_non_empty(candidate.trim(), expected_token)
}

/// Verify a session cookie value against the active configured token.
pub fn verify_session_cookie(candidate: &str, active_token: &str) -> bool {
    if active_token.is_empty() {
        return false;
    }
    let expected = derive_session_cookie(active_token);
    constant_time_eq_non_empty(candidate.trim(), &expected)
}

/// Validate a port choice for the LAN web control plane.
pub fn validate_port(port: u16, proxy_port: Option<u16>) -> Result<(), String> {
    if port < MIN_LAN_WEB_PORT {
        return Err(format!(
            "Port must be between {MIN_LAN_WEB_PORT} and 65535 (got {port})."
        ));
    }
    if let Some(pp) = proxy_port {
        if port == pp {
            return Err(format!(
                "Port {port} is reserved by the local proxy server; please choose a different port."
            ));
        }
    }
    Ok(())
}

pub fn config_path() -> PathBuf {
    crate::config::get_app_config_dir().join(LAN_WEB_CONFIG_FILENAME)
}

pub fn load_config() -> Result<LanWebConfig, String> {
    load_config_from(&config_path())
}

pub fn load_config_from(path: &std::path::Path) -> Result<LanWebConfig, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<LanWebConfig>(&raw)
            .map_err(|e| format!("Failed to parse {}: {e}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(LanWebConfig::default()),
        Err(err) => Err(format!("Failed to read {}: {err}", path.display())),
    }
}

pub fn save_config(cfg: &LanWebConfig) -> Result<(), String> {
    save_config_to(&config_path(), cfg)
}

pub fn save_config_to(path: &std::path::Path, cfg: &LanWebConfig) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("Failed to serialize LAN web config: {e}"))?;
    json.push('\n');

    crate::config::atomic_write_private(path, json.as_bytes()).map_err(|e| e.to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn token_generation_and_session_derivation_work() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert_eq!(t1.len(), 64);
        assert_ne!(t1, t2);
        assert!(verify_access_token(&t1, &t1));
        assert!(!verify_access_token(&t2, &t1));
        assert!(!verify_access_token("", &t1));

        let cookie = derive_session_cookie(&t1);
        assert_eq!(cookie.len(), 64);
        assert_ne!(cookie, t1);
        assert!(verify_session_cookie(&cookie, &t1));
        // Rotating the token invalidates previous cookies immediately.
        assert!(!verify_session_cookie(&cookie, &t2));
    }

    #[test]
    fn validate_port_rejects_low_and_proxy_collision() {
        assert!(validate_port(80, Some(15721)).is_err());
        assert!(validate_port(1023, Some(15721)).is_err());
        assert!(validate_port(15721, Some(15721)).is_err());
        assert!(validate_port(15722, Some(15721)).is_ok());
        assert!(validate_port(65535, None).is_ok());
    }

    #[test]
    fn config_roundtrip_sets_private_permissions() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lan-web.json");
        let cfg = LanWebConfig {
            mode: LanWebMode::Lan,
            port: 15800,
            allow_control: true,
            remote_control_apps: vec!["claude".to_string(), "agy".to_string()],
            token: generate_token(),
        };
        save_config_to(&path, &cfg).unwrap();
        let loaded = load_config_from(&path).unwrap();
        assert_eq!(loaded, cfg);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}

//! Storage adapter for Usage Sync.
//!
//! Reuses the connection parameters and HTTP primitives of the existing
//! WebDAV / S3 configuration sync; only the namespace differs
//! (`{remote_root}/usage-v1/{profile}/` instead of `{remote_root}/v2/db-vN/{profile}/`).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::error::AppError;
use crate::services::s3::{self, S3Credentials};
use crate::services::webdav::{self, WebDavAuth};
use crate::settings::{S3SyncSettings, UsageSyncBackend, WebDavSyncSettings};

use super::protocol::{is_valid_device_id, DEVICES_DIR, USAGE_NAMESPACE};

pub(crate) enum UsageTransport {
    WebDav {
        base_url: String,
        auth: WebDavAuth,
        /// `remote_root` + `usage-v1` + `profile` segments.
        root: Vec<String>,
    },
    S3 {
        creds: S3Credentials,
        /// `remote_root/usage-v1/profile` (no leading/trailing slash).
        prefix: String,
    },
    #[cfg(test)]
    Memory(std::sync::Arc<memory::MemoryStore>),
}

fn namespace_segments(remote_root: &str, profile: &str) -> Vec<String> {
    let mut segments: Vec<String> = webdav::path_segments(remote_root)
        .map(str::to_string)
        .collect();
    segments.push(USAGE_NAMESPACE.to_string());
    segments.extend(webdav::path_segments(profile).map(str::to_string));
    segments
}

/// WebDAV directories already created in this process (MKCOL is not free).
fn ensured_dirs() -> &'static Mutex<HashSet<String>> {
    static DIRS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    DIRS.get_or_init(|| Mutex::new(HashSet::new()))
}

impl UsageTransport {
    pub(crate) fn from_webdav(settings: &WebDavSyncSettings) -> Result<Self, AppError> {
        settings.validate()?;
        webdav::parse_base_url(&settings.base_url)?;
        Ok(Self::WebDav {
            base_url: settings.base_url.clone(),
            auth: webdav::auth_from_credentials(&settings.username, &settings.password),
            root: namespace_segments(&settings.remote_root, &settings.profile),
        })
    }

    pub(crate) fn from_s3(settings: &S3SyncSettings) -> Result<Self, AppError> {
        settings.validate()?;
        Ok(Self::S3 {
            creds: S3Credentials {
                access_key_id: settings.access_key_id.clone(),
                secret_access_key: settings.secret_access_key.clone(),
                region: settings.region.clone(),
                bucket: settings.bucket.clone(),
                endpoint: settings.endpoint.clone(),
            },
            prefix: namespace_segments(&settings.remote_root, &settings.profile).join("/"),
        })
    }

    /// Resolve the transport for `backend` from the current cloud settings.
    pub(crate) fn resolve(backend: UsageSyncBackend) -> Result<Self, AppError> {
        match backend {
            UsageSyncBackend::Webdav => {
                let settings = crate::settings::get_webdav_sync_settings().ok_or_else(|| {
                    AppError::localized(
                        "usage_sync.webdav_not_configured",
                        "尚未配置 WebDAV，无法同步用量",
                        "WebDAV is not configured; usage cannot be synced.",
                    )
                })?;
                Self::from_webdav(&settings)
            }
            UsageSyncBackend::S3 => {
                let settings = crate::settings::get_s3_sync_settings().ok_or_else(|| {
                    AppError::localized(
                        "usage_sync.s3_not_configured",
                        "尚未配置 S3，无法同步用量",
                        "S3 is not configured; usage cannot be synced.",
                    )
                })?;
                Self::from_s3(&settings)
            }
        }
    }

    /// Stable identifier of the remote namespace this transport reads and
    /// writes: backend, server location (scheme, host, port, path; never
    /// user info, credentials or query), bucket and the namespace path
    /// (remote root, `usage-v1`, profile). Hashed, so it can be stored and
    /// compared without keeping the location itself.
    pub(crate) fn namespace_key(&self) -> String {
        fn location(raw: &str) -> String {
            match url::Url::parse(raw.trim()) {
                Ok(url) => format!(
                    "{}://{}:{}{}",
                    url.scheme(),
                    url.host_str().unwrap_or_default().to_ascii_lowercase(),
                    url.port_or_known_default().unwrap_or_default(),
                    url.path().trim_end_matches('/')
                ),
                Err(_) => raw.trim().trim_end_matches('/').to_string(),
            }
        }
        let raw = match self {
            Self::WebDav { base_url, root, .. } => {
                format!("webdav\n{}\n{}", location(base_url), root.join("/"))
            }
            Self::S3 { creds, prefix } => format!(
                "s3\n{}\n{}\n{}\n{}",
                location(&creds.endpoint),
                creds.region.trim(),
                creds.bucket.trim(),
                prefix
            ),
            #[cfg(test)]
            Self::Memory(store) => format!("memory\n{:p}", std::sync::Arc::as_ptr(store)),
        };
        crate::services::sync_protocol::sha256_hex(raw.as_bytes())
    }

    /// Human-readable remote location (no credentials, no query).
    pub(crate) fn display_location(&self) -> String {
        match self {
            Self::WebDav { root, .. } => format!("WebDAV: /{}", root.join("/")),
            Self::S3 { creds, prefix } => format!("S3: {}/{}", creds.bucket, prefix),
            #[cfg(test)]
            Self::Memory(_) => "memory".to_string(),
        }
    }

    /// `false` when credentials and usage data would travel over plain HTTP to
    /// a non-loopback host. Shown as a warning; never logged with credentials.
    pub(crate) fn is_transport_secure(&self) -> bool {
        fn secure(raw: &str) -> bool {
            let Ok(url) = url::Url::parse(raw) else {
                return true;
            };
            if url.scheme() == "https" {
                return true;
            }
            matches!(
                url.host_str(),
                Some("localhost") | Some("127.0.0.1") | Some("::1") | Some("[::1]")
            )
        }
        match self {
            Self::WebDav { base_url, .. } => secure(base_url),
            Self::S3 { creds, .. } => {
                !creds.endpoint.trim_start().starts_with("http://") || secure(&creds.endpoint)
            }
            #[cfg(test)]
            Self::Memory(_) => true,
        }
    }

    /// Relative object paths are built from protocol names only; refuse any
    /// path that could be normalized out of the usage namespace (`.`, `..`,
    /// empty segments, backslashes, control characters) before it reaches a
    /// URL parser or a request signer.
    fn check_rel(rel: &str) -> Result<(), AppError> {
        let bad = rel.is_empty()
            || rel.starts_with('/')
            || rel.contains('\\')
            || rel.chars().any(|c| c.is_control())
            || rel
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..");
        if bad {
            return Err(AppError::Message(format!(
                "refusing unsafe usage sync path {rel:?}"
            )));
        }
        Ok(())
    }

    fn webdav_url(base_url: &str, root: &[String], rel: &str) -> Result<String, AppError> {
        let mut segments = root.to_vec();
        segments.extend(webdav::path_segments(rel).map(str::to_string));
        webdav::build_remote_url(base_url, &segments)
    }

    fn s3_key(prefix: &str, rel: &str) -> String {
        if prefix.is_empty() {
            rel.to_string()
        } else {
            format!("{prefix}/{rel}")
        }
    }

    pub(crate) async fn get(
        &self,
        rel: &str,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, AppError> {
        Self::check_rel(rel)?;
        match self {
            Self::WebDav {
                base_url,
                auth,
                root,
            } => {
                let url = Self::webdav_url(base_url, root, rel)?;
                Ok(webdav::get_bytes(&url, auth, max_bytes)
                    .await?
                    .map(|(bytes, _)| bytes))
            }
            Self::S3 { creds, prefix } => {
                Ok(s3::get_object(creds, &Self::s3_key(prefix, rel), max_bytes)
                    .await?
                    .map(|(bytes, _)| bytes))
            }
            #[cfg(test)]
            Self::Memory(store) => store.get(rel, max_bytes),
        }
    }

    /// Ensure the parent collections of `device_dir` exist (WebDAV only).
    pub(crate) async fn prepare_device_dir(&self, device_dir: &str) -> Result<(), AppError> {
        match self {
            Self::WebDav {
                base_url,
                auth,
                root,
            } => {
                let mut segments = root.clone();
                segments.extend(webdav::path_segments(device_dir).map(str::to_string));
                segments.push(super::protocol::PARTS_DIR.to_string());
                let cache_key = format!("{base_url}|{}", segments.join("/"));
                debug_assert_eq!(
                    Some(&cache_key),
                    self.device_dir_cache_key(device_dir).as_ref()
                );
                if ensured_dirs()
                    .lock()
                    .map(|dirs| dirs.contains(&cache_key))
                    .unwrap_or(false)
                {
                    return Ok(());
                }
                webdav::ensure_remote_directories(base_url, &segments, auth).await?;
                if let Ok(mut dirs) = ensured_dirs().lock() {
                    dirs.insert(cache_key);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(crate) async fn put(
        &self,
        rel: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), AppError> {
        Self::check_rel(rel)?;
        match self {
            Self::WebDav {
                base_url,
                auth,
                root,
            } => {
                let url = Self::webdav_url(base_url, root, rel)?;
                webdav::put_bytes(&url, auth, bytes, content_type).await
            }
            Self::S3 { creds, prefix } => {
                s3::put_object(creds, &Self::s3_key(prefix, rel), bytes, content_type).await
            }
            #[cfg(test)]
            Self::Memory(store) => store.put(rel, bytes),
        }
    }

    /// PUT below a device directory. If the remote directory was removed while
    /// this process cached it as existing (WebDAV MKCOL cache), the first PUT
    /// fails; drop the cache entry, re-create the directory and retry once.
    pub(crate) async fn put_in_device_dir(
        &self,
        device_dir: &str,
        rel: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), AppError> {
        match self {
            Self::WebDav { .. } => {
                self.prepare_device_dir(device_dir).await?;
                match self.put(rel, bytes.clone(), content_type).await {
                    Ok(()) => Ok(()),
                    Err(first) => {
                        log::debug!(
                            "[UsageSync] PUT failed, re-creating remote directory once: {first}"
                        );
                        self.forget_device_dir(device_dir);
                        self.prepare_device_dir(device_dir).await?;
                        self.put(rel, bytes, content_type).await
                    }
                }
            }
            _ => self.put(rel, bytes, content_type).await,
        }
    }

    fn device_dir_cache_key(&self, device_dir: &str) -> Option<String> {
        match self {
            Self::WebDav { base_url, root, .. } => {
                let mut segments = root.clone();
                segments.extend(webdav::path_segments(device_dir).map(str::to_string));
                segments.push(super::protocol::PARTS_DIR.to_string());
                Some(format!("{base_url}|{}", segments.join("/")))
            }
            _ => None,
        }
    }

    pub(crate) fn forget_device_dir(&self, device_dir: &str) {
        if let Some(key) = self.device_dir_cache_key(device_dir) {
            if let Ok(mut dirs) = ensured_dirs().lock() {
                dirs.remove(&key);
            }
        }
    }

    /// Object names (`parts/…`) currently stored below a device directory.
    pub(crate) async fn list_device_parts(&self, device_id: &str) -> Result<Vec<String>, AppError> {
        let parts_rel = format!(
            "{}/{}",
            super::protocol::device_dir(device_id),
            super::protocol::PARTS_DIR
        );
        let names: Vec<String> = match self {
            Self::WebDav {
                base_url,
                auth,
                root,
            } => {
                let url = Self::webdav_url(base_url, root, &parts_rel)?;
                webdav::list_collection(&url, auth)
                    .await?
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|entry| !entry.is_collection)
                    .map(|entry| entry.name)
                    .collect()
            }
            Self::S3 { creds, prefix } => {
                let list_prefix = format!("{}/", Self::s3_key(prefix, &parts_rel));
                s3::list_objects(creds, &list_prefix, None)
                    .await?
                    .keys
                    .into_iter()
                    .filter_map(|key| key.strip_prefix(&list_prefix).map(str::to_string))
                    .filter(|name| !name.contains('/'))
                    .collect()
            }
            #[cfg(test)]
            Self::Memory(store) => {
                let prefix = format!("{parts_rel}/");
                store
                    .keys()
                    .into_iter()
                    .filter_map(|key| key.strip_prefix(&prefix).map(str::to_string))
                    .collect()
            }
        };
        Ok(names
            .into_iter()
            .map(|name| format!("{}/{name}", super::protocol::PARTS_DIR))
            .collect())
    }

    pub(crate) async fn delete(&self, rel: &str) -> Result<(), AppError> {
        Self::check_rel(rel)?;
        match self {
            Self::WebDav {
                base_url,
                auth,
                root,
            } => {
                let url = Self::webdav_url(base_url, root, rel)?;
                webdav::delete_resource(&url, auth).await
            }
            Self::S3 { creds, prefix } => {
                s3::delete_object(creds, &Self::s3_key(prefix, rel)).await
            }
            #[cfg(test)]
            Self::Memory(store) => store.delete(rel),
        }
    }

    /// Discover device directories under `devices/`. Invalid names are ignored.
    pub(crate) async fn list_device_ids(&self) -> Result<Vec<String>, AppError> {
        let mut ids: Vec<String> = match self {
            Self::WebDav {
                base_url,
                auth,
                root,
            } => {
                let url = Self::webdav_url(base_url, root, DEVICES_DIR)?;
                webdav::list_collection(&url, auth)
                    .await?
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|entry| entry.is_collection)
                    .map(|entry| entry.name)
                    .collect()
            }
            Self::S3 { creds, prefix } => {
                let list_prefix = format!("{}/", Self::s3_key(prefix, DEVICES_DIR));
                s3::list_objects(creds, &list_prefix, Some("/"))
                    .await?
                    .common_prefixes
                    .into_iter()
                    .filter_map(|p| {
                        p.strip_prefix(&list_prefix)
                            .map(|rest| rest.trim_end_matches('/').to_string())
                    })
                    .collect()
            }
            #[cfg(test)]
            Self::Memory(store) => store.list_device_ids()?,
        };
        ids.retain(|id| is_valid_device_id(id));
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

#[cfg(test)]
pub(crate) mod memory {
    //! In-memory remote with fault injection, used by multi-device tests.
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use crate::error::AppError;

    #[derive(Default)]
    pub(crate) struct MemoryStore {
        pub objects: Mutex<BTreeMap<String, Vec<u8>>>,
        /// PUTs whose path contains this substring fail (simulated interruption).
        pub fail_put_containing: Mutex<Option<String>>,
        /// GETs whose path contains this substring fail with a transport error.
        pub fail_get_containing: Mutex<Option<String>>,
        pub fail_list: Mutex<bool>,
        /// DELETEs whose path contains this substring fail.
        pub fail_delete_containing: Mutex<Option<String>>,
        pub put_count: Mutex<usize>,
        pub get_count: Mutex<usize>,
        /// Runs once, before the first GET whose path contains the needle
        /// (simulates a user action while a download is in flight).
        #[allow(clippy::type_complexity)]
        pub before_get: Mutex<Option<(String, Box<dyn FnOnce() + Send>)>>,
    }

    fn transport_error(op: &str, path: &str) -> AppError {
        AppError::Message(format!("simulated {op} failure: {path}"))
    }

    impl MemoryStore {
        pub fn get(&self, path: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, AppError> {
            *self.get_count.lock().unwrap() += 1;
            let hook = {
                let mut slot = self.before_get.lock().unwrap();
                match slot.as_ref() {
                    Some((needle, _)) if path.contains(needle.as_str()) => slot.take(),
                    _ => None,
                }
            };
            if let Some((_, action)) = hook {
                action();
            }
            if let Some(needle) = self.fail_get_containing.lock().unwrap().as_deref() {
                if path.contains(needle) {
                    return Err(transport_error("GET", path));
                }
            }
            let objects = self.objects.lock().unwrap();
            match objects.get(path) {
                Some(bytes) if bytes.len() > max_bytes => {
                    Err(AppError::Message(format!("object too large: {path}")))
                }
                Some(bytes) => Ok(Some(bytes.clone())),
                None => Ok(None),
            }
        }

        pub fn put(&self, path: &str, bytes: Vec<u8>) -> Result<(), AppError> {
            if let Some(needle) = self.fail_put_containing.lock().unwrap().as_deref() {
                if path.contains(needle) {
                    return Err(transport_error("PUT", path));
                }
            }
            *self.put_count.lock().unwrap() += 1;
            self.objects.lock().unwrap().insert(path.to_string(), bytes);
            Ok(())
        }

        pub fn delete(&self, path: &str) -> Result<(), AppError> {
            if let Some(needle) = self.fail_delete_containing.lock().unwrap().as_deref() {
                if path.contains(needle) {
                    return Err(transport_error("DELETE", path));
                }
            }
            self.objects.lock().unwrap().remove(path);
            Ok(())
        }

        pub fn list_device_ids(&self) -> Result<Vec<String>, AppError> {
            if *self.fail_list.lock().unwrap() {
                return Err(transport_error("LIST", "devices/"));
            }
            let objects = self.objects.lock().unwrap();
            Ok(objects
                .keys()
                .filter_map(|key| key.strip_prefix("devices/"))
                .filter_map(|rest| rest.split('/').next())
                .map(str::to_string)
                .collect())
        }

        pub fn keys(&self) -> Vec<String> {
            self.objects.lock().unwrap().keys().cloned().collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_is_separate_from_config_snapshot() {
        let segments = namespace_segments("/cc-switch-sync/", "default");
        assert_eq!(segments, vec!["cc-switch-sync", "usage-v1", "default"]);
    }

    #[test]
    fn transport_security_flags_plain_http_to_remote_hosts() {
        let insecure = UsageTransport::WebDav {
            base_url: "http://nas.example.com/dav".into(),
            auth: None,
            root: vec![],
        };
        assert!(!insecure.is_transport_secure());
        let loopback = UsageTransport::WebDav {
            base_url: "http://127.0.0.1:8080/dav".into(),
            auth: None,
            root: vec![],
        };
        assert!(loopback.is_transport_secure());
        let tls = UsageTransport::WebDav {
            base_url: "https://dav.example.com".into(),
            auth: None,
            root: vec![],
        };
        assert!(tls.is_transport_secure());
        let s3_plain = UsageTransport::S3 {
            creds: S3Credentials {
                access_key_id: "a".into(),
                secret_access_key: "b".into(),
                region: "us-east-1".into(),
                bucket: "b".into(),
                endpoint: "http://minio.lan:9000".into(),
            },
            prefix: String::new(),
        };
        assert!(!s3_plain.is_transport_secure());
    }

    #[test]
    fn display_location_never_contains_credentials() {
        let transport = UsageTransport::S3 {
            creds: S3Credentials {
                access_key_id: "AKIASECRET".into(),
                secret_access_key: "verysecret".into(),
                region: "us-east-1".into(),
                bucket: "bucket".into(),
                endpoint: String::new(),
            },
            prefix: "cc-switch-sync/usage-v1/default".into(),
        };
        let shown = transport.display_location();
        assert!(!shown.contains("AKIASECRET"));
        assert!(!shown.contains("verysecret"));
    }
}

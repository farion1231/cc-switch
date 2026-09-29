//! Local VPS host data and generated SSH/client catalogs.
//! No SSH processes or Skill deployments are started by this module.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::app_config::{AppType, SkillApps};
use crate::config::{atomic_write_private, sorted_json_bytes};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VpsServer {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub purpose: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<PathBuf>,
    #[serde(default, deserialize_with = "deserialize_skill_apps")]
    pub apps: SkillApps,
}

impl VpsServer {
    /// Generate the identity once; reuse the same record when retrying a save.
    pub fn new(name: String, host: String, user: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            purpose: String::new(),
            host,
            port: 22,
            user,
            identity_file: None,
            apps: SkillApps::default(),
        }
    }

    pub fn ssh_alias(&self) -> String {
        format!("cc-switch-vps-{}", self.id)
    }

    pub fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        validate_text(&self.name, "name", 128, false)?;
        validate_text(&self.purpose, "purpose", 2048, true)?;
        if self.host.parse::<IpAddr>().is_err() {
            let hostname = self.host.strip_suffix('.').unwrap_or(&self.host);
            if self.host.len() > 253
                || !hostname.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && label
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                })
            {
                bail!("Invalid VPS host: use an IP address or a DNS hostname");
            }
        }
        if self.user.is_empty()
            || self.user.len() > 64
            || self.user.starts_with('-')
            || !self
                .user
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        {
            bail!("Invalid VPS user: use a username without SSH options or whitespace");
        }
        if self.port == 0 {
            bail!("Invalid VPS port: expected 1 through 65535");
        }
        if let Some(path) = &self.identity_file {
            ssh_identity_path(path)?;
        }
        Ok(())
    }
}

fn deserialize_skill_apps<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<SkillApps, D::Error> {
    use serde::de::Error;
    let value = serde_json::Value::deserialize(deserializer)?;
    let supported = serde_json::to_value(SkillApps::default()).map_err(D::Error::custom)?;
    let object = value
        .as_object()
        .ok_or_else(|| D::Error::custom("expected Skill client bindings"))?;
    if object.keys().any(|key| supported.get(key).is_none()) {
        return Err(D::Error::custom("unsupported VPS Skill client binding"));
    }
    serde_json::from_value(value).map_err(D::Error::custom)
}

fn validate_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id) {
        Ok(())
    } else {
        bail!("Invalid VPS id: expected a canonical UUID")
    }
}

fn validate_text(value: &str, field: &str, max_length: usize, allow_empty: bool) -> Result<()> {
    if (!allow_empty && value.trim().is_empty())
        || value.chars().count() > max_length
        || value.chars().any(char::is_control)
    {
        bail!("Invalid VPS {field}: empty, too long, or contains control characters");
    }
    Ok(())
}

fn ssh_identity_path(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| anyhow!("VPS identity path must be valid Unicode"))?;
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || value
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '%' | '$'))
    {
        bail!("Invalid VPS identity path: use an absolute path without SSH expansions or control characters");
    }
    // Quoted OpenSSH paths support spaces; forward slashes avoid backslash escapes on Windows.
    #[cfg(windows)]
    let value = value.replace('\\', "/");
    #[cfg(not(windows))]
    let value = {
        if value.contains('\\') {
            bail!("Invalid VPS identity path: backslash escapes are not supported");
        }
        value.to_string()
    };
    Ok(value)
}

fn validate_servers(servers: &[VpsServer]) -> Result<()> {
    let mut ids = HashSet::new();
    for server in servers {
        server.validate()?;
        if !ids.insert(&server.id) {
            bail!("Duplicate VPS id in servers.json");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VpsClientAction {
    Install,
    Update,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpsClientChange {
    pub app: AppType,
    pub action: VpsClientAction,
}

pub fn required_apps(servers: &[VpsServer]) -> SkillApps {
    let mut apps = SkillApps::default();
    for app in AppType::all() {
        apps.set_enabled_for(
            &app,
            servers
                .iter()
                .any(|server| server.apps.is_enabled_for(&app)),
        );
    }
    apps
}

fn servers_for_app<'a>(servers: &'a [VpsServer], app: &AppType) -> Vec<&'a VpsServer> {
    let mut selected: Vec<_> = servers
        .iter()
        .filter(|server| server.apps.is_enabled_for(app))
        .collect();
    selected.sort_by(|a, b| a.id.cmp(&b.id));
    selected
}

/// Desired-state differences, not proof of a successful Skill deployment.
pub fn plan_client_changes(before: &[VpsServer], after: &[VpsServer]) -> Vec<VpsClientChange> {
    AppType::all()
        .filter_map(|app| {
            let before = servers_for_app(before, &app);
            let after = servers_for_app(after, &app);
            let action = match (before.is_empty(), after.is_empty()) {
                (true, false) => VpsClientAction::Install,
                (false, true) => VpsClientAction::Remove,
                (false, false) if before != after => VpsClientAction::Update,
                _ => return None,
            };
            Some(VpsClientChange { app, action })
        })
        .collect()
}

pub fn render_ssh_config(servers: &[VpsServer]) -> Result<String> {
    validate_servers(servers)?;
    let mut servers: Vec<_> = servers.iter().collect();
    servers.sort_by(|a, b| a.id.cmp(&b.id));
    let mut config = String::from(
        "# Generated by CC Switch. Edit servers.json instead.\n\nHost *\n    BatchMode yes\n    StrictHostKeyChecking yes\n    PasswordAuthentication no\n    KbdInteractiveAuthentication no\n",
    );
    for server in servers {
        config.push_str(&format!(
            "\nHost {}\n    HostName {}\n    User {}\n    Port {}\n",
            server.ssh_alias(),
            server.host,
            server.user,
            server.port,
        ));
        match &server.identity_file {
            Some(path) => config.push_str(&format!(
                "    IdentityFile \"{}\"\n    IdentitiesOnly yes\n",
                ssh_identity_path(path)?,
            )),
            None => config.push_str("    IdentityFile none\n"),
        }
    }
    Ok(config)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VpsDocument {
    version: u32,
    servers: Vec<VpsServer>,
    /// Accepted hashes include an interrupted generation until it is completed.
    #[serde(default)]
    generated_files: BTreeMap<String, BTreeSet<String>>,
}

impl Default for VpsDocument {
    fn default() -> Self {
        Self {
            version: 1,
            servers: Vec::new(),
            generated_files: BTreeMap::new(),
        }
    }
}

impl VpsDocument {
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported VPS servers.json version: {}", self.version);
        }
        validate_servers(&self.servers)?;
        for (path, hashes) in &self.generated_files {
            let allowed = path == "ssh_config"
                || AppType::all().any(|app| {
                    !SkillApps::only(&app).is_empty()
                        && path == &format!("clients/{}.json", app.as_str())
                });
            if !allowed
                || hashes.is_empty()
                || hashes.iter().any(|hash| {
                    hash.len() != 64
                        || !hash
                            .bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                })
            {
                bail!("Invalid generated file metadata in VPS servers.json");
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientCatalog<'a> {
    version: u32,
    app: &'a str,
    ssh_config: PathBuf,
    servers: Vec<ClientServer<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientServer<'a> {
    id: &'a str,
    name: &'a str,
    purpose: &'a str,
    ssh_alias: String,
}

fn generated_files(root: &Path, servers: &[VpsServer]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::from([(
        "ssh_config".into(),
        render_ssh_config(servers)?.into_bytes(),
    )]);
    for app in AppType::all() {
        let selected = servers_for_app(servers, &app);
        if selected.is_empty() {
            continue;
        }
        let catalog = ClientCatalog {
            version: 1,
            app: app.as_str(),
            ssh_config: root.join("ssh_config"),
            servers: selected
                .into_iter()
                .map(|server| ClientServer {
                    id: &server.id,
                    name: &server.name,
                    purpose: &server.purpose,
                    ssh_alias: server.ssh_alias(),
                })
                .collect(),
        };
        files.insert(
            format!("clients/{}.json", app.as_str()),
            sorted_json_bytes(&catalog)?,
        );
    }
    Ok(files)
}

pub(crate) fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(crate) fn check_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link(&metadata) || !metadata.is_dir() => {
            bail!(
                "VPS directory is a link or not a directory: {}",
                path.display()
            )
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Read VPS directory {}", path.display())),
    }
}

pub(crate) fn read_regular_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link(&metadata) || !metadata.is_file() => {
            bail!(
                "VPS file is a link or not a regular file: {}",
                path.display()
            )
        }
        Ok(_) => fs::read(path)
            .map(Some)
            .with_context(|| format!("Read VPS file {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Read VPS file {}", path.display())),
    }
}

// Serializes read/modify/write across service instances, including recovery after a failed save.
fn state_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Clone)]
pub struct VpsService {
    root: PathBuf,
    #[cfg(test)]
    fail_write: Option<usize>,
}

impl Default for VpsService {
    fn default() -> Self {
        Self::new()
    }
}

impl VpsService {
    pub fn new() -> Self {
        Self {
            root: crate::config::get_app_config_dir().join("vps"),
            #[cfg(test)]
            fail_write: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn check_root(&self) -> Result<()> {
        if !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            bail!("VPS data directory must be an absolute path without parent traversal");
        }
        check_directory(&self.root)?;
        check_directory(&self.root.join("clients"))
    }

    fn read_document(&self) -> Result<(VpsDocument, Option<Vec<u8>>)> {
        self.check_root()?;
        let path = self.root.join("servers.json");
        let bytes = read_regular_file(&path)?;
        let document = match &bytes {
            Some(bytes) => serde_json::from_slice::<VpsDocument>(bytes)
                .with_context(|| format!("Parse VPS data {}", path.display()))?,
            None => VpsDocument::default(),
        };
        document.validate()?;
        Ok((document, bytes))
    }

    pub fn load(&self) -> Result<Vec<VpsServer>> {
        let _guard = state_lock()
            .lock()
            .map_err(|_| anyhow!("VPS state lock is poisoned"))?;
        Ok(self.read_document()?.0.servers)
    }

    /// Save desired host data and regenerate catalogs, not a successful Skill binding.
    pub fn save_server(&self, server: VpsServer) -> Result<()> {
        server.validate()?;
        let _guard = state_lock()
            .lock()
            .map_err(|_| anyhow!("VPS state lock is poisoned"))?;
        let (mut document, bytes) = self.read_document()?;
        if let Some(existing) = document
            .servers
            .iter_mut()
            .find(|item| item.id == server.id)
        {
            *existing = server;
        } else {
            document.servers.push(server);
        }
        self.apply(document, bytes)
    }

    pub fn delete_server(&self, id: &str) -> Result<()> {
        validate_id(id)?;
        let _guard = state_lock()
            .lock()
            .map_err(|_| anyhow!("VPS state lock is poisoned"))?;
        let (mut document, bytes) = self.read_document()?;
        document.servers.retain(|server| server.id != id);
        self.apply(document, bytes)
    }

    /// Retry an incomplete generation, or apply direct edits to servers.json.
    pub fn reconcile(&self) -> Result<()> {
        let _guard = state_lock()
            .lock()
            .map_err(|_| anyhow!("VPS state lock is poisoned"))?;
        let (document, bytes) = self.read_document()?;
        self.apply(document, bytes)
    }

    fn replace_checked(
        &self,
        relative: &str,
        expected: Option<&[u8]>,
        desired: Option<&[u8]>,
        writes: &mut usize,
    ) -> Result<()> {
        self.check_root()?;
        let path = self.root.join(relative);
        let current = read_regular_file(&path)?;
        if current.as_deref() != expected {
            bail!(
                "VPS file changed during generation; not overwritten: {}",
                path.display()
            );
        }
        if current.as_deref() == desired {
            return Ok(());
        }
        #[cfg(test)]
        if self.fail_write == Some(*writes) {
            bail!("injected VPS write failure: {}", path.display());
        }
        *writes += 1;
        match desired {
            Some(bytes) => atomic_write_private(&path, bytes)
                .with_context(|| format!("Write VPS file {}", path.display())),
            None => fs::remove_file(&path)
                .with_context(|| format!("Remove VPS file {}", path.display())),
        }
    }

    fn apply(&self, mut document: VpsDocument, previous: Option<Vec<u8>>) -> Result<()> {
        document.validate()?;
        document.servers.sort_by(|a, b| a.id.cmp(&b.id));
        let generated = generated_files(&self.root, &document.servers)?;
        let paths: BTreeSet<_> = document
            .generated_files
            .keys()
            .chain(generated.keys())
            .cloned()
            .collect();
        let mut existing = BTreeMap::new();
        let mut completed_hashes = BTreeMap::new();
        // Check all targets before saving desired data; never claim same-name foreign files.
        for relative in &paths {
            let path = self.root.join(relative);
            let current = read_regular_file(&path)?;
            if let Some(bytes) = &current {
                let hash = content_hash(bytes);
                if !document
                    .generated_files
                    .get(relative)
                    .is_some_and(|hashes| hashes.contains(&hash))
                {
                    bail!(
                        "VPS generated file is unowned or modified; not overwritten: {}",
                        path.display()
                    );
                }
            }
            existing.insert(relative.clone(), current);
            if let Some(bytes) = generated.get(relative) {
                let hash = content_hash(bytes);
                completed_hashes.insert(relative.clone(), BTreeSet::from([hash.clone()]));
                document
                    .generated_files
                    .entry(relative.clone())
                    .or_default()
                    .insert(hash);
            }
        }

        // Persist accepted old/new hashes before file changes. A restart can recognize either
        // side of an interrupted write without treating a saved binding as deployed.
        let intent = sorted_json_bytes(&document)?;
        let mut writes = 0;
        self.replace_checked(
            "servers.json",
            previous.as_deref(),
            Some(&intent),
            &mut writes,
        )?;
        let generation = (|| -> Result<()> {
            for relative in paths {
                self.replace_checked(
                    &relative,
                    existing[&relative].as_deref(),
                    generated.get(&relative).map(Vec::as_slice),
                    &mut writes,
                )?;
            }
            document.generated_files = completed_hashes;
            let completed = sorted_json_bytes(&document)?;
            self.replace_checked("servers.json", Some(&intent), Some(&completed), &mut writes)
        })();
        generation.context("VPS host data is saved, but file generation is incomplete; retry generation before reporting client access")
    }
}

mod managed;

#[cfg(test)]
#[path = "vps/tests.rs"]
mod tests;

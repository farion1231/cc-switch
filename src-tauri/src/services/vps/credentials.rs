//! Device-local OS credentials. Neither passwords nor vault contents are serializable.
use super::{content_hash, validate_id, VpsAuthMethod, VpsDocument, VpsServer, VpsService};
use anyhow::{anyhow, bail, Context, Result};
use std::{fmt, fs, path::Path};
use zeroize::Zeroizing;

pub struct SecretString(Zeroizing<String>);

impl SecretString {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn provided(value: Option<String>) -> Option<Self> {
        value
            .map(Self::new)
            .filter(|secret| !secret.expose().is_empty())
    }

    pub(super) fn validate(&self) -> Result<()> {
        // OpenSSH askpass uses one UTF-8 line. Never silently trim or truncate passwords.
        if self.expose().is_empty()
            || self.expose().len() > 4096
            || self.expose().contains(['\0', '\r', '\n'])
        {
            bail!("VPS password must be nonempty, at most 4096 bytes, and contain no NUL or line breaks");
        }
        Ok(())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// Injection boundary: tests must use an in-memory implementation, never a native vault.
/// `namespace` includes the canonical data root and stable server UUID; `id` is a revision UUID.
pub trait CredentialStore: Send + Sync {
    fn get(&self, namespace: &str, id: &str) -> Result<Option<SecretString>>;
    fn set(&self, namespace: &str, id: &str, password: &SecretString) -> Result<()>;
    fn delete(&self, namespace: &str, id: &str) -> Result<()>;
}

pub struct OsCredentialStore;

impl OsCredentialStore {
    fn entry(namespace: &str, id: &str) -> Result<keyring::Entry> {
        // Guard against accidental use by any unit test, including old fixtures.
        if cfg!(test) {
            bail!("Native VPS credential storage is disabled in unit tests; inject a mock");
        }
        if !cfg!(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux"
        )) {
            bail!("Native VPS credential storage is unavailable on this platform");
        }
        validate_id(id)?;
        // keyring 3.6.3 uses CRED_PERSIST_ENTERPRISE on Windows. CC Switch does not
        // synchronize these entries; any OS-level credential roaming remains OS policy.
        keyring::Entry::new(namespace, id).map_err(|_| {
            anyhow!("The OS credential store is unavailable; no plaintext fallback is used")
        })
    }
}

impl CredentialStore for OsCredentialStore {
    fn get(&self, namespace: &str, id: &str) -> Result<Option<SecretString>> {
        match Self::entry(namespace, id)?.get_password() {
            Ok(password) => Ok(Some(SecretString::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => bail!("Could not read the VPS password from the OS credential store"),
        }
    }

    fn set(&self, namespace: &str, id: &str, password: &SecretString) -> Result<()> {
        Self::entry(namespace, id)?
            .set_password(password.expose())
            .map_err(|_| anyhow!("Could not save the VPS password in the OS credential store"))
    }

    fn delete(&self, namespace: &str, id: &str) -> Result<()> {
        match Self::entry(namespace, id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => bail!(
                "Could not delete the VPS password from the OS credential store; retry cleanup"
            ),
        }
    }
}

/// Canonical root namespaces isolate daily-use, test and overridden data directories.
pub(super) fn namespace(root: &Path) -> Result<String> {
    super::check_directory(root)?;
    fs::create_dir_all(root).context("Create VPS credential namespace directory")?;
    let canonical = fs::canonicalize(root).context("Resolve VPS credential namespace directory")?;
    let canonical = canonical
        .to_str()
        .ok_or_else(|| anyhow!("VPS credential directory must be Unicode"))?;
    Ok(format!(
        "cc-switch.vps.password.v1.{}",
        content_hash(canonical.as_bytes())
    ))
}

fn server_namespace(root: &Path, id: &str) -> Result<String> {
    validate_id(id)?;
    Ok(format!("{}.{}", namespace(root)?, id))
}

pub(super) fn password_from_document(
    root: &Path,
    document: &VpsDocument,
    id: &str,
    store: &dyn CredentialStore,
) -> Result<Option<SecretString>> {
    let Some(revision) = document.password_revisions.get(id) else {
        return Ok(None);
    };
    store.get(&server_namespace(root, id)?, revision)
}

#[derive(Debug, thiserror::Error)]
#[error("SSH connection details do not match the saved password's target")]
pub(super) struct PasswordTargetChanged;

/// Snapshot the target and active immutable revision under the state lock, then release it
/// before accessing the OS vault. Concurrent cleanup may remove it, never substitute a new secret.
/// Legacy documents and unsaved hosts never cause any OS-vault access.
pub(super) fn read_password(
    root: &Path,
    server: &VpsServer,
    store: &dyn CredentialStore,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Option<SecretString>> {
    validate_id(&server.id)?;
    let guard = super::state_lock()
        .lock()
        .map_err(|_| anyhow!("VPS state lock is poisoned"))?;
    if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        bail!("SSH test cancelled before credential access");
    }
    super::check_directory(root)?;
    let Some(bytes) = super::read_regular_file(&root.join("servers.json"))? else {
        return Ok(None);
    };
    let document: VpsDocument =
        serde_json::from_slice(&bytes).context("Read VPS password reference")?;
    document.validate()?;
    let Some(saved) = document.servers.iter().find(|saved| saved.id == server.id) else {
        return Ok(None);
    };
    if saved.host != server.host
        || saved.port != server.port
        || saved.user != server.user
        || saved.auth_method != server.auth_method
    {
        return Err(PasswordTargetChanged.into());
    }
    drop(guard);
    if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        bail!("SSH test cancelled before credential access");
    }
    password_from_document(root, &document, &server.id, store)
}

impl VpsService {
    pub fn with_credential_store(
        mut self,
        credentials: std::sync::Arc<dyn CredentialStore>,
    ) -> Self {
        self.credentials = credentials;
        self
    }

    fn retire_password(document: &mut VpsDocument, id: &str) {
        if let Some(revision) = document.password_revisions.remove(id) {
            document
                .pending_credential_deletions
                .entry(id.into())
                .or_default()
                .insert(revision);
        }
    }

    pub(super) fn upsert(document: &mut VpsDocument, server: VpsServer) {
        if server.auth_method != Some(VpsAuthMethod::Password) {
            Self::retire_password(document, &server.id);
        }
        if let Some(existing) = document
            .servers
            .iter_mut()
            .find(|item| item.id == server.id)
        {
            *existing = server;
        } else {
            document.servers.push(server);
        }
    }

    pub(super) fn remove(document: &mut VpsDocument, id: &str) {
        Self::retire_password(document, id);
        document.servers.retain(|server| server.id != id);
    }

    /// Journal an immutable new entry BEFORE touching the vault. A crash, failed vault write
    /// or failed host commit leaves only an obsolete revision to clean, never a lost old secret.
    fn journal_password_revision(
        &self,
        id: &str,
        revision: &str,
        expected: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let (mut current, bytes) = self.read_document()?;
        if bytes.as_deref() != expected {
            bail!("VPS host data changed before credential staging; not overwritten");
        }
        current
            .pending_credential_deletions
            .entry(id.into())
            .or_default()
            .insert(revision.into());
        current.validate()?;
        let journal = crate::config::sorted_json_bytes(&current)?;
        self.replace_checked("servers.json", expected, Some(&journal), &mut 0)?;
        Ok(journal)
    }

    pub(super) fn apply_authenticated(
        &self,
        mut document: VpsDocument,
        mut bytes: Option<Vec<u8>>,
        authentication: Option<(&VpsServer, Option<&SecretString>)>,
    ) -> Result<()> {
        if let Some((server, supplied)) = authentication {
            if server.auth_method == Some(VpsAuthMethod::Password) {
                if let Some(password) = supplied {
                    password.validate()?;
                    self.check_root()?;
                    let namespace = server_namespace(&self.root, &server.id)?;
                    let revision = uuid::Uuid::new_v4().to_string();
                    bytes = Some(self.journal_password_revision(
                        &server.id,
                        &revision,
                        bytes.as_deref(),
                    )?);
                    // Never overwrite the active revision, even if cleanup/rollback would fail.
                    if let Err(error) = self.credentials.set(&namespace, &revision, password) {
                        return self.finish_credential_operation(Err(error));
                    }
                    Self::retire_password(&mut document, &server.id);
                    document
                        .password_revisions
                        .insert(server.id.clone(), revision);
                } else {
                    let password = password_from_document(
                        &self.root,
                        &document,
                        &server.id,
                        self.credentials.as_ref(),
                    )?
                    .ok_or_else(|| {
                        anyhow!(
                            "Password authentication requires a saved or newly supplied password"
                        )
                    })?;
                    password.validate()?;
                }
            } else if supplied.is_some() {
                bail!("A VPS password can only be supplied for password authentication");
            }
        }
        let result = self.apply(document, bytes);
        // Select cleanup from the persisted document, not the desired one. Before commit this
        // removes only the staged revision; after commit it removes only retired revisions.
        self.finish_credential_operation(result)
    }

    fn finish_credential_operation(&self, result: Result<()>) -> Result<()> {
        let cleanup = self.finish_credential_cleanup();
        match (result, cleanup) {
            (Err(error), Err(cleanup)) => Err(error.context(format!(
                "Credential cleanup is also incomplete: {cleanup:#}"
            ))),
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        }
    }

    fn finish_credential_cleanup(&self) -> Result<()> {
        let (mut document, previous) = self.read_document()?;
        if document.pending_credential_deletions.is_empty() {
            return Ok(());
        }
        // read_document validates that no cleanup receipt names an active revision.
        for (id, revisions) in &document.pending_credential_deletions {
            let namespace = server_namespace(&self.root, id)?;
            for revision in revisions {
                self.credentials.delete(&namespace, revision).context(
                    "VPS credential cleanup is incomplete; saved passwords remain available",
                )?;
            }
        }
        document.pending_credential_deletions.clear();
        self.replace_checked(
            "servers.json",
            previous.as_deref(),
            Some(&crate::config::sorted_json_bytes(&document)?),
            &mut 0,
        )
        .context(
            "VPS credential cleanup completed, but its receipt could not be saved; retry safely",
        )
    }
}

#[cfg(test)]
pub(super) mod tests;

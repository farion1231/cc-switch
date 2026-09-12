//! Oh My Pi-native instruction files and slash-command templates.
//!
//! Oh My Pi keeps its instruction files (`AGENTS.md`, `SYSTEM.md`) and
//! slash-command templates (`commands/*.md`) under `~/.omp/agent`. This service
//! mirrors the Pi prompt-file service with atomic writes + revision guards.

use crate::config::atomic_write;
#[cfg(test)]
use crate::config::get_home_dir;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

const MISSING_REVISION: &str = "missing";
const MAX_PROMPT_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TEMPLATE_SLUG_BYTES: usize = 128;
static PROMPT_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OhMyPiPromptFileKind {
    Agents,
    SystemOverride,
    SystemAppend,
}

impl OhMyPiPromptFileKind {
    fn filename(self) -> &'static str {
        match self {
            Self::Agents => "AGENTS.md",
            Self::SystemOverride => "SYSTEM.md",
            Self::SystemAppend => "APPEND_SYSTEM.md",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Agents => "AGENTS.md",
            Self::SystemOverride => "SYSTEM.md",
            Self::SystemAppend => "APPEND_SYSTEM.md",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OhMyPiAgentsFileSnapshot {
    pub content: Option<String>,
    pub revision: String,
}

/// Coordinates every CC Switch read-modify-write operation on Oh My Pi's
/// `AGENTS.md`.
///
/// Keeping the guard alive across the database update lets callers compare the
/// file revision immediately before an atomic replacement and roll back their
/// database write if Oh My Pi or another editor changed the file in the
/// meantime. Reads and writes go directly through the file system (not
/// `OhMyPiPromptFileService`) so the guard's lock is not re-acquired —
/// `std::sync::Mutex` is not reentrant.
pub struct OhMyPiAgentsFileGuard {
    _guard: MutexGuard<'static, ()>,
    path: PathBuf,
}

impl OhMyPiAgentsFileGuard {
    pub fn acquire() -> Result<Self, AppError> {
        Ok(Self {
            _guard: lock_prompt_files()?,
            path: get_agent_dir()?.join("AGENTS.md"),
        })
    }

    pub fn read(&self) -> Result<OhMyPiAgentsFileSnapshot, AppError> {
        let (content, file_revision) = match fs::File::open(&self.path) {
            Ok(_) => {
                let bytes = read_limited(&self.path)?;
                let file_revision = revision(&bytes);
                let content = String::from_utf8(bytes).map_err(|error| {
                    AppError::Config(format!(
                        "Oh My Pi AGENTS.md must be UTF-8 ({}): {error}",
                        self.path.display()
                    ))
                })?;
                (Some(content), file_revision)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (None, MISSING_REVISION.to_string())
            }
            Err(error) => return Err(AppError::io(&self.path, error)),
        };
        Ok(OhMyPiAgentsFileSnapshot {
            content,
            revision: file_revision,
        })
    }

    pub fn replace(&self, expected_revision: &str, content: &str) -> Result<(), AppError> {
        validate_content_size(content, "Oh My Pi AGENTS.md")?;
        ensure_revision(&self.path, expected_revision, "Oh My Pi AGENTS.md")?;
        atomic_write(&self.path, content.as_bytes())
    }

    pub fn delete(&self, expected_revision: &str) -> Result<(), AppError> {
        ensure_revision(&self.path, expected_revision, "Oh My Pi AGENTS.md")?;
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io(&self.path, error)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OhMyPiPromptFileSnapshot {
    pub exists: bool,
    pub revision: String,
    pub content: String,
}

pub struct OhMyPiPromptFileService;

impl OhMyPiPromptFileService {
    pub fn read(kind: OhMyPiPromptFileKind) -> Result<OhMyPiPromptFileSnapshot, AppError> {
        let _guard = lock_prompt_files()?;
        read_prompt_file(&get_agent_dir()?, kind)
    }

    pub fn replace(
        kind: OhMyPiPromptFileKind,
        expected_revision: &str,
        content: &str,
    ) -> Result<OhMyPiPromptFileSnapshot, AppError> {
        validate_instruction_content(content)?;
        let _guard = lock_prompt_files()?;
        let root = get_agent_dir()?;
        let path = root.join(kind.filename());
        ensure_revision(&path, expected_revision, kind.label())?;
        atomic_write(&path, content.as_bytes())?;
        read_prompt_file(&root, kind)
    }

    pub fn delete(kind: OhMyPiPromptFileKind, expected_revision: &str) -> Result<bool, AppError> {
        let _guard = lock_prompt_files()?;
        let path = get_agent_dir()?.join(kind.filename());
        ensure_revision(&path, expected_revision, kind.label())?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AppError::io(&path, error)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OhMyPiPromptTemplate {
    pub slug: String,
    pub content: String,
    pub revision: String,
}

pub struct OhMyPiPromptTemplateService;

impl OhMyPiPromptTemplateService {
    pub fn list() -> Result<Vec<OhMyPiPromptTemplate>, AppError> {
        let _guard = lock_prompt_files()?;
        let dir = get_agent_dir()?.join("commands");
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(AppError::io(&dir, error)),
        };

        let mut templates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| AppError::io(&dir, error))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let Some(slug) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if validate_template_slug(slug).is_err() {
                continue;
            }
            let bytes = read_limited(&path)?;
            let content = String::from_utf8(bytes).map_err(|error| {
                AppError::InvalidInput(format!(
                    "Oh My Pi prompt template must be UTF-8 ({}): {error}",
                    path.display()
                ))
            })?;
            templates.push(OhMyPiPromptTemplate {
                slug: slug.to_string(),
                revision: revision(content.as_bytes()),
                content,
            });
        }
        templates.sort_by(|left, right| left.slug.cmp(&right.slug));
        Ok(templates)
    }

    pub fn upsert(
        slug: &str,
        original_slug: Option<&str>,
        expected_revision: &str,
        content: &str,
    ) -> Result<OhMyPiPromptTemplate, AppError> {
        validate_template_slug(slug)?;
        if let Some(original_slug) = original_slug {
            validate_template_slug(original_slug)?;
        }
        validate_content_size(content, "Oh My Pi prompt template")?;
        let _guard = lock_prompt_files()?;
        let dir = get_agent_dir()?.join("commands");
        let path = template_path(&dir, slug);

        if let Some(original_slug) = original_slug.filter(|value| *value != slug) {
            let original_path = template_path(&dir, original_slug);
            ensure_revision(
                &original_path,
                expected_revision,
                "Oh My Pi prompt template",
            )?;
            ensure_revision(&path, MISSING_REVISION, "Oh My Pi prompt template")?;
            fs::rename(&original_path, &path)
                .map_err(|error| AppError::io(&original_path, error))?;

            if let Err(write_error) = atomic_write(&path, content.as_bytes()) {
                if let Err(rollback_error) = fs::rename(&path, &original_path) {
                    return Err(AppError::Message(format!(
                        "Oh My Pi prompt template save failed ({write_error}); rename rollback also failed: {rollback_error}"
                    )));
                }
                return Err(write_error);
            }
        } else {
            ensure_revision(&path, expected_revision, "Oh My Pi prompt template")?;
            atomic_write(&path, content.as_bytes())?;
        }

        Ok(OhMyPiPromptTemplate {
            slug: slug.to_string(),
            content: content.to_string(),
            revision: revision(content.as_bytes()),
        })
    }

    pub fn delete(slug: &str, expected_revision: &str) -> Result<bool, AppError> {
        validate_template_slug(slug)?;
        let _guard = lock_prompt_files()?;
        let path = template_path(&get_agent_dir()?.join("commands"), slug);
        ensure_revision(&path, expected_revision, "Oh My Pi prompt template")?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AppError::io(&path, error)),
        }
    }
}

fn get_agent_dir() -> Result<PathBuf, AppError> {
    crate::ohmypi_config::get_ohmypi_agent_dir()
}

fn lock_prompt_files() -> Result<MutexGuard<'static, ()>, AppError> {
    PROMPT_FILE_LOCK.lock().map_err(|error| {
        AppError::Config(format!("Oh My Pi prompt file lock is poisoned: {error}"))
    })
}

fn read_prompt_file(
    root: &Path,
    kind: OhMyPiPromptFileKind,
) -> Result<OhMyPiPromptFileSnapshot, AppError> {
    let path = root.join(kind.filename());
    if !path.exists() {
        return Ok(OhMyPiPromptFileSnapshot {
            exists: false,
            revision: MISSING_REVISION.to_string(),
            content: String::new(),
        });
    }
    let bytes = read_limited(&path)?;
    let content = String::from_utf8(bytes).map_err(|error| {
        AppError::Config(format!(
            "{} file must be UTF-8 ({}): {error}",
            kind.label(),
            path.display()
        ))
    })?;
    Ok(OhMyPiPromptFileSnapshot {
        exists: true,
        revision: revision(content.as_bytes()),
        content,
    })
}

fn ensure_revision(path: &Path, expected_revision: &str, label: &str) -> Result<(), AppError> {
    let actual_revision = match fs::File::open(path) {
        Ok(_) => revision(&read_limited(path)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => MISSING_REVISION.to_string(),
        Err(error) => return Err(AppError::io(path, error)),
    };
    if actual_revision == expected_revision {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "{label} changed outside CC Switch: {}",
            path.display()
        )))
    }
}

fn read_limited(path: &Path) -> Result<Vec<u8>, AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.len() > MAX_PROMPT_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Oh My Pi prompt file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    fs::read(path).map_err(|error| AppError::io(path, error))
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_instruction_content(content: &str) -> Result<(), AppError> {
    validate_content_size(content, "Oh My Pi instruction file")?;
    if content.contains('\0') {
        return Err(AppError::InvalidInput(
            "Oh My Pi instruction file must not contain NUL bytes".to_string(),
        ));
    }
    Ok(())
}

fn validate_content_size(content: &str, label: &str) -> Result<(), AppError> {
    if content.len() > MAX_PROMPT_FILE_BYTES as usize {
        return Err(AppError::InvalidInput(format!(
            "{label} exceeds the 1 MiB limit"
        )));
    }
    Ok(())
}

fn validate_template_slug(slug: &str) -> Result<(), AppError> {
    if slug.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Oh My Pi prompt template slug cannot be empty".to_string(),
        ));
    }
    if slug.len() > MAX_TEMPLATE_SLUG_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Oh My Pi prompt template slug exceeds {MAX_TEMPLATE_SLUG_BYTES} bytes"
        )));
    }
    if !slug
        .chars()
        .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(AppError::InvalidInput(
            "Oh My Pi prompt template slug may only contain letters, digits, '-' and '_'"
                .to_string(),
        ));
    }
    Ok(())
}

fn template_path(dir: &Path, slug: &str) -> PathBuf {
    dir.join(format!("{slug}.md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_file_uses_agents_md() {
        assert_eq!(OhMyPiPromptFileKind::Agents.filename(), "AGENTS.md");
        assert_eq!(OhMyPiPromptFileKind::SystemOverride.filename(), "SYSTEM.md");
        assert_eq!(
            OhMyPiPromptFileKind::SystemAppend.filename(),
            "APPEND_SYSTEM.md"
        );
    }

    #[test]
    fn template_slug_validation() {
        assert!(validate_template_slug("hello-world").is_ok());
        assert!(validate_template_slug("hello world").is_err());
        assert!(validate_template_slug("").is_err());
    }

    #[test]
    fn root_resolution_falls_back_to_default_omp() {
        // Ensure resolution does not panic under the default environment.
        let _ = get_home_dir();
        let _ = crate::ohmypi_config::get_ohmypi_agent_dir();
    }
}

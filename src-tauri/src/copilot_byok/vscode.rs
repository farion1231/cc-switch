use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

const LANGUAGE_MODELS_FILE: &str = "chatLanguageModels.json";
const PROMPTS_DIRECTORY: &str = "prompts";
const MCP_FILE: &str = "mcp.json";
const PROFILE_STORAGE_FILE: &str = "storage.json";
const MAX_DISCOVERED_TARGETS: usize = 64;
const MAX_PROFILE_STORAGE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VsCodeStorage {
    #[serde(default)]
    user_data_profiles: Vec<VsCodeStoredProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VsCodeStoredProfile {
    #[serde(default)]
    location: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    use_default_flags: Option<VsCodeUseDefaultFlags>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VsCodeUseDefaultFlags {
    #[serde(default)]
    language_models: bool,
    #[serde(default)]
    prompts: bool,
    #[serde(default)]
    mcp: bool,
}

impl VsCodeUseDefaultFlags {
    fn agents_window() -> Self {
        Self {
            language_models: true,
            prompts: true,
            mcp: true,
        }
    }
}

fn effective_use_default_flags(
    profile_location: &str,
    stored: Option<VsCodeUseDefaultFlags>,
) -> VsCodeUseDefaultFlags {
    // VS Code overrides the stored flags for its built-in Agents window
    // profile with AGENTS_WINDOW_PROFILE_FLAGS. Mirror the three resources CC
    // Switch manages instead of trusting potentially absent/stale metadata.
    if profile_location.replace('\\', "/") == "builtin/agents" {
        VsCodeUseDefaultFlags::agents_window()
    } else {
        stored.unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum VsCodeEdition {
    Stable,
    Insiders,
}

impl VsCodeEdition {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Insiders => "insiders",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Stable => "Visual Studio Code",
            Self::Insiders => "Visual Studio Code Insiders",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VsCodeProfileResources {
    pub language_models_path: String,
    pub prompts_home: String,
    pub mcp_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VsCodeProfileTarget {
    pub id: String,
    pub edition: VsCodeEdition,
    pub edition_name: String,
    pub profile_id: Option<String>,
    pub profile_name: String,
    pub is_default: bool,
    pub user_dir: String,
    pub resources: VsCodeProfileResources,
    pub config_exists: bool,
    pub backup_exists: bool,
}

impl VsCodeProfileTarget {
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.resources.language_models_path)
    }

    pub fn prompts_home(&self) -> PathBuf {
        PathBuf::from(&self.resources.prompts_home)
    }

    pub fn mcp_path(&self) -> PathBuf {
        PathBuf::from(&self.resources.mcp_path)
    }
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.cc-switch.bak")
}

fn target(
    edition: VsCodeEdition,
    user_dir: &Path,
    profile_id: Option<String>,
    profile_name: Option<String>,
    profile_dir: &Path,
    use_default_flags: VsCodeUseDefaultFlags,
) -> VsCodeProfileTarget {
    let is_default = profile_id.is_none();
    let profile_name = profile_name.unwrap_or_else(|| "Default".to_string());
    let id = match profile_id.as_deref() {
        Some(profile_id) => format!("{}:profile:{profile_id}", edition.slug()),
        None => format!("{}:default", edition.slug()),
    };
    let inherits = |select: fn(&VsCodeUseDefaultFlags) -> bool| select(&use_default_flags);
    let language_models_home = if inherits(|flags| flags.language_models) {
        user_dir
    } else {
        profile_dir
    };
    let prompts_home = if inherits(|flags| flags.prompts) {
        user_dir.join(PROMPTS_DIRECTORY)
    } else {
        profile_dir.join(PROMPTS_DIRECTORY)
    };
    let mcp_path = if inherits(|flags| flags.mcp) {
        user_dir.join(MCP_FILE)
    } else {
        profile_dir.join(MCP_FILE)
    };
    let language_models_path = language_models_home.join(LANGUAGE_MODELS_FILE);

    VsCodeProfileTarget {
        id,
        edition,
        edition_name: edition.display_name().to_string(),
        profile_id,
        profile_name,
        is_default,
        user_dir: user_dir.to_string_lossy().to_string(),
        config_exists: language_models_path.exists(),
        backup_exists: backup_path(&language_models_path).exists(),
        resources: VsCodeProfileResources {
            language_models_path: language_models_path.to_string_lossy().to_string(),
            prompts_home: prompts_home.to_string_lossy().to_string(),
            mcp_path: mcp_path.to_string_lossy().to_string(),
        },
    }
}

fn read_stored_profiles(user_dir: &Path) -> Result<Vec<VsCodeStoredProfile>, AppError> {
    let path = user_dir.join("globalStorage").join(PROFILE_STORAGE_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let metadata = fs::symlink_metadata(&path).map_err(|error| AppError::io(&path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::InvalidInput(format!(
            "VS Code profile metadata is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_PROFILE_STORAGE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "VS Code profile metadata exceeds {} MiB: {}",
            MAX_PROFILE_STORAGE_BYTES / 1024 / 1024,
            path.display()
        )));
    }

    let contents = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    let storage: VsCodeStorage =
        serde_json::from_str(&contents).map_err(|error| AppError::json(&path, error))?;
    Ok(storage.user_data_profiles)
}

fn resolve_profile_dir(profiles_dir: &Path, location: &str) -> Option<PathBuf> {
    let relative = Path::new(location.trim());
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }

    let profile_dir = profiles_dir.join(relative);
    if !profile_dir.is_dir() {
        return None;
    }

    let canonical_profiles = fs::canonicalize(profiles_dir).ok()?;
    let canonical_profile = fs::canonicalize(&profile_dir).ok()?;
    canonical_profile
        .starts_with(&canonical_profiles)
        .then_some(profile_dir)
}

fn default_user_roots() -> Vec<(VsCodeEdition, PathBuf)> {
    let Some(config_dir) = dirs::config_dir() else {
        return Vec::new();
    };

    vec![
        (VsCodeEdition::Stable, config_dir.join("Code").join("User")),
        (
            VsCodeEdition::Insiders,
            config_dir.join("Code - Insiders").join("User"),
        ),
    ]
}

pub fn discover_vscode_targets() -> Result<Vec<VsCodeProfileTarget>, AppError> {
    discover_from_roots(&default_user_roots())
}

pub(crate) fn discover_from_roots(
    roots: &[(VsCodeEdition, PathBuf)],
) -> Result<Vec<VsCodeProfileTarget>, AppError> {
    let mut targets = Vec::new();

    for (edition, user_dir) in roots {
        if !user_dir.is_dir() {
            continue;
        }

        targets.push(target(
            *edition,
            user_dir,
            None,
            None,
            user_dir,
            VsCodeUseDefaultFlags::default(),
        ));
        if targets.len() >= MAX_DISCOVERED_TARGETS {
            break;
        }

        let profiles_dir = user_dir.join("profiles");
        if !profiles_dir.is_dir() {
            continue;
        }

        let stored_profiles = match read_stored_profiles(user_dir) {
            Ok(profiles) => profiles,
            Err(error) => {
                log::warn!(
                    "Failed to read VS Code profile metadata from {}: {error}",
                    user_dir.display()
                );
                Vec::new()
            }
        };

        for profile in stored_profiles {
            if targets.len() >= MAX_DISCOVERED_TARGETS {
                break;
            }

            let profile_id = profile.location.trim().to_string();
            let profile_name = profile.name.trim().to_string();
            if profile_id.is_empty() || profile_name.is_empty() {
                continue;
            }
            let Some(profile_dir) = resolve_profile_dir(&profiles_dir, &profile_id) else {
                continue;
            };
            let use_default_flags =
                effective_use_default_flags(&profile_id, profile.use_default_flags);

            targets.push(target(
                *edition,
                user_dir,
                Some(profile_id),
                Some(profile_name),
                &profile_dir,
                use_default_flags,
            ));
        }
    }

    targets.sort_by(|left, right| {
        left.edition
            .cmp(&right.edition)
            .then_with(|| right.is_default.cmp(&left.is_default))
            .then_with(|| left.profile_name.cmp(&right.profile_name))
    });
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_default_and_named_profiles() {
        let temp = tempfile::tempdir().expect("temp directory");
        let user_dir = temp.path().join("Code").join("User");
        let profile_dir = user_dir.join("profiles").join("work-profile");
        fs::create_dir_all(&profile_dir).expect("create profile directory");
        let global_storage = user_dir.join("globalStorage");
        fs::create_dir_all(&global_storage).expect("create global storage directory");
        fs::write(
            global_storage.join(PROFILE_STORAGE_FILE),
            r#"{"userDataProfiles":[{"location":"work-profile","name":"Work"}]}"#,
        )
        .expect("write profile metadata");
        fs::write(user_dir.join(LANGUAGE_MODELS_FILE), "[]").expect("write default config");

        let targets = discover_from_roots(&[(VsCodeEdition::Stable, user_dir.clone())])
            .expect("discover targets");

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].id, "stable:default");
        assert!(targets[0].config_exists);
        assert_eq!(targets[1].id, "stable:profile:work-profile");
        assert_eq!(targets[1].profile_name, "Work");
        assert_eq!(targets[1].path(), profile_dir.join(LANGUAGE_MODELS_FILE));
    }

    #[test]
    fn resolves_each_profile_resource_inheritance_independently() {
        let temp = tempfile::tempdir().expect("temp directory");
        let user_dir = temp.path().join("Code").join("User");
        let profile_dir = user_dir.join("profiles").join("models-shared");
        let global_storage = user_dir.join("globalStorage");
        fs::create_dir_all(&profile_dir).expect("create profile directory");
        fs::create_dir_all(&global_storage).expect("create global storage directory");
        fs::write(
            global_storage.join(PROFILE_STORAGE_FILE),
            r#"{
                "userDataProfiles": [
                    {
                        "location": "models-shared",
                        "name": "Models Shared",
                        "useDefaultFlags": {
                            "languageModels": true,
                            "prompts": false,
                            "mcp": false
                        }
                    }
                ]
            }"#,
        )
        .expect("write profile metadata");

        let targets = discover_from_roots(&[(VsCodeEdition::Stable, user_dir.clone())])
            .expect("discover targets");

        assert_eq!(targets.len(), 2);
        let profile = targets
            .iter()
            .find(|target| target.id == "stable:profile:models-shared")
            .expect("named profile");
        assert_eq!(profile.path(), user_dir.join(LANGUAGE_MODELS_FILE));
        assert_eq!(profile.prompts_home(), profile_dir.join(PROMPTS_DIRECTORY));
        assert_eq!(profile.mcp_path(), profile_dir.join(MCP_FILE));
    }

    #[test]
    fn built_in_agents_profile_uses_vscode_forced_default_resources() {
        let temp = tempfile::tempdir().expect("temp directory");
        let user_dir = temp.path().join("Code").join("User");
        let agents_dir = user_dir.join("profiles").join("builtin").join("agents");
        let global_storage = user_dir.join("globalStorage");
        fs::create_dir_all(&agents_dir).expect("create agents profile directory");
        fs::create_dir_all(&global_storage).expect("create global storage directory");
        fs::write(
            global_storage.join(PROFILE_STORAGE_FILE),
            r#"{
                "userDataProfiles": [{
                    "location": "builtin/agents",
                    "name": "Agents",
                    "useDefaultFlags": {
                        "languageModels": false,
                        "prompts": false,
                        "mcp": false
                    }
                }]
            }"#,
        )
        .expect("write profile metadata");

        let targets = discover_from_roots(&[(VsCodeEdition::Stable, user_dir.clone())])
            .expect("discover targets");
        let agents = targets
            .iter()
            .find(|target| target.id == "stable:profile:builtin/agents")
            .expect("agents profile");

        assert_eq!(agents.path(), user_dir.join(LANGUAGE_MODELS_FILE));
        assert_eq!(agents.prompts_home(), user_dir.join(PROMPTS_DIRECTORY));
        assert_eq!(agents.mcp_path(), user_dir.join(MCP_FILE));
    }

    #[test]
    fn inherited_prompts_and_mcp_do_not_follow_profile_language_models() {
        let temp = tempfile::tempdir().expect("temp directory");
        let user_dir = temp.path().join("Code").join("User");
        let profile_dir = user_dir.join("profiles").join("work-profile");
        let global_storage = user_dir.join("globalStorage");
        fs::create_dir_all(&profile_dir).expect("create profile directory");
        fs::create_dir_all(&global_storage).expect("create global storage directory");
        fs::write(
            global_storage.join(PROFILE_STORAGE_FILE),
            r#"{
                "userDataProfiles": [{
                    "location": "work-profile",
                    "name": "Work",
                    "useDefaultFlags": {
                        "languageModels": false,
                        "prompts": true,
                        "mcp": true
                    }
                }]
            }"#,
        )
        .expect("write profile metadata");

        let targets = discover_from_roots(&[(VsCodeEdition::Stable, user_dir.clone())])
            .expect("discover targets");
        let work = targets
            .iter()
            .find(|target| target.id == "stable:profile:work-profile")
            .expect("work profile");

        assert_eq!(work.path(), profile_dir.join(LANGUAGE_MODELS_FILE));
        assert_eq!(work.prompts_home(), user_dir.join(PROMPTS_DIRECTORY));
        assert_eq!(work.mcp_path(), user_dir.join(MCP_FILE));
    }

    #[test]
    fn ignores_profile_directories_missing_from_vscode_metadata() {
        let temp = tempfile::tempdir().expect("temp directory");
        let user_dir = temp.path().join("Code").join("User");
        fs::create_dir_all(user_dir.join("profiles").join("builtin"))
            .expect("create profile container");

        let targets =
            discover_from_roots(&[(VsCodeEdition::Stable, user_dir)]).expect("discover targets");

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "stable:default");
    }

    #[test]
    fn corrupt_profile_metadata_does_not_hide_the_default_target() {
        let temp = tempfile::tempdir().expect("temp directory");
        let user_dir = temp.path().join("Code").join("User");
        let global_storage = user_dir.join("globalStorage");
        fs::create_dir_all(&global_storage).expect("create global storage directory");
        fs::write(global_storage.join(PROFILE_STORAGE_FILE), "{not-json")
            .expect("write corrupt profile metadata");

        let targets =
            discover_from_roots(&[(VsCodeEdition::Stable, user_dir)]).expect("discover targets");

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "stable:default");
    }

    #[test]
    fn rejects_profile_locations_outside_the_profiles_directory() {
        let temp = tempfile::tempdir().expect("temp directory");
        let user_dir = temp.path().join("Code").join("User");
        let global_storage = user_dir.join("globalStorage");
        fs::create_dir_all(user_dir.join("profiles")).expect("create profiles directory");
        fs::create_dir_all(user_dir.join("outside")).expect("create outside directory");
        fs::create_dir_all(&global_storage).expect("create global storage directory");
        fs::write(
            global_storage.join(PROFILE_STORAGE_FILE),
            r#"{"userDataProfiles":[{"location":"../outside","name":"Outside"}]}"#,
        )
        .expect("write profile metadata");

        let targets =
            discover_from_roots(&[(VsCodeEdition::Stable, user_dir)]).expect("discover targets");

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "stable:default");
    }

    #[test]
    fn ignores_missing_installations() {
        let temp = tempfile::tempdir().expect("temp directory");
        let targets = discover_from_roots(&[(
            VsCodeEdition::Insiders,
            temp.path().join("missing").join("User"),
        )])
        .expect("discover targets");
        assert!(targets.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinked_profile_directories() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp directory");
        let user_dir = temp.path().join("Code").join("User");
        let profiles_dir = user_dir.join("profiles");
        let outside = temp.path().join("outside");
        let global_storage = user_dir.join("globalStorage");
        fs::create_dir_all(&profiles_dir).expect("create profiles directory");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::create_dir_all(&global_storage).expect("create global storage directory");
        symlink(&outside, profiles_dir.join("linked")).expect("create symlink");
        fs::write(
            global_storage.join(PROFILE_STORAGE_FILE),
            r#"{"userDataProfiles":[{"location":"linked","name":"Linked"}]}"#,
        )
        .expect("write profile metadata");

        let targets =
            discover_from_roots(&[(VsCodeEdition::Stable, user_dir)]).expect("discover targets");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "stable:default");
    }
}

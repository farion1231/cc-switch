//! Opt-in routing for Skills owned by a workspace or another local tool.
//!
//! There is deliberately no SSOT copy (or SSOT link) for these entries. The
//! device-local settings hold their source and the projections we actually own;
//! database/cloud snapshots contain only ordinary, path-free Skill metadata.
use super::*;
use crate::settings::{
    get_linked_skill_source, set_linked_skill_projection, set_linked_skill_source,
    LinkedSkillSource,
};

impl SkillService {
    /// Register one explicitly selected local original without moving or copying it.
    pub fn link_source(
        db: &Arc<Database>,
        path: &Path,
        current_app: &AppType,
    ) -> Result<InstalledSkill> {
        Self::link_source_confirmed(db, path, current_app, false)
    }

    /// Replacing an existing managed snapshot requires explicit confirmation of
    /// the selected original. No timestamp comparison or content merge is used.
    pub fn link_source_confirmed(
        db: &Arc<Database>,
        path: &Path,
        current_app: &AppType,
        replace_existing: bool,
    ) -> Result<InstalledSkill> {
        let _state_guard = skill_state_write_guard();
        if matches!(current_app, AppType::ClaudeDesktop | AppType::OpenClaw) {
            return Err(anyhow!("This application does not support managed Skills"));
        }
        if !path.is_absolute() {
            return Err(anyhow!("Select an absolute Skill source directory"));
        }
        let directory = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("Select a Skill directory containing SKILL.md"))?;
        let directory = Self::require_valid_directory(directory)?;
        let source = path.canonicalize().context("Skill source is unavailable")?;
        Self::validate_sync_source_dir(&source, &directory)?;
        if Self::paths_overlap(&source, &Self::get_ssot_dir()?)
            || Self::paths_overlap(&source, &Self::get_backup_dir()?)
        {
            return Err(anyhow!(
                "Select an original outside managed Skill storage and backups"
            ));
        }
        let sources = crate::settings::get_settings().linked_skill_sources;
        if sources.iter().any(|(name, existing)| {
            name.eq_ignore_ascii_case(&directory)
                || Self::paths_alias(Path::new(&existing.path), &source)
        }) {
            return Err(anyhow!(
                "Skill source or directory is already registered: {directory}"
            ));
        }
        let mut existing: Vec<_> = db
            .get_all_installed_skills()?
            .into_values()
            .filter(|skill| skill.directory.eq_ignore_ascii_case(&directory))
            .collect();
        if existing.len() > 1 {
            return Err(anyhow!(
                "Multiple managed Skills use this directory: {directory}"
            ));
        }
        let existing = existing.pop();
        if existing.is_some() && !replace_existing {
            return Err(anyhow!("Confirm using the selected folder as the original for the existing Skill: {directory}"));
        }
        let directory = existing
            .as_ref()
            .map(|skill| skill.directory.clone())
            .unwrap_or(directory);
        let slot = Self::get_ssot_dir()?.join(&directory);
        if existing.is_none() && (slot.exists() || Self::is_symlink(&slot)) {
            return Err(anyhow!(
                "Managed storage already contains this Skill name: {directory}"
            ));
        }
        let (name, description) = Self::read_skill_name_desc(&source.join("SKILL.md"), &directory);
        let mut apps = existing
            .as_ref()
            .map(|skill| skill.apps.clone())
            .unwrap_or_default();
        if existing.is_some() {
            apps.pi = Self::skill_exists_in_app(&directory, &AppType::Pi);
        }
        apps.set_enabled_for(current_app, true);
        let mut old_routes = Vec::new();
        let mut seen_old_routes = std::collections::HashSet::new();
        // A real original or a pre-existing foreign link remains owned by its
        // existing tool. Reflect that exposure without taking ownership of it.
        for app in AppType::all() {
            if !matches!(app, AppType::ClaudeDesktop | AppType::OpenClaw) {
                if let Ok(root) = Self::get_app_skills_dir(&app) {
                    let destination = root.join(&directory);
                    if existing.is_some() {
                        if let Ok(target) = fs::read_link(&destination) {
                            let resolved = if target.is_absolute() {
                                target.clone()
                            } else {
                                root.join(&target)
                            };
                            // Compare the pointer's location without following
                            // the slot itself, which may also be a symlink.
                            let at_slot = resolved.file_name() == slot.file_name()
                                && matches!((resolved.parent().and_then(|p| p.canonicalize().ok()), slot.parent().and_then(|p| p.canonicalize().ok())), (Some(left), Some(right)) if left == right);
                            if at_slot {
                                // Overrides can make multiple apps share one physical
                                // link. Remove it only once, even through path aliases.
                                let physical_route =
                                    root.canonicalize().unwrap_or(root.clone()).join(&directory);
                                if seen_old_routes.insert(physical_route) {
                                    old_routes.push((destination, target));
                                }
                                continue;
                            }
                        }
                    }
                    if Self::paths_alias(&source, &destination) {
                        apps.set_enabled_for(&app, true);
                        continue;
                    }
                    if apps.is_enabled_for(&app)
                        && (destination.exists() || Self::is_symlink(&destination))
                    {
                        return Err(anyhow!("Existing directory or external link was preserved; resolve it before linking this original: {}", destination.display()));
                    }
                }
            }
        }
        let skill = InstalledSkill {
            id: existing
                .as_ref()
                .map(|skill| skill.id.clone())
                .unwrap_or_else(|| format!("local:{directory}")),
            name,
            description,
            directory: directory.clone(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps,
            installed_at: Utc::now().timestamp(),
            content_hash: None,
            updated_at: 0,
        };
        // Preserve only the old managed snapshot, never copy the chosen original.
        // Staging the old slot also makes rollback possible if any host rejects
        // symlink creation; pre-existing host originals are never moved.
        let mut staged = None;
        if let Some(previous) = &existing {
            if slot.exists() || Self::is_symlink(&slot) {
                if !Self::is_symlink(&slot) {
                    Self::validate_sync_source_dir(&slot, &directory)?;
                    Self::create_uninstall_backup_excluding(previous, Some(&source))?
                        .ok_or_else(|| anyhow!("Could not preserve the managed Skill snapshot"))?;
                }
                let staging = tempfile::Builder::new()
                    .prefix(".link-source-")
                    .tempdir_in(Self::get_ssot_dir()?)?;
                fs::rename(&slot, staging.path().join("skill"))?;
                staged = Some(staging);
            }
        }
        let result = (|| -> Result<()> {
            set_linked_skill_source(
                &directory,
                Some(LinkedSkillSource {
                    path: source.to_string_lossy().to_string(),
                    projections: Default::default(),
                }),
            )?;
            for (destination, expected) in &old_routes {
                if fs::read_link(destination).ok().as_ref() != Some(expected) {
                    return Err(anyhow!(
                        "Skill route changed during conversion: {}",
                        destination.display()
                    ));
                }
                Self::remove_path(destination)?;
            }
            for app in skill.apps.enabled_apps() {
                Self::sync_linked_source(&directory, &app)?;
            }
            db.save_skill(&skill)?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut failures = Vec::new();
            for app in AppType::all() {
                if let Err(rollback) = Self::remove_linked_projection(&directory, &app, true) {
                    failures.push(rollback.to_string());
                }
            }
            if let Some(staging) = staged.take() {
                let restored = if slot.exists() || Self::is_symlink(&slot) {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "Managed storage changed during conversion",
                    ))
                } else {
                    fs::rename(staging.path().join("skill"), &slot)
                };
                if let Err(rollback) = restored {
                    let recovery = staging.keep();
                    failures.push(format!(
                        "{rollback}; managed snapshot retained at {}",
                        recovery.display()
                    ));
                }
            }
            for (destination, target) in &old_routes {
                if !destination.exists() && !Self::is_symlink(destination) {
                    if let Err(rollback) = Self::create_symlink(target, destination) {
                        failures.push(rollback.to_string());
                    }
                }
            }
            if let Some(previous) = &existing {
                if let Err(rollback) = db.save_skill(previous) {
                    failures.push(rollback.to_string());
                }
            }
            if let Err(rollback) = set_linked_skill_source(&directory, None) {
                failures.push(rollback.to_string());
            }
            if !failures.is_empty() {
                return Err(error.context(format!(
                    "Conversion rollback requires attention: {}",
                    failures.join("; ")
                )));
            }
            return Err(error);
        }
        Ok(skill)
    }

    pub(super) fn sync_linked_source(directory: &str, app: &AppType) -> Result<()> {
        Self::require_valid_directory(directory)?;
        if matches!(app, AppType::ClaudeDesktop | AppType::OpenClaw) {
            return Err(anyhow!("This application does not support managed Skills"));
        }
        let registration = get_linked_skill_source(directory)
            .ok_or_else(|| anyhow!("Linked Skill source is not registered: {directory}"))?;
        let source = PathBuf::from(&registration.path);
        // A detached disk or missing source is an error, never an invitation to
        // revive an old SSOT copy or silently fall back to Copy mode.
        Self::validate_sync_source_dir(&source, directory)
            .context("Linked Skill source is unavailable; existing routes were preserved")?;
        let root = Self::get_app_skills_dir(app)?;
        let destination = root.join(directory);
        // The destination may not exist yet. Resolve its nearest existing
        // ancestor so symlinked parents and `..` cannot conceal a source loop.
        let mut ancestor = root.as_path();
        let mut missing = Vec::new();
        while !ancestor.exists() && !Self::is_symlink(ancestor) {
            missing.push(
                ancestor
                    .file_name()
                    .ok_or_else(|| anyhow!("Invalid Skill route directory"))?,
            );
            ancestor = ancestor
                .parent()
                .ok_or_else(|| anyhow!("Invalid Skill route directory"))?;
        }
        let mut resolved = ancestor
            .canonicalize()
            .context("Skill route directory is unavailable")?;
        for component in missing.into_iter().rev() {
            resolved.push(component);
        }
        resolved.push(directory);
        if (resolved != source && resolved.starts_with(&source))
            || (!Self::paths_alias(&source, &destination)
                && Self::paths_overlap(&source, &resolved))
        {
            return Err(anyhow!(
                "Skill source and application route must not contain one another: {directory}"
            ));
        }
        if destination.exists() || Self::is_symlink(&destination) {
            if Self::paths_alias(&source, &destination) {
                return Ok(());
            }
            return Err(anyhow!(
                "Existing Skill is not the registered source; refusing to replace it: {directory}"
            ));
        }
        fs::create_dir_all(&root)?;
        // Explicit linked sources require real links on every platform. If the
        // platform does not allow them, return the error without making a copy.
        Self::create_symlink(&source, &destination)?;
        if let Err(error) = set_linked_skill_projection(directory, app, &destination, true) {
            if fs::read_link(&destination).ok().as_ref() == Some(&source) {
                Self::remove_path(&destination)?;
            }
            return Err(error.into());
        }
        Ok(())
    }

    /// Only the exact symbolic links created by this registration are removable.
    /// Unregistering preserves source/foreign entries; disabling one reports why
    /// its original remains discoverable instead of claiming it was hidden.
    pub(super) fn remove_linked_projection(
        directory: &str,
        app: &AppType,
        unregistering: bool,
    ) -> Result<()> {
        Self::require_valid_directory(directory)?;
        let Some(registration) = get_linked_skill_source(directory) else {
            return Ok(());
        };
        let current = Self::get_app_skills_dir(app)?.join(directory);
        let source = PathBuf::from(&registration.path);
        let owned = registration
            .projections
            .get(app.as_str())
            .cloned()
            .unwrap_or_default();
        if !unregistering
            && (current.exists() || Self::is_symlink(&current))
            && (!owned.contains(&current) || fs::read_link(&current).ok().as_ref() != Some(&source))
        {
            return Err(anyhow!("This Skill directory is externally owned. Move the original to a workspace or remove its external link before disabling this application's route: {directory}"));
        }
        for destination in owned {
            // Settings overrides may have moved since this route was created.
            // Remember exact paths rather than treating any link in a host as ours.
            if fs::read_link(&destination).ok().as_ref() == Some(&source) {
                Self::remove_path(&destination)?;
                if let Err(error) = set_linked_skill_projection(directory, app, &destination, false)
                {
                    // Recreate the exact removed pointer, even if its disk is detached.
                    Self::create_symlink(&source, &destination)?;
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    pub(super) fn unlink_source(
        db: &Arc<Database>,
        skill: &InstalledSkill,
    ) -> Result<SkillUninstallResult> {
        let directory = Self::require_valid_directory(&skill.directory)?;
        for app in AppType::all() {
            Self::remove_linked_projection(&directory, &app, true)?;
        }
        db.delete_skill(&skill.id)?;
        set_linked_skill_source(&directory, None)?;
        Ok(SkillUninstallResult {
            backup_path: None,
            preserved_pi_path: None,
            pi_cleanup_incomplete: false,
        })
    }
}

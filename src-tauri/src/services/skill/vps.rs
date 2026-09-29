//! Generated VPS Skills use the ordinary Skill materializer and native ownership hashes.
//! Only deployment receipts live here; desired bindings remain in vps/servers.json.

use super::*;
use crate::services::vps::{check_directory, content_hash, is_link, read_regular_file};
use std::collections::BTreeSet;

pub(crate) const SKILL_ID: &str = "internal:vps";
pub(crate) const DIRECTORY: &str = "cc-switch-vps";
const TEMPLATE: &str = include_str!("vps-template.md");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Deployment {
    app: String,
    path: PathBuf,
    hashes: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipts {
    version: u32,
    deployments: Vec<Deployment>,
}

impl Default for Receipts {
    fn default() -> Self {
        Self {
            version: 1,
            deployments: Vec::new(),
        }
    }
}

struct Candidate {
    app: AppType,
    destination: PathBuf,
    bytes: Vec<u8>,
    hash: String,
}

pub(crate) struct VpsSkillPlan {
    root: PathBuf,
    previous: Option<Vec<u8>>,
    receipts: Receipts,
    current: Option<InstalledSkill>,
    required: SkillApps,
    candidates: Vec<Candidate>,
    inactive: bool,
}

fn app_type(value: &str) -> Result<AppType> {
    let app: AppType = value.parse()?;
    if app.as_str() != value || SkillApps::only(&app).is_empty() {
        return Err(anyhow!("Invalid VPS Skill client in local receipts"));
    }
    Ok(app)
}

fn source_path(root: &Path, app: &str) -> PathBuf {
    root.join("skill-projections").join(app)
}

fn state_path(root: &Path) -> PathBuf {
    root.join("skill-state.json")
}

fn validate_destination(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.file_name().is_none_or(|name| name != DIRECTORY)
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(anyhow!("Invalid VPS Skill deployment path"));
    }
    check_directory(
        path.parent()
            .ok_or_else(|| anyhow!("Missing Skill parent"))?,
    )
}

fn load_receipts(root: &Path) -> Result<(Receipts, Option<Vec<u8>>)> {
    check_directory(root)?;
    check_directory(&root.join("skill-projections"))?;
    let bytes = read_regular_file(&state_path(root))?;
    let receipts: Receipts = match &bytes {
        Some(bytes) => serde_json::from_slice(bytes).context("Invalid VPS Skill receipts")?,
        None => Receipts::default(),
    };
    if receipts.version != 1 {
        return Err(anyhow!("Unsupported VPS Skill receipt version"));
    }
    let mut entries = HashSet::new();
    for receipt in &receipts.deployments {
        app_type(&receipt.app)?;
        validate_destination(&receipt.path)?;
        if receipt.hashes.is_empty()
            || receipt
                .hashes
                .iter()
                .any(|hash| hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
            || !entries.insert((receipt.app.clone(), receipt.path.clone()))
        {
            return Err(anyhow!("Invalid VPS Skill ownership receipts"));
        }
    }
    Ok((receipts, bytes))
}

fn source_hashes(receipts: &Receipts, app: &str) -> BTreeSet<String> {
    receipts
        .deployments
        .iter()
        .filter(|receipt| receipt.app == app)
        .flat_map(|receipt| receipt.hashes.iter().cloned())
        .collect()
}

// A generated Skill has exactly one regular SKILL.md. Any additional entry is a user change.
fn checked_tree_hash(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) if is_link(&metadata) || !metadata.is_dir() => {
            return Err(anyhow!(
                "VPS Skill tree is not an owned directory: {}",
                path.display()
            ));
        }
        Ok(_) => {}
    }
    let entries: Vec<_> = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    if entries.len() != 1 || entries[0].file_name() != "SKILL.md" {
        return Err(anyhow!(
            "VPS Skill tree contains user changes: {}",
            path.display()
        ));
    }
    read_regular_file(&path.join("SKILL.md"))?
        .ok_or_else(|| anyhow!("VPS Skill manifest disappeared"))?;
    SkillService::compute_pi_deployment_hash(path).map(Some)
}

fn verify_source(root: &Path, receipts: &Receipts, app: &str) -> Result<()> {
    let source = source_path(root, app);
    check_directory(&root.join("skill-projections"))?;
    if let Some(hash) = checked_tree_hash(&source)? {
        if !source_hashes(receipts, app).contains(&hash) {
            return Err(anyhow!(
                "VPS Skill source is unowned or modified: {}",
                source.display()
            ));
        }
    }
    Ok(())
}

fn same_projection_target(actual: &Path, expected: &Path) -> bool {
    if SkillService::paths_alias(actual, expected) {
        return true;
    }
    #[cfg(windows)]
    {
        let normalize = |path: &Path| {
            let value = path.to_string_lossy().replace('/', "\\");
            let value = if let Some(unc) = value.strip_prefix("\\\\?\\UNC\\") {
                format!("\\\\{unc}")
            } else {
                value.strip_prefix("\\\\?\\").unwrap_or(&value).to_string()
            };
            value.to_lowercase()
        };
        normalize(actual) == normalize(expected)
    }
    #[cfg(not(windows))]
    {
        actual == expected
    }
}

fn verify_deployment(root: &Path, receipt: &Deployment) -> Result<bool> {
    validate_destination(&receipt.path)?;
    let metadata = match fs::symlink_metadata(&receipt.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        // CC Switch always creates absolute links for generated projections.
        if !same_projection_target(
            &fs::read_link(&receipt.path)?,
            &source_path(root, &receipt.app),
        ) {
            return Err(anyhow!(
                "VPS Skill link target changed: {}",
                receipt.path.display()
            ));
        }
    } else if is_link(&metadata)
        || checked_tree_hash(&receipt.path)?.is_none_or(|hash| !receipt.hashes.contains(&hash))
    {
        return Err(anyhow!(
            "VPS Skill deployment is unowned or modified: {}",
            receipt.path.display()
        ));
    }
    Ok(true)
}

fn write_receipts(root: &Path, previous: Option<&[u8]>, receipts: &Receipts) -> Result<Vec<u8>> {
    check_directory(root)?;
    let path = state_path(root);
    let actual = read_regular_file(&path)?;
    if actual.as_deref() != previous {
        return Err(anyhow!("VPS Skill receipts changed during reconciliation"));
    }
    let next = crate::config::sorted_json_bytes(receipts)?;
    if actual.as_deref() != Some(next.as_slice()) {
        crate::config::atomic_write_private(&path, &next)?;
    }
    Ok(next)
}

#[cfg(all(test, windows))]
mod path_tests {
    use super::*;

    #[test]
    fn generated_symlink_targets_accept_windows_verbatim_prefixes() {
        assert!(same_projection_target(
            Path::new(r"\\?\C:\cc-switch-test\skill-projections\claude"),
            Path::new(r"C:\cc-switch-test\skill-projections\claude"),
        ));
        assert!(!same_projection_target(
            Path::new(r"\\?\C:\cc-switch-test\skill-projections\codex"),
            Path::new(r"C:\cc-switch-test\skill-projections\claude"),
        ));
    }
}

pub(crate) fn local_owned_paths() -> Result<Vec<PathBuf>> {
    let root = get_app_config_dir().join("vps");
    let (receipts, _) = load_receipts(&root)?;
    Ok(receipts
        .deployments
        .into_iter()
        .map(|receipt| receipt.path)
        .collect())
}

pub(crate) fn is_generated_manifest(path: &Path) -> bool {
    read_regular_file(path).ok().flatten().is_some_and(|bytes| {
        String::from_utf8_lossy(&bytes)
            .lines()
            .take(8)
            .any(|line| line == "cc-switch-generated: vps")
    })
}

pub(crate) fn reject_generated_manifest(path: &Path, directory: &str) -> Result<()> {
    if is_generated_manifest(path) {
        return Err(anyhow!(format_skill_error(
            "SKILL_MANAGED_BY_VPS",
            &[("directory", directory)],
            Some("manageInVps")
        )));
    }
    Ok(())
}

impl SkillService {
    /// Caller holds the Skill state write guard, then the VPS document lock.
    pub(crate) fn prepare_vps_skill(
        db: &Arc<Database>,
        root: &Path,
        required: &SkillApps,
    ) -> Result<VpsSkillPlan> {
        let (receipts, previous) = load_receipts(root)?;
        let current = db.get_installed_skill(SKILL_ID)?;
        let inactive = required.is_empty()
            && receipts.deployments.is_empty()
            && current
                .as_ref()
                .is_none_or(|skill| skill.managed_by.as_deref() != Some("vps"));
        let mut plan = VpsSkillPlan {
            root: root.to_path_buf(),
            previous,
            receipts,
            current,
            required: required.clone(),
            candidates: Vec::new(),
            inactive,
        };
        if inactive {
            return Ok(plan);
        }

        for skill in db.get_all_installed_skills()?.values() {
            if (skill.id == SKILL_ID || skill.directory.eq_ignore_ascii_case(DIRECTORY))
                && (skill.id != SKILL_ID
                    || skill.directory != DIRECTORY
                    || skill.managed_by.as_deref() != Some("vps"))
            {
                return Err(anyhow!(
                    "VPS Skill conflicts with a user-managed Skill; use a different ID/directory"
                ));
            }
        }
        let ssot = Self::get_ssot_dir()?;
        if Self::paths_overlap(root, &ssot) {
            return Err(anyhow!(
                "Local VPS data must not overlap portable Skill storage"
            ));
        }
        if fs::symlink_metadata(ssot.join(DIRECTORY)).is_ok() {
            return Err(anyhow!(
                "VPS Skill conflicts with an existing SSOT directory; it will not be adopted"
            ));
        }
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut apps = BTreeSet::new();
        for receipt in &plan.receipts.deployments {
            apps.insert(receipt.app.clone());
            verify_deployment(root, receipt)?;
            paths.push(receipt.path.clone());
        }
        for app in required.enabled_apps() {
            let destination = Self::get_app_skills_dir(&app)?.join(DIRECTORY);
            validate_destination(&destination)?;
            if Self::paths_overlap(&destination, root) || Self::paths_overlap(&destination, &ssot) {
                return Err(anyhow!(
                    "VPS Skill destination overlaps local data or Skill storage"
                ));
            }
            if !plan
                .receipts
                .deployments
                .iter()
                .any(|receipt| receipt.app == app.as_str() && receipt.path == destination)
                && fs::symlink_metadata(&destination).is_ok()
            {
                return Err(anyhow!(
                    "VPS Skill destination already exists without ownership: {}",
                    destination.display()
                ));
            }
            apps.insert(app.as_str().to_string());
            if !paths.contains(&destination) {
                paths.push(destination.clone());
            }
            let catalog = root.join(format!("clients/{}.json", app.as_str()));
            let bytes = format!("{TEMPLATE}\n## Client catalog\n\nRead this exact path (JSON string notation):\n\n```json\n{}\n```\n", serde_json::to_string(&catalog)?).into_bytes();
            let temp = tempfile::tempdir()?;
            crate::config::atomic_write_private(&temp.path().join("SKILL.md"), &bytes)?;
            let hash = Self::compute_pi_deployment_hash(temp.path())?;
            plan.candidates.push(Candidate {
                app,
                destination,
                bytes,
                hash,
            });
        }
        for (index, path) in paths.iter().enumerate() {
            if paths
                .iter()
                .skip(index + 1)
                .any(|other| Self::paths_overlap(path, other))
            {
                return Err(anyhow!("VPS Skill client destinations overlap"));
            }
        }
        // Distinct clients cannot share one path even if both had pending receipts.
        for (index, candidate) in plan.candidates.iter().enumerate() {
            if plan
                .candidates
                .iter()
                .skip(index + 1)
                .any(|other| Self::paths_overlap(&candidate.destination, &other.destination))
            {
                return Err(anyhow!("VPS Skill client destinations overlap"));
            }
        }
        for app in apps {
            verify_source(root, &plan.receipts, &app)?;
        }
        Ok(plan)
    }

    pub(crate) fn apply_vps_skill(db: &Arc<Database>, mut plan: VpsSkillPlan) -> Result<()> {
        if plan.inactive {
            return Ok(());
        }
        let mut pending = plan.receipts.clone();
        for candidate in &plan.candidates {
            for receipt in pending
                .deployments
                .iter_mut()
                .filter(|receipt| receipt.app == candidate.app.as_str())
            {
                receipt.hashes.insert(candidate.hash.clone());
            }
            match pending.deployments.iter_mut().find(|receipt| {
                receipt.app == candidate.app.as_str() && receipt.path == candidate.destination
            }) {
                Some(receipt) => {
                    receipt.hashes.insert(candidate.hash.clone());
                }
                None => pending.deployments.push(Deployment {
                    app: candidate.app.as_str().into(),
                    path: candidate.destination.clone(),
                    hashes: BTreeSet::from([candidate.hash.clone()]),
                }),
            }
        }
        let intent = write_receipts(&plan.root, plan.previous.as_deref(), &pending)?;
        if !plan.required.is_empty() {
            let hash = content_hash(TEMPLATE.as_bytes());
            if plan
                .current
                .as_ref()
                .is_none_or(|skill| skill.content_hash.as_deref() != Some(&hash))
            {
                let skill = InstalledSkill {
                    id: SKILL_ID.into(),
                    name: "CC Switch VPS".into(),
                    description: Some("VPS access managed by CC Switch".into()),
                    directory: DIRECTORY.into(),
                    repo_owner: None,
                    repo_name: None,
                    repo_branch: None,
                    readme_url: None,
                    apps: plan
                        .current
                        .as_ref()
                        .map(|skill| skill.apps.clone())
                        .unwrap_or_default(),
                    installed_at: plan
                        .current
                        .as_ref()
                        .map(|skill| skill.installed_at)
                        .unwrap_or_else(|| Utc::now().timestamp()),
                    content_hash: Some(hash),
                    updated_at: if plan.current.is_some() {
                        Utc::now().timestamp()
                    } else {
                        0
                    },
                    managed_by: Some("vps".into()),
                };
                db.save_skill(&skill)?;
                plan.current = Some(skill);
            }
        }
        for candidate in &plan.candidates {
            let source = source_path(&plan.root, candidate.app.as_str());
            verify_source(&plan.root, &pending, candidate.app.as_str())?;
            check_directory(&source)?;
            let path = source.join("SKILL.md");
            if read_regular_file(&path)?.as_deref() != Some(candidate.bytes.as_slice()) {
                crate::config::atomic_write_private(&path, &candidate.bytes)?;
            }
        }
        for receipt in &pending.deployments {
            if plan.candidates.iter().any(|candidate| {
                candidate.app.as_str() == receipt.app && candidate.destination == receipt.path
            }) {
                continue;
            }
            verify_source(&plan.root, &pending, &receipt.app)?;
            if verify_deployment(&plan.root, receipt)? {
                Self::remove_path(&receipt.path)?;
            }
        }
        for candidate in &plan.candidates {
            let receipt = pending
                .deployments
                .iter()
                .find(|receipt| {
                    receipt.app == candidate.app.as_str() && receipt.path == candidate.destination
                })
                .unwrap();
            let source = source_path(&plan.root, candidate.app.as_str());
            let exists = verify_deployment(&plan.root, receipt)?;
            let method = Self::get_sync_method();
            let unchanged = exists
                && if Self::is_symlink(&candidate.destination) {
                    method != SyncMethod::Copy
                } else {
                    method != SyncMethod::Symlink
                        && checked_tree_hash(&candidate.destination)?.as_deref()
                            == Some(&candidate.hash)
                };
            if !unchanged {
                fs::create_dir_all(candidate.destination.parent().unwrap())?;
                Self::materialize_skill_source(
                    &source,
                    &candidate.destination,
                    DIRECTORY,
                    &candidate.app,
                )?;
            }
        }
        for app in pending
            .deployments
            .iter()
            .map(|receipt| receipt.app.clone())
            .collect::<BTreeSet<_>>()
        {
            if plan.required.is_enabled_for(&app_type(&app)?) {
                continue;
            }
            verify_source(&plan.root, &pending, &app)?;
            let source = source_path(&plan.root, &app);
            if source.exists() {
                Self::remove_path(&source)?;
            }
        }
        if plan.required.is_empty() {
            if plan.current.is_some() {
                db.delete_skill(SKILL_ID)?;
            }
        } else {
            let mut persisted_apps = plan.required;
            persisted_apps.pi = false; // Pi still follows native file presence, never a persisted enable flag.
            if plan
                .current
                .as_ref()
                .is_none_or(|skill| skill.apps != persisted_apps)
            {
                db.update_skill_apps(SKILL_ID, &persisted_apps)?;
            }
        }
        let completed = Receipts {
            version: 1,
            deployments: plan
                .candidates
                .into_iter()
                .map(|candidate| Deployment {
                    app: candidate.app.as_str().into(),
                    path: candidate.destination,
                    hashes: BTreeSet::from([candidate.hash]),
                })
                .collect(),
        };
        write_receipts(&plan.root, Some(&intent), &completed)?;
        Ok(())
    }
}

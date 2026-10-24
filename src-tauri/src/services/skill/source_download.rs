//! Download complete installed skill sources without unrelated repository assets.

use super::*;
use futures::{StreamExt, TryStreamExt};
use sha1::{Digest, Sha1};
use std::io::{Read, Seek, SeekFrom, Write};

const SOURCE_DOWNLOAD_TIMEOUT_SECONDS: u64 = 180;

struct SourceSnapshot {
    repo: SkillRepo,
    commit: String,
    tree: GithubDiscoveryTree,
}

struct SelectedSource {
    prefix: String,
    local_dir: PathBuf,
}

fn source_error(reason: &str) -> anyhow::Error {
    anyhow!(format_skill_error(
        "DISCOVERY_METADATA_INVALID",
        &[("reason", reason)],
        Some("checkRepoUrl"),
    ))
}

fn valid_git_object_id(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_source_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 1024
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.ends_with([' ', '.'])
                || windows_device_component(part)
        })
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(source_error("invalidSourcePath"));
    }
    Ok(())
}

fn windows_device_component(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or(part)
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

fn path_in_source(path: &str, source: &str) -> bool {
    source.is_empty()
        || path == source
        || path
            .strip_prefix(source)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn source_raw_url(repo: &SkillRepo, commit: &str, path: &str) -> Result<String> {
    SkillService::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;
    validate_source_path(path)?;
    if !valid_git_object_id(commit) {
        return Err(source_error("invalidCommit"));
    }
    let mut url = reqwest::Url::parse("https://raw.githubusercontent.com/")?;
    url.path_segments_mut()
        .map_err(|_| source_error("invalidRawUrl"))?
        .pop_if_empty()
        .extend([repo.owner.as_str(), repo.name.as_str(), commit])
        .extend(path.split('/'));
    Ok(url.to_string())
}

fn api_url(repo: &SkillRepo, parts: &[&str]) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse("https://api.github.com/repos/")?;
    url.path_segments_mut()
        .map_err(|_| source_error("invalidApiUrl"))?
        .pop_if_empty()
        .extend([repo.owner.as_str(), repo.name.as_str()])
        .extend(parts);
    Ok(url)
}

/// Git object identity is used only to verify reusable bytes. Installed content
/// hashes continue to use the existing SHA-256 directory algorithm.
fn blob_matches(file: &mut fs::File, size: u64, expected_sha: &str) -> Result<bool> {
    if !valid_git_object_id(expected_sha) {
        return Err(source_error("invalidBlobId"));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha1::new();
    hash.update(format!("blob {size}\0").as_bytes());
    let mut buffer = [0u8; 16 * 1024];
    let mut received = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        received = received.saturating_add(count as u64);
        if received > size {
            file.seek(SeekFrom::Start(0))?;
            return Ok(false);
        }
        hash.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(received == size && format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected_sha))
}

fn blob_identity(entry: &GithubDiscoveryEntry) -> Result<(u64, &str)> {
    let size = entry.size.ok_or_else(|| source_error("missingBlobSize"))?;
    let sha = entry
        .sha
        .as_deref()
        .filter(|sha| valid_git_object_id(sha))
        .ok_or_else(|| source_error("invalidBlobId"))?;
    if size > MAX_ARCHIVE_TOTAL_BYTES {
        return Err(anyhow!(format_skill_error(
            "ARCHIVE_TOO_LARGE",
            &[("limit_mb", "512")],
            Some("checkRepoUrl")
        )));
    }
    Ok((size, sha))
}

/// Return a private, verified snapshot, never a handle to a live local file.
/// This avoids a local edit racing the subsequent copy into the staged source.
fn verified_local_blob(path: &Path, entry: &GithubDiscoveryEntry) -> Result<Option<fs::File>> {
    let (size, sha) = blob_identity(entry)?;
    let Ok(mut input) = fs::File::open(path) else {
        return Ok(None);
    };
    let Ok(metadata) = input.metadata() else {
        return Ok(None);
    };
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE_TOTAL_BYTES {
        return Ok(None);
    }
    if metadata.len() == size {
        let mut candidate = tempfile::tempfile()?;
        std::io::copy(
            &mut (&mut input).take(size.saturating_add(1)),
            &mut candidate,
        )?;
        if blob_matches(&mut candidate, size, sha)? {
            return Ok(Some(candidate));
        }
    }
    // CRLF normalization is only a candidate. Accept it only if its exact size
    // and Git blob ID agree with the authoritative remote tree.
    input.seek(SeekFrom::Start(0))?;
    let mut candidate = tempfile::tempfile()?;
    let mut input_buffer = [0u8; 16 * 1024];
    let mut output_buffer = Vec::with_capacity(input_buffer.len() + 1);
    let mut pending_cr = false;
    let mut written = 0u64;
    loop {
        let count = input.read(&mut input_buffer)?;
        if count == 0 {
            break;
        }
        output_buffer.clear();
        for byte in &input_buffer[..count] {
            if pending_cr {
                pending_cr = false;
                if *byte == b'\n' {
                    output_buffer.push(b'\n');
                    continue;
                }
                output_buffer.push(b'\r');
            }
            if *byte == b'\r' {
                pending_cr = true;
            } else {
                output_buffer.push(*byte);
            }
        }
        written = written.saturating_add(output_buffer.len() as u64);
        if written > size {
            return Ok(None);
        }
        candidate.write_all(&output_buffer)?;
    }
    if pending_cr {
        candidate.write_all(b"\r")?;
    }
    Ok(blob_matches(&mut candidate, size, sha)?.then_some(candidate))
}

async fn fetch_source_file(url: String, limit: u64) -> Result<fs::File> {
    let mut response = crate::proxy::http_client::get()
        .get(url)
        .header(reqwest::header::USER_AGENT, "CC-Switch-Skill-Update")
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status().as_u16().to_string();
        return Err(anyhow!(format_skill_error(
            "DOWNLOAD_FAILED",
            &[("status", &status)],
            match status.as_str() {
                "403" => Some("http403"),
                "404" => Some("http404"),
                "429" => Some("http429"),
                _ => Some("checkNetwork"),
            }
        )));
    }
    let mut file = tempfile::tempfile()?;
    let mut received = 0u64;
    while let Some(chunk) = response.chunk().await? {
        received = received.saturating_add(chunk.len() as u64);
        if received > limit {
            return Err(source_error("blobSizeMismatch"));
        }
        file.write_all(&chunk)?;
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}

async fn fetch_source_snapshot<F, Fut>(repo: &SkillRepo, fetch: F) -> Result<SourceSnapshot>
where
    F: Fn(String, u64) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>>>,
{
    SkillService::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;
    let mut branches = Vec::new();
    if repo.branch.is_empty() || repo.branch.eq_ignore_ascii_case("HEAD") {
        let bytes = fetch(api_url(repo, &[])?.to_string(), 1024 * 1024).await?;
        let info: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| source_error("repositoryInfo"))?;
        let branch = info["default_branch"]
            .as_str()
            .filter(|branch| !branch.is_empty() && !branch.eq_ignore_ascii_case("HEAD"))
            .ok_or_else(|| source_error("defaultBranch"))?;
        SkillService::validate_repo_ref(&repo.owner, &repo.name, branch)?;
        branches.push(branch.to_string());
    } else {
        branches.push(repo.branch.clone());
    }
    for fallback in ["main", "master"] {
        if !branches.iter().any(|branch| branch == fallback) {
            branches.push(fallback.to_string());
        }
    }
    let mut last_error = None;
    for branch in branches {
        let bytes = match fetch(
            api_url(repo, &["git", "ref", "heads", &branch])?.to_string(),
            1024 * 1024,
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(error) if SkillService::is_branch_not_found(&error) => {
                last_error = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        let reference: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| source_error("branchReference"))?;
        if reference["object"]["type"].as_str() != Some("commit") {
            return Err(source_error("invalidCommit"));
        }
        let commit = reference["object"]["sha"]
            .as_str()
            .filter(|sha| valid_git_object_id(sha))
            .ok_or_else(|| source_error("invalidCommit"))?
            .to_string();
        let mut url = api_url(repo, &["git", "trees", &commit])?;
        url.query_pairs_mut().append_pair("recursive", "1");
        let bytes = fetch(url.to_string(), MAX_DISCOVERY_TREE_BYTES).await?;
        let tree: GithubDiscoveryTree =
            serde_json::from_slice(&bytes).map_err(|_| source_error("repositoryTree"))?;
        if tree.truncated {
            return Err(anyhow!(format_skill_error(
                "DISCOVERY_TREE_TRUNCATED",
                &[],
                Some("retryLater")
            )));
        }
        let mut resolved = repo.clone();
        resolved.branch = branch;
        return Ok(SourceSnapshot {
            repo: resolved,
            commit,
            tree,
        });
    }
    Err(last_error.unwrap_or_else(|| source_error("missingBranch")))
}

async fn select_sources<F, Fut>(
    snapshot: &SourceSnapshot,
    targets: &[InstalledSkill],
    local_root: &Path,
    fetch: F,
) -> Result<Option<Vec<SelectedSource>>>
where
    F: Fn(String, u64) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>>>,
{
    let mut paths = Vec::new();
    let mut catalog = Vec::new();
    for entry in &snapshot.tree.tree {
        if entry.kind != "blob"
            || !matches!(entry.mode.as_str(), "100644" | "100755")
            || !(entry.path == "SKILL.md" || entry.path.ends_with("/SKILL.md"))
        {
            continue;
        }
        validate_source_path(&entry.path)?;
        let directory = entry
            .path
            .strip_suffix("/SKILL.md")
            .unwrap_or(&snapshot.repo.name);
        catalog.push(SkillService::skill_from_metadata(
            SkillMetadata {
                name: None,
                description: None,
            },
            directory,
            &entry.path,
            &snapshot.repo,
        ));
        paths.push(entry.path.clone());
    }
    let mut resolved = targets
        .iter()
        .map(|target| {
            SkillService::find_remote_skill_for_install(
                &catalog,
                &target.directory,
                target.readme_url.as_deref(),
            )
            .cloned()
        })
        .collect::<Vec<_>>();
    if resolved.iter().any(Option::is_none) {
        // An unexpanded directory symlink may itself be a skill source.
        if snapshot
            .tree
            .tree
            .iter()
            .any(|entry| entry.mode == "120000")
        {
            return Ok(None);
        }
        let mut pinned = snapshot.repo.clone();
        pinned.branch = snapshot.commit.clone();
        let metadata_catalog = SkillService::fetch_discovery_manifests(&pinned, paths, |url| {
            fetch(url, MAX_DISCOVERY_MANIFEST_BYTES)
        })
        .await?;
        // Metadata is a fallback only. A pinned-commit catalog must not change
        // the already-established source of another target in the same group.
        for (target, remote) in targets.iter().zip(&mut resolved) {
            if remote.is_none() {
                *remote = SkillService::find_remote_skill_for_install(
                    &metadata_catalog,
                    &target.directory,
                    target.readme_url.as_deref(),
                )
                .cloned();
            }
        }
    }
    let mut selected = Vec::new();
    for (target, remote) in targets.iter().zip(resolved) {
        let directory = SkillService::require_valid_directory(&target.directory)?;
        let Some(remote) = remote else {
            continue;
        };
        // This URL was constructed from the resolved repository ref above.
        // Strip that exact ref: branches can contain more than one path segment.
        let doc_prefix = format!(
            "https://github.com/{}/{}/blob/{}/",
            remote.repo_owner, remote.repo_name, remote.repo_branch
        );
        let doc_path = remote
            .readme_url
            .as_deref()
            .and_then(|url| url.strip_prefix(&doc_prefix))
            .ok_or_else(|| source_error("missingSourcePath"))?;
        let prefix = doc_path.strip_suffix("/SKILL.md").unwrap_or("").to_string();
        if prefix.is_empty() {
            return Ok(None);
        } // A root skill really includes the repository.
        validate_source_path(&prefix)?;
        if snapshot.tree.tree.iter().any(|entry| {
            path_in_source(&entry.path, &prefix)
                && (entry.mode == "120000" || entry.kind == "commit")
        }) {
            return Ok(None);
        }
        selected.push(SelectedSource {
            prefix,
            local_dir: local_root.join(directory),
        });
    }
    Ok(Some(selected))
}

fn selected_files<'a>(
    tree: &'a GithubDiscoveryTree,
    selected: &[SelectedSource],
) -> Result<Vec<&'a GithubDiscoveryEntry>> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut declared_bytes = 0u64;
    for entry in &tree.tree {
        if !selected
            .iter()
            .any(|source| path_in_source(&entry.path, &source.prefix))
        {
            continue;
        }
        validate_source_path(&entry.path)?;
        if !seen.insert(entry.path.to_lowercase()) {
            return Err(source_error("duplicateSourcePath"));
        }
        if seen.len() > MAX_ARCHIVE_ENTRIES {
            return Err(anyhow!(format_skill_error(
                "ARCHIVE_TOO_MANY_ENTRIES",
                &[
                    ("count", &seen.len().to_string()),
                    ("limit", &MAX_ARCHIVE_ENTRIES.to_string())
                ],
                Some("checkRepoUrl")
            )));
        }
        if entry.kind == "tree" {
            continue;
        }
        if entry.kind != "blob" || !matches!(entry.mode.as_str(), "100644" | "100755") {
            return Err(source_error("unsupportedSourceEntry"));
        }
        let (size, _) = blob_identity(entry)?;
        SkillService::charge_archive_budget(&mut declared_bytes, size.max(FILE_ENTRY_BUDGET_COST))?;
        files.push(entry);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn local_candidate(
    entry: &GithubDiscoveryEntry,
    selected: &[SelectedSource],
) -> Result<Option<fs::File>> {
    for source in selected {
        let Some(relative) = entry.path.strip_prefix(&format!("{}/", source.prefix)) else {
            continue;
        };
        let candidate = source.local_dir.join(relative);
        let (Ok(root), Ok(canonical)) = (source.local_dir.canonicalize(), candidate.canonicalize())
        else {
            continue;
        };
        if !canonical.starts_with(&root) {
            continue;
        }
        if let Some(file) = verified_local_blob(&canonical, entry)? {
            return Ok(Some(file));
        }
    }
    Ok(None)
}

/// The TempDir is owned by this future, including every failure/cancellation path.
async fn materialize_sources<F, Fut>(
    snapshot: &SourceSnapshot,
    selected: &[SelectedSource],
    staging: tempfile::TempDir,
    fetch: F,
) -> Result<tempfile::TempDir>
where
    F: Fn(String, u64) -> Fut,
    Fut: std::future::Future<Output = Result<fs::File>>,
{
    // Own each queued entry so the Send future exposed through Tauri does not
    // retain an iterator closure whose argument has a borrowed tree lifetime.
    let files = selected_files(&snapshot.tree, selected)?
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut total_bytes = 0u64;
    for entry in &snapshot.tree.tree {
        if entry.kind == "tree"
            && selected
                .iter()
                .any(|source| path_in_source(&entry.path, &source.prefix))
        {
            SkillService::create_dir_all_within_budget(
                &staging.path().join(&entry.path),
                &mut total_bytes,
            )?;
        }
    }
    for entry in &files {
        if let Some(parent) = staging.path().join(&entry.path).parent() {
            SkillService::create_dir_all_within_budget(parent, &mut total_bytes)?;
        }
    }
    let requests = files.into_iter().map(|entry| {
        let fetch = &fetch;
        async move {
            let (size, sha) = blob_identity(&entry)?;
            let (mut file, reused) = match local_candidate(&entry, selected)? {
                Some(file) => (file, true),
                None => (
                    fetch(
                        source_raw_url(&snapshot.repo, &snapshot.commit, &entry.path)?,
                        size,
                    )
                    .await?,
                    false,
                ),
            };
            if !blob_matches(&mut file, size, sha)? {
                return Err(source_error("blobContentMismatch"));
            }
            Ok::<_, anyhow::Error>((entry, file, reused))
        }
    });
    let mut downloads =
        futures::stream::iter(requests).buffer_unordered(DISCOVERY_METADATA_CONCURRENCY);
    let mut reused_files = 0usize;
    let mut downloaded_files = 0usize;
    while let Some((entry, mut file, reused)) = downloads.try_next().await? {
        SkillService::write_file_within_budget(
            &mut file,
            &staging.path().join(&entry.path),
            &mut total_bytes,
        )?;
        if reused {
            reused_files += 1;
        } else {
            downloaded_files += 1;
        }
    }
    log::info!("Skill source snapshot {}/{}: {reused_files} verified local files reused, {downloaded_files} files downloaded", snapshot.repo.owner, snapshot.repo.name);
    Ok(staging)
}

impl SkillService {
    pub(super) async fn download_repo_for_skills(
        &self,
        repo: &SkillRepo,
        targets: &[InstalledSkill],
    ) -> Result<(tempfile::TempDir, String)> {
        Self::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;
        timeout(
            std::time::Duration::from_secs(SOURCE_DOWNLOAD_TIMEOUT_SECONDS),
            async {
                #[cfg(test)]
                if self.repo_fixture.is_some() {
                    return self.download_repo(repo).await;
                }

                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
                let cache_dir = get_app_config_dir().join("cache/skill-discovery");
                let route = Self::read_discovery_plan(&cache_dir, repo, now);
                if let Some(plan) = route.as_ref().filter(|plan| plan.manifest_paths.is_none()) {
                    let mut resolved_repo = repo.clone();
                    resolved_repo.branch = plan.resolved_branch.clone();
                    return timeout(
                        std::time::Duration::from_secs(60),
                        self.download_repo(&resolved_repo),
                    )
                    .await
                    .map_err(|_| anyhow!(Self::download_timeout_error(repo)))?;
                }
                let snapshot = match fetch_source_snapshot(repo, Self::fetch_discovery_bytes).await
                {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        if route.is_some() {
                            return Err(error);
                        }
                        log::warn!("Skill source preflight failed; trying archive: {error}");
                        return timeout(
                            std::time::Duration::from_secs(60),
                            self.download_repo(repo),
                        )
                        .await
                        .map_err(|_| anyhow!(Self::download_timeout_error(repo)))?;
                    }
                };
                let size = snapshot
                    .tree
                    .tree
                    .iter()
                    .filter(|entry| entry.kind == "blob")
                    .fold(0u64, |total, entry| {
                        total.saturating_add(entry.size.unwrap_or(0))
                    });
                if size <= DISCOVERY_ARCHIVE_THRESHOLD_BYTES {
                    let plan = SkillDiscoveryPlan {
                        version: 1,
                        fetched_at: now,
                        resolved_branch: snapshot.repo.branch.clone(),
                        manifest_paths: None,
                    };
                    if let Err(error) = Self::write_discovery_plan(&cache_dir, repo, &plan) {
                        log::debug!("Could not cache source route: {error}");
                    }
                    return timeout(
                        std::time::Duration::from_secs(60),
                        self.download_repo(&snapshot.repo),
                    )
                    .await
                    .map_err(|_| anyhow!(Self::download_timeout_error(repo)))?;
                }
                let local_root = Self::get_ssot_dir()?;
                let Some(selected) =
                    select_sources(&snapshot, targets, &local_root, Self::fetch_discovery_bytes)
                        .await?
                else {
                    return self.download_repo(&snapshot.repo).await;
                };
                let staged = materialize_sources(
                    &snapshot,
                    &selected,
                    tempfile::tempdir()?,
                    fetch_source_file,
                )
                .await?;
                Ok((staged, snapshot.repo.branch))
            },
        )
        .await
        .map_err(|_| {
            anyhow!(format_skill_error(
                "DOWNLOAD_TIMEOUT",
                &[
                    ("owner", &repo.owner),
                    ("name", &repo.name),
                    ("timeout", "180")
                ],
                Some("checkNetwork")
            ))
        })?
        .map_err(|error| Self::with_repo_context(error, repo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest, Sha1};

    fn blob(path: &str, bytes: &[u8]) -> GithubDiscoveryEntry {
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", bytes.len()).as_bytes());
        hash.update(bytes);
        GithubDiscoveryEntry {
            path: path.to_string(),
            kind: "blob".to_string(),
            mode: "100644".to_string(),
            size: Some(bytes.len() as u64),
            sha: Some(format!("{:x}", hash.finalize())),
        }
    }

    #[test]
    fn source_download_selection_observes_directory_boundaries() {
        assert!(path_in_source("skills/foo/SKILL.md", "skills/foo"));
        assert!(path_in_source("skills/foo/.hidden", "skills/foo"));
        assert!(!path_in_source("skills/foobar/SKILL.md", "skills/foo"));
        assert!(!path_in_source("skills/foo.md", "skills/foo"));
        assert!(path_in_source("SKILL.md", ""));
    }

    #[test]
    fn source_download_reuses_only_verified_remote_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("SKILL.md");
        let remote = b"---\nname: example\n---\n";
        let entry = blob("skills/example/SKILL.md", remote);
        fs::write(&path, remote).unwrap();
        let mut reused = verified_local_blob(&path, &entry)
            .unwrap()
            .expect("matching raw bytes");
        let mut bytes = Vec::new();
        reused.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, remote);

        fs::write(&path, b"---\r\nname: example\r\n---\r\n").unwrap();
        let mut reused = verified_local_blob(&path, &entry)
            .unwrap()
            .expect("verified CRLF normalization");
        bytes.clear();
        reused.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, remote);

        fs::write(&path, b"---\r\nname: changed\r\n---\r\n").unwrap();
        assert!(verified_local_blob(&path, &entry).unwrap().is_none());
        assert!(verified_local_blob(&temp.path().join("missing"), &entry)
            .unwrap()
            .is_none());
    }

    fn fixture_repo() -> SkillRepo {
        SkillRepo {
            owner: "owner".into(),
            name: "repo".into(),
            branch: "main".into(),
            enabled: true,
        }
    }

    fn fixture_snapshot(entries: Vec<GithubDiscoveryEntry>) -> SourceSnapshot {
        SourceSnapshot {
            repo: fixture_repo(),
            commit: "0123456789abcdef0123456789abcdef01234567".into(),
            tree: GithubDiscoveryTree {
                tree: entries,
                truncated: false,
            },
        }
    }

    fn fixture_file(bytes: &[u8]) -> fs::File {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(bytes).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file
    }

    fn installed(directory: &str, doc: &str) -> InstalledSkill {
        InstalledSkill {
            id: format!("owner/repo:{directory}"),
            name: directory.into(),
            description: None,
            directory: directory.into(),
            repo_owner: Some("owner".into()),
            repo_name: Some("repo".into()),
            repo_branch: Some("main".into()),
            readme_url: Some(format!("https://github.com/owner/repo/blob/main/{doc}")),
            apps: SkillApps::default(),
            installed_at: 1,
            content_hash: None,
            updated_at: 0,
        }
    }

    #[test]
    fn source_download_rejects_unsafe_paths_and_unbounded_blobs() {
        for path in [
            "../secret",
            "/absolute",
            "a/../secret",
            "a\\b",
            "C:/secret",
            "a:file",
            "a//b",
            "a/./b",
            "a./b",
            "a\n/b",
            "skills/NUL",
            "skills/con.txt",
            "skills/COM1",
            "LPT9/file",
            "COM¹.txt",
            "LPT²/file",
            "skills/NUL .txt",
        ] {
            assert!(validate_source_path(path).is_err(), "{path}");
        }
        assert!(validate_source_path(".claude/skills/example/.config").is_ok());
        let mut oversized = blob("skills/example/data", b"data");
        oversized.size = Some(MAX_ARCHIVE_TOTAL_BYTES + 1);
        assert!(blob_identity(&oversized).is_err());
        oversized.size = Some(4);
        oversized.sha = Some("../../not-a-sha".into());
        assert!(blob_identity(&oversized).is_err());
    }

    #[tokio::test]
    async fn source_download_snapshot_and_raw_requests_pin_the_commit() {
        let expected = "0123456789abcdef0123456789abcdef01234567";
        let calls = std::sync::Mutex::new(Vec::new());
        let snapshot = fetch_source_snapshot(&fixture_repo(), |url, _| {
            calls.lock().unwrap().push(url.clone());
            let bytes = if url.ends_with("/git/ref/heads/main") {
                serde_json::to_vec(
                    &serde_json::json!({ "object": { "type": "commit", "sha": expected } }),
                )
                .unwrap()
            } else {
                assert!(url.ends_with(&format!("/git/trees/{expected}?recursive=1")));
                br#"{"tree":[],"truncated":false}"#.to_vec()
            };
            std::future::ready(Ok(bytes))
        })
        .await
        .unwrap();
        assert_eq!(snapshot.commit, expected);
        assert_eq!(snapshot.repo.branch, "main");
        assert_eq!(calls.lock().unwrap().len(), 2);
        assert_eq!(
            source_raw_url(&snapshot.repo, &snapshot.commit, "skills/a/SKILL.md").unwrap(),
            format!("https://raw.githubusercontent.com/owner/repo/{expected}/skills/a/SKILL.md")
        );
        let error = fetch_source_snapshot(&fixture_repo(), |url, _| {
            std::future::ready(Ok(if url.ends_with("/git/ref/heads/main") {
                serde_json::to_vec(
                    &serde_json::json!({ "object": { "type": "commit", "sha": expected } }),
                )
                .unwrap()
            } else {
                br#"{"tree":[],"truncated":true}"#.to_vec()
            }))
        })
        .await
        .err()
        .expect("truncated snapshot must fail");
        assert!(error.to_string().contains("DISCOVERY_TREE_TRUNCATED"));
    }

    #[tokio::test]
    async fn source_download_missing_branch_ref_falls_back_to_main() {
        let mut repo = fixture_repo();
        repo.branch = "master".into();
        let expected = "0123456789abcdef0123456789abcdef01234567";
        let calls = std::sync::Mutex::new(Vec::new());
        let snapshot = fetch_source_snapshot(&repo, |url, _| {
            calls.lock().unwrap().push(url.clone());
            std::future::ready(if url.ends_with("/git/ref/heads/master") {
                Err(anyhow!(format_skill_error(
                    "DOWNLOAD_FAILED",
                    &[("status", "404")],
                    Some("http404")
                )))
            } else if url.ends_with("/git/ref/heads/main") {
                Ok(serde_json::to_vec(
                    &serde_json::json!({ "object": { "type": "commit", "sha": expected } }),
                )
                .unwrap())
            } else {
                assert!(url.ends_with(&format!("/git/trees/{expected}?recursive=1")));
                Ok(br#"{"tree":[],"truncated":false}"#.to_vec())
            })
        })
        .await
        .unwrap();
        assert_eq!(snapshot.repo.branch, "main");
        assert_eq!(snapshot.commit, expected);
        assert_eq!(calls.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn source_download_branch_ref_does_not_retry_unrelated_errors() {
        for status in ["422", "403", "429"] {
            let calls = std::sync::Mutex::new(0usize);
            let error = fetch_source_snapshot(&fixture_repo(), |_, _| {
                *calls.lock().unwrap() += 1;
                std::future::ready(Err(anyhow!(format_skill_error(
                    "DOWNLOAD_FAILED",
                    &[("status", status)],
                    Some("checkNetwork")
                ))))
            })
            .await
            .err()
            .expect("unrelated branch lookup errors must propagate");
            let parsed: serde_json::Value = serde_json::from_str(&error.to_string()).unwrap();
            assert_eq!(parsed["context"]["status"], status);
            assert_eq!(*calls.lock().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn source_download_head_resolves_and_encodes_slash_branch_ref() {
        let mut repo = fixture_repo();
        repo.branch = "HEAD".into();
        let expected = "0123456789abcdef0123456789abcdef01234567";
        let calls = std::sync::Mutex::new(Vec::new());
        let snapshot = fetch_source_snapshot(&repo, |url, _| {
            calls.lock().unwrap().push(url.clone());
            let body = if url == "https://api.github.com/repos/owner/repo" {
                br#"{"default_branch":"release/next"}"#.to_vec()
            } else if url == "https://api.github.com/repos/owner/repo/git/ref/heads/release%2Fnext"
            {
                serde_json::to_vec(
                    &serde_json::json!({ "object": { "type": "commit", "sha": expected } }),
                )
                .unwrap()
            } else {
                assert!(url.ends_with(&format!("/git/trees/{expected}?recursive=1")));
                br#"{"tree":[],"truncated":false}"#.to_vec()
            };
            std::future::ready(Ok(body))
        })
        .await
        .unwrap();
        assert_eq!(snapshot.repo.branch, "release/next");
        assert_eq!(snapshot.commit, expected);
        assert_eq!(calls.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn source_download_selection_preserves_stored_path_and_metadata_fallback() {
        let local = tempfile::tempdir().unwrap();
        let documents = HashMap::from([
            (
                "skills/old-name/SKILL.md",
                b"---\nname: renamed\n---\n".to_vec(),
            ),
            (
                "skills/current-name/SKILL.md",
                b"---\nname: metadata-name\n---\n".to_vec(),
            ),
            (
                "skills/installed-name/SKILL.md",
                b"---\nname: competitor\n---\n".to_vec(),
            ),
        ]);
        let snapshot = fixture_snapshot(
            documents
                .iter()
                .map(|(path, bytes)| blob(path, bytes))
                .collect(),
        );
        let target = installed("installed-name", "skills/old-name/SKILL.md");
        let selected = select_sources(&snapshot, &[target], local.path(), |_, _| {
            std::future::ready(Err(anyhow!(
                "direct stored path should not download a catalog"
            )))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selected[0].prefix, "skills/old-name");

        let moved = installed("metadata-name", "removed/SKILL.md");
        let selected = select_sources(&snapshot, &[moved], local.path(), |url, _| {
            let path = url.split(&format!("/{}/", snapshot.commit)).nth(1).unwrap();
            std::future::ready(Ok(documents[path].clone()))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selected[0].prefix, "skills/current-name");
    }

    #[tokio::test]
    async fn source_download_selection_preserves_slash_branches() {
        let local = tempfile::tempdir().unwrap();
        let mut snapshot = fixture_snapshot(vec![blob("skills/example/SKILL.md", b"skill")]);
        snapshot.repo.branch = "release/next".into();
        let mut target = installed("example", "skills/example/SKILL.md");
        target.repo_branch = Some("release/next".into());
        target.readme_url =
            Some("https://github.com/owner/repo/blob/release/next/skills/example/SKILL.md".into());
        let selected = select_sources(&snapshot, &[target], local.path(), |_, _| {
            std::future::ready(Err(anyhow!("stored source should not fetch metadata")))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selected[0].prefix, "skills/example");
    }

    #[tokio::test]
    async fn source_download_metadata_fallback_keeps_other_resolved_sources() {
        let local = tempfile::tempdir().unwrap();
        let documents = HashMap::from([
            (
                "skills/original/SKILL.md",
                b"---\nname: renamed-skill\n---\n".to_vec(),
            ),
            (
                "skills/installed-name/SKILL.md",
                b"---\nname: competitor\n---\n".to_vec(),
            ),
            (
                "skills/metadata-folder/SKILL.md",
                b"---\nname: metadata-name\n---\n".to_vec(),
            ),
        ]);
        let mut snapshot = fixture_snapshot(
            documents
                .iter()
                .map(|(path, bytes)| blob(path, bytes))
                .collect(),
        );
        snapshot.repo.branch = "release/next".into();
        let mut existing = installed("installed-name", "skills/original/SKILL.md");
        existing.repo_branch = Some(snapshot.repo.branch.clone());
        existing.readme_url =
            Some("https://github.com/owner/repo/blob/release/next/skills/original/SKILL.md".into());
        let moved = installed("metadata-name", "removed/SKILL.md");
        let selected = select_sources(&snapshot, &[existing, moved], local.path(), |url, _| {
            let path = url.split(&format!("/{}/", snapshot.commit)).nth(1).unwrap();
            std::future::ready(Ok(documents[path].clone()))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].prefix, "skills/original", "resolving another skill must not replace a valid persisted source with a basename competitor");
        assert_eq!(selected[1].prefix, "skills/metadata-folder");
    }

    #[tokio::test]
    async fn source_download_selected_tree_matches_archive_hash_and_keeps_hidden_files() {
        let local = tempfile::tempdir().unwrap();
        let local_skill = local.path().join("example");
        fs::create_dir_all(local_skill.join("scripts")).unwrap();
        let documents = HashMap::from([
            (
                "skills/example/SKILL.md",
                b"---\nname: example\n---\n".to_vec(),
            ),
            (
                "skills/example/scripts/run.py",
                b"print('new code')\n".to_vec(),
            ),
            ("skills/example/asset.bin", vec![0, 255, 1, 2]),
            ("skills/example/.config", b"hidden setting".to_vec()),
            ("skills/example-extra/SKILL.md", b"unrelated".to_vec()),
        ]);
        fs::write(
            local_skill.join("SKILL.md"),
            &documents["skills/example/SKILL.md"],
        )
        .unwrap();
        fs::write(local_skill.join("scripts/run.py"), b"old code").unwrap();
        fs::write(local_skill.join("asset.bin"), [0, 254, 1, 2]).unwrap();
        fs::write(
            local_skill.join("retired.txt"),
            b"local extra must not enter remote hash",
        )
        .unwrap();
        let snapshot = fixture_snapshot(
            documents
                .iter()
                .map(|(path, bytes)| blob(path, bytes))
                .collect(),
        );
        let selected = vec![SelectedSource {
            prefix: "skills/example".into(),
            local_dir: local_skill.clone(),
        }];
        let calls = std::sync::Mutex::new(Vec::new());
        let staged = materialize_sources(
            &snapshot,
            &selected,
            tempfile::tempdir().unwrap(),
            |url, limit| {
                let path = url.split(&format!("/{}/", snapshot.commit)).nth(1).unwrap();
                calls.lock().unwrap().push(path.to_string());
                let bytes = &documents[path];
                assert_eq!(limit, bytes.len() as u64);
                std::future::ready(Ok(fixture_file(bytes)))
            },
        )
        .await
        .unwrap();
        let staged_skill = staged.path().join("skills/example");
        assert!(!staged.path().join("skills/example-extra").exists());
        assert!(!staged_skill.join("retired.txt").exists());
        assert_eq!(
            fs::read(staged_skill.join(".config")).unwrap(),
            b"hidden setting"
        );
        assert_eq!(
            fs::read(staged_skill.join("asset.bin")).unwrap(),
            [0, 255, 1, 2]
        );
        assert_eq!(
            calls.lock().unwrap().len(),
            3,
            "only changed or missing files need HTTP"
        );

        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, bytes) in &documents {
            zip.start_file(
                format!("repo-main/{path}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        let archive = zip::ZipArchive::new(zip.finish().unwrap()).unwrap();
        let extracted = tempfile::tempdir().unwrap();
        SkillService::extract_repo_archive(archive, extracted.path()).unwrap();
        assert_eq!(
            SkillService::compute_dir_hash(&staged_skill).unwrap(),
            SkillService::compute_dir_hash(&extracted.path().join("skills/example")).unwrap()
        );
        assert_ne!(
            SkillService::compute_dir_hash(&staged_skill).unwrap(),
            SkillService::compute_dir_hash(&local_skill).unwrap()
        );
        assert_eq!(
            fs::read(local_skill.join("scripts/run.py")).unwrap(),
            b"old code"
        );
    }

    #[tokio::test]
    async fn source_download_failure_cleans_staging_and_preserves_local_files() {
        let parent = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        fs::write(local.path().join("keep.txt"), b"untouched").unwrap();
        let snapshot = fixture_snapshot(vec![blob("skills/example/SKILL.md", b"expected")]);
        let selected = vec![SelectedSource {
            prefix: "skills/example".into(),
            local_dir: local.path().join("missing"),
        }];
        for fail_request in [false, true] {
            let staging = tempfile::tempdir_in(parent.path()).unwrap();
            let path = staging.path().to_path_buf();
            let result = materialize_sources(&snapshot, &selected, staging, |_, _| {
                std::future::ready(if fail_request {
                    Err(anyhow!("fixture network failure"))
                } else {
                    Ok(fixture_file(b"corrupt!"))
                })
            })
            .await;
            assert!(result.is_err());
            assert!(!path.exists());
        }
        assert_eq!(
            fs::read(local.path().join("keep.txt")).unwrap(),
            b"untouched"
        );
    }

    #[tokio::test]
    async fn source_download_symlinks_keep_the_archive_fallback() {
        let local = tempfile::tempdir().unwrap();
        let mut link = blob("skills/example/shared", b"../../shared");
        link.mode = "120000".into();
        let snapshot = fixture_snapshot(vec![blob("skills/example/SKILL.md", b"skill"), link]);
        let selected = select_sources(
            &snapshot,
            &[installed("example", "skills/example/SKILL.md")],
            local.path(),
            |_, _| std::future::ready(Err(anyhow!("unexpected metadata download"))),
        )
        .await
        .unwrap();
        assert!(
            selected.is_none(),
            "symlink target closure must use the established archive materializer"
        );
    }
}

//! Materialize Git submodules at the commits recorded by the parent repository.

use super::*;
use futures::{stream, StreamExt, TryStreamExt};
use std::sync::atomic::AtomicU64;

const MAX_SUBMODULES: usize = 256;
const MAX_SUBMODULE_DEPTH: usize = 8;
const MAX_MODULES_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TREE_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const SUBMODULE_DOWNLOAD_CONCURRENCY: usize = 8;

#[derive(Default)]
pub(super) struct SubmoduleBudget {
    downloaded: AtomicU64,
    pub(super) expanded: Mutex<u64>,
}

impl SubmoduleBudget {
    pub(super) fn charge_download(&self, bytes: u64) -> bool {
        self.downloaded
            .fetch_add(bytes, Ordering::Relaxed)
            .saturating_add(bytes)
            <= MAX_ARCHIVE_DOWNLOAD_BYTES
    }
}

#[derive(Debug, Clone)]
struct ModuleConfig {
    path: String,
    url: String,
}

#[derive(Debug, Clone)]
struct ModuleLink {
    path: String,
    revision: String,
}

#[derive(Clone)]
struct PendingRepo {
    repo: SkillRepo,
    revision: String,
    root: PathBuf,
    depth: usize,
    ancestors: Vec<(String, String, String, String)>,
}

#[derive(Clone)]
pub(super) struct SubmoduleMount {
    root: PathBuf,
    repo: SkillRepo,
    revision: String,
    platform: RepoPlatform,
}

impl SkillService {
    /// Branch archives omit gitlinks. Resolve the parent branch once, then use
    /// that exact commit for its archive, tree, and all child revisions.
    pub(super) async fn download_with_submodules(
        &self,
        repo: &SkillRepo,
        branch: &str,
        platform: RepoPlatform,
        token: Option<&str>,
    ) -> Result<(tempfile::TempDir, Vec<SubmoduleMount>)> {
        let revision = self
            .repo_branch_revision(repo, platform, branch, token)
            .await?;
        let snapshot = tempfile::tempdir()?;
        let budget = Arc::new(SubmoduleBudget::default());
        let url = Self::submodule_archive_url(repo, platform, &revision, token.is_some())?;
        self.download_and_extract_with_budget(&url, snapshot.path(), token, Some(&budget))
            .await?;
        let mounts = self
            .materialize_submodules(repo, &revision, snapshot.path(), budget)
            .await?;
        Ok((snapshot, mounts))
    }

    async fn materialize_submodules(
        &self,
        repo: &SkillRepo,
        revision: &str,
        root: &Path,
        budget: Arc<SubmoduleBudget>,
    ) -> Result<Vec<SubmoduleMount>> {
        let configured_hosts: HashSet<String> = self
            .repo_token_list()
            .map(|tokens| tokens.hosts.into_iter().collect())
            .unwrap_or_default();
        let mut pending = vec![PendingRepo {
            repo: repo.clone(),
            revision: revision.to_owned(),
            root: root.to_path_buf(),
            depth: 0,
            ancestors: vec![Self::module_identity(repo, revision)],
        }];
        let mut total_modules = 0;
        let mut mounts = Vec::new();

        while let Some(parent) = pending.pop() {
            let modules_file = parent.root.join(".gitmodules");
            if !modules_file.is_file() {
                continue;
            }
            if parent.depth >= MAX_SUBMODULE_DEPTH {
                return Err(anyhow!("Git submodule nesting exceeds the supported depth"));
            }
            if fs::metadata(&modules_file)?.len() > MAX_MODULES_FILE_BYTES {
                return Err(anyhow!("Git submodule configuration is too large"));
            }
            let config = Self::parse_submodule_config(&fs::read(&modules_file)?)?;
            let epoch = self.credential_epoch.load(Ordering::SeqCst);
            let parent_token = self.repo_token(&parent.repo.host).await?;
            let platform = self
                .resolve_platform(&parent.repo.host, parent_token.as_deref(), epoch)
                .await;
            let links = self
                .repo_gitlinks(
                    &parent.repo,
                    platform,
                    &parent.revision,
                    parent_token.as_deref(),
                )
                .await?;
            let plans = Self::plan_submodules(&parent.repo, config, links, &configured_hosts)?;
            total_modules += plans.len();
            if total_modules > MAX_SUBMODULES {
                return Err(anyhow!("Repository exceeds the Git submodule limit"));
            }

            let mut children = Vec::with_capacity(plans.len());
            for (path, child_repo, child_revision) in plans {
                let identity = Self::module_identity(&child_repo, &child_revision);
                if parent.ancestors.contains(&identity) {
                    return Err(anyhow!("Git submodule cycle detected"));
                }
                let child_root = Self::prepare_submodule_destination(&parent.root, &path, &budget)?;
                let mut ancestors = parent.ancestors.clone();
                ancestors.push(identity);
                children.push(PendingRepo {
                    repo: child_repo,
                    revision: child_revision,
                    root: child_root,
                    depth: parent.depth + 1,
                    ancestors,
                });
            }

            let downloaded: Vec<PendingRepo> = stream::iter(children)
                .map(|child| {
                    let budget = Arc::clone(&budget);
                    async move {
                        let epoch = self.credential_epoch.load(Ordering::SeqCst);
                        let token = self.repo_token(&child.repo.host).await?;
                        let platform = self
                            .resolve_platform(&child.repo.host, token.as_deref(), epoch)
                            .await;
                        let url = Self::submodule_archive_url(
                            &child.repo,
                            platform,
                            &child.revision,
                            token.is_some(),
                        )?;
                        self.download_and_extract_with_budget(
                            &url,
                            &child.root,
                            token.as_deref(),
                            Some(&budget),
                        )
                        .await?;
                        Ok::<_, anyhow::Error>(child)
                    }
                })
                .buffer_unordered(SUBMODULE_DOWNLOAD_CONCURRENCY)
                .try_collect()
                .await?;
            for child in downloaded {
                mounts.push(SubmoduleMount {
                    root: child.root.clone(),
                    repo: child.repo.clone(),
                    revision: child.revision.clone(),
                    platform: self.known_platform(&child.repo.host),
                });
                pending.push(child);
            }
        }
        Ok(mounts)
    }

    pub(super) fn submodule_doc_url(source: &Path, mounts: &[SubmoduleMount]) -> Option<String> {
        let mount = mounts
            .iter()
            .filter(|mount| source.starts_with(&mount.root))
            .max_by_key(|mount| mount.root.components().count())?;
        let relative = source.strip_prefix(&mount.root).ok()?;
        let mut url = url::Url::parse(&format!("https://{}/", mount.repo.host)).ok()?;
        {
            let mut parts = url.path_segments_mut().ok()?;
            parts.extend(mount.repo.owner.split('/'));
            parts.push(&mount.repo.name);
            if mount.repo.is_github() {
                parts.push("blob");
            } else if mount.platform == RepoPlatform::Gitea {
                parts.extend(["src", "commit"]);
            } else {
                parts.extend(["-", "blob"]);
            }
            parts.push(&mount.revision);
            for component in relative.components() {
                let Component::Normal(name) = component else {
                    return None;
                };
                parts.push(name.to_str()?);
            }
            parts.push("SKILL.md");
        }
        Some(url.into())
    }

    pub(super) fn apply_submodule_doc_urls(
        root: &Path,
        skills: &mut [DiscoverableSkill],
        mounts: &[SubmoduleMount],
    ) {
        for skill in skills {
            if let Some(url) = Self::submodule_doc_url(&root.join(&skill.directory), mounts) {
                skill.readme_url = Some(url);
            }
        }
    }

    fn module_identity(repo: &SkillRepo, revision: &str) -> (String, String, String, String) {
        (
            repo.host.clone(),
            repo.owner.clone(),
            repo.name.clone(),
            revision.to_owned(),
        )
    }

    fn parse_submodule_config(bytes: &[u8]) -> Result<Vec<ModuleConfig>> {
        let text =
            std::str::from_utf8(bytes).context("Git submodule configuration is not UTF-8")?;
        let config = gix_config::File::try_from(text)
            .map_err(|_| anyhow!("Invalid Git submodule configuration"))?;
        let mut modules: HashMap<Vec<u8>, (Option<String>, Option<String>)> = HashMap::new();
        if let Some(sections) = config.sections_by_name("submodule") {
            for section in sections {
                let name = section
                    .header()
                    .subsection_name()
                    .ok_or_else(|| anyhow!("Submodule name missing"))?
                    .to_vec();
                let values = modules.entry(name).or_default();
                if let Some(path) = section.value("path") {
                    values.0 = Some(String::from_utf8(path.to_vec())?);
                }
                if let Some(url) = section.value("url") {
                    values.1 = Some(String::from_utf8(url.to_vec())?);
                }
            }
        }
        let mut result: Vec<ModuleConfig> = modules
            .into_values()
            .map(|(path, url)| {
                Ok(ModuleConfig {
                    path: path.ok_or_else(|| anyhow!("Submodule path missing"))?,
                    url: url.ok_or_else(|| anyhow!("Submodule URL missing"))?,
                })
            })
            .collect::<Result<_>>()?;
        result.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(result)
    }

    fn plan_submodules(
        parent: &SkillRepo,
        configs: Vec<ModuleConfig>,
        links: Vec<ModuleLink>,
        configured_hosts: &HashSet<String>,
    ) -> Result<Vec<(PathBuf, SkillRepo, String)>> {
        let mut by_path = HashMap::new();
        for config in configs {
            if by_path.insert(config.path.clone(), config.url).is_some() {
                return Err(anyhow!("Duplicate Git submodule path"));
            }
        }
        if links.len() > MAX_SUBMODULES {
            return Err(anyhow!("Repository exceeds the Git submodule limit"));
        }
        let mut plans = Vec::new();
        let mut paths: Vec<String> = Vec::new();
        for link in links {
            if !Self::valid_git_object_id(&link.revision) {
                return Err(anyhow!("Invalid Git submodule revision"));
            }
            let path = Self::safe_submodule_path(&link.path)?;
            let folded = link.path.to_lowercase();
            if paths.iter().any(|old| {
                old == &folded
                    || folded.starts_with(&format!("{old}/"))
                    || old.starts_with(&format!("{folded}/"))
            }) {
                return Err(anyhow!("Git submodule paths overlap"));
            }
            paths.push(folded);
            let url = by_path
                .remove(&link.path)
                .ok_or_else(|| anyhow!("Git submodule has no URL in .gitmodules"))?;
            let child = Self::resolve_submodule_repo(parent, &url, configured_hosts)?;
            plans.push((path, child, link.revision));
        }
        Ok(plans)
    }

    fn safe_submodule_path(raw: &str) -> Result<PathBuf> {
        if raw.len() > 4096
            || raw.contains('\\')
            || raw.chars().any(char::is_control)
            || raw.split('/').count() > 64
            || raw
                .split('/')
                .any(|part| part.len() > 255 || part.eq_ignore_ascii_case(".git"))
        {
            return Err(anyhow!("Invalid Git submodule path"));
        }
        let path = Self::sanitize_skill_source_path(raw)
            .ok_or_else(|| anyhow!("Invalid Git submodule path"))?;
        if path.to_string_lossy().replace('\\', "/") != raw {
            return Err(anyhow!("Invalid Git submodule path"));
        }
        Ok(path)
    }

    fn prepare_submodule_destination(
        root: &Path,
        relative: &Path,
        budget: &SubmoduleBudget,
    ) -> Result<PathBuf> {
        let mut dest = root.to_path_buf();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(anyhow!("Invalid Git submodule path"));
            };
            dest.push(name);
            match fs::symlink_metadata(&dest) {
                Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                    return Err(anyhow!("Unsafe Git submodule destination"));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let mut expanded = budget.expanded.lock_recover();
                    Self::charge_archive_budget(&mut expanded, DIRECTORY_BUDGET_COST)?;
                    fs::create_dir(&dest)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        if fs::read_dir(&dest)?.next().is_some() {
            return Err(anyhow!("Git submodule destination is not empty"));
        }
        Ok(dest)
    }

    fn resolve_submodule_repo(
        parent: &SkillRepo,
        raw: &str,
        configured_hosts: &HashSet<String>,
    ) -> Result<SkillRepo> {
        if raw.contains('\\') || raw.contains('?') || raw.contains('#') {
            return Err(anyhow!("Unsupported Git submodule URL"));
        }
        let url = if raw.starts_with("../") || raw.starts_with("./") {
            let base = format!("https://{}/{}/{}/", parent.host, parent.owner, parent.name);
            url::Url::parse(&base)?.join(raw)?
        } else if let Some(rest) = raw.strip_prefix("git@") {
            let (host, path) = rest
                .split_once(':')
                .ok_or_else(|| anyhow!("Invalid Git URL"))?;
            url::Url::parse(&format!("https://{host}/{path}"))?
        } else {
            let parsed = url::Url::parse(raw).map_err(|_| anyhow!("Invalid Git URL"))?;
            if parsed.scheme() == "ssh" && parsed.username() == "git" && parsed.password().is_none()
            {
                let hostname = parsed
                    .host_str()
                    .ok_or_else(|| anyhow!("Invalid Git URL"))?;
                let parent_origin = url::Url::parse(&format!("https://{}", parent.host))?;
                let target_host = if parent_origin.host_str() == Some(hostname) {
                    parent.host.as_str()
                } else {
                    hostname
                };
                url::Url::parse(&format!("https://{target_host}{}", parsed.path()))?
            } else {
                parsed
            }
        };
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(anyhow!("Unsupported Git submodule URL"));
        }
        let host = match url.port() {
            Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
            None => url.host_str().unwrap_or_default().to_owned(),
        };
        if host != parent.host
            && host != GITHUB_HOST
            && host != "gitlab.com"
            && !configured_hosts.contains(&host)
        {
            return Err(anyhow!("Git submodule host is not configured: {host}"));
        }
        let mut segments: Vec<&str> = url
            .path_segments()
            .ok_or_else(|| anyhow!("Invalid Git URL"))?
            .collect();
        let name = segments.pop().ok_or_else(|| anyhow!("Invalid Git URL"))?;
        let name = name.strip_suffix(".git").unwrap_or(name).to_owned();
        let child = SkillRepo {
            host,
            owner: segments.join("/"),
            name,
            branch: "main".to_owned(),
            enabled: true,
        };
        Self::validate_repo(&child)?;
        Ok(child)
    }

    fn valid_git_object_id(value: &str) -> bool {
        matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
    }

    fn submodule_archive_url(
        repo: &SkillRepo,
        platform: RepoPlatform,
        revision: &str,
        authenticated: bool,
    ) -> Result<String> {
        Self::validate_repo(repo)?;
        if !Self::valid_git_object_id(revision) {
            return Err(anyhow!("Invalid Git commit ID"));
        }
        if repo.is_github() {
            return Ok(if authenticated {
                format!(
                    "https://api.github.com/repos/{}/{}/zipball/{revision}",
                    repo.owner, repo.name
                )
            } else {
                format!(
                    "https://github.com/{}/{}/archive/{revision}.zip",
                    repo.owner, repo.name
                )
            });
        }
        Self::build_archive_url(repo, platform, revision, authenticated)
    }

    fn repo_api_url(repo: &SkillRepo, platform: RepoPlatform, tail: &[&str]) -> Result<url::Url> {
        Self::validate_repo(repo)?;
        let origin = if repo.is_github() {
            "https://api.github.com".to_owned()
        } else {
            format!("https://{}", repo.host)
        };
        let mut url = url::Url::parse(&origin)?;
        {
            let mut parts = url
                .path_segments_mut()
                .map_err(|_| anyhow!("Invalid repository host"))?;
            if repo.is_github() {
                parts.extend(["repos", &repo.owner, &repo.name]);
            } else if platform == RepoPlatform::Gitea {
                if repo.owner.contains('/') {
                    return Err(anyhow!("Invalid Gitea repository owner"));
                }
                parts.extend(["api", "v1", "repos", &repo.owner, &repo.name]);
            } else {
                parts.extend([
                    "api",
                    "v4",
                    "projects",
                    &format!("{}/{}", repo.owner, repo.name),
                ]);
            }
            parts.extend(tail.iter().copied());
        }
        Ok(url)
    }

    async fn repo_branch_revision(
        &self,
        repo: &SkillRepo,
        platform: RepoPlatform,
        branch: &str,
        token: Option<&str>,
    ) -> Result<String> {
        let tail = if platform == RepoPlatform::GitLab && !repo.is_github() {
            vec!["repository", "branches", branch]
        } else {
            vec!["branches", branch]
        };
        let url = Self::repo_api_url(repo, platform, &tail)?;
        let json = self.submodule_api_json(url, token).await?;
        let id = json
            .pointer("/commit/sha")
            .or_else(|| json.pointer("/commit/id"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("Repository branch has no commit ID"))?;
        if !Self::valid_git_object_id(id) {
            return Err(anyhow!("Invalid repository commit ID"));
        }
        Ok(id.to_owned())
    }

    async fn repo_gitlinks(
        &self,
        repo: &SkillRepo,
        platform: RepoPlatform,
        revision: &str,
        token: Option<&str>,
    ) -> Result<Vec<ModuleLink>> {
        let mut links = Vec::new();
        let mut seen_entries = 0usize;
        let mut seen_paths = HashSet::new();
        for page in 1..=301 {
            let gitlab = platform == RepoPlatform::GitLab && !repo.is_github();
            let tail = if gitlab {
                vec!["repository", "tree"]
            } else {
                vec!["git", "trees", revision]
            };
            let mut url = Self::repo_api_url(repo, platform, &tail)?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("recursive", if gitlab { "true" } else { "1" });
                if gitlab {
                    query.append_pair("ref", revision);
                }
                if !repo.is_github() {
                    query.append_pair("per_page", if gitlab { "100" } else { "1000" });
                    query.append_pair("page", &page.to_string());
                }
            }
            let json = self.submodule_api_json(url, token).await?;
            let entries = if gitlab {
                json.as_array()
            } else {
                json.get("tree").and_then(serde_json::Value::as_array)
            }
            .ok_or_else(|| anyhow!("Invalid repository tree response"))?;
            seen_entries += entries.len();
            if seen_entries > MAX_ARCHIVE_ENTRIES {
                return Err(anyhow!("Repository tree has too many entries"));
            }
            for entry in entries {
                let path = entry
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow!("Repository tree entry has no path"))?;
                if !seen_paths.insert(path.to_owned()) {
                    return Err(anyhow!("Repository tree contains duplicate entries"));
                }
                let kind = entry.get("type").and_then(serde_json::Value::as_str);
                let mode = entry.get("mode").and_then(serde_json::Value::as_str);
                if kind != Some("commit") && mode != Some("160000") {
                    continue;
                }
                let id = entry
                    .get("sha")
                    .or_else(|| entry.get("id"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow!("Gitlink has no commit ID"))?;
                links.push(ModuleLink {
                    path: path.to_owned(),
                    revision: id.to_owned(),
                });
            }
            if repo.is_github() {
                if json.get("truncated").and_then(serde_json::Value::as_bool) == Some(true) {
                    return Err(anyhow!("Repository tree is truncated"));
                }
                return Ok(links);
            }
            if !gitlab {
                if let Some(total) = json.get("total_count").and_then(serde_json::Value::as_u64) {
                    if seen_entries as u64 >= total {
                        return Ok(links);
                    }
                    if entries.is_empty() {
                        return Err(anyhow!("Repository tree is incomplete"));
                    }
                    continue;
                }
                if json.get("truncated").and_then(serde_json::Value::as_bool) == Some(true) {
                    if entries.is_empty() {
                        return Err(anyhow!("Repository tree is incomplete"));
                    }
                    continue;
                }
            }
            let page_size = if gitlab { 100 } else { 1000 };
            if entries.len() < page_size {
                return Ok(links);
            }
        }
        Err(anyhow!("Repository tree exceeds the page limit"))
    }

    async fn submodule_api_json(
        &self,
        url: url::Url,
        token: Option<&str>,
    ) -> Result<serde_json::Value> {
        #[cfg(test)]
        if let Some(responses) = &self.submodule_http_fixture {
            let bytes = responses
                .get(url.as_str())
                .ok_or_else(|| anyhow!("Missing API fixture: {url}"))?;
            return serde_json::from_slice(bytes).context("Invalid Git repository API response");
        }
        let mut request = Self::http_client(token.is_some())?
            .get(url)
            .header(reqwest::header::USER_AGENT, SKILL_DOWNLOAD_USER_AGENT);
        if let Some(token) = token {
            request = Self::with_bearer(request, token)?;
        }
        let mut response = timeout(DOWNLOAD_STALL_TIMEOUT, request.send())
            .await
            .map_err(|_| Self::download_stalled_error())??;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Git repository API returned HTTP {}",
                response.status().as_u16()
            ));
        }
        let mut body = Vec::new();
        while let Some(chunk) = timeout(DOWNLOAD_STALL_TIMEOUT, response.chunk())
            .await
            .map_err(|_| Self::download_stalled_error())??
        {
            if body.len().saturating_add(chunk.len()) > MAX_TREE_RESPONSE_BYTES {
                return Err(anyhow!("Git repository API response is too large"));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).context("Invalid Git repository API response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const PARENT_SHA: &str = "1111111111111111111111111111111111111111";
    const CHILD_SHA: &str = "2222222222222222222222222222222222222222";
    const HELPER_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn repo() -> SkillRepo {
        SkillRepo {
            host: GITHUB_HOST.to_owned(),
            owner: "example".to_owned(),
            name: "catalog".to_owned(),
            branch: "main".to_owned(),
            enabled: true,
        }
    }

    fn archive(files: &[(&str, &str)], dirs: &[&str]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for dir in dirs {
            writer.add_directory(*dir, options).unwrap();
        }
        for (name, content) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn submodule_discovery_materializes_the_pinned_skill() {
        let test_home = crate::test_support::TestHome::new();
        let modules = "[submodule \"skills/child\"]\n path = skills/child\n url = ../child.git\n";
        let parent = archive(
            &[
                ("catalog/.gitmodules", modules),
                ("catalog/skills/local/SKILL.md", "---\nname: local\n---\n"),
            ],
            &["catalog/skills/child/"],
        );
        let child = archive(
            &[
                (
                    "child/SKILL.md",
                    "---\nname: child\ndescription: pinned\n---\n",
                ),
                ("child/scripts/check.txt", "pinned child content"),
                (
                    "child/.gitmodules",
                    "[submodule \"shared\"]\n path = scripts/shared\n url = ../helper.git\n",
                ),
            ],
            &["child/scripts/shared/"],
        );
        let helper = archive(&[("helper/tool.txt", "nested submodule content")], &[]);
        let tree = serde_json::json!({"truncated": false, "tree": [
            {"path": "skills/child", "mode": "160000", "type": "commit", "sha": CHILD_SHA}
        ]});
        let mut responses = HashMap::new();
        responses.insert(
            "https://github.com/example/catalog/archive/refs/heads/main.zip".into(),
            parent.clone(),
        );
        responses.insert(
            format!("https://github.com/example/catalog/archive/{PARENT_SHA}.zip"),
            parent,
        );
        responses.insert(
            "https://api.github.com/repos/example/catalog/branches/main".into(),
            serde_json::json!({"commit":{"sha": PARENT_SHA}})
                .to_string()
                .into_bytes(),
        );
        responses.insert(
            format!(
                "https://api.github.com/repos/example/catalog/git/trees/{PARENT_SHA}?recursive=1"
            ),
            tree.to_string().into_bytes(),
        );
        responses.insert(
            format!("https://github.com/example/child/archive/{CHILD_SHA}.zip"),
            child,
        );
        responses.insert(
            format!("https://api.github.com/repos/example/child/git/trees/{CHILD_SHA}?recursive=1"),
            serde_json::json!({"truncated":false,"tree":[{"path":"scripts/shared","mode":"160000","type":"commit","sha":HELPER_SHA}]}).to_string().into_bytes(),
        );
        responses.insert(
            format!("https://github.com/example/helper/archive/{HELPER_SHA}.zip"),
            helper,
        );

        let credentials_dir = tempfile::tempdir().unwrap();
        let (store, _) =
            crate::services::skill_repo_tokens::tests::memory_store(credentials_dir.path());
        let mut service = SkillService::new().with_repo_tokens(store);
        service.submodule_http_fixture = Some(Arc::new(responses));
        let result = service.discover_repo(&repo()).await;
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        let names: HashSet<&str> = result
            .skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect();
        assert_eq!(names, HashSet::from(["child", "local"]));
        let child_skill = result
            .skills
            .iter()
            .find(|skill| skill.name == "child")
            .unwrap();
        assert_eq!(child_skill.directory, "skills/child");
        assert_eq!(
            child_skill.readme_url.as_deref(),
            Some(format!("https://github.com/example/child/blob/{CHILD_SHA}/SKILL.md").as_str())
        );

        let (snapshot, branch, _) = service.download_repo_bounded(&repo()).await.unwrap();
        assert_eq!(branch, "main");
        let source =
            SkillService::resolve_skill_source_dir(snapshot.path(), &child_skill.directory)
                .unwrap();
        assert_eq!(
            fs::read_to_string(source.join("scripts/check.txt")).unwrap(),
            "pinned child content"
        );
        assert_eq!(
            fs::read_to_string(source.join("scripts/shared/tool.txt")).unwrap(),
            "nested submodule content"
        );

        let db = Arc::new(Database::memory().unwrap());
        let installed = service
            .install(&db, child_skill, &AppType::Claude)
            .await
            .unwrap();
        assert_eq!(installed.readme_url, child_skill.readme_url);
        let installed_dir = SkillService::get_installed_skill_dir(&installed.directory).unwrap();
        assert!(installed_dir.starts_with(test_home.path()));
        assert_eq!(
            fs::read_to_string(installed_dir.join("scripts/check.txt")).unwrap(),
            "pinned child content"
        );
        assert_eq!(
            fs::read_to_string(installed_dir.join("scripts/shared/tool.txt")).unwrap(),
            "nested submodule content"
        );

        let next_parent = "3333333333333333333333333333333333333333";
        let next_child = "4444444444444444444444444444444444444444";
        let updated_parent = archive(
            &[
                ("catalog/.gitmodules", modules),
                ("catalog/skills/local/SKILL.md", "---\nname: local\n---\n"),
            ],
            &["catalog/skills/child/"],
        );
        let updated_child = archive(
            &[
                (
                    "child/SKILL.md",
                    "---\nname: child\ndescription: updated\n---\n",
                ),
                ("child/scripts/check.txt", "updated child content"),
            ],
            &[],
        );
        let fixtures = Arc::get_mut(service.submodule_http_fixture.as_mut().unwrap()).unwrap();
        fixtures.insert(
            "https://api.github.com/repos/example/catalog/branches/main".into(),
            serde_json::json!({"commit":{"sha": next_parent}})
                .to_string()
                .into_bytes(),
        );
        fixtures.insert(
            format!("https://github.com/example/catalog/archive/{next_parent}.zip"),
            updated_parent,
        );
        fixtures.insert(format!("https://api.github.com/repos/example/catalog/git/trees/{next_parent}?recursive=1"), serde_json::json!({"truncated": false,"tree":[{"path":"skills/child","mode":"160000","type":"commit","sha":next_child}]}).to_string().into_bytes());
        fixtures.insert(
            format!("https://github.com/example/child/archive/{next_child}.zip"),
            updated_child,
        );

        let updates = service.check_updates_report(&db).await.unwrap();
        assert!(updates.failures.is_empty(), "{:?}", updates.failures);
        assert_eq!(updates.updates.len(), 1);
        let updated = service.update_skill(&db, &installed.id).await.unwrap();
        assert_eq!(
            updated.readme_url.as_deref(),
            Some(format!("https://github.com/example/child/blob/{next_child}/SKILL.md").as_str())
        );
        assert_eq!(
            fs::read_to_string(installed_dir.join("scripts/check.txt")).unwrap(),
            "updated child content"
        );
        assert!(!installed_dir.join("scripts/shared/tool.txt").exists());
    }

    #[test]
    fn submodule_urls_preserve_relative_repository_and_host_boundaries() {
        let parent = repo();
        let allowed = HashSet::new();
        for raw in [
            "../child.git",
            "https://github.com/example/child.git",
            "git@github.com:example/child.git",
            "ssh://git@github.com/example/child.git",
        ] {
            let child = SkillService::resolve_submodule_repo(&parent, raw, &allowed)
                .unwrap_or_else(|error| panic!("{raw}: {error}"));
            assert_eq!(
                (
                    child.host.as_str(),
                    child.owner.as_str(),
                    child.name.as_str()
                ),
                ("github.com", "example", "child")
            );
        }
        for raw in [
            "file:///etc/passwd",
            "http://github.com/example/child",
            "https://user:pass@github.com/example/child",
            "https://unknown.example/example/child",
            "../../evil?x=y",
        ] {
            assert!(
                SkillService::resolve_submodule_repo(&parent, raw, &allowed).is_err(),
                "{raw}"
            );
        }
    }

    #[test]
    fn submodule_planning_rejects_unsafe_paths_and_unpinned_links() {
        let parent = repo();
        for path in [
            "../escape",
            "/absolute",
            "skills\\escape",
            "skills/.git/child",
        ] {
            assert!(SkillService::safe_submodule_path(path).is_err(), "{path}");
        }
        let config = SkillService::parse_submodule_config(
            b"[submodule \"x\"]\n path = skills/x\n url = ../x.git\n",
        )
        .unwrap();
        let bad = vec![ModuleLink {
            path: "skills/x".into(),
            revision: "main".into(),
        }];
        assert!(
            SkillService::plan_submodules(&parent, config.clone(), bad, &HashSet::new()).is_err()
        );
        let missing = vec![ModuleLink {
            path: "skills/y".into(),
            revision: CHILD_SHA.into(),
        }];
        assert!(SkillService::plan_submodules(&parent, config, missing, &HashSet::new()).is_err());
    }

    #[test]
    fn submodule_config_combines_repeated_sections_using_git_rules() {
        let config = SkillService::parse_submodule_config(
            b"[submodule \"first\"]\n path = skills/x\n[submodule \"first\"]\n url = ../x.git\n",
        )
        .unwrap();
        assert_eq!(config.len(), 1);
        assert_eq!(config[0].path, "skills/x");
        assert_eq!(config[0].url, "../x.git");
    }

    #[test]
    fn submodule_doc_urls_point_to_the_child_commit() {
        let root = PathBuf::from("skill-root");
        let mut child = repo();
        child.host = "git.example.test".into();
        child.owner = "team".into();
        child.name = "child".into();
        let mut mount = SubmoduleMount {
            root: root.clone(),
            repo: child,
            revision: CHILD_SHA.into(),
            platform: RepoPlatform::Gitea,
        };
        assert_eq!(
            SkillService::submodule_doc_url(&root.join("nested name"), &[mount.clone()]).unwrap(),
            format!(
                "https://git.example.test/team/child/src/commit/{CHILD_SHA}/nested%20name/SKILL.md"
            )
        );
        mount.platform = RepoPlatform::GitLab;
        mount.repo.owner = "team/sub".into();
        assert_eq!(
            SkillService::submodule_doc_url(&root, &[mount]).unwrap(),
            format!("https://git.example.test/team/sub/child/-/blob/{CHILD_SHA}/SKILL.md")
        );
    }

    #[test]
    fn update_matching_distinguishes_submodules_with_the_same_document_path() {
        let example = |name: &str| DiscoverableSkill {
            key: name.into(),
            name: name.into(),
            description: String::new(),
            directory: format!("skills/{name}"),
            readme_url: Some(format!(
                "https://github.com/example/{name}/blob/{CHILD_SHA}/SKILL.md"
            )),
            repo_host: GITHUB_HOST.into(),
            repo_owner: "example".into(),
            repo_name: "catalog".into(),
            repo_branch: "main".into(),
        };
        let skills = [example("other"), example("wanted")];
        let old_url = format!("https://github.com/example/wanted/blob/{PARENT_SHA}/SKILL.md");
        let found =
            SkillService::find_remote_skill_for_install(&skills, "old-name", Some(&old_url))
                .unwrap();
        assert_eq!(found.name, "wanted");
    }

    #[tokio::test]
    async fn forgejo_and_gitlab_tree_responses_keep_recorded_commits() {
        let home = tempfile::tempdir().unwrap();
        let (store, _) = crate::services::skill_repo_tokens::tests::memory_store(home.path());
        let mut service = SkillService::new().with_repo_tokens(store);
        let mut responses = HashMap::new();
        responses.insert(
            "https://git.example.test/api/v1/repos/team/catalog/branches/main".into(),
            serde_json::json!({"commit":{"id":PARENT_SHA}})
                .to_string()
                .into_bytes(),
        );
        responses.insert(format!("https://git.example.test/api/v1/repos/team/catalog/git/trees/{PARENT_SHA}?recursive=1&per_page=1000&page=1"), serde_json::json!({"total_count":2,"truncated":true,"tree":[{"path":"skills/child","type":"commit","sha":CHILD_SHA}]}).to_string().into_bytes());
        responses.insert(format!("https://git.example.test/api/v1/repos/team/catalog/git/trees/{PARENT_SHA}?recursive=1&per_page=1000&page=2"), serde_json::json!({"total_count":2,"truncated":true,"tree":[{"path":"README.md","type":"blob"}]}).to_string().into_bytes());
        responses.insert("https://git.example.test/api/v4/projects/team%2Fsub%2Fcatalog/repository/branches/main".into(), serde_json::json!({"commit":{"id":PARENT_SHA}}).to_string().into_bytes());
        responses.insert(format!("https://git.example.test/api/v4/projects/team%2Fsub%2Fcatalog/repository/tree?recursive=true&ref={PARENT_SHA}&per_page=100&page=1"), serde_json::json!([{"path":"skills/child","type":"commit","id":CHILD_SHA}]).to_string().into_bytes());
        service.submodule_http_fixture = Some(Arc::new(responses));
        for (platform, owner) in [
            (RepoPlatform::Gitea, "team"),
            (RepoPlatform::GitLab, "team/sub"),
        ] {
            let repo = SkillRepo {
                host: "git.example.test".into(),
                owner: owner.into(),
                name: "catalog".into(),
                branch: "main".into(),
                enabled: true,
            };
            assert_eq!(
                service
                    .repo_branch_revision(&repo, platform, "main", None)
                    .await
                    .unwrap(),
                PARENT_SHA
            );
            let links = service
                .repo_gitlinks(&repo, platform, PARENT_SHA, None)
                .await
                .unwrap();
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].path, "skills/child");
            assert_eq!(links[0].revision, CHILD_SHA);
        }
    }

    #[test]
    fn submodule_downloads_share_a_single_budget() {
        let budget = SubmoduleBudget::default();
        assert!(budget.charge_download(MAX_ARCHIVE_DOWNLOAD_BYTES - 1));
        assert!(budget.charge_download(1));
        assert!(!budget.charge_download(1));
    }
}

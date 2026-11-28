//! 发现功能：扫描仓库里的技能并解析元数据。

use super::*;
use std::ffi::OsStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskDiscoveryCacheEntry {
    host: String,
    owner: String,
    name: String,
    branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commit_sha: Option<String>,
    resolved_branch: String,
    skills: Vec<DiscoverableSkill>,
}

impl SkillService {
    // ========== 发现功能 ==========

    /// 列出所有可发现的技能（从仓库获取）
    pub async fn discover_available(
        &self,
        repos: Vec<SkillRepo>,
    ) -> Result<Vec<DiscoverableSkill>> {
        Ok(self.discover_available_report(repos, false).await?.skills)
    }

    /// 发现可安装的 Skill，并逐仓库报告没读到的仓库（停用的仓库不读也不报）。
    ///
    /// `force_refresh` 为 false 时，有效期内的仓库直接用缓存，不重新下载。
    pub async fn discover_available_report(
        &self,
        repos: Vec<SkillRepo>,
        force_refresh: bool,
    ) -> Result<SkillDiscoveryResult> {
        self.discover_available_inner(repos, force_refresh, None)
            .await
    }

    /// 从数据库读取仓库列表执行发现，并把哨兵分支（空串 / `HEAD`）解析出的真实分支回写数据库。
    pub(crate) async fn discover_available_for_db(
        &self,
        db: &Database,
        force_refresh: bool,
    ) -> Result<SkillDiscoveryResult> {
        let repos = db.get_skill_repos()?;
        self.discover_available_inner(repos, force_refresh, Some(db))
            .await
    }

    async fn discover_available_inner(
        &self,
        repos: Vec<SkillRepo>,
        force_refresh: bool,
        db: Option<&Database>,
    ) -> Result<SkillDiscoveryResult> {
        let mut skills = Vec::new();
        let mut failures = Vec::new();

        // 仅使用启用的仓库
        let enabled_repos: Vec<SkillRepo> = repos.into_iter().filter(|repo| repo.enabled).collect();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(DISCOVERY_MAX_CONCURRENCY));

        let fetch_tasks = enabled_repos.iter().map(|repo| {
            let semaphore = Arc::clone(&semaphore);
            async move {
                let _permit = semaphore.acquire_owned().await.ok();
                self.fetch_repo_skills_cached(repo, force_refresh).await
            }
        });

        let results: Vec<Result<(Vec<DiscoverableSkill>, String, bool)>> =
            futures::future::join_all(fetch_tasks).await;

        for (repo, result) in enabled_repos.into_iter().zip(results) {
            match result {
                Ok((repo_skills, resolved_branch, from_head_symref)) => {
                    if let Some(db) = db {
                        if from_head_symref {
                            Self::persist_resolved_branch(db, &repo, &resolved_branch);
                        }
                    }
                    skills.extend(repo_skills);
                }
                Err(e) => {
                    log::warn!("获取仓库 {}/{} 技能失败: {}", repo.owner, repo.name, e);
                    failures.push(Self::repo_failure(&repo, e.to_string()));
                }
            }
        }

        // 去重并排序
        Self::deduplicate_discoverable_skills(&mut skills);
        skills.sort_by_key(|skill| skill.name.to_lowercase());

        Ok(SkillDiscoveryResult { skills, failures })
    }

    /// 只发现一个仓库（添加、重新启用仓库时用），不碰其它仓库。
    pub async fn discover_repo(&self, repo: &SkillRepo) -> SkillDiscoveryResult {
        self.discover_repo_inner(repo, None).await
    }

    pub(crate) async fn discover_repo_for_db(
        &self,
        db: &Database,
        repo: &SkillRepo,
    ) -> SkillDiscoveryResult {
        self.discover_repo_inner(repo, Some(db)).await
    }

    async fn discover_repo_inner(
        &self,
        repo: &SkillRepo,
        db: Option<&Database>,
    ) -> SkillDiscoveryResult {
        if !repo.enabled {
            return SkillDiscoveryResult::default();
        }
        match self.fetch_repo_skills_cached(repo, false).await {
            Ok((mut skills, resolved_branch, from_head_symref)) => {
                if let Some(db) = db {
                    if from_head_symref {
                        Self::persist_resolved_branch(db, repo, &resolved_branch);
                    }
                }
                Self::deduplicate_discoverable_skills(&mut skills);
                skills.sort_by_key(|skill| skill.name.to_lowercase());
                SkillDiscoveryResult {
                    skills,
                    failures: Vec::new(),
                }
            }
            Err(e) => {
                log::warn!("获取仓库 {}/{} 技能失败: {}", repo.owner, repo.name, e);
                SkillDiscoveryResult {
                    skills: Vec::new(),
                    failures: vec![Self::repo_failure(repo, e.to_string())],
                }
            }
        }
    }

    pub(super) fn persist_resolved_branch(db: &Database, repo: &SkillRepo, resolved_branch: &str) {
        if Self::is_default_branch_sentinel(&repo.branch)
            && !Self::is_default_branch_sentinel(resolved_branch)
        {
            if let Err(e) = db.update_skill_repo_branch(
                &repo.host,
                &repo.owner,
                &repo.name,
                &repo.branch,
                resolved_branch,
            ) {
                log::warn!(
                    "回写仓库 {}/{} 真实分支 {} 失败: {}",
                    repo.owner,
                    repo.name,
                    resolved_branch,
                    e
                );
            }
        }
    }

    pub(super) fn discovery_cache_key(repo: &SkillRepo) -> RepoCacheKey {
        (
            repo.host.clone(),
            repo.owner.clone(),
            repo.name.clone(),
            repo.branch.clone(),
        )
    }

    pub(super) fn discovery_cache_path() -> PathBuf {
        get_app_config_dir().join(DISCOVERY_CACHE_FILENAME)
    }

    /// 懒加载本地发现缓存文件（损坏或缺失时忽略）。
    pub(super) fn ensure_discovery_cache_loaded(&self) {
        if self.discovery_cache_loaded.swap(true, Ordering::AcqRel) {
            return;
        }
        let path = Self::discovery_cache_path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return;
        };
        let Ok(entries) = serde_json::from_str::<Vec<DiskDiscoveryCacheEntry>>(&raw) else {
            log::warn!("忽略损坏的技能发现缓存: {}", path.display());
            return;
        };
        let mut cache = self.discovery_cache.lock_recover();
        for entry in entries {
            let key = (entry.host, entry.owner, entry.name, entry.branch);
            cache.entry(key).or_insert(CachedRepoDiscovery {
                fetched_at: None,
                commit_sha: entry.commit_sha,
                resolved_branch: entry.resolved_branch,
                skills: entry.skills,
            });
        }
    }

    /// 在串行落盘锁内读取内存最新快照并写入磁盘，确保旧扫描不会覆盖后发生的清理或更新。
    pub(super) fn persist_discovery_cache(&self) {
        let _disk_guard = self.discovery_disk_lock.lock_recover();
        let snapshot = self.discovery_cache.lock_recover().clone();
        Self::save_discovery_cache_to_disk(&snapshot);
    }

    pub(super) fn save_discovery_cache_to_disk(cache: &HashMap<RepoCacheKey, CachedRepoDiscovery>) {
        let entries: Vec<DiskDiscoveryCacheEntry> = cache
            .iter()
            .map(
                |((host, owner, name, branch), value)| DiskDiscoveryCacheEntry {
                    host: host.clone(),
                    owner: owner.clone(),
                    name: name.clone(),
                    branch: branch.clone(),
                    commit_sha: value.commit_sha.clone(),
                    resolved_branch: value.resolved_branch.clone(),
                    skills: value.skills.clone(),
                },
            )
            .collect();
        let Ok(bytes) = serde_json::to_vec(&entries) else {
            return;
        };
        let path = Self::discovery_cache_path();
        if let Err(e) = crate::config::atomic_write_private(&path, &bytes) {
            log::warn!("保存技能发现缓存失败 ({}): {}", path.display(), e);
        }
    }

    /// 先查缓存；需要刷新时先通过轻量 `info/refs` 比对 commit SHA，未变更直接跳过下载。
    ///
    /// 返回 `(skills, resolved_branch, from_head_symref)`。
    /// 缓存时间记的是开始下载的时刻：前端取消请求停不掉这里的下载，较早开始、
    /// 较晚结束的旧下载不能覆盖之后（比如强制刷新）写入的结果。
    pub(super) async fn fetch_repo_skills_cached(
        &self,
        repo: &SkillRepo,
        force_refresh: bool,
    ) -> Result<(Vec<DiscoverableSkill>, String, bool)> {
        #[cfg(test)]
        let use_disk_cache = self.repo_fixture.is_none();
        #[cfg(not(test))]
        let use_disk_cache = true;

        if use_disk_cache {
            self.ensure_discovery_cache_loaded();
        }

        let key = Self::discovery_cache_key(repo);
        if !force_refresh {
            let cache = self.discovery_cache.lock_recover();
            if let Some(entry) = cache.get(&key) {
                if entry
                    .fetched_at
                    .is_some_and(|fetched_at| fetched_at.elapsed() < DISCOVERY_CACHE_TTL)
                {
                    return Ok((entry.skills.clone(), entry.resolved_branch.clone(), false));
                }
            }
        }

        let started_at = Instant::now();
        let epoch = self.credential_epoch.load(Ordering::SeqCst);

        #[cfg(test)]
        if let Some(fixture) = &self.repo_fixture {
            Self::validate_repo(repo)?;
            let mut skills = Vec::new();
            let resolved_branch = if Self::is_default_branch_sentinel(&repo.branch) {
                "main".to_string()
            } else {
                repo.branch.clone()
            };
            let mut resolved_repo = repo.clone();
            resolved_repo.branch = resolved_branch.clone();
            self.scan_dir_recursive(fixture, fixture, &resolved_repo, &mut skills)?;
            Self::keep_one_per_install_name(&mut skills);
            self.store_discovery_cache(
                repo,
                started_at,
                epoch,
                None,
                resolved_branch.clone(),
                skills.clone(),
                false,
                false,
            );
            return Ok((skills, resolved_branch, false));
        }

        Self::validate_repo(repo)?;
        let token = self.repo_token(&repo.host).await?;
        let token_ref = token.as_deref();
        let platform = self.resolve_platform(&repo.host, token_ref, epoch).await;

        let probed = self.probe_repo_ref(repo, platform, token_ref).await;
        if let Some(probed) = &probed {
            let probed_key = (
                repo.host.clone(),
                repo.owner.clone(),
                repo.name.clone(),
                probed.branch.clone(),
            );
            let matched = {
                let mut cache = self.discovery_cache.lock_recover();
                let found = cache
                    .get(&key)
                    .or_else(|| cache.get(&probed_key))
                    .filter(|entry| {
                        entry.commit_sha.as_deref() == Some(probed.commit_sha.as_str())
                            && entry.resolved_branch == probed.branch
                    })
                    .cloned();
                if let Some(mut entry) = found {
                    if self.credential_epoch.load(Ordering::SeqCst) == epoch {
                        entry.fetched_at = Some(started_at);
                        let target_key = if Self::is_default_branch_sentinel(&repo.branch)
                            && probed.from_head_symref
                        {
                            probed_key
                        } else {
                            key.clone()
                        };
                        cache.insert(target_key, entry.clone());
                    }
                    Some((entry.skills, entry.resolved_branch, probed.from_head_symref))
                } else {
                    None
                }
            };
            if let Some(hit) = matched {
                return Ok(hit);
            }
        }

        // 优先按探测到的不可变 commit SHA 下载归档，确保扫描结果与缓存 SHA 严格对应；
        // 仅当 commit 归档 404（如探测与下载间隙被 force-push）或未探测到时才回退分支下载，
        // 且分支回退结果不绑定未校验的 SHA。
        let (archive_file, resolved_branch, verified_sha, from_head_symref) = match &probed {
            Some(probed) => match self
                .download_commit_archive_bounded(repo, platform, &probed.commit_sha, token_ref)
                .await
            {
                Ok(file) => (
                    file,
                    probed.branch.clone(),
                    Some(probed.commit_sha.clone()),
                    probed.from_head_symref,
                ),
                Err(e) if Self::is_branch_not_found(&e) => {
                    let mut fallback_repo = repo.clone();
                    fallback_repo.branch = probed.branch.clone();
                    let (file, branch) = self
                        .download_repo_archive_bounded(&fallback_repo, platform, token_ref)
                        .await?;
                    let trusted_symref = probed.from_head_symref && branch == probed.branch;
                    (file, branch, None, trusted_symref)
                }
                Err(e) => return Err(e),
            },
            None => {
                let (file, branch) = self
                    .download_repo_archive_bounded(repo, platform, token_ref)
                    .await?;
                (file, branch, None, false)
            }
        };

        let mut resolved_repo = repo.clone();
        resolved_repo.branch = resolved_branch.clone();
        let skills = tokio::task::spawn_blocking(move || {
            Self::scan_skills_from_archive(archive_file, &resolved_repo, platform)
        })
        .await
        .map_err(|e| anyhow!("扫描仓库归档任务异常中止: {e}"))??;

        self.store_discovery_cache(
            repo,
            started_at,
            epoch,
            verified_sha,
            resolved_branch.clone(),
            skills.clone(),
            use_disk_cache,
            from_head_symref,
        );

        Ok((skills, resolved_branch, from_head_symref))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn store_discovery_cache(
        &self,
        repo: &SkillRepo,
        started_at: Instant,
        epoch: u64,
        commit_sha: Option<String>,
        resolved_branch: String,
        skills: Vec<DiscoverableSkill>,
        persist_disk: bool,
        from_head_symref: bool,
    ) {
        let store_branch = if Self::is_default_branch_sentinel(&repo.branch) && from_head_symref {
            resolved_branch.clone()
        } else {
            repo.branch.clone()
        };
        let key = (
            repo.host.clone(),
            repo.owner.clone(),
            repo.name.clone(),
            store_branch,
        );
        let updated = {
            let mut cache = self.discovery_cache.lock_recover();
            if self.credential_epoch.load(Ordering::SeqCst) != epoch {
                return;
            }
            let can_write = cache
                .get(&key)
                .is_none_or(|entry| entry.fetched_at.is_none_or(|t| t <= started_at));
            if !can_write {
                return;
            }
            cache.insert(
                key,
                CachedRepoDiscovery {
                    fetched_at: Some(started_at),
                    commit_sha,
                    resolved_branch,
                    skills,
                },
            );
            true
        };
        if updated && persist_disk {
            self.persist_discovery_cache();
        }
    }

    /// 仅从 ZIP 归档中读取 `SKILL.md` 条目解析元数据，避免发现阶段把整个仓库解压落盘。
    ///
    /// 同时解析文件级与目录级符号链接，并对累计解压字节与保留元数据总量施加独立预算。
    pub(super) fn scan_skills_from_archive<R: std::io::Read + std::io::Seek>(
        reader: R,
        repo: &SkillRepo,
        platform: RepoPlatform,
    ) -> Result<Vec<DiscoverableSkill>> {
        let mut archive = Self::open_downloaded_archive(reader)?;
        if archive.is_empty() {
            return Err(anyhow!(format_skill_error(
                "EMPTY_ARCHIVE",
                &[],
                Some("checkRepoUrl"),
            )));
        }
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            let count = archive.len().to_string();
            let limit = MAX_ARCHIVE_ENTRIES.to_string();
            return Err(anyhow!(format_skill_error(
                "ARCHIVE_TOO_MANY_ENTRIES",
                &[("count", &count), ("limit", &limit)],
                Some("checkZipContent"),
            )));
        }

        let root_name = {
            let first = archive.by_index(0)?;
            first.name().split('/').next().unwrap_or("").to_string()
        };

        let mut regular_candidates: Vec<(String, String, SkillMetadata)> = Vec::new();
        let mut symlinks: Vec<(String, String)> = Vec::new();
        let mut scan_bytes: u64 = 0;
        let mut metadata_bytes: usize = 0;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            if file.is_dir() {
                continue;
            }
            let Some(safe_path) = file.enclosed_name() else {
                continue;
            };
            let Ok(relative_path) = safe_path.strip_prefix(&root_name) else {
                continue;
            };
            if relative_path
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            {
                continue;
            }

            if file.is_symlink() {
                let Some(target) = Self::read_symlink_target_within(&mut file, &mut scan_bytes)?
                else {
                    continue;
                };
                if let Some(resolved_target) =
                    Self::resolve_archive_symlink_target(relative_path, &target)
                {
                    let link_rel = relative_path.to_string_lossy().replace('\\', "/");
                    // 防止目标包含链接自身（如 `dir/link -> ..`）造成递归膨胀
                    if link_rel != resolved_target
                        && !link_rel.starts_with(&format!("{resolved_target}/"))
                    {
                        symlinks.push((link_rel, resolved_target));
                    }
                }
                continue;
            }

            if relative_path.file_name() != Some(OsStr::new("SKILL.md")) {
                continue;
            }

            let parent_rel = relative_path
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let doc_path = relative_path.to_string_lossy().replace('\\', "/");
            let content = Self::read_limited_utf8(&mut file, MAX_SKILL_MD_BYTES, &mut scan_bytes)?;
            let meta = Self::parse_skill_metadata_str(&content);
            Self::charge_metadata_budget(&mut metadata_bytes, &meta, &parent_rel)?;
            regular_candidates.push((parent_rel, doc_path, meta));
        }

        let mut candidates = regular_candidates.clone();

        for (link_rel, resolved_target) in symlinks {
            let is_skill_md_link = link_rel == "SKILL.md" || link_rel.ends_with("/SKILL.md");
            if is_skill_md_link {
                let full_name = format!("{root_name}/{resolved_target}");
                let Ok(mut target_file) = archive.by_name(&full_name) else {
                    continue;
                };
                if target_file.is_dir() || target_file.is_symlink() {
                    continue;
                }
                let parent_rel = link_rel
                    .rsplit_once('/')
                    .map(|(head, _)| head.to_string())
                    .unwrap_or_default();
                let content =
                    Self::read_limited_utf8(&mut target_file, MAX_SKILL_MD_BYTES, &mut scan_bytes)?;
                let meta = Self::parse_skill_metadata_str(&content);
                Self::charge_metadata_budget(&mut metadata_bytes, &meta, &parent_rel)?;
                candidates.push((parent_rel, link_rel, meta));
                continue;
            }

            // 目录符号链接（如 `skills/alias -> ../real-skill`）：把目标目录及其子树下的
            // SKILL.md 投影到链接路径下，与全量解压时的 `resolve_symlinks_in_dir` 一致。
            let target_prefix = format!("{resolved_target}/");
            for (parent_rel, _, meta) in &regular_candidates {
                let mapped_parent = if parent_rel == &resolved_target {
                    link_rel.clone()
                } else if let Some(rest) = parent_rel.strip_prefix(&target_prefix) {
                    format!("{link_rel}/{rest}")
                } else {
                    continue;
                };
                Self::charge_metadata_budget(&mut metadata_bytes, meta, &mapped_parent)?;
                let mapped_doc = format!("{mapped_parent}/SKILL.md");
                candidates.push((mapped_parent, mapped_doc, meta.clone()));
            }
        }

        // 与 scan_dir_recursive 保持一致：父目录已有 SKILL.md 时不再收入其子目录。
        candidates.sort_by(|a, b| {
            let depth = |p: &str| {
                if p.is_empty() {
                    0
                } else {
                    p.matches('/').count() + 1
                }
            };
            depth(&a.0).cmp(&depth(&b.0)).then_with(|| a.0.cmp(&b.0))
        });

        let mut accepted_dirs: HashSet<String> = HashSet::new();
        let mut skills = Vec::new();
        for (parent_rel, doc_path, meta) in candidates {
            if accepted_dirs.contains("") || Self::has_ancestor_dir(&parent_rel, &accepted_dirs) {
                continue;
            }
            let directory = if parent_rel.is_empty() {
                repo.name.clone()
            } else {
                parent_rel.clone()
            };
            accepted_dirs.insert(parent_rel);
            skills.push(Self::build_skill_from_parsed_metadata(
                meta, &directory, &doc_path, repo, platform,
            ));
        }

        Self::keep_one_per_install_name(&mut skills);
        Ok(skills)
    }

    fn charge_discovery_scan_budget(total_bytes: &mut u64, amount: u64) -> Result<()> {
        if total_bytes.saturating_add(amount) > MAX_DISCOVERY_SCAN_BYTES {
            let limit_mb = (MAX_DISCOVERY_SCAN_BYTES / 1024 / 1024).to_string();
            return Err(anyhow!(format_skill_error(
                "ARCHIVE_TOO_LARGE",
                &[("limit_mb", &limit_mb)],
                Some("checkZipContent"),
            )));
        }
        *total_bytes += amount;
        Ok(())
    }

    fn charge_metadata_budget(
        total_bytes: &mut usize,
        meta: &SkillMetadata,
        parent_rel: &str,
    ) -> Result<()> {
        let entry_bytes = meta.name.as_deref().map_or(0, str::len)
            + meta.description.as_deref().map_or(0, str::len)
            + parent_rel.len();
        if total_bytes.saturating_add(entry_bytes) > MAX_DISCOVERY_METADATA_BYTES {
            let limit_mb = (MAX_DISCOVERY_METADATA_BYTES / 1024 / 1024).to_string();
            return Err(anyhow!(format_skill_error(
                "ARCHIVE_TOO_LARGE",
                &[("limit_mb", &limit_mb)],
                Some("checkZipContent"),
            )));
        }
        *total_bytes += entry_bytes;
        Ok(())
    }

    fn read_symlink_target_within<R: std::io::Read>(
        reader: &mut R,
        total_bytes: &mut u64,
    ) -> Result<Option<String>> {
        let mut raw = Vec::new();
        let mut limited = std::io::Read::take(reader, MAX_SYMLINK_TARGET_BYTES + 1);
        std::io::Read::read_to_end(&mut limited, &mut raw)?;
        Self::charge_discovery_scan_budget(total_bytes, raw.len() as u64)?;
        if raw.len() as u64 > MAX_SYMLINK_TARGET_BYTES {
            return Ok(None);
        }
        Ok(String::from_utf8(raw)
            .ok()
            .map(|target| target.trim().to_string()))
    }

    fn read_limited_utf8<R: std::io::Read>(
        reader: &mut R,
        limit: u64,
        total_bytes: &mut u64,
    ) -> Result<String> {
        let mut raw = Vec::new();
        let mut buf = [0u8; 8 * 1024];
        let mut remaining = limit;
        while remaining > 0 {
            let to_read = (remaining as usize).min(buf.len());
            let read = reader.read(&mut buf[..to_read])?;
            if read == 0 {
                break;
            }
            Self::charge_discovery_scan_budget(total_bytes, read as u64)?;
            raw.extend_from_slice(&buf[..read]);
            remaining -= read as u64;
        }
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }

    fn resolve_archive_symlink_target(relative_symlink: &Path, target: &str) -> Option<String> {
        if target.is_empty() || target.starts_with('/') || target.starts_with('\\') {
            return None;
        }
        let mut parts: Vec<String> = relative_symlink
            .parent()
            .map(|p| {
                p.components()
                    .filter_map(|c| match c {
                        Component::Normal(s) => Some(s.to_string_lossy().to_string()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        for seg in target.split(['/', '\\']) {
            match seg {
                "" | "." => continue,
                ".." => {
                    parts.pop()?;
                }
                other => parts.push(other.to_string()),
            }
        }
        (!parts.is_empty()).then(|| parts.join("/"))
    }

    fn has_ancestor_dir(parent_rel: &str, accepted: &HashSet<String>) -> bool {
        if accepted.contains(parent_rel) {
            return true;
        }
        let mut cursor = parent_rel;
        while let Some((head, _)) = cursor.rsplit_once('/') {
            if accepted.contains(head) {
                return true;
            }
            cursor = head;
        }
        false
    }

    /// 同一仓库里安装目录名（路径最后一段）相同的技能只留一条。
    ///
    /// 安装只取最后一段当目录名，同仓库同名的第二条装下去只会复用第一条，
    /// 列出来只是一模一样的重复行。同时给多个客户端发技能的仓库常见这种布局，
    /// 如 tw93/Waza 的 `skills/check` 和 `plugins/waza/skills/check`。
    /// 留离仓库根最近的那条，层级相同按路径字典序，结果与扫描顺序无关。
    ///
    /// 只用于发现列表：检查更新直接调 `scan_dir_recursive`，要看到每一份。
    pub(super) fn keep_one_per_install_name(skills: &mut Vec<DiscoverableSkill>) {
        let install_name = |skill: &DiscoverableSkill| {
            skill
                .directory
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_lowercase()
        };
        skills.sort_by(|a, b| {
            let depth = |skill: &DiscoverableSkill| skill.directory.matches('/').count();
            depth(a)
                .cmp(&depth(b))
                .then_with(|| a.directory.cmp(&b.directory))
        });
        let mut seen = HashSet::new();
        skills.retain(|skill| seen.insert(install_name(skill)));
    }

    /// 递归扫描目录查找 SKILL.md
    pub(super) fn scan_dir_recursive(
        &self,
        current_dir: &Path,
        base_dir: &Path,
        repo: &SkillRepo,
        skills: &mut Vec<DiscoverableSkill>,
    ) -> Result<()> {
        let skill_md = current_dir.join("SKILL.md");

        if skill_md.exists() {
            let directory = if current_dir == base_dir {
                repo.name.clone()
            } else {
                current_dir
                    .strip_prefix(base_dir)
                    .unwrap_or(current_dir)
                    .to_string_lossy()
                    .replace('\\', "/")
            };

            let doc_path = skill_md
                .strip_prefix(base_dir)
                .unwrap_or(skill_md.as_path())
                .to_string_lossy()
                .replace('\\', "/");

            if let Ok(skill) =
                self.build_skill_from_metadata(&skill_md, &directory, &doc_path, repo)
            {
                skills.push(skill);
            }

            return Ok(());
        }

        for entry in fs::read_dir(current_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                self.scan_dir_recursive(&path, base_dir, repo, skills)?;
            }
        }

        Ok(())
    }

    /// 从 SKILL.md 构建技能对象
    pub(super) fn build_skill_from_metadata(
        &self,
        skill_md: &Path,
        directory: &str,
        doc_path: &str,
        repo: &SkillRepo,
    ) -> Result<DiscoverableSkill> {
        let meta = self.parse_skill_metadata(skill_md)?;
        Ok(Self::build_skill_from_parsed_metadata(
            meta,
            directory,
            doc_path,
            repo,
            self.known_platform(&repo.host),
        ))
    }

    fn build_skill_from_parsed_metadata(
        meta: SkillMetadata,
        directory: &str,
        doc_path: &str,
        repo: &SkillRepo,
        platform: RepoPlatform,
    ) -> DiscoverableSkill {
        DiscoverableSkill {
            key: format!("{}/{}:{}", repo.owner, repo.name, directory),
            name: meta.name.unwrap_or_else(|| directory.to_string()),
            description: meta.description.unwrap_or_default(),
            directory: directory.to_string(),
            readme_url: Self::build_skill_doc_url(
                platform,
                &repo.host,
                &repo.owner,
                &repo.name,
                &repo.branch,
                doc_path,
            ),
            repo_host: repo.host.clone(),
            repo_owner: repo.owner.clone(),
            repo_name: repo.name.clone(),
            repo_branch: repo.branch.clone(),
        }
    }

    /// 解析技能元数据
    pub(super) fn parse_skill_metadata(&self, path: &Path) -> Result<SkillMetadata> {
        Self::parse_skill_metadata_static(path)
    }

    /// 静态方法：解析技能元数据
    pub(super) fn parse_skill_metadata_static(path: &Path) -> Result<SkillMetadata> {
        let content = fs::read_to_string(path)?;
        Ok(Self::parse_skill_metadata_str(&content))
    }

    pub(super) fn parse_skill_metadata_str(content: &str) -> SkillMetadata {
        let content = content.trim_start_matches('\u{feff}');

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return SkillMetadata {
                name: None,
                description: None,
            };
        }

        let front_matter = parts[1].trim();
        serde_yaml::from_str(front_matter).unwrap_or(SkillMetadata {
            name: None,
            description: None,
        })
    }

    /// 从 SKILL.md 读取名称和描述，不存在则用目录名兜底
    pub(super) fn read_skill_name_desc(
        skill_md: &Path,
        fallback_name: &str,
    ) -> (String, Option<String>) {
        if skill_md.exists() {
            match Self::parse_skill_metadata_static(skill_md) {
                Ok(meta) => (
                    meta.name.unwrap_or_else(|| fallback_name.to_string()),
                    meta.description,
                ),
                Err(_) => (fallback_name.to_string(), None),
            }
        } else {
            (fallback_name.to_string(), None)
        }
    }
}

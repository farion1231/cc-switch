//! 发现功能：扫描仓库里的技能并解析元数据。

use super::*;

impl SkillService {
    // ========== 发现功能（保留原有逻辑）==========

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
        let mut skills = Vec::new();
        let mut failures = Vec::new();

        // 仅使用启用的仓库
        let enabled_repos: Vec<SkillRepo> = repos.into_iter().filter(|repo| repo.enabled).collect();

        let fetch_tasks = enabled_repos
            .iter()
            .map(|repo| self.fetch_repo_skills_cached(repo, force_refresh));

        let results: Vec<Result<Vec<DiscoverableSkill>>> =
            futures::future::join_all(fetch_tasks).await;

        for (repo, result) in enabled_repos.into_iter().zip(results) {
            match result {
                Ok(repo_skills) => skills.extend(repo_skills),
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
        if !repo.enabled {
            return SkillDiscoveryResult::default();
        }
        match self.fetch_repo_skills_cached(repo, false).await {
            Ok(mut skills) => {
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

    pub(super) fn discovery_cache_key(repo: &SkillRepo) -> RepoCacheKey {
        (
            repo.host.clone(),
            repo.owner.clone(),
            repo.name.clone(),
            repo.branch.clone(),
        )
    }

    /// 先查缓存，过期或 `force_refresh` 时再下载；下载成功才写回缓存。
    ///
    /// 缓存时间记的是开始下载的时刻：前端取消请求停不掉这里的下载，较早开始、
    /// 较晚结束的旧下载不能覆盖之后（比如强制刷新）写入的结果。
    pub(super) async fn fetch_repo_skills_cached(
        &self,
        repo: &SkillRepo,
        force_refresh: bool,
    ) -> Result<Vec<DiscoverableSkill>> {
        let key = Self::discovery_cache_key(repo);
        if !force_refresh {
            let cache = self.discovery_cache.lock_recover();
            if let Some((fetched_at, skills)) = cache.get(&key) {
                if fetched_at.elapsed() < DISCOVERY_CACHE_TTL {
                    return Ok(skills.clone());
                }
            }
        }

        let started_at = Instant::now();
        let skills = self.fetch_repo_skills(repo).await?;
        let mut cache = self.discovery_cache.lock_recover();
        if cache
            .get(&key)
            .is_none_or(|(fetched_at, _)| *fetched_at <= started_at)
        {
            cache.insert(key, (started_at, skills.clone()));
        }
        Ok(skills)
    }

    /// 从仓库获取技能列表
    pub(super) async fn fetch_repo_skills(
        &self,
        repo: &SkillRepo,
    ) -> Result<Vec<DiscoverableSkill>> {
        let (temp_guard, resolved_branch) = self.download_repo_bounded(repo).await?;

        let mut skills = Vec::new();
        let scan_dir = temp_guard.path();
        let mut resolved_repo = repo.clone();
        resolved_repo.branch = resolved_branch;
        self.scan_dir_recursive(scan_dir, scan_dir, &resolved_repo, &mut skills)?;

        Ok(skills)
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

        Ok(DiscoverableSkill {
            key: format!("{}/{}:{}", repo.owner, repo.name, directory),
            name: meta.name.unwrap_or_else(|| directory.to_string()),
            description: meta.description.unwrap_or_default(),
            directory: directory.to_string(),
            readme_url: Self::build_skill_doc_url(
                self.known_platform(&repo.host),
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
        })
    }

    /// 解析技能元数据
    pub(super) fn parse_skill_metadata(&self, path: &Path) -> Result<SkillMetadata> {
        Self::parse_skill_metadata_static(path)
    }

    /// 静态方法：解析技能元数据
    pub(super) fn parse_skill_metadata_static(path: &Path) -> Result<SkillMetadata> {
        let content = fs::read_to_string(path)?;
        let content = content.trim_start_matches('\u{feff}');

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return Ok(SkillMetadata {
                name: None,
                description: None,
            });
        }

        let front_matter = parts[1].trim();
        let meta: SkillMetadata = serde_yaml::from_str(front_matter).unwrap_or(SkillMetadata {
            name: None,
            description: None,
        });

        Ok(meta)
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

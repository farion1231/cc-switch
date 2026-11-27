//! 更新检测：目录哈希、检查与执行更新、本地元数据刷新。

use super::*;

impl SkillService {
    // ========== 更新检测 ==========

    /// 计算目录内容的 SHA-256 哈希
    ///
    /// 递归遍历目录下所有非隐藏文件，按相对路径字典序排列，
    /// 将 "相对路径\0内容\0" 逐文件 feed 给同一个 hasher。
    /// Python 字节码（`__pycache__/`、`*.pyc`）不计入，见 `collect_files_for_hash`。
    pub fn compute_dir_hash(dir: &Path) -> Result<String> {
        Self::compute_dir_hash_with(dir, true)
    }

    /// 旧版算法：字节码也计入。库里在 #7772 修复前保存的 content_hash 是用它
    /// 算的；只给 check_updates 识别并迁移这类旧缓存用。
    pub(super) fn compute_legacy_dir_hash(dir: &Path) -> Result<String> {
        Self::compute_dir_hash_with(dir, false)
    }

    pub(super) fn compute_dir_hash_with(dir: &Path, skip_bytecode: bool) -> Result<String> {
        use sha2::{Digest, Sha256};

        let mut files: Vec<PathBuf> = Vec::new();
        Self::collect_files_for_hash(dir, dir, skip_bytecode, &mut files)?;
        files.sort();

        let mut hasher = Sha256::new();
        for file_path in &files {
            let relative = file_path.strip_prefix(dir).unwrap_or(file_path);
            let rel_str = relative.to_string_lossy().replace('\\', "/");
            hasher.update(rel_str.as_bytes());
            hasher.update(b"\0");
            let content = fs::read(file_path)
                .with_context(|| format!("读取文件失败: {}", file_path.display()))?;
            hasher.update(&content);
            hasher.update(b"\0");
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// 递归收集目录下所有非隐藏文件。
    ///
    /// `__pycache__/` 目录与 `*.pyc` 文件是 Python 运行/导入时生成的字节码缓存，
    /// 不属于 Skill 自身内容；只跑一遍 Skill（不修改任何源文件）就会让目录哈希
    /// 发生变化，结果是 Skill 会被报为「有更新」，用户点更新又会触发整目录替
    /// 换（见 #7772）。忽略这两类条目后，运行 Skill 不再影响哈希。
    #[allow(clippy::only_used_in_recursion)]
    pub(super) fn collect_files_for_hash(
        base: &Path,
        current: &Path,
        skip_bytecode: bool,
        files: &mut Vec<PathBuf>,
    ) -> Result<()> {
        let entries = fs::read_dir(current)
            .with_context(|| format!("读取目录失败: {}", current.display()))?;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                if skip_bytecode && name == "__pycache__" {
                    continue;
                }
                Self::collect_files_for_hash(base, &path, skip_bytecode, files)?;
            } else {
                if skip_bytecode && name.ends_with(".pyc") {
                    continue;
                }
                files.push(path);
            }
        }
        Ok(())
    }

    /// Pi copy 部署的破坏性操作需要比较完整目录树，不能沿用更新检测哈希：
    /// 隐藏文件、空目录或文件类型变化都意味着用户已修改原生目录。
    pub(super) fn compute_pi_deployment_hash(dir: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};

        let mut entries = Vec::new();
        Self::collect_tree_entries(dir, &mut entries)?;
        entries.sort();

        let mut hasher = Sha256::new();
        for path in entries {
            let relative = path.strip_prefix(dir).unwrap_or(&path);
            hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
            hasher.update(b"\0");

            let metadata = fs::symlink_metadata(&path)?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                hasher.update(b"link\0");
                hasher.update(fs::read_link(&path)?.to_string_lossy().as_bytes());
            } else if file_type.is_dir() {
                hasher.update(b"dir\0");
            } else if file_type.is_file() {
                hasher.update(b"file\0");
                hasher.update(fs::read(&path)?);
            } else {
                hasher.update(b"other\0");
            }
            hasher.update(b"\0");
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    pub(super) fn collect_tree_entries(current: &Path, entries: &mut Vec<PathBuf>) -> Result<()> {
        for entry in
            fs::read_dir(current).with_context(|| format!("读取目录失败: {}", current.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            entries.push(path.clone());
            if file_type.is_dir() {
                Self::collect_tree_entries(&path, entries)?;
            }
        }
        Ok(())
    }

    /// 判定 check_updates 应使用的本地哈希。
    ///
    /// 次序关键：必须先确认 SSOT 目录存在，再信任数据库缓存的 content_hash。
    /// 换机恢复数据库备份后 Skill 文件不随库迁移，此时缓存哈希仍在而目录已
    /// 缺失；若先取缓存会把这类 Skill 误报成「无更新」，缺失状态被永久掩盖。
    /// 目录缺失 → 返回 None，与远端哈希必然不等，Skill 进入更新列表，
    /// update_skill 对缺失目录本就容忍（存在才删、随后整体复制），点更新即可重建。
    ///
    /// 返回 `(hash, freshly_computed)`；freshly_computed 表示哈希是现场算出的，
    /// 调用方应回填数据库缓存。
    pub(super) fn local_hash_for_update_check(
        ssot_dir: &Path,
        raw_directory: &str,
        cached_hash: Option<&str>,
    ) -> Option<(String, bool)> {
        // 脏 directory 会让 compute_dir_hash 递归遍历任意目录，且哈希结果经
        // 「有无更新」的界面状态泄露少量信息；无法安全拼路径时也不能报
        // 「可更新」——update_skill 会在同一校验上硬报错，只能沿用缓存。
        let directory = match Self::require_valid_directory(raw_directory) {
            Ok(d) => d,
            Err(err) => {
                log::warn!("Skill directory 非法，跳过本地目录检查: {err}");
                return cached_hash.map(|h| (h.to_string(), false));
            }
        };

        let local_dir = ssot_dir.join(&directory);
        if !local_dir.exists() {
            return None;
        }

        if let Some(h) = cached_hash {
            return Some((h.to_string(), false));
        }

        match Self::compute_dir_hash(&local_dir) {
            Ok(h) => Some((h, true)),
            Err(_) => None,
        }
    }

    /// 库里缓存的哈希是否是该远端目录按旧算法（字节码计入）算出的值。
    ///
    /// 上游仓库自带 `.pyc` / `__pycache__` 时，#7772 修复前安装的 Skill 缓存的
    /// 是旧算法的哈希，与新算法的远端哈希必然不等；不识别就会对未变化的
    /// Skill 误报「有更新」，用户点更新又是一次整目录替换。远端不含字节码时
    /// 新旧算法结果相同，前面的相等比较已经放行，不会走到这里。
    pub(super) fn is_legacy_hash_of(cached_hash: &str, remote_dir: &Path) -> bool {
        Self::compute_legacy_dir_hash(remote_dir).is_ok_and(|legacy| legacy == cached_hash)
    }

    /// 检查所有已安装 Skill 的更新
    ///
    /// 仅检查有 repo_owner 的 Skill（本地 Skill 跳过），
    /// 按仓库分组下载，避免重复下载同一仓库。
    pub async fn check_updates(&self, db: &Arc<Database>) -> Result<Vec<SkillUpdateInfo>> {
        Ok(self.check_updates_report(db).await?.updates)
    }

    /// 检查更新，并逐仓库报告没读到的仓库（下载失败 / 超时 / 扫描失败）。
    pub async fn check_updates_report(&self, db: &Arc<Database>) -> Result<SkillUpdateCheckResult> {
        let skills = db.get_all_installed_skills()?;
        let mut updates = Vec::new();
        let mut failures = Vec::new();

        // 按 (host, owner, name, branch) 分组
        let mut repo_groups: HashMap<RepoCacheKey, Vec<InstalledSkill>> = HashMap::new();

        for skill in skills.into_values() {
            let (owner, name) = match (&skill.repo_owner, &skill.repo_name) {
                (Some(o), Some(n)) => (o.clone(), n.clone()),
                _ => continue,
            };
            let host = skill.repo_host.clone().unwrap_or_else(default_repo_host);
            let branch = skill
                .repo_branch
                .clone()
                .unwrap_or_else(|| "main".to_string());
            repo_groups
                .entry((host, owner, name, branch))
                .or_default()
                .push(skill);
        }

        let ssot_dir = Self::get_ssot_dir()?;

        for ((host, owner, name, branch), group_skills) in &repo_groups {
            let repo = SkillRepo {
                host: host.clone(),
                owner: owner.clone(),
                name: name.clone(),
                branch: branch.clone(),
                enabled: true,
            };

            // 下载仓库 ZIP
            let (temp_guard, _used_branch) = match self.download_repo_bounded(&repo).await {
                Ok(result) => result,
                Err(e) => {
                    log::warn!("检查更新时下载 {}/{} 失败: {e}", owner, name);
                    failures.push(Self::repo_failure(&repo, e.to_string()));
                    continue;
                }
            };
            let temp_dir = temp_guard.path();

            // 扫描仓库中的所有 Skill 目录
            let mut remote_skills: Vec<DiscoverableSkill> = Vec::new();
            if let Err(e) = self.scan_dir_recursive(temp_dir, temp_dir, &repo, &mut remote_skills) {
                // 扫到一半失败：已扫到的照常比对，但如实报告这个仓库可能不完整
                log::warn!("检查更新时扫描 {}/{} 失败: {e}", owner, name);
                failures.push(Self::repo_failure(&repo, e.to_string()));
            }

            // Remote I/O is complete. Stabilize the local DB + SSOT while hashes
            // are read and any missing hash metadata is backfilled.
            let _state_guard = skill_state_read_guard();

            for skill in group_skills {
                let remote_match = Self::find_remote_skill_for_install(
                    &remote_skills,
                    &skill.directory,
                    skill.readme_url.as_deref(),
                );
                let remote_skill_dir = match remote_match {
                    Some(rs) => match Self::resolve_skill_source_dir(temp_dir, &rs.directory) {
                        Some(path) => path,
                        None => continue,
                    },
                    None => continue,
                };

                let remote_hash = match Self::compute_dir_hash(&remote_skill_dir) {
                    Ok(h) => h,
                    Err(e) => {
                        log::warn!("计算远程哈希失败 {}: {e}", skill.id);
                        continue;
                    }
                };

                let local_hash = match Self::local_hash_for_update_check(
                    &ssot_dir,
                    &skill.directory,
                    skill.content_hash.as_deref(),
                ) {
                    Some((h, freshly_computed)) => {
                        if freshly_computed {
                            let _ = db.update_skill_hash(&skill.id, &h, 0);
                            Some(h)
                        } else if h != remote_hash && Self::is_legacy_hash_of(&h, &remote_skill_dir)
                        {
                            // 旧版缓存（字节码计入）与上游一致：迁移成新算法的
                            // 值，不报「有更新」；更新时间保持原值。
                            let _ = db.update_skill_hash(&skill.id, &remote_hash, skill.updated_at);
                            Some(remote_hash.clone())
                        } else {
                            Some(h)
                        }
                    }
                    None => None,
                };

                if local_hash.as_deref() != Some(&remote_hash) {
                    updates.push(SkillUpdateInfo {
                        id: skill.id.clone(),
                        name: skill.name.clone(),
                        current_hash: local_hash,
                        remote_hash,
                    });
                }
            }
        }

        // 分组用的是 HashMap，排一下让结果稳定
        failures.sort_by(|a: &SkillRepoFailure, b| (&a.owner, &a.name).cmp(&(&b.owner, &b.name)));
        Ok(SkillUpdateCheckResult { updates, failures })
    }

    pub(super) fn repo_failure(repo: &SkillRepo, error: String) -> SkillRepoFailure {
        SkillRepoFailure {
            owner: repo.owner.clone(),
            name: repo.name.clone(),
            branch: repo.branch.clone(),
            error,
        }
    }

    pub(super) fn download_timeout_error(repo: &SkillRepo) -> String {
        let seconds = DOWNLOAD_TOTAL_TIMEOUT.as_secs().to_string();
        format_skill_error(
            "DOWNLOAD_TIMEOUT",
            &[
                ("owner", &repo.owner),
                ("name", &repo.name),
                ("timeout", &seconds),
            ],
            Some("checkNetwork"),
        )
    }

    /// 下载仓库并套上总时长上限。停滞判定在 `download_and_extract` 里逐块做，
    /// 这里只兜底「一直有零星数据却始终下不完」这种不该无限挂着的情况。
    pub(super) async fn download_repo_bounded(
        &self,
        repo: &SkillRepo,
    ) -> Result<(tempfile::TempDir, String)> {
        timeout(DOWNLOAD_TOTAL_TIMEOUT, self.download_repo(repo))
            .await
            .map_err(|_| anyhow!(Self::download_timeout_error(repo)))?
    }

    /// 持久化更新后的 Skill 元数据，并重新读取数据库中的权威应用启用状态。
    ///
    /// 更新过程包含网络下载，期间用户可能切换启用状态或卸载 Skill。这里必须
    /// 使用只更新现有记录的 DAO，避免旧快照覆盖 `enabled_*`，也避免已卸载记录
    /// 被重新插入。
    pub(super) fn persist_updated_skill_metadata(
        db: &Arc<Database>,
        updated_skill: &InstalledSkill,
    ) -> Result<InstalledSkill> {
        if !db.update_skill_metadata(updated_skill)? {
            return Err(anyhow!("Skill no longer installed: {}", updated_skill.id));
        }

        db.get_installed_skill(&updated_skill.id)?
            .ok_or_else(|| anyhow!("Skill no longer installed: {}", updated_skill.id))
    }

    /// 更新单个 Skill（重新下载并替换本地文件）
    pub async fn update_skill(&self, db: &Arc<Database>, skill_id: &str) -> Result<InstalledSkill> {
        let mut skill = db
            .get_installed_skill(skill_id)?
            .ok_or_else(|| anyhow!("Skill not found: {skill_id}"))?;
        skill.apps.pi = Self::skill_exists_in_app(&skill.directory, &AppType::Pi);
        skill.apps.omp = Self::skill_exists_in_app(&skill.directory, &AppType::Omp);

        // 本函数后续三种危险操作都用 directory 拼路径：备份源（把任意目录复制进
        // 备份区并在界面列出）、remove_dir_all（删任意目录）、copy_dir_recursive
        // （把远端仓库内容写到任意路径）。校验必须在这三者之前。
        Self::require_valid_directory(&skill.directory)?;

        let (owner, name, branch) = match (&skill.repo_owner, &skill.repo_name) {
            (Some(o), Some(n)) => (
                o.clone(),
                n.clone(),
                skill
                    .repo_branch
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            ),
            _ => return Err(anyhow!("Cannot update local skill: {skill_id}")),
        };

        let host = skill.repo_host.clone().unwrap_or_else(default_repo_host);
        let repo = SkillRepo {
            host: host.clone(),
            owner: owner.clone(),
            name: name.clone(),
            branch: branch.clone(),
            enabled: true,
        };

        let ssot_dir = Self::get_ssot_dir()?;
        if skill.apps.pi {
            Self::get_distinct_app_skills_dir(&ssot_dir, &AppType::Pi)?;
        }
        if skill.apps.omp {
            Self::get_distinct_app_skills_dir(&ssot_dir, &AppType::Omp)?;
        }

        // 下载仓库
        let (temp_guard, used_branch) = self.download_repo_bounded(&repo).await?;
        let temp_dir = temp_guard.path();

        // 在解压的仓库中查找 Skill 源目录
        let mut remote_skills: Vec<DiscoverableSkill> = Vec::new();
        let _ = self.scan_dir_recursive(temp_dir, temp_dir, &repo, &mut remote_skills);

        let remote_match = Self::find_remote_skill_for_install(
            &remote_skills,
            &skill.directory,
            skill.readme_url.as_deref(),
        )
        .ok_or_else(|| {
            anyhow!(format_skill_error(
                "SKILL_DIR_NOT_FOUND",
                &[("path", &skill.directory)],
                Some("checkRepoUrl"),
            ))
        })?;

        let source =
            Self::resolve_skill_source_dir(temp_dir, &remote_match.directory).ok_or_else(|| {
                let missing = temp_dir.join(&remote_match.directory).display().to_string();
                anyhow!(format_skill_error(
                    "SKILL_DIR_NOT_FOUND",
                    &[("path", &missing)],
                    Some("checkRepoUrl"),
                ))
            })?;

        let canonical_temp = temp_dir
            .canonicalize()
            .unwrap_or_else(|_| temp_dir.to_path_buf());
        let resolved_doc_path = source
            .canonicalize()
            .ok()
            .and_then(|source| Self::doc_path_for_source(&canonical_temp, &source));

        // Downloads do not mutate local state, so acquire only now and hold the
        // guard through the SSOT replacement, DB metadata update, and app sync.
        let _state_guard = skill_state_write_guard();

        // 下载和扫描期间用户可能已经卸载了该 Skill。必须在任何备份、删除或
        // 复制之前重新确认记录仍存在；否则即使最终的 metadata UPDATE 能发现
        // 缺行，这里也会先把已卸载的 SSOT 目录重新创建出来。
        let mut current_skill = db
            .get_installed_skill(&skill.id)?
            .ok_or_else(|| anyhow!("Skill no longer installed: {}", skill.id))?;
        if current_skill.directory != skill.directory
            || current_skill.repo_host != skill.repo_host
            || current_skill.repo_owner != skill.repo_owner
            || current_skill.repo_name != skill.repo_name
            || current_skill.repo_branch != skill.repo_branch
            || current_skill.installed_at != skill.installed_at
        {
            return Err(anyhow!("Skill changed during update: {}", skill.id));
        }
        Self::require_valid_directory(&current_skill.directory)?;
        current_skill.apps.pi = Self::skill_exists_in_app(&current_skill.directory, &AppType::Pi);
        current_skill.apps.omp = Self::skill_exists_in_app(&current_skill.directory, &AppType::Omp);
        let skill = current_skill;

        let dest = ssot_dir.join(&skill.directory);
        let mut deployments = Vec::new();
        for app in [
            AppType::Pi,
            AppType::Mcode,
            AppType::Agy,
            AppType::Kimi,
            AppType::Dsh,
            AppType::Omp,
            AppType::Copilot,
            AppType::WorkBuddy,
            AppType::Codebuddy,
        ] {
            if skill.apps.is_enabled_for(&app) {
                let destination =
                    Self::get_distinct_app_skills_dir(&ssot_dir, &app)?.join(&skill.directory);
                if let Some(deployment) =
                    Self::inspect_pi_skill_destination(&dest, &destination, &skill.directory, &app)?
                {
                    deployments.push((destination, deployment, app));
                }
            }
        }

        // 备份旧文件
        if let Err(e) = Self::create_uninstall_backup(&skill) {
            log::warn!("更新前备份 Skill {} 失败: {e}", skill.directory);
        }

        if !(skill.apps.mcode
            || skill.apps.agy
            || skill.apps.kimi
            || skill.apps.dsh
            || skill.apps.copilot
            || skill.apps.workbuddy
            || skill.apps.codebuddy)
        {
            Self::replace_dir_staged(&source, &dest)?;
        }

        // 计算新哈希 + 解析新元数据
        let metadata_source = if skill.apps.mcode
            || skill.apps.agy
            || skill.apps.kimi
            || skill.apps.dsh
            || skill.apps.copilot
            || skill.apps.workbuddy
            || skill.apps.codebuddy
        {
            &source
        } else {
            &dest
        };
        let new_hash = Self::compute_dir_hash(metadata_source).ok();
        let skill_md = metadata_source.join("SKILL.md");
        let (new_name, new_description) = Self::read_skill_name_desc(&skill_md, &skill.directory);

        // 更新 readme_url
        let doc_path = Self::choose_doc_path(
            resolved_doc_path,
            skill.readme_url.as_deref(),
            &skill.directory,
        );
        let readme_url = Self::build_skill_doc_url(
            self.known_platform(&host),
            &host,
            &owner,
            &name,
            &used_branch,
            &doc_path,
        );

        let updated_metadata = InstalledSkill {
            id: skill.id.clone(),
            name: new_name,
            description: new_description,
            directory: skill.directory.clone(),
            repo_host: skill.repo_host.clone(),
            repo_owner: skill.repo_owner.clone(),
            repo_name: skill.repo_name.clone(),
            repo_branch: Some(used_branch),
            readme_url,
            apps: skill.apps.clone(),
            installed_at: skill.installed_at,
            content_hash: new_hash,
            updated_at: chrono::Utc::now().timestamp(),
        };

        let mut updated_skill = if skill.apps.mcode
            || skill.apps.agy
            || skill.apps.kimi
            || skill.apps.dsh
            || skill.apps.copilot
            || skill.apps.workbuddy
            || skill.apps.codebuddy
        {
            Self::update_managed_skill_files(
                &source,
                &dest,
                &skill.directory,
                &deployments,
                &[
                    AppType::Mcode,
                    AppType::Agy,
                    AppType::Kimi,
                    AppType::Dsh,
                    AppType::Copilot,
                    AppType::WorkBuddy,
                    AppType::Codebuddy,
                ]
                .iter()
                .filter(|app| skill.apps.is_enabled_for(app))
                .map(|app| Self::get_app_skills_dir(app).map(|dir| dir.join(&skill.directory)))
                .collect::<Result<Vec<_>>>()?,
                || {
                    if !db.update_skill_metadata(&updated_metadata)? {
                        return Err(anyhow!("Skill no longer installed: {}", skill.id));
                    }
                    Ok(updated_metadata)
                },
            )?
        } else {
            for (destination, deployment, app) in deployments {
                Self::refresh_pi_skill_destination(
                    &dest,
                    &destination,
                    &updated_metadata.directory,
                    &deployment,
                    &app,
                )?;
            }
            Self::persist_updated_skill_metadata(db, &updated_metadata)?
        };
        updated_skill.apps.pi = Self::skill_exists_in_app(&updated_skill.directory, &AppType::Pi);
        updated_skill.apps.omp = Self::skill_exists_in_app(&updated_skill.directory, &AppType::Omp);

        // 同步到所有已启用的应用目录
        for app in updated_skill.apps.enabled_apps() {
            if matches!(
                app,
                AppType::Pi
                    | AppType::Mcode
                    | AppType::Agy
                    | AppType::Kimi
                    | AppType::Dsh
                    | AppType::Omp
                    | AppType::Copilot
                    | AppType::WorkBuddy
                    | AppType::Codebuddy
            ) {
                continue;
            }
            if let Err(e) = Self::sync_to_app_dir(&updated_skill.directory, &app) {
                log::warn!("同步更新后的 skill 到 {:?} 失败: {e}", app);
            }
        }

        log::info!("Skill {} 更新成功", updated_skill.name);
        Ok(updated_skill)
    }

    /// Copy `source` into a sibling staging directory first and only then swap it
    /// in for `dest`, so a copy that fails part-way (I/O error, permissions, path
    /// length) leaves the installed skill untouched instead of deleting it (#9521).
    pub(super) fn replace_dir_staged(source: &Path, dest: &Path) -> Result<()> {
        let parent = dest.parent().context("Skill destination has no parent")?;
        fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix(".skill-update-")
            .tempdir_in(parent)?;
        let replacement = staging.path().join("new");
        Self::copy_dir_recursive(source, &replacement)?;

        let previous = staging.path().join("old");
        let existed = dest.exists() || Self::is_symlink(dest);
        if existed {
            fs::rename(dest, &previous)?;
        }
        if let Err(error) = fs::rename(&replacement, dest) {
            if existed {
                if let Err(restore_error) = fs::rename(&previous, dest) {
                    let kept = staging.keep();
                    return Err(anyhow!(
                        "{error}; restoring the previous skill failed: {restore_error}; previous copy kept at {}",
                        kept.join("old").display()
                    ));
                }
            }
            return Err(error.into());
        }
        // The new copy is in place; a leftover staging dir is hidden from skill
        // scans, so failing to delete it only warrants a warning.
        let staging_path = staging.path().to_path_buf();
        if let Err(error) = staging.close() {
            log::warn!(
                "Skill updated, but removing staging dir {} failed: {error}",
                staging_path.display()
            );
        }
        Ok(())
    }

    // Keep the old SSOT and native copies until metadata is committed. In particular,
    // a failed native write must not make the old copy appear to be a user edit.
    #[cfg(test)]
    pub(super) fn update_mcode_skill_files<T>(
        source: &Path,
        ssot: &Path,
        directory: &str,
        deployments: &[(PathBuf, PiSkillDeployment, AppType)],
        commit: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        Self::update_managed_skill_files(
            source,
            ssot,
            directory,
            deployments,
            &[Self::get_app_skills_dir(&AppType::Mcode)?.join(directory)],
            commit,
        )
    }

    pub(super) fn update_managed_skill_files<T>(
        source: &Path,
        ssot: &Path,
        directory: &str,
        deployments: &[(PathBuf, PiSkillDeployment, AppType)],
        native_targets: &[PathBuf],
        commit: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        Self::validate_sync_source_dir(source, directory)?;
        let mut targets = vec![(ssot.to_path_buf(), false)];
        for (destination, deployment, app) in deployments {
            Self::ensure_pi_skill_destination_matches(ssot, destination, directory, app)?;
            targets.push((
                destination.clone(),
                matches!(deployment, PiSkillDeployment::Symlink { .. }),
            ));
        }
        for native in native_targets {
            if !native.exists() && !Self::is_symlink(native) {
                targets.push((native.clone(), Self::get_sync_method() != SyncMethod::Copy));
            }
        }

        let mut staged = Vec::new();
        for (destination, symlink) in targets {
            let parent = destination
                .parent()
                .context("Skill destination has no parent")?;
            fs::create_dir_all(parent)?;
            let staging = tempfile::Builder::new()
                .prefix(".managed-skill-update-")
                .tempdir_in(parent)?;
            let replacement = staging.path().join("new");
            if symlink {
                if let Err(error) = Self::create_symlink(ssot, &replacement) {
                    if Self::get_sync_method() == SyncMethod::Symlink
                        || Self::is_symlink(&destination)
                    {
                        return Err(error);
                    }
                    Self::copy_dir_recursive(source, &replacement)?;
                }
            } else {
                Self::copy_dir_recursive(source, &replacement)?;
            }
            staged.push((destination, staging));
        }

        let mut changed = 0;
        let result = (|| {
            for (destination, staging) in &staged {
                if destination.exists() || Self::is_symlink(destination) {
                    fs::rename(destination, staging.path().join("old"))?;
                }
                changed += 1;
                fs::rename(staging.path().join("new"), destination)?;
            }
            commit()
        })();
        if let Err(error) = result {
            let mut rollback_errors = Vec::new();
            for (destination, staging) in staged[..changed].iter().rev() {
                let restore = || -> Result<()> {
                    Self::remove_path(destination)?;
                    let old = staging.path().join("old");
                    if old.exists() || Self::is_symlink(&old) {
                        fs::rename(old, destination)?;
                    }
                    Ok(())
                };
                if let Err(restore_error) = restore() {
                    rollback_errors.push(format!("{}: {restore_error}", destination.display()));
                }
            }
            if !rollback_errors.is_empty() {
                let backups: Vec<_> = staged.into_iter().map(|(_, stage)| stage.keep()).collect();
                return Err(anyhow!(
                    "{error}; MiniMax Code Skill rollback failed: {}; backups: {backups:?}",
                    rollback_errors.join("; ")
                ));
            }
            return Err(error);
        }
        result
    }

    /// 为本地 Skill（无仓库来源）刷新过期元数据（name/description/content_hash）。
    ///
    /// 手动编辑 SSOT 目录下的 SKILL.md 后，数据库元数据仍是安装时的快照，
    /// 技能列表会一直显示旧描述（issue #7848）。按 content_hash 失配检测
    /// 本地文件变化并回写重新解析的结果。仓库 Skill 不在此处理：其哈希
    /// 语义由 check_updates / update_skill 管理，重算会干扰远端更新比对。
    ///
    /// 目录哈希可能很慢，因此只在快照技能列表时短暂持有读锁，扫描本身不占
    /// 全局写锁（否则技能开关等同步命令会一直等扫描结束）；写回时再短持写锁，
    /// 重新读取并核对安装代次、目录与旧哈希，避免覆盖扫描期间的并发更新。
    pub fn refresh_local_skill_metadata(db: &Arc<Database>) -> Result<usize> {
        let ssot_dir = Self::get_ssot_dir()?;
        let candidates = Self::collect_local_skill_metadata_candidates(&ssot_dir, db)?;
        if candidates.is_empty() {
            return Ok(0);
        }

        let _state_guard = skill_state_write_guard();
        // 等待写锁期间可能发生了 SSOT 迁移；旧目录上的扫描结果不能写回。
        if Self::get_ssot_dir()?.as_path() != ssot_dir {
            return Ok(0);
        }
        Self::apply_local_skill_metadata_candidates(db, candidates)
    }

    #[cfg(test)]
    pub(super) fn refresh_local_skill_metadata_in(
        ssot_dir: &Path,
        db: &Arc<Database>,
    ) -> Result<usize> {
        let candidates = Self::collect_local_skill_metadata_candidates(ssot_dir, db)?;
        if candidates.is_empty() {
            return Ok(0);
        }
        let _state_guard = skill_state_write_guard();
        Self::apply_local_skill_metadata_candidates(db, candidates)
    }

    pub(super) fn collect_local_skill_metadata_candidates(
        ssot_dir: &Path,
        db: &Arc<Database>,
    ) -> Result<Vec<LocalSkillMetadataCandidate>> {
        let skills = {
            let _state_guard = skill_state_read_guard();
            db.get_all_installed_skills()?
        };
        let mut candidates = Vec::new();

        for skill in skills.into_values() {
            if skill.repo_owner.is_some() {
                continue;
            }
            let directory = match Self::require_valid_directory(&skill.directory) {
                Ok(d) => d,
                Err(err) => {
                    log::warn!("Skill directory 非法，跳过元数据刷新: {err}");
                    continue;
                }
            };
            let local_dir = ssot_dir.join(&directory);
            let skill_md = local_dir.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            let new_hash = match Self::compute_dir_hash(&local_dir) {
                Ok(h) => h,
                Err(err) => {
                    log::warn!("计算 Skill {directory} 哈希失败，跳过元数据刷新: {err}");
                    continue;
                }
            };
            if skill.content_hash.as_deref() == Some(new_hash.as_str()) {
                continue;
            }
            let (new_name, new_description) = Self::read_skill_name_desc(&skill_md, &directory);
            candidates.push(LocalSkillMetadataCandidate {
                id: skill.id,
                directory,
                installed_at: skill.installed_at,
                previous_hash: skill.content_hash,
                name: new_name,
                description: new_description,
                new_hash,
            });
        }

        Ok(candidates)
    }

    /// 调用方必须持有 [`skill_state_write_guard`]。
    pub(super) fn apply_local_skill_metadata_candidates(
        db: &Arc<Database>,
        candidates: Vec<LocalSkillMetadataCandidate>,
    ) -> Result<usize> {
        let mut refreshed = 0;
        for candidate in candidates {
            let Some(mut skill) = db.get_installed_skill(&candidate.id)? else {
                continue;
            };
            if skill.installed_at != candidate.installed_at
                || skill.directory != candidate.directory
                || skill.repo_owner.is_some()
                || skill.content_hash != candidate.previous_hash
            {
                continue;
            }
            skill.name = candidate.name;
            skill.description = candidate.description;
            skill.content_hash = Some(candidate.new_hash);
            skill.updated_at = chrono::Utc::now().timestamp();
            if db.update_skill_metadata(&skill)? {
                refreshed += 1;
                log::info!("已刷新本地 Skill {} 的过期元数据", candidate.directory);
            }
        }
        Ok(refreshed)
    }
}

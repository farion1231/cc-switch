//! 卸载备份：创建、列出、删除、恢复。

use super::*;

impl SkillService {
    pub fn list_backups() -> Result<Vec<SkillBackupEntry>> {
        let backup_dir = Self::get_backup_dir()?;
        let mut entries = Vec::new();

        for entry in fs::read_dir(&backup_dir)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    log::warn!("读取 Skill 备份目录项失败: {err}");
                    continue;
                }
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            match Self::read_backup_metadata(&path) {
                Ok(metadata) => entries.push(SkillBackupEntry {
                    backup_id: entry.file_name().to_string_lossy().to_string(),
                    backup_path: path.to_string_lossy().to_string(),
                    created_at: metadata.backup_created_at,
                    skill: metadata.skill,
                }),
                Err(err) => {
                    log::warn!("解析 Skill 备份失败 {}: {err:#}", path.display());
                }
            }
        }

        entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
        Ok(entries)
    }

    pub fn delete_backup(backup_id: &str) -> Result<()> {
        let backup_path = Self::backup_path_for_id(backup_id)?;
        let metadata = fs::symlink_metadata(&backup_path)
            .with_context(|| format!("failed to access {}", backup_path.display()))?;

        if !metadata.is_dir() {
            return Err(anyhow!(
                "Skill backup is not a directory: {}",
                backup_path.display()
            ));
        }

        fs::remove_dir_all(&backup_path)
            .with_context(|| format!("failed to delete {}", backup_path.display()))?;

        log::info!("Skill 备份已删除: {}", backup_path.display());
        Ok(())
    }

    pub fn restore_from_backup(
        db: &Arc<Database>,
        backup_id: &str,
        current_app: &AppType,
    ) -> Result<InstalledSkill> {
        let _state_guard = skill_state_write_guard();
        let backup_path = Self::backup_path_for_id(backup_id)?;
        let metadata = Self::read_backup_metadata(&backup_path)?;
        let backup_skill_dir = backup_path.join("skill");
        if !backup_skill_dir.join("SKILL.md").exists() {
            return Err(anyhow!(
                "Skill backup is invalid or missing SKILL.md: {}",
                backup_path.display()
            ));
        }

        let existing_skills = db.get_all_installed_skills()?;
        if existing_skills.contains_key(&metadata.skill.id)
            || existing_skills.values().any(|skill| {
                skill
                    .directory
                    .eq_ignore_ascii_case(&metadata.skill.directory)
            })
        {
            return Err(anyhow!(
                "Skill already exists, please uninstall the current one first: {}",
                metadata.skill.directory
            ));
        }

        // meta.json 是文件内容（可能来自手工放置或不可信备份），directory 此前
        // 未经任何校验就直接 join——可穿越出 SSOT 目录写任意位置。必须先校验。
        let directory = Self::require_valid_directory(&metadata.skill.directory)?;

        let ssot_dir = Self::get_ssot_dir()?;
        let restore_path = ssot_dir.join(&directory);
        if restore_path.exists() || Self::is_symlink(&restore_path) {
            return Err(anyhow!(
                "Restore target already exists: {}",
                restore_path.display()
            ));
        }

        let mut restored_skill = metadata.skill;
        restored_skill.directory = directory;
        restored_skill.installed_at = Utc::now().timestamp();
        restored_skill.apps = SkillApps::only(current_app);
        restored_skill.updated_at = 0;

        Self::copy_dir_recursive(&backup_skill_dir, &restore_path)?;

        // 重新计算内容哈希
        restored_skill.content_hash = Self::compute_dir_hash(&restore_path).ok();

        if let Err(err) = db.save_skill(&restored_skill) {
            let _ = fs::remove_dir_all(&restore_path);
            return Err(err.into());
        }

        if !restored_skill.apps.is_empty() {
            if let Err(err) = Self::sync_to_app_dir(&restored_skill.directory, current_app) {
                let _ = db.delete_skill(&restored_skill.id);
                let _ = fs::remove_dir_all(&restore_path);
                return Err(err);
            }
        }

        log::info!(
            "Skill {} 已从备份恢复到 {}",
            restored_skill.name,
            restore_path.display()
        );

        Ok(restored_skill)
    }

    pub(super) fn resolve_uninstall_backup_source_excluding(
        skill: &InstalledSkill,
        excluded_paths: &[PathBuf],
    ) -> Result<Option<PathBuf>> {
        // 返回值会被整目录复制进 ~/.cc-switch/skill-backups/ 并由 get_skill_backups
        // 在界面上列出——脏 directory 在这里等于任意文件读取 + 外泄通道。
        let directory = Self::require_valid_directory(&skill.directory)?;

        let ssot_path = Self::get_ssot_dir()?.join(&directory);
        if ssot_path.is_dir()
            && !excluded_paths
                .iter()
                .any(|path| Self::paths_overlap(&ssot_path, path))
        {
            return Ok(Some(ssot_path));
        }

        for app in AppType::all() {
            if matches!(
                app,
                AppType::Mcode
                    | AppType::Agy
                    | AppType::Kimi
                    | AppType::Dsh
                    | AppType::Copilot
                    | AppType::WorkBuddy
                    | AppType::Codebuddy
            ) && !skill.apps.is_enabled_for(&app)
            {
                continue;
            }
            let app_dir = match Self::get_app_skills_dir(&app) {
                Ok(dir) => dir,
                Err(_) => continue,
            };
            let candidate = app_dir.join(&directory);
            if candidate.is_dir()
                && !excluded_paths
                    .iter()
                    .any(|path| Self::paths_overlap(&candidate, path))
            {
                return Ok(Some(candidate));
            }
        }

        Ok(None)
    }

    pub(super) fn sanitize_backup_segment(segment: &str) -> String {
        let sanitized = segment
            .chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
                _ => '-',
            })
            .collect::<String>()
            .trim_matches('-')
            .to_string();

        if sanitized.is_empty() {
            "skill".to_string()
        } else {
            sanitized
        }
    }

    pub(super) fn cleanup_old_skill_backups(dir: &Path) -> Result<()> {
        let mut entries = fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                if !metadata.is_dir() {
                    return None;
                }
                Some((entry.path(), metadata.modified().ok()))
            })
            .collect::<Vec<_>>();

        if entries.len() <= SKILL_BACKUP_RETAIN_COUNT {
            return Ok(());
        }

        entries.sort_by_key(|(_, modified)| *modified);
        let remove_count = entries.len().saturating_sub(SKILL_BACKUP_RETAIN_COUNT);

        for (path, _) in entries.into_iter().take(remove_count) {
            fs::remove_dir_all(&path)?;
        }

        Ok(())
    }

    pub(super) fn backup_path_for_id(backup_id: &str) -> Result<PathBuf> {
        if backup_id.contains("..")
            || backup_id.contains('/')
            || backup_id.contains('\\')
            || backup_id.trim().is_empty()
        {
            return Err(anyhow!("Invalid backup id: {backup_id}"));
        }

        Ok(Self::get_backup_dir()?.join(backup_id))
    }

    pub(super) fn read_backup_metadata(backup_path: &Path) -> Result<SkillBackupMetadata> {
        let metadata_path = backup_path.join("meta.json");
        let content = fs::read_to_string(&metadata_path)
            .with_context(|| format!("failed to read {}", metadata_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", metadata_path.display()))
    }

    pub(super) fn create_uninstall_backup(skill: &InstalledSkill) -> Result<Option<PathBuf>> {
        Self::create_uninstall_backup_excluding(skill, &[])
    }

    pub(super) fn create_uninstall_backup_excluding(
        skill: &InstalledSkill,
        excluded_paths: &[PathBuf],
    ) -> Result<Option<PathBuf>> {
        let Some(source_path) =
            Self::resolve_uninstall_backup_source_excluding(skill, excluded_paths)?
        else {
            log::warn!(
                "Skill {} 卸载前未找到可备份的目录，将跳过备份",
                skill.directory
            );
            return Ok(None);
        };

        let backup_root = Self::get_backup_dir()?;
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let slug = Self::sanitize_backup_segment(&skill.directory);
        let mut backup_path = backup_root.join(format!("{timestamp}_{slug}"));
        let mut counter = 1;
        while backup_path.exists() {
            backup_path = backup_root.join(format!("{timestamp}_{slug}_{counter}"));
            counter += 1;
        }

        let write_backup = || -> Result<()> {
            let skill_backup_dir = backup_path.join("skill");
            Self::copy_dir_recursive(&source_path, &skill_backup_dir)?;

            let metadata = SkillBackupMetadata {
                skill: skill.clone(),
                backup_created_at: Utc::now().timestamp(),
                source_path: source_path.to_string_lossy().to_string(),
            };
            let metadata_path = backup_path.join("meta.json");
            let metadata_json = serde_json::to_string_pretty(&metadata)
                .context("failed to serialize skill backup metadata")?;
            fs::write(&metadata_path, metadata_json)
                .with_context(|| format!("failed to write {}", metadata_path.display()))?;
            Ok(())
        };

        if let Err(err) = write_backup() {
            let _ = fs::remove_dir_all(&backup_path);
            return Err(err);
        }

        if let Err(err) = Self::cleanup_old_skill_backups(&backup_root) {
            log::warn!("清理旧 Skill 备份失败: {err:#}");
        }

        log::info!(
            "Skill {} 已在卸载前备份到 {}",
            skill.name,
            backup_path.display()
        );

        Ok(Some(backup_path))
    }

    /// 解析 ZIP 中的符号链接：将目标内容复制到 symlink 位置
    ///
    /// GitHub ZIP 归档保留了 symlink 元数据，解压时可通过 `is_symlink()` 检测。
    /// 此方法将 symlink 解析为实际文件/目录内容（而非创建真实 symlink），
    /// 以确保跨平台兼容且 skill 内容自包含。
    pub(super) fn resolve_symlinks_in_dir(
        base_dir: &Path,
        symlinks: &[(PathBuf, String)],
        total_bytes: &mut u64,
    ) -> Result<()> {
        // 规范化 base_dir（macOS 上 /tmp → /private/tmp，需保持一致）
        let canonical_base = base_dir
            .canonicalize()
            .unwrap_or_else(|_| base_dir.to_path_buf());

        for (link_path, target) in symlinks {
            // 计算 symlink 的父目录，然后拼接目标的相对路径
            let parent = link_path.parent().unwrap_or(base_dir);
            let resolved = parent.join(target);

            // 规范化路径（解析 .. 等）
            let resolved = match resolved.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    log::warn!(
                        "Symlink 目标不存在，跳过: {} -> {}",
                        link_path.display(),
                        target
                    );
                    continue;
                }
            };

            // 安全检查一：确保目标在 base_dir 内（防止路径穿越）
            if !resolved.starts_with(&canonical_base) {
                log::warn!(
                    "Symlink 目标超出仓库范围，跳过: {} -> {}",
                    link_path.display(),
                    resolved.display()
                );
                continue;
            }

            // 安全检查二：目标不能包含 link 自身。上面那条防的是「跑出 base」，
            // 防不住「套进自己」——`dir/link -> ..` 解析后正是 base 本身，完全
            // 合规，随后递归复制会把归档根复制进自己的子目录；每递归一层都重新
            // 看到刚落盘的副本，目录树逐层膨胀直到 PATH_MAX 才失败。
            //
            // 比较必须在**规范形式**上做：`enclosed_name()` 不规范化路径，只保证
            // 净深度非负，所以 link_path 里可能带着未消解的 `..`（`e/../d/self`）。
            // 拿它按字面跟 canonicalize 过的 resolved 比组件，第一段就会错开
            // （`e` vs `d`），检查形同虚设。link_path 自身此刻尚未落盘，但它的父
            // 目录一定存在——`resolved` 能 canonicalize 成功就蕴含了这一点。
            let canonical_link = match parent.canonicalize() {
                Ok(canonical_parent) => match link_path.file_name() {
                    Some(name) => canonical_parent.join(name),
                    None => canonical_parent,
                },
                // 父目录都不存在时退回字面形式：此时 resolved 多半也解析不出来，
                // 上面就已经 continue 了；留着只是不让守卫在意外形状上 panic。
                Err(_) => match link_path.strip_prefix(base_dir) {
                    Ok(relative) => canonical_base.join(relative),
                    Err(_) => link_path.clone(),
                },
            };
            if canonical_link.starts_with(&resolved) {
                log::warn!(
                    "Symlink 目标包含链接自身，跳过（会导致递归自复制）: {} -> {}",
                    link_path.display(),
                    resolved.display()
                );
                continue;
            }

            // 复制目标内容到 symlink 位置。必须与解压循环共用同一个字节预算：
            // 物化走的是这条独立路径，不计费的话「一个大文件 + N 个指向它的
            // symlink」能写下 N 倍字节，而 MAX_ARCHIVE_TOTAL_BYTES 全程显示合规。
            if resolved.is_dir() {
                Self::copy_dir_within_budget(&resolved, link_path, total_bytes)?;
            } else if resolved.is_file() {
                if let Some(parent) = link_path.parent() {
                    Self::create_dir_all_within_budget(parent, total_bytes)?;
                }
                Self::copy_file_within_budget(&resolved, link_path, total_bytes)?;
            }
        }
        Ok(())
    }
}

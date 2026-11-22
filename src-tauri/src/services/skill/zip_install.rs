//! 从本地 ZIP 文件安装。

use super::*;

impl SkillService {
    // ========== 从 ZIP 文件安装 ==========

    /// 从本地 ZIP 文件安装 Skills
    ///
    /// 流程：
    /// 1. 解压 ZIP 到临时目录
    /// 2. 扫描目录查找包含 SKILL.md 的技能
    /// 3. 复制到 SSOT 并保存到数据库
    /// 4. 同步到当前应用目录
    pub fn install_from_zip(
        db: &Arc<Database>,
        zip_path: &Path,
        current_app: &AppType,
    ) -> Result<ZipInstallResult> {
        // 解压到临时目录
        let temp_guard = Self::extract_local_zip(zip_path)?;
        let temp_dir = temp_guard.path();

        // 扫描所有包含 SKILL.md 的目录
        let skill_dirs = Self::scan_skills_in_dir(temp_dir)?;

        if skill_dirs.is_empty() {
            return Err(anyhow!(format_skill_error(
                "NO_SKILLS_IN_ZIP",
                &[],
                Some("checkZipContent"),
            )));
        }

        let _state_guard = skill_state_write_guard();
        let ssot_dir = Self::get_ssot_dir()?;
        let mut installed = Vec::new();
        let mut skipped = Vec::new();
        let existing_skills = db.get_all_installed_skills()?;
        let zip_stem = zip_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        for skill_dir in skill_dirs {
            // 解析元数据（提前解析，用于确定安装名）
            let skill_md = skill_dir.join("SKILL.md");
            let meta = if skill_md.exists() {
                Self::parse_skill_metadata_static(&skill_md).ok()
            } else {
                None
            };

            // 获取目录名称作为安装名
            // 当 SKILL.md 在 ZIP 根目录时，skill_dir == temp_dir，
            // file_name() 会返回临时目录名（如 .tmpDZKGpF），需要回退到其他来源
            let install_name = {
                let dir_name = skill_dir
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                if skill_dir.as_path() == temp_dir
                    || dir_name.is_empty()
                    || dir_name.starts_with('.')
                {
                    // SKILL.md 在根目录：优先用元数据 name，否则用 ZIP 文件名
                    meta.as_ref()
                        .and_then(|m| m.name.as_deref())
                        .and_then(Self::sanitize_install_name)
                        .or_else(|| zip_stem.as_deref().and_then(Self::sanitize_install_name))
                } else {
                    Self::sanitize_install_name(&dir_name)
                        .or_else(|| {
                            meta.as_ref()
                                .and_then(|m| m.name.as_deref())
                                .and_then(Self::sanitize_install_name)
                        })
                        .or_else(|| zip_stem.as_deref().and_then(Self::sanitize_install_name))
                }
            };
            let install_name = match install_name {
                Some(name) => name,
                None => {
                    return Err(anyhow!(format_skill_error(
                        "INVALID_SKILL_DIRECTORY",
                        &[("zip", &zip_path.display().to_string())],
                        Some("checkZipContent"),
                    )));
                }
            };

            // 检查是否已有同名 directory 的 skill
            let conflict = existing_skills
                .values()
                .find(|s| s.directory.eq_ignore_ascii_case(&install_name));

            if let Some(existing) = conflict {
                log::warn!(
                    "Skill directory '{}' already exists (from {}), skipping",
                    install_name,
                    existing.id
                );
                skipped.push(ZipSkippedSkill {
                    directory: install_name,
                    existing_id: existing.id.clone(),
                    existing_name: existing.name.clone(),
                });
                continue;
            }

            let (name, description) = match meta {
                Some(m) => (
                    m.name.unwrap_or_else(|| install_name.clone()),
                    m.description,
                ),
                None => (install_name.clone(), None),
            };

            Self::preflight_install_destination(&skill_dir, &install_name, current_app)?;

            // 复制到 SSOT
            let dest = ssot_dir.join(&install_name);
            if dest.exists() {
                let _ = fs::remove_dir_all(&dest);
            }
            Self::copy_dir_recursive(&skill_dir, &dest)?;

            // 计算内容哈希
            let content_hash = Self::compute_dir_hash(&dest).ok();

            // 创建 InstalledSkill 记录
            let skill = InstalledSkill {
                id: format!("local:{install_name}"),
                name,
                description,
                directory: install_name.clone(),
                repo_host: None,
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps: SkillApps::only(current_app),
                installed_at: chrono::Utc::now().timestamp(),
                content_hash,
                updated_at: 0,
            };

            Self::persist_and_sync_new_skill(db, &skill, current_app)?;

            log::info!(
                "Skill {} installed from ZIP, enabled for {:?}",
                skill.name,
                current_app
            );
            installed.push(skill);
        }

        Ok(ZipInstallResult { installed, skipped })
    }

    /// 解压本地 ZIP 文件到临时目录
    ///
    /// 返回 `TempDir` 而不是 `PathBuf`：调用方在解压之后还有扫描、复制、写库、
    /// 同步等一长串 `?`，任何一处提前返回都会把最多 512 MiB 的临时内容永久留在
    /// 磁盘上。守卫交给调用方持有，清理就变成作用域结束时自动发生，不再依赖每条
    /// 出口都记得手写 `remove_dir_all`（实测漏了不止一条）。
    pub(super) fn extract_local_zip(zip_path: &Path) -> Result<tempfile::TempDir> {
        Self::extract_local_zip_in(zip_path, &std::env::temp_dir())
    }

    /// 与 [`Self::extract_local_zip`] 相同，但临时目录的落点由调用方指定。
    /// 测试用它把解压根钉在私有目录里，而不是劫持进程级 `TMPDIR`——后者会把
    /// 并发测试的临时目录一起吸进被观测目录，"目录必须为空"的断言就会随机失败。
    pub(super) fn extract_local_zip_in(
        zip_path: &Path,
        base_dir: &Path,
    ) -> Result<tempfile::TempDir> {
        let file = fs::File::open(zip_path)
            .with_context(|| format!("Failed to open ZIP file: {}", zip_path.display()))?;

        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("Failed to read ZIP file: {}", zip_path.display()))?;

        if archive.is_empty() {
            return Err(anyhow!(format_skill_error(
                "EMPTY_ARCHIVE",
                &[],
                Some("checkZipContent"),
            )));
        }

        // 与远端归档同一套上限。本地 ZIP 是用户自选文件（信任度更高），但"用户
        // 被诱导打开一个压缩炸弹"仍是常见路径，且两条解压路径共用同一个物化器。
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            let count = archive.len().to_string();
            let limit = MAX_ARCHIVE_ENTRIES.to_string();
            return Err(anyhow!(format_skill_error(
                "ARCHIVE_TOO_MANY_ENTRIES",
                &[("count", &count), ("limit", &limit)],
                Some("checkZipContent"),
            )));
        }

        // 守卫持有到解压全部成功为止：中途任何 `?` 都会让它清掉半成品目录。
        // 原来在这里就 keep()，超限或解压出错都会留下永久残留。
        let temp_dir = tempfile::tempdir_in(base_dir)?;
        let temp_path = temp_dir.path().to_path_buf();

        let mut symlinks: Vec<(PathBuf, String)> = Vec::new();
        let mut total_bytes: u64 = 0;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_path = match file.enclosed_name() {
                Some(path) => path.to_owned(),
                None => continue,
            };

            // `enclosed_name()` 只保证净深度非负，**不消解** `..`。这里没有
            // `strip_prefix` 吃掉深度预算，所以逃不出 temp_path；但留着未消解的
            // 路径会让后面的 symlink 自包含检查失去可比性（`e/../d/self` 与 `d`
            // 逐组件比在第一段就错开）。在入口就把它们挡掉，保证落进 symlinks
            // 表里的路径都是规范形状。
            if file_path
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            {
                log::warn!("跳过越界的压缩包条目: {}", file.name());
                continue;
            }

            let outpath = temp_path.join(&file_path);

            if file.is_symlink() {
                let Some(target) = Self::read_symlink_target(&mut file, &mut total_bytes)? else {
                    log::warn!("跳过目标不合法的 symlink 条目: {}", file.name());
                    continue;
                };
                symlinks.push((outpath, target));
            } else if file.is_dir() {
                Self::create_dir_all_within_budget(&outpath, &mut total_bytes)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    Self::create_dir_all_within_budget(parent, &mut total_bytes)?;
                }
                Self::write_file_within_budget(&mut file, &outpath, &mut total_bytes)?;
            }
        }

        // 解析 symlink
        Self::resolve_symlinks_in_dir(&temp_path, &symlinks, &mut total_bytes)?;

        Ok(temp_dir)
    }

    /// 递归扫描目录查找包含 SKILL.md 的技能目录
    pub(super) fn scan_skills_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut skill_dirs = Vec::new();
        Self::scan_skills_recursive(dir, &mut skill_dirs)?;
        Ok(skill_dirs)
    }

    /// 递归扫描辅助函数
    pub(super) fn scan_skills_recursive(current: &Path, results: &mut Vec<PathBuf>) -> Result<()> {
        // 检查当前目录是否包含 SKILL.md
        let skill_md = current.join("SKILL.md");
        if skill_md.exists() {
            results.push(current.to_path_buf());
            // 找到后不再递归子目录（一个 skill 目录）
            return Ok(());
        }

        // 递归子目录
        if let Ok(entries) = fs::read_dir(current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // 跳过隐藏目录
                    let dir_name = entry.file_name().to_string_lossy().to_string();
                    if dir_name.starts_with('.') {
                        continue;
                    }
                    Self::scan_skills_recursive(&path, results)?;
                }
            }
        }

        Ok(())
    }
}

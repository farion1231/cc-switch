//! 把 SSOT 里的技能同步（链接或复制）到各应用目录，及从应用目录移除。

use super::*;

impl SkillService {
    // ========== 文件同步方法 ==========

    /// 创建符号链接（跨平台）
    ///
    /// - Unix: 使用 std::os::unix::fs::symlink
    /// - Windows: 使用 std::os::windows::fs::symlink_dir
    ///
    /// 命令层用 `to_string()` 把错误交给前端，只显示最外层 context，所以系统错误
    /// （如 Windows 的 `os error 5`）必须写进消息本身，否则用户只看到「失败」。
    pub(super) fn create_symlink(src: &Path, dest: &Path) -> Result<()> {
        #[cfg(unix)]
        let result = std::os::unix::fs::symlink(src, dest);
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_dir(src, dest);
        result.map_err(|error| {
            anyhow!(
                "创建符号链接失败: {} -> {}: {error}",
                src.display(),
                dest.display()
            )
        })
    }

    /// 检查路径是否为符号链接
    pub(super) fn is_symlink(path: &Path) -> bool {
        path.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    pub(super) fn skill_exists_in_app(directory: &str, app: &AppType) -> bool {
        let Ok(directory) = Self::require_valid_directory(directory) else {
            return false;
        };
        let Ok(app_dir) = Self::get_app_skills_dir(app) else {
            return false;
        };
        app_dir.join(directory).is_dir()
    }

    pub(super) fn preflight_install_destination(
        source: &Path,
        directory: &str,
        app: &AppType,
    ) -> Result<()> {
        let ssot_dir = Self::get_ssot_dir()?;
        let app_dir = Self::get_distinct_app_skills_dir(&ssot_dir, app)?;
        if !matches!(
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
            return Ok(());
        }
        let destination = app_dir.join(directory);
        if destination.exists() || Self::is_symlink(&destination) {
            Self::ensure_pi_skill_destination_matches(source, &destination, directory)?;
        }
        Ok(())
    }

    pub(super) fn persist_and_sync_new_skill(
        db: &Arc<Database>,
        skill: &InstalledSkill,
        app: &AppType,
    ) -> Result<()> {
        let source = Self::get_ssot_dir()?.join(&skill.directory);
        Self::preflight_install_destination(&source, &skill.directory, app)?;
        db.save_skill(skill)?;
        if let Err(error) = Self::sync_to_app_dir(&skill.directory, app) {
            if let Err(rollback_error) = db.delete_skill(&skill.id) {
                log::error!(
                    "Failed to roll back Skill {} after sync error: {rollback_error}",
                    skill.id
                );
            }
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn ensure_pi_skill_destination_matches(
        source: &Path,
        destination: &Path,
        directory: &str,
    ) -> Result<()> {
        Self::inspect_pi_skill_destination(source, destination, directory).map(|_| ())
    }

    pub(super) fn link_points_to_managed_copy(resolved: &Path, source: &Path) -> bool {
        matches!(
            (resolved.canonicalize(), source.canonicalize()),
            (Ok(resolved), Ok(source)) if resolved == source
        ) || (!source.exists() && resolved == source)
    }

    pub(super) fn inspect_pi_skill_destination(
        source: &Path,
        destination: &Path,
        directory: &str,
    ) -> Result<Option<PiSkillDeployment>> {
        if !destination.exists() && !Self::is_symlink(destination) {
            return Ok(None);
        }

        if Self::is_symlink(destination) {
            let target = fs::read_link(destination)?;
            let resolved = if target.is_absolute() {
                target
            } else {
                destination
                    .parent()
                    .map(|parent| parent.join(&target))
                    .unwrap_or(target)
            };
            if Self::link_points_to_managed_copy(&resolved, source) {
                return Ok(Some(PiSkillDeployment::Symlink {
                    expected_target: resolved,
                }));
            }
        } else if destination.is_dir() {
            if let (Ok(destination_hash), Ok(source_hash)) = (
                Self::compute_pi_deployment_hash(destination),
                Self::compute_pi_deployment_hash(source),
            ) {
                if destination_hash == source_hash {
                    return Ok(Some(PiSkillDeployment::Copy {
                        expected_hash: destination_hash,
                    }));
                }
            }
        }

        Err(anyhow!(
            "Pi 中已存在同名但内容不同的 Skill，拒绝覆盖或删除: {directory}"
        ))
    }

    pub(super) fn remove_verified_pi_destination(
        source: &Path,
        destination: &Path,
        directory: &str,
    ) -> Result<()> {
        match Self::inspect_pi_skill_destination(source, destination, directory)? {
            Some(_) => Self::remove_path(destination),
            None => Ok(()),
        }
    }

    pub(super) fn refresh_pi_skill_destination(
        source: &Path,
        destination: &Path,
        directory: &str,
        deployment: &PiSkillDeployment,
    ) -> Result<()> {
        Self::validate_sync_source_dir(source, directory)?;

        match deployment {
            PiSkillDeployment::Symlink { expected_target } => {
                if !Self::is_symlink(destination) {
                    return Err(anyhow!(
                        "Pi 中的 Skill 已在操作期间发生变化，拒绝覆盖: {directory}"
                    ));
                }
                let target = fs::read_link(destination)?;
                let resolved = if target.is_absolute() {
                    target
                } else {
                    destination
                        .parent()
                        .map(|parent| parent.join(&target))
                        .unwrap_or(target)
                };
                if &resolved != expected_target {
                    return Err(anyhow!(
                        "Pi 中的 Skill 已在操作期间发生变化，拒绝覆盖: {directory}"
                    ));
                }
                Self::remove_path(destination)?;
                Self::create_symlink(source, destination)?;
            }
            PiSkillDeployment::Copy { expected_hash } => {
                if Self::is_symlink(destination)
                    || !destination.is_dir()
                    || !matches!(
                        Self::compute_pi_deployment_hash(destination),
                        Ok(current_hash) if &current_hash == expected_hash
                    )
                {
                    return Err(anyhow!(
                        "Pi 中的 Skill 已在操作期间发生变化，拒绝覆盖: {directory}"
                    ));
                }
                Self::replace_dest_with_copy(source, destination, directory)?;
            }
        }

        Ok(())
    }

    /// 获取当前同步方式配置
    pub(super) fn get_sync_method() -> SyncMethod {
        crate::settings::get_skill_sync_method()
    }

    /// 同步 Skill 到应用目录（使用 symlink 或 copy）
    ///
    /// 根据配置和平台选择最佳同步方式：
    /// - Auto: 优先尝试 symlink，失败时回退到 copy
    /// - Symlink: 仅使用 symlink
    /// - Copy: 仅使用文件复制
    pub fn sync_to_app_dir(directory: &str, app: &AppType) -> Result<()> {
        if matches!(app, AppType::ClaudeDesktop) {
            return Ok(());
        }

        // directory 可能来自被污染的 DB 行（如同步导入的远端快照），join 前必须校验。
        let directory = Self::require_valid_directory(directory)?;

        let ssot_dir = Self::get_ssot_dir()?;
        let source = ssot_dir.join(&directory);

        Self::validate_sync_source_dir(&source, &directory)?;

        let app_dir = Self::get_distinct_app_skills_dir(&ssot_dir, app)?;
        fs::create_dir_all(&app_dir)?;

        let dest = app_dir.join(&directory);

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
        ) && (dest.exists() || Self::is_symlink(&dest))
        {
            Self::ensure_pi_skill_destination_matches(&source, &dest, &directory)?;
        }

        let sync_method = Self::get_sync_method();
        // WSL targets reject directory symlinks outright (ERROR_ACCESS_DENIED),
        // so Auto skips the doomed symlink attempt and goes straight to copy.
        let sync_method =
            if matches!(sync_method, SyncMethod::Auto) && crate::config::is_wsl_path(&dest) {
                SyncMethod::Copy
            } else {
                sync_method
            };

        match sync_method {
            SyncMethod::Auto => {
                if dest.exists() && !Self::is_symlink(&dest) {
                    Self::replace_dest_with_copy(&source, &dest, &directory)?;
                    log::debug!("Skill {directory} 已通过复制同步到 {app:?}");
                    return Ok(());
                }

                if Self::is_symlink(&dest) {
                    Self::remove_path(&dest)?;
                }

                // 优先尝试 symlink
                match Self::create_symlink(&source, &dest) {
                    Ok(()) => {
                        log::debug!("Skill {directory} 已通过 symlink 同步到 {app:?}");
                        return Ok(());
                    }
                    Err(err) => {
                        log::warn!(
                            "Symlink 创建失败，将回退到文件复制: {} -> {}. 错误: {err:#}",
                            source.display(),
                            dest.display()
                        );
                    }
                }
                // Fallback 到 copy
                Self::replace_dest_with_copy(&source, &dest, &directory)?;
                log::debug!("Skill {directory} 已通过复制同步到 {app:?}");
            }
            SyncMethod::Symlink => {
                if dest.exists() || Self::is_symlink(&dest) {
                    Self::remove_path(&dest)?;
                }
                Self::create_symlink(&source, &dest)?;
                log::debug!("Skill {directory} 已通过 symlink 同步到 {app:?}");
            }
            SyncMethod::Copy => {
                Self::replace_dest_with_copy(&source, &dest, &directory)?;
                log::debug!("Skill {directory} 已通过复制同步到 {app:?}");
            }
        }

        Ok(())
    }

    /// 删除路径（支持 symlink 和真实目录）
    pub(super) fn remove_path(path: &Path) -> Result<()> {
        if Self::is_symlink(path) {
            // 符号链接：仅删除链接本身，不影响源文件
            #[cfg(unix)]
            fs::remove_file(path)?;
            #[cfg(windows)]
            fs::remove_dir(path)?; // Windows 的目录 symlink 需要用 remove_dir
        } else if path.is_dir() {
            // 真实目录：递归删除
            fs::remove_dir_all(path)?;
        } else if path.exists() {
            // 普通文件
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub(super) fn validate_sync_source_dir(source: &Path, directory: &str) -> Result<()> {
        if !source.is_dir() {
            return Err(anyhow!("Skill 不存在于 SSOT: {directory}"));
        }

        let manifest = source.join("SKILL.md");
        if !manifest.is_file() {
            return Err(anyhow!(
                "Skill 源目录缺少 SKILL.md，拒绝同步以避免覆盖目标目录: {}",
                source.display()
            ));
        }

        Ok(())
    }

    pub(super) fn replace_dest_with_copy(
        source: &Path,
        dest: &Path,
        directory: &str,
    ) -> Result<()> {
        Self::validate_sync_source_dir(source, directory)?;

        let parent = dest
            .parent()
            .ok_or_else(|| anyhow!("Invalid skill destination: {}", dest.display()))?;
        fs::create_dir_all(parent)?;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_name = Self::sanitize_backup_segment(directory);
        let tmp = parent.join(format!(".{tmp_name}.tmp-{}-{nonce}", std::process::id()));

        if tmp.exists() || Self::is_symlink(&tmp) {
            Self::remove_path(&tmp)?;
        }

        let copy_result = Self::copy_dir_recursive(source, &tmp);
        if let Err(err) = copy_result {
            let _ = Self::remove_path(&tmp);
            return Err(err);
        }

        if dest.exists() || Self::is_symlink(dest) {
            Self::remove_path(dest)?;
        }

        fs::rename(&tmp, dest).with_context(|| {
            let _ = Self::remove_path(&tmp);
            format!(
                "替换 Skill 目录失败: {} -> {}",
                tmp.display(),
                dest.display()
            )
        })?;

        Ok(())
    }

    /// 判断路径是否为指向 SSOT 目录内的符号链接。
    pub(super) fn is_symlink_to_ssot(path: &Path, ssot_dir: &Path) -> bool {
        if !Self::is_symlink(path) {
            return false;
        }

        let Ok(target) = fs::read_link(path) else {
            return false;
        };

        if target.is_absolute() && target.starts_with(ssot_dir) {
            return true;
        }

        let resolved = path
            .parent()
            .map(|parent| parent.join(&target))
            .unwrap_or(target.clone());

        let canonical_ssot = ssot_dir
            .canonicalize()
            .unwrap_or_else(|_| ssot_dir.to_path_buf());
        let canonical_target = resolved.canonicalize().unwrap_or(resolved);

        canonical_target.starts_with(&canonical_ssot)
    }

    /// 从应用目录删除 Skill（支持 symlink 和真实目录）
    pub fn remove_from_app(directory: &str, app: &AppType) -> Result<()> {
        Self::remove_from_app_preserving(directory, app, &[])
    }

    pub(super) fn remove_from_app_preserving(
        directory: &str,
        app: &AppType,
        preserved_paths: &[PathBuf],
    ) -> Result<()> {
        if matches!(app, AppType::ClaudeDesktop) {
            return Ok(());
        }

        // directory 可能来自被污染的 DB 行（如同步导入的远端快照），
        // 这里执行的是删除操作，join 前必须校验，防止任意目录删除。
        let directory = Self::require_valid_directory(directory)?;

        let ssot_dir = Self::get_ssot_dir()?;
        let app_dir = Self::get_distinct_app_skills_dir(&ssot_dir, app)?;
        let skill_path = app_dir.join(&directory);

        if preserved_paths
            .iter()
            .any(|path| Self::paths_overlap(&skill_path, path))
        {
            log::debug!("Skill {directory} 的 {app:?} 路径与保留的原生副本相同，跳过删除");
            return Ok(());
        }

        if skill_path.exists() || Self::is_symlink(&skill_path) {
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
                let source = ssot_dir.join(&directory);
                Self::ensure_pi_skill_destination_matches(&source, &skill_path, &directory)?;
            }
            Self::remove_path(&skill_path)?;
            log::debug!("Skill {directory} 已从 {app:?} 删除");
        }

        Ok(())
    }

    /// 同步所有已启用的 Skills 到指定应用
    pub fn sync_to_app(db: &Arc<Database>, app: &AppType) -> Result<()> {
        let _state_guard = skill_state_read_guard();
        Self::sync_to_app_unlocked(db, app).map(|_| ())
    }

    /// Skills 不由 `sync_to_app` 投影的应用：Claude Desktop、OpenClaw 不支持 Skills，
    /// Pi 没有数据库列、按目录是否存在现算。这些应用的 skills 目录不归 CC Switch 管，
    /// 同步时一个字节都不能碰——OpenClaw 自己的 `~/.openclaw/skills` 里和受管
    /// Skill 同名的真实目录，否则会被当成「已关掉的投影」删掉。
    pub(super) fn is_sync_managed_app(app: &AppType) -> bool {
        !matches!(
            app,
            AppType::ClaudeDesktop | AppType::OpenClaw | AppType::Pi | AppType::Omp
        )
    }

    /// 「立即重新同步」：按数据库里的开关和当前同步方式，把 Skill 重新投影到各应用目录，
    /// 逐应用报告结果（Claude Desktop、OpenClaw、Pi、Omp 不由这里同步，不在结果里）。
    pub fn resync_all_apps(db: &Arc<Database>) -> Vec<SkillAppSyncOutcome> {
        let _state_guard = skill_state_read_guard();
        AppType::all()
            .filter(Self::is_sync_managed_app)
            .map(|app| {
                let (error, failed_skills) = match Self::sync_to_app_unlocked(db, &app) {
                    Ok(failed) => (None, failed),
                    Err(err) => {
                        log::warn!("重新同步 Skill 到 {app:?} 失败: {err:#}");
                        (Some(format!("{err:#}")), Vec::new())
                    }
                };
                SkillAppSyncOutcome {
                    app: app.as_str().to_string(),
                    ok: error.is_none() && failed_skills.is_empty(),
                    error,
                    failed_skills,
                }
            })
            .collect()
    }

    /// Caller must hold either the Skills state read or write guard.
    /// 返回同步失败、被跳过的 Skill（整个应用失败时返回 Err）。
    pub(super) fn sync_to_app_unlocked(
        db: &Arc<Database>,
        app: &AppType,
    ) -> Result<Vec<SkillSyncFailure>> {
        let mut failed = Vec::new();
        if !Self::is_sync_managed_app(app) {
            return Ok(failed);
        }

        let skills = db.get_all_installed_skills()?;
        let ssot_dir = Self::get_ssot_dir()?;
        let app_dir = Self::get_distinct_app_skills_dir(&ssot_dir, app)?;

        let indexed_skills: HashMap<String, &InstalledSkill> = skills
            .values()
            .map(|skill| (skill.directory.to_lowercase(), skill))
            .collect();

        // Unselected MCode, Agy, Kimi, Copilot, WorkBuddy and CodeBuddy directories may have been installed outside CC Switch.
        // Explicit disable/uninstall handles removal of managed deployments.
        if app_dir.exists()
            && !matches!(
                app,
                AppType::Mcode
                    | AppType::Agy
                    | AppType::Kimi
                    | AppType::Dsh
                    | AppType::Copilot
                    | AppType::WorkBuddy
                    | AppType::Codebuddy
            )
        {
            for entry in fs::read_dir(&app_dir)? {
                let entry = entry?;
                let path = entry.path();
                let dir_name = entry.file_name().to_string_lossy().to_string();

                if dir_name.starts_with('.') {
                    continue;
                }

                if let Some(skill) = indexed_skills.get(&dir_name.to_lowercase()) {
                    if !skill.apps.is_enabled_for(app) {
                        Self::remove_path(&path)?;
                    }
                    continue;
                }

                if Self::is_symlink_to_ssot(&path, &ssot_dir) {
                    Self::remove_path(&path)?;
                }
            }
        }

        for skill in skills.values() {
            if skill.apps.is_enabled_for(app) {
                // 逐条容错而非 `?` 传播：本函数在切换供应商时被调用，一条脏
                // directory（存量点开头目录、或同步导入灌进来的行）不得让整个
                // 应用的 skill 同步全部失效。
                if let Err(err) = Self::sync_to_app_dir(&skill.directory, app) {
                    log::warn!(
                        "同步 skill {} 到 {app:?} 失败，跳过该条: {err}",
                        skill.directory
                    );
                    failed.push(SkillSyncFailure {
                        directory: skill.directory.clone(),
                        error: format!("{err:#}"),
                    });
                }
            }
        }

        Ok(failed)
    }
}

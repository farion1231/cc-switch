//! 下载仓库归档并在预算内解压；在解压出的仓库里定位技能目录。

use super::*;

impl SkillService {
    /// 在目录树中查找名称匹配且包含 SKILL.md 的子目录
    ///
    /// 用于 skills.sh 安装回退：API 只返回 skillId（如 "find-skills"），
    /// 但实际文件可能在仓库子目录中（如 "skills/find-skills"）。
    ///
    /// 先只走非隐藏目录（保持既有选择顺序）；找不到再放开点开头目录，
    /// 因为不少仓库只把技能放在 `.claude/skills/<name>`、`.agents/skills/<name>`
    /// 这类客户端目录里（#10040）。`.git` 始终跳过。
    pub(super) fn find_skill_dir_by_name(root: &Path, target_name: &str) -> Option<PathBuf> {
        fn walk(dir: &Path, target: &str, depth: usize, include_hidden: bool) -> Option<PathBuf> {
            if depth > 3 {
                return None;
            }
            let entries = fs::read_dir(dir).ok()?;
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') && (!include_hidden || name_str == ".git") {
                    continue;
                }
                if name_str.eq_ignore_ascii_case(target) && path.join("SKILL.md").exists() {
                    return Some(path);
                }
                if let Some(found) = walk(&path, target, depth + 1, include_hidden) {
                    return Some(found);
                }
            }
            None
        }
        walk(root, target_name, 0, false).or_else(|| walk(root, target_name, 0, true))
    }

    /// 在仓库扫描结果中定位已安装的技能：已保存的源路径优先，其次目录名，
    /// metadata name 仅作唯一兜底。
    pub(super) fn find_remote_skill_for_install<'a>(
        remote_skills: &'a [DiscoverableSkill],
        install_name: &str,
        stored_readme_url: Option<&str>,
    ) -> Option<&'a DiscoverableSkill> {
        let stored_doc_path = stored_readme_url.and_then(Self::extract_doc_path_from_url);
        stored_doc_path
            .as_deref()
            .and_then(|doc_path| {
                remote_skills.iter().find(|skill| {
                    skill
                        .readme_url
                        .as_deref()
                        .and_then(Self::extract_doc_path_from_url)
                        .as_deref()
                        == Some(doc_path)
                })
            })
            .or_else(|| {
                remote_skills.iter().find(|skill| {
                    skill
                        .directory
                        .rsplit('/')
                        .next()
                        .unwrap_or(&skill.directory)
                        .eq_ignore_ascii_case(install_name)
                })
            })
            .or_else(|| {
                let mut matches = remote_skills
                    .iter()
                    .filter(|skill| skill.name.trim().eq_ignore_ascii_case(install_name));
                let found = matches.next()?;
                matches.next().is_none().then_some(found)
            })
    }

    /// 将 discoverable skill 的目录信息重新解析为解压目录中的真实源目录。
    ///
    /// **核心原则：返回的目录必定含 `SKILL.md`**（以 SKILL.md 为锚点）。解析顺序：
    /// 1. 直接相对路径命中（如 `skills/foo`），校验含 `SKILL.md`——明确路径优先；
    /// 2. 按安装名递归查找名字匹配 **且** 含 `SKILL.md` 的目录；
    /// 3. 按 `SKILL.md` 的 metadata name 查找唯一匹配；
    /// 4. 兜底：仓库根本身含 `SKILL.md`。
    pub(super) fn resolve_skill_source_dir(root: &Path, raw_directory: &str) -> Option<PathBuf> {
        let source_rel = Self::sanitize_skill_source_path(raw_directory)?;
        let install_name = source_rel
            .file_name()
            .map(|n| n.to_string_lossy().to_string())?;

        // 1. 直接相对路径命中（明确路径优先）——必须校验 SKILL.md，否则同名空壳目录
        //    （如 ast-grep/agent-skill 根下的 plugin 包目录 ast-grep/）会被误判为源目录。
        let direct = root.join(&source_rel);
        if direct.is_dir() && direct.join("SKILL.md").is_file() {
            return Some(direct);
        }

        // 2. 按名字递归查找（find_skill_dir_by_name 已校验 SKILL.md）
        if let Some(found) = Self::find_skill_dir_by_name(root, &install_name) {
            log::info!(
                "Skill directory '{}' not found at direct path, using fallback: {}",
                install_name,
                found.display()
            );
            return Some(found);
        }

        // 3. skills.sh 的 skillId 可能与目录名不同；仅接受唯一 metadata 匹配，
        //    避免同名 skill 因文件系统遍历顺序不同而随机安装。
        if let Ok(skill_dirs) = Self::scan_skills_in_dir(root) {
            let mut metadata_matches = skill_dirs.into_iter().filter(|path| {
                Self::parse_skill_metadata_static(&path.join("SKILL.md"))
                    .ok()
                    .and_then(|metadata| metadata.name)
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case(install_name.as_str()))
            });
            if let Some(found) = metadata_matches.next() {
                if metadata_matches.next().is_some() {
                    log::warn!(
                        "Multiple skill directories declare metadata name '{}'; refusing ambiguous install",
                        install_name
                    );
                    return None;
                }
                log::info!(
                    "Skill directory '{}' resolved from SKILL.md metadata: {}",
                    install_name,
                    found.display()
                );
                return Some(found);
            }
        }

        // 4. 兜底：仓库根本身是 skill
        if root.join("SKILL.md").is_file() {
            log::info!(
                "Skill directory '{}' not found, but SKILL.md exists at root, using repo root",
                install_name,
            );
            return Some(root.to_path_buf());
        }

        None
    }

    /// 由真实解析出的源目录推导 SKILL.md 在仓库内的相对文档路径（正斜杠）。
    /// 两个参数都应是已 canonicalize 的路径（安装流程已做包含性校验）。
    pub(super) fn doc_path_for_source(repo_root: &Path, source: &Path) -> Option<String> {
        let rel = source.strip_prefix(repo_root).ok()?;
        let mut parts: Vec<String> = rel
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
                _ => None,
            })
            .collect();
        parts.push("SKILL.md".to_string());
        Some(parts.join("/"))
    }

    /// 选择 readme_url 使用的仓库内文档路径：真实解析出的源目录优先，其次是
    /// 旧 readme_url 中保存的路径，最后才按 directory 拼接。skills.sh 的
    /// `directory` 只是 skillId（末级目录名），嵌套目录场景直接拼接会丢路径、
    /// 文档链接 404（#6111），所以真实源目录必须排第一优先级。
    pub(super) fn choose_doc_path(
        resolved_source_doc_path: Option<String>,
        readme_url: Option<&str>,
        directory: &str,
    ) -> String {
        if let Some(path) = resolved_source_doc_path {
            return path;
        }
        if let Some(path) = readme_url.and_then(Self::extract_doc_path_from_url) {
            if path.ends_with("/SKILL.md") || path == "SKILL.md" {
                return path;
            }
            return format!("{}/SKILL.md", path.trim_end_matches('/'));
        }
        format!("{}/SKILL.md", directory.trim_end_matches('/'))
    }

    /// 去重技能列表（基于完整 key，不同仓库的同名 skill 分开显示）
    pub(super) fn deduplicate_discoverable_skills(skills: &mut Vec<DiscoverableSkill>) {
        let mut seen = HashMap::new();
        skills.retain(|skill| {
            // 使用完整 key（owner/repo:directory）作为唯一标识
            // 这样不同仓库的同名 skill 会分开显示
            let unique_key = skill.key.to_lowercase();
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(unique_key) {
                e.insert(true);
                true
            } else {
                false
            }
        });
    }

    /// 下载仓库
    ///
    /// 这里是仓库坐标进入 URL 的**唯一收敛点**——`fetch_repo_skills`、`install`、
    /// `check_updates`、`update_skill` 四条路径都经过它，而 `skill_repos` / `skills`
    /// 两张表都会被同步导入的远端快照整表覆盖，入库校验管不住它们。所以主防线放这里。
    pub(super) async fn download_repo(
        &self,
        repo: &SkillRepo,
    ) -> Result<(tempfile::TempDir, String)> {
        Self::validate_repo(repo)?;

        // 守卫全程持有，成功后连同目录一起交给调用方（见 `extract_local_zip` 的说明）。
        // 原来这里立刻 keep()，任何一步失败——下载超时、ARCHIVE_TOO_LARGE、解压出错
        // ——都会把半个解压目录永久留在磁盘上，反复触发即可持续填盘。
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path();

        // Exercise the real install/update flow without network access in unit tests.
        #[cfg(test)]
        if let Some(fixture) = &self.repo_fixture {
            Self::copy_dir_recursive(fixture, temp_path)?;
            return Ok((temp_dir, repo.branch.clone()));
        }

        let epoch = self.credential_epoch.load(Ordering::SeqCst);
        let token = self.repo_token(&repo.host).await?;
        let token = token.as_deref();
        let platform = self.resolve_platform(&repo.host, token, epoch).await;
        let branch = Self::try_repo_branches(
            repo,
            platform,
            token.is_some(),
            temp_path,
            |url| async move { self.download_and_extract(&url, temp_path, token).await },
        )
        .await?;
        Ok((temp_dir, branch))
    }

    /// 这台主机的访问令牌。读钥匙串在 macOS 上会起子进程，放到阻塞线程里做。
    pub(super) async fn repo_token(&self, host: &str) -> Result<Option<String>> {
        let store = Arc::clone(&self.repo_tokens);
        let host = host.to_string();
        tokio::task::spawn_blocking(move || store.get(&host)).await?
    }

    /// 设置了访问令牌的主机，给仓库管理面板看（不含令牌本身）。
    pub fn repo_token_list(&self) -> Result<RepoTokenList> {
        self.repo_tokens.list()
    }

    /// 保存一台主机的访问令牌（覆盖旧的），返回规范化后的主机名。
    pub fn set_repo_token(&self, host: &str, token: &str) -> Result<String> {
        let host = self.repo_tokens.set(host, token)?;
        self.forget_platform(&host);
        Ok(host)
    }

    /// 删除一台主机的访问令牌，返回规范化后的主机名。
    pub fn delete_repo_token(&self, host: &str) -> Result<String> {
        let host = self.repo_tokens.remove(host)?;
        self.forget_platform(&host);
        Ok(host)
    }

    /// 令牌变了，平台要按新身份重新探测：没令牌时强制登录的 Gitea 会被认成 GitLab。
    /// 这台主机的发现缓存也作废：删了令牌，私有仓库的技能列表不该再显示 10 分钟。
    pub(super) fn forget_platform(&self, host: &str) {
        {
            let mut cache = self.platform_cache.lock_recover();
            self.credential_epoch.fetch_add(1, Ordering::SeqCst);
            cache.remove(host);
        }
        self.discovery_cache
            .lock_recover()
            .retain(|key, _| key.0 != host);
    }

    #[cfg(test)]
    pub(crate) fn with_repo_tokens(mut self, store: RepoTokenStore) -> Self {
        self.repo_tokens = Arc::new(store);
        self
    }

    /// 依次尝试「配置分支 → main → master」，返回第一个下载成功的分支。
    ///
    /// 只有「分支不存在」（HTTP 404）才换下一个分支：超时、超过体积上限、解压失败、
    /// 网络错误换个分支也不会好，继续试只会让 master 的 404 盖掉真实原因，界面变成
    /// 「仓库或分支不存在」。抽成接收 `attempt` 的函数是为了不联网也能单测这条规则。
    pub(super) async fn try_repo_branches<F, Fut>(
        repo: &SkillRepo,
        platform: RepoPlatform,
        authenticated: bool,
        temp_path: &Path,
        mut attempt: F,
    ) -> Result<String>
    where
        F: FnMut(String) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut branches = Vec::new();
        if !repo.branch.is_empty() && !repo.branch.eq_ignore_ascii_case("HEAD") {
            branches.push(repo.branch.as_str());
        }
        if !branches.contains(&"main") {
            branches.push("main");
        }
        if !branches.contains(&"master") {
            branches.push("master");
        }

        let mut last_error = None;
        for branch in branches {
            let url = Self::build_archive_url(repo, platform, branch, authenticated)?;

            match attempt(url).await {
                Ok(()) => return Ok(branch.to_string()),
                Err(e) if Self::is_branch_not_found(&e) => {
                    // 每个分支各自重算预算，所以失败后必须把上一轮的残留清掉——
                    // 否则 N 个候选分支等于 N 倍的落盘量堆在同一个目录里。
                    let _ = fs::remove_dir_all(temp_path);
                    let _ = fs::create_dir_all(temp_path);
                    last_error = Some(e);
                }
                Err(e) => return Err(Self::with_repo_context(e, repo)),
            }
        }

        Err(last_error
            .map(|e| Self::with_repo_context(e, repo))
            .unwrap_or_else(|| anyhow::anyhow!("所有分支下载失败")))
    }

    /// 归档地址返回 404，即这个分支（或整个仓库）不存在。
    pub(super) fn is_branch_not_found(error: &anyhow::Error) -> bool {
        serde_json::from_str::<serde_json::Value>(&error.to_string())
            .is_ok_and(|v| v["code"] == "DOWNLOAD_FAILED" && v["context"]["status"] == "404")
    }

    /// 给结构化错误补上仓库坐标。
    ///
    /// 下载 / 解压这一层只拿到 URL，报出的错误不带 owner / name；而前端文案
    /// （如 `skills.error.downloadTimeout`）要插值 `{{owner}}/{{name}}`，缺了就会把
    /// 占位符原样显示出来。已有的键不覆盖，非结构化错误原样返回。
    pub(super) fn with_repo_context(error: anyhow::Error, repo: &SkillRepo) -> anyhow::Error {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&error.to_string()) else {
            return error;
        };
        let Some(context) = value
            .get_mut("context")
            .and_then(serde_json::Value::as_object_mut)
        else {
            return error;
        };
        for (key, val) in [("owner", &repo.owner), ("name", &repo.name)] {
            context
                .entry(key)
                .or_insert_with(|| serde_json::Value::String(val.clone()));
        }
        anyhow::anyhow!(value.to_string())
    }

    /// 下载并解压 ZIP。`token` 是这台主机的访问令牌，没设置为 `None`。
    pub(super) async fn download_and_extract(
        &self,
        url: &str,
        dest: &Path,
        token: Option<&str>,
    ) -> Result<()> {
        let client = Self::http_client(token.is_some())?;
        // 共享客户端不带默认 User-Agent；GitHub 归档下载对无 UA 请求更容易被限流成 429（#8813）
        let mut request = client
            .get(url)
            .header(reqwest::header::USER_AGENT, SKILL_DOWNLOAD_USER_AGENT);
        if let Some(token) = token {
            request = Self::with_bearer(request, token)?;
        }
        let response = timeout(DOWNLOAD_STALL_TIMEOUT, request.send())
            .await
            .map_err(|_| Self::download_stalled_error())??;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let github = url::Url::parse(url)
                .is_ok_and(|u| matches!(u.host_str(), Some("github.com" | "api.github.com")));
            let status = status.to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "DOWNLOAD_FAILED",
                &[("status", &status)],
                Some(Self::download_failure_hint(
                    &status,
                    token.is_some(),
                    github
                )),
            )));
        }

        // 逐块读并卡住压缩体大小：`response.bytes()` 会先把攻击者控制的整个归档
        // 收进内存，之后才轮到 ZipArchive 和解压预算——那时候堆已经被吃光了。
        // 不能只信 Content-Length（可以撒谎或缺失），必须按实际收到的字节数算。
        // 归档写进匿名临时文件（关闭即删，失败路径也不留残留），内存只占一个块。
        let mut response = response;
        let mut body = std::io::BufWriter::new(tempfile::tempfile()?);
        let mut received: u64 = 0;
        while let Some(chunk) = timeout(DOWNLOAD_STALL_TIMEOUT, response.chunk())
            .await
            .map_err(|_| Self::download_stalled_error())??
        {
            received = received.saturating_add(chunk.len() as u64);
            if received > MAX_ARCHIVE_DOWNLOAD_BYTES {
                let limit_mb = (MAX_ARCHIVE_DOWNLOAD_BYTES / 1024 / 1024).to_string();
                return Err(anyhow::anyhow!(format_skill_error(
                    "ARCHIVE_DOWNLOAD_TOO_LARGE",
                    &[("limit_mb", &limit_mb)],
                    None,
                )));
            }
            std::io::Write::write_all(&mut body, &chunk)?;
        }

        let file = body.into_inner().map_err(|e| e.into_error())?;
        let archive = Self::open_downloaded_archive(file)?;
        Self::extract_repo_archive(archive, dest)
    }

    /// 打开下载到的归档。不兼容 GitHub / GitLab / Gitea 接口的站点（如 cnb.cool，#9812）
    /// 对任何地址都回 200 的网页，拿到的不是 ZIP：报成结构化错误，让仓库显示「没有读到」
    /// 和原因，而不是 zip 库的原文。
    pub(super) fn open_downloaded_archive<R: std::io::Read + std::io::Seek>(
        reader: R,
    ) -> Result<zip::ZipArchive<R>> {
        zip::ZipArchive::new(reader).map_err(|e| {
            log::warn!("下载到的内容不是 ZIP 归档: {e}");
            anyhow::anyhow!(format_skill_error(
                "ARCHIVE_INVALID",
                &[],
                Some("checkRepoUrl"),
            ))
        })
    }

    /// 下载停滞的结构化错误。owner / name 由分支回落那一层的 `with_repo_context` 补上。
    pub(super) fn download_stalled_error() -> anyhow::Error {
        let seconds = DOWNLOAD_STALL_TIMEOUT.as_secs().to_string();
        anyhow!(format_skill_error(
            "DOWNLOAD_STALLED",
            &[("timeout", &seconds)],
            Some("checkNetwork"),
        ))
    }

    /// 归档下载失败时给前端的提示键。带没带令牌要分开说：私有仓库不带令牌时 GitHub、
    /// GitLab 都回 404，只说「仓库不存在」会把人引到检查地址上；带了令牌还被拒，
    /// 就是令牌本身的问题。强制登录的 Gitea 对匿名请求回 401 / 403。
    pub(super) fn download_failure_hint(
        status: &str,
        authenticated: bool,
        github: bool,
    ) -> &'static str {
        match (status, authenticated) {
            ("401", true) => "http401Token",
            ("403", true) => "http403Token",
            ("404", true) => "http404Token",
            ("401", false) => "httpAuthRequired",
            // GitHub 匿名 403 多半是限流（#8813），别的主机匿名 403 是要求登录
            ("403", false) if github => "http403",
            ("403", false) => "httpAuthRequired",
            ("404", false) => "http404",
            ("429", _) => "http429",
            _ => "checkNetwork",
        }
    }

    /// 按预算把单个归档条目写出，累计超限即中止。
    ///
    /// 逐块累加而非读取归档头里声明的 size：那个值由归档作者填写，压缩炸弹会撒谎。
    pub(super) fn copy_entry_within_budget<R: std::io::Read, W: std::io::Write>(
        reader: &mut R,
        writer: &mut W,
        total_bytes: &mut u64,
    ) -> Result<()> {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                return Ok(());
            }
            Self::charge_archive_budget(total_bytes, read as u64)?;
            writer.write_all(&buffer[..read])?;
        }
    }

    /// 把单个文件条目写到 `dest` 并计入归档预算，不足一个磁盘块的按一块补齐。
    ///
    /// 解压路径（远端归档、本地 ZIP）与 symlink 物化都经这里落盘，保证"一个文件
    /// 至少一块"的计费只有一处定义。
    pub(super) fn write_file_within_budget<R: std::io::Read>(
        reader: &mut R,
        dest: &Path,
        total_bytes: &mut u64,
    ) -> Result<()> {
        let mut writer = fs::File::create(dest)?;
        let before = *total_bytes;
        Self::copy_entry_within_budget(reader, &mut writer, total_bytes)?;
        let written = total_bytes.saturating_sub(before);
        if written < FILE_ENTRY_BUDGET_COST {
            Self::charge_archive_budget(total_bytes, FILE_ENTRY_BUDGET_COST - written)?;
        }
        Ok(())
    }

    /// 读取 symlink 条目声明的目标路径。
    ///
    /// 这条分支曾是唯一一处不经预算的解压：`read_to_string` 直接把整条解压流吞进
    /// 内存，而 zip 2.4.2 的 `make_reader`（read.rs:437-449）只叠了 CRC 校验，
    /// **没有**按声明的 uncompressed_size 截断。于是一个打着 symlink 标志、
    /// deflate 后能膨胀到数 GB 的条目就是一颗内存炸弹，且预算读数全程为 0。
    ///
    /// 超长或非 UTF-8 一律返回 `None` 让调用方跳过：合法的 symlink 目标是一条
    /// 路径，这两种形状都不可能是真实数据。
    pub(super) fn read_symlink_target<R: std::io::Read>(
        reader: &mut R,
        total_bytes: &mut u64,
    ) -> Result<Option<String>> {
        let mut raw = Vec::new();
        // 多读一个字节，用来区分"正好到上限"和"被截断"
        let mut limited = std::io::Read::take(reader, MAX_SYMLINK_TARGET_BYTES + 1);
        std::io::Read::read_to_end(&mut limited, &mut raw)?;
        if raw.len() as u64 > MAX_SYMLINK_TARGET_BYTES {
            return Ok(None);
        }
        Self::charge_archive_budget(total_bytes, raw.len() as u64)?;
        Ok(String::from_utf8(raw)
            .ok()
            .map(|target| target.trim().to_string()))
    }

    /// 建目录并按**实际新建的层数**计费。
    ///
    /// `create_dir_all` 会一次性把缺失的父目录全建出来，所以一个条目名
    /// `a/a/…/a/f.txt` 可以隐式造出几百层目录。只在 symlink 物化那条路径上给目录
    /// 计费是不够的：常规解压这条路上，不到 10_000 个条目照样能造出数百万目录，
    /// 而内容字节几乎为零。
    pub(super) fn create_dir_all_within_budget(path: &Path, total_bytes: &mut u64) -> Result<()> {
        let missing = path.ancestors().take_while(|p| !p.exists()).count() as u64;
        if missing > 0 {
            Self::charge_archive_budget(total_bytes, missing * DIRECTORY_BUDGET_COST)?;
        }
        fs::create_dir_all(path)?;
        Ok(())
    }

    /// 归档预算的唯一扣费点。
    ///
    /// 抽出来是因为「写文件内容」不是归档能消耗的唯一资源：symlink 物化出来的
    /// 目录一个字节都不写，但每一个都要占 inode 与一个目录块，而第二遍的
    /// symlink 解析可以让目录数量按层数指数增长。只按内容字节计费时，一个全是
    /// 空目录的归档能把预算读数一直停在 0。
    pub(super) fn charge_archive_budget(total_bytes: &mut u64, amount: u64) -> Result<()> {
        if total_bytes.saturating_add(amount) > MAX_ARCHIVE_TOTAL_BYTES {
            let limit_mb = (MAX_ARCHIVE_TOTAL_BYTES / 1024 / 1024).to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "ARCHIVE_TOO_LARGE",
                &[("limit_mb", &limit_mb)],
                Some("checkZipContent"),
            )));
        }
        *total_bytes += amount;
        Ok(())
    }

    /// 把 GitHub 仓库归档解压到 `dest`（剥掉归档自带的一层根目录）。
    ///
    /// 与 `download_and_extract` 分离，使 zip-slip 防护可在不联网的情况下被单测覆盖。
    pub(super) fn extract_repo_archive<R: std::io::Read + std::io::Seek>(
        mut archive: zip::ZipArchive<R>,
        dest: &Path,
    ) -> Result<()> {
        let root_name = if !archive.is_empty() {
            let first_file = archive.by_index(0)?;
            let name = first_file.name();
            name.split('/').next().unwrap_or("").to_string()
        } else {
            return Err(anyhow::anyhow!(format_skill_error(
                "EMPTY_ARCHIVE",
                &[],
                Some("checkRepoUrl"),
            )));
        };

        // 归档字节完全由第三方控制（仓库可经 deeplink 添加），所以解压必须限量，
        // 否则一个几 MB 的压缩炸弹就能塞满磁盘。webdav_sync/archive.rs 早有同款
        // 双重上限，这条下载路径一直没有。
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            let count = archive.len().to_string();
            let limit = MAX_ARCHIVE_ENTRIES.to_string();
            return Err(anyhow::anyhow!(format_skill_error(
                "ARCHIVE_TOO_MANY_ENTRIES",
                &[("count", &count), ("limit", &limit)],
                Some("checkZipContent"),
            )));
        }
        let mut total_bytes: u64 = 0;

        // 第一遍：解压普通文件和目录，收集 symlink 条目
        let mut symlinks: Vec<(PathBuf, String)> = Vec::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            // 第一道：enclosed_name() 拒绝绝对路径、盘符前缀，以及净深度为负
            // （即逃出归档自身根目录）的条目。skill 仓库可由 deeplink 添加，
            // 压缩包内容属第三方可控输入。
            let Some(safe_path) = file.enclosed_name() else {
                log::warn!("跳过不安全的压缩包条目: {}", file.name());
                continue;
            };

            // GitHub 归档统一带一层 `<repo>-<branch>/` 根目录，需剥掉后再落盘。
            let Ok(relative_path) = safe_path.strip_prefix(&root_name) else {
                continue;
            };

            // 第二道：enclosed_name() 的保证是相对**归档根**的，且它不规范化路径
            // ——`..` 会原样留在返回值里。上面剥掉 root_name 等于花掉一级深度预算，
            // 于是 `repo-main/../evil` 这类条目仍能落到 dest 之外（Unix 逃一层；
            // Windows 上 root_name 可含反斜杠而被当作多段，逃逸深度随之放大）。
            // 因此 join 之前必须对**实际使用的相对路径**再验一次。
            if relative_path
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            {
                log::warn!("跳过越界的压缩包条目: {}", file.name());
                continue;
            }

            if relative_path.as_os_str().is_empty() {
                continue;
            }

            let outpath = dest.join(relative_path);

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
                // 按实际写入的字节累计，而不是信任归档头里声明的 size——
                // 压缩炸弹的声明值可以是假的。
                Self::write_file_within_budget(&mut file, &outpath, &mut total_bytes)?;
            }
        }

        // 第二遍：解析 symlink，将目标内容复制到 symlink 位置
        Self::resolve_symlinks_in_dir(dest, &symlinks, &mut total_bytes)?;

        Ok(())
    }

    /// 与 `copy_dir_recursive` 同语义，但把写出的字节计入归档总预算。
    /// 仅用于解压期间物化 symlink——常规的目录复制（安装、备份、迁移）不该受
    /// 归档预算约束，所以两个函数刻意不合并。
    pub(super) fn copy_dir_within_budget(
        src: &Path,
        dest: &Path,
        total_bytes: &mut u64,
    ) -> Result<()> {
        Self::create_dir_all_within_budget(dest, total_bytes)?;

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if path.is_dir() {
                Self::copy_dir_within_budget(&path, &dest_path, total_bytes)?;
            } else {
                Self::copy_file_within_budget(&path, &dest_path, total_bytes)?;
            }
        }

        Ok(())
    }

    /// 复制单个文件并计入归档总预算，复用 `write_file_within_budget` 以保证
    /// 上限、最小计费与报错文案只有一处定义。
    pub(super) fn copy_file_within_budget(
        src: &Path,
        dest: &Path,
        total_bytes: &mut u64,
    ) -> Result<()> {
        let mut reader = fs::File::open(src)?;
        Self::write_file_within_budget(&mut reader, dest, total_bytes)
    }

    /// 递归复制目录
    pub(super) fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
        fs::create_dir_all(dest)?;

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if path.is_dir() {
                Self::copy_dir_recursive(&path, &dest_path)?;
            } else {
                fs::copy(&path, &dest_path)?;
            }
        }

        Ok(())
    }
}

//! 仓库坐标：路径与名称校验、托管平台识别、归档地址。

use super::*;

impl SkillService {
    /// 校验并规范化技能源路径（允许多级目录），拒绝路径穿越和绝对路径
    pub(super) fn sanitize_skill_source_path(raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let mut normalized = PathBuf::new();
        let mut has_component = false;

        for component in Path::new(trimmed).components() {
            match component {
                Component::Normal(name) => {
                    let segment = name.to_string_lossy().trim().to_string();
                    if segment.is_empty() || segment == "." || segment == ".." {
                        return None;
                    }
                    normalized.push(segment);
                    has_component = true;
                }
                Component::CurDir
                | Component::ParentDir
                | Component::RootDir
                | Component::Prefix(_) => {
                    return None;
                }
            }
        }

        has_component.then_some(normalized)
    }

    /// 校验并规范化安装目录名（最终落盘目录名，仅单段）
    pub(super) fn sanitize_install_name(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 显式拒绝两种分隔符，不能依赖 components() 的平台语义：
        // `\` 在 Linux/macOS 上不是分隔符，会被当成合法单段名放行，
        // 但同一个值同步/还原到 Windows 上就变成了嵌套路径。
        if trimmed.contains('/') || trimmed.contains('\\') {
            return None;
        }

        let path = Path::new(trimmed);
        let mut components = path.components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => {
                let normalized = name.to_string_lossy().trim().to_string();
                if normalized.is_empty()
                    || normalized == "."
                    || normalized == ".."
                    || normalized.starts_with('.')
                {
                    None
                } else {
                    Some(normalized)
                }
            }
            _ => None,
        }
    }

    /// 校验来自 DB 行 / 备份 meta.json 等外部来源的 directory 字段。
    ///
    /// 存储值按构造本应是单段安装名（见 sanitize_install_name），但有两个入口
    /// 会绕过安装期校验：同步导入的远端快照直接灌库（raw SQL），以及手工放置 /
    /// 不可信备份里的 meta.json。任何把它 join 进文件系统路径的使用点（尤其是
    /// remove_dir_all 这类删除操作）必须先过这道校验，拒绝路径穿越。
    ///
    /// 只校验、不归一化：`sanitize_install_name` 会 `trim()`，若拿它的返回值替换
    /// 原值，磁盘上真实带空格的目录名就再也 join 不中。所以这里要求归一化结果与
    /// 原值逐字相同，否则一律视为非法。
    pub(super) fn require_valid_directory(directory: &str) -> Result<String> {
        match Self::sanitize_install_name(directory) {
            Some(normalized) if normalized == directory => Ok(normalized),
            _ => Err(anyhow!(
                "Invalid skill directory (possible path traversal): {directory:?}"
            )),
        }
    }

    /// GitHub 账号名（user / org login）。
    ///
    /// 只放行 ASCII 字母数字与 `-`。这比 GitHub 自身的规则更严，但该字段会被拼进
    /// 下载 URL，任何 `/`、`.`、`%`、`\` 都可能改写请求落点（见 validate_repo_ref）。
    pub(super) fn is_valid_github_owner(owner: &str) -> bool {
        !owner.is_empty()
            && owner.len() <= 39
            && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }

    /// GitHub 仓库名。允许 `.` `-` `_`，但整体不能是 `.` 或 `..`。
    pub(super) fn is_valid_github_repo_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 100
            && name != "."
            && name != ".."
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    }

    /// git 分支名。
    ///
    /// 分支名合法含 `/`（`feature/x`），所以不能整体禁掉分隔符——按段做白名单。
    /// 逐段 `!starts_with('.')` 比整体 `contains("..")` 更稳：它同时挡掉 `a/./b`、
    /// `a/.../b` 这类变形。除 `git check-ref-format` 的规则外还额外禁掉 `#` 与 `%`：
    /// 前者会把 URL 后半截变成 fragment，后者可用百分号编码绕过字符检查。
    pub(super) fn is_valid_git_branch(branch: &str) -> bool {
        // 空串和 "HEAD" 都是 `download_repo` 的哨兵，语义都是「用仓库默认分支」：
        // 分支候选表对两者一视同仁地跳过，改试 main / master，所以它们**永远不会
        // 被拼进 URL**，也就没有可校验的攻击面。空串必须放行——`skill_repos` 的
        // 存量行可以是空 branch（建表默认值是 'main'，但不禁止空串），前端两处
        // `repo.branch || "main"` 就是照着这个前提写的。把它当非法会让那些仓库
        // 在 download_repo 第一行就报 INVALID_REPO_REF，技能面板直接列不出来。
        if branch.is_empty() || branch.eq_ignore_ascii_case("HEAD") {
            return true;
        }
        if branch.len() > 255 {
            return false;
        }
        if branch.starts_with('/') || branch.ends_with('/') || branch.contains("//") {
            return false;
        }
        if branch.contains("@{") {
            return false;
        }
        // `is_ascii_control()` 的范围是 U+0000..=U+001F **加上** U+007F DELETE，
        // 所以不需要另外再点名 DEL。
        if branch
            .chars()
            .any(|c| c.is_ascii_control() || " ~^:?*[\\#%".contains(c))
        {
            return false;
        }
        branch.split('/').all(|segment| {
            !segment.is_empty()
                && !segment.starts_with('.')
                && !segment.ends_with('.')
                && !segment.ends_with(".lock")
        })
    }

    /// 校验一组仓库坐标，用于任何会被拼进 github.com URL 的地方。
    ///
    /// 动机：`download_repo` 把 owner/name/branch 直接 format 进
    /// `https://github.com/{owner}/{name}/archive/refs/heads/{branch}.zip`，而 URL
    /// 解析会消解点段——branch 写成 `../../../releases/download/v1/evil` 时，落点变成
    /// 该仓库的 **release asset**，即攻击者可上传的任意字节。归档内容一旦可控，
    /// 解压路径校验就成了唯一防线，所以这一层必须堵死。
    pub(crate) fn validate_repo_ref(owner: &str, name: &str, branch: &str) -> Result<()> {
        if !Self::is_valid_github_owner(owner) || !Self::is_valid_github_repo_name(name) {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name)],
                Some("checkRepoUrl"),
            )));
        }
        if !Self::is_valid_git_branch(branch) {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name), ("branch", branch)],
                Some("checkRepoUrl"),
            )));
        }
        Ok(())
    }

    /// GitLab 实例的主机名，可带端口：`gitlab.com`、`git.example.com:8443`、`10.0.0.5`。
    ///
    /// 白名单而不是交给 URL 解析去猜：这个值会被拼到 `https://` 后面，任何 `/`、`@`、
    /// `?`、`#`、`\` 都能改写请求落点。只收小写（入库前已规范化），IPv6 字面量不支持。
    pub(crate) fn is_valid_gitlab_host(host: &str) -> bool {
        let (name, port) = match host.rsplit_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        };
        if let Some(port) = port {
            if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
                return false;
            }
        }
        !name.is_empty()
            && name.len() <= 253
            && name != GITHUB_HOST
            && name.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
    }

    /// GitLab 的分组路径：`group` 或 `group/subgroup/...`。
    ///
    /// 每一段的字符集与 GitLab 自己的规则一致（字母数字、`_`、`-`、`.`），另外不许
    /// 以 `-` 开头、不许是 `.` / `..`。`/` 只作分段符，拼 URL 时整体百分号编码。
    pub(super) fn is_valid_gitlab_namespace(namespace: &str) -> bool {
        namespace.len() <= 512
            && namespace.split('/').count() <= 20
            && namespace.split('/').all(|segment| {
                !segment.is_empty()
                    && segment.len() <= 255
                    && segment != "."
                    && segment != ".."
                    && !segment.starts_with('-')
                    && segment
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            })
    }

    /// 按主机分派的仓库坐标校验：`github.com` 用 GitHub 的规则，其余按 GitLab
    /// （分组可以嵌套；Gitea 的 owner 只有一段，是它的子集，拼地址时再收紧）。
    ///
    /// 入库（`add_skill_repo`）和出库（`download_repo`、文档链接）都走这一个函数，
    /// 两头的规则不会走偏。
    pub(crate) fn validate_repo_coords(
        host: &str,
        owner: &str,
        name: &str,
        branch: &str,
    ) -> Result<()> {
        if host == GITHUB_HOST {
            return Self::validate_repo_ref(owner, name, branch);
        }
        if !Self::is_valid_gitlab_host(host)
            || !Self::is_valid_gitlab_namespace(owner)
            || !Self::is_valid_github_repo_name(name)
        {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name)],
                Some("checkRepoUrl"),
            )));
        }
        if !Self::is_valid_git_branch(branch) {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name), ("branch", branch)],
                Some("checkRepoUrl"),
            )));
        }
        Ok(())
    }

    pub(crate) fn validate_repo(repo: &SkillRepo) -> Result<()> {
        Self::validate_repo_coords(&repo.host, &repo.owner, &repo.name, &repo.branch)
    }

    /// 主机没探明时按什么平台处理：`github.com` 是 GitHub，其余按 GitLab。
    pub(super) fn default_platform(host: &str) -> RepoPlatform {
        if host == GITHUB_HOST {
            RepoPlatform::GitHub
        } else {
            RepoPlatform::GitLab
        }
    }

    /// 只查内存里的探测结果，不联网。
    ///
    /// 给拼文档链接用，而且只用在刚下载完的地方（发现、更新）：那里平台已经探明。
    /// 没探过的主机按 GitLab，与加入 Gitea 之前的行为一致。安装可能跳过下载，
    /// 走 `platform_for_doc_url`。
    pub(super) fn known_platform(&self, host: &str) -> RepoPlatform {
        self.platform_cache
            .lock_recover()
            .get(host)
            .copied()
            .unwrap_or_else(|| Self::default_platform(host))
    }

    /// 给一个待安装的技能重拼文档链接时用哪种平台的格式。
    ///
    /// 探测过的主机以探测结果为准；没探测过时看发现阶段给出的链接是不是 Gitea 的
    /// 形状。链接来自前端，但这里只用它在两种格式之间二选一，最终的地址仍由校验过的
    /// 坐标重拼，不会把它原样存下来。
    pub(super) fn platform_for_doc_url(&self, skill: &DiscoverableSkill) -> RepoPlatform {
        let host = &skill.repo_host;
        if let Some(platform) = self.platform_cache.lock_recover().get(host).copied() {
            return platform;
        }
        let gitea_prefix = format!(
            "https://{host}/{}/{}/src/branch/",
            skill.repo_owner, skill.repo_name
        );
        let looks_like_gitea = host != GITHUB_HOST
            && skill
                .readme_url
                .as_deref()
                .is_some_and(|url| url.starts_with(&gitea_prefix));
        if looks_like_gitea {
            RepoPlatform::Gitea
        } else {
            Self::default_platform(host)
        }
    }

    /// 判断主机跑的是 GitLab 还是 Gitea，每个主机每次启动只探测一次。
    ///
    /// 探测对已有的 GitLab 用户必须无害：请求失败、超时、限流都按 GitLab 继续，
    /// 而且不记结果，下次下载重新探测。调用方须已校验过 `host`。
    ///
    /// `token` 是这台主机的访问令牌：强制登录的 Gitea 对匿名的版本查询回 403，不带令牌
    /// 会被认成 GitLab、拼错归档地址。令牌增删时清掉这台主机的缓存（`forget_platform`）。
    /// `epoch` 是读 `token` 之前记下的 `credential_epoch`：令牌在这之后变过，探测结果不入缓存。
    pub(super) async fn resolve_platform(
        &self,
        host: &str,
        token: Option<&str>,
        epoch: u64,
    ) -> RepoPlatform {
        if host == GITHUB_HOST || !Self::is_valid_gitlab_host(host) {
            return Self::default_platform(host);
        }
        if let Some(platform) = self.platform_cache.lock_recover().get(host).copied() {
            return platform;
        }
        match Self::probe_platform(host, token).await {
            Some(platform) => {
                self.remember_platform(host, platform, epoch);
                platform
            }
            None => RepoPlatform::GitLab,
        }
    }

    /// 记下探测结果，除非令牌在 `epoch` 之后变过。与 `forget_platform` 同在缓存锁里
    /// 读写代次，比对和写入之间令牌改不了。
    pub(super) fn remember_platform(&self, host: &str, platform: RepoPlatform, epoch: u64) {
        let mut cache = self.platform_cache.lock_recover();
        if self.credential_epoch.load(Ordering::SeqCst) == epoch {
            cache.insert(host.to_string(), platform);
        }
    }

    /// 向主机要 `/api/v1/version`：Gitea / Forgejo 不登录就返回 `{"version": "…"}`，
    /// GitLab 没有这个地址（404，或跳去登录页）。`None` 表示这次没探明。
    pub(super) async fn probe_platform(host: &str, token: Option<&str>) -> Option<RepoPlatform> {
        let url = format!("https://{host}/api/v1/version");
        let mut request = Self::http_client(token.is_some())
            .inspect_err(|e| log::warn!("探测 {host} 的平台失败，按 GitLab 处理: {e}"))
            .ok()?
            .get(&url)
            .header(reqwest::header::USER_AGENT, SKILL_DOWNLOAD_USER_AGENT)
            .timeout(PLATFORM_PROBE_TIMEOUT);
        if let Some(token) = token {
            request = Self::with_bearer(request, token).ok()?;
        }
        let mut response = request
            .send()
            .await
            .inspect_err(|e| log::warn!("探测 {host} 的平台失败，按 GitLab 处理: {e}"))
            .ok()?;
        let status = response.status().as_u16();
        // 响应体来自用户填的任意主机：只读开头一小段，够放下版本号就行
        let mut body = Vec::new();
        while body.len() < PLATFORM_PROBE_MAX_BODY_BYTES {
            match response.chunk().await {
                Ok(Some(chunk)) => body.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(e) => {
                    log::warn!("探测 {host} 的平台失败，按 GitLab 处理: {e}");
                    return None;
                }
            }
        }
        Self::classify_platform_probe(status, &body, token.is_some())
    }

    /// 把探测响应归类。抽成纯函数是为了不联网也能单测这张判定表。
    ///
    /// 只有 200 且是带 `version` 字符串的 JSON 才算 Gitea：自建 GitLab 前面常有
    /// 统一登录或网关，对任何地址都回 200 的网页，不能只看状态码。
    /// 429 和 5xx 说明这次没问到，返回 `None`，不下结论。
    ///
    /// 带着令牌问到 401 也不下结论：Gitea / Forgejo 对无效令牌连版本查询都回 401
    /// （11-19 对 codeberg.org 实测），这只说明令牌被拒，按 GitLab 记下来会让令牌填错的
    /// Gitea 用户一直拼错归档地址。匿名的 401 仍按 GitLab 处理。
    pub(super) fn classify_platform_probe(
        status: u16,
        body: &[u8],
        authenticated: bool,
    ) -> Option<RepoPlatform> {
        if status == 429 || status >= 500 || (authenticated && status == 401) {
            return None;
        }
        let is_gitea = status == 200
            && body.len() <= PLATFORM_PROBE_MAX_BODY_BYTES
            && serde_json::from_slice::<serde_json::Value>(body)
                .is_ok_and(|value| value.get("version").is_some_and(|v| v.is_string()));
        Some(if is_gitea {
            RepoPlatform::Gitea
        } else {
            RepoPlatform::GitLab
        })
    }

    /// 某个分支的归档下载地址。
    ///
    /// GitLab 走 API 的归档接口而不是网页上的下载链接：分支放在查询参数里，带 `/` 的
    /// 分支名不会和路径混在一起；而且分支不存在、项目不存在都返回 404（网页链接遇到
    /// 不存在或私有的项目会跳去登录页），正好对上 `try_repo_branches` 的换分支规则。
    /// Gitea 的 API 归档接口同样两种情况都是 404。
    ///
    /// `authenticated`：这次要带访问令牌。GitHub 的网页下载链接不认令牌，改走 API 的
    /// zipball 接口（它 302 到 codeload 的一次性地址，跨主机跳转时 reqwest 会去掉令牌头）；
    /// GitLab、Gitea 本来就走 API，地址不变。
    pub(super) fn build_archive_url(
        repo: &SkillRepo,
        platform: RepoPlatform,
        branch: &str,
        authenticated: bool,
    ) -> Result<String> {
        if repo.is_github() && authenticated {
            // 写成 refs/heads/<分支>：zipball 接受任意 ref，只写分支名时同名 tag 也能命中，
            // 分支删了却还有同名 tag 就会装上 tag 的内容，也不再 404 换分支（11-19 实测）
            let url = format!(
                "https://api.github.com/repos/{}/{}/zipball/refs/heads/{}",
                repo.owner, repo.name, branch
            );
            Self::assert_github_api_archive_url(&url, &repo.owner, &repo.name)?;
            return Ok(url);
        }
        if repo.is_github() {
            let url = format!(
                "https://github.com/{}/{}/archive/refs/heads/{}.zip",
                repo.owner, repo.name, branch
            );
            Self::assert_github_archive_url(&url, &repo.owner, &repo.name)?;
            return Ok(url);
        }

        let invalid = || {
            anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", &repo.owner), ("name", &repo.name)],
                Some("checkRepoUrl"),
            ))
        };
        let origin = format!("https://{}/", repo.host);
        let mut url = url::Url::parse(&origin).map_err(|_| invalid())?;

        if platform == RepoPlatform::Gitea {
            // Gitea 没有嵌套分组：owner 只有一段。带 `/` 的是 GitLab 式路径，
            // 拼进去只会落到别的接口上。
            if repo.owner.contains('/') {
                return Err(invalid());
            }
            // `/api/v1/repos/{owner}/{repo}/archive/{分支}.zip`，分支里的 `/` 是真的路径分隔
            let archive = format!("{branch}.zip");
            let prefix = ["api", "v1", "repos", &repo.owner, &repo.name, "archive"];
            url.path_segments_mut()
                .map_err(|_| invalid())?
                .extend(prefix)
                .extend(archive.split('/'));
            // 出口断言：前六段原样落在预期位置，后面只有分支那几段，没有多出查询或片段
            let segments: Vec<&str> = url
                .path_segments()
                .map(Iterator::collect)
                .unwrap_or_default();
            if url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.as_str().starts_with(&origin)
                || segments.len() != prefix.len() + archive.split('/').count()
                || segments[..prefix.len()] != prefix
            {
                return Err(invalid());
            }
            return Ok(url.into());
        }

        // 项目路径是 API 路径里的**一段**，`/` 会被编码成 %2F
        let project = format!("{}/{}", repo.owner, repo.name);
        url.path_segments_mut().map_err(|_| invalid())?.extend([
            "api",
            "v4",
            "projects",
            project.as_str(),
            "repository",
            "archive.zip",
        ]);
        url.query_pairs_mut().append_pair("sha", branch);

        // 出口断言，理由同 assert_github_archive_url：拼完再确认落点没被改写。
        let expected_path = format!(
            "/api/v4/projects/{}/repository/archive.zip",
            project.replace('/', "%2F")
        );
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.as_str().split('?').next() != Some(&format!("{origin}{}", &expected_path[1..]))
        {
            return Err(invalid());
        }
        Ok(url.into())
    }

    /// 出口断言：URL 拼好后再确认它确实指向预期的 github.com 路径。
    ///
    /// 这是纵深防御——即便上面的字符集校验将来漏了某种变形（百分号编码、新的
    /// 分隔符语义等），这里也能拦住落点被改写的请求。
    pub(super) fn assert_github_archive_url(url: &str, owner: &str, name: &str) -> Result<()> {
        let parsed = url::Url::parse(url).map_err(|e| anyhow!("Invalid archive URL: {e}"))?;
        let expected_prefix = format!("/{owner}/{name}/archive/refs/heads/");
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("github.com")
            || !parsed.path().starts_with(&expected_prefix)
        {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name)],
                Some("checkRepoUrl"),
            )));
        }
        Ok(())
    }

    /// 带令牌下载 GitHub 仓库时的出口断言：只许落在 api.github.com 这个仓库的 zipball 上。
    /// 请求头里有令牌，落点被改写比匿名下载的代价大得多。
    pub(super) fn assert_github_api_archive_url(url: &str, owner: &str, name: &str) -> Result<()> {
        let parsed = url::Url::parse(url).map_err(|e| anyhow!("Invalid archive URL: {e}"))?;
        let expected_prefix = format!("/repos/{owner}/{name}/zipball/refs/heads/");
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("api.github.com")
            || parsed.port().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.path().starts_with(&expected_prefix)
        {
            return Err(anyhow!(format_skill_error(
                "INVALID_REPO_REF",
                &[("owner", owner), ("name", name)],
                Some("checkRepoUrl"),
            )));
        }
        Ok(())
    }

    /// 给请求加上 `Authorization: Bearer <令牌>`，并标成敏感值（不进调试输出）。
    ///
    /// GitHub、GitLab、Gitea / Forgejo 都认 Bearer。不用 GitLab 惯用的 `PRIVATE-TOKEN`：
    /// reqwest 跨主机跳转时只去掉标准认证头，自定义头会被带到新主机。
    ///
    /// 带令牌的请求一律用 [`Self::http_client`] 选出的只走 https 的客户端发。
    pub(super) fn with_bearer(
        request: reqwest::RequestBuilder,
        token: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| anyhow!("访问令牌含有不能放进请求头的字符"))?;
        value.set_sensitive(true);
        Ok(request.header(reqwest::header::AUTHORIZATION, value))
    }

    /// 下载、探测用的客户端。带令牌时换成只走 https 的：reqwest 只在换主机或端口时
    /// 去掉 `Authorization`，同主机非默认端口从 https 跳到 http 会把令牌明文发出去。
    /// 匿名请求仍用共享客户端，行为不变。
    pub(super) fn http_client(authenticated: bool) -> Result<reqwest::Client> {
        if authenticated {
            crate::proxy::http_client::https_only().map_err(|e| anyhow!(e))
        } else {
            Ok(crate::proxy::http_client::get())
        }
    }
}

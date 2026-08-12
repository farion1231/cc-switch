//! 大仓库 skill 发现/更新路径
//!
//! 背景：某些 skill 仓库较大（如 hugohe3/ppt-master 759MB）整仓 ZIP 下载会超过
//! `MAX_ARCHIVE_DOWNLOAD_BYTES`(128MB) 预算和 60s 超时。本模块提供不下载整仓的
//! 双后端（git CLI / GitHub REST API）实现：
//! - `fetch_tree`：只取文件清单（`git ls-tree` / trees API），不取文件内容
//! - `fetch_file`：按需取单个文件（`git cat-file` / raw API）
//!
//! 安全模型与 skill.rs 的 ZIP 路径对齐：所有进入 URL / 文件系统的路径都要过
//! `validate_repo_ref` / `sanitize_tree_path`，所有下载都有字节预算。
//!
//! 本模块已接入 skill.rs 的 `fetch_repo_skills` / `check_updates` / `install` /
//! `update_skill` 四个集成点（size 门控 + 后端回退链 + ZIP 兜底）。部分入口
//! 函数仍可能因调用路径未覆盖而暂未使用，统一 allow(dead_code) 兜底。
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::task::spawn_blocking;
use tokio::time::timeout;

use crate::services::skill::{
    DiscoverableSkill, SkillRepo, SkillService, MAX_ARCHIVE_DOWNLOAD_BYTES, MAX_ARCHIVE_TOTAL_BYTES,
};

// ========== 常量 ==========

/// 大仓库判定阈值：仓库 size（KB）超过该值走本模块路径（32MB）
pub const LARGE_REPO_THRESHOLD_KB: u64 = 32 * 1024;

/// 本地哈希存储前缀：SHA-1 blob 方案
pub const BLOB_SHA1_PREFIX: &str = "blob-sha1:";
/// 本地哈希存储前缀：SHA-256 blob 方案
pub const BLOB_SHA256_PREFIX: &str = "blob-sha256:";

/// 单次 tree 响应条目数上限（对齐 `MAX_ARCHIVE_ENTRIES` 量级）
pub const TREE_ENTRY_LIMIT: usize = 500_000;
/// tree API 响应体字节上限
pub const TREE_RESPONSE_BYTES_LIMIT: u64 = 64 * 1024 * 1024;
/// clone 后 `.git` 目录磁盘占用上限（对齐 `MAX_ARCHIVE_TOTAL_BYTES`）
pub const CLONE_DISK_LIMIT: u64 = 512 * 1024 * 1024;

/// git 命令超时（秒）
const GIT_TIMEOUT_SECS: u64 = 60;
/// GitHub REST/raw API 请求超时（秒）
const API_TIMEOUT_SECS: u64 = 60;

// ========== 数据结构 ==========

/// 仓库内单个文件（来自 tree 清单）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFile {
    /// 仓库内相对路径，/ 分隔
    pub path: String,
    /// 40 hex (SHA-1) 或 64 hex (SHA-256)
    pub blob_sha: String,
    pub size: u64,
    /// 100644/100755/120000(symlink)/160000(submodule)
    pub mode: u32,
}

/// 远程 SKILL.md 内容（供上层批量读取）
#[derive(Debug, Clone)]
pub struct RemoteSkillMd {
    pub path: String,
    pub content: String,
}

// ========== 后端 trait ==========

/// 大仓库后端原语
///
/// `Send + Sync`：调用方在 async 上下文中持有 `&dyn LargeRepoBackend` 跨 await，
/// 且 `backend_chain()` 返回的 `Box<dyn LargeRepoBackend>` 需要跨线程传递。
#[async_trait::async_trait]
pub trait LargeRepoBackend: Send + Sync {
    /// 获取仓库文件清单（内部解析分支：配置分支 → main → master）
    async fn fetch_tree(&self, repo: &SkillRepo) -> Result<(Vec<RepoFile>, String)>;
    /// 读取单个文件原始字节
    async fn fetch_file(&self, repo: &SkillRepo, branch: &str, file: &RepoFile) -> Result<Vec<u8>>;
}

// ========== GitBackend ==========

/// git CLI 后端：partial clone（`--filter=blob:none`）只拉 tree，文件按需 lazy fetch。
///
/// 每次操作由调用方新建实例（无并发问题），`clone` 字段持有活动 clone 供
/// `fetch_file` 使用。
pub struct GitBackend {
    git: PathBuf,
    /// clone URL 前缀（默认 `https://github.com`；测试注入 `file://` 本地仓库）
    base_url: String,
    clone: Mutex<Option<(String, TempDir)>>,
}

impl GitBackend {
    pub fn new() -> Self {
        Self {
            git: detect_git().unwrap_or_else(|| PathBuf::from("git")),
            base_url: "https://github.com".to_string(),
            clone: Mutex::new(None),
        }
    }

    /// 测试用构造器：注入 git 可执行文件与 clone URL 前缀（`file://` 本地仓库）
    #[cfg(test)]
    pub(crate) fn with_git_and_base(git: PathBuf, base_url: String) -> Self {
        Self {
            git,
            base_url,
            clone: Mutex::new(None),
        }
    }
}

#[async_trait::async_trait]
impl LargeRepoBackend for GitBackend {
    async fn fetch_tree(&self, repo: &SkillRepo) -> Result<(Vec<RepoFile>, String)> {
        log::debug!(
            "[large_repo][GitBackend] fetch_tree 开始: {}/{} branch_config={}",
            repo.owner,
            repo.name,
            repo.branch
        );
        SkillService::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;
        let candidates = branch_candidates(repo);
        // 轻量预检：用一次 ls-remote --heads 拿到远端真实存在的分支。
        // 若候选分支在远端全都不存在（如 "Remote branch main not found"），
        // 直接快速失败，避免对每个候选分支都跑一遍完整 clone。
        match ls_remote_branches(&self.git, &self.base_url, repo).await {
            Ok(remote_branches) => {
                let existing: Vec<String> = candidates
                    .iter()
                    .filter(|b| remote_branches.iter().any(|rb| rb.eq_ignore_ascii_case(b)))
                    .cloned()
                    .collect();
                if existing.is_empty() {
                    let err = anyhow!(
                        "远端不存在候选分支 {} (远端可用分支: {})",
                        candidates.join("/"),
                        remote_branches.join("/")
                    );
                    log::info!(
                        "[large_repo][GitBackend] fetch_tree 快速失败: {}/{}: {err:#}",
                        repo.owner,
                        repo.name
                    );
                    return Err(err);
                }
                log::debug!(
                    "[large_repo][GitBackend] ls-remote 命中可用分支: {:?}（候选 {:?}）",
                    existing,
                    candidates
                );
            }
            Err(e) => {
                // ls-remote 自身失败（如无网络）时不阻断，退化为原逻辑逐个 clone。
                log::debug!(
                    "[large_repo][GitBackend] ls-remote 预检失败，退化为逐个 clone: {e:#}"
                );
            }
        }
        let mut last_error = None;
        for branch in candidates {
            log::debug!("[large_repo][GitBackend] fetch_tree 尝试分支: {branch}");
            let temp_dir = tempfile::tempdir()?;
            match clone_repo(&self.git, &self.base_url, repo, &branch, temp_dir.path()).await {
                Ok(()) => {
                    log::debug!("[large_repo][GitBackend] clone 成功: {branch}");
                    // clone 成功后检查 .git 目录大小，超限丢弃该 clone 报错
                    let git_dir = temp_dir.path().join(".git");
                    if dir_size(&git_dir) > CLONE_DISK_LIMIT {
                        log::info!(
                            "[large_repo][GitBackend] clone 的 .git 目录超过磁盘上限({}MB): {branch}",
                            CLONE_DISK_LIMIT / 1024 / 1024
                        );
                        last_error = Some(anyhow!("clone 的 .git 目录超过磁盘上限"));
                        continue;
                    }
                    match run_git_ls_tree(
                        &self.git,
                        temp_dir.path(),
                        crate::proxy::http_client::get_current_proxy_url().as_deref(),
                    )
                    .await
                    {
                        Ok(output) => {
                            let files = parse_ls_tree_output(&output)?;
                            log::debug!(
                                "[large_repo][GitBackend] ls-tree 成功: {branch}, 文件数={}",
                                files.len()
                            );
                            let repo_key = format!("{}/{}", repo.owner, repo.name);
                            *self.clone.lock().unwrap() = Some((repo_key, temp_dir));
                            log::info!(
                                "[large_repo][GitBackend] fetch_tree 成功: {}/{} 命中分支={branch} 文件数={}",
                                repo.owner,
                                repo.name,
                                files.len()
                            );
                            return Ok((files, branch));
                        }
                        Err(e) => {
                            log::debug!("[large_repo][GitBackend] ls-tree 失败: {branch}: {e:#}");
                            last_error = Some(e);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    log::debug!("[large_repo][GitBackend] clone 失败: {branch}: {e:#}");
                    last_error = Some(e);
                    continue;
                }
            }
        }
        let err = last_error.unwrap_or_else(|| anyhow!("所有候选分支 clone 失败"));
        log::info!(
            "[large_repo][GitBackend] fetch_tree 失败: {}/{}: {err:#}",
            repo.owner,
            repo.name
        );
        Err(err)
    }

    async fn fetch_file(
        &self,
        repo: &SkillRepo,
        _branch: &str,
        file: &RepoFile,
    ) -> Result<Vec<u8>> {
        log::debug!(
            "[large_repo][GitBackend] fetch_file 开始: {}/{} file={} blob_sha={}",
            repo.owner,
            repo.name,
            file.path,
            file.blob_sha
        );
        let expected_key = format!("{}/{}", repo.owner, repo.name);
        let (repo_key, workdir) = {
            let guard = self.clone.lock().unwrap();
            let (key, dir) = guard
                .as_ref()
                .ok_or_else(|| anyhow!("没有活动的 clone，请先调用 fetch_tree"))?;
            (key.clone(), dir.path().to_path_buf())
        };
        if repo_key != expected_key {
            log::info!(
                "[large_repo][GitBackend] fetch_file 失败: 持有的 clone 与请求仓库不匹配 (持有={repo_key} 期望={expected_key})"
            );
            return Err(anyhow!("持有的 clone 与请求仓库不匹配"));
        }
        let git = self.git.clone();
        let sha = file.blob_sha.clone();
        let proxy = crate::proxy::http_client::get_current_proxy_url();
        let bytes = timeout(
            Duration::from_secs(GIT_TIMEOUT_SECS),
            spawn_blocking(move || {
                cat_file_with_budget(&git, &workdir, &sha, proxy.as_deref())
            }),
        )
        .await
        .map_err(|_| anyhow!("git cat-file 超时"))?
        .map_err(|e| anyhow!("git 任务失败: {e}"))??;
        log::debug!(
            "[large_repo][GitBackend] fetch_file 成功: {}/{} file={} 字节数={}",
            repo.owner,
            repo.name,
            file.path,
            bytes.len()
        );
        Ok(bytes)
    }
}

/// 轻量预检：列出远端仓库的 head 分支名（`git ls-remote --heads <url>`）。
///
/// 仅做一次网络往返拿分支列表，用于在 `fetch_tree` 中快速判断候选分支是否存在，
/// 避免对不存在的分支反复触发完整 clone。失败（如无网络/超时）时返回 Err 由调用方
/// 退化为原逻辑，不应阻断主流程。
async fn ls_remote_branches(git: &Path, base_url: &str, repo: &SkillRepo) -> Result<Vec<String>> {
    let url = format!("{base_url}/{}/{}.git", repo.owner, repo.name);
    log::debug!("[large_repo][ls-remote] 开始: url={url}");
    let mut cmd = Command::new(git);
    cmd.arg("-c").arg("credential.helper=");
    if let Some(proxy) = crate::proxy::http_client::get_current_proxy_url() {
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    cmd.arg("ls-remote")
        .arg("--heads")
        .arg("--end-of-options")
        .arg(&url)
        .env("GIT_TERMINAL_PROMPT", "0");

    let output = timeout(
        Duration::from_secs(GIT_TIMEOUT_SECS),
        spawn_blocking(move || cmd.output()),
    )
    .await
    .map_err(|_| anyhow!("git ls-remote 超时"))?
    .map_err(|e| anyhow!("git 任务失败: {e}"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        log::debug!("[large_repo][ls-remote] 失败: url={url}: {stderr}");
        return Err(anyhow!("git ls-remote 失败: {stderr}"));
    }
    // 每行格式：`\t<sha>\trefs/heads/<branch>`
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.split('\t').nth(1) {
            if let Some(branch) = rest.strip_prefix("refs/heads/") {
                branches.push(branch.to_string());
            }
        }
    }
    log::debug!(
        "[large_repo][ls-remote] 成功: url={url} 分支数={}",
        branches.len()
    );
    Ok(branches)
}

/// 执行 `git clone --depth 1 --filter=blob:none --branch=<b> <url> <dest>`
///
/// 安全要点：
/// - 全部用参数数组，不用 shell（分支名等参数不会被 shell 解释）
/// - `--branch=<b>` 等号形式 + 位置参数前 `--end-of-options`（git ≥2.24）
/// - 应用配置了代理时传 `-c http.proxy` / `-c https.proxy`（无代理不传）
/// - `GIT_TERMINAL_PROMPT=0` + `-c credential.helper=`：禁用凭据提示，防挂起/弹窗
async fn clone_repo(
    git: &Path,
    base_url: &str,
    repo: &SkillRepo,
    branch: &str,
    dest: &Path,
) -> Result<()> {
    let url = format!("{base_url}/{}/{}.git", repo.owner, repo.name);
    log::debug!(
        "[large_repo][clone_repo] clone 开始: url={url} branch={branch} dest={}",
        dest.display()
    );
    let mut cmd = Command::new(git);
    cmd.arg("-c").arg("credential.helper=");
    if let Some(proxy) = crate::proxy::http_client::get_current_proxy_url() {
        log::debug!("[large_repo][clone_repo] 使用代理: {proxy}");
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    } else {
        log::debug!("[large_repo][clone_repo] 未配置代理");
    }
    cmd.arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--filter=blob:none")
        .arg(format!("--branch={branch}"))
        .arg("--end-of-options")
        .arg(&url)
        .arg(dest)
        .env("GIT_TERMINAL_PROMPT", "0");

    let argv: Vec<String> = std::iter::once(git.to_string_lossy().into_owned())
        .chain(cmd.get_args().map(|a| a.to_string_lossy().into_owned()))
        .collect();
    log::debug!(
        "[large_repo][clone_repo] 原始 argv: {}",
        argv.join(" ")
    );

    let output = timeout(
        Duration::from_secs(GIT_TIMEOUT_SECS),
        spawn_blocking(move || cmd.output()),
    )
    .await
    .map_err(|_| anyhow!("git clone 超时"))?
    .map_err(|e| anyhow!("git 任务失败: {e}"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        log::info!(
            "[large_repo][clone_repo] clone 失败: url={url} branch={branch}: {stderr}"
        );
        return Err(anyhow!("git clone 失败: {stderr}"));
    }
    log::debug!("[large_repo][clone_repo] clone 命令成功: url={url} branch={branch}");
    Ok(())
}

/// 在 clone 目录里执行 `git ls-tree -r -l -z HEAD`（30s 超时）
async fn run_git_ls_tree(git: &Path, workdir: &Path, proxy: Option<&str>) -> Result<Vec<u8>> {
    let git = git.to_path_buf();
    let workdir = workdir.to_path_buf();
    let proxy = proxy.map(|p| p.to_string());
    timeout(
        Duration::from_secs(30),
        spawn_blocking(move || {
            log::debug!(
                "[large_repo][ls-tree] 开始: workdir={}",
                workdir.display()
            );
            let mut cmd = Command::new(&git);
            if let Some(proxy) = &proxy {
                log::debug!("[large_repo][ls-tree] 使用代理: {proxy}");
                cmd.arg("-c").arg(format!("http.proxy={proxy}"));
                cmd.arg("-c").arg(format!("https.proxy={proxy}"));
            } else {
                log::debug!("[large_repo][ls-tree] 未配置代理");
            }
            let output = cmd
                .arg("ls-tree")
                .arg("-r")
                .arg("-l")
                .arg("-z")
                .arg("HEAD")
                .current_dir(&workdir)
                .output()?;
            let argv: Vec<String> = std::iter::once(git.to_string_lossy().into_owned())
                .chain(cmd.get_args().map(|a| a.to_string_lossy().into_owned()))
                .collect();
            log::debug!(
                "[large_repo][ls-tree] 原始 argv: {}",
                argv.join(" ")
            );
            if !output.status.success() {
                log::info!("[large_repo][ls-tree] 失败: workdir={}", workdir.display());
                return Err(anyhow!("git ls-tree 失败"));
            }
            log::debug!(
                "[large_repo][ls-tree] 成功: workdir={} 输出字节数={}",
                workdir.display(),
                output.stdout.len()
            );
            Ok(output.stdout)
        }),
    )
    .await
    .map_err(|_| {
        log::info!("[large_repo][ls-tree] 超时");
        anyhow!("git ls-tree 超时")
    })?
    .map_err(|e| anyhow!("git ls-tree 任务失败: {e}"))?
}

/// `git cat-file blob <sha>` 流式读取，带 `MAX_ARCHIVE_DOWNLOAD_BYTES` 预算
///
/// partial clone 下 blob 缺失时会触发 lazy fetch（显式传代理，与 clone/ls-tree 一致）。
fn cat_file_with_budget(git: &Path, workdir: &Path, sha: &str, proxy: Option<&str>) -> Result<Vec<u8>> {
    use std::io::Read;
    log::debug!(
        "[large_repo][cat-file] 开始: sha={sha} workdir={}",
        workdir.display()
    );
    let mut cmd = Command::new(git);
    if let Some(proxy) = proxy {
        log::debug!("[large_repo][cat-file] 使用代理: {proxy}");
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    } else {
        log::debug!("[large_repo][cat-file] 未配置代理");
    }
    let mut child = cmd
        .arg("cat-file")
        .arg("blob")
        .arg(sha)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let argv: Vec<String> = std::iter::once(git.to_string_lossy().into_owned())
        .chain(cmd.get_args().map(|a| a.to_string_lossy().into_owned()))
        .collect();
    log::debug!(
        "[large_repo][cat-file] 原始 argv: {}",
        argv.join(" ")
    );
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("无法读取 git 输出"))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = stdout.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        if buf.len().saturating_add(n) as u64 > MAX_ARCHIVE_DOWNLOAD_BYTES {
            let _ = child.kill();
            log::info!(
                "[large_repo][cat-file] 失败: sha={sha} 文件超过大小上限 ({}MB)",
                MAX_ARCHIVE_DOWNLOAD_BYTES / 1024 / 1024
            );
            return Err(anyhow!("文件超过大小上限"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let status = child.wait()?;
    if !status.success() {
        log::info!("[large_repo][cat-file] 失败: sha={sha} git 退出非 0");
        return Err(anyhow!("git cat-file 失败"));
    }
    log::debug!(
        "[large_repo][cat-file] 成功: sha={sha} 字节数={}",
        buf.len()
    );
    Ok(buf)
}

/// 递归统计目录占用字节数
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total = total.saturating_add(dir_size(&p));
            } else if let Ok(meta) = entry.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

// ========== ApiBackend ==========

/// GitHub REST API 后端：trees API 取清单，raw API 取文件。
///
/// base URL 可注入（默认 https://api.github.com / https://raw.githubusercontent.com），
/// 便于 mock 测试。
pub struct ApiBackend {
    api_base: String,
    raw_base: String,
    client: reqwest::Client,
}

impl ApiBackend {
    pub fn new() -> Self {
        Self {
            api_base: "https://api.github.com".to_string(),
            raw_base: "https://raw.githubusercontent.com".to_string(),
            client: crate::proxy::http_client::get(),
        }
    }

    /// 测试用构造器：注入 base URL（mock server）。
    ///
    /// 客户端用独立构建的 `no_proxy` 实例，避免测试机系统代理把 localhost
    /// 请求转发到代理导致 mock server 收不到。
    #[cfg(test)]
    pub(crate) fn with_bases(api_base: &str, raw_base: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self {
            api_base: api_base.to_string(),
            raw_base: raw_base.to_string(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl LargeRepoBackend for ApiBackend {
    async fn fetch_tree(&self, repo: &SkillRepo) -> Result<(Vec<RepoFile>, String)> {
        log::debug!(
            "[large_repo][ApiBackend] fetch_tree 开始: {}/{} branch_config={}",
            repo.owner,
            repo.name,
            repo.branch
        );
        SkillService::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;
        let mut last_error = None;
        for branch in branch_candidates(repo) {
            log::debug!("[large_repo][ApiBackend] fetch_tree 尝试分支: {branch}");
            let mut url = url::Url::parse(&format!(
                "{}/repos/{}/{}/git/trees/",
                self.api_base, repo.owner, repo.name
            ))?;
            url.path_segments_mut()
                .map_err(|_| anyhow!("无法构造 tree URL"))?
                .push(&branch);
            url.query_pairs_mut().append_pair("recursive", "1");

            let resp = match timeout(
                Duration::from_secs(API_TIMEOUT_SECS),
                self.client
                    .get(url)
                    .header("User-Agent", "cc-switch")
                    .send(),
            )
            .await
            {
                Ok(inner) => inner,
                Err(_) => {
                    last_error = Some(anyhow!("tree API 请求超时"));
                    continue;
                }
            };
            match resp {
                Ok(r) if r.status().as_u16() == 404 => {
                    log::debug!("[large_repo][ApiBackend] tree API 404: 分支不存在 {branch}");
                    last_error = Some(anyhow!("分支不存在: {branch}"));
                    continue;
                }
                Ok(r) if observe_rate_limit(&r) => {
                    // 命中 GitHub 限流：已标记，本轮后续分支不再走 REST API；
                    // 继续尝试其他分支无意义，直接失败冒泡由上层回退。
                    log::info!(
                        "[large_repo][ApiBackend] fetch_tree 失败: tree API 限流(403) {branch}"
                    );
                    last_error = Some(anyhow!("tree API 限流(403): {}", r.status()));
                    break;
                }
                Ok(r) if !r.status().is_success() => {
                    log::debug!(
                        "[large_repo][ApiBackend] tree API 非成功状态: {} {}",
                        r.status(),
                        branch
                    );
                    last_error = Some(anyhow!("tree API 失败: {}", r.status()));
                    continue;
                }
                Ok(r) => {
                    let body = read_body_limited(r, TREE_RESPONSE_BYTES_LIMIT).await?;
                    let (files, truncated) =
                        parse_tree_api_response(&String::from_utf8_lossy(&body))?;
                    if truncated {
                        log::info!(
                            "[large_repo][ApiBackend] fetch_tree 失败: tree 响应被截断，需回退 git 后端: {branch}"
                        );
                        return Err(anyhow!(
                            "tree 响应被截断（truncated=true），请回退 git 后端"
                        ));
                    }
                    if files.len() > TREE_ENTRY_LIMIT {
                        log::info!(
                            "[large_repo][ApiBackend] fetch_tree 失败: tree 条目数超过上限({}): {branch}",
                            TREE_ENTRY_LIMIT
                        );
                        return Err(anyhow!("tree 条目数超过上限"));
                    }
                    log::info!(
                        "[large_repo][ApiBackend] fetch_tree 成功: {}/{} 命中分支={branch} 文件数={}",
                        repo.owner,
                        repo.name,
                        files.len()
                    );
                    return Ok((files, branch));
                }
                Err(e) => {
                    log::debug!("[large_repo][ApiBackend] tree API 请求失败: {branch}: {e}");
                    last_error = Some(anyhow!("tree API 请求失败: {e}"));
                    continue;
                }
            }
        }
        let err = last_error.unwrap_or_else(|| anyhow!("所有候选分支 tree 请求失败"));
        log::info!(
            "[large_repo][ApiBackend] fetch_tree 失败: {}/{}: {err:#}",
            repo.owner,
            repo.name
        );
        Err(err)
    }

    async fn fetch_file(&self, repo: &SkillRepo, branch: &str, file: &RepoFile) -> Result<Vec<u8>> {
        log::debug!(
            "[large_repo][ApiBackend] fetch_file 开始: {}/{} branch={} file={}",
            repo.owner,
            repo.name,
            branch,
            file.path
        );
        let mut url = url::Url::parse(&format!(
            "{}/{}/{}/{}/",
            self.raw_base, repo.owner, repo.name, branch
        ))?;
        for seg in file.path.split('/') {
            url.path_segments_mut()
                .map_err(|_| anyhow!("无法构造 raw URL"))?
                .push(seg);
        }
        let resp = timeout(
            Duration::from_secs(API_TIMEOUT_SECS),
            self.client
                .get(url)
                .header("User-Agent", "cc-switch")
                .send(),
        )
        .await
        .map_err(|_| anyhow!("raw 文件获取超时"))??;
        if observe_rate_limit(&resp) {
            // 命中 GitHub 限流：已标记，本次操作失败冒泡回退ZIP/GitBackend。
            log::info!(
                "[large_repo][ApiBackend] fetch_file 失败: raw 限流(403): {}/{} file={}",
                repo.owner,
                repo.name,
                file.path
            );
            return Err(anyhow!("raw 文件获取限流(403): {}", resp.status()));
        }
        if !resp.status().is_success() {
            log::info!(
                "[large_repo][ApiBackend] fetch_file 失败: raw 状态 {}: {}/{} file={}",
                resp.status(),
                repo.owner,
                repo.name,
                file.path
            );
            return Err(anyhow!("raw 文件获取失败: {}", resp.status()));
        }
        let bytes = read_body_limited(resp, MAX_ARCHIVE_DOWNLOAD_BYTES).await?;
        log::debug!(
            "[large_repo][ApiBackend] fetch_file 成功: {}/{} file={} 字节数={}",
            repo.owner,
            repo.name,
            file.path,
            bytes.len()
        );
        Ok(bytes)
    }
}

/// 流式读取响应体并卡住字节上限
async fn read_body_limited(mut resp: reqwest::Response, limit: u64) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if body.len().saturating_add(chunk.len()) as u64 > limit {
            log::info!("[large_repo][read_body] 失败: 响应体超过大小上限 ({}MB)", limit / 1024 / 1024);
            return Err(anyhow!("响应体超过大小上限"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

// ========== 共享纯逻辑（无 I/O，可离线单测） ==========

/// 分支候选：配置分支（非空且非 HEAD）→ main → master，去重
pub fn branch_candidates(repo: &SkillRepo) -> Vec<String> {
    log::debug!(
        "[large_repo][branch_candidates] 配置分支={} 是否HEAD={}",
        repo.branch,
        repo.branch.eq_ignore_ascii_case("HEAD")
    );
    let mut out = Vec::new();
    if !repo.branch.is_empty() && !repo.branch.eq_ignore_ascii_case("HEAD") {
        out.push(repo.branch.clone());
    }
    if !out.iter().any(|b| b == "main") {
        out.push("main".to_string());
    }
    if !out.iter().any(|b| b == "master") {
        out.push("master".to_string());
    }
    log::debug!("[large_repo][branch_candidates] 候选顺序={:?}", out);
    out
}

/// 解析 `git ls-tree -r -l -z` 输出（NUL 分隔）。
///
/// 每行格式：`<mode> <type> <sha> <size>\t<path>`。只保留 type=blob 的条目。
/// 非 UTF-8 文件名按字节做 lossy 转换，不能直接丢弃。
pub fn parse_ls_tree_output(output: &[u8]) -> Result<Vec<RepoFile>> {
    log::debug!(
        "[large_repo][parse_ls_tree] 开始: 输入字节数={}",
        output.len()
    );
    let mut files = Vec::new();
    for entry in output.split(|&b| b == 0) {
        if entry.is_empty() {
            continue;
        }
        let tab_pos = entry
            .iter()
            .position(|&b| b == b'\t')
            .ok_or_else(|| anyhow!("ls-tree 条目缺少 tab 分隔"))?;
        let meta = std::str::from_utf8(&entry[..tab_pos])
            .map_err(|_| anyhow!("ls-tree 元数据不是 UTF-8"))?;
        let mut parts = meta.split_whitespace();
        let mode = parts.next().ok_or_else(|| anyhow!("缺少 mode"))?;
        let obj_type = parts.next().ok_or_else(|| anyhow!("缺少 type"))?;
        let sha = parts.next().ok_or_else(|| anyhow!("缺少 sha"))?;
        let size = parts.next().unwrap_or("-");
        if obj_type != "blob" {
            continue;
        }
        files.push(RepoFile {
            path: String::from_utf8_lossy(&entry[tab_pos + 1..]).into_owned(),
            blob_sha: sha.to_string(),
            size: size.parse::<u64>().unwrap_or(0),
            mode: u32::from_str_radix(mode, 8).unwrap_or(0),
        });
    }
    log::debug!(
        "[large_repo][parse_ls_tree] 完成: 解析出 blob 文件数={}",
        files.len()
    );
    Ok(files)
}

/// 解析 GitHub trees API 响应。
///
/// 返回 (文件清单, truncated)。只保留 type=blob 的条目。
pub fn parse_tree_api_response(json: &str) -> Result<(Vec<RepoFile>, bool)> {
    log::debug!(
        "[large_repo][parse_tree_api] 开始: 输入长度={}",
        json.len()
    );
    let value: serde_json::Value = serde_json::from_str(json)?;
    let truncated = value
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let entries = value
        .get("tree")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("tree 响应缺少 tree 数组"))?;
    let mut files = Vec::new();
    for entry in entries {
        if entry.get("type").and_then(|v| v.as_str()) != Some("blob") {
            continue;
        }
        let path = entry
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let blob_sha = entry
            .get("sha")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let size = entry.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let mode = entry
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        files.push(RepoFile {
            path,
            blob_sha,
            size,
            mode: u32::from_str_radix(&mode, 8).unwrap_or(0),
        });
    }
    log::debug!(
        "[large_repo][parse_tree_api] 完成: 解析出 blob 文件数={} truncated={truncated}",
        files.len()
    );
    Ok((files, truncated))
}

/// 精确匹配 "SKILL.md" 后缀（大小写敏感，与 skill.rs 的 scan_dir_recursive 一致）
pub fn filter_skill_md_paths(files: &[RepoFile]) -> Vec<&RepoFile> {
    files
        .iter()
        .filter(|f| f.path == "SKILL.md" || f.path.ends_with("/SKILL.md"))
        .collect()
}

/// 目录内文件（大小写不敏感匹配目录前缀），rel path 排序，
/// SHA-256 over 每个 (rel_path + "\0" + blob_sha + "\0")。
///
/// 目录不存在（无匹配文件）→ None；过滤 mode 160000（submodule）。
pub fn dir_blob_hash(files: &[RepoFile], directory: &str) -> Option<String> {
    let mut entries: Vec<(String, &str)> = Vec::new();
    for f in files {
        if f.mode == 0o160000 {
            continue;
        }
        if let Some(rel) = rel_path_in_dir(&f.path, directory) {
            entries.push((rel.to_string(), f.blob_sha.as_str()));
        }
    }
    if entries.is_empty() {
        return None;
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (rel, sha) in entries {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(sha.as_bytes());
        hasher.update(b"\0");
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// 计算 git blob 的 SHA-1：SHA-1("blob " + size + "\0" + content)
pub fn git_blob_sha(content: &[u8]) -> String {
    let mut hasher = sha1::Sha1::new();
    hasher.update(format!("blob {}\0", content.len()).as_bytes());
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// 计算 git blob 的 SHA-256：SHA-256("blob " + size + "\0" + content)
///
/// SHA-256 对象格式仓库（`git init --object-format=sha256`）的 blob id 是
/// 64 hex，本地重算时必须用同一格式，否则与远端 `dir_blob_hash` 对不上。
pub fn git_blob_sha256(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("blob {}\0", content.len()).as_bytes());
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// blob 哈希方案（由远端 blob id 长度决定：40 hex = SHA-1，64 hex = SHA-256）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobHashScheme {
    Sha1,
    Sha256,
}

impl BlobHashScheme {
    /// 从存储前缀推断方案；无前缀（旧方案）返回 None
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            BLOB_SHA1_PREFIX => Some(Self::Sha1),
            BLOB_SHA256_PREFIX => Some(Self::Sha256),
            _ => None,
        }
    }

    /// 本方案对应的存储前缀
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Sha1 => BLOB_SHA1_PREFIX,
            Self::Sha256 => BLOB_SHA256_PREFIX,
        }
    }
}

/// 从 blob id 长度推断仓库的 blob 哈希方案（40 hex = SHA-1，64 hex = SHA-256）
pub fn blob_scheme_from_sha(blob_sha: &str) -> BlobHashScheme {
    if blob_sha.len() == 64 {
        BlobHashScheme::Sha256
    } else {
        BlobHashScheme::Sha1
    }
}

/// 从 tree 清单推断仓库的 blob 哈希方案（同一仓库内所有 blob 用同一对象格式）
pub fn tree_blob_scheme(files: &[RepoFile]) -> BlobHashScheme {
    files
        .iter()
        .find(|f| f.mode != 0o160000)
        .map(|f| blob_scheme_from_sha(&f.blob_sha))
        .unwrap_or(BlobHashScheme::Sha1)
}

/// 计算本地目录的 blob 哈希（与 `dir_blob_hash` 相同算法）。
///
/// 与旧 `compute_dir_hash` 不同：**包含隐藏文件**（点开头文件也算）。
/// 路径分隔符归一化 \ → /。`scheme` 决定每个文件用 SHA-1 还是 SHA-256
/// 计算 git blob id，必须与远端仓库的对象格式一致。
pub fn compute_local_blob_hash(dir: &Path, scheme: BlobHashScheme) -> Result<String> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_all_files(dir, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for file_path in &files {
        let rel = file_path.strip_prefix(dir).unwrap_or(file_path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let content = fs::read(file_path)?;
        let blob_sha = match scheme {
            BlobHashScheme::Sha1 => git_blob_sha(&content),
            BlobHashScheme::Sha256 => git_blob_sha256(&content),
        };
        hasher.update(rel_str.as_bytes());
        hasher.update(b"\0");
        hasher.update(blob_sha.as_bytes());
        hasher.update(b"\0");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 递归收集目录下所有文件（含隐藏文件）
fn collect_all_files(current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_all_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// 校验并规范化 tree 内路径，拒绝路径穿越与非法字符。
///
/// 拒绝：`..`、`\`、前导 `/`、控制字符、`#?%`。
pub fn sanitize_tree_path(path: &str) -> Result<String> {
    if path.is_empty() {
        return Err(anyhow!("路径为空"));
    }
    if path.starts_with('/') {
        return Err(anyhow!("路径不能以 / 开头"));
    }
    if path.contains('\\') {
        return Err(anyhow!("路径不能包含反斜杠"));
    }
    if path.split('/').any(|seg| seg == "..") {
        return Err(anyhow!("路径不能包含 .."));
    }
    if path.chars().any(|c| c.is_ascii_control()) {
        return Err(anyhow!("路径不能包含控制字符"));
    }
    if path.contains('#') || path.contains('?') || path.contains('%') {
        return Err(anyhow!("路径不能包含 # ? %"));
    }
    Ok(path.to_string())
}

/// 大仓库路径判定：None → true（size 探测失败时优先尝试大仓库后端，
/// 后端全失败后仍有 ZIP 兜底）；Some(s) → s > 32MB
///
/// size 探测请求（GitHub REST API）可能被限流/阻断/瞬时不可用，此时返回 None。
/// 若直接走旧 ZIP 路径，恰恰是本次要支持的超大仓库会在 128MiB 预算处失败，
/// 而 git 后端（smart HTTP 协议，不吃 REST 限流）仍可用。
pub fn should_use_large_repo_path(size_kb: Option<u64>) -> bool {
    let result = match size_kb {
        None => true,
        Some(s) => s > LARGE_REPO_THRESHOLD_KB,
    };
    log::debug!(
        "[large_repo][should_use_large_repo_path] size_kb={:?} 阈值={}KB => {result}",
        size_kb,
        LARGE_REPO_THRESHOLD_KB
    );
    result
}

/// 仓库 size 缓存 TTL：1 小时
const SIZE_CACHE_TTL: Duration = Duration::from_secs(3600);
type SizeCache = HashMap<(String, String), (Instant, u64)>;
static SIZE_CACHE: LazyLock<Mutex<SizeCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// GitHub 匿名 REST API 限流冷却窗口：1 小时（对齐匿名配额窗口）。
/// 窗口内命中限流后，跳过所有 GitHub REST API 后端，让位给 git 后端或回退 ZIP。
const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(3600);
/// 上次遇到 GitHub REST API 限流的时间戳（全局共享，按 IP 配额计，不区分 owner/端点）。
static RATE_LIMITED_AT: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));

/// 标记 GitHub REST API 限流：记录当前时刻。后续 1 小时内的裁决会跳过 ApiBackend。
pub fn mark_rate_limited() {
    *RATE_LIMITED_AT.lock().unwrap() = Some(Instant::now());
}

/// 是否处于限流冷却窗口内（限流时间戳存在且未过期）。
pub fn is_rate_limited() -> bool {
    match *RATE_LIMITED_AT.lock().unwrap() {
        Some(ts) => ts.elapsed() < RATE_LIMIT_COOLDOWN,
        None => false,
    }
}

/// 解析 GitHub 限流剩余配额响应头 `X-RateLimit-Remaining`。
fn rate_limit_remaining(resp: &reqwest::Response) -> Option<u64> {
    resp.headers()
        .get("X-RateLimit-Remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

/// 判定响应是否为 GitHub 限流信号：HTTP 403 + `X-RateLimit-Remaining: 0`。
///
/// 仅双条件同时满足才视为限流，避免把「无权限 403」「其他非 2xx」误判成限流。
fn is_rate_limited_response(resp: &reqwest::Response) -> bool {
    resp.status().as_u16() == 403 && rate_limit_remaining(resp) == Some(0)
}

/// 检测并消费一个限流响应：命中则标记并返回 true。
fn observe_rate_limit(resp: &reqwest::Response) -> bool {
    if is_rate_limited_response(resp) {
        log::info!(
            "[large_repo][observe_rate_limit] 命中 GitHub REST 限流 (403 remaining=0)，进入 1h 冷却窗口"
        );
        mark_rate_limited();
        true
    } else {
        false
    }
}

/// 获取仓库 size（KB）。失败返回 Ok(None)（上层优先尝试大仓库后端）。1h TTL 缓存。
pub async fn fetch_repo_size_kb(owner: &str, name: &str) -> Result<Option<u64>> {
    log::debug!("[large_repo][size] 开始: {owner}/{name}");
    let key = (owner.to_string(), name.to_string());
    if let Some((ts, size)) = SIZE_CACHE.lock().unwrap().get(&key) {
        if ts.elapsed() < SIZE_CACHE_TTL {
            log::debug!("[large_repo][size] 命中缓存: {owner}/{name} size={size}KB");
            return Ok(Some(*size));
        }
    }
    // 限流冷却窗口内：不再发起无谓的 GitHub REST 请求，直接返回 None。
    // None → should_use_large_repo_path → true → 进大仓库路径；此时 backend_chain
    // 已剔除 ApiBackend，落到 GitBackend 或回退 ZIP。符合「size 限流后无需再用 REST API」。
    if is_rate_limited() {
        log::debug!("[large_repo][size] 限流冷却窗口内，跳过 size 探测返回 None");
        return Ok(None);
    }
    let url = format!("https://api.github.com/repos/{owner}/{name}");
    let client = crate::proxy::http_client::get();
    let size = match timeout(
        Duration::from_secs(API_TIMEOUT_SECS),
        client.get(&url).header("User-Agent", "cc-switch").send(),
    )
    .await
    {
        Ok(Ok(resp)) => {
            if observe_rate_limit(&resp) {
                // 命中限流：已标记，返回 None 让下游裁决（不再走 REST API）。
                log::info!("[large_repo][size] 限流，size 探测返回 None: {owner}/{name}");
                None
            } else if resp.status().is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        let s = json.get("size").and_then(|v| v.as_u64());
                        log::debug!("[large_repo][size] 探测成功: {owner}/{name} size={:?}KB", s);
                        s
                    }
                    Err(_) => {
                        log::debug!("[large_repo][size] 响应 JSON 解析失败: {owner}/{name}");
                        None
                    }
                }
            } else {
                log::debug!(
                    "[large_repo][size] 非成功状态 {}: {owner}/{name}",
                    resp.status()
                );
                None
            }
        }
        _ => {
            log::debug!("[large_repo][size] 请求超时或失败: {owner}/{name}");
            None
        }
    };
    if let Some(size) = size {
        SIZE_CACHE
            .lock()
            .unwrap()
            .insert(key, (Instant::now(), size));
        Ok(Some(size))
    } else {
        Ok(None)
    }
}

/// 探测 git 可执行文件：PATH + 常见安装位置，结果缓存
pub fn detect_git() -> Option<PathBuf> {
    static CACHE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CACHE.get_or_init(detect_git_uncached).clone()
}

fn detect_git_uncached() -> Option<PathBuf> {
    // 1. PATH
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for name in ["git.exe", "git"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    // 2. 常见安装位置
    let mut candidates = Vec::new();
    if let Some(pf) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
        candidates.push(pf.join("Git").join("cmd").join("git.exe"));
    }
    if let Some(pf) = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from) {
        candidates.push(pf.join("Git").join("cmd").join("git.exe"));
    }
    // 3. GitHub Desktop PortableGit（app-<version> 目录版本号会变，扫目录）
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let gh_desktop = local.join("GitHubDesktop");
        if let Ok(entries) = fs::read_dir(&gh_desktop) {
            let mut app_dirs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .map(|n| n.to_string_lossy().starts_with("app-"))
                            .unwrap_or(false)
                })
                .collect();
            app_dirs.sort();
            for app_dir in app_dirs.iter().rev() {
                let git = app_dir
                    .join("resources")
                    .join("app")
                    .join("git")
                    .join("cmd")
                    .join("git.exe");
                if git.is_file() {
                    return Some(git);
                }
            }
        }
    }
    candidates.into_iter().find(|c| c.is_file())
}

/// 选择后端：有 git → GitBackend，无 → ApiBackend
pub fn select_backend() -> Box<dyn LargeRepoBackend> {
    let git = detect_git().is_some();
    log::debug!("[large_repo][select_backend] git 可用={git}");
    if git {
        Box::new(GitBackend::new())
    } else {
        Box::new(ApiBackend::new())
    }
}

/// 后端回退链：git 可用 → [Git, API]；git 不可用 → [API]。
///
/// 上层按序尝试，全部失败后再回退旧 ZIP 路径。
///
/// 限流冷却窗口内：剔除 ApiBackend（GitHub REST API 已被限流，再试必然失败且浪费配额）。
/// 此时 git 可用 → [Git]（git 走 smart HTTP，不吃 REST 限流）；git 不可用 → 空链，
/// 调用方收到空链即视为「大仓库后端全失败」→ 回退旧 ZIP 路径（codeload 域独立配额）。
pub fn backend_chain() -> Vec<Box<dyn LargeRepoBackend>> {
    let git = detect_git().is_some();
    let use_api = !is_rate_limited();
    log::debug!(
        "[large_repo][backend_chain] git={git} use_api={use_api} (限流冷却={})",
        is_rate_limited()
    );
    let mut chain: Vec<Box<dyn LargeRepoBackend>> = Vec::new();
    if git {
        chain.push(Box::new(GitBackend::new()));
    }
    if use_api {
        chain.push(Box::new(ApiBackend::new()));
    }
    log::debug!("[large_repo][backend_chain] 回退链长度={}", chain.len());
    chain
}

/// 本地存储哈希是否需要按远端方案重算
///
/// 双向对齐：远端什么 blob 方案（SHA-1/SHA-256/旧无前缀），本地就重算成什么方案。
/// - `remote_scheme` 非空（blob-sha1:/blob-sha256:）：本地必须带相同前缀
/// - `remote_scheme` 为空（旧方案）：本地必须是无前缀的旧哈希
pub fn hash_needs_recompute(stored: &str, remote_scheme: &str) -> bool {
    if remote_scheme.is_empty() {
        // 远端旧方案：本地带 blob 前缀 → 需要重算成旧方案
        stored.starts_with(BLOB_SHA1_PREFIX) || stored.starts_with(BLOB_SHA256_PREFIX)
    } else {
        // 远端 blob 方案：本地必须带相同前缀
        !stored.starts_with(remote_scheme)
    }
}

/// 从 SKILL.md 内容构建 DiscoverableSkill（复刻 skill.rs 的构造逻辑）
///
/// - 根级 SKILL.md（path 无 /）→ directory = repo.name
/// - 否则 directory = 父目录
/// - readme_url 用 build_skill_doc_url（坐标非法时返回 None）
///
/// 路径先过 `sanitize_tree_path`：tree 路径来自远端仓库，可能含 `..`/`#` 等
/// 会改写 readme_url 落点的字符（前端用 openExternal 打开），纵深防御。
pub fn build_discoverable_skill(
    owner: &str,
    name: &str,
    branch: &str,
    path: &str,
    content: &str,
) -> Result<DiscoverableSkill> {
    let path = sanitize_tree_path(path)?;
    let meta = SkillService::parse_skill_metadata_content(content);
    let directory = if path.contains('/') {
        path.rsplit_once('/')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default()
    } else {
        name.to_string()
    };
    let readme_url = SkillService::build_skill_doc_url(owner, name, branch, &path);
    Ok(DiscoverableSkill {
        key: format!("{owner}/{name}:{directory}"),
        name: meta.name.unwrap_or_else(|| directory.clone()),
        description: meta.description.unwrap_or_default(),
        directory,
        readme_url,
        repo_owner: owner.to_string(),
        repo_name: name.to_string(),
        repo_branch: branch.to_string(),
    })
}

// ========== 派生操作（泛型 over backend） ==========

/// 列出仓库内所有 skill（SKILL.md → DiscoverableSkill）
pub async fn list_skill_mds(
    backend: &dyn LargeRepoBackend,
    repo: &SkillRepo,
) -> Result<Vec<DiscoverableSkill>> {
    log::debug!(
        "[large_repo][list_skill_mds] 开始: {}/{}",
        repo.owner,
        repo.name
    );
    let (files, branch) = backend.fetch_tree(repo).await?;
    let md_files = filter_skill_md_paths(&files);
    log::debug!(
        "[large_repo][list_skill_mds] 发现 SKILL.md 文件数={} branch={branch}",
        md_files.len()
    );
    let mut skills = Vec::new();
    for f in md_files {
        let content = backend.fetch_file(repo, &branch, f).await?;
        let content = String::from_utf8_lossy(&content);
        if let Ok(skill) =
            build_discoverable_skill(&repo.owner, &repo.name, &branch, &f.path, &content)
        {
            skills.push(skill);
        }
    }
    log::info!(
        "[large_repo][list_skill_mds] 完成: {}/{} 解析出 skill 数={}",
        repo.owner,
        repo.name,
        skills.len()
    );
    Ok(skills)
}

/// 从 tree 文件清单枚举所有技能目录（含 SKILL.md 的目录）。
///
/// 嵌套 SKILL.md → 父目录（仓库相对路径，如 `skills/foo`）；
/// 根级 SKILL.md → 空字符串（表示仓库根，哈希范围是整个仓库）。
/// 与 `build_discoverable_skill` / `scan_dir_recursive` 的目录推导一致。
pub fn skill_dirs_from_tree(files: &[RepoFile]) -> Vec<String> {
    let mut dirs = Vec::new();
    for f in filter_skill_md_paths(files) {
        let dir = f
            .path
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
            .unwrap_or_default();
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// 将安装名（SSOT 落盘名，目录最后一段）匹配到枚举出的技能目录。
///
/// 嵌套 skill 的安装名只是目录最后一段（`foo` 对应仓库里的 `skills/foo`），
/// 按最后一段匹配才能命中；根级 skill（空串目录）用仓库名作键。
fn match_install_name<'a>(
    skill_dirs: &'a [String],
    install_name: &str,
    repo_name: &str,
) -> Option<&'a str> {
    skill_dirs
        .iter()
        .find(|d| {
            d.rsplit('/')
                .next()
                .is_some_and(|last| last.eq_ignore_ascii_case(install_name))
                || (d.is_empty() && install_name.eq_ignore_ascii_case(repo_name))
        })
        .map(|x| x.as_str())
}

/// 计算多个安装名的远端 blob 哈希（未匹配的安装名跳过）。
///
/// `install_names` 是 SSOT 落盘名（目录最后一段，如 `foo`），仓库里实际可能是
/// 嵌套路径（如 `skills/foo`）。因此先枚举 tree 中所有技能目录，再按**目录最后
/// 一段**匹配安装名，嵌套 skill 也能命中——与旧 ZIP 路径 `check_updates_via_zip`
/// 的扫描 + 末段匹配行为一致。
///
/// 返回 `(目录, 哈希)` 对，目录是仓库相对路径（根级 skill 用仓库名作键），
/// 哈希带方案前缀（`blob-sha1:` / `blob-sha256:`），方案由 tree 内 blob id
/// 长度推断（40 hex = SHA-1，64 hex = SHA-256）。
pub async fn skill_dir_hashes(
    backend: &dyn LargeRepoBackend,
    repo: &SkillRepo,
    install_names: &[String],
) -> Result<Vec<(String, String)>> {
    log::debug!(
        "[large_repo][skill_dir_hashes] 开始: {}/{} 安装名数={}",
        repo.owner,
        repo.name,
        install_names.len()
    );
    let (files, _branch) = backend.fetch_tree(repo).await?;
    let scheme = tree_blob_scheme(&files);
    let skill_dirs = skill_dirs_from_tree(&files);
    log::debug!(
        "[large_repo][skill_dir_hashes] 枚举技能目录数={} 方案={}",
        skill_dirs.len(),
        scheme.prefix()
    );
    let mut out = Vec::new();
    for name in install_names {
        let Some(dir) = match_install_name(&skill_dirs, name, &repo.name) else {
            log::debug!("[large_repo][skill_dir_hashes] 安装名未匹配: {name}");
            continue;
        };
        let Some(h) = dir_blob_hash(&files, dir) else {
            log::debug!("[large_repo][skill_dir_hashes] 目录无 blob 哈希: {dir}");
            continue;
        };
        let key = if dir.is_empty() {
            repo.name.clone()
        } else {
            dir.to_string()
        };
        out.push((key, format!("{}{}", scheme.prefix(), h)));
    }
    log::info!(
        "[large_repo][skill_dir_hashes] 完成: {}/{} 计算出哈希数={}",
        repo.owner,
        repo.name,
        out.len()
    );
    Ok(out)
}

/// 物化 skill 目录到临时目录，返回 (TempDir, used_branch, blob 方案)。
///
/// 外层 60s 超时（对齐旧路径 download_repo 的 60s 预算）；单文件读取另有
/// `fetch_file` 内部的 60s 超时与 `MAX_ARCHIVE_DOWNLOAD_BYTES` 预算，
/// 目录总字节上限对齐 `MAX_ARCHIVE_TOTAL_BYTES`。
pub async fn materialize_skill_dir(
    backend: &dyn LargeRepoBackend,
    repo: &SkillRepo,
    dir: &str,
) -> Result<(TempDir, String, BlobHashScheme)> {
    log::debug!(
        "[large_repo][materialize] 开始: {}/{} dir={}",
        repo.owner,
        repo.name,
        dir
    );
    timeout(Duration::from_secs(60), async {
        let (files, branch) = backend.fetch_tree(repo).await?;
        let scheme = tree_blob_scheme(&files);
        let temp_dir = tempfile::tempdir()?;
        let mut total_bytes: u64 = 0;
        // 根级 skill 的 directory 是仓库名哨兵（见 build_discoverable_skill），
        // 代表仓库根而不是 <仓库名>/ 子目录；先把哨兵解析为 tree 根。
        let dir = resolve_materialize_dir(dir, &repo.name);
        let mut materialized_skill_md = false;
        // mode 120000 的符号链接条目：blob 内容是目标路径文本，不能当普通文件写。
        // 第一遍物化所有普通文件并收集 symlink 条目，第二遍（普通文件全部落盘后）
        // 调用 SkillService::resolve_symlinks_in_dir 把目标内容复制到链接位置。
        // 语义与 skill.rs 的 ZIP 路径（extract_repo_archive）完全一致，确保两条
        // 路径产出逐字节等价的自包含副本。
        let mut symlinks: Vec<(PathBuf, String)> = Vec::new();
        for f in &files {
            if f.mode == 0o160000 {
                continue;
            }
            let Some(rel) = rel_path_in_dir(&f.path, dir) else {
                continue;
            };
            let rel = sanitize_tree_path(rel)?;
            if rel == "SKILL.md" {
                materialized_skill_md = true;
            }
            let dest = temp_dir.path().join(&rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let bytes = backend.fetch_file(repo, &branch, f).await?;
            if f.mode == 0o120000 {
                // symlink：bytes 是目标路径文本。用 read_symlink_target 复用 ZIP
                // 路径的限长 + 计费语义（超长/非 UTF-8 返回 None → 跳过）。
                let mut cursor = std::io::Cursor::new(&bytes);
                match SkillService::read_symlink_target(&mut cursor, &mut total_bytes)? {
                    Some(target) => symlinks.push((dest, target)),
                    None => log::warn!("跳过目标不合法的 symlink 条目: {}", f.path),
                }
                continue;
            }
            total_bytes = total_bytes.saturating_add(bytes.len() as u64);
            if total_bytes > MAX_ARCHIVE_TOTAL_BYTES {
                log::info!(
                    "[large_repo][materialize] 失败: 物化目录超过字节上限({}MB)",
                    MAX_ARCHIVE_TOTAL_BYTES / 1024 / 1024
                );
                return Err(anyhow!("物化目录超过字节上限"));
            }
            fs::write(&dest, &bytes)?;
        }
        // 第二遍：解析 symlink 到目标内容。必须在所有普通文件落盘之后调用，
        // 确保 symlink target 已存在可供 canonicalize。复用 skill.rs 的实现，
        // 含越界守卫、self-containing 守卫与共享 total_bytes 预算计费。
        SkillService::resolve_symlinks_in_dir(temp_dir.path(), &symlinks, &mut total_bytes)?;
        // 目录里没有 SKILL.md 说明不是合法 skill 目录（dir 未命中或错配），
        // 必须显式报错让调用方的后端回退链继续，而不是静默返回空目录。
        if !materialized_skill_md {
            log::info!(
                "[large_repo][materialize] 失败: 物化目录未找到 SKILL.md: {}/{} dir={}",
                repo.owner,
                repo.name,
                dir
            );
            return Err(anyhow!("物化目录未找到 SKILL.md"));
        }
        log::info!(
            "[large_repo][materialize] 成功: {}/{} dir={} branch={branch} 方案={} 总字节数={}",
            repo.owner,
            repo.name,
            dir,
            scheme.prefix(),
            total_bytes
        );
        Ok((temp_dir, branch, scheme))
    })
    .await
    .map_err(|_| {
        log::info!(
            "[large_repo][materialize] 失败: 物化超时(60s): {}/{} dir={}",
            repo.owner,
            repo.name,
            dir
        );
        anyhow!("物化 skill 目录超时")
    })?
}

/// 返回 path 相对 dir 的路径；不在 dir 下返回 None（大小写不敏感匹配目录前缀）
fn rel_path_in_dir<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    let dir = dir.trim_matches('/');
    if dir.is_empty() {
        return Some(path);
    }
    let prefix = format!("{dir}/");
    if path.len() > prefix.len() && path[..prefix.len()].eq_ignore_ascii_case(&prefix) {
        Some(&path[prefix.len()..])
    } else {
        None
    }
}

/// 解析物化目标目录为 tree 相对路径。
///
/// 根级 skill 的 `directory` 是仓库名哨兵（见 `build_discoverable_skill`），
/// 表示仓库根而不是 `<仓库名>/` 子目录；嵌套 skill 原样返回。
fn resolve_materialize_dir<'a>(dir: &'a str, repo_name: &str) -> &'a str {
    if dir.eq_ignore_ascii_case(repo_name) {
        ""
    } else {
        dir
    }
}

// ========== 纯逻辑单测（全部离线） ==========

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Mock 后端：用预构造的 RepoFile 列表 + blob 内容映射模拟仓库，
    /// 专门测试 materialize_skill_dir 对 mode 120000 symlink 的处理。
    /// 跨平台：不依赖文件系统 symlink，blob 内容由测试直接提供。
    struct MockSymlinkBackend {
        files: Vec<RepoFile>,
        /// blob_sha → 文件字节（普通文件）或目标路径文本（symlink）
        blobs: HashMap<String, Vec<u8>>,
        branch: String,
    }

    #[async_trait::async_trait]
    impl LargeRepoBackend for MockSymlinkBackend {
        async fn fetch_tree(&self, _repo: &SkillRepo) -> Result<(Vec<RepoFile>, String)> {
            Ok((self.files.clone(), self.branch.clone()))
        }

        async fn fetch_file(
            &self,
            _repo: &SkillRepo,
            _branch: &str,
            file: &RepoFile,
        ) -> Result<Vec<u8>> {
            self.blobs
                .get(&file.blob_sha)
                .cloned()
                .ok_or_else(|| anyhow!("mock 缺少 blob: {}", file.blob_sha))
        }
    }

    /// 构造含 symlink 的 mock 仓库：
    /// - SKILL.md（普通文件，满足合法 skill 校验）
    /// - target.txt（普通文件，作为 symlink 目标）
    /// - shared/（普通目录，作为 symlink 目标）
    /// - shared/nested.txt
    /// - link-to-file → target.txt（mode 120000）
    /// - link-to-dir → shared（mode 120000）
    /// - link-self → .（mode 120000，self-containing，应跳过）
    /// - link-escape → ../../etc/passwd（mode 120000，越界，应跳过）
    fn mock_symlink_backend() -> MockSymlinkBackend {
        let skill_md_sha = "skillmd".to_string();
        let target_sha = "target".to_string();
        let nested_sha = "nested".to_string();
        let link_file_sha = "linkfile".to_string();
        let link_dir_sha = "linkdir".to_string();
        let link_self_sha = "linkself".to_string();
        let link_escape_sha = "linkescape".to_string();

        let files = vec![
            RepoFile {
                path: "SKILL.md".to_string(),
                blob_sha: skill_md_sha.clone(),
                size: 10,
                mode: 0o100644,
            },
            RepoFile {
                path: "target.txt".to_string(),
                blob_sha: target_sha.clone(),
                size: 7,
                mode: 0o100644,
            },
            RepoFile {
                path: "shared/nested.txt".to_string(),
                blob_sha: nested_sha.clone(),
                size: 8,
                mode: 0o100644,
            },
            RepoFile {
                path: "link-to-file".to_string(),
                blob_sha: link_file_sha.clone(),
                size: 10,
                mode: 0o120000,
            },
            RepoFile {
                path: "link-to-dir".to_string(),
                blob_sha: link_dir_sha.clone(),
                size: 6,
                mode: 0o120000,
            },
            RepoFile {
                path: "link-self".to_string(),
                blob_sha: link_self_sha.clone(),
                size: 1,
                mode: 0o120000,
            },
            RepoFile {
                path: "link-escape".to_string(),
                blob_sha: link_escape_sha.clone(),
                size: 20,
                mode: 0o120000,
            },
        ];

        let mut blobs = HashMap::new();
        blobs.insert(skill_md_sha, b"---\nname: t\n---\n".to_vec());
        blobs.insert(target_sha, b"content".to_vec());
        blobs.insert(nested_sha, b"nested\n".to_vec());
        // symlink blob 内容是目标路径文本
        blobs.insert(link_file_sha, b"target.txt".to_vec());
        blobs.insert(link_dir_sha, b"shared".to_vec());
        blobs.insert(link_self_sha, b".".to_vec());
        blobs.insert(link_escape_sha, b"../../etc/passwd".to_vec());

        MockSymlinkBackend {
            files,
            blobs,
            branch: "main".to_string(),
        }
    }

    #[test]
    fn should_use_large_repo_path_threshold() {
        // None（size 探测失败）→ true（优先尝试大仓库后端，ZIP 兜底仍可用）
        assert!(should_use_large_repo_path(None));
        // 边界：正好 32MB → false
        assert!(!should_use_large_repo_path(Some(32 * 1024)));
        assert!(!should_use_large_repo_path(Some(0)));
        // 超过 32MB → true
        assert!(should_use_large_repo_path(Some(32 * 1024 + 1)));
        assert!(should_use_large_repo_path(Some(1024 * 1024)));
    }

    #[test]
    fn branch_candidates_order_and_dedup() {
        let repo = |branch: &str| SkillRepo {
            owner: "o".into(),
            name: "r".into(),
            branch: branch.into(),
            enabled: true,
        };
        // 配置分支 → main → master
        assert_eq!(
            branch_candidates(&repo("dev")),
            vec!["dev", "main", "master"]
        );
        // 配置分支就是 main：不重复
        assert_eq!(branch_candidates(&repo("main")), vec!["main", "master"]);
        // 配置分支就是 master：main 补在后面
        assert_eq!(branch_candidates(&repo("master")), vec!["master", "main"]);
        // 空配置 / HEAD 哨兵：跳过，改试 main / master
        assert_eq!(branch_candidates(&repo("")), vec!["main", "master"]);
        assert_eq!(branch_candidates(&repo("HEAD")), vec!["main", "master"]);
    }

    #[test]
    fn parse_ls_tree_output_parses_blobs_and_skips_trees() {
        let mut out = Vec::new();
        out.extend_from_slice(
            b"100644 blob 3b18e512dba79e4c8300dd08aeb37f8e728b8dad 15\tREADME.md",
        );
        out.push(0);
        out.extend_from_slice(b"040000 tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904 -\tdir");
        out.push(0);
        out.extend_from_slice(
            b"100755 blob 9daeafb9864cf43055ae93beb0afd6c7d144bfa4 4\tscripts/run.sh",
        );
        out.push(0);
        let files = parse_ls_tree_output(&out).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "README.md");
        assert_eq!(files[0].mode, 0o100644);
        assert_eq!(files[0].size, 15);
        assert_eq!(
            files[0].blob_sha,
            "3b18e512dba79e4c8300dd08aeb37f8e728b8dad"
        );
        assert_eq!(files[1].path, "scripts/run.sh");
        assert_eq!(files[1].mode, 0o100755);
        assert_eq!(files[1].size, 4);
    }

    #[test]
    fn parse_ls_tree_output_handles_non_utf8_paths() {
        let mut out = Vec::new();
        out.extend_from_slice(b"100644 blob 3b18e512dba79e4c8300dd08aeb37f8e728b8dad 3\t");
        out.extend_from_slice(&[0xFF, 0xFE, 0xFD]);
        out.push(0);
        let files = parse_ls_tree_output(&out).unwrap();
        assert_eq!(files.len(), 1);
        // lossy 转换，条目不能被丢弃
        assert!(files[0].path.contains('\u{FFFD}'));
    }

    #[test]
    fn parse_tree_api_response_parses_entries() {
        let json = r#"{
            "sha": "abc",
            "truncated": false,
            "tree": [
                {"path": "SKILL.md", "mode": "100644", "type": "blob", "sha": "aaa", "size": 10},
                {"path": "sub", "mode": "040000", "type": "tree", "sha": "bbb", "size": 0},
                {"path": "sub/SKILL.md", "mode": "100644", "type": "blob", "sha": "ccc", "size": 20}
            ]
        }"#;
        let (files, truncated) = parse_tree_api_response(json).unwrap();
        assert!(!truncated);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "SKILL.md");
        assert_eq!(files[0].mode, 0o100644);
        assert_eq!(files[0].size, 10);
        assert_eq!(files[1].path, "sub/SKILL.md");
        assert_eq!(files[1].blob_sha, "ccc");
    }

    #[test]
    fn parse_tree_api_response_reports_truncated() {
        let json = r#"{"truncated": true, "tree": [{"path": "a", "mode": "100644", "type": "blob", "sha": "s", "size": 1}]}"#;
        let (files, truncated) = parse_tree_api_response(json).unwrap();
        assert!(truncated);
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn parse_tree_api_response_rejects_malformed_json() {
        assert!(parse_tree_api_response("not json").is_err());
        assert!(parse_tree_api_response(r#"{"tree": "nope"}"#).is_err());
        assert!(parse_tree_api_response(r#"{}"#).is_err());
    }

    #[test]
    fn filter_skill_md_paths_matches_exact_suffix() {
        let files = vec![
            RepoFile {
                path: "SKILL.md".into(),
                blob_sha: "a".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "skills/foo/SKILL.md".into(),
                blob_sha: "b".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "skills/foo/README.md".into(),
                blob_sha: "c".into(),
                size: 1,
                mode: 0o100644,
            },
            // 大小写敏感：skill.md 不匹配
            RepoFile {
                path: "skills/foo/skill.md".into(),
                blob_sha: "d".into(),
                size: 1,
                mode: 0o100644,
            },
            // 非精确后缀：xSKILL.md 不匹配
            RepoFile {
                path: "xSKILL.md".into(),
                blob_sha: "e".into(),
                size: 1,
                mode: 0o100644,
            },
        ];
        let matched = filter_skill_md_paths(&files);
        let paths: Vec<&str> = matched.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["SKILL.md", "skills/foo/SKILL.md"]);
    }

    #[test]
    fn skill_dirs_from_tree_derives_parent_dirs_and_dedups() {
        let files = vec![
            RepoFile {
                path: "SKILL.md".into(),
                blob_sha: "a".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "skills/foo/SKILL.md".into(),
                blob_sha: "b".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "skills/foo/helper.py".into(),
                blob_sha: "c".into(),
                size: 1,
                mode: 0o100644,
            },
            // 同一目录下的另一个 SKILL.md 不应产生重复目录
            RepoFile {
                path: "skills/foo/docs/SKILL.md".into(),
                blob_sha: "d".into(),
                size: 1,
                mode: 0o100644,
            },
        ];
        let mut dirs = skill_dirs_from_tree(&files);
        dirs.sort();
        // 根级 SKILL.md → ""（仓库根）；嵌套 SKILL.md → 父目录
        assert_eq!(dirs, vec!["", "skills/foo", "skills/foo/docs"]);
    }

    #[test]
    fn match_install_name_finds_nested_dir_by_last_segment() {
        let dirs = vec![
            "skills/foo".to_string(),
            "skills/bar".to_string(),
            "".to_string(),
        ];
        // 安装名是落盘名（最后一段），仓库里是嵌套路径
        assert_eq!(match_install_name(&dirs, "foo", "repo"), Some("skills/foo"));
        // 大小写不敏感
        assert_eq!(match_install_name(&dirs, "FOO", "repo"), Some("skills/foo"));
        // 根级 skill 用仓库名作键
        assert_eq!(match_install_name(&dirs, "repo", "repo"), Some(""));
        // 未命中返回 None
        assert_eq!(match_install_name(&dirs, "nope", "repo"), None);
        // 空清单返回 None
        assert_eq!(match_install_name(&[], "foo", "repo"), None);
    }

    #[test]
    fn resolve_materialize_dir_treats_repo_name_sentinel_as_tree_root() {
        // 根级 skill 的 directory 是仓库名哨兵 → tree 根
        assert_eq!(resolve_materialize_dir("fixture-repo", "fixture-repo"), "");
        // 大小写不敏感
        assert_eq!(resolve_materialize_dir("FIXTURE-REPO", "fixture-repo"), "");
        // 嵌套目录原样返回
        assert_eq!(
            resolve_materialize_dir("skills/skill-a", "fixture-repo"),
            "skills/skill-a"
        );
        // 空目录（已是 tree 根）保持空
        assert_eq!(resolve_materialize_dir("", "fixture-repo"), "");
    }

    #[test]
    fn dir_blob_hash_known_input() {
        let files = vec![
            RepoFile {
                path: "d/b.txt".into(),
                blob_sha: "bbbb".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "d/a.txt".into(),
                blob_sha: "aaaa".into(),
                size: 1,
                mode: 0o100644,
            },
        ];
        let h = dir_blob_hash(&files, "d").unwrap();
        // 独立计算：SHA-256("a.txt\0aaaa\0b.txt\0bbbb\0")
        assert_eq!(
            h,
            "83b218eff0b62707a78e62d34465ecdf2ac7b6cb19b13c6f194c53ae4dd02b56"
        );
    }

    #[test]
    fn dir_blob_hash_sort_order_independent() {
        let files_a = vec![
            RepoFile {
                path: "d/b.txt".into(),
                blob_sha: "bbbb".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "d/a.txt".into(),
                blob_sha: "aaaa".into(),
                size: 1,
                mode: 0o100644,
            },
        ];
        let files_b = vec![
            RepoFile {
                path: "d/a.txt".into(),
                blob_sha: "aaaa".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "d/b.txt".into(),
                blob_sha: "bbbb".into(),
                size: 1,
                mode: 0o100644,
            },
        ];
        assert_eq!(dir_blob_hash(&files_a, "d"), dir_blob_hash(&files_b, "d"));
    }

    #[test]
    fn dir_blob_hash_none_for_missing_dir() {
        let files = vec![RepoFile {
            path: "d/a.txt".into(),
            blob_sha: "a".into(),
            size: 1,
            mode: 0o100644,
        }];
        assert!(dir_blob_hash(&files, "missing").is_none());
        // 目录存在但无文件（空清单）→ None
        assert!(dir_blob_hash(&[], "").is_none());
        // 根目录（""）下有文件 → Some
        assert!(dir_blob_hash(&files, "").is_some());
    }

    #[test]
    fn dir_blob_hash_case_insensitive_dir_match() {
        let files = vec![
            RepoFile {
                path: "Skills/Foo/SKILL.md".into(),
                blob_sha: "s".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "Skills/Foo/scripts/x.py".into(),
                blob_sha: "x".into(),
                size: 1,
                mode: 0o100644,
            },
        ];
        let h = dir_blob_hash(&files, "skills/foo").unwrap();
        // rel path 用真实路径：SHA-256("SKILL.md\0s\0scripts/x.py\0x\0")
        assert_eq!(
            h,
            "576f410eeb559e6107c7f1f2279e553301347c841beb5ab52134633a4bb811ab"
        );
    }

    #[test]
    fn dir_blob_hash_filters_submodules() {
        let files = vec![
            RepoFile {
                path: "d/a.txt".into(),
                blob_sha: "a".into(),
                size: 1,
                mode: 0o100644,
            },
            RepoFile {
                path: "d/sub".into(),
                blob_sha: "s".into(),
                size: 1,
                mode: 0o160000,
            },
        ];
        let h = dir_blob_hash(&files, "d").unwrap();
        // SHA-256("a.txt\0a\0")
        assert_eq!(
            h,
            "db57b2cfc22ddf1949369bc3220597ec48f5f1a3c5181925552aee84d2a45fac"
        );
    }

    #[test]
    fn git_blob_sha_matches_git_hash_object() {
        // SHA-1("blob 4\0test")，与 git hash-object 输出一致
        assert_eq!(
            git_blob_sha(b"test"),
            "30d74d258442c7c65512eafab474568dd706c430"
        );
        // 空 blob：SHA-1("blob 0\0")
        assert_eq!(
            git_blob_sha(b""),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
    }

    #[test]
    fn compute_local_blob_hash_matches_dir_blob_hash() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("b.txt"), b"hello").unwrap();
        fs::write(dir.path().join("sub/a.txt"), b"world").unwrap();
        // 隐藏文件也要算（与旧 compute_dir_hash 跳过点开头文件不同）
        fs::write(dir.path().join(".hidden"), b"secret").unwrap();

        let local = compute_local_blob_hash(dir.path(), BlobHashScheme::Sha1).unwrap();

        // 等价 RepoFile 列表（根目录 ""）
        let files = vec![
            RepoFile {
                path: ".hidden".into(),
                blob_sha: git_blob_sha(b"secret"),
                size: 6,
                mode: 0o100644,
            },
            RepoFile {
                path: "b.txt".into(),
                blob_sha: git_blob_sha(b"hello"),
                size: 5,
                mode: 0o100644,
            },
            RepoFile {
                path: "sub/a.txt".into(),
                blob_sha: git_blob_sha(b"world"),
                size: 5,
                mode: 0o100644,
            },
        ];
        let remote = dir_blob_hash(&files, "").unwrap();
        assert_eq!(local, remote);

        // SHA-256 方案：每个文件用 SHA-256("blob <size>\0<content>")
        let local256 = compute_local_blob_hash(dir.path(), BlobHashScheme::Sha256).unwrap();
        let files256 = vec![
            RepoFile {
                path: ".hidden".into(),
                blob_sha: git_blob_sha256(b"secret"),
                size: 6,
                mode: 0o100644,
            },
            RepoFile {
                path: "b.txt".into(),
                blob_sha: git_blob_sha256(b"hello"),
                size: 5,
                mode: 0o100644,
            },
            RepoFile {
                path: "sub/a.txt".into(),
                blob_sha: git_blob_sha256(b"world"),
                size: 5,
                mode: 0o100644,
            },
        ];
        assert_eq!(local256, dir_blob_hash(&files256, "").unwrap());
    }

    #[test]
    fn sanitize_tree_path_rejects_dangerous_paths() {
        assert!(sanitize_tree_path("a/b/c.txt").is_ok());
        assert!(sanitize_tree_path("a/../b.txt").is_err());
        assert!(sanitize_tree_path("..").is_err());
        assert!(sanitize_tree_path("a\\b.txt").is_err());
        assert!(sanitize_tree_path("/abs.txt").is_err());
        assert!(sanitize_tree_path("a/\u{0000}b").is_err());
        assert!(sanitize_tree_path("a#b.txt").is_err());
        assert!(sanitize_tree_path("a?b.txt").is_err());
        assert!(sanitize_tree_path("a%b.txt").is_err());
        assert!(sanitize_tree_path("").is_err());
    }

    #[test]
    fn hash_needs_recompute_bidirectional() {
        // 旧方案 → 新方案：需要重算
        assert!(hash_needs_recompute("deadbeef", BLOB_SHA1_PREFIX));
        assert!(hash_needs_recompute("deadbeef", BLOB_SHA256_PREFIX));
        // 新方案 → 旧方案（remote_scheme 为空）：需要重算（双向对齐）
        assert!(hash_needs_recompute("blob-sha1:abc", ""));
        assert!(hash_needs_recompute("blob-sha256:abc", ""));
        // 同方案：不需要
        assert!(!hash_needs_recompute("blob-sha1:abc", BLOB_SHA1_PREFIX));
        assert!(!hash_needs_recompute("blob-sha256:abc", BLOB_SHA256_PREFIX));
        // 旧方案 → 旧方案：不需要
        assert!(!hash_needs_recompute("deadbeef", ""));
    }

    #[test]
    fn git_blob_sha256_matches_sha256_object_format() {
        // SHA-256("blob 4\0test") 的已知值
        assert_eq!(
            git_blob_sha256(b"test"),
            "aa19560d465e7d43915547490a1f6b73eb55702e3d12cb82fb577df60bad4928"
        );
        // 空 blob：SHA-256("blob 0\0")
        assert_eq!(
            git_blob_sha256(b""),
            "473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813"
        );
    }

    #[test]
    fn build_discoverable_skill_parses_front_matter() {
        let content = "---\nname: My Skill\ndescription: Does things\n---\n# Body";
        let skill =
            build_discoverable_skill("owner", "repo", "main", "skills/my-skill/SKILL.md", content)
                .unwrap();
        assert_eq!(skill.key, "owner/repo:skills/my-skill");
        assert_eq!(skill.name, "My Skill");
        assert_eq!(skill.description, "Does things");
        assert_eq!(skill.directory, "skills/my-skill");
        assert_eq!(skill.repo_owner, "owner");
        assert_eq!(skill.repo_name, "repo");
        assert_eq!(skill.repo_branch, "main");
        assert_eq!(
            skill.readme_url.as_deref(),
            Some("https://github.com/owner/repo/blob/main/skills/my-skill/SKILL.md")
        );
    }

    #[test]
    fn build_discoverable_skill_falls_back_without_front_matter() {
        let content = "# Just a heading";
        let skill =
            build_discoverable_skill("owner", "repo", "main", "skills/my-skill/SKILL.md", content)
                .unwrap();
        // 无 front matter：name 回退为 directory，description 为空
        assert_eq!(skill.name, "skills/my-skill");
        assert_eq!(skill.description, "");
    }

    #[test]
    fn build_discoverable_skill_root_level_uses_repo_name() {
        let content = "---\nname: Root Skill\n---\n";
        let skill = build_discoverable_skill("owner", "repo", "main", "SKILL.md", content).unwrap();
        assert_eq!(skill.directory, "repo");
        assert_eq!(skill.key, "owner/repo:repo");
        assert_eq!(
            skill.readme_url.as_deref(),
            Some("https://github.com/owner/repo/blob/main/SKILL.md")
        );
    }

    #[test]
    fn build_discoverable_skill_rejects_dangerous_path() {
        // 路径含 ..：拒绝（保护 readme_url 落点）
        assert!(build_discoverable_skill("owner", "repo", "main", "../SKILL.md", "x").is_err());
    }

    // ========== 适配器测试（GitBackend / ApiBackend 一致性） ==========

    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    /// fixture 仓库坐标（必须通过 validate_repo_ref）
    const FIXTURE_OWNER: &str = "test-owner";
    const FIXTURE_NAME: &str = "fixture-repo";
    const FIXTURE_BRANCH: &str = "main";

    /// 写 fixture 文件树（根级 + skills/ + .hidden/）
    fn write_fixture(repo_dir: &Path) {
        fs::write(
            repo_dir.join("SKILL.md"),
            "---\nname: Root Skill\ndescription: Root level skill\n---\n# Root\n",
        )
        .unwrap();
        fs::create_dir_all(repo_dir.join("skills/skill-a")).unwrap();
        fs::write(
            repo_dir.join("skills/skill-a/SKILL.md"),
            "---\nname: Skill A\ndescription: Skill A description\n---\n# A\n",
        )
        .unwrap();
        fs::write(
            repo_dir.join("skills/skill-a/helper.py"),
            "print('hello')\n",
        )
        .unwrap();
        fs::create_dir_all(repo_dir.join("skills/skill-b/data")).unwrap();
        fs::write(
            repo_dir.join("skills/skill-b/SKILL.md"),
            "---\nname: Skill B\ndescription: Skill B description\n---\n# B\n",
        )
        .unwrap();
        fs::write(
            repo_dir.join("skills/skill-b/data/config.json"),
            "{\"key\": \"value\"}\n",
        )
        .unwrap();
        fs::create_dir_all(repo_dir.join(".hidden")).unwrap();
        fs::write(
            repo_dir.join(".hidden/SKILL.md"),
            "---\nname: Hidden Skill\ndescription: Hidden skill description\n---\n# Hidden\n",
        )
        .unwrap();
    }

    /// 执行 git 命令并断言成功
    fn run_git(git: &Path, cwd: &Path, args: &[&str]) {
        let output = Command::new(git)
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} 失败: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// 创建 fixture 仓库（git init + 写文件 + add + commit）。
    /// 返回 (root, repo_dir, owner, name, branch)。
    fn create_fixture_repo(git: &Path) -> (TempDir, PathBuf, String, String, String) {
        let root = tempdir().unwrap();
        let repo_dir = root
            .path()
            .join(FIXTURE_OWNER)
            .join(format!("{FIXTURE_NAME}.git"));
        fs::create_dir_all(&repo_dir).unwrap();
        write_fixture(&repo_dir);
        // git init（-b 需要 git ≥2.28；旧版回退 init + checkout -b）
        let init = Command::new(git)
            .args(["init", "-b", FIXTURE_BRANCH])
            .current_dir(&repo_dir)
            .output()
            .unwrap();
        if !init.status.success() {
            run_git(git, &repo_dir, &["init"]);
            let co = Command::new(git)
                .args(["checkout", "-b", FIXTURE_BRANCH])
                .current_dir(&repo_dir)
                .output()
                .unwrap();
            if !co.status.success() {
                // 默认分支已是 main（init.defaultBranch=main）
                run_git(git, &repo_dir, &["checkout", FIXTURE_BRANCH]);
            }
        }
        // -c core.autocrlf=false：避免 Windows 换行转换改变 blob 内容
        run_git(git, &repo_dir, &["-c", "core.autocrlf=false", "add", "-A"]);
        run_git(
            git,
            &repo_dir,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-m",
                "fixture",
            ],
        );
        (
            root,
            repo_dir,
            FIXTURE_OWNER.to_string(),
            FIXTURE_NAME.to_string(),
            FIXTURE_BRANCH.to_string(),
        )
    }

    /// 把本地路径转成 file:// URL（百分号编码空格等）
    fn file_url_for(path: &Path) -> String {
        let s = path.to_string_lossy().replace('\\', "/");
        let mut out = String::from("file://");
        for c in s.chars() {
            if c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '~') {
                out.push(c);
            } else {
                for b in c.to_string().as_bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
        out
    }

    /// 从真实 `git ls-tree -r -l -z HEAD` 输出生成 trees API JSON
    fn ls_tree_json(git: &Path, repo_dir: &Path) -> String {
        let output = Command::new(git)
            .arg("ls-tree")
            .arg("-r")
            .arg("-l")
            .arg("-z")
            .arg("HEAD")
            .current_dir(repo_dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "git ls-tree 失败");
        let files = parse_ls_tree_output(&output.stdout).unwrap();
        let tree: Vec<serde_json::Value> = files
            .iter()
            .map(|f| {
                serde_json::json!({
                    "path": f.path,
                    "mode": format!("{:o}", f.mode),
                    "type": "blob",
                    "sha": f.blob_sha,
                    "size": f.size,
                })
            })
            .collect();
        serde_json::json!({
            "sha": "fixture-tree",
            "truncated": false,
            "tree": tree,
        })
        .to_string()
    }

    /// 最小 HTTP mock server（std TcpListener，每连接一线程）
    struct MockServer {
        addr: SocketAddr,
        shutdown: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl MockServer {
        fn start(repo_root: &Path, tree_json: String) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_clone = shutdown.clone();
            let repo_root = repo_root.to_path_buf();
            let handle = thread::spawn(move || {
                while !shutdown_clone.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let repo_root = repo_root.clone();
                            let tree_json = tree_json.clone();
                            thread::spawn(move || {
                                let _ = handle_http(stream, &repo_root, &tree_json);
                            });
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                addr,
                shutdown,
                handle: Some(handle),
            }
        }

        fn api_base(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn raw_base(&self) -> String {
            format!("http://{}/raw", self.addr)
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// 处理单个 HTTP 连接：读请求头 → 路由 → 响应
    fn handle_http(
        mut stream: TcpStream,
        repo_root: &Path,
        tree_json: &str,
    ) -> std::io::Result<()> {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match stream.read(&mut tmp) {
                Ok(n) if n == 0 => return Ok(()),
                Ok(n) => {
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                    if buf.len() > 64 * 1024 {
                        return Ok(());
                    }
                }
                // Windows 上 accept 出的流会继承 listener 的非阻塞标记：请求数据
                // 尚未到达时 read 立即返回 WouldBlock，若直接退出连接会被静默丢弃
                // （并发跑多个 mock 测试时偶发，导致 API 后端回退到下一个分支）。
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(e) => return Err(e),
            }
        }
        let request = String::from_utf8_lossy(&buf);
        let request_line = request.lines().next().unwrap_or("");
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let target = parts.next().unwrap_or("");
        if method != "GET" {
            return Ok(());
        }
        let path = target.split('?').next().unwrap_or(target);
        let (status, body) = route(path, repo_root, tree_json);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes())?;
        stream.write_all(&body)?;
        stream.flush()?;
        Ok(())
    }

    /// 路由：trees API → tree JSON；raw API → 文件内容
    fn route(path: &str, repo_root: &Path, tree_json: &str) -> (String, Vec<u8>) {
        if let Some(rest) = path.strip_prefix("/repos/") {
            let segs: Vec<&str> = rest.split('/').collect();
            // {o}/{r}/git/trees/{branch...}
            if segs.len() >= 4 && segs[2] == "git" && segs[3] == "trees" {
                return ("200 OK".to_string(), tree_json.as_bytes().to_vec());
            }
        }
        if let Some(rest) = path.strip_prefix("/raw/") {
            // 归一化连续斜杠：url crate 的 path_segments_mut 在尾部斜杠后
            // 会产生空段（如 main//.hidden），GitHub 会归一化，mock 需自行处理
            let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
            // {o}/{r}/{branch}/{path...}
            if segs.len() >= 4 {
                let file_path = segs[3..].join("/");
                let full = repo_root.join(&file_path);
                if let Ok(bytes) = fs::read(&full) {
                    return ("200 OK".to_string(), bytes);
                }
                return ("404 Not Found".to_string(), b"not found".to_vec());
            }
        }
        ("404 Not Found".to_string(), b"not found".to_vec())
    }

    /// git 不可用时返回 None（测试直接通过，等价 skip）
    fn git_available() -> Option<PathBuf> {
        detect_git()
    }

    fn fixture_repo() -> SkillRepo {
        SkillRepo {
            owner: FIXTURE_OWNER.to_string(),
            name: FIXTURE_NAME.to_string(),
            branch: FIXTURE_BRANCH.to_string(),
            enabled: true,
        }
    }

    fn git_backend_for(git: &Path, root: &TempDir) -> GitBackend {
        GitBackend::with_git_and_base(git.to_path_buf(), file_url_for(root.path()))
    }

    /// 创建 fixture 仓库 + 启动 mock server
    fn fixture_with_mock(git: &Path) -> (TempDir, PathBuf, MockServer) {
        let (root, repo_dir, _o, _n, _b) = create_fixture_repo(git);
        let tree_json = ls_tree_json(git, &repo_dir);
        let server = MockServer::start(&repo_dir, tree_json);
        (root, repo_dir, server)
    }

    #[tokio::test]
    async fn git_backend_fetch_tree_matches_real_ls_tree() {
        let Some(git) = git_available() else { return };
        let (root, repo_dir, _o, _n, branch) = create_fixture_repo(&git);
        let backend = git_backend_for(&git, &root);
        let repo = fixture_repo();
        let (files, used_branch) = backend.fetch_tree(&repo).await.unwrap();
        assert_eq!(used_branch, branch);
        // 与真实 ls-tree 输出逐条一致（path / blob_sha / size / mode）
        let expected = parse_ls_tree_output(
            &Command::new(&git)
                .arg("ls-tree")
                .arg("-r")
                .arg("-l")
                .arg("-z")
                .arg("HEAD")
                .current_dir(&repo_dir)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(files, expected);
        // 关键文件断言
        let skill_a = files
            .iter()
            .find(|f| f.path == "skills/skill-a/SKILL.md")
            .unwrap();
        assert_eq!(skill_a.mode, 0o100644);
        let content = b"---\nname: Skill A\ndescription: Skill A description\n---\n# A\n";
        assert_eq!(skill_a.size, content.len() as u64);
        assert_eq!(skill_a.blob_sha, git_blob_sha(content));
        // 隐藏目录中的文件也在清单里
        assert!(files.iter().any(|f| f.path == ".hidden/SKILL.md"));
    }

    #[tokio::test]
    async fn git_backend_list_skill_mds_discovers_all_skills() {
        let Some(git) = git_available() else { return };
        let (root, _repo_dir, _o, _n, _b) = create_fixture_repo(&git);
        let backend = git_backend_for(&git, &root);
        let repo = fixture_repo();
        let skills = list_skill_mds(&backend, &repo).await.unwrap();
        let mut keys: Vec<String> = skills.iter().map(|s| s.key.clone()).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "test-owner/fixture-repo:.hidden",
                "test-owner/fixture-repo:fixture-repo",
                "test-owner/fixture-repo:skills/skill-a",
                "test-owner/fixture-repo:skills/skill-b",
            ]
        );
        let skill_a = skills
            .iter()
            .find(|s| s.directory == "skills/skill-a")
            .unwrap();
        assert_eq!(skill_a.name, "Skill A");
        assert_eq!(skill_a.description, "Skill A description");
        assert_eq!(
            skill_a.readme_url.as_deref(),
            Some("https://github.com/test-owner/fixture-repo/blob/main/skills/skill-a/SKILL.md")
        );
        let root_skill = skills
            .iter()
            .find(|s| s.directory == "fixture-repo")
            .unwrap();
        assert_eq!(root_skill.name, "Root Skill");
        assert_eq!(
            root_skill.readme_url.as_deref(),
            Some("https://github.com/test-owner/fixture-repo/blob/main/SKILL.md")
        );
    }

    #[tokio::test]
    async fn git_backend_skill_dir_hashes_are_blob_sha1_prefixed() {
        let Some(git) = git_available() else { return };
        let (root, repo_dir, _o, _n, _b) = create_fixture_repo(&git);
        let backend = git_backend_for(&git, &root);
        let repo = fixture_repo();
        // 安装名是落盘名（目录最后一段），仓库里实际是嵌套路径
        let install_names = vec![
            "skill-a".to_string(),
            "skill-b".to_string(),
            ".hidden".to_string(),
        ];
        let hashes = skill_dir_hashes(&backend, &repo, &install_names)
            .await
            .unwrap();
        assert_eq!(hashes.len(), 3);
        for (dir, hash) in &hashes {
            assert!(
                hash.starts_with(BLOB_SHA1_PREFIX),
                "{dir} 哈希缺少 blob-sha1: 前缀: {hash}"
            );
            // 返回的目录键是仓库相对路径（嵌套目录原样返回）
            assert!(dir.starts_with("skills/") || dir == ".hidden", "{dir}");
            // 与本地 compute_local_blob_hash 一致（同一 fixture 内容）
            let local = compute_local_blob_hash(&repo_dir.join(dir), BlobHashScheme::Sha1).unwrap();
            assert_eq!(hash, &format!("{BLOB_SHA1_PREFIX}{local}"));
        }
    }

    #[tokio::test]
    async fn git_backend_materialize_skill_dir_matches_fixture_bytes() {
        let Some(git) = git_available() else { return };
        let (root, _repo_dir, _o, _n, _b) = create_fixture_repo(&git);
        let backend = git_backend_for(&git, &root);
        let repo = fixture_repo();
        let (temp_dir, used_branch, scheme) =
            materialize_skill_dir(&backend, &repo, "skills/skill-a")
                .await
                .unwrap();
        assert_eq!(used_branch, FIXTURE_BRANCH);
        assert_eq!(scheme, BlobHashScheme::Sha1);
        assert_eq!(
            fs::read(temp_dir.path().join("SKILL.md")).unwrap(),
            b"---\nname: Skill A\ndescription: Skill A description\n---\n# A\n"
        );
        assert_eq!(
            fs::read(temp_dir.path().join("helper.py")).unwrap(),
            b"print('hello')\n"
        );
        // 目录外文件不物化
        assert!(!temp_dir.path().join("data").exists());
    }

    #[tokio::test]
    async fn git_backend_materialize_root_skill_materializes_whole_tree() {
        let Some(git) = git_available() else { return };
        let (root, _repo_dir, _o, _n, _b) = create_fixture_repo(&git);
        let backend = git_backend_for(&git, &root);
        let repo = fixture_repo();
        // 根级 skill 的 directory 是仓库名哨兵，应解析为 tree 根
        let (temp_dir, used_branch, scheme) = materialize_skill_dir(&backend, &repo, FIXTURE_NAME)
            .await
            .unwrap();
        assert_eq!(used_branch, FIXTURE_BRANCH);
        assert_eq!(scheme, BlobHashScheme::Sha1);
        // 仓库根与嵌套目录都物化（根级 skill 的范围是整仓）
        assert_eq!(
            fs::read(temp_dir.path().join("SKILL.md")).unwrap(),
            b"---\nname: Root Skill\ndescription: Root level skill\n---\n# Root\n"
        );
        assert_eq!(
            fs::read(temp_dir.path().join("skills/skill-a/SKILL.md")).unwrap(),
            b"---\nname: Skill A\ndescription: Skill A description\n---\n# A\n"
        );
        assert_eq!(
            fs::read(temp_dir.path().join("skills/skill-b/data/config.json")).unwrap(),
            b"{\"key\": \"value\"}\n"
        );
    }

    #[tokio::test]
    async fn git_backend_materialize_missing_dir_errors_instead_of_silent_empty() {
        let Some(git) = git_available() else { return };
        let (root, _repo_dir, _o, _n, _b) = create_fixture_repo(&git);
        let backend = git_backend_for(&git, &root);
        let repo = fixture_repo();
        // dir 未命中任何 tree 路径：必须显式报错，而不是静默返回空目录
        let err = materialize_skill_dir(&backend, &repo, "no/such/dir")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("SKILL.md"), "{err:#}");
    }

    #[tokio::test]
    async fn api_backend_fetch_tree_matches_real_ls_tree() {
        let Some(git) = git_available() else { return };
        let (_root, repo_dir, server) = fixture_with_mock(&git);
        let backend = ApiBackend::with_bases(&server.api_base(), &server.raw_base());
        let repo = fixture_repo();
        let (files, used_branch) = backend.fetch_tree(&repo).await.unwrap();
        assert_eq!(used_branch, FIXTURE_BRANCH);
        // 与真实 ls-tree 输出一致（同一 fixture，相同 blob SHA）
        let expected = parse_ls_tree_output(
            &Command::new(&git)
                .arg("ls-tree")
                .arg("-r")
                .arg("-l")
                .arg("-z")
                .arg("HEAD")
                .current_dir(&repo_dir)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(files, expected);
    }

    #[tokio::test]
    async fn api_backend_list_skill_mds_discovers_all_skills() {
        let Some(git) = git_available() else { return };
        let (_root, _repo_dir, server) = fixture_with_mock(&git);
        let backend = ApiBackend::with_bases(&server.api_base(), &server.raw_base());
        let repo = fixture_repo();
        let skills = list_skill_mds(&backend, &repo).await.unwrap();
        let mut keys: Vec<String> = skills.iter().map(|s| s.key.clone()).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "test-owner/fixture-repo:.hidden",
                "test-owner/fixture-repo:fixture-repo",
                "test-owner/fixture-repo:skills/skill-a",
                "test-owner/fixture-repo:skills/skill-b",
            ]
        );
        let skill_a = skills
            .iter()
            .find(|s| s.directory == "skills/skill-a")
            .unwrap();
        assert_eq!(skill_a.name, "Skill A");
        assert_eq!(skill_a.description, "Skill A description");
        assert_eq!(
            skill_a.readme_url.as_deref(),
            Some("https://github.com/test-owner/fixture-repo/blob/main/skills/skill-a/SKILL.md")
        );
    }

    #[tokio::test]
    async fn api_backend_skill_dir_hashes_are_blob_sha1_prefixed() {
        let Some(git) = git_available() else { return };
        let (_root, repo_dir, server) = fixture_with_mock(&git);
        let backend = ApiBackend::with_bases(&server.api_base(), &server.raw_base());
        let repo = fixture_repo();
        // 安装名是落盘名（目录最后一段），仓库里实际是嵌套路径
        let install_names = vec![
            "skill-a".to_string(),
            "skill-b".to_string(),
            ".hidden".to_string(),
        ];
        let hashes = skill_dir_hashes(&backend, &repo, &install_names)
            .await
            .unwrap();
        assert_eq!(hashes.len(), 3);
        for (dir, hash) in &hashes {
            assert!(
                hash.starts_with(BLOB_SHA1_PREFIX),
                "{dir} 哈希缺少 blob-sha1: 前缀: {hash}"
            );
            // 返回的目录键是仓库相对路径（嵌套目录原样返回）
            assert!(dir.starts_with("skills/") || dir == ".hidden", "{dir}");
            let local = compute_local_blob_hash(&repo_dir.join(dir), BlobHashScheme::Sha1).unwrap();
            assert_eq!(hash, &format!("{BLOB_SHA1_PREFIX}{local}"));
        }
    }

    #[tokio::test]
    async fn api_backend_materialize_skill_dir_matches_fixture_bytes() {
        let Some(git) = git_available() else { return };
        let (_root, repo_dir, server) = fixture_with_mock(&git);
        let backend = ApiBackend::with_bases(&server.api_base(), &server.raw_base());
        let repo = fixture_repo();
        let (temp_dir, used_branch, scheme) =
            materialize_skill_dir(&backend, &repo, "skills/skill-a")
                .await
                .unwrap();
        assert_eq!(used_branch, FIXTURE_BRANCH);
        assert_eq!(scheme, BlobHashScheme::Sha1);
        assert_eq!(
            fs::read(temp_dir.path().join("SKILL.md")).unwrap(),
            b"---\nname: Skill A\ndescription: Skill A description\n---\n# A\n"
        );
        assert_eq!(
            fs::read(temp_dir.path().join("helper.py")).unwrap(),
            b"print('hello')\n"
        );
    }

    #[tokio::test]
    async fn api_backend_materialize_root_skill_materializes_whole_tree() {
        let Some(git) = git_available() else { return };
        let (_root, _repo_dir, server) = fixture_with_mock(&git);
        let backend = ApiBackend::with_bases(&server.api_base(), &server.raw_base());
        let repo = fixture_repo();
        // 根级 skill 的 directory 是仓库名哨兵，应解析为 tree 根
        let (temp_dir, used_branch, scheme) = materialize_skill_dir(&backend, &repo, FIXTURE_NAME)
            .await
            .unwrap();
        assert_eq!(used_branch, FIXTURE_BRANCH);
        assert_eq!(scheme, BlobHashScheme::Sha1);
        assert_eq!(
            fs::read(temp_dir.path().join("SKILL.md")).unwrap(),
            b"---\nname: Root Skill\ndescription: Root level skill\n---\n# Root\n"
        );
        assert_eq!(
            fs::read(temp_dir.path().join("skills/skill-a/SKILL.md")).unwrap(),
            b"---\nname: Skill A\ndescription: Skill A description\n---\n# A\n"
        );
    }

    /// 核心一致性：同一 fixture 下两个后端产出完全相同的结果
    #[tokio::test]
    async fn git_and_api_backends_produce_identical_results() {
        let Some(git) = git_available() else { return };
        let (root, repo_dir, _o, _n, _b) = create_fixture_repo(&git);
        let git_backend = git_backend_for(&git, &root);
        let server = MockServer::start(&repo_dir, ls_tree_json(&git, &repo_dir));
        let api_backend = ApiBackend::with_bases(&server.api_base(), &server.raw_base());
        let repo = fixture_repo();

        // 1. fetch_tree 文件清单一致
        let (git_files, git_branch) = git_backend.fetch_tree(&repo).await.unwrap();
        let (api_files, api_branch) = api_backend.fetch_tree(&repo).await.unwrap();
        assert_eq!(git_branch, api_branch);
        assert_eq!(git_files, api_files);

        // 2. list_skill_mds 一致（按 key 排序后逐字段比较）
        let git_skills = list_skill_mds(&git_backend, &repo).await.unwrap();
        let api_skills = list_skill_mds(&api_backend, &repo).await.unwrap();
        let mut git_sorted = git_skills.clone();
        let mut api_sorted = api_skills.clone();
        git_sorted.sort_by(|a, b| a.key.cmp(&b.key));
        api_sorted.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(git_sorted.len(), api_sorted.len());
        for (g, a) in git_sorted.iter().zip(api_sorted.iter()) {
            assert_eq!(g.key, a.key);
            assert_eq!(g.name, a.name);
            assert_eq!(g.description, a.description);
            assert_eq!(g.directory, a.directory);
            assert_eq!(g.readme_url, a.readme_url);
            assert_eq!(g.repo_owner, a.repo_owner);
            assert_eq!(g.repo_name, a.repo_name);
            assert_eq!(g.repo_branch, a.repo_branch);
        }

        // 3. skill_dir_hashes 一致（安装名是落盘名，仓库里是嵌套路径）
        let install_names = vec![
            "skill-a".to_string(),
            "skill-b".to_string(),
            ".hidden".to_string(),
        ];
        let git_hashes = skill_dir_hashes(&git_backend, &repo, &install_names)
            .await
            .unwrap();
        let api_hashes = skill_dir_hashes(&api_backend, &repo, &install_names)
            .await
            .unwrap();
        assert_eq!(git_hashes, api_hashes);

        // 4. materialize_skill_dir 内容一致（逐字节）
        let (git_temp, git_branch2, git_scheme) =
            materialize_skill_dir(&git_backend, &repo, "skills/skill-b")
                .await
                .unwrap();
        let (api_temp, api_branch2, api_scheme) =
            materialize_skill_dir(&api_backend, &repo, "skills/skill-b")
                .await
                .unwrap();
        assert_eq!(git_branch2, api_branch2);
        assert_eq!(git_scheme, api_scheme);
        assert_eq!(
            fs::read(git_temp.path().join("SKILL.md")).unwrap(),
            fs::read(api_temp.path().join("SKILL.md")).unwrap()
        );
        assert_eq!(
            fs::read(git_temp.path().join("data/config.json")).unwrap(),
            fs::read(api_temp.path().join("data/config.json")).unwrap()
        );
    }

    /// symlink 指向文件：物化后 link 位置应是目标文件内容副本，而非路径文本。
    #[tokio::test]
    async fn materialize_resolves_symlink_to_file_content() {
        let backend = mock_symlink_backend();
        let repo = SkillRepo {
            owner: "o".to_string(),
            name: "r".to_string(),
            branch: "main".to_string(),
            enabled: true,
        };
        // 根级 skill（directory == 仓库名哨兵 → tree 根）
        let (temp_dir, _branch, _scheme) =
            materialize_skill_dir(&backend, &repo, "r").await.unwrap();

        // 普通文件照常物化
        assert_eq!(
            fs::read(temp_dir.path().join("SKILL.md")).unwrap(),
            b"---\nname: t\n---\n"
        );
        assert_eq!(
            fs::read(temp_dir.path().join("target.txt")).unwrap(),
            b"content"
        );

        // symlink 物化为目标内容副本（关键断言：不是路径文本 "target.txt"）
        assert_eq!(
            fs::read(temp_dir.path().join("link-to-file")).unwrap(),
            b"content",
            "指向文件的 symlink 应物化为目标文件内容，而非路径文本"
        );
    }

    /// symlink 指向目录：物化后 link 位置应是目标目录的递归副本。
    #[tokio::test]
    async fn materialize_resolves_symlink_to_dir_content() {
        let backend = mock_symlink_backend();
        let repo = SkillRepo {
            owner: "o".to_string(),
            name: "r".to_string(),
            branch: "main".to_string(),
            enabled: true,
        };
        let (temp_dir, _branch, _scheme) =
            materialize_skill_dir(&backend, &repo, "r").await.unwrap();

        // symlink → shared 目录：link-to-dir/nested.txt 应为目录副本
        assert!(
            temp_dir.path().join("link-to-dir").is_dir(),
            "指向目录的 symlink 应物化为目录副本"
        );
        assert_eq!(
            fs::read(temp_dir.path().join("link-to-dir/nested.txt")).unwrap(),
            b"nested\n",
            "目录 symlink 的子文件应为目标内容副本"
        );
    }

    /// self-containing symlink（target == "."）与越界 symlink（target 含 ../../）
    /// 应被跳过，不物化、不报错（非致命语义，对齐 skill.rs 的 ZIP 路径）。
    #[tokio::test]
    async fn materialize_skips_self_containing_and_escaping_symlinks() {
        let backend = mock_symlink_backend();
        let repo = SkillRepo {
            owner: "o".to_string(),
            name: "r".to_string(),
            branch: "main".to_string(),
            enabled: true,
        };
        let (temp_dir, _branch, _scheme) =
            materialize_skill_dir(&backend, &repo, "r").await.unwrap();

        // self-containing 与越界 symlink 不应物化（文件不存在）
        assert!(
            !temp_dir.path().join("link-self").exists(),
            "self-containing symlink 应被跳过"
        );
        assert!(
            !temp_dir.path().join("link-escape").exists(),
            "越界 symlink 应被跳过"
        );
        // 其他合法条目仍正常物化
        assert!(temp_dir.path().join("SKILL.md").is_file());
        assert!(temp_dir.path().join("link-to-file").is_file());
    }

    /// ls-remote 预检：正确解析本地 fixture 仓库的分支列表。
    #[tokio::test]
    async fn ls_remote_branches_lists_fixture_heads() {
        let Some(git) = git_available() else { return };
        let (root, _repo_dir, _o, _n, branch) = create_fixture_repo(&git);
        let base_url = file_url_for(root.path());
        let repo = fixture_repo();
        let branches = ls_remote_branches(&git, &base_url, &repo).await.unwrap();
        assert!(
            branches.iter().any(|b| b == &branch),
            "ls-remote 应列出 fixture 分支 {branch}，实际: {branches:?}"
        );
    }

    /// 快速失败：候选分支在远端全都不存在时，fetch_tree 应通过 ls-remote 预检直接失败，
    /// 而不对每个候选分支逐一触发完整 clone。构造一个仅含 weird-branch、无 main/master
    /// 的仓库，并请求一个不存在的分支。
    #[tokio::test]
    async fn fetch_tree_quick_fails_when_no_candidate_branch_exists() {
        let Some(git) = git_available() else { return };
        let root = tempdir().unwrap();
        let repo_dir = root
            .path()
            .join("qowner")
            .join("qrepo.git");
        fs::create_dir_all(&repo_dir).unwrap();
        write_fixture(&repo_dir);
        run_git(&git, &repo_dir, &["init", "-b", "weird-branch"]);
        run_git(&git, &repo_dir, &["add", "-A"]);
        run_git(&git, &repo_dir, &["commit", "-m", "init"]);

        let backend = git_backend_for(&git, &root);
        // 请求一个不存在的分支；候选仅 ["missing-branch"]（不含 main/master，
        // 因 weird-branch 已作为配置分支占用，main/master 不会被追加；且 weird-branch
        // 与 missing-branch 不匹配，故 ls-remote 预检交集为空 → 快速失败）。
        let repo = SkillRepo {
            owner: "qowner".to_string(),
            name: "qrepo".to_string(),
            branch: "missing-branch".to_string(),
            enabled: true,
        };
        let err = backend.fetch_tree(&repo).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("远端不存在候选分支"),
            "应快速失败提示候选分支不存在，实际: {msg}"
        );
    }

    /// 预检不阻断正常路径：配置分支存在于远端时仍应 clone 成功（退化为逐个 clone 的
    /// 场景之外，ls-remote 命中后应继续正常 clone）。
    #[tokio::test]
    async fn fetch_tree_succeeds_when_config_branch_exists() {
        let Some(git) = git_available() else { return };
        let (root, _repo_dir, _o, _n, _b) = create_fixture_repo(&git);
        let backend = git_backend_for(&git, &root);
        let repo = fixture_repo(); // branch = main，fixture 含 main
        let (files, used_branch) = backend.fetch_tree(&repo).await.unwrap();
        assert_eq!(used_branch, FIXTURE_BRANCH);
        assert!(!files.is_empty());
    }
}

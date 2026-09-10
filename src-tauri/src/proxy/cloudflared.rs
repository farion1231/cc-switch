//! Cloudflare 隧道管理（Quick Tunnel + 命名隧道）
//!
//! Cursor 的 公网路由 拒绝 localhost / 私有网络地址（"Access to private networks
//! is forbidden"），所以本地代理必须经公网隧道暴露给 Cursor。这里管理
//! cloudflared 子进程：按需下载二进制、启动、停止与清理。
//!
//! 两种模式：
//! - 快速隧道（免账号）：`cloudflared tunnel --url <local>`，从 stderr 解析
//!   临时 trycloudflare.com 地址，URL 每次重建都会变；
//! - 命名隧道（需 Cloudflare 账户）：`cloudflared tunnel run <name>`，固定
//!   域名，Cursor 里只需粘贴一次。凭证与 DNS 由用户一次性配置
//!   （cloudflared login / tunnel create / route dns），cc-switch 只写一份
//!   专属 ingress 配置（不污染用户 ~/.cloudflared/config.yml）并启动进程。

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;

/// 隧道模式
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelMode {
    /// 免账号快速隧道：URL 每次重建都变
    Quick,
    /// Cloudflare 账户命名隧道：固定域名（tunnel 为隧道名或 UUID）
    Named { tunnel: String, hostname: String },
}

/// ~/.cloudflared 目录（cloudflared login / tunnel create 的产物位置）
fn cloudflared_home() -> PathBuf {
    crate::config::get_home_dir().join(".cloudflared")
}

/// cloudflared login 生成的账户凭证
fn cert_pem_path() -> PathBuf {
    cloudflared_home().join("cert.pem")
}

/// cc-switch 专属命名隧道配置文件（不覆盖用户自己的 config.yml）
fn named_config_path() -> PathBuf {
    crate::config::get_app_config_dir().join("cloudflared-named.yml")
}

/// 渲染命名隧道 ingress 配置：仅暴露指定 hostname，其余 404。
fn render_named_config(
    tunnel_id: &str,
    credentials: &std::path::Path,
    hostname: &str,
    local_url: &str,
) -> String {
    format!(
        "tunnel: {tunnel_id}\ncredentials-file: {}\ningress:\n  - hostname: {hostname}\n    service: {local_url}\n  - service: http_status:404\n",
        credentials.display()
    )
}

/// 命名隧道信息（cloudflared tunnel list）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedTunnel {
    pub id: String,
    pub name: String,
}

/// 解析 `cloudflared tunnel list --output json` 输出为隧道列表。
fn parse_tunnel_list(list_json: &str) -> Vec<NamedTunnel> {
    let Ok(entries) = serde_json::from_str::<serde_json::Value>(list_json) else {
        return Vec::new();
    };
    entries
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let id = e.get("id")?.as_str()?.to_string();
                    let name = e.get("name")?.as_str()?.to_string();
                    Some(NamedTunnel { id, name })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 从 `cloudflared tunnel list --output json` 输出解析隧道 ID（按 name 或 id 匹配）。
fn parse_tunnel_id(list_json: &str, name_or_id: &str) -> Option<String> {
    let entries: serde_json::Value = serde_json::from_str(list_json).ok()?;
    for entry in entries.as_array()? {
        let id = entry.get("id")?.as_str()?;
        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name == name_or_id || id == name_or_id {
            return Some(id.to_string());
        }
    }
    None
}

/// 命名隧道就绪信号：stderr 出现首个 "Registered tunnel connection"。
/// （"Unregistered" 为小写 r，不会误匹配。）
fn is_connection_registered_line(line: &str) -> bool {
    line.contains("Registered tunnel connection")
}

/// 隧道运行状态
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct TunnelStatus {
    pub running: bool,
    /// 公网地址（含 https://，不含路径）
    pub public_url: Option<String>,
    /// 本地转发目标（含 http:// 与端口）
    pub local_url: Option<String>,
    /// 最近一次错误（若有）
    pub last_error: Option<String>,
}

/// 全局隧道管理器（单进程一个 cloudflared 即可，转发到本地代理端口）
#[derive(Clone)]
pub struct CloudflaredManager {
    state: Arc<RwLock<TunnelStatus>>,
    child: Arc<RwLock<Option<Child>>>,
    /// 生命周期串行锁：start/stop 互斥，防止并发 start 的 TOCTOU（#4 review）。
    lifecycle: Arc<tokio::sync::Mutex<()>>,
    /// 隧道代际：每次 start 递增；stderr 解析器按代际判断是否仍属当前隧道，
    /// 防止被杀隧道的残留 stderr 把死 URL 写进新隧道状态（#4 review）。
    generation: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for CloudflaredManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudflaredManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(TunnelStatus::default())),
            child: Arc::new(RwLock::new(None)),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// cloudflared 二进制存放目录（~/.cc-switch/bin）
    fn bin_dir() -> PathBuf {
        crate::config::get_app_config_dir().join("bin")
    }

    fn bin_path() -> PathBuf {
        Self::bin_dir().join(if cfg!(windows) {
            "cloudflared.exe"
        } else {
            "cloudflared"
        })
    }

    /// 确保 cloudflared 可用：PATH/常见目录与本地缓存统一做有效性校验，最后下载
    async fn ensure_binary(&self) -> Result<PathBuf, String> {
        let mut dirs = search_dirs();
        dirs.push(Self::bin_dir());
        if let Some(p) = find_valid_in_dirs(&dirs).await {
            return Ok(p);
        }
        // 本地缓存存在但损坏（如历史 bug 把 GitHub 404 页面写成了二进制）→ 删除重下
        let local = Self::bin_path();
        if local.exists() {
            log::warn!("[Tunnel] 本地 cloudflared 已损坏，删除后重新下载");
            let _ = std::fs::remove_file(&local);
        }
        self.download().await
    }

    /// 从 GitHub releases 下载对应平台的 cloudflared
    async fn download(&self) -> Result<PathBuf, String> {
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "linux"
        };
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "amd64"
        };
        let name = asset_name(os, arch);
        let url =
            format!("https://github.com/cloudflare/cloudflared/releases/latest/download/{name}");

        let dir = Self::bin_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建 bin 目录失败: {e}"))?;
        let target = Self::bin_path();

        log::info!("[Tunnel] 下载 cloudflared: {url}");
        let bytes = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| format!("构建下载客户端失败: {e}"))?
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("下载 cloudflared 失败: {e}"))?
            // 非 2xx（如 404）直接报错，避免把错误页面写成"二进制"
            .error_for_status()
            .map_err(|e| format!("下载 cloudflared 失败: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("读取 cloudflared 响应失败: {e}"))?;

        if name.ends_with(".tgz") {
            extract_cloudflared_tgz(&bytes, &target)?;
        } else {
            std::fs::write(&target, &bytes).map_err(|e| format!("写入 cloudflared 失败: {e}"))?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&target)
                .map_err(|e| format!("读取权限失败: {e}"))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&target, perms)
                .map_err(|e| format!("设置可执行权限失败: {e}"))?;
        }

        // 下载后验证：无法执行则删除报错，避免坏文件被反复使用
        if !validate_binary(&target).await {
            let _ = std::fs::remove_file(&target);
            return Err("下载的 cloudflared 无法执行，已删除".to_string());
        }

        log::info!("[Tunnel] cloudflared 已下载到: {}", target.display());
        Ok(target)
    }

    pub async fn status(&self) -> TunnelStatus {
        self.state.read().await.clone()
    }

    /// 启动隧道，把本地 `local_url`（如 http://127.0.0.1:15721）暴露为公网地址。
    /// 返回解析到的公网地址。
    ///
    /// 生命周期串行（start/stop 互斥），并对已崩溃的旧进程自愈重建（#4 review）。
    pub async fn start(&self, local_url: &str, mode: &TunnelMode) -> Result<TunnelStatus, String> {
        let _guard = self.lifecycle.lock().await;

        // 自愈：已有 child 但已退出（崩溃）→ 回收后重新启动；仍在运行则幂等返回。
        {
            let mut guard = self.child.write().await;
            if !tunnel_child_needs_restart(guard.as_mut()) {
                return Ok(self.status().await);
            }
            if guard.take().is_some() {
                log::warn!("[Tunnel] 检测到 cloudflared 已退出，清理后重新启动");
            }
        }

        let current_gen = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        match mode {
            TunnelMode::Quick => self.start_quick(local_url, current_gen).await,
            TunnelMode::Named { tunnel, hostname } => {
                self.start_named(local_url, tunnel, hostname, current_gen)
                    .await
            }
        }
    }

    /// 快速隧道（免账号）：URL 从 stderr 解析，每次重建都变。
    async fn start_quick(&self, local_url: &str, current_gen: u64) -> Result<TunnelStatus, String> {
        let bin = self.ensure_binary().await?;

        // cloudflared 需要 host:port 形式；剥离 scheme
        let local = local_url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();

        // 清理指向同一本地地址的残留 cloudflared（App 异常退出时 kill_on_drop
        // 不会触发，孤儿进程会让旧 trycloudflare URL 继续存活，造成混淆）。
        Self::kill_stale_tunnels(&local).await;

        log::info!("[Tunnel] 启动 cloudflared 隧道 -> {local}");
        let mut child = Command::new(&bin)
            .args([
                "tunnel",
                "--url",
                &format!("http://{local}"),
                "--no-autoupdate",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("启动 cloudflared 失败: {e}"))?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "无法捕获 cloudflared stderr".to_string())?;

        // 更新状态为启动中
        {
            let mut st = self.state.write().await;
            st.running = false;
            st.local_url = Some(format!("http://{local}"));
            st.public_url = None;
            st.last_error = None;
        }

        // 解析 stderr 中的 trycloudflare.com URL（cloudflared 把日志写到 stderr）
        let state = self.state.clone();
        let found_url = Arc::new(RwLock::new(None::<String>));
        let found_url_writer = found_url.clone();
        let generation = self.generation.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(url) = extract_trycloudflare_url(&line) {
                    // 代际守卫：仅当仍是本代隧道时才写状态，防止被杀隧道的残留
                    // stderr 把死 URL 写进新隧道（#4 review TOCTOU）。
                    if generation.load(std::sync::atomic::Ordering::SeqCst) != current_gen {
                        return;
                    }
                    let mut w = found_url_writer.write().await;
                    if w.is_none() {
                        *w = Some(url.clone());
                        let mut st = state.write().await;
                        st.running = true;
                        st.public_url = Some(url.clone());
                        log::info!("[Tunnel] 公网地址就绪: {url}");
                    }
                }
            }
        });

        // 等待 URL 就绪（最多 ~25s）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
        loop {
            if let Some(url) = found_url.read().await.clone() {
                let mut st = self.state.write().await;
                st.running = true;
                st.public_url = Some(url.clone());
                *self.child.write().await = Some(child);
                return Ok(st.clone());
            }
            if std::time::Instant::now() > deadline {
                // 超时：杀掉子进程，递增代际使残留 stderr 解析器失效，记录错误
                self.generation
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = child.kill().await;
                let mut st = self.state.write().await;
                st.running = false;
                st.public_url = None;
                st.last_error = Some("cloudflared 启动超时，未获取到公网地址".to_string());
                return Err(st.last_error.clone().unwrap());
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }

    /// 命名隧道（需 Cloudflare 账户）：固定域名，就绪信号为
    /// "Registered tunnel connection"。凭证/隧道/DNS 由用户一次性配置。
    async fn start_named(
        &self,
        local_url: &str,
        tunnel: &str,
        hostname: &str,
        current_gen: u64,
    ) -> Result<TunnelStatus, String> {
        let bin = self.ensure_binary().await?;

        if !cert_pem_path().exists() {
            return Err(
                "未找到 cloudflared 登录凭证（~/.cloudflared/cert.pem），请先在终端运行：cloudflared login"
                    .to_string(),
            );
        }

        // cloudflared 需要 host:port 形式；剥离 scheme
        let local = local_url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();

        // 隧道名 → UUID（也接受直接填 UUID）
        let output = Command::new(&bin)
            .args(["tunnel", "list", "--output", "json"])
            .output()
            .await
            .map_err(|e| format!("查询隧道列表失败: {e}"))?;
        let list = String::from_utf8_lossy(&output.stdout);
        let tunnel_id = parse_tunnel_id(&list, tunnel).ok_or_else(|| {
            format!("未找到隧道「{tunnel}」，请先在终端运行：cloudflared tunnel create {tunnel}")
        })?;

        let creds = cloudflared_home().join(format!("{tunnel_id}.json"));
        if !creds.exists() {
            return Err(format!(
                "缺少隧道凭证文件 {}，请重新运行 cloudflared tunnel create {tunnel}",
                creds.display()
            ));
        }

        // DNS 路由：已存在时 cloudflared 会报错，但路由本身有效，忽略失败
        match Command::new(&bin)
            .args(["tunnel", "route", "dns", tunnel, hostname])
            .output()
            .await
        {
            Ok(o) if o.status.success() => {
                log::info!("[Tunnel] 已为 {hostname} 配置 DNS 路由")
            }
            Ok(o) => log::info!(
                "[Tunnel] DNS 路由配置返回非零（通常表示已存在，忽略）: {}",
                String::from_utf8_lossy(&o.stderr)
                    .lines()
                    .next()
                    .unwrap_or("")
            ),
            Err(e) => log::warn!("[Tunnel] 配置 DNS 路由失败（忽略）: {e}"),
        }

        // 写 cc-switch 专属 ingress 配置
        let cfg_path = named_config_path();
        if let Some(parent) = cfg_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(
            &cfg_path,
            render_named_config(&tunnel_id, &creds, hostname, &format!("http://{local}")),
        )
        .map_err(|e| format!("写入隧道配置失败: {e}"))?;

        // 清理残留（按专属配置路径匹配，不误伤用户其它隧道）
        Self::kill_stale_named(&cfg_path).await;

        log::info!("[Tunnel] 启动命名隧道 {tunnel}（{hostname}）-> {local}");
        let mut child = Command::new(&bin)
            .args(["tunnel", "--no-autoupdate", "--config"])
            .arg(&cfg_path)
            .arg("run")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("启动 cloudflared 失败: {e}"))?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "无法捕获 cloudflared stderr".to_string())?;

        {
            let mut st = self.state.write().await;
            st.running = false;
            st.local_url = Some(format!("http://{local}"));
            st.public_url = None;
            st.last_error = None;
        }

        let public_url = format!(
            "https://{}",
            hostname
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
        );

        let state = self.state.clone();
        let registered = Arc::new(RwLock::new(false));
        let registered_writer = registered.clone();
        let ready_url = public_url.clone();
        let generation = self.generation.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if is_connection_registered_line(&line) {
                    // 代际守卫：仅当仍是本代隧道时才写状态（#4 review TOCTOU）。
                    if generation.load(std::sync::atomic::Ordering::SeqCst) != current_gen {
                        return;
                    }
                    let mut w = registered_writer.write().await;
                    if !*w {
                        *w = true;
                        let mut st = state.write().await;
                        st.running = true;
                        st.public_url = Some(ready_url.clone());
                        log::info!("[Tunnel] 命名隧道已连接: {ready_url}");
                    }
                }
            }
        });

        // 等待首个连接注册（最多 ~25s）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
        loop {
            if *registered.read().await {
                let mut st = self.state.write().await;
                st.running = true;
                st.public_url = Some(public_url.clone());
                *self.child.write().await = Some(child);
                return Ok(st.clone());
            }
            if std::time::Instant::now() > deadline {
                // 递增代际使残留 stderr 解析器失效，再杀进程
                self.generation
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = child.kill().await;
                let mut st = self.state.write().await;
                st.running = false;
                st.public_url = None;
                st.last_error = Some("命名隧道启动超时，请检查隧道名、域名与 DNS 配置".to_string());
                return Err(st.last_error.clone().unwrap());
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }

    /// 停止隧道
    pub async fn stop(&self) -> Result<(), String> {
        let _guard = self.lifecycle.lock().await;
        if let Some(mut child) = self.child.write().await.take() {
            let _ = child.kill().await;
            log::info!("[Tunnel] cloudflared 隧道已停止");
        }
        // 递增代际：停止后任何残留 stderr 解析器都不再写状态
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut st = self.state.write().await;
        st.running = false;
        st.public_url = None;
        Ok(())
    }

    /// 列出 Cloudflare 账户下已有的命名隧道（需已 cloudflared login）。
    pub async fn list_named_tunnels(&self) -> Result<Vec<NamedTunnel>, String> {
        if !cert_pem_path().exists() {
            return Err(
                "未找到 cloudflared 登录凭证（~/.cloudflared/cert.pem），请先在终端运行：cloudflared login"
                    .to_string(),
            );
        }
        let bin = self.ensure_binary().await?;
        let output = Command::new(&bin)
            .args(["tunnel", "list", "--output", "json"])
            .output()
            .await
            .map_err(|e| format!("查询隧道列表失败: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "查询隧道列表失败: {}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("未知错误")
            ));
        }
        Ok(parse_tunnel_list(&String::from_utf8_lossy(&output.stdout)))
    }

    /// 清理指向同一本地地址的残留 cloudflared 进程（仅 unix；Windows 上无 pkill，
    /// 孤儿进程概率也低，静默跳过）。
    async fn kill_stale_tunnels(local: &str) {
        #[cfg(unix)]
        {
            let pattern = format!("cloudflared tunnel --url http://{local}");
            match tokio::process::Command::new("pkill")
                .args(["-f", &pattern])
                .status()
                .await
            {
                Ok(status) if status.success() => {
                    log::info!("[Tunnel] 已清理指向 {local} 的残留 cloudflared 进程")
                }
                Ok(_) => {} // 无匹配进程
                Err(e) => log::warn!("[Tunnel] 清理残留 cloudflared 失败（忽略）: {e}"),
            }
        }
        #[cfg(not(unix))]
        let _ = local;
    }

    /// 清理使用 cc-switch 专属配置的残留命名隧道进程（仅 unix）。
    async fn kill_stale_named(config_path: &std::path::Path) {
        #[cfg(unix)]
        {
            let pattern = format!(
                "cloudflared tunnel --no-autoupdate --config {}",
                config_path.display()
            );
            match tokio::process::Command::new("pkill")
                .args(["-f", &pattern])
                .status()
                .await
            {
                Ok(status) if status.success() => {
                    log::info!("[Tunnel] 已清理残留的命名隧道进程")
                }
                Ok(_) => {}
                Err(e) => log::warn!("[Tunnel] 清理残留命名隧道失败（忽略）: {e}"),
            }
        }
        #[cfg(not(unix))]
        let _ = config_path;
    }
}

/// 各平台 cloudflared release 资产名。
/// 注意：macOS 官方只发布 `.tgz` 压缩包（和 .pkg），没有裸二进制；
/// 裸二进制仅 Linux / Windows 提供。
fn asset_name(os: &str, arch: &str) -> String {
    match os {
        "darwin" => format!("cloudflared-darwin-{arch}.tgz"),
        "windows" => format!("cloudflared-windows-{arch}.exe"),
        _ => format!("cloudflared-linux-{arch}"),
    }
}

/// cloudflared 查找目录：PATH + macOS GUI app 缺失的常见安装目录
/// （GUI 启动的 app PATH 只有 /usr/bin:/bin 等系统目录，找不到 Homebrew
/// 安装的 /usr/local/bin 或 /opt/homebrew/bin 下的 cloudflared）。
fn search_dirs() -> Vec<PathBuf> {
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    #[cfg(unix)]
    {
        let mut dirs = path_dirs;
        for extra in ["/usr/local/bin", "/opt/homebrew/bin"] {
            let p = PathBuf::from(extra);
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
        dirs
    }
    #[cfg(not(unix))]
    {
        path_dirs
    }
}

/// 在候选目录中找第一个能正常执行的 cloudflared（逐个 `--version` 验证）
async fn find_valid_in_dirs(dirs: &[PathBuf]) -> Option<PathBuf> {
    let cmd = if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    };
    for dir in dirs {
        let candidate = dir.join(cmd);
        if candidate.is_file() && validate_binary(&candidate).await {
            return Some(candidate);
        }
    }
    None
}

/// 校验二进制是否真的能执行（`--version`，5s 超时）。
/// 防止损坏文件（如 404 错误页面）被当作 cloudflared 使用。
async fn validate_binary(path: &std::path::Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let check = Command::new(path)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // 超时 drop future 时必须连子进程一起杀掉，否则挂死的二进制变孤儿
        .kill_on_drop(true)
        .status();
    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(5), check).await,
        Ok(Ok(status)) if status.success()
    )
}

/// 从 tgz 字节流中解出 cloudflared 二进制写入 target
fn extract_cloudflared_tgz(bytes: &[u8], target: &std::path::Path) -> Result<(), String> {
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| format!("解析 tgz 失败: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("读取 tgz 条目失败: {e}"))?;
        let is_cloudflared = entry
            .path()
            .ok()
            .and_then(|p| p.file_name().map(|n| n == "cloudflared"))
            .unwrap_or(false);
        // 只解包普通文件：symlink 条目会让后续 chmod 跟随链接修改任意文件
        if is_cloudflared && entry.header().entry_type() == tar::EntryType::Regular {
            entry
                .unpack(target)
                .map_err(|e| format!("解压 cloudflared 失败: {e}"))?;
            return Ok(());
        }
    }
    Err("tgz 中未找到 cloudflared 二进制".to_string())
}

/// 从 cloudflared 日志行提取 trycloudflare.com 公网地址
/// 判断已有 cloudflared 子进程是否需要重建：
/// - None → 需要启动；
/// - Some 且已退出/无法判定（崩溃）→ 需要回收后重启（自愈）；
/// - Some 且仍在运行 → 无需重建（start 幂等短路）。
fn tunnel_child_needs_restart(child: Option<&mut Child>) -> bool {
    match child {
        None => true,
        Some(c) => !c.try_wait().map(|s| s.is_none()).unwrap_or(false),
    }
}

fn extract_trycloudflare_url(line: &str) -> Option<String> {
    // 形如:  https://random-words-here.trycloudflare.com
    let marker = "https://";
    let mut search_from = 0;
    while let Some(idx) = line[search_from..].find(marker) {
        let start = search_from + idx + marker.len();
        let rest = &line[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '"' || c == '|')
            .unwrap_or(rest.len());
        let host = &rest[..end];
        if host.ends_with(".trycloudflare.com") {
            return Some(format!("https://{host}"));
        }
        search_from = start + end;
        if search_from >= line.len() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tunnel_child_needs_restart_when_none() {
        assert!(tunnel_child_needs_restart(None));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tunnel_child_needs_restart_when_exited() {
        // 短命进程：等它退出后再判定 → 应判定为需要重建（崩溃自愈场景）
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn");
        let _status = child.wait().await.expect("wait");
        assert!(tunnel_child_needs_restart(Some(&mut child)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tunnel_child_does_not_need_restart_when_running() {
        // 仍在运行的子进程 → 不应重建（start 幂等短路）
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "sleep 5"])
            .spawn()
            .expect("spawn");
        assert!(!tunnel_child_needs_restart(Some(&mut child)));
        let _ = child.kill().await;
    }

    #[test]
    fn render_named_config_contains_tunnel_credentials_and_ingress() {
        let yaml = render_named_config(
            "6f7b0d2e-1234-4abc-9def-000000000000",
            std::path::Path::new("/home/u/.cloudflared/6f7b0d2e-1234-4abc-9def-000000000000.json"),
            "cc.example.com",
            "http://127.0.0.1:15721",
        );
        assert!(yaml.contains("tunnel: 6f7b0d2e-1234-4abc-9def-000000000000"));
        assert!(yaml.contains(
            "credentials-file: /home/u/.cloudflared/6f7b0d2e-1234-4abc-9def-000000000000.json"
        ));
        assert!(yaml.contains("- hostname: cc.example.com"));
        assert!(yaml.contains("service: http://127.0.0.1:15721"));
        // 必须带 404 兜底，否则非匹配路径会暴露异常
        assert!(yaml.contains("- service: http_status:404"));
    }

    #[test]
    fn parse_tunnel_list_resolves_name_to_id() {
        let output = r#"[
            {"id":"6f7b0d2e-1234-4abc-9def-000000000000","name":"cc-switch","created_at":"2026-08-05T08:00:00Z","connections":[]},
            {"id":"11111111-2222-3333-4444-555555555555","name":"other","created_at":"2026-08-05T08:00:00Z","connections":[]}
        ]"#;
        assert_eq!(
            parse_tunnel_id(output, "cc-switch"),
            Some("6f7b0d2e-1234-4abc-9def-000000000000".to_string())
        );
    }

    #[test]
    fn parse_tunnel_list_accepts_id_directly() {
        let output = r#"[{"id":"6f7b0d2e-1234-4abc-9def-000000000000","name":"cc-switch","connections":[]}]"#;
        assert_eq!(
            parse_tunnel_id(output, "6f7b0d2e-1234-4abc-9def-000000000000"),
            Some("6f7b0d2e-1234-4abc-9def-000000000000".to_string())
        );
        assert_eq!(parse_tunnel_id(output, "missing"), None);
        assert_eq!(parse_tunnel_id("not json", "cc-switch"), None);
    }

    #[test]
    fn detects_named_tunnel_connection_registered() {
        let line = "2026-08-05T08:00:01Z INF Registered tunnel connection connIndex=0 connection=abc protocol=quic";
        assert!(is_connection_registered_line(line));
        assert!(!is_connection_registered_line("INF Starting tunnel"));
        assert!(!is_connection_registered_line(
            "2026-08-05T08:00:01Z INF Unregistered tunnel connection connIndex=0"
        ));
    }

    #[test]
    fn extracts_trycloudflare_url_from_log() {
        let line = "2026-08-03T18:00:00Z INF +--------------------------------------------------------------+  https://happy-cat-dog-bird.trycloudflare.com  +";
        assert_eq!(
            extract_trycloudflare_url(line),
            Some("https://happy-cat-dog-bird.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn ignores_non_trycloudflare_urls() {
        let line = "INF Registering tunnel https://api.trycloudflare.com/tunnel and https://example.com/foo";
        assert_eq!(extract_trycloudflare_url(line), None);
    }

    #[test]
    fn ignores_lines_without_url() {
        assert_eq!(extract_trycloudflare_url("INF Starting tunnel"), None);
    }

    #[test]
    fn asset_name_matches_official_release_assets() {
        // macOS 官方只发布 .tgz，没有裸二进制（404 陷阱）
        assert_eq!(
            asset_name("darwin", "arm64"),
            "cloudflared-darwin-arm64.tgz"
        );
        assert_eq!(
            asset_name("darwin", "amd64"),
            "cloudflared-darwin-amd64.tgz"
        );
        assert_eq!(asset_name("linux", "arm64"), "cloudflared-linux-arm64");
        assert_eq!(asset_name("linux", "amd64"), "cloudflared-linux-amd64");
        assert_eq!(
            asset_name("windows", "amd64"),
            "cloudflared-windows-amd64.exe"
        );
    }

    #[tokio::test]
    async fn validate_binary_rejects_error_page_saved_as_binary() {
        // 复现 bug：GitHub 404 响应体 "Not Found" 被当作可执行文件
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("cloudflared");
        std::fs::write(&fake, b"Not Found").unwrap();
        make_executable(&fake);
        assert!(!validate_binary(&fake).await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn validate_binary_accepts_working_executable() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("cloudflared");
        std::fs::write(&bin, "#!/bin/sh\necho 'cloudflared version 2026.7.3'\n").unwrap();
        make_executable(&bin);
        assert!(validate_binary(&bin).await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn validate_binary_kills_hung_process_on_timeout() {
        // 挂死的"二进制"：超时后子进程必须被杀死，不能留孤儿
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("cloudflared");
        std::fs::write(&bin, "#!/bin/sh\nexec sleep 4738\n").unwrap();
        make_executable(&bin);
        assert!(!validate_binary(&bin).await);
        // 被杀死的子进程可能短暂处于僵尸态等待 runtime 回收，轮询而非单次检查
        let mut alive = true;
        for _ in 0..30 {
            let output = std::process::Command::new("pgrep")
                .args(["-f", "sleep 4738"])
                .output()
                .unwrap();
            if !output.status.success() {
                alive = false;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(!alive, "超时后 sleep 4738 仍存活（孤儿进程）");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn find_valid_in_dirs_returns_first_working_binary() {
        let dir = tempfile::tempdir().unwrap();
        let bad_dir = dir.path().join("bad");
        let good_dir = dir.path().join("good");
        std::fs::create_dir_all(&bad_dir).unwrap();
        std::fs::create_dir_all(&good_dir).unwrap();
        // bad 目录里是损坏文件；good 目录里是可用的
        let bad = bad_dir.join("cloudflared");
        std::fs::write(&bad, b"Not Found").unwrap();
        make_executable(&bad);
        let good = good_dir.join("cloudflared");
        std::fs::write(&good, "#!/bin/sh\necho ok\n").unwrap();
        make_executable(&good);

        let found = find_valid_in_dirs(&[bad_dir, good_dir.clone()]).await;
        assert_eq!(found, Some(good_dir.join("cloudflared")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn find_valid_in_dirs_skips_directory_named_cloudflared() {
        let dir = tempfile::tempdir().unwrap();
        // 同名目录不能被误判为二进制
        std::fs::create_dir_all(dir.path().join("cloudflared")).unwrap();
        assert_eq!(find_valid_in_dirs(&[dir.path().to_path_buf()]).await, None);
    }

    #[tokio::test]
    async fn validate_binary_rejects_missing_file() {
        assert!(!validate_binary(std::path::Path::new("/nonexistent/cloudflared")).await);
    }

    #[cfg(unix)]
    #[test]
    fn search_dirs_include_common_macos_bin_dirs() {
        // macOS GUI app 的 PATH 不含这两个目录，必须显式补充
        let dirs = search_dirs();
        assert!(dirs.contains(&PathBuf::from("/usr/local/bin")));
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn extract_tgz_writes_cloudflared_binary() {
        // 内存构造 tgz（与官方 cloudflared-darwin-*.tgz 同构）
        let mut tar_builder = tar::Builder::new(Vec::new());
        let content = b"fake-cloudflared-binary";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar_builder
            .append_data(&mut header, "cloudflared", &content[..])
            .unwrap();
        let tar_bytes = tar_builder.into_inner().unwrap();

        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        let tgz = encoder.finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("cloudflared");
        extract_cloudflared_tgz(&tgz, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), content);
    }

    #[test]
    fn extract_tgz_rejects_archive_without_cloudflared() {
        let mut tar_builder = tar::Builder::new(Vec::new());
        let content = b"unrelated";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_cksum();
        tar_builder
            .append_data(&mut header, "README.txt", &content[..])
            .unwrap();
        let tar_bytes = tar_builder.into_inner().unwrap();

        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        let tgz = encoder.finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("cloudflared");
        assert!(extract_cloudflared_tgz(&tgz, &target).is_err());
        assert!(!target.exists());
    }

    #[test]
    fn extract_tgz_rejects_symlink_entry() {
        // 符号链接条目不能被解包（后续 chmod 会跟随链接修改目标文件）
        let mut tar_builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        tar_builder
            .append_link(&mut header, "cloudflared", "/etc/passwd")
            .unwrap();
        let tar_bytes = tar_builder.into_inner().unwrap();

        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        let tgz = encoder.finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("cloudflared");
        assert!(extract_cloudflared_tgz(&tgz, &target).is_err());
        assert!(!target.exists() && target.symlink_metadata().is_err());
    }

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &std::path::Path) {}
}

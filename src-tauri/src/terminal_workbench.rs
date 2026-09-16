//! 终端工作台（三栏式主界面）后端命令：
//! - 终端实例 / 项目数据持久化（settings.json 的 `terminalHub` / `projects` 字段）
//! - 跨平台拉起原生终端（返回可管理 PID，Windows / Linux 支持 kill/存活探测）
//! - 项目文件浏览、在线编辑、Git 状态与差异
//! - Claude Code / OpenCode 本机会话扫描（用于终端快速恢复会话）

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::settings;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// GUI 应用（windows_subsystem = "windows"）从无控制台进程启动
/// 控制台子程序（tasklist/taskkill/powershell 等）时，Windows 会新建
/// 一个控制台窗口；设置此标志可抑制，避免「黑窗口一闪而过」。
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// ============================================================================
// 数据结构（与前端 src/types/terminal.ts、src/lib/api/project.ts 对齐）
// ============================================================================

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminalHubState {
    #[serde(default)]
    pub instances: Vec<TerminalInstance>,
    #[serde(default)]
    pub project_dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_project_dir: Option<String>,
    #[serde(default)]
    pub expanded: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInstance {
    pub id: String,
    pub name: String,
    pub app: String,
    pub project_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_launch_at: Option<i64>,
    /// 重启应用后是否自动恢复到上次的工具会话（claude / opencode）。
    /// 缺省视为 true；置 false 则每次都开一个全新会话。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resume: Option<bool>,
    /// 用户显式指定要恢复的会话 id（优先于自动探测）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_session_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DevProject {
    pub id: String,
    pub name: String,
    pub project_dir: String,
    #[serde(default)]
    pub tools: HashMap<String, ProjectToolBinding>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectToolBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TerminalLaunchConfig {
    pub instance_id: String,
    pub app: String,
    pub project_dir: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolSessionInfo {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDirEntry {
    pub name: String,
    pub rel: String,
    pub is_dir: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusResult {
    pub git: bool,
    #[serde(default)]
    pub entries: Vec<GitStatusEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusEntry {
    pub raw: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffResult {
    pub git: bool,
    #[serde(default)]
    pub diff: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untracked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub mcp: bool,
    pub skills: usize,
    pub memory: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplyProjectResult {
    pub success: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub results: HashMap<String, ApplyToolResult>,
    pub snapshot: ProjectSnapshot,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplyToolResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============================================================================
// settings.json 持久化（terminalHub / projects 字段）
// ============================================================================

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn read_hub() -> TerminalHubState {
    settings::get_settings()
        .terminal_hub
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn write_hub(hub: &TerminalHubState) -> Result<(), String> {
    let value = serde_json::to_value(hub).map_err(|e| e.to_string())?;
    settings::mutate_settings(|s| s.terminal_hub = Some(value)).map_err(|e| e.to_string())
}

fn read_projects() -> Vec<DevProject> {
    settings::get_settings()
        .projects
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn write_projects(projects: &[DevProject]) -> Result<(), String> {
    let value = serde_json::to_value(projects).map_err(|e| e.to_string())?;
    settings::mutate_settings(|s| s.projects = Some(value)).map_err(|e| e.to_string())
}

// ============================================================================
// 终端实例管理
// ============================================================================

#[tauri::command]
pub fn get_terminal_hub_state() -> TerminalHubState {
    read_hub()
}

#[tauri::command]
pub fn save_terminal_hub_state(state: TerminalHubState) -> Result<(), String> {
    write_hub(&state)
}

#[tauri::command]
pub fn create_terminal_instance(
    name: String,
    app: String,
    #[allow(non_snake_case)] projectDir: String,
    tool: String,
    #[allow(non_snake_case)] projectId: Option<String>,
    #[allow(non_snake_case)] customCommand: Option<String>,
    args: Option<String>,
    terminal: Option<String>,
    #[allow(non_snake_case)] autoResume: Option<bool>,
) -> Result<TerminalHubState, String> {
    let mut hub = read_hub();
    hub.instances.push(TerminalInstance {
        id: uuid_v4(),
        name: if name.trim().is_empty() {
            "终端".to_string()
        } else {
            name
        },
        app,
        project_dir: projectDir,
        project_id: projectId,
        tool,
        custom_command: customCommand,
        args,
        terminal,
        pid: None,
        created_at: now_secs(),
        last_launch_at: None,
        auto_resume: autoResume,
        last_session_id: None,
    });
    write_hub(&hub)?;
    Ok(hub)
}

#[tauri::command]
pub fn delete_terminal_instance(id: String) -> Result<TerminalHubState, String> {
    let mut hub = read_hub();
    hub.instances.retain(|item| item.id != id);
    write_hub(&hub)?;
    // 连同落盘的滚动历史一起清除，避免残留文件无限堆积
    remove_persisted_history(&id);

    // 清理该实例残留的内嵌会话（进程 + 订阅者）：HUD 与大屏是独立前端实例，
    // 另一窗口启动的 pty 会话在前端本地注册表里可能没有记录，删除实例时
    // 必须按 instance_id 兜底清理，避免删除后进程仍在后台运行。
    let stale: Vec<u64> = {
        let sessions = EMBEDDED.lock().unwrap();
        sessions
            .iter()
            .filter(|(_, s)| s.instance_id == id)
            .map(|(pty_id, _)| *pty_id)
            .collect()
    };
    for pty_id in stale {
        tauri::async_runtime::spawn(async move {
            let _ = close_embedded_terminal(pty_id).await;
        });
    }

    Ok(hub)
}

// ============================================================================
// 内嵌终端：会话由后端持有，前端只是连接/显示层。
// 断开（关闭面板/窗口）不杀进程；重新连接时复用会话并回放输出历史。
// ============================================================================

/// 推送给前端的终端输出事件。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EmbeddedOutputEvent {
    /// 原始字节输出（UTF-8 / ANSI 转义序列）
    Data { data: Vec<u8> },
    /// 进程已退出
    Exit { code: Option<i32> },
}

/// 输出历史上限：截断保留最近 N 字节（重连时回放恢复屏幕）。
const HISTORY_LIMIT: usize = 2 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 输出历史落盘：应用重启后 PTY 进程无法存活，但滚动内容可以。
// 每个终端实例一个文件，重连（ensure）时回放，做到「重启后仍能加载会话」。
// ---------------------------------------------------------------------------

/// 单个实例落盘的上限（只保留最近 N 字节，避免 settings 目录无限膨胀）。
const PERSIST_HISTORY_LIMIT: usize = 512 * 1024;
/// 落盘节流间隔：读线程空闲时也会周期性醒来，够用了。
const PERSIST_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

/// 历史文件路径；instance id 只保留安全字符，防止路径穿越。
fn history_file_path(instance_id: &str) -> Option<PathBuf> {
    let safe: String = instance_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return None;
    }
    let dir = crate::config::get_app_config_dir().join("terminal-history");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{safe}.log")))
}

/// 原子写入历史（写临时文件后 rename，避免退出中断留下半截文件）。
fn persist_history(instance_id: &str, history: &[u8]) {
    let Some(path) = history_file_path(instance_id) else {
        return;
    };
    let start = history.len().saturating_sub(PERSIST_HISTORY_LIMIT);
    let bytes = &history[start..];
    let tmp = path.with_extension("log.tmp");
    if fs::write(&tmp, bytes).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

fn load_persisted_history(instance_id: &str) -> Vec<u8> {
    match history_file_path(instance_id) {
        Some(path) => fs::read(&path).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn remove_persisted_history(instance_id: &str) {
    if let Some(path) = history_file_path(instance_id) {
        let _ = fs::remove_file(&path);
    }
}

/// 后端持有的内嵌终端会话（与前端连接数无关，进程存活期间保持）。
struct EmbeddedSession {
    instance_id: String,
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Box<dyn Write + Send>>,
    /// 与读线程共享；读线程用它判断真实退出，close 时用它 kill。
    child: Arc<Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>>,
    /// 输出历史（原始字节，含 ANSI 转义），新连接时回放
    history: Arc<Mutex<Vec<u8>>>,
    /// 实时事件订阅者（每个 attach 连接一个）
    listeners: Arc<Mutex<Vec<std::sync::mpsc::Sender<EmbeddedOutputEvent>>>>,
    /// 进程是否已退出
    exited: Arc<AtomicBool>,
}

static EMBEDDED: Lazy<Mutex<HashMap<u64, EmbeddedSession>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_PTY_ID: AtomicU64 = AtomicU64::new(1);

/// 内嵌终端使用的启动器（Windows 用 cmd /k 保持会话，类 Unix 用 bash -lc）。
fn embedded_builder(_config: &TerminalLaunchConfig, command: &str) -> CommandBuilder {
    #[cfg(target_os = "windows")]
    {
        let mut cb = CommandBuilder::new("cmd");
        cb.arg("/k");
        if !command.is_empty() {
            cb.arg(command);
        }
        cb
    }
    #[cfg(not(target_os = "windows"))]
    {
        let full = if command.is_empty() {
            "$SHELL -l".to_string()
        } else {
            command.to_string()
        };
        let mut cb = CommandBuilder::new("bash");
        cb.arg("-lc");
        cb.arg(&full);
        cb
    }
}

/// 广播事件给所有连接；发送失败（对端已断开）的连接自动移除。
fn broadcast_to(
    listeners: Arc<Mutex<Vec<std::sync::mpsc::Sender<EmbeddedOutputEvent>>>>,
    evt: EmbeddedOutputEvent,
) {
    let mut guard = listeners.lock().unwrap();
    guard.retain(|tx| tx.send(evt.clone()).is_ok());
}

/// 幂等打开：同一 instance_id 已有未退出会话则复用，否则创建新会话。
/// 返回 pty id（会话标识，可多次 attach）。
#[tauri::command]
pub fn ensure_embedded_terminal(config: TerminalLaunchConfig) -> Result<u64, String> {
    {
        let sessions = EMBEDDED.lock().unwrap();
        for (id, session) in sessions.iter() {
            if session.instance_id == config.instance_id && !session.exited.load(Ordering::Relaxed) {
                return Ok(*id);
            }
        }
    }

    // 会话恢复（读盘 + 查工具会话）：耗时操作，放在 EMBEDDED 锁之外。
    // persisted 非空 ⇒ 该终端在上一轮应用生命周期里跑过，这次属于「重启后恢复」，
    // 需要回放历史并尽量恢复工具会话；否则按全新终端处理。
    let persisted = load_persisted_history(&config.instance_id);
    let restoring = !persisted.is_empty();
    let instance = read_hub()
        .instances
        .into_iter()
        .find(|item| item.id == config.instance_id);
    let resume = resolve_resume_session(&config, instance.as_ref(), restoring);
    let command = build_shell_command_with(&config, resume.as_deref());

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("创建伪终端失败: {e}"))?;

    let mut builder = embedded_builder(&config, &command);
    let cwd = if config.project_dir.trim().is_empty() {
        None
    } else {
        let path = Path::new(&config.project_dir);
        if path.is_dir() {
            Some(path.to_path_buf())
        } else {
            None
        }
    };
    if let Some(dir) = &cwd {
        builder.cwd(dir);
    }

    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(|e| format!("启动终端进程失败: {e}"))?;
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("获取终端写句柄失败: {e}"))?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("获取终端读句柄失败: {e}"))?;

    let child_shared: Arc<Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>> =
        Arc::new(Mutex::new(Some(child)));
    // 用上一轮的落盘内容作为历史起点：前端 attach 时立即回放，
    // 于是重启应用后看到的是原来的会话内容，而不是一个空白新终端。
    let history = Arc::new(Mutex::new(persisted));
    let listeners: Arc<Mutex<Vec<std::sync::mpsc::Sender<EmbeddedOutputEvent>>>> =
        Arc::new(Mutex::new(Vec::new()));
    let exited = Arc::new(AtomicBool::new(false));

    let id = NEXT_PTY_ID.fetch_add(1, Ordering::Relaxed);
    let mut sessions = EMBEDDED.lock().unwrap();
    sessions.insert(
        id,
        EmbeddedSession {
            instance_id: config.instance_id.clone(),
            master: pair.master,
            writer: Mutex::new(writer),
            child: child_shared.clone(),
            history: history.clone(),
            listeners: listeners.clone(),
            exited: exited.clone(),
        },
    );
    drop(sessions);

    // 读线程：PTY 输出 → 历史 + 广播。
    // 注意 ConPTY 的 ReadFile 在无数据时可能返回 0 字节（并非 EOF），
    // 必须结合子进程状态判断真实退出，否则运行中的终端会被误报为已退出。
    let flush_id = config.instance_id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut last_flush = std::time::Instant::now();
        let mut dirty = false;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let is_exited = child_shared
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|c| c.try_wait().ok().flatten().is_some())
                        .unwrap_or(true);
                    if is_exited {
                        break;
                    }
                    // 进程仍在运行：短暂等待后继续读
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    // 顺序固定：先历史后监听者（与 attach 一致，避免死锁）
                    {
                        let mut h = history.lock().unwrap();
                        h.extend_from_slice(&chunk);
                        if h.len() > HISTORY_LIMIT {
                            let drop_len = h.len() - HISTORY_LIMIT;
                            h.drain(..drop_len);
                        }
                    }
                    dirty = true;
                    broadcast_to(listeners.clone(), EmbeddedOutputEvent::Data { data: chunk });
                }
                Err(_) => break,
            }
            // 节流落盘：只在内容有变化且距上次落盘超过阈值时写，
            // 避免大输出时反复写整个历史（上限 512KB）。
            if dirty && last_flush.elapsed() >= PERSIST_FLUSH_INTERVAL {
                let snapshot = history.lock().unwrap().clone();
                persist_history(&flush_id, &snapshot);
                last_flush = std::time::Instant::now();
                dirty = false;
            }
        }
        // 退出前收尾落盘，保证重启后能完整回放
        if dirty || last_flush.elapsed() >= PERSIST_FLUSH_INTERVAL {
            let snapshot = history.lock().unwrap().clone();
            persist_history(&flush_id, &snapshot);
        }
        exited.store(true, Ordering::Relaxed);
        broadcast_to(listeners.clone(), EmbeddedOutputEvent::Exit { code: None });
    });

    Ok(id)
}

/// 连接会话：先回放输出历史，再订阅实时事件；进程已退出时立即收到 Exit。
/// 断开连接（Channel 关闭）不影响会话存活。
#[tauri::command]
pub fn attach_embedded_terminal(
    pty_id: u64,
    on_data: tauri::ipc::Channel<EmbeddedOutputEvent>,
) -> Result<(), String> {
    // 只取会话的共享句柄后立即释放全局锁：回放历史可能很大（上限 2MB），
    // 若在锁内同步发送会阻塞其它终端命令（写入 / 调整尺寸 / 新建），
    // 表现为「进入终端卡一下」。
    let (history, listeners, exited) = {
        let sessions = EMBEDDED.lock().unwrap();
        let session = sessions
            .get(&pty_id)
            .ok_or_else(|| "内嵌终端不存在或已关闭".to_string())?;
        (
            session.history.clone(),
            session.listeners.clone(),
            session.exited.clone(),
        )
    };
    let (tx, rx) = std::sync::mpsc::channel::<EmbeddedOutputEvent>();
    {
        // 加锁顺序与读线程一致：先 history 后 listeners
        let history = history.lock().unwrap();
        let mut listeners = listeners.lock().unwrap();
        for chunk in history.chunks(256 * 1024) {
            if on_data
                .send(EmbeddedOutputEvent::Data {
                    data: chunk.to_vec(),
                })
                .is_err()
            {
                return Err("连接已断开".to_string());
            }
        }
        if exited.load(Ordering::Relaxed) {
            let _ = on_data.send(EmbeddedOutputEvent::Exit { code: None });
        } else {
            listeners.push(tx);
        }
    }
    // 转发线程：会话事件 → IPC 通道（Channel 断开即退出，Sender 随之 drop）
    std::thread::spawn(move || {
        while let Ok(evt) = rx.recv() {
            if on_data.send(evt).is_err() {
                break;
            }
        }
    });
    Ok(())
}

/// 向内嵌终端写入输入（键盘等）。
#[tauri::command]
pub fn write_embedded_terminal(pty_id: u64, data: Vec<u8>) -> Result<(), String> {
    let sessions = EMBEDDED.lock().unwrap();
    let session = sessions
        .get(&pty_id)
        .ok_or_else(|| "内嵌终端不存在或已关闭".to_string())?;
    let mut writer = session.writer.lock().unwrap();
    writer
        .write_all(&data)
        .map_err(|e| format!("写入终端失败: {e}"))
}

/// 调整内嵌终端尺寸（列 × 行）。
#[tauri::command]
pub fn resize_embedded_terminal(pty_id: u64, cols: u16, rows: u16) -> Result<(), String> {
    let sessions = EMBEDDED.lock().unwrap();
    let session = sessions
        .get(&pty_id)
        .ok_or_else(|| "内嵌终端不存在或已关闭".to_string())?;
    session
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("调整终端尺寸失败: {e}"))
}

/// 关闭会话并终止其进程树（cmd 只是包装进程，需连带杀子进程）。
/// 仅用户主动停止/重置/删除时调用；面板关闭不会触发。
/// 异步执行：taskkill 杀进程树可能在子进程无响应时阻塞较久，
/// 放入阻塞线程池，避免同步命令在主线程上冻结 UI（删除/重置时"卡一下"）。
#[tauri::command]
pub async fn close_embedded_terminal(pty_id: u64) -> Result<(), String> {
    let session = EMBEDDED.lock().unwrap().remove(&pty_id);
    if let Some(session) = session {
        session.exited.store(true, Ordering::Relaxed);
        // 收尾落盘：用户主动停止时也要保留滚动内容，下次启动可回放
        {
            let snapshot = session.history.lock().unwrap().clone();
            persist_history(&session.instance_id, &snapshot);
        }
        if let Some(mut child) = session.child.lock().unwrap().take() {
            let pid = child.process_id();
            child.kill().ok();
            #[cfg(target_os = "windows")]
            if let Some(pid) = pid {
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/T", "/F"])
                        .creation_flags(CREATE_NO_WINDOW)
                        .output();
                });
            }
        }
        broadcast_to(
            session.listeners.clone(),
            EmbeddedOutputEvent::Exit { code: None },
        );
    }
    Ok(())
}

/// 程序退出时清理全部内嵌终端会话（进程随应用退出而终止）。
pub fn cleanup_all_embedded() {
    let sessions = EMBEDDED.lock().unwrap().drain().collect::<Vec<_>>();
    for (_id, session) in sessions {
        session.exited.store(true, Ordering::Relaxed);
        // 应用退出：把滚动内容落盘，下次启动 ensure 时回放（会话恢复的关键）
        {
            let snapshot = session.history.lock().unwrap().clone();
            persist_history(&session.instance_id, &snapshot);
        }
        if let Some(mut child) = session.child.lock().unwrap().take() {
            let pid = child.process_id();
            child.kill().ok();
            #[cfg(target_os = "windows")]
            if let Some(pid) = pid {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            }
        }
    }
}

/// 清除某个终端实例落盘的输出历史（「清除会话并重新初始化」用）。
/// 只删磁盘快照，不影响正在运行的会话；需要真正全新终端时先关闭会话再调用。
#[tauri::command]
pub fn clear_terminal_history(
    #[allow(non_snake_case)] instanceId: String,
) -> Result<(), String> {
    remove_persisted_history(&instanceId);
    Ok(())
}

// ============================================================================
// 终端启动 / 停止 / 存活探测
// ============================================================================

#[tauri::command]
pub fn detect_available_terminals() -> Vec<String> {
    let mut list = Vec::new();
    #[cfg(target_os = "windows")]
    {
        list.push("cmd".to_string());
        list.push("powershell".to_string());
        list.push("wt".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        list.push("terminal".to_string());
        list.push("iterm".to_string());
        list.push("ghostty".to_string());
        list.push("wezterm".to_string());
        list.push("kitty".to_string());
        list.push("alacritty".to_string());
    }
    #[cfg(target_os = "linux")]
    {
        list.push("x-terminal-emulator".to_string());
        list.push("gnome-terminal".to_string());
        list.push("konsole".to_string());
        list.push("kitty".to_string());
        list.push("alacritty".to_string());
    }
    list
}

/// 根据实例配置构造要执行的命令（与前端 resolveShellCommand 语义一致）。
fn build_shell_command(config: &TerminalLaunchConfig) -> String {
    build_shell_command_with(config, None)
}

/// 在 `build_shell_command` 基础上追加会话恢复参数（`--resume <id>` / `--session <id>`）。
fn build_shell_command_with(config: &TerminalLaunchConfig, resume: Option<&str>) -> String {
    if config.tool == "shell" {
        return String::new();
    }
    if config.tool == "custom" {
        return config
            .custom_command
            .clone()
            .unwrap_or_default()
            .trim()
            .to_string();
    }
    let args = config.args.as_deref().unwrap_or("").trim();
    let resume = resume.map(str::trim).filter(|s| !s.is_empty());
    let resume_flag = session_resume_flag(&config.tool);
    let mut command = if args.is_empty() {
        config.tool.clone()
    } else {
        format!("{} {}", config.tool, args)
    };
    if let (Some(flag), Some(session)) = (resume_flag, resume) {
        command.push_str(&format!(" {flag} {session}"));
    }
    command
}

/// 工具 → 会话恢复参数名（与前端 SESSION_ARG_FLAG 保持一致）。
fn session_resume_flag(tool: &str) -> Option<&'static str> {
    match tool {
        "claude" => Some("--resume"),
        "opencode" => Some("--session"),
        _ => None,
    }
}

/// 启动参数里已有的会话标记（用户显式指定时不再自动追加）。
const SESSION_ARG_TOKENS: &[&str] = &["--resume", "--session", "--continue", "-c", "-r", "-s"];

/// 决定本次启动要恢复到哪个工具会话（返回 None 表示开全新会话）。
///
/// 只在「重启后恢复」场景生效：即该实例有落盘历史（上一轮跑过）。
/// 首次创建的终端不会误恢复别人的会话。
fn resolve_resume_session(
    config: &TerminalLaunchConfig,
    instance: Option<&TerminalInstance>,
    restoring: bool,
) -> Option<String> {
    if session_resume_flag(&config.tool).is_none() {
        return None;
    }
    // 用户已在启动参数里写了会话标记：尊重显式配置
    let args = config.args.as_deref().unwrap_or("");
    if args
        .split_whitespace()
        .any(|token| SESSION_ARG_TOKENS.contains(&token))
    {
        return None;
    }
    // 用户显式指定的会话 id 优先
    if let Some(sid) = instance
        .and_then(|item| item.last_session_id.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(sid.to_string());
    }
    if !restoring {
        return None;
    }
    // 默认开启自动恢复；auto_resume = false 时保持全新会话
    if instance.and_then(|item| item.auto_resume) == Some(false) {
        return None;
    }
    let dir = if config.project_dir.trim().is_empty() {
        None
    } else {
        Some(config.project_dir.as_str())
    };
    let latest = match config.tool.as_str() {
        "claude" => claude_sessions(dir).into_iter().next(),
        "opencode" => opencode_sessions(dir).into_iter().next(),
        _ => None,
    };
    latest.map(|item| item.session_id)
}

/// 拉起原生终端，返回可管理 PID（不可管理时返回 0）。
#[tauri::command]
pub fn launch_terminal(config: TerminalLaunchConfig) -> Result<i64, String> {
    let command = build_shell_command(&config);
    let cwd = if config.project_dir.trim().is_empty() {
        None
    } else {
        let path = Path::new(&config.project_dir);
        if path.is_dir() {
            Some(path.to_path_buf())
        } else {
            None
        }
    };

    #[cfg(target_os = "windows")]
    {
        let shell = config.terminal.as_deref().unwrap_or("cmd");
        if shell == "wt" {
            // Windows Terminal：多标签由 wt 自身管理，进程不可按 PID 管理，返回 0。
            let mut cmd = Command::new("wt.exe");
            cmd.arg("new-tab");
            if let Some(dir) = &cwd {
                cmd.args(["--workingDirectory"]).arg(dir);
            }
            if command.is_empty() {
                cmd.arg("cmd").arg("/k");
            } else {
                cmd.arg("cmd").arg("/k").arg(&command);
            }
            cmd.spawn().map_err(|e| format!("启动终端失败: {e}"))?;
            return Ok(0);
        }

        let child;
        if shell == "powershell" {
            let mut cmd = Command::new("powershell");
            cmd.arg("-NoExit").arg("-Command");
            if !command.is_empty() {
                cmd.arg(&command);
            }
            if let Some(dir) = &cwd {
                cmd.current_dir(dir);
            }
            child = cmd.spawn().map_err(|e| format!("启动终端失败: {e}"))?;
        } else {
            // cmd /k 保持窗口打开；空命令直接开 cmd
            let mut cmd = Command::new("cmd");
            if command.is_empty() {
                cmd.arg("/k");
            } else {
                cmd.arg("/k").arg(&command);
            }
            if let Some(dir) = &cwd {
                cmd.current_dir(dir);
            }
            child = cmd.spawn().map_err(|e| format!("启动终端失败: {e}"))?;
        }
        let pid = child.id() as i64;
        // 注意：不能 kill 主进程——杀掉 cmd 会立即关闭控制台窗口，
        // 命令子进程虽继续运行，但输出无处可见，窗口也不可聚焦。
        Ok(pid)
    }

    #[cfg(target_os = "macos")]
    {
        let preferred = settings::get_settings().preferred_terminal;
        let target = config
            .terminal
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| match preferred.as_deref() {
                Some("iterm2") => "iterm".to_string(),
                Some(t) if !t.is_empty() => t.to_string(),
                _ => "terminal".to_string(),
            });
        let full = if command.is_empty() {
            "exec $SHELL -l".to_string()
        } else {
            command
        };
        crate::session_manager::terminal::launch_terminal(
            &target,
            &full,
            cwd.as_deref().map(|p| p.to_str().unwrap_or("")),
            None,
        )?;
        Ok(0)
    }

    #[cfg(target_os = "linux")]
    {
        let full = if command.is_empty() {
            "$SHELL -l".to_string()
        } else {
            command
        };
        let mut cmd = Command::new("x-terminal-emulator");
        cmd.arg("-e").arg("bash").arg("-lc").arg(&full);
        if let Some(dir) = &cwd {
            cmd.current_dir(dir);
        }
        let child = cmd.spawn().map_err(|e| format!("启动终端失败: {e}"))?;
        Ok(child.id() as i64)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err("不支持的操作系统".to_string())
    }
}

/// 终止终端进程树。异步执行：taskkill 可能阻塞较久，放入线程池避免冻结 UI。
#[tauri::command]
pub async fn kill_terminal(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        let target = pid;
        tauri::async_runtime::spawn_blocking(move || {
            Command::new("taskkill")
                .args(["/PID", &target.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let target = pid;
        tauri::async_runtime::spawn_blocking(move || {
            Command::new("kill")
                .args(["-9", &target.to_string()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    }
}

/// 把已运行的终端窗口带到前台（取消最小化并聚焦）。
/// 进程不存在或无法取得窗口句柄时返回 false（调用方应重新拉起终端）。
#[tauri::command]
pub fn focus_terminal(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        // 注意：控制台进程的 MainWindowHandle 常为 0，必须枚举顶层窗口按 PID 匹配。
        let script = format!(
            r#"$ErrorActionPreference = 'SilentlyContinue'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class FocusHelper {{
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
}}
'@
$targetPid = {pid}
$script:found = [IntPtr]::Zero
$callback = [FocusHelper+EnumWindowsProc] {{
    param($hwnd, $lparam)
    $winPid = [uint32]0
    [FocusHelper]::GetWindowThreadProcessId($hwnd, [ref]$winPid) | Out-Null
    if ($winPid -eq $targetPid -and [FocusHelper]::IsWindowVisible($hwnd)) {{
        $script:found = $hwnd
        return $false
    }}
    return $true
}}
[FocusHelper]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
if ($script:found -ne [IntPtr]::Zero) {{
    if ([FocusHelper]::IsIconic($script:found)) {{ [FocusHelper]::ShowWindow($script:found, 9) | Out-Null }}
    [FocusHelper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero) | Out-Null
    [FocusHelper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero) | Out-Null
    [FocusHelper]::SetForegroundWindow($script:found) | Out-Null
    Write-Output '1'
}} else {{
    Write-Output '0'
}}"#,
            pid = pid
        );
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        return output
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
            .unwrap_or(false);
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"System Events\" to set frontmost of first process whose unix id is {} to true",
            pid
        );
        let output = Command::new("osascript").args(["-e", &script]).output();
        return output.map(|o| o.status.success()).unwrap_or(false);
    }

    #[cfg(target_os = "linux")]
    {
        let output = Command::new("xdotool")
            .args([
                "search",
                "--pid",
                &pid.to_string(),
                "windowactivate",
                "--sync",
            ])
            .output();
        return output.map(|o| o.status.success()).unwrap_or(false);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

#[tauri::command]
pub fn check_terminal_alive(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let stdout = output.map(|o| o.stdout).unwrap_or_default();
        String::from_utf8_lossy(&stdout).contains(&pid.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

// ============================================================================
// 开发项目（多工具绑定 + 应用）
// ============================================================================

#[tauri::command]
pub fn list_projects() -> Vec<DevProject> {
    let mut projects = read_projects();
    projects.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    projects
}

#[tauri::command]
pub fn create_project(
    name: String,
    #[allow(non_snake_case)] projectDir: String,
    tools: Option<HashMap<String, ProjectToolBinding>>,
) -> Result<DevProject, String> {
    let name = name.trim().to_string();
    let project_dir = projectDir.trim().to_string();
    if name.is_empty() {
        return Err("项目名称不能为空".to_string());
    }
    if project_dir.is_empty() {
        return Err("请选择项目目录".to_string());
    }
    let mut projects = read_projects();
    let project = DevProject {
        id: uuid_v4(),
        name,
        project_dir,
        tools: tools.unwrap_or_default(),
        created_at: now_secs(),
        updated_at: now_secs(),
    };
    projects.push(project.clone());
    write_projects(&projects)?;
    Ok(project)
}

#[tauri::command]
pub fn update_project(
    id: String,
    name: String,
    #[allow(non_snake_case)] projectDir: String,
) -> Result<DevProject, String> {
    let name = name.trim().to_string();
    let project_dir = projectDir.trim().to_string();
    if name.is_empty() {
        return Err("项目名称不能为空".to_string());
    }
    if project_dir.is_empty() {
        return Err("请选择项目目录".to_string());
    }
    let mut projects = read_projects();
    let project = projects
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "项目不存在".to_string())?;
    project.name = name;
    project.project_dir = project_dir;
    project.updated_at = now_secs();
    let updated = project.clone();
    write_projects(&projects)?;
    Ok(updated)
}

#[tauri::command]
pub fn delete_project(id: String) -> bool {
    let mut projects = read_projects();
    let before = projects.len();
    projects.retain(|p| p.id != id);
    if projects.len() == before {
        return false;
    }
    if write_projects(&projects).is_ok() {
        true
    } else {
        false
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    #[serde(flatten)]
    pub project: DevProject,
    #[serde(default)]
    pub apps: HashMap<String, ProjectAppDetail>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAppDetail {
    #[serde(default)]
    pub providers: Vec<ProjectProviderBrief>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_provider_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProviderBrief {
    pub id: String,
    pub name: String,
    pub category: String,
}

#[tauri::command]
pub fn get_project_detail(id: String) -> Result<ProjectDetail, String> {
    let projects = read_projects();
    let project = projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| "项目不存在".to_string())?;
    let mut apps = HashMap::new();
    for (app, binding) in &project.tools {
        apps.insert(
            app.clone(),
            ProjectAppDetail {
                providers: Vec::new(),
                bound_provider_id: binding.provider_id.clone(),
            },
        );
    }
    Ok(ProjectDetail { project, apps })
}

/// 应用项目：把各工具的绑定供应商切换到当前配置（Claude 写盘，其余记录选择）。
#[tauri::command]
pub async fn apply_project(
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<ApplyProjectResult, String> {
    let projects = read_projects();
    let project = projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| "项目不存在".to_string())?;

    let mut warnings = Vec::new();
    let mut results = HashMap::new();
    for (app, binding) in &project.tools {
        let Some(provider_id) = binding.provider_id.as_deref() else {
            continue;
        };
        let result = crate::commands::switch_provider(
            app_handle.clone(),
            app.clone(),
            provider_id.to_string(),
        )
        .await;
        match result {
            Ok(_) => {
                results.insert(
                    app.clone(),
                    ApplyToolResult {
                        ok: true,
                        error: None,
                    },
                );
            }
            Err(e) => {
                warnings.push(format!("{app}：{e}"));
                results.insert(
                    app.clone(),
                    ApplyToolResult {
                        ok: false,
                        error: Some(e),
                    },
                );
            }
        }
    }

    Ok(ApplyProjectResult {
        success: warnings.is_empty(),
        warnings,
        results,
        snapshot: ProjectSnapshot {
            mcp: false,
            skills: 0,
            memory: false,
        },
    })
}

// ============================================================================
// 工具本机会话（Claude Code / OpenCode）
// ============================================================================

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Claude Code 项目目录 munging：与 Claude Code 端规则完全一致——
/// 仅把 `\`、`/`、`:` 替换为 `-`，**不合并**连续连字符、不修剪首尾。
/// （旧实现会合并连续 `-`，导致 `D:\x` → `D-x` 而 Claude Code 实际生成
/// `D--x`，会话目录永远匹配不上、历史会话无法加载。）
fn claude_munge_candidates(project_dir: &str) -> Vec<String> {
    let abs = Path::new(project_dir);
    let abs_str = abs.to_string_lossy().replace('/', "\\");
    let munge = |s: &str| s.replace(['\\', '/', ':'], "-");
    let mut candidates = vec![munge(&abs_str)];
    candidates.sort();
    candidates.dedup();
    candidates.into_iter().filter(|c| !c.is_empty()).collect()
}

/// 从 jsonl 文件头部提取会话标题（第一条 user 消息文本）。
fn claude_session_title(file_path: &Path) -> String {
    let Ok(bytes) = fs::read(file_path) else {
        return String::new();
    };
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(16 * 1024)]);
    for line in head.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(obj) =
            serde_json::from_str::<serde_json::Value>(line.trim_start_matches('\u{feff}'))
        else {
            continue;
        };
        let Some(msg) = obj.get("message") else {
            continue;
        };
        if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let title = extract_claude_text(msg.get("content")).unwrap_or_default();
        if !title.is_empty() {
            return truncate_title(&title);
        }
    }
    String::new()
}

fn extract_claude_text(content: Option<&serde_json::Value>) -> Option<String> {
    match content {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(parts)) => {
            for part in parts {
                if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.trim().is_empty() {
                            return Some(text.to_string());
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn truncate_title(text: &str) -> String {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return String::new();
    }
    if t.chars().count() > 80 {
        let truncated: String = t.chars().take(80).collect();
        format!("{}…", truncated)
    } else {
        t
    }
}

fn claude_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}

fn claude_sessions(project_dir: Option<&str>) -> Vec<ToolSessionInfo> {
    let projects_root = claude_config_dir().join("projects");
    if !projects_root.is_dir() {
        return Vec::new();
    }
    let candidates = project_dir.map(claude_munge_candidates);
    let mut sessions = Vec::new();
    let Ok(entries) = fs::read_dir(&projects_root) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().to_string();
        if let Some(cands) = &candidates {
            // 大小写不敏感匹配：Windows 路径在终端输入与实际运行时的大小写可能不一致
            let dir_lower = dir_name.to_ascii_lowercase();
            let matched = cands.iter().any(|c| {
                let c_lower = c.to_ascii_lowercase();
                dir_lower == c_lower || dir_lower.starts_with(&format!("{c_lower}-"))
            });
            if !matched {
                continue;
            }
        }
        let Ok(files) = fs::read_dir(entry.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            let session_id = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if session_id.is_empty() {
                continue;
            }
            let last_active_at = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .map(|ms| {
                    // ISO 8601（毫秒时间戳）
                    let secs = ms / 1000;
                    let days = secs / 86400;
                    format!(
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                        days / 365 + 1970,
                        (days % 365) / 30 + 1,
                        days % 30 + 1,
                        (secs % 86400) / 3600,
                        (secs % 3600) / 60,
                        secs % 60,
                        ms % 1000
                    )
                })
                .unwrap_or_default();
            sessions.push(ToolSessionInfo {
                session_id,
                title: {
                    let t = claude_session_title(&path);
                    if t.is_empty() {
                        None
                    } else {
                        Some(t)
                    }
                },
                last_active_at: if last_active_at.is_empty() {
                    None
                } else {
                    Some(last_active_at)
                },
                project_dir: project_dir.map(|p| p.to_string()),
            });
        }
    }
    sessions.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
    sessions.truncate(50);
    sessions
}

/// 收集 OpenCode session JSON（~/.local/share/opencode 项目级 + 全局）。
/// 旧版 opencode 才有 JSON；新版一律走 `opencode.db`（见 opencode_db_sessions）。
/// 递归遍历是为了兼容 `storage/session/<项目哈希>/ses_*.json` 这类嵌套布局。
fn collect_opencode_sessions(session_dir: &Path, out: &mut Vec<ToolSessionInfo>) {
    if !session_dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(session_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_opencode_sessions(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}'))
        else {
            continue;
        };
        let session_id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        let created = obj
            .get("time")
            .and_then(|t| t.get("created"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        out.push(ToolSessionInfo {
            session_id,
            title: obj
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| truncate_title(s))
                .filter(|s| !s.is_empty()),
            last_active_at: if created > 0 {
                Some(iso_from_ms(created))
            } else {
                None
            },
            project_dir: obj
                .get("project")
                .or_else(|| obj.get("cwd"))
                .or_else(|| obj.get("directory"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        });
    }
}

/// epoch 毫秒 → ISO8601 UTC 展示串（civil_from_days 算法，正确处理闰年）。
fn iso_from_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yy = if m <= 2 { y + 1 } else { y };
    format!("{yy:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}.{millis:03}Z")
}

/// 按项目目录过滤会话（大小写不敏感，`\` 归一为 `/`）。
fn filter_by_project(
    found: Vec<ToolSessionInfo>,
    project_dir: Option<&str>,
) -> Vec<ToolSessionInfo> {
    let target = project_dir.map(|p| p.replace('\\', "/").to_ascii_lowercase());
    match target {
        Some(t) if !t.is_empty() => found
            .into_iter()
            .filter(|s| {
                s.project_dir
                    .as_deref()
                    .map(|p| p.replace('\\', "/").to_ascii_lowercase())
                    .map(|p| p == t || p.starts_with(&format!("{t}/")))
                    .unwrap_or(false)
            })
            .collect(),
        _ => found,
    }
}

/// 从 OpenCode 的 SQLite 会话库读最近会话。OpenCode ≥ 0.3 把会话存在
/// `~/.local/share/opencode/opencode.db`，不再是 JSON 文件；WAL 模式并发读
/// 安全，用只读打开避免干扰运行中的 opencode。
fn opencode_db_sessions(project_dir: Option<&str>) -> Vec<ToolSessionInfo> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let db_path = home
        .join(".local")
        .join("share")
        .join("opencode")
        .join("opencode.db");
    if !db_path.is_file() {
        return Vec::new();
    }
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT id, title, directory, time_updated \
         FROM session \
         WHERE time_archived IS NULL \
         ORDER BY time_updated DESC \
         LIMIT 50",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut found = Vec::new();
    for row in rows.flatten() {
        let (id, title, directory, updated_ms) = row;
        found.push(ToolSessionInfo {
            session_id: id,
            title: title
                .as_deref()
                .map(truncate_title)
                .filter(|s| !s.is_empty()),
            last_active_at: updated_ms.map(iso_from_ms),
            project_dir: directory,
        });
    }
    filter_by_project(found, project_dir)
}

fn opencode_sessions(project_dir: Option<&str>) -> Vec<ToolSessionInfo> {
    // 新版 opencode 会话全在 SQLite；JSON 文件扫描只作旧版本兜底。
    let from_db = opencode_db_sessions(project_dir);
    if !from_db.is_empty() {
        return from_db;
    }
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let root = home.join(".local").join("share").join("opencode");
    if !root.is_dir() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let project_root = root.join("project");
    if project_root.is_dir() {
        if let Ok(dirs) = fs::read_dir(&project_root) {
            for dir in dirs.flatten() {
                if !dir.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                collect_opencode_sessions(&dir.path().join("storage").join("session"), &mut found);
            }
        }
    }
    collect_opencode_sessions(&root.join("storage").join("session"), &mut found);
    filter_by_project(found, project_dir)
}

#[tauri::command]
pub fn list_tool_sessions(
    tool: String,
    #[allow(non_snake_case)] projectDir: Option<String>,
) -> Vec<ToolSessionInfo> {
    match tool.as_str() {
        "claude" => claude_sessions(projectDir.as_deref()),
        "opencode" => opencode_sessions(projectDir.as_deref()),
        _ => Vec::new(),
    }
}

/// AI 提示词润色：经本地代理（按当前供应商转发）调用 chat/completions。
#[tauri::command]
pub async fn polish_prompt(_app: String, text: String) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("请输入需要润色的内容".to_string());
    }
    let port = crate::proxy::http_client::proxy_port();
    if port == 0 {
        return Err("本地代理未运行，请先在设置中开启本地代理".to_string());
    }
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let body = serde_json::json!({
        "model": "gpt-5",
        "stream": false,
        "messages": [
            {
                "role": "system",
                "content": "你是专业的提示词润色助手。把用户输入的内容润色为清晰、结构化、可直接用于 AI 编程助手的指令：修正错别字与语病、补充必要的上下文、用列表/要点组织复杂需求，保留用户原有意图与技术细节，不要编造无关内容。直接输出润色结果，不要解释过程。"
            },
            { "role": "user", "content": text }
        ]
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("创建请求客户端失败: {e}"))?;
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("调用润色服务失败: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err_body = resp.text().await.unwrap_or_default();
        return Err(format!("润色服务返回 {status}: {err_body}"));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析润色结果失败: {e}"))?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "润色服务未返回内容".to_string())?;
    Ok(content)
}

/// 重置窗口大小：删除 window-state 插件的记忆文件（.window-state.json），
/// 并立即恢复到 tauri.conf.json 的默认尺寸并居中；下次启动也使用默认大小。
#[tauri::command]
pub fn reset_window_size(app_handle: tauri::AppHandle) -> Result<(), String> {
    let dir = crate::config::get_app_config_dir();
    let state_file = dir.join(".window-state.json");
    if state_file.exists() {
        std::fs::remove_file(&state_file).map_err(|e| e.to_string())?;
    }
    let window = app_handle
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window
        .set_size(tauri::LogicalSize::new(1400.0, 860.0))
        .map_err(|e| e.to_string())?;
    window.center().map_err(|e| e.to_string())?;
    Ok(())
}

// ============================================================================
// 项目文件浏览 / 在线编辑 / Git 差异
// ============================================================================

const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".cache",
    ".turbo",
    ".parcel-cache",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    "coverage",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".DS_Store",
];

fn is_ignored_dir(name: &str) -> bool {
    IGNORED_DIRS.contains(&name) || name.starts_with('.')
}

/// 解析项目根内的相对路径（防路径穿越）。
fn safe_resolve(root: &str, rel: &str) -> Result<PathBuf, String> {
    let base = Path::new(root).canonicalize().map_err(|e| e.to_string())?;
    let target = if rel.trim().is_empty() {
        base.clone()
    } else {
        base.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
    };
    if target != base && !target.starts_with(&base) {
        return Err("路径越界：只允许访问项目目录内文件".to_string());
    }
    Ok(target)
}

#[tauri::command]
pub fn list_project_dir(root: String, rel: Option<String>) -> Result<Vec<ProjectDirEntry>, String> {
    let dir = safe_resolve(&root, rel.as_deref().unwrap_or(""))?;
    if !dir.is_dir() {
        return Err(format!("目录不存在：{}", dir.display()));
    }
    let mut items = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(items);
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && is_ignored_dir(&name) {
            continue;
        }
        let rel = match rel.as_deref().unwrap_or("") {
            "" => name.clone(),
            prefix => format!("{}/{}", prefix, name),
        };
        items.push(ProjectDirEntry { name, rel, is_dir });
    }
    items.sort_by(|a, b| {
        a.is_dir
            .cmp(&b.is_dir)
            .reverse()
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(items)
}

#[tauri::command]
pub fn read_project_file(path: String) -> Result<String, String> {
    let target = PathBuf::from(&path);
    if !target.is_file() {
        return Err(format!("文件不存在：{}", target.display()));
    }
    let bytes = fs::read(&target).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "二进制文件无法在文本编辑器中打开".to_string())
}

#[tauri::command]
pub fn write_project_file(path: String, content: String) -> Result<bool, String> {
    let target = PathBuf::from(&path);
    fs::write(&target, content).map_err(|e| e.to_string())?;
    Ok(true)
}

fn run_git(root: &Path, args: &[&str]) -> (i32, String, String) {
    // GUI 应用（windows_subsystem = "windows"）无控制台，直接拉 git.exe 会被
    // Windows 新建控制台窗口（git status 每 8s 轮询 → 反复弹出黑窗）；必须静默。
    let mut command = Command::new("git");
    command.args(args).current_dir(root);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().map_err(|e| e.to_string());
    match output {
        Ok(o) => (
            o.status.code().unwrap_or(1),
            String::from_utf8_lossy(&o.stdout).to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
        ),
        Err(e) => (1, String::new(), e),
    }
}

#[tauri::command]
pub fn git_status(root: String) -> GitStatusResult {
    let dir = PathBuf::from(&root);
    if !dir.join(".git").exists() {
        return GitStatusResult {
            git: false,
            entries: Vec::new(),
            error: None,
        };
    }
    let (code, stdout, stderr) = run_git(&dir, &["status", "--porcelain", "--untracked-files=all"]);
    if code != 0 {
        return GitStatusResult {
            git: true,
            entries: Vec::new(),
            error: Some(stderr.trim().to_string()),
        };
    }
    let entries = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let raw = line.chars().take(2).collect::<String>();
            let path = line.chars().skip(3).collect::<String>();
            GitStatusEntry {
                raw: raw.trim().to_string(),
                path,
            }
        })
        .collect();
    GitStatusResult {
        git: true,
        entries,
        error: None,
    }
}

#[tauri::command]
pub fn git_diff(root: String, path: Option<String>) -> GitDiffResult {
    let dir = PathBuf::from(&root);
    if !dir.join(".git").exists() {
        return GitDiffResult {
            git: false,
            diff: String::new(),
            untracked: None,
            content: None,
            error: None,
        };
    }
    let (code, stdout, stderr) = match &path {
        Some(target) => run_git(&dir, &["diff", "--no-ext-diff", "--", target]),
        None => run_git(&dir, &["diff", "--no-ext-diff"]),
    };
    if code != 0 {
        return GitDiffResult {
            git: true,
            diff: String::new(),
            untracked: None,
            content: None,
            error: Some(stderr.trim().to_string()),
        };
    }
    if !stdout.trim().is_empty() {
        return GitDiffResult {
            git: true,
            diff: stdout,
            untracked: None,
            content: None,
            error: None,
        };
    }
    // 无差异输出：可能是未跟踪文件，回退返回文件全文
    if let Some(target) = &path {
        let abs = safe_resolve(&root, target).ok();
        if let Some(abs) = abs {
            if abs.is_file() {
                if let Ok(content) = fs::read_to_string(&abs) {
                    return GitDiffResult {
                        git: true,
                        diff: String::new(),
                        untracked: Some(true),
                        content: Some(content),
                        error: None,
                    };
                }
            }
        }
    }
    GitDiffResult {
        git: true,
        diff: String::new(),
        untracked: None,
        content: None,
        error: None,
    }
}

fn uuid_v4() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let rand_part = (now ^ (counter << 16)) & 0x0000_ffff_ffff_ffff;
    format!("t-{:016x}-{:04x}-{:012x}", now, counter, rand_part)
}

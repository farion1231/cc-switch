//! 让 GUI 启动的 cc-doctor 进程能拿到用户登录 shell 里的 PATH。
//!
//! 背景：macOS GUI 应用从 Dock/Finder 启动时，进程 PATH 通常只有
//! `/usr/bin:/bin:/usr/sbin:/sbin` —— launchd 提供的"系统默认"，
//! 不会继承用户在 `~/.zshrc` / `~/.bash_profile` 里 `export PATH=...`
//! 配置的那一份。结果就是 `Command::new("claude")`、`Command::new("brew")`、
//! `Command::new("node")` 等等几乎一定 "command not found"，即便用户
//! 在 terminal 里能跑得好好的。
//!
//! 修法：进程启动最早期跑一次用户 SHELL 的 login + interactive 模式，
//! 把它最终算出的 `$PATH` 拉出来 setenv 到当前进程。**一次性**，所有
//! 后续 `Command::new(...)` 派生的子进程都会继承新的 PATH。
//!
//! ## 调用时机
//!
//! 必须在 [`tauri::Builder::default()`] 之前、`setup_panic_hook()` 之后
//! 调用——此时进程是单线程，`std::env::set_var` 安全。
//!
//! ## 为什么用 `-l -i -c`
//!
//! - `-l`（login）：让 shell 读 `~/.zprofile`、`~/.bash_profile`、`~/.profile`
//! - `-i`（interactive）：让 shell 读 `~/.zshrc`、`~/.bashrc`
//!
//! macOS 上**绝大多数教程都教用户把 `export PATH=...` 写到 `~/.zshrc`**
//! （即 interactive 路径），而不是 `.zprofile`。所以光 `-l` 不够，必须 `-i`。
//!
//! `-i` 在非 tty 下 zsh 可能因 `bindkey` / `compdef` 等命令在 stderr 打 warning，
//! 不影响 stdout 输出，丢弃即可。

use std::process::Command;

/// 用 `printf` 而非 `echo`：echo 在不同 shell（dash / sh / zsh）下行为
/// 略有差异，printf 更跨 shell 稳定。`%s` 格式不会附加换行（虽然我们
/// 之后会 trim），但语义更明确。
const PATH_PROBE_SCRIPT: &str = r#"printf '%s' "$PATH""#;

/// 从用户登录 shell 拉真实 PATH 并写入当前进程环境。
///
/// 失败时不 panic、不 return Err，仅记录日志并保留原 PATH——最差情况
/// 跟"完全没修"一样，不会让 cc-doctor 启动失败。
#[cfg(target_os = "macos")]
pub fn fix_path_from_login_shell() {
    let original = std::env::var("PATH").unwrap_or_default();
    let raw_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    // 只信任 zsh / bash —— fish / nushell 等非 POSIX shell 的 -lic 行为
    // 不一致，强行用会拿到错的 PATH。这种少数派直接 fallback /bin/zsh
    // （macOS 13+ 系统默认 zsh 一定存在），至少能读到系统级 .zshenv 和
    // 用户级 .zshrc 里的 PATH。
    let shell = if raw_shell.ends_with("/zsh") || raw_shell.ends_with("/bash") {
        raw_shell.clone()
    } else {
        log::info!(
            "fix_path: SHELL={:?} 非 zsh/bash，回退到 /bin/zsh 探测 PATH",
            raw_shell
        );
        "/bin/zsh".to_string()
    };

    let output = Command::new(&shell)
        .args(["-l", "-i", "-c", PATH_PROBE_SCRIPT])
        .output();

    let new_path = match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        Ok(out) => {
            log::warn!(
                "fix_path: {} 退出码非 0 ({:?})，保留原 PATH",
                shell,
                out.status.code()
            );
            return;
        }
        Err(e) => {
            log::warn!("fix_path: 启动 {} 失败 ({})，保留原 PATH", shell, e);
            return;
        }
    };

    // 简单 sanity check：拿到的 PATH 至少应该包含一个冒号分隔符且非空。
    // 防止 shell 因为 .zshrc 里有 syntax error / exit 1 等情况返回了
    // 半截结果或空字符串。
    if new_path.is_empty() || !new_path.contains(':') {
        log::warn!(
            "fix_path: 拿到的 PATH 不像样 (len={}, has_colon={})，保留原 PATH",
            new_path.len(),
            new_path.contains(':')
        );
        return;
    }

    if new_path == original {
        log::info!("fix_path: 用户 PATH 与进程 PATH 已一致，跳过");
        return;
    }

    log::info!(
        "fix_path: 已用 {} 的 PATH 替换进程 PATH（{} → {} chars）",
        shell,
        original.len(),
        new_path.len()
    );

    // SAFETY: 由 doc comment 约定，本函数只在 Tauri builder 启动前、单线程
    // 上下文中调用一次，没有其他线程并发读 env。Rust 1.81+ 把 set_var 标
    // 为可能不安全主要是为了警告"多线程下并发改 env 是 UB"，单线程使用
    // 完全安全。
    unsafe {
        std::env::set_var("PATH", new_path);
    }
}

/// 非 macOS 平台占位 —— Linux 桌面应用从 .desktop 文件启动时 PATH 也
/// 可能不全，未来若需扩展再实现。Windows 不存在这个问题（GUI 进程会
/// 继承用户级 PATH 注册表项）。
#[cfg(not(target_os = "macos"))]
pub fn fix_path_from_login_shell() {}

//! 用宿主的 xdg-open 打开链接与目录。
//!
//! AppImage 的 AppRun 会把挂载点（`$APPDIR`，如 `/tmp/.mount_CC-SwiXXXXX`）
//! 垫到 `PATH` 与 `LD_LIBRARY_PATH` 前面，注入一批指向挂载点的
//! `PYTHONHOME` / `QT_PLUGIN_PATH` / `XDG_DATA_DIRS`，还会强制
//! `GDK_BACKEND=x11`、`GTK_THEME`（为 AppImage 自己的 WebKit 打的补丁）。
//! `tauri-plugin-opener` 带着这些环境起 `xdg-open`，于是：
//!
//! - 命中的是 AppImage 自带的那份 xdg-open 1.1.3。它的 `open_kde` 只认
//!   `KDE_SESSION_VERSION` 4 / 5，Plasma 6（值为 6）匹配不到任何分支，什么也
//!   不做就 `exit_success`——用户看到的是「点了没反应」，日志里连错误都没有；
//! - 即使换成系统的 xdg-open，宿主 `kde-open` 也会继承 `LD_LIBRARY_PATH`，载到
//!   挂载点里的 `libssl.so.3`，报 `OPENSSL_3.5.0 not found`（Arch 已是 3.5，
//!   AppImage 里是 3.0）。
//!
//! 所以 AppImage 内自己起宿主 xdg-open，清理后的环境只交给子进程，主进程照旧
//! 用挂载点里的库。deb / rpm、macOS、Windows 没有这层污染，仍然交给插件。

use std::path::Path;

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// 用系统浏览器打开 `http:` / `https:` / `mailto:` 链接。
pub fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if let Some(root) = appimage::mount_root() {
        return appimage::open_with_host_launcher(std::ffi::OsStr::new(url), &root);
    }

    app.opener()
        .open_url(url, None::<String>)
        .map_err(|e| e.to_string())
}

/// 用系统文件管理器打开目录（调用方负责保证目录已存在）。
pub fn open_path(app: &AppHandle, path: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if let Some(root) = appimage::mount_root() {
        return appimage::open_with_host_launcher(path.as_os_str(), &root);
    }

    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| e.to_string())
}

/// 只在 Linux + AppImage 下生效的部分。
#[cfg(target_os = "linux")]
mod appimage {
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    /// 以路径分隔符连接的变量：逐段丢掉挂载点下的段，一段不剩就整条删除。
    const PATH_LIST_VARS: &[&str] = &[
        "PATH",
        "LD_LIBRARY_PATH",
        "PYTHONPATH",
        "PERLLIB",
        "XDG_DATA_DIRS",
        "GI_TYPELIB_PATH",
        "GIO_EXTRA_MODULES",
        "QT_PLUGIN_PATH",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
    ];

    /// `LD_PRELOAD` 同时认冒号与空白分隔，任一段来自挂载点就整条丢弃。
    const PRELOAD_VARS: &[&str] = &["LD_PRELOAD"];

    /// AppRun 为自己的 WebKit 强制覆盖的变量：不是用户的选择，宿主程序用不到。
    /// 留给浏览器会把它推到 XWayland（无 XWayland 时直接起不来）并钉住
    /// AppImage 的主题，所以整条删除。
    const FORCED_VARS: &[&str] = &["GDK_BACKEND", "GTK_THEME"];

    /// 单值路径变量：整套都由 AppImage 注入，宿主程序用不到，落在挂载点里就删。
    const SINGLE_PATH_VARS: &[&str] = &[
        "PYTHONHOME",
        "GTK_DATA_PREFIX",
        "GTK_EXE_PREFIX",
        "GTK_PATH",
        "GTK_IM_MODULE_FILE",
        "GSETTINGS_SCHEMA_DIR",
        "GDK_PIXBUF_MODULE_FILE",
    ];

    const LAUNCHER: &str = "xdg-open";

    /// AppImage 挂载根。`APPIMAGE` 与 `APPDIR` 齐备（AppRun 才会同时设置）且
    /// `APPDIR` 是绝对路径时才算 AppImage；否则返回 `None`，走插件。
    pub(super) fn mount_root() -> Option<PathBuf> {
        mount_root_from(std::env::var_os("APPIMAGE"), std::env::var_os("APPDIR"))
    }

    fn mount_root_from(appimage: Option<OsString>, appdir: Option<OsString>) -> Option<PathBuf> {
        let (appimage, appdir) = (appimage?, appdir?);
        if appimage.is_empty() || appdir.is_empty() {
            return None;
        }
        let root = PathBuf::from(appdir);
        root.is_absolute().then_some(root)
    }

    /// 起宿主 `xdg-open` 打开 `target`（URL 或路径）。
    ///
    /// 子进程拿清理过的环境，否则宿主 `kde-open` / `gio` 会加载挂载点里的
    /// `libssl` / `libgio` 而崩掉。退出码不阻塞调用方：`setsid` 后的子进程在后台
    /// 收尸，失败只记日志（浏览器起不来不该让 UI 卡住，也不该静默）。
    pub(super) fn open_with_host_launcher(target: &OsStr, root: &Path) -> Result<(), String> {
        let path_env = std::env::var_os("PATH").unwrap_or_default();
        let launcher = host_launcher(&path_env, root).ok_or_else(|| {
            format!(
                "PATH 里没有 AppImage 挂载点 {} 之外可执行的 {LAUNCHER}",
                root.display()
            )
        })?;
        log::debug!("AppImage: 用 {} 打开目标", launcher.display());

        let mut cmd = host_command(&launcher, target, root);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        detach(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("启动 {} 失败: {e}", launcher.display()))?;

        std::thread::spawn(move || match child.wait() {
            Ok(status) if !status.success() => {
                log::warn!("{LAUNCHER} 退出码 {status}，目标可能没有打开");
            }
            Ok(_) => {}
            Err(e) => log::warn!("等待 {LAUNCHER} 退出失败: {e}"),
        });

        Ok(())
    }

    fn detach(cmd: &mut Command) {
        use std::os::unix::process::CommandExt;

        // setsid 而非只换进程组：新会话脱离控制终端，从终端启动时 Ctrl-C
        // 不会连带打断已经交出去的浏览器。
        // SAFETY: setsid 是 async-signal-safe；fork 出的子进程继承父进程组、
        // 必不是会话组长，调用不会因 EPERM 失败。
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    /// PATH 里第一个落在挂载点之外、且真的可执行的 `xdg-open`。
    ///
    /// AppImage 自带的那份就在挂载点里，正好被这里跳过。只看绝对路径段：
    /// 相对段是相对子进程的 cwd 解析的，而 AppImage 的 cwd 就在挂载点里。
    fn host_launcher(path_env: &OsStr, root: &Path) -> Option<PathBuf> {
        std::env::split_paths(path_env)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(LAUNCHER))
            .find(|candidate| !candidate.starts_with(root) && is_executable(candidate))
    }

    fn is_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;

        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    fn host_command(launcher: &Path, target: &OsStr, root: &Path) -> Command {
        host_command_with(launcher, target, root, |key| std::env::var_os(key))
    }

    /// `env` 是变量名到值的查找函数；测试传入假环境，生产走真实环境。
    fn host_command_with(
        launcher: &Path,
        target: &OsStr,
        root: &Path,
        env: impl Fn(&str) -> Option<OsString>,
    ) -> Command {
        let mut cmd = Command::new(launcher);
        cmd.arg(target);
        for key in PATH_LIST_VARS {
            if let Some(value) = env(key) {
                scrub_path_list(&mut cmd, OsStr::new(key), &value, root);
            }
        }
        for key in SINGLE_PATH_VARS {
            if let Some(value) = env(key) {
                scrub_single_path(&mut cmd, OsStr::new(key), &value, root);
            }
        }
        for key in PRELOAD_VARS {
            if let Some(value) = env(key) {
                scrub_preload(&mut cmd, OsStr::new(key), &value, root);
            }
        }
        for key in FORCED_VARS {
            cmd.env_remove(key);
        }
        cmd
    }

    fn scrub_path_list(cmd: &mut Command, key: &OsStr, value: &OsStr, root: &Path) {
        // 空段要一起丢掉：AppRun 拼出来的 LD_LIBRARY_PATH 以分隔符结尾，
        // 空段在动态链接器眼里等于当前目录。
        let kept: Vec<PathBuf> = std::env::split_paths(value)
            .filter(|segment| !segment.as_os_str().is_empty())
            .filter(|segment| !segment.starts_with(root))
            .collect();

        match std::env::join_paths(&kept) {
            // 清完一段不剩就是 AppImage 独有的变量，留着只会污染宿主程序。
            Ok(_) if kept.is_empty() => {
                cmd.env_remove(key);
            }
            Ok(joined) => {
                cmd.env(key, joined);
            }
            // 段里带了分隔符之外的异常字符，宁可不传。
            Err(_) => {
                cmd.env_remove(key);
            }
        }
    }

    fn scrub_preload(cmd: &mut Command, key: &OsStr, value: &OsStr, root: &Path) {
        let from_mount = value
            .to_string_lossy()
            .split(|c: char| c == ':' || c.is_whitespace())
            .filter(|token| !token.is_empty())
            .any(|token| Path::new(token).starts_with(root));

        if from_mount {
            cmd.env_remove(key);
        } else {
            cmd.env(key, value);
        }
    }

    fn scrub_single_path(cmd: &mut Command, key: &OsStr, value: &OsStr, root: &Path) {
        if Path::new(value).starts_with(root) {
            cmd.env_remove(key);
        } else {
            cmd.env(key, value);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        use std::collections::HashMap;
        use std::os::unix::fs::PermissionsExt;

        const MOUNT: &str = "/tmp/.mount_CC-SwifmjiFo";

        /// `Some(None)` = 被显式删除，`Some(Some(v))` = 被设为 v，`None` = 没碰。
        fn env_of(cmd: &Command, key: &str) -> Option<Option<OsString>> {
            cmd.get_envs()
                .find(|(name, _)| *name == OsStr::new(key))
                .map(|(_, value)| value.map(|v| v.to_os_string()))
        }

        fn write_file(path: &Path, mode: u32) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        #[test]
        fn mount_root_needs_both_appimage_vars_and_an_absolute_appdir() {
            assert_eq!(
                mount_root_from(Some("/home/u/CC.AppImage".into()), Some(MOUNT.into())),
                Some(PathBuf::from(MOUNT))
            );
            assert_eq!(
                mount_root_from(Some("/home/u/CC.AppImage".into()), None),
                None
            );
            assert_eq!(mount_root_from(None, Some(MOUNT.into())), None);
            assert_eq!(mount_root_from(Some("".into()), Some(MOUNT.into())), None);
            assert_eq!(
                mount_root_from(
                    Some("/home/u/CC.AppImage".into()),
                    Some("squashfs-root".into())
                ),
                None
            );
        }

        #[test]
        fn path_lists_lose_the_mount_segments_and_the_empty_tail() {
            let mut cmd = Command::new("xdg-open");

            // AppRun 真正写出来的形态：只有挂载点、且以分隔符结尾。
            scrub_path_list(
                &mut cmd,
                OsStr::new("LD_LIBRARY_PATH"),
                OsStr::new(&format!("{MOUNT}/usr/lib:{MOUNT}/lib64:")),
                Path::new(MOUNT),
            );
            assert_eq!(env_of(&cmd, "LD_LIBRARY_PATH"), Some(None));

            // 真实值里的段带尾斜杠，例如 `$APPDIR/usr/bin/`。
            scrub_path_list(
                &mut cmd,
                OsStr::new("PATH"),
                OsStr::new(&format!(
                    "{MOUNT}/usr/bin/:{MOUNT}/bin/:/usr/local/bin:/usr/bin"
                )),
                Path::new(MOUNT),
            );
            assert_eq!(
                env_of(&cmd, "PATH"),
                Some(Some(OsString::from("/usr/local/bin:/usr/bin")))
            );
        }

        #[test]
        fn path_lists_match_by_component_not_by_string_prefix() {
            let mut cmd = Command::new("xdg-open");

            // 长得像挂载点的兄弟目录（/tmp/.mount_X-foo）是用户自己的路径，保留。
            scrub_path_list(
                &mut cmd,
                OsStr::new("PATH"),
                OsStr::new(&format!("{MOUNT}-other/bin:{MOUNT}2/bin:/usr/bin")),
                Path::new(MOUNT),
            );
            assert_eq!(
                env_of(&cmd, "PATH"),
                Some(Some(OsString::from(format!(
                    "{MOUNT}-other/bin:{MOUNT}2/bin:/usr/bin"
                ))))
            );
        }

        #[test]
        fn single_path_vars_are_dropped_only_inside_the_mount() {
            let mut cmd = Command::new("xdg-open");

            scrub_single_path(
                &mut cmd,
                OsStr::new("PYTHONHOME"),
                OsStr::new(&format!("{MOUNT}//usr")),
                Path::new(MOUNT),
            );
            assert_eq!(env_of(&cmd, "PYTHONHOME"), Some(None));

            // hook 里 `GTK_DATA_PREFIX=$APPDIR`，就是挂载根本身。
            scrub_single_path(
                &mut cmd,
                OsStr::new("GTK_DATA_PREFIX"),
                OsStr::new(MOUNT),
                Path::new(MOUNT),
            );
            assert_eq!(env_of(&cmd, "GTK_DATA_PREFIX"), Some(None));

            // 用户自己的 PYTHONHOME 不该被牵连。
            scrub_single_path(
                &mut cmd,
                OsStr::new("PYTHONHOME"),
                OsStr::new("/opt/python"),
                Path::new(MOUNT),
            );
            assert_eq!(
                env_of(&cmd, "PYTHONHOME"),
                Some(Some(OsString::from("/opt/python")))
            );
        }

        #[test]
        fn preload_is_dropped_when_any_token_comes_from_the_mount() {
            let mut cmd = Command::new("xdg-open");

            // glibc 的 LD_PRELOAD 除了冒号也认空白。
            scrub_preload(
                &mut cmd,
                OsStr::new("LD_PRELOAD"),
                OsStr::new(&format!("/usr/lib/host.so {MOUNT}/usr/lib/mount.so")),
                Path::new(MOUNT),
            );
            assert_eq!(env_of(&cmd, "LD_PRELOAD"), Some(None));

            scrub_preload(
                &mut cmd,
                OsStr::new("LD_PRELOAD"),
                OsStr::new("/usr/lib/host.so:/opt/extra.so"),
                Path::new(MOUNT),
            );
            assert_eq!(
                env_of(&cmd, "LD_PRELOAD"),
                Some(Some(OsString::from("/usr/lib/host.so:/opt/extra.so")))
            );
        }

        #[test]
        fn launcher_skips_the_appimage_copy_the_non_executable_and_the_directory() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("mount");
            write_file(&root.join("usr/bin").join(LAUNCHER), 0o755);

            let noexec = tmp.path().join("noexec/bin");
            write_file(&noexec.join(LAUNCHER), 0o644);

            // 目录带执行位，但它不是可执行文件。
            std::fs::create_dir_all(tmp.path().join("dir/bin").join(LAUNCHER)).unwrap();

            let host = tmp.path().join("host/bin").join(LAUNCHER);
            write_file(&host, 0o755);

            let path_env = std::env::join_paths([
                root.join("usr/bin"),
                noexec,
                tmp.path().join("dir/bin"),
                host.parent().unwrap().to_path_buf(),
            ])
            .unwrap();

            assert_eq!(host_launcher(&path_env, &root), Some(host));
        }

        #[test]
        fn launcher_is_none_when_only_the_appimage_copy_exists() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("mount");
            write_file(&root.join("usr/bin").join(LAUNCHER), 0o755);

            let path_env = std::env::join_paths([root.join("usr/bin")]).unwrap();
            assert_eq!(host_launcher(&path_env, &root), None);
        }

        #[test]
        fn command_gets_the_scrubbed_app_run_environment() {
            let mut env: HashMap<&str, OsString> = HashMap::new();
            env.insert(
                "PATH",
                format!("{MOUNT}/usr/bin:{MOUNT}/bin:/usr/local/bin:/usr/bin").into(),
            );
            env.insert(
                "LD_LIBRARY_PATH",
                format!("{MOUNT}/usr/lib:{MOUNT}/lib64:").into(),
            );
            env.insert(
                "XDG_DATA_DIRS",
                format!("{MOUNT}/usr/share/:/usr/share").into(),
            );
            env.insert("PYTHONHOME", format!("{MOUNT}/usr/").into());
            env.insert("PERLLIB", format!("{MOUNT}/usr/share/perl5/:").into());
            env.insert(
                "QT_PLUGIN_PATH",
                format!("{MOUNT}/usr/lib/qt5/plugins/").into(),
            );
            env.insert(
                "LD_PRELOAD",
                format!("{MOUNT}/usr/lib/x86_64-linux-gnu/libx.so").into(),
            );
            // AppRun 为自己的 WebKit 强制覆盖的两个变量。
            env.insert("GDK_BACKEND", "x11".into());
            env.insert("GTK_THEME", "Adwaita:dark".into());

            let cmd = host_command_with(
                Path::new("/usr/bin/xdg-open"),
                OsStr::new("https://example.com"),
                Path::new(MOUNT),
                |key| env.get(key).cloned(),
            );

            assert_eq!(cmd.get_program(), OsStr::new("/usr/bin/xdg-open"));
            assert_eq!(
                cmd.get_args().collect::<Vec<_>>(),
                vec![OsStr::new("https://example.com")]
            );
            assert_eq!(
                env_of(&cmd, "PATH"),
                Some(Some(OsString::from("/usr/local/bin:/usr/bin")))
            );
            assert_eq!(env_of(&cmd, "LD_LIBRARY_PATH"), Some(None));
            assert_eq!(
                env_of(&cmd, "XDG_DATA_DIRS"),
                Some(Some(OsString::from("/usr/share")))
            );
            assert_eq!(env_of(&cmd, "PYTHONHOME"), Some(None));
            assert_eq!(env_of(&cmd, "PERLLIB"), Some(None));
            assert_eq!(env_of(&cmd, "QT_PLUGIN_PATH"), Some(None));
            assert_eq!(env_of(&cmd, "LD_PRELOAD"), Some(None));
            // 宿主浏览器自己选后端和主题，不继承 AppImage 的 WebKit 补丁。
            assert_eq!(env_of(&cmd, "GDK_BACKEND"), Some(None));
            assert_eq!(env_of(&cmd, "GTK_THEME"), Some(None));
        }
    }
}

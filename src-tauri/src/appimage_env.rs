//! AppImage 下启动宿主程序时的环境清理（#7946）。
//!
//! AppImage 的 AppRun 和 GTK 启动钩子会给主进程注入一批指向挂载目录（`$APPDIR`，
//! 形如 `/tmp/.mount_CC-SwiXXXXX`）的环境变量：`LD_LIBRARY_PATH`、`PYTHONHOME`、
//! `PYTHONPATH`、`PATH` 前缀、`GTK_*` / `GDK_*` 等。主进程自己和 WebKitGTK 的
//! 子进程要靠它们找到打包进来的库，所以不能在启动时全局清掉。
//!
//! 但 CC Switch 启动的是宿主系统上的程序（登录 shell、`codex --version`、终端模拟器、
//! `ps` 等），它们继承这套环境后会加载 AppImage 自带的库：`/usr/bin/env` 因为
//! `libsystemd.so.0` 缺少新的符号版本直接起不来（所有 `#!/usr/bin/env node` 脚本
//! 都跟着失败），宿主 python3 因为 `PYTHONHOME` 找不到 `encodings` 模块而崩溃。
//!
//! [`scrub_command`] 在 spawn 前去掉这些变量里位于挂载目录下的路径段。AppRun 只在
//! 原值前面追加路径段，去掉后就是用户自己的值；整条都来自 AppImage 的变量直接删除。
//! 不在 AppImage 里运行时什么都不做。

use std::ffi::{OsStr, OsString};
use std::process::Command;

/// 清理即将启动的宿主程序的环境。须放在所有 `.env()` 之后调用：由当前 `PATH`
/// 拼出来、显式设置的 `PATH` 同样带着挂载目录，也要一起清理。
pub(crate) fn scrub_command(cmd: &mut Command) {
    if let Some(mount) = appimage_mount_dir() {
        scrub_with(cmd, &mount, std::env::vars_os());
    }
}

/// 当前 AppImage 的挂载目录。`APPIMAGE` 和 `APPDIR` 都由 AppImage 运行时设置，
/// 两者同时存在才认为是在 AppImage 里运行。
fn appimage_mount_dir() -> Option<String> {
    if !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_none() {
        return None;
    }
    normalize_mount_dir(&std::env::var("APPDIR").ok()?)
}

fn normalize_mount_dir(appdir: &str) -> Option<String> {
    // 只接受根目录以外的绝对路径：`/` 会让所有路径段都算作挂载目录下的，
    // 去掉末尾的 `/` 后它变成空串，在这里被拒绝。
    let mount = appdir.trim_end_matches('/');
    mount.starts_with('/').then(|| mount.to_string())
}

fn scrub_with(
    cmd: &mut Command,
    mount: &str,
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) {
    let explicit: Vec<(OsString, Option<OsString>)> = cmd
        .get_envs()
        .map(|(key, value)| (key.to_os_string(), value.map(OsStr::to_os_string)))
        .collect();

    // 子进程实际拿到的值：显式设置的优先，其余来自继承；显式删除的不用再管。
    let mut effective: Vec<(OsString, OsString)> = inherited
        .into_iter()
        .filter(|(key, _)| !explicit.iter().any(|(explicit_key, _)| explicit_key == key))
        .collect();
    effective.extend(
        explicit
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key, value))),
    );

    for (key, value) in effective {
        // 挂载目录是 UTF-8 路径，非 UTF-8 的值不可能包含它。
        let Some(value) = value.to_str() else {
            continue;
        };
        let Some(kept) = strip_mount_segments(value, mount) else {
            continue;
        };
        if kept.is_empty() {
            cmd.env_remove(&key);
        } else {
            cmd.env(&key, kept);
        }
    }
}

/// 去掉 `value` 里位于 `mount` 下的 `:` 分隔路径段。没有这样的路径段时返回 `None`，
/// 否则返回剩下的部分（可能为空）。
fn strip_mount_segments(value: &str, mount: &str) -> Option<String> {
    let under_mount = |segment: &str| {
        segment
            .strip_prefix(mount)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    if !value.split(':').any(under_mount) {
        return None;
    }
    Some(
        value
            .split(':')
            .filter(|segment| !under_mount(segment))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const MOUNT: &str = "/tmp/.mount_CC-SwiAbC123";

    fn mounted(path: &str) -> String {
        format!("{MOUNT}{path}")
    }

    /// AppImageKit AppRun 写入的 `LD_LIBRARY_PATH`：10 段挂载目录，后面接原值。
    fn apprun_ld_library_path(original: &str) -> String {
        let segments = [
            "/usr/lib/",
            "/usr/lib/i386-linux-gnu/",
            "/usr/lib/x86_64-linux-gnu/",
            "/usr/lib32/",
            "/usr/lib64/",
            "/lib/",
            "/lib/i386-linux-gnu/",
            "/lib/x86_64-linux-gnu/",
            "/lib32/",
            "/lib64/",
        ];
        let mut value = segments
            .iter()
            .map(|segment| mounted(segment))
            .collect::<Vec<_>>()
            .join(":");
        value.push(':');
        value.push_str(original);
        value
    }

    #[test]
    fn strips_apprun_segments_back_to_the_original_value() {
        assert_eq!(
            strip_mount_segments(&apprun_ld_library_path("/opt/cuda/lib64"), MOUNT).as_deref(),
            Some("/opt/cuda/lib64")
        );
        assert_eq!(
            strip_mount_segments(
                &format!(
                    "{}:{}:/usr/local/bin:/usr/bin",
                    mounted("/usr/bin/"),
                    mounted("/usr/sbin/")
                ),
                MOUNT
            )
            .as_deref(),
            Some("/usr/local/bin:/usr/bin")
        );
    }

    #[test]
    fn values_that_only_come_from_the_appimage_become_empty() {
        assert_eq!(
            strip_mount_segments(&apprun_ld_library_path(""), MOUNT).as_deref(),
            Some("")
        );
        assert_eq!(
            strip_mount_segments(&mounted("/usr/"), MOUNT).as_deref(),
            Some("")
        );
        assert_eq!(strip_mount_segments(MOUNT, MOUNT).as_deref(), Some(""));
    }

    #[test]
    fn values_outside_the_mount_are_left_alone() {
        assert_eq!(strip_mount_segments("/usr/local/bin:/usr/bin", MOUNT), None);
        assert_eq!(strip_mount_segments("", MOUNT), None);
        // 同一前缀的其他目录不在挂载目录下。
        assert_eq!(
            strip_mount_segments(&format!("{MOUNT}2/usr/lib"), MOUNT),
            None
        );
    }

    #[test]
    fn mount_dir_must_be_an_absolute_path_other_than_root() {
        assert_eq!(normalize_mount_dir(MOUNT).as_deref(), Some(MOUNT));
        assert_eq!(
            normalize_mount_dir(&format!("{MOUNT}/")).as_deref(),
            Some(MOUNT)
        );
        assert_eq!(normalize_mount_dir("/"), None);
        assert_eq!(normalize_mount_dir(""), None);
        assert_eq!(normalize_mount_dir("tmp/.mount_x"), None);
    }

    #[test]
    fn scrubs_inherited_and_explicit_values() {
        let inherited = [
            ("LD_LIBRARY_PATH", apprun_ld_library_path("")),
            ("PYTHONHOME", mounted("/usr/")),
            (
                "PYTHONPATH",
                format!("{}:/home/me/py", mounted("/usr/share/pyshared/")),
            ),
            ("PATH", format!("{}:/usr/bin", mounted("/usr/bin/"))),
            ("HOME", "/home/me".to_string()),
            ("GDK_BACKEND", "x11".to_string()),
        ]
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));

        let mut cmd = Command::new("tool");
        cmd.env(
            "PATH",
            format!("/home/me/.local/bin:{}:/usr/bin", mounted("/usr/bin/")),
        )
        .env_remove("PYTHONPATH");
        scrub_with(&mut cmd, MOUNT, inherited);

        let envs: HashMap<String, Option<String>> = cmd
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();

        assert_eq!(envs.get("LD_LIBRARY_PATH"), Some(&None));
        assert_eq!(envs.get("PYTHONHOME"), Some(&None));
        // 调用方显式删除的变量保持删除。
        assert_eq!(envs.get("PYTHONPATH"), Some(&None));
        // 显式设置的 PATH 按显式值清理，而不是退回继承的 PATH。
        assert_eq!(
            envs.get("PATH"),
            Some(&Some("/home/me/.local/bin:/usr/bin".to_string()))
        );
        assert!(!envs.contains_key("HOME"));
        assert!(!envs.contains_key("GDK_BACKEND"));
    }

    #[cfg(unix)]
    #[test]
    fn spawned_child_sees_the_scrubbed_environment() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args([
            "-c",
            r#"printf '%s|%s' "${LD_LIBRARY_PATH-unset}" "${PYTHONHOME-unset}""#,
        ])
        .env("LD_LIBRARY_PATH", apprun_ld_library_path("/opt/lib"))
        .env("PYTHONHOME", mounted("/usr/"));
        scrub_with(&mut cmd, MOUNT, std::iter::empty());

        let output = cmd.output().expect("run /bin/sh");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "/opt/lib|unset");
    }
}

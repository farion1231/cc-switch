//! Claude Code 跑在哪：本机，还是 Windows 上某个 WSL 发行版里。
//!
//! 配置目录是 `\\wsl.localhost\<distro>\home\<user>\.claude`（或 `\\wsl$\...`、`\root\.claude`）
//! 时，登录和「打开终端」要在该发行版里执行，`CLAUDE_CONFIG_DIR` 用 Linux 路径。
//! 路径解析和命令拼接不碰文件系统，Linux CI 也能测。真正启动 `wsl.exe` 只在 Windows 上。
//!
//! 非 Windows 上这些函数只被测试调用，库本身用不到。
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::Path;

/// 能在 WSL 里放账号目录时的发行版。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslTarget {
    /// `wsl.localhost` 或 `wsl$`，按用户写的来，用来拼回 Windows 能读的 UNC。
    pub host: String,
    pub distro: String,
    /// `/home/<user>` 或 `/root`。
    pub linux_home: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunTarget {
    Native,
    Wsl(WslTarget),
}

/// 从 Claude 配置目录的 Windows 路径判断运行目标。
/// 指向 WSL、但既不在 `/home/<user>` 也不在 `/root` 下时返回 [`RunTarget::Native`]：
/// 没有可以安全放置账号目录的家目录（调用方仍可用「发行版名在不在」决定要不要挡住切卡）。
pub fn from_override_path(path: &str) -> RunTarget {
    let Some(unc) = parse_wsl_unc(path) else {
        return RunTarget::Native;
    };
    let Some(linux_home) = linux_home_of(&unc.linux_path) else {
        return RunTarget::Native;
    };
    RunTarget::Wsl(WslTarget {
        host: unc.host,
        distro: unc.distro,
        linux_home,
    })
}

/// 当前进程是不是「Claude 配置目录在 WSL 家目录里」。只有 Windows 上会是。
pub fn current_wsl() -> Option<WslTarget> {
    #[cfg(windows)]
    {
        let dir = crate::settings::get_claude_override_dir()?;
        return match from_override_path(&dir.to_string_lossy()) {
            RunTarget::Wsl(wsl) => Some(wsl),
            RunTarget::Native => None,
        };
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// 账号目录在发行版里的 Windows 视图：
/// `\\<host>\<distro>\home\<user>\.cc-switch\claude-accounts`。
pub fn account_root_unc(override_dir: &str) -> Option<String> {
    let RunTarget::Wsl(wsl) = from_override_path(override_dir) else {
        return None;
    };
    let rel = wsl.linux_home.trim_start_matches('/').replace('/', "\\");
    Some(format!(
        r"\\{}\{}\{}\.cc-switch\claude-accounts",
        wsl.host, wsl.distro, rel
    ))
}

/// `\\wsl.localhost\Ubuntu\home\me\proj` → `/home/me/proj`。
/// `C:\Users\me\proj` → `/mnt/c/Users/me/proj`。
/// UNC 的发行版必须和 `distro` 是同一个（大小写不敏感）。
pub fn windows_path_to_linux(path: &str, distro: &str) -> Result<String, String> {
    if let Some(unc) = parse_wsl_unc(path) {
        if !unc.distro.eq_ignore_ascii_case(distro) {
            return Err(format!(
                "目录在 WSL 发行版 {} 里，当前 Claude 配置目录在 {} 里",
                unc.distro, distro
            ));
        }
        return Ok(unc.linux_path);
    }
    drive_to_mnt(path).ok_or_else(|| format!("无法把目录转成 WSL 里的路径: {path}"))
}

/// `wsl.exe` 的参数（不含程序名）：在发行版的登录 shell 里跑一次 `claude`。
///
/// 先按 `-lic`、`-lc`、`-c` 找得到 `claude` 的 flag（nvm 这类写在 rc 里），找到就 `exec`，
/// 不再换 flag。`env` 放在 rc 之后，rc 里 export 的同名变量盖不住这里的值。
pub fn wsl_claude_argv(
    distro: &str,
    linux_cd: &str,
    set: &[(&str, &str)],
    unset: &[&str],
    claude_args: &[&str],
) -> Result<Vec<String>, String> {
    validate_distro(distro)?;
    validate_linux_abs(linux_cd)?;
    let script = shell_payload(set, unset, claude_args)?;
    Ok(vec![
        "-d".to_string(),
        distro.to_string(),
        "--cd".to_string(),
        linux_cd.to_string(),
        "--".to_string(),
        "sh".to_string(),
        "-c".to_string(),
        script,
    ])
}

/// 写进 Windows `.bat` 的一行：`wsl.exe -d <distro> --cd <linux> -- sh -c ...`。
/// `claude_args` 的第一项是 `claude`。路径或参数里有 `%`、`"`、`^` 时拒绝，避免 cmd 展开。
pub fn wsl_terminal_batch_line(
    distro: &str,
    linux_cd: &str,
    set: &[(&str, &str)],
    unset: &[&str],
    claude_args: &[String],
) -> Result<String, String> {
    validate_distro(distro)?;
    let cd = cmd_quote(linux_cd)?;
    let args: Vec<&str> = claude_args.iter().map(String::as_str).collect();
    let script = shell_payload(set, unset, &args)?;
    if script.contains('%') {
        return Err("WSL 命令里有 %，Windows 终端无法安全传递".to_string());
    }
    Ok(format!(
        "wsl.exe -d {distro} --cd {cd} -- sh -c {}",
        cmd_double_quote(&script)
    ))
}

/// 把 UNC 账号目录转成给 `CLAUDE_CONFIG_DIR` 的 Linux 路径。不是这条发行版的 UNC 时为 `None`。
#[cfg_attr(not(windows), allow(dead_code))]
pub fn linux_config_dir(config_dir: &Path, distro: &str) -> Option<String> {
    windows_path_to_linux(&config_dir.to_string_lossy(), distro).ok()
}

struct WslUnc {
    host: String,
    distro: String,
    linux_path: String,
}

fn parse_wsl_unc(raw: &str) -> Option<WslUnc> {
    let normalized = raw.trim().replace('/', "\\");
    let normalized = strip_verbatim_unc(&normalized);
    let body = normalized.trim_start_matches('\\').trim_end_matches('\\');
    if body.is_empty() || normalized.starts_with('\\') && !normalized.starts_with("\\\\") {
        return None;
    }
    if !normalized.starts_with("\\\\") {
        return None;
    }
    let mut parts = body.split('\\');
    let host = parts.next()?;
    let distro = parts.next()?;
    if !host.eq_ignore_ascii_case("wsl.localhost") && !host.eq_ignore_ascii_case("wsl$") {
        return None;
    }
    if !valid_distro_name(distro) {
        return None;
    }
    let mut linux = String::new();
    for part in parts {
        if !valid_path_part(part) {
            return None;
        }
        linux.push('/');
        linux.push_str(part);
    }
    if linux.is_empty() {
        return None;
    }
    Some(WslUnc {
        host: host.to_string(),
        distro: distro.to_string(),
        linux_path: linux,
    })
}

fn strip_verbatim_unc(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    let prefix = r"\\?\unc\";
    if lower.starts_with(prefix) {
        format!(r"\\{}", &path[prefix.len()..])
    } else {
        path.to_string()
    }
}

fn linux_home_of(linux_path: &str) -> Option<String> {
    let parts: Vec<&str> = linux_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    match parts.as_slice() {
        ["home", user, ..] if valid_path_part(user) => Some(format!("/home/{user}")),
        ["root", ..] => Some("/root".to_string()),
        _ => None,
    }
}

fn drive_to_mnt(path: &str) -> Option<String> {
    let normalized = path.trim().replace('/', "\\");
    let without_verbatim = normalized
        .strip_prefix(r"\\?\")
        .filter(|rest| !rest.to_ascii_lowercase().starts_with("unc\\"))
        .unwrap_or(&normalized);
    let mut chars = without_verbatim.chars();
    let letter = chars.next()?;
    if !letter.is_ascii_alphabetic() || chars.next() != Some(':') {
        return None;
    }
    let rest = without_verbatim[2..].trim_start_matches('\\');
    let mut linux = format!("/mnt/{}", letter.to_ascii_lowercase());
    if rest.is_empty() {
        return Some(linux);
    }
    for part in rest.split('\\') {
        if !valid_path_part(part) {
            return None;
        }
        linux.push('/');
        linux.push_str(part);
    }
    Some(linux)
}

fn valid_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn valid_path_part(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.chars().any(|c| c.is_control() || c == '\0')
}

fn validate_distro(distro: &str) -> Result<(), String> {
    if valid_distro_name(distro) {
        Ok(())
    } else {
        Err(format!("Invalid WSL distro name: {distro}"))
    }
}

fn validate_linux_abs(path: &str) -> Result<(), String> {
    if path.starts_with('/') && !path.contains("//") && path.split('/').skip(1).all(valid_path_part)
    {
        Ok(())
    } else {
        Err(format!("无效的 Linux 路径: {path}"))
    }
}

fn valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn shell_payload(
    set: &[(&str, &str)],
    unset: &[&str],
    claude_args: &[&str],
) -> Result<String, String> {
    for key in unset {
        if !valid_env_key(key) {
            return Err(format!("invalid env name: {key}"));
        }
    }
    let mut inner = String::from("exec env");
    for key in unset {
        inner.push_str(" -u ");
        inner.push_str(key);
    }
    for (key, value) in set {
        if !valid_env_key(key) || value.chars().any(|c| c.is_control()) {
            return Err(format!("invalid env for {key}"));
        }
        inner.push(' ');
        inner.push_str(key);
        inner.push('=');
        inner.push_str(&shell_single_quote(value));
    }
    // 参数是给 claude 的；命令名在这里补上，调用方不用再写一遍。
    inner.push_str(" claude");
    for arg in claude_args {
        if arg.chars().any(|c| c.is_control()) {
            return Err("命令参数里不能有控制字符".to_string());
        }
        inner.push(' ');
        inner.push_str(&shell_word(arg));
    }
    let quoted = shell_single_quote(&inner);
    Ok(format!(
        "for flag in -lic -lc -c; do if \"${{SHELL:-sh}}\" \"$flag\" 'command -v claude' >/dev/null 2>&1; then exec \"${{SHELL:-sh}}\" \"$flag\" {quoted}; fi; done; echo 'claude: not found' >&2; exit 127"
    ))
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn shell_word(value: &str) -> String {
    let safe = !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '+' | ',')
        });
    if safe {
        value.to_string()
    } else {
        shell_single_quote(value)
    }
}

fn cmd_quote(value: &str) -> Result<String, String> {
    if value
        .chars()
        .any(|c| c.is_control() || matches!(c, '%' | '"' | '^'))
    {
        return Err(format!("路径里有 Windows 终端传不过去的字符: {value}"));
    }
    validate_linux_abs(value)?;
    let plain = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '='));
    if plain {
        Ok(value.to_string())
    } else {
        Ok(format!("\"{value}\""))
    }
}

fn cmd_double_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_wsl_home_and_root_targets() {
        match from_override_path(r"\\wsl.localhost\Ubuntu\home\me\.claude") {
            RunTarget::Wsl(wsl) => {
                assert_eq!(wsl.host, "wsl.localhost");
                assert_eq!(wsl.distro, "Ubuntu");
                assert_eq!(wsl.linux_home, "/home/me");
            }
            RunTarget::Native => panic!("expected WSL target"),
        }
        match from_override_path(r"//wsl$/Ubuntu-22.04/root/.claude") {
            RunTarget::Wsl(wsl) => {
                assert_eq!(wsl.host, "wsl$");
                assert_eq!(wsl.distro, "Ubuntu-22.04");
                assert_eq!(wsl.linux_home, "/root");
            }
            RunTarget::Native => panic!("expected WSL root target"),
        }
        assert!(matches!(
            from_override_path(r"\\?\UNC\wsl.localhost\Debian\home\Ada\.claude"),
            RunTarget::Wsl(wsl) if wsl.distro == "Debian" && wsl.linux_home == "/home/Ada"
        ));
        assert!(matches!(
            from_override_path(r"C:\Users\me\.claude"),
            RunTarget::Native
        ));
        assert!(matches!(
            from_override_path(r"\\wsl$\Ubuntu\opt\claude"),
            RunTarget::Native
        ));
        assert!(matches!(
            from_override_path(r"\\wsl.localhost\..\home\me"),
            RunTarget::Native
        ));
    }

    #[test]
    fn account_root_stays_inside_the_linux_home() {
        assert_eq!(
            account_root_unc(r"\\wsl.localhost\Ubuntu\home\me\.claude").as_deref(),
            Some(r"\\wsl.localhost\Ubuntu\home\me\.cc-switch\claude-accounts")
        );
        assert_eq!(
            account_root_unc(r"\\wsl$\Ubuntu\root\.claude").as_deref(),
            Some(r"\\wsl$\Ubuntu\root\.cc-switch\claude-accounts")
        );
        assert_eq!(account_root_unc(r"D:\claude"), None);
        let root = account_root_unc(r"\\wsl.localhost\Ubuntu\home\me\.claude").unwrap();
        let config = format!(r"{root}\0123456789abcdef0123456789abcdef\config");
        assert_eq!(
            windows_path_to_linux(&config, "Ubuntu").unwrap(),
            "/home/me/.cc-switch/claude-accounts/0123456789abcdef0123456789abcdef/config"
        );
    }

    #[test]
    fn converts_unc_and_drive_paths() {
        assert_eq!(
            windows_path_to_linux(r"\\wsl.localhost\Ubuntu\home\me\proj", "Ubuntu").unwrap(),
            "/home/me/proj"
        );
        assert_eq!(
            windows_path_to_linux(r"\\wsl.localhost\ubuntu\home\me\proj", "Ubuntu").unwrap(),
            "/home/me/proj"
        );
        assert_eq!(
            windows_path_to_linux(r"C:\Users\me\My Proj", "Ubuntu").unwrap(),
            "/mnt/c/Users/me/My Proj"
        );
        assert_eq!(
            windows_path_to_linux(r"\\?\C:\work", "Ubuntu").unwrap(),
            "/mnt/c/work"
        );
        assert_eq!(windows_path_to_linux("D:/", "Ubuntu").unwrap(), "/mnt/d");
        assert!(windows_path_to_linux(r"\\wsl.localhost\Debian\home\me", "Ubuntu").is_err());
        assert!(windows_path_to_linux(r"\\wsl$\Ubuntu\home\me\..\etc", "Ubuntu").is_err());
        assert!(windows_path_to_linux(r"\\server\share\file", "Ubuntu").is_err());
    }

    #[test]
    fn login_argv_sets_linux_config_dir_and_unsets_after_the_shell_rc() {
        let argv = wsl_claude_argv(
            "Ubuntu-22.04",
            "/home/me",
            &[(
                "CLAUDE_CONFIG_DIR",
                "/home/me/.cc-switch/claude-accounts/abc/config",
            )],
            &["ANTHROPIC_API_KEY", "CLAUDE_CODE_USE_BEDROCK"],
            &["auth", "login", "--claudeai"],
        )
        .unwrap();
        assert_eq!(
            &argv[..7],
            ["-d", "Ubuntu-22.04", "--cd", "/home/me", "--", "sh", "-c",]
        );
        let script = argv.last().unwrap();
        assert!(script.contains("command -v claude"));
        assert!(script.contains("-u ANTHROPIC_API_KEY"));
        assert!(script.contains("-u CLAUDE_CODE_USE_BEDROCK"));
        assert!(script.contains("/home/me/.cc-switch/claude-accounts/abc/config"));
        assert!(script.contains("CLAUDE_CONFIG_DIR="));
        assert!(script.contains("claude auth login --claudeai"));
        assert!(script.contains("for flag in -lic -lc -c"));
        assert!(wsl_claude_argv("Ubuntu;id", "/home/me", &[], &[], &["claude"]).is_err());
        assert!(wsl_claude_argv("-Ubuntu", "/home/me", &[], &[], &["claude"]).is_err());
        assert!(wsl_claude_argv("Ubuntu", "home/me", &[], &[], &["claude"]).is_err());
        assert!(wsl_claude_argv(
            "Ubuntu",
            "/home/me",
            &[("CLAUDE_CONFIG_DIR", "/tmp/bad\npath")],
            &[],
            &["claude"],
        )
        .is_err());
    }

    #[test]
    fn terminal_line_quotes_spaces_and_rejects_batch_metacharacters() {
        let line = wsl_terminal_batch_line(
            "Ubuntu",
            "/home/Jane Doe/proj",
            &[(
                "CLAUDE_CONFIG_DIR",
                "/home/Jane Doe/.cc-switch/claude-accounts/abc/config",
            )],
            &["ANTHROPIC_API_KEY"],
            &["--dangerously-skip-permissions".to_string()],
        )
        .unwrap();
        assert!(line.starts_with("wsl.exe -d Ubuntu --cd \"/home/Jane Doe/proj\" -- sh -c "));
        assert!(line.contains("-u ANTHROPIC_API_KEY"));
        assert!(line.contains("/home/Jane Doe/.cc-switch/claude-accounts/abc/config"));
        assert!(line.contains("CLAUDE_CONFIG_DIR="));
        assert!(line.contains("claude --dangerously-skip-permissions"));

        let default_slot =
            wsl_terminal_batch_line("Ubuntu", "/root", &[], &["CLAUDE_CONFIG_DIR"], &[]).unwrap();
        assert!(default_slot.contains("-u CLAUDE_CONFIG_DIR"));
        assert!(!default_slot.contains("CLAUDE_CONFIG_DIR="));
        assert!(default_slot.starts_with("wsl.exe -d Ubuntu --cd /root -- sh -c "));

        assert!(wsl_terminal_batch_line("Ubuntu & calc", "/home/me", &[], &[], &[],).is_err());
        assert!(wsl_terminal_batch_line("Ubuntu", "/home/me/%PATH%", &[], &[], &[],).is_err());
    }

    /// 生成的 shell 真的会清掉变量、把 Linux 路径交给 `claude`。用假的 `claude` 跑，不依赖 WSL。
    #[cfg(unix)]
    #[test]
    fn shell_payload_reaches_a_fake_claude_with_the_linux_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let fake = bin.join("claude");
        std::fs::write(
            &fake,
            "#!/bin/sh\n[ -n \"$ANTHROPIC_API_KEY\" ] && echo LEAK && exit 1\nprintf '%s\\n' \"$CLAUDE_CONFIG_DIR\"\nprintf '%s\\n' \"$1\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let argv = wsl_claude_argv(
            "Ubuntu",
            "/home/me",
            &[(
                "CLAUDE_CONFIG_DIR",
                "/home/me/.cc-switch/claude-accounts/abc/config",
            )],
            &["ANTHROPIC_API_KEY"],
            &["auth", "login", "--claudeai"],
        )
        .unwrap();
        let script = argv.last().unwrap();
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .env("PATH", &path)
            .env("SHELL", "/bin/sh")
            .env("ANTHROPIC_API_KEY", "secret")
            .env_remove("ENV")
            .env_remove("BASH_ENV")
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            output.status.success(),
            "status {:?}\nscript {script}\nstdout {stdout}\nstderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            stdout.contains("/home/me/.cc-switch/claude-accounts/abc/config"),
            "{stdout}"
        );
        assert!(stdout.contains("auth"), "{stdout}");
        assert!(!stdout.contains("LEAK"), "{stdout}");
    }
}

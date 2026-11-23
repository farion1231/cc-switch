use super::*;

/// 探测 helper 正常路径：spawn（含 pre_exec setsid）能启动、输出能捕获。
/// `/bin/echo --version` 在 macOS/Linux 均即刻成功退出。
#[cfg(not(target_os = "windows"))]
#[test]
fn probe_version_command_captures_healthy_tool_output() {
    let out = run_probe_version_command(Path::new("/bin/echo"), std::ffi::OsStr::new(""))
        .expect("probe of /bin/echo should succeed");
    assert!(out.status.success());
}

#[test]
fn legacy_kimi_native_update_reruns_the_installer() {
    let install = |path: &str, version: &str| ToolInstallation {
        path: path.to_string(),
        version: Some(version.to_string()),
        runnable: true,
        error: None,
        exited_with_error: false,
        source: infer_install_source(Path::new(path)).to_string(),
        is_path_default: true,
        real: PathBuf::from(path),
    };
    let (path, root) = if cfg!(target_os = "windows") {
        (
            r"C:\Users\sc\.kimi-code\bin\kimi.exe",
            r"C:\Users\sc\.kimi-code",
        )
    } else {
        ("/home/me/.kimi-code/bin/kimi", "/home/me/.kimi-code")
    };

    // 0.43.0 之前 `upgrade --yes` 在非 TTY 下只打印提示就 exit 0（#9032）。
    assert_eq!(
        installs_anchored_command("kimi", &[install(path, "0.23.1")]),
        Some(kimi_installer_update_command(root))
    );
    // 新版照旧走 CLI 自升级。
    let current = installs_anchored_command("kimi", &[install(path, "2.4.0")]).unwrap();
    assert!(current.contains("upgrade --yes"), "{current}");
    assert_eq!(
        installs_anchored_command("kimi", &[install(path, "0.43.0")]),
        Some(current)
    );
    // Volta 的 shim 也是 `bin/kimi`，必须交给 Volta，不能让 installer 覆盖。
    #[cfg(not(target_os = "windows"))]
    {
        let volta =
            installs_anchored_command("kimi", &[install("/home/me/.volta/bin/kimi", "0.23.1")])
                .unwrap();
        assert!(
            volta.ends_with("volta install @moonshot-ai/kimi-code"),
            "{volta}"
        );
    }
}

/// 锚定落空后的兜底:认不出渠道的原生可执行文件不再退回裸 `npm i -g`(#7650),
/// 脚本 shim / node_modules 里的入口 / 定位不到默认那处仍保持旧的静态命令。
mod unmanaged_native_update {
    use super::super::*;

    const MACH_O_64: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];

    /// 在 tempdir 的 `rel` 位置写入 `bytes`,返回指向它的安装记录。TempDir 须保活。
    fn installed(
        dir: &tempfile::TempDir,
        rel: &str,
        bytes: &[u8],
        is_default: bool,
    ) -> ToolInstallation {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        ToolInstallation {
            path: path.to_string_lossy().to_string(),
            version: Some("1.0.0".to_string()),
            runnable: true,
            error: None,
            exited_with_error: false,
            source: infer_install_source(&path).to_string(),
            is_path_default: is_default,
            real: path,
        }
    }

    #[test]
    fn native_binary_outside_node_modules_is_unmanaged() {
        let dir = tempfile::tempdir().unwrap();
        let gemini = installed(&dir, "nix-profile/bin/gemini", &MACH_O_64, true);
        assert_eq!(
            resolve_update_command("gemini", &[gemini]),
            UpdateCommand::Unmanaged
        );
        let pi = installed(&dir, "bin/pi", b"\x7fELF\x02\x01\x01", true);
        assert_eq!(
            resolve_update_command("pi", &[pi]),
            UpdateCommand::Unmanaged
        );
    }

    #[test]
    fn script_shim_keeps_static_npm_command() {
        // asdf / nodenv 的 shim 是脚本,背后可能正是 npm 全局包,不能拒绝。
        let dir = tempfile::tempdir().unwrap();
        let shim = installed(
            &dir,
            ".asdf/shims/gemini",
            b"#!/usr/bin/env bash\nexec asdf exec gemini \"$@\"\n",
            true,
        );
        assert_eq!(
            resolve_update_command("gemini", &[shim]),
            UpdateCommand::Static(static_fallback_command("gemini"))
        );
    }

    #[test]
    fn native_binary_inside_node_modules_keeps_static_npm_command() {
        // 有的 npm 包把原生二进制放在包内,真身仍在 node_modules 下,归 npm 管。
        let dir = tempfile::tempdir().unwrap();
        let bundled = installed(
            &dir,
            "prefix/lib/node_modules/@google/gemini-cli/bin/gemini",
            &MACH_O_64,
            true,
        );
        assert_eq!(
            resolve_update_command("gemini", &[bundled]),
            UpdateCommand::Static(static_fallback_command("gemini"))
        );
    }

    #[test]
    fn unknown_default_keeps_static_command() {
        // 多处安装又定位不到默认那处:不知道命令行用的是哪份,维持旧行为(前端会弹确认)。
        let dir = tempfile::tempdir().unwrap();
        let a = installed(&dir, "a/bin/gemini", &MACH_O_64, false);
        let b = installed(&dir, "b/bin/gemini", &MACH_O_64, false);
        assert_eq!(
            resolve_update_command("gemini", &[a, b]),
            UpdateCommand::Static(static_fallback_command("gemini"))
        );
    }

    #[test]
    fn native_executable_magic() {
        let dir = tempfile::tempdir().unwrap();
        let cases: [(&str, &[u8], bool); 7] = [
            ("elf", b"\x7fELF\x02", true),
            ("macho", &MACH_O_64, true),
            ("fat", &[0xca, 0xfe, 0xba, 0xbe], true),
            ("pe.exe", b"MZ\x90\x00", true),
            ("script", b"#!/bin/sh\n", false),
            ("cmd-shim.cmd", b"@ECHO off\r\n", false),
            ("short", b"MZ", false),
        ];
        for (name, bytes, expected) in cases {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(is_native_executable(&path), expected, "{name}");
        }
        assert!(!is_native_executable(&dir.path().join("missing")));
    }
}

/// `infer_install_source` 是判定锚定 idiom 的入口——nvm/homebrew/volta/pnpm/...
/// 各对应不同的升级命令形态。函数内部已 `replace('\\','/').to_ascii_lowercase()`
/// 归一化,Windows 反斜杠 + 大小写差异在此处不需要分平台。这里固化"哪条路径
/// 算哪种来源"的归类断言,避免未来调整子串顺序时静默改变分类。
mod install_source_classification {
    use super::super::*;
    use std::path::Path;

    #[test]
    fn macos_homebrew_caskroom_is_homebrew() {
        assert_eq!(
            infer_install_source(Path::new("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex")),
            "homebrew"
        );
        // Intel prefix 不含 `/homebrew/`，必须靠 `/caskroom/` 本身命中。
        assert_eq!(
            infer_install_source(Path::new("/usr/local/Caskroom/codex/0.146.0/bin/codex")),
            "homebrew"
        );
    }

    #[test]
    fn intel_cask_launcher_uses_resolved_target() {
        // 标准 Intel cask：PATH 入口是 `/usr/local/bin/codex`，真身才在 Caskroom。
        // 只看 launcher 会落到 `system`，徽章和冲突诊断都会错。
        assert_eq!(
            infer_install_source(Path::new("/usr/local/bin/codex")),
            "system"
        );
        assert_eq!(
            infer_install_source_for_install(
                Path::new("/usr/local/bin/codex"),
                Path::new("/usr/local/Caskroom/codex/0.146.0/bin/codex"),
            ),
            "homebrew"
        );
        // nvm shim 仍按 launcher 分类，不能被真身路径抢走。
        assert_eq!(
            infer_install_source_for_install(
                Path::new("/Users/me/.nvm/versions/node/v22.0.0/bin/codex"),
                Path::new(
                    "/Users/me/.nvm/versions/node/v22.0.0/lib/node_modules/@openai/codex/bin/codex.js"
                ),
            ),
            "nvm"
        );
    }

    #[test]
    fn macos_volta_with_dot_prefix() {
        assert_eq!(
            infer_install_source(Path::new("/Users/me/.volta/bin/codex")),
            "volta"
        );
    }

    #[test]
    fn windows_volta_localappdata_no_dot() {
        // `%LOCALAPPDATA%\Volta\bin\codex.exe` —— 没有前导点,靠兜底的 `/volta/`
        // 命中(归一化后小写)。如果只识别 `/.volta/`,Windows 这一类会落到 system。
        assert_eq!(
            infer_install_source(Path::new(
                "C:\\Users\\me\\AppData\\Local\\Volta\\bin\\codex.exe"
            )),
            "volta"
        );
    }

    #[test]
    fn windows_pnpm_localappdata() {
        // `%LOCALAPPDATA%\pnpm\codex.cmd` —— pnpm 全局 bin 目录,识别为 pnpm 后
        // 锚定命令走 `pnpm add -g <pkg>@latest`,而不是 sibling npm。
        assert_eq!(
            infer_install_source(Path::new("C:\\Users\\me\\AppData\\Local\\pnpm\\codex.cmd")),
            "pnpm"
        );
    }

    #[test]
    fn windows_nvm_falls_back_to_system() {
        // nvm-windows 安装的工具路径不含 `.nvm`(它通常装在 `%APPDATA%\nvm` 或
        // `C:\Program Files\nodejs` symlink),刻意不识别成专属 source——锚定层
        // 会按 system → sibling npm.cmd 处理,跟 nvm-windows 的实际 idiom 一致
        // (它的全局包就是当前选中的 node 的 npm 装的)。
        assert_eq!(
            infer_install_source(Path::new(
                "C:\\Users\\me\\AppData\\Roaming\\nvm\\v22.0.0\\codex.cmd"
            )),
            "system"
        );
    }

    #[test]
    fn windows_scoop_still_identified() {
        // Scoop 已有 `/scoop/` 分支;我们的 6 个工具都不是 scoop formula,所以这条
        // 实际不影响锚定决策(锚定层会用 sibling npm.cmd),但归类保留方便未来。
        assert_eq!(
            infer_install_source(Path::new("C:\\Users\\me\\scoop\\shims\\codex.cmd")),
            "scoop"
        );
    }
}

/// 锚定升级命令生成：用真实勘察到的安装路径固化为回归断言——
/// 一台机器上 4 个工具恰好对应 4 种升级方式（原生 self-update / brew / nvm npm /
/// homebrew npm），任何改动若打破其中一种都会立刻被这些用例拦下。
#[cfg(not(target_os = "windows"))]
mod anchored_upgrade {
    use super::super::*;
    use std::path::Path;

    fn inst(path: &str, is_default: bool) -> ToolInstallation {
        ToolInstallation {
            path: path.to_string(),
            version: None,
            runnable: true,
            error: None,
            exited_with_error: false,
            source: infer_install_source(Path::new(path)).to_string(),
            is_path_default: is_default,
            // 测试场景下不需要走 fs canonicalize——POSIX 锚定测试关心的是
            // path/real 都被传给 anchored_command_from_paths 的纯字符串判定,
            // 已有用例(brew_formula_extraction / claude_native_*)是直接
            // 调 anchored_command_from_paths,不通过 installs_anchored_command,
            // 这里 real 是给上层 default_install + read 用,填同值即可。
            real: std::path::PathBuf::from(path),
        }
    }

    #[test]
    fn claude_native_installer_uses_self_update() {
        // ~/.local/bin/claude → 真身在 ~/.local/share/claude/versions/,自带 self-update;
        // 它不归 npm 管,且在 PATH 里比 nvm/homebrew 更靠前,用 npm 升级纯属白装。
        // **绝对路径调用 launcher** 避免 GUI 非登录 `bash -c` 时 PATH 没有
        // ~/.local/bin 导致 `claude: not found`(exit 127)而失败。
        let cmd = anchored_command_from_paths(
            "claude",
            "/Users/me/.local/bin/claude",
            "/Users/me/.local/share/claude/versions/2.1.146",
        );
        assert_eq!(cmd.as_deref(), Some("/Users/me/.local/bin/claude update"));
    }

    #[test]
    fn grok_native_installer_uses_self_update_with_installer_fallback() {
        // ~/.grok/bin/grok is a launcher symlink into ~/.grok/downloads.
        // Updating it through npm would create or mutate a different install.
        let cmd = anchored_command_from_paths(
            "grok",
            "/Users/me/.grok/bin/grok",
            "/Users/me/.grok/downloads/grok-macos-aarch64",
        );
        assert_eq!(
            cmd.as_deref(),
            Some(
                format!(
                    "npm_config_allow_scripts=@xai-official/grok npm_config_ignore_scripts=false npm_config_include=optional /Users/me/.grok/bin/grok update || {GROK_INSTALL_UNIX}"
                )
                .as_str()
            )
        );
    }

    #[test]
    fn grok_native_update_falls_back_to_installer_not_npm() {
        // 反向锁定:`grok update` 内部靠 `npm view` + `npm i -g` 完成升级,GUI 的窄
        // PATH 下会 ENOENT。fallback 必须是官方 installer —— 换成 `npm i -g` 就与
        // primary 同源(无 node / 镜像缺 tarball 时一起失败),`||` 形同虚设。
        let cmd = anchored_command_from_paths(
            "grok",
            "/Users/me/.grok/bin/grok",
            "/Users/me/.grok/downloads/grok-macos-aarch64",
        )
        .expect("native grok should anchor");
        let (_, fallback) = cmd.split_once(" || ").unwrap();
        assert!(fallback.contains("x.ai/cli/install.sh"), "{cmd}");
        assert!(
            !fallback.contains("npm"),
            "npm must not be the fallback: {cmd}"
        );
    }

    #[test]
    fn grok_custom_bin_dir_is_native_when_target_is_official_download() {
        let cmd = anchored_command_from_paths(
            "grok",
            "/Users/me/bin/grok",
            "/Users/me/.grok/downloads/grok-macos-aarch64",
        );
        assert_eq!(
            cmd.as_deref(),
            Some(
                format!(
                    "npm_config_allow_scripts=@xai-official/grok npm_config_ignore_scripts=false npm_config_include=optional /Users/me/bin/grok update || {GROK_INSTALL_UNIX}"
                )
                .as_str()
            )
        );
    }

    #[test]
    fn gemini_homebrew_formula_uses_brew_upgrade() {
        // /opt/homebrew/bin/gemini → Cellar/gemini-cli/...:是 brew formula 而非 npm 全局包,
        // 且 formula 名(gemini-cli) ≠ npm 包名(@google/gemini-cli)。
        // **brew 与 formula 入口同目录**,用 `<dir>/brew` 绝对路径调用,避免 GUI
        // 非登录 `bash -c` 时 PATH 没有 /opt/homebrew/bin 导致 `brew: not found`。
        let cmd = anchored_command_from_paths(
            "gemini",
            "/opt/homebrew/bin/gemini",
            "/opt/homebrew/Cellar/gemini-cli/0.13.0/libexec/lib/node_modules/@google/gemini-cli/dist/index.js",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("/opt/homebrew/bin/brew upgrade gemini-cli")
        );
    }

    #[test]
    fn codex_homebrew_formula_uses_brew_not_self_update() {
        // Homebrew formula 归 brew 管理;即使 Codex 有 self-update,也不先改动
        // Cellar 内的安装内容。
        let cmd = anchored_command_from_paths(
            "codex",
            "/opt/homebrew/bin/codex",
            "/opt/homebrew/Cellar/codex/1.2.3/bin/codex",
        );
        assert_eq!(cmd.as_deref(), Some("/opt/homebrew/bin/brew upgrade codex"));
    }

    #[test]
    fn codex_homebrew_cask_uses_brew_upgrade_cask() {
        // `/opt/homebrew/bin/codex` → Caskroom/codex/...:是 brew cask 而非 npm 全局包。
        // 若误走 sibling `npm i -g @openai/codex`，npm bin-links 会因
        // `/opt/homebrew/bin/codex` 不属于它而 EEXIST（#6562）。
        let cmd = anchored_command_from_paths(
            "codex",
            "/opt/homebrew/bin/codex",
            "/opt/homebrew/Caskroom/codex/0.146.0/bin/codex",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("/opt/homebrew/bin/brew upgrade --cask codex")
        );
    }

    #[test]
    fn intel_homebrew_cask_without_homebrew_prefix_still_uses_brew() {
        // Intel 默认 prefix 是 `/usr/local`，Caskroom 路径不含 `/homebrew/`。
        // 真身解析必须靠 `Caskroom` 段本身，不能依赖 prefix 子串。
        let cmd = anchored_command_from_paths(
            "codex",
            "/usr/local/bin/codex",
            "/usr/local/Caskroom/codex/0.146.0/bin/codex",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("/usr/local/bin/brew upgrade --cask codex")
        );
    }

    #[test]
    fn gemini_nvm_anchors_to_npm_without_cli_update() {
        let cmd = anchored_command_from_paths(
            "gemini",
            "/Users/me/.nvm/versions/node/v22.14.0/bin/gemini",
            "/Users/me/.nvm/versions/node/v22.14.0/lib/node_modules/@google/gemini-cli/dist/index.js",
        );
        assert_eq!(
            cmd.as_deref(),
            Some(
                "PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @google/gemini-cli@latest"
            )
        );
    }

    #[test]
    fn grok_nvm_anchors_to_npm_without_cli_update() {
        let cmd = anchored_command_from_paths(
            "grok",
            "/Users/me/.nvm/versions/node/v22.14.0/bin/grok",
            "/Users/me/.nvm/versions/node/v22.14.0/lib/node_modules/@xai-official/grok/bin/grok",
        );
        assert_eq!(
            cmd.as_deref(),
            Some(
                "PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @xai-official/grok@latest --ignore-scripts=false --include=optional --allow-scripts=@xai-official/grok"
            )
        );
    }

    #[test]
    fn mcode_script_install_reruns_official_installer_in_place() {
        // `~/.minimax-code/bin/mcode` 是官方 installer 的 sh launcher，不归全局 npm 管；
        // `mcode update` 非 TTY 下不安装，所以改为带 MCODE_INSTALL_DIR 重跑 installer。
        let expected = format!("MCODE_INSTALL_DIR='/Users/me/.minimax-code' {MCODE_INSTALL_UNIX}");
        for bin_path in [
            "/Users/me/.minimax-code/bin/mcode",
            "/Users/me/.local/bin/mcode",
        ] {
            let cmd =
                anchored_command_from_paths("mcode", bin_path, "/Users/me/.minimax-code/bin/mcode");
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }
    }

    #[test]
    fn mcode_nvm_anchors_to_npm_with_allow_scripts() {
        let cmd = anchored_command_from_paths(
            "mcode",
            "/Users/me/.nvm/versions/node/v22.19.0/bin/mcode",
            "/Users/me/.nvm/versions/node/v22.19.0/lib/node_modules/@minimax-ai/code/cli.js",
        );
        assert_eq!(
            cmd.as_deref(),
            Some(
                "PATH='/Users/me/.nvm/versions/node/v22.19.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.19.0/bin/npm i -g @minimax-ai/code@latest --ignore-scripts=false --include=optional \"--allow-scripts=@minimax-ai/code,better-sqlite3\""
            )
        );
    }

    #[test]
    fn claude_npm_sources_scope_postinstall_settings_to_primary_and_full_fallback() {
        for (path, dir) in [
            (
                "/Users/me/.nvm/versions/node/v22/bin/claude",
                "/Users/me/.nvm/versions/node/v22/bin",
            ),
            (
                "/Users/me/.local/share/fnm_multishells/12345_abc/bin/claude",
                "/Users/me/.local/share/fnm_multishells/12345_abc/bin",
            ),
            (
                "/Users/me/.local/share/mise/installs/node/22/bin/claude",
                "/Users/me/.local/share/mise/installs/node/22/bin",
            ),
            ("/opt/homebrew/bin/claude", "/opt/homebrew/bin"),
        ] {
            let mut install = inst(path, true);
            install.real = PathBuf::from(format!(
                "{}/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
                dir.trim_end_matches("/bin")
            ));
            let command = installs_anchored_command("claude", &[install]).unwrap();
            let (primary, fallback) = command.split_once(" || ").unwrap();
            assert_eq!(
                primary,
                format!(
                    "npm_config_allow_scripts=@anthropic-ai/claude-code npm_config_ignore_scripts=false npm_config_include=optional {path} update"
                )
            );
            assert_eq!(
                fallback,
                format!(
                    "PATH='{dir}':\"$PATH\" {dir}/npm i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code"
                )
            );
        }
    }

    #[test]
    fn opencode_npm_sources_scope_postinstall_settings_to_primary_and_fallback() {
        // opencode-ai's postinstall lays the platform binary over the text
        // placeholder at bin/opencode.exe; npm 12 blocks it by default (#8863).
        let dir = "/Users/me/.nvm/versions/node/v22/bin";
        let path = format!("{dir}/opencode");
        let mut install = inst(&path, true);
        install.real = PathBuf::from(
            "/Users/me/.nvm/versions/node/v22/lib/node_modules/opencode-ai/bin/opencode.exe",
        );
        let command = installs_anchored_command("opencode", &[install]).unwrap();
        let (primary, fallback) = command.split_once(" || ").unwrap();
        assert_eq!(
            primary,
            format!(
                "npm_config_allow_scripts=opencode-ai npm_config_ignore_scripts=false npm_config_include=optional {path} upgrade"
            )
        );
        assert_eq!(
            fallback,
            format!(
                "PATH='{dir}':\"$PATH\" {dir}/npm i -g opencode-ai@latest --ignore-scripts=false --include=optional --allow-scripts=opencode-ai"
            )
        );
        assert_eq!(
            anchored_command_from_paths(
                "opencode",
                "/usr/local/bin/opencode",
                "/usr/local/lib/node_modules/opencode-ai/bin/opencode.exe",
            ),
            Some(
                "npm_config_allow_scripts=opencode-ai npm_config_ignore_scripts=false npm_config_include=optional /usr/local/bin/opencode upgrade"
                    .to_string()
            )
        );
    }

    #[test]
    fn claude_system_npm_prefixes_scope_postinstall_settings_without_fallback() {
        // Launchers classified as `system` have no sibling-npm fallback, so the
        // primary update is their only chance to keep npm 12 from breaking Claude.
        for (path, real) in [
            (
                "/usr/local/bin/claude",
                "/usr/local/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
            ),
            (
                "/Users/me/.npm-global/bin/claude",
                "/Users/me/.npm-global/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
            ),
            (
                "/usr/bin/claude",
                "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
            ),
        ] {
            assert_eq!(
                anchored_command_from_paths("claude", path, real),
                Some(format!(
                    "npm_config_allow_scripts=@anthropic-ai/claude-code npm_config_ignore_scripts=false npm_config_include=optional {path} update"
                ))
            );
        }
    }

    #[test]
    fn claude_other_install_sources_keep_their_update_policy() {
        for (path, real, expected) in [
            (
                "/Users/me/.nvm/versions/node/v22/bin/claude",
                "/Users/me/.local/share/claude/versions/2.1.146",
                "/Users/me/.nvm/versions/node/v22/bin/claude update",
            ),
            (
                "/opt/homebrew/bin/claude",
                "/opt/homebrew/Cellar/claude-code/2.1.146/bin/claude",
                "/opt/homebrew/bin/brew upgrade claude-code",
            ),
            (
                "/Users/me/.volta/bin/claude",
                "/Users/me/.volta/bin/claude",
                "/Users/me/.volta/bin/claude update || /Users/me/.volta/bin/volta install @anthropic-ai/claude-code",
            ),
            (
                "/Users/me/.bun/bin/claude",
                "/Users/me/.bun/bin/claude",
                "/Users/me/.bun/bin/claude update || /Users/me/.bun/bin/bun add -g @anthropic-ai/claude-code@latest",
            ),
            (
                "/usr/local/bin/claude",
                "/usr/local/bin/claude",
                "/usr/local/bin/claude update",
            ),
        ] {
            assert_eq!(
                anchored_command_from_paths("claude", path, real).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn claude_updater_child_inherits_scoped_settings_without_parent_leakage() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let temp = tempfile::tempdir().expect("temp dir should be created");
        let bin = temp.path().join("home dir/.nvm/versions/node/v22/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        let npm = bin.join("npm");
        std::fs::write(
            &claude,
            "#!/bin/sh\n[ \"$1\" = update ] || exit 99\nnpm install -g @anthropic-ai/claude-code@latest\nexit \"$TEST_CLAUDE_EXIT\"\n",
        )
        .unwrap();
        std::fs::write(
            &npm,
            "#!/bin/sh\nprintf '%s|%s|%s|%s\n' \"\u{24}{npm_config_allow_scripts-unset}\" \"\u{24}{npm_config_ignore_scripts-unset}\" \"\u{24}{npm_config_include-unset}\" \"\u{24}*\"\n[ \"$1\" != i ] || exit \"$TEST_FALLBACK_EXIT\"\n",
        )
        .unwrap();
        for executable in [&claude, &npm] {
            std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let mut install = inst(&claude.to_string_lossy(), true);
        install.real = temp.path().join(
            "home dir/.nvm/versions/node/v22/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
        );
        let command = installs_anchored_command("claude", &[install]).unwrap();
        let script = format!(
            "set -e\n{command}\nprintf 'after|%s|%s|%s\n' \"$npm_config_allow_scripts\" \"$npm_config_ignore_scripts\" \"$npm_config_include\""
        );
        let primary = "@anthropic-ai/claude-code|false|optional|install -g @anthropic-ai/claude-code@latest\n";
        let fallback = "previous-package|true|prod|i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code\n";
        for (primary_exit, fallback_exit, expected_code, expected_output) in [
            (
                "0",
                "0",
                0,
                format!("{primary}after|previous-package|true|prod\n"),
            ),
            (
                "1",
                "0",
                0,
                format!("{primary}{fallback}after|previous-package|true|prod\n"),
            ),
            ("1", "42", 42, format!("{primary}{fallback}")),
        ] {
            let output = Command::new("/bin/bash")
                .args(["-c", &script])
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .env("npm_config_allow_scripts", "previous-package")
                .env("npm_config_ignore_scripts", "true")
                .env("npm_config_include", "prod")
                .env("TEST_CLAUDE_EXIT", primary_exit)
                .env("TEST_FALLBACK_EXIT", fallback_exit)
                .output()
                .expect("Claude update chain should start");
            assert_eq!(
                output.status.code(),
                Some(expected_code),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout), expected_output);
        }
    }

    #[test]
    fn codex_nvm_anchors_to_that_npm() {
        // Codex 不走 self-update（`codex update` 在 npm 安装上只是裸 `npm install -g`，
        // 却会假成功掩盖平台二进制漏装）——直接锚定到同一个 node 的 npm，而非 PATH
        // 第一个 npm。损坏时的 uninstall+install 自愈见 codex_missing_platform_binary_*。
        let cmd = anchored_command_from_paths(
            "codex",
            "/Users/me/.nvm/versions/node/v22.14.0/bin/codex",
            "/Users/me/.nvm/versions/node/v22.14.0/lib/node_modules/@openai/codex/bin/codex.js",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
        );
    }

    #[test]
    fn homebrew_npm_global_package_anchors_not_brew() {
        // openclaw 装在 Homebrew node 的全局目录(lib/node_modules，非 Cellar)：
        // 是 npm 全局包，官方 update 失败后走 npm 锚定而非 brew upgrade。
        let cmd = anchored_command_from_paths(
            "openclaw",
            "/opt/homebrew/bin/openclaw",
            "/opt/homebrew/lib/node_modules/openclaw/openclaw.mjs",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("/opt/homebrew/bin/openclaw update --yes || PATH='/opt/homebrew/bin':\"$PATH\" /opt/homebrew/bin/npm i -g openclaw@latest")
        );
    }

    #[test]
    fn volta_self_update_chain_anchors_to_volta() {
        // `~/.volta/bin` 通常不在 GUI 非登录 `bash -c` 的 PATH 里,且用户可能
        // PATH 上还有另一份 volta → 必须绝对路径锚定到命令行命中的这一份。
        // 用 openclaw（仍在 prefers_official_update）覆盖 volta 分支的 self-update 链;
        // codex 已改为不 self-update（见 codex_volta_anchors_to_volta_install）。
        let cmd = anchored_command_from_paths(
            "openclaw",
            "/Users/me/.volta/bin/openclaw",
            "/Users/me/.volta/tools/image/packages/openclaw/lib/node_modules/openclaw",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("/Users/me/.volta/bin/openclaw update --yes || /Users/me/.volta/bin/volta install openclaw")
        );
    }

    #[test]
    fn codex_volta_anchors_to_volta_install() {
        // codex 锚定到命令行命中的那份 volta，但不 self-update：纯 `volta install`。
        let cmd = anchored_command_from_paths(
            "codex",
            "/Users/me/.volta/bin/codex",
            "/Users/me/.volta/tools/image/packages/codex/lib/node_modules/@openai/codex",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("/Users/me/.volta/bin/volta install @openai/codex")
        );
    }

    #[test]
    fn bun_uses_bun_add() {
        // OpenCode 先跑官方 upgrade;失败后 bun 同 volta:绝对路径写回原安装源。
        let cmd = anchored_command_from_paths(
            "opencode",
            "/Users/me/.bun/bin/opencode",
            "/Users/me/.bun/install/global/node_modules/opencode-ai/bin/opencode",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("/Users/me/.bun/bin/opencode upgrade || /Users/me/.bun/bin/bun add -g opencode-ai@latest")
        );
    }

    #[test]
    fn omp_updates_through_its_own_updater_with_source_specific_fallbacks() {
        // Homebrew owns its formula even though `omp update` would also delegate.
        assert_eq!(
            anchored_command_from_paths(
                "omp",
                "/opt/homebrew/bin/omp",
                "/opt/homebrew/Cellar/omp/14.1.0/bin/omp",
            )
            .as_deref(),
            Some("/opt/homebrew/bin/brew upgrade omp")
        );
        // A bun global keeps bun as the fallback.
        assert_eq!(
            anchored_command_from_paths(
                "omp",
                "/Users/me/.bun/bin/omp",
                "/Users/me/.bun/install/global/node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts",
            )
            .as_deref(),
            Some("/Users/me/.bun/bin/omp update || /Users/me/.bun/bin/bun add -g @oh-my-pi/pi-coding-agent@latest")
        );
        // The installer's prebuilt binary updates itself; no npm fallback is invented.
        assert_eq!(
            anchored_command_from_paths(
                "omp",
                "/Users/me/.local/bin/omp",
                "/Users/me/.local/bin/omp",
            )
            .as_deref(),
            Some("/Users/me/.local/bin/omp update")
        );
        // npm and other node managers cannot run omp, so they never become a fallback.
        assert_eq!(
            anchored_command_from_paths(
                "omp",
                "/Users/me/.nvm/versions/node/v22.0.0/bin/omp",
                "/Users/me/.nvm/versions/node/v22.0.0/lib/node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts",
            )
            .as_deref(),
            Some("/Users/me/.nvm/versions/node/v22.0.0/bin/omp update")
        );
        // `omp update` exits 0 on Nix without updating: leave it to the user.
        assert_eq!(
            anchored_command_from_paths(
                "omp",
                "/Users/me/.nix-profile/bin/omp",
                "/nix/store/abc-omp-14.1.0/bin/omp",
            ),
            None
        );
    }

    #[test]
    fn volta_path_with_space_is_quoted() {
        // volta 分支用 `<dir>/volta`,目录含空格时同样要 POSIX 引号包裹。
        let cmd = anchored_command_from_paths(
            "codex",
            "/Users/my name/.volta/bin/codex",
            "/Users/my name/.volta/tools/image/packages/codex/lib/node_modules/@openai/codex",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("'/Users/my name/.volta/bin/volta' install @openai/codex")
        );
    }

    #[test]
    fn bun_path_with_space_is_quoted() {
        // bun 分支与 volta 共享 sibling_bin + quote_path_if_spaced,
        // 这条用例锁住 `bun add -g` 命令头部的引号包裹形态。
        let cmd = anchored_command_from_paths(
            "opencode",
            "/Users/my name/.bun/bin/opencode",
            "/Users/my name/.bun/install/global/node_modules/opencode-ai/bin/opencode",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("'/Users/my name/.bun/bin/opencode' upgrade || '/Users/my name/.bun/bin/bun' add -g opencode-ai@latest")
        );
    }

    #[test]
    fn hermes_uses_cli_update_anchor() {
        // Hermes 自带 `hermes update`;锚定到命令行默认那处 CLI,避免 cc-switch 猜
        // 系统 Python/pip 时撞上 Python >=3.11 或 pyenv shim 问题。
        let cmd =
            anchored_command_from_paths("hermes", "/usr/local/bin/hermes", "/usr/local/bin/hermes");
        assert_eq!(cmd.as_deref(), Some("/usr/local/bin/hermes update"));
    }

    #[test]
    fn agy_updates_itself_even_from_a_homebrew_cask() {
        // The cask is auto_updates, so `brew upgrade` skips it; agy replaces its own binary.
        for real in [
            "/opt/homebrew/Caskroom/antigravity-cli/1.3.4,5524158944247808/antigravity",
            "/opt/homebrew/bin/agy",
        ] {
            let cmd = anchored_command_from_paths("agy", "/opt/homebrew/bin/agy", real);
            assert_eq!(cmd.as_deref(), Some("/opt/homebrew/bin/agy update"));
        }
    }

    #[test]
    fn opencode_native_install_uses_cli_upgrade_without_package_fallback() {
        // opencode install.sh 装到 ~/.opencode/bin（独立二进制、无同级 npm）：
        // 不能锚定到 `<dir>/npm`（必失败），但可以锚定到 CLI 自身跑官方 upgrade。
        let cmd = anchored_command_from_paths(
            "opencode",
            "/Users/me/.opencode/bin/opencode",
            "/Users/me/.opencode/bin/opencode",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("/Users/me/.opencode/bin/opencode upgrade")
        );
    }

    #[test]
    fn go_bin_opencode_uses_cli_upgrade_without_package_fallback() {
        // ~/go/bin 同理：无同级 npm，但 OpenCode 官方 upgrade 可由 CLI 自己处理。
        let cmd = anchored_command_from_paths(
            "opencode",
            "/Users/me/go/bin/opencode",
            "/Users/me/go/bin/opencode",
        );
        assert_eq!(cmd.as_deref(), Some("/Users/me/go/bin/opencode upgrade"));
    }

    #[test]
    fn fnm_install_anchors_to_that_npm() {
        // fnm 是自带同级 npm 的 node 管理器 → 锚定到那处的 npm。
        let cmd = anchored_command_from_paths(
            "codex",
            "/Users/me/.local/share/fnm_multishells/12345_abc/bin/codex",
            "/Users/me/.local/share/fnm_multishells/12345_abc/lib/node_modules/@openai/codex/bin/codex.js",
        );
        assert_eq!(
            cmd.as_deref(),
            Some(
                "PATH='/Users/me/.local/share/fnm_multishells/12345_abc/bin':\"$PATH\" /Users/me/.local/share/fnm_multishells/12345_abc/bin/npm i -g @openai/codex@latest"
            )
        );
    }

    #[test]
    fn path_with_space_is_quoted() {
        let cmd = anchored_command_from_paths(
            "codex",
            "/Users/my name/.nvm/versions/node/v22/bin/codex",
            "/Users/my name/.nvm/versions/node/v22/lib/node_modules/@openai/codex/bin/codex.js",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("PATH='/Users/my name/.nvm/versions/node/v22/bin':\"$PATH\" '/Users/my name/.nvm/versions/node/v22/bin/npm' i -g @openai/codex@latest")
        );
    }

    #[test]
    fn npm_anchor_supplies_sibling_node_to_env_shebang() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let temp = tempfile::tempdir().expect("temp dir should be created");
        let bin = temp.path().join("home dir/.nvm/versions/node/v22.14.0/bin");
        std::fs::create_dir_all(&bin).expect("node bin should be created");

        let marker = temp.path().join("sibling-node-used");
        let node = bin.join("node");
        let npm = bin.join("npm");
        std::fs::write(
            &node,
            format!(
                "#!/bin/sh\nprintf sibling-node > {}\n",
                shell_single_quote(&marker.to_string_lossy())
            ),
        )
        .expect("fake node should be written");
        std::fs::write(&npm, "#!/usr/bin/env node\n").expect("fake npm should be written");
        for executable in [&node, &npm] {
            let mut permissions = std::fs::metadata(executable)
                .expect("fake executable metadata should exist")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(executable, permissions)
                .expect("fake executable should be executable");
        }

        let codex = bin.join("codex").to_string_lossy().into_owned();
        let real = temp
            .path()
            .join(
                "home dir/.nvm/versions/node/v22.14.0/lib/node_modules/@openai/codex/bin/codex.js",
            )
            .to_string_lossy()
            .into_owned();
        let command = anchored_command_from_paths("codex", &codex, &real)
            .expect("nvm codex should produce an anchored npm command");
        let output = Command::new("/bin/bash")
            .args(["-c", &command])
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("anchored npm command should start");

        assert!(
            output.status.success(),
            "anchored npm command failed: {}",
            decode_command_output(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(marker).expect("sibling node should leave a marker"),
            "sibling-node"
        );
    }

    #[test]
    fn claude_native_path_with_space_is_quoted() {
        // claude 分支同样要 POSIX 引号包裹含空格的 bin_path,
        // 否则 `/Users/my name/.local/bin/claude update` 会被 shell 拆词。
        let cmd = anchored_command_from_paths(
            "claude",
            "/Users/my name/.local/bin/claude",
            "/Users/my name/.local/share/claude/versions/2.1.146",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("'/Users/my name/.local/bin/claude' update")
        );
    }

    #[test]
    fn brew_path_with_space_is_quoted() {
        // brew 分支用 `<bin_path 同目录>/brew`,目录含空格时同样要引号包裹。
        let cmd = anchored_command_from_paths(
            "gemini",
            "/opt/my brew/bin/gemini",
            "/opt/my brew/Cellar/gemini-cli/0.13.0/libexec/lib/node_modules/@google/gemini-cli/dist/index.js",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("'/opt/my brew/bin/brew' upgrade gemini-cli")
        );
    }

    #[test]
    fn brew_cask_path_with_space_is_quoted() {
        let cmd = anchored_command_from_paths(
            "codex",
            "/opt/my brew/bin/codex",
            "/opt/my brew/Caskroom/codex/0.146.0/bin/codex",
        );
        assert_eq!(
            cmd.as_deref(),
            Some("'/opt/my brew/bin/brew' upgrade --cask codex")
        );
    }

    #[test]
    fn brew_formula_extraction() {
        assert_eq!(
            brew_formula_from_path("/opt/homebrew/Cellar/gemini-cli/0.13.0/bin/gemini").as_deref(),
            Some("gemini-cli")
        );
        // node 全局包不在 Cellar 下 → 不是 formula。
        assert_eq!(
            brew_formula_from_path("/opt/homebrew/lib/node_modules/openclaw/openclaw.mjs"),
            None
        );
        assert_eq!(
            brew_formula_from_path("/Users/me/.nvm/versions/node/v22/lib/node_modules/x"),
            None
        );
        // Caskroom 不是 formula。
        assert_eq!(
            brew_formula_from_path("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex"),
            None
        );
    }

    #[test]
    fn brew_cask_extraction() {
        assert_eq!(
            brew_cask_from_path("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex").as_deref(),
            Some("codex")
        );
        // Intel prefix 不含 /homebrew/，仍能抽出 token。
        assert_eq!(
            brew_cask_from_path("/usr/local/Caskroom/codex/0.146.0/bin/codex").as_deref(),
            Some("codex")
        );
        // formula / npm 全局包都不是 cask。
        assert_eq!(
            brew_cask_from_path("/opt/homebrew/Cellar/codex/1.2.3/bin/codex"),
            None
        );
        assert_eq!(
            brew_cask_from_path("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
            None
        );
    }

    #[test]
    fn sibling_bin_returns_none_when_bin_path_has_no_directory() {
        // bin_path 不含 `/` → parent_dir 返回空 → sibling_bin 不能拼出绝对路径
        // → None,让上游 anchored_command_from_paths 整体退化为静态命令兜底,
        // 而不是悄悄拼出 `npm i -g <pkg>` 这种依赖 PATH 的指令(违背"必须绝对路径"
        // 不变量)。实际从 enumerate_tool_installations 走的 bin_path 都是绝对路径,
        // 这条防线不期望被触发,但闭合了 helper 与函数文档的语义一致。
        assert_eq!(sibling_bin("codex", "npm"), None);
        assert_eq!(sibling_bin("", "brew"), None);
        // 含 `/` 即可拼出绝对路径——这是常规路径。
        assert_eq!(
            sibling_bin("/opt/homebrew/bin/gemini", "brew").as_deref(),
            Some("/opt/homebrew/bin/brew")
        );
    }

    #[test]
    fn default_install_prefers_path_default() {
        let installs = vec![
            inst("/opt/homebrew/bin/openclaw", false),
            inst("/Users/me/.nvm/versions/node/v22/bin/openclaw", true),
        ];
        assert_eq!(
            default_install(&installs).map(|i| i.path.as_str()),
            Some("/Users/me/.nvm/versions/node/v22/bin/openclaw")
        );
    }

    #[test]
    fn default_install_falls_back_to_sole_entry() {
        let installs = vec![inst("/opt/homebrew/bin/gemini", false)];
        assert_eq!(
            default_install(&installs).map(|i| i.path.as_str()),
            Some("/opt/homebrew/bin/gemini")
        );
    }

    #[test]
    fn default_install_none_when_ambiguous() {
        let installs = vec![
            inst("/opt/homebrew/bin/openclaw", false),
            inst("/Users/me/.nvm/versions/node/v22/bin/openclaw", false),
        ];
        assert!(default_install(&installs).is_none());
    }

    #[test]
    fn codex_missing_platform_binary_self_heals_via_uninstall_install() {
        // 平台二进制缺失 → `codex --version` 报 "Missing optional dependency" 退出非 0
        // → enumerate 标记 runnable=false。此状态下普通 `npm i -g @latest` 是 no-op 修不好,
        // 升级路径改用 uninstall+install 重装补回平台二进制（`|| true` 让 uninstall 在
        // set -e 下对半损坏包返回非 0 时仍继续 install）。
        let mut broken = inst("/Users/me/.nvm/versions/node/v22.14.0/bin/codex", true);
        broken.runnable = false;
        broken.exited_with_error = true;
        assert_eq!(
            installs_anchored_command("codex", &[broken]).as_deref(),
            Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm uninstall -g @openai/codex || true; PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
        );
    }

    #[test]
    fn codex_probe_timeout_uses_plain_npm_not_uninstall() {
        // `--version` 超时被击杀 / 启动失败：runnable=false 但不是命令自己报错，不能当成
        // 装坏了。走卸载重装时，一旦装失败旧版就没了，用户看到「未安装」；普通
        // `npm i -g` 失败 npm 会回滚、旧版还在。
        let mut probe_failed = inst("/Users/me/.nvm/versions/node/v22.14.0/bin/codex", true);
        probe_failed.runnable = false;
        probe_failed.error = Some("Command timed out after 10s".to_string());
        let cmd = installs_anchored_command("codex", &[probe_failed]);
        assert_eq!(
            cmd.as_deref(),
            Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
        );
        assert!(!cmd.unwrap().contains("uninstall"));
    }

    #[test]
    fn codex_runnable_uses_plain_npm_not_self_heal() {
        // 正常（runnable=true）的 codex 升级：锚定 npm，既不重装、也不跑会假成功
        // 掩盖损坏的 `codex update`。
        let healthy = inst("/Users/me/.nvm/versions/node/v22.14.0/bin/codex", true);
        let cmd = installs_anchored_command("codex", &[healthy]);
        assert_eq!(
            cmd.as_deref(),
            Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
        );
        assert!(!cmd.unwrap().contains("uninstall"));
    }

    #[test]
    fn codex_broken_homebrew_formula_uses_brew_not_npm_repair() {
        // brew formula 装的坏 codex（real 在 Cellar）：自愈门控必须收窄放行，回落到
        // `brew upgrade codex`——若误走 npm 重装，npm 够不到 Cellar 那份、反而旁路
        // 装第二份 npm 全局 codex 制造双安装。
        let broken = ToolInstallation {
            path: "/opt/homebrew/bin/codex".to_string(),
            version: None,
            runnable: false,
            error: None,
            exited_with_error: true,
            source: "homebrew".to_string(),
            is_path_default: true,
            real: std::path::PathBuf::from("/opt/homebrew/Cellar/codex/1.2.3/bin/codex"),
        };
        assert_eq!(
            installs_anchored_command("codex", &[broken]).as_deref(),
            Some("/opt/homebrew/bin/brew upgrade codex")
        );
    }

    #[test]
    fn codex_broken_homebrew_cask_uses_brew_not_npm_repair() {
        // brew cask 装的坏 codex（real 在 Caskroom）：与 formula 同理，必须回落到
        // `brew upgrade --cask`。误走 npm uninstall+install 会撞 EEXIST，或旁路
        // 装第二份 npm 全局包争抢同一个 `/opt/homebrew/bin/codex`。
        let broken = ToolInstallation {
            path: "/opt/homebrew/bin/codex".to_string(),
            version: None,
            runnable: false,
            error: None,
            exited_with_error: true,
            source: "homebrew".to_string(),
            is_path_default: true,
            real: std::path::PathBuf::from("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex"),
        };
        assert_eq!(
            installs_anchored_command("codex", &[broken]).as_deref(),
            Some("/opt/homebrew/bin/brew upgrade --cask codex")
        );
    }

    #[test]
    fn codex_broken_volta_uses_volta_install_not_npm_repair() {
        // volta 装的坏 codex：回落到 `volta install`，不走 npm 重装。
        let mut broken = inst("/Users/me/.volta/bin/codex", true);
        broken.runnable = false;
        broken.exited_with_error = true;
        assert_eq!(
            installs_anchored_command("codex", &[broken]).as_deref(),
            Some("/Users/me/.volta/bin/volta install @openai/codex")
        );
    }

    #[test]
    fn codex_broken_bun_uses_bun_add_not_phantom_npm() {
        // bun 装的坏 codex：回落到 `bun add`，且**绝不**拼出 `~/.bun/bin/npm`
        // （bun 目录下没有 npm，那条路径不存在、执行会直接失败）。
        let mut broken = inst("/Users/me/.bun/bin/codex", true);
        broken.runnable = false;
        broken.exited_with_error = true;
        let cmd = installs_anchored_command("codex", &[broken]);
        assert_eq!(
            cmd.as_deref(),
            Some("/Users/me/.bun/bin/bun add -g @openai/codex@latest")
        );
        assert!(!cmd.unwrap().contains("npm"));
    }

    #[test]
    fn codex_standalone_reruns_official_installer_in_place() {
        // #7650:官方独立安装器装的 codex 此前被判 system 来源、锚定落空,退回裸
        // `npm i -g` 另装一份 npm 版。现在带 CODEX_INSTALL_DIR / CODEX_HOME 重跑官方
        // installer;不用 `codex update`——它断网时 exit 0 假成功。
        let cmd = anchored_command_from_paths(
            "codex",
            "/Users/me/.local/bin/codex",
            "/Users/me/.codex/packages/standalone/releases/0.157.0-aarch64-apple-darwin/bin/codex",
        )
        .unwrap();
        assert_eq!(
            cmd,
            format!(
                "CODEX_NON_INTERACTIVE=1 CODEX_INSTALL_DIR='/Users/me/.local/bin' CODEX_HOME='/Users/me/.codex' {CODEX_INSTALL_UNIX}"
            )
        );
        assert!(!cmd.contains("npm"), "npm must not be used: {cmd}");
        assert!(
            !cmd.contains("codex update"),
            "codex update fakes success: {cmd}"
        );
    }

    #[test]
    fn codex_standalone_custom_dirs_are_written_back() {
        // 自定义 CODEX_INSTALL_DIR / CODEX_HOME 装的一处:不显式传回去,installer 会装进
        // 默认的 ~/.local/bin 与 ~/.codex,命令行实际在用的那份原地不动。
        let cmd = anchored_command_from_paths(
            "codex",
            "/opt/tools/codex bin/codex",
            "/data/codex-home/packages/standalone/releases/0.157.0-x86_64-unknown-linux-musl/bin/codex",
        );
        assert_eq!(
            cmd,
            Some(format!(
                "CODEX_NON_INTERACTIVE=1 CODEX_INSTALL_DIR='/opt/tools/codex bin' CODEX_HOME='/data/codex-home' {CODEX_INSTALL_UNIX}"
            ))
        );
    }

    #[test]
    fn codex_broken_standalone_reinstalls_via_installer_not_npm_repair() {
        // 跑不起来的独立安装版:npm 自愈门控只放行 nvm/fnm/mise/homebrew,这里交回锚定,
        // 由官方 installer 重装同一处。
        let broken = ToolInstallation {
            path: "/Users/me/.local/bin/codex".to_string(),
            version: None,
            runnable: false,
            error: None,
            exited_with_error: true,
            source: "system".to_string(),
            is_path_default: true,
            real: std::path::PathBuf::from(
                "/Users/me/.codex/packages/standalone/releases/0.157.0-aarch64-apple-darwin/bin/codex",
            ),
        };
        let cmd = installs_anchored_command("codex", &[broken]).unwrap();
        assert!(cmd.ends_with(CODEX_INSTALL_UNIX), "{cmd}");
        assert!(!cmd.contains("npm"), "{cmd}");
    }

    #[test]
    fn codex_broken_standalone_in_node_manager_dir_skips_npm_repair() {
        // CODEX_INSTALL_DIR 指到 Homebrew / nvm 的 bin 目录时,入口路径会被判成
        // homebrew / nvm 来源;npm 自愈门控只看入口来源的话会抢先返回 npm 重装,
        // 把独立安装版换成 npm 版。真身仍在 standalone 发布包里,必须交回 installer。
        for bin in [
            "/opt/homebrew/bin/codex",
            "/Users/me/.nvm/versions/node/v22.19.0/bin/codex",
        ] {
            let broken = ToolInstallation {
                path: bin.to_string(),
                version: None,
                runnable: false,
                error: None,
                exited_with_error: true,
                source: infer_install_source(Path::new(bin)).to_string(),
                is_path_default: true,
                real: std::path::PathBuf::from(
                    "/Users/me/.codex/packages/standalone/releases/0.157.0-aarch64-apple-darwin/bin/codex",
                ),
            };
            let cmd = installs_anchored_command("codex", &[broken]).unwrap();
            assert!(cmd.ends_with(CODEX_INSTALL_UNIX), "{bin}: {cmd}");
            assert!(!cmd.contains("npm"), "{bin}: {cmd}");
        }
    }

    #[test]
    fn claude_homebrew_cask_uses_brew_not_self_update_or_npm() {
        // cask 归 brew 管:与 formula 一样不先跑 `claude update`,也不挂 npm fallback。
        let cmd = anchored_command_from_paths(
            "claude",
            "/opt/homebrew/bin/claude",
            "/opt/homebrew/Caskroom/claude-code/2.1.274/claude",
        )
        .unwrap();
        assert_eq!(cmd, "/opt/homebrew/bin/brew upgrade --cask claude-code");
        assert!(!cmd.contains("claude update"));
        assert!(!cmd.contains("npm"));
    }

    #[test]
    fn first_abs_path_line_skips_shell_noise() {
        // 交互式 .zshrc 先打印欢迎语（如 powerlevel10k / 自定义提示），
        // command -v 的真实路径在其后 → 跳过噪音取真路径。
        assert_eq!(
            first_abs_path_line("🚀 Welcome back!\n/Users/me/.local/bin/claude\n"),
            Some("/Users/me/.local/bin/claude")
        );
        // 无噪音时取第一行。
        assert_eq!(
            first_abs_path_line("/opt/homebrew/bin/gemini\n"),
            Some("/opt/homebrew/bin/gemini")
        );
        // 输出里没有任何绝对路径 → None。
        assert_eq!(first_abs_path_line("welcome\nbye\n"), None);
    }

    #[test]
    fn path_line_from_env_output_survives_shell_noise() {
        // `$SHELL -lic /usr/bin/env` 的 stdout 前面可能有交互式 rc 的欢迎语。
        let raw = "🚀 Welcome back, Jason!\nSHELL=/bin/zsh\nPATH=/opt/homebrew/bin:/usr/bin\nHOME=/Users/me\n";
        assert_eq!(
            path_line_from_env_output(raw),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        // 多行值的环境变量里恰好有一行以 `PATH=` 开头时，「值须以 / 开头」把它筛掉。
        let poisoned = "SOME_SCRIPT=line1\nPATH=not-a-path\nPATH=/usr/bin:/bin\n";
        assert_eq!(path_line_from_env_output(poisoned), Some("/usr/bin:/bin"));
        // 完全没有 PATH 行 → None，调用方保持不注入。
        assert_eq!(path_line_from_env_output("HOME=/Users/me\n"), None);
    }

    #[test]
    fn merge_path_segments_dedupes_preserving_login_order() {
        // 登录 shell 的段全部在前且保序；继承 PATH 里的新段追加在后。
        assert_eq!(
            merge_path_segments(
                "/Users/me/.nvm/versions/node/v22/bin:/usr/bin:/bin",
                "/usr/bin:/bin:/usr/sbin"
            ),
            "/Users/me/.nvm/versions/node/v22/bin:/usr/bin:/bin:/usr/sbin"
        );
        // 空段（`a::b` 在 POSIX 下意为当前目录）不该被注入。
        assert_eq!(merge_path_segments("/usr/bin::/bin", ""), "/usr/bin:/bin");
    }

    #[test]
    fn is_conflicting_thresholds() {
        let make = |version: Option<&str>, runnable: bool| ToolInstallation {
            path: "/x".to_string(),
            version: version.map(str::to_string),
            runnable,
            error: None,
            exited_with_error: false,
            source: "nvm".to_string(),
            is_path_default: false,
            real: std::path::PathBuf::from("/x"),
        };
        // 单处 → 不冲突。
        assert!(!is_conflicting(&[make(Some("1.0.0"), true)]));
        // 两处同版本、都能跑 → 不冲突（同版本装两遍不打扰）。
        assert!(!is_conflicting(&[
            make(Some("1.0.0"), true),
            make(Some("1.0.0"), true)
        ]));
        // 版本分歧 → 冲突。
        assert!(is_conflicting(&[
            make(Some("1.0.0"), true),
            make(Some("2.0.0"), true)
        ]));
        // 同版本但运行态混合（一个能跑、一个跑不起来）→ 冲突。
        assert!(is_conflicting(&[
            make(Some("1.0.0"), true),
            make(Some("1.0.0"), false)
        ]));
    }
}

#[test]
fn opencode_extra_search_paths_includes_install_and_fallback_dirs() {
    let home = PathBuf::from("/home/tester");
    let install_dir = Some(std::ffi::OsString::from("/custom/opencode/bin"));
    let xdg_bin_dir = Some(std::ffi::OsString::from("/xdg/bin"));
    let gopath =
        std::env::join_paths([PathBuf::from("/go/path1"), PathBuf::from("/go/path2")]).ok();

    let paths = opencode_extra_search_paths(&home, install_dir, xdg_bin_dir, gopath);

    assert_eq!(paths[0], PathBuf::from("/custom/opencode/bin"));
    assert_eq!(paths[1], PathBuf::from("/xdg/bin"));
    assert!(paths.contains(&PathBuf::from("/home/tester/bin")));
    assert!(paths.contains(&PathBuf::from("/home/tester/.opencode/bin")));
    assert!(paths.contains(&PathBuf::from("/home/tester/.bun/bin")));
    assert!(paths.contains(&PathBuf::from("/home/tester/go/bin")));
    assert!(paths.contains(&PathBuf::from("/go/path1/bin")));
    assert!(paths.contains(&PathBuf::from("/go/path2/bin")));
}

#[test]
fn opencode_extra_search_paths_deduplicates_repeated_entries() {
    let home = PathBuf::from("/home/tester");
    let same_dir = Some(std::ffi::OsString::from("/same/path"));

    let paths = opencode_extra_search_paths(&home, same_dir.clone(), same_dir, None);

    let count = paths
        .iter()
        .filter(|path| path.as_path() == Path::new("/same/path"))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn opencode_extra_search_paths_deduplicates_bun_default_dir() {
    let home = PathBuf::from("/home/tester");
    let paths = opencode_extra_search_paths(&home, None, None, None);

    let count = paths
        .iter()
        .filter(|path| path.as_path() == Path::new("/home/tester/.bun/bin"))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn grok_extra_search_paths_prefers_override_then_default_native_dir() {
    let home = PathBuf::from("/home/tester");
    let paths = grok_extra_search_paths(&home, Some(std::ffi::OsString::from("/custom/grok/bin")));

    assert_eq!(paths[0], PathBuf::from("/custom/grok/bin"));
    assert_eq!(paths[1], PathBuf::from("/home/tester/.grok/bin"));
}

#[cfg(not(target_os = "windows"))]
#[test]
fn mcode_extra_search_paths_use_installer_bin_dir() {
    let home = PathBuf::from("/home/tester");
    let paths = mcode_extra_search_paths(&home, Some(std::ffi::OsString::from("/opt/mcode")));
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/opt/mcode/bin"),
            PathBuf::from("/home/tester/.minimax-code/bin"),
        ]
    );
    // 空的 MCODE_INSTALL_DIR 视同未设置（installer 同样回退默认目录）。
    let paths = mcode_extra_search_paths(&home, Some(std::ffi::OsString::new()));
    assert_eq!(paths, vec![PathBuf::from("/home/tester/.minimax-code/bin")]);
}

#[cfg(target_os = "windows")]
#[test]
fn mcode_extra_search_paths_use_installer_root_on_windows() {
    // Windows installer 把 mcode.cmd / mcode.ps1 放在安装根目录，不在 bin 下。
    let home = PathBuf::from("C:\\Users\\tester");
    let paths = mcode_extra_search_paths(&home, None);
    assert_eq!(paths, vec![home.join(".minimax-code")]);
}

#[test]
fn grok_extra_search_paths_deduplicates_default_override() {
    let home = PathBuf::from("/home/tester");
    let paths = grok_extra_search_paths(
        &home,
        Some(std::ffi::OsString::from("/home/tester/.grok/bin")),
    );

    assert_eq!(paths, vec![PathBuf::from("/home/tester/.grok/bin")]);
}

#[test]
fn unique_search_paths_preserve_order_and_skip_empty_entries() {
    let first = PathBuf::from("first/bin");
    let second = PathBuf::from("second/bin");
    let third = PathBuf::from("third/bin");
    let mut paths = IndexSet::new();

    for path in [
        PathBuf::new(),
        first.clone(),
        second.clone(),
        first.clone(),
        PathBuf::new(),
        third.clone(),
        second.clone(),
    ] {
        push_unique_path(&mut paths, path);
    }

    assert_eq!(
        paths.into_iter().collect::<Vec<_>>(),
        vec![first, second, third]
    );
}

#[test]
fn unique_search_paths_keep_path_equality_and_first_spelling() {
    let first = PathBuf::from("node//bin");
    let equivalent = PathBuf::from("node/./bin");
    let parent = PathBuf::from("node/../node/bin");
    let different_case = PathBuf::from("Node/bin");
    assert_eq!(first, equivalent);
    let mut paths = IndexSet::new();

    for path in [
        first.clone(),
        equivalent,
        parent.clone(),
        different_case.clone(),
    ] {
        push_unique_path(&mut paths, path);
    }

    assert_eq!(paths[0].as_os_str(), first.as_os_str());
    assert_eq!(
        paths.into_iter().collect::<Vec<_>>(),
        vec![first, parent, different_case]
    );
}

#[test]
fn unique_search_paths_handle_many_multishell_paths_and_late_duplicates() {
    let expected: Vec<_> = (0..4096)
        .map(|i| PathBuf::from(format!("fnm_multishells/{i}/bin")))
        .collect();
    let mut paths = IndexSet::new();

    for path in expected.iter().chain(expected.iter().rev()) {
        push_unique_path(&mut paths, path.clone());
    }

    assert_eq!(paths.into_iter().collect::<Vec<_>>(), expected);
}

#[cfg(unix)]
#[test]
fn unique_search_paths_preserve_non_utf8_paths() {
    use std::os::unix::ffi::OsStringExt;

    let first = PathBuf::from(std::ffi::OsString::from_vec(b"node-\xff/bin".to_vec()));
    let second = PathBuf::from(std::ffi::OsString::from_vec(b"node-\xfe/bin".to_vec()));
    let mut paths = IndexSet::new();
    for path in [first.clone(), second.clone(), first.clone()] {
        push_unique_path(&mut paths, path);
    }

    assert_eq!(paths.into_iter().collect::<Vec<_>>(), vec![first, second]);
}

#[test]
fn cli_path_env_search_paths_include_path_entries_and_dedupe() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    std::fs::create_dir_all(&first).expect("first dir should be created");
    std::fs::create_dir_all(&second).expect("second dir should be created");

    let path_env = std::env::join_paths([first.clone(), second.clone(), first.clone()])
        .expect("test path env should be joinable");
    let mut paths = IndexSet::from([first.clone()]);

    extend_from_cli_path_env(&mut paths, Some(path_env));

    assert!(paths.contains(&second));
    assert_eq!(paths.iter().filter(|path| *path == &first).count(), 1);
}

#[test]
fn child_search_paths_include_existing_children_with_suffix() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let base = temp.path().join("node");
    let bin = base.join("25.8.0").join("bin");
    std::fs::create_dir_all(&bin).expect("version bin should be created");

    let mut paths = IndexSet::new();
    extend_existing_child_search_paths(&mut paths, &base, Some("bin"));

    assert!(paths.contains(&bin));
}

#[test]
fn env_child_dir_appends_child_and_dedupes() {
    let base = std::ffi::OsString::from("/custom/toolchain");
    let mut paths = IndexSet::new();

    push_env_child_dir(&mut paths, Some(base.clone()), "bin");
    push_env_child_dir(&mut paths, Some(base), "bin");

    assert_eq!(
        paths.into_iter().collect::<Vec<_>>(),
        vec![PathBuf::from("/custom/toolchain").join("bin")]
    );
}

#[cfg(target_os = "windows")]
#[test]
fn cli_path_env_skips_windows_apps_alias_dir() {
    assert!(is_windows_app_execution_alias_dir(Path::new(
        r"C:\Users\tester\AppData\Local\Microsoft\WindowsApps"
    )));
    assert!(!is_windows_app_execution_alias_dir(Path::new(
        r"C:\Users\tester\AppData\Roaming\npm"
    )));
}

#[test]
fn fnm_default_alias_dir_candidates_follow_fnm_base_dir_order() {
    let home = Path::new("/home/tester");
    let data_dir = std::path::PathBuf::from("/home/tester/AppData/Roaming");
    let alias = |base: &str| {
        std::path::PathBuf::from(base)
            .join("aliases")
            .join("default")
    };

    assert_eq!(
        fnm_default_alias_dir_candidates(Some("/opt/fnm".into()), home, Some(data_dir.clone())),
        vec![
            alias("/opt/fnm"),
            alias("/home/tester/.fnm"),
            alias("/home/tester/AppData/Roaming/fnm"),
        ]
    );
    // Empty FNM_DIR / home are ignored instead of becoming relative paths.
    assert_eq!(
        fnm_default_alias_dir_candidates(Some("".into()), Path::new(""), Some(data_dir)),
        vec![alias("/home/tester/AppData/Roaming/fnm")]
    );
}

#[cfg(unix)]
#[test]
fn prepend_search_dir_to_path_preserves_non_utf8_bytes() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let current = std::ffi::OsString::from_vec(b"/usr/bin:/tmp/\xff/bin".to_vec());
    let combined = prepend_search_dir_to_path(Path::new("/candidate/bin"), &current);

    assert_eq!(
        combined.as_os_str().as_bytes(),
        b"/candidate/bin:/usr/bin:/tmp/\xff/bin"
    );
}

#[cfg(unix)]
#[test]
fn install_probe_base_path_puts_login_path_before_inherited() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let inherited = std::ffi::OsString::from_vec(b"/usr/bin:/tmp/\xff/bin".to_vec());
    assert_eq!(
        install_probe_base_path(Some("/opt/homebrew/bin:/usr/bin"), &inherited)
            .as_os_str()
            .as_bytes(),
        b"/opt/homebrew/bin:/usr/bin:/usr/bin:/tmp/\xff/bin"
    );
    // 解析不到登录 shell：沿用继承的 PATH。
    assert_eq!(install_probe_base_path(None, &inherited), inherited);
    assert_eq!(install_probe_base_path(Some(""), &inherited), inherited);
    assert_eq!(
        install_probe_base_path(Some("/login/bin"), std::ffi::OsStr::new("")),
        std::ffi::OsString::from("/login/bin")
    );
}

/// `#!/usr/bin/env node` 的启动器所在目录里没有 node、继承的 PATH 里也没有：只有补上登录
/// shell 的 PATH，`--version` 探测才跑得起来（#9810）。
#[cfg(unix)]
#[test]
fn install_probe_finds_interpreter_through_login_path() {
    use std::os::unix::fs::PermissionsExt;
    let write_exec = |path: &Path, body: &str| {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, body).expect("write script");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    };
    let interpreter = "cc-switch-test-fake-node-9810";
    let dir = tempfile::tempdir().expect("create temp dir");
    let node_bin = dir.path().join("node/bin");
    write_exec(
        &node_bin.join(interpreter),
        "#!/bin/sh\necho 'codex-cli 0.170.0'\n",
    );
    let npm_bin = dir.path().join("npm/bin");
    let launcher = npm_bin.join("codex");
    write_exec(&launcher, &format!("#!/usr/bin/env {interpreter}\n"));

    let inherited = std::ffi::OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
    let without_login =
        prepend_search_dir_to_path(&npm_bin, &install_probe_base_path(None, inherited));
    let out = run_probe_version_command(&launcher, &without_login).expect("spawn");
    assert!(!out.status.success());

    let login_path = node_bin.to_string_lossy().into_owned();
    let with_login = prepend_search_dir_to_path(
        &npm_bin,
        &install_probe_base_path(Some(&login_path), inherited),
    );
    let out = run_probe_version_command(&launcher, &with_login).expect("spawn");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "codex-cli 0.170.0"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn expand_env_chars_preserves_unknown_vars_and_plain_text() {
    // No percent signs -> verbatim.
    assert_eq!(
        expand_env_chars(r"C:\Program Files\nodejs"),
        r"C:\Program Files\nodejs"
    );
    // Undefined variable is preserved verbatim (nothing dropped).
    assert_eq!(
        expand_env_chars(r"D:\npm-global\%DEFINITELY_NOT_A_REAL_VAR_xyz%\bin"),
        r"D:\npm-global\%DEFINITELY_NOT_A_REAL_VAR_xyz%\bin"
    );
    // Empty percent pair is not treated as a variable name.
    assert_eq!(expand_env_chars(r"C:\path\%%\tail"), r"C:\path\%%\tail");
}

#[cfg(target_os = "windows")]
#[test]
fn expand_env_chars_recovers_missing_user_variables_and_nested_references() {
    let process = |name: &str| match name.to_ascii_uppercase().as_str() {
        "USERPROFILE" => Some(r"C:\Users\tester".into()),
        "OVERRIDE" => Some(r"D:\runtime".into()),
        _ => None,
    };
    let registry = |name: &str| match name.to_ascii_uppercase().as_str() {
        "PNPM_HOME" => Some(r"%LOCALAPPDATA%\pnpm".into()),
        "LOCALAPPDATA" => Some(r"%USERPROFILE%\AppData\Local".into()),
        "OVERRIDE" => Some(r"D:\registry".into()),
        "CYCLE_A" => Some("%CYCLE_B%".into()),
        "CYCLE_B" => Some("%cycle_a%".into()),
        _ => None,
    };
    let expanded = expand_env_chars_with_lookup(
        r"%pnpm_home%;%OVERRIDE%;%MISSING%;%CYCLE_A%;%PNPM_HOME%;unfinished%",
        &process,
        &registry,
        &mut Vec::new(),
    );
    assert_eq!(
        expanded,
        r"C:\Users\tester\AppData\Local\pnpm;D:\runtime;%MISSING%;%cycle_a%;C:\Users\tester\AppData\Local\pnpm;unfinished%"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn build_tool_search_paths_includes_standalone_installer_dirs() {
    // Non-npm installer locations must be scanned even when the process PATH
    // dropped them (regression guard for #6061 / #6278 / #6047).
    let local_data = dirs::data_local_dir().expect("LOCALAPPDATA should resolve");

    let codex_paths = build_tool_search_paths("codex");
    assert!(codex_paths.contains(
        &local_data
            .join("Programs")
            .join("OpenAI")
            .join("Codex")
            .join("bin")
    ));

    let claude_paths = build_tool_search_paths("claude");
    assert!(claude_paths.contains(&local_data.join("Programs").join("claude")));

    // The standalone Codex dir is codex-specific; it must not pollute other tools.
    assert!(!build_tool_search_paths("gemini").contains(
        &local_data
            .join("Programs")
            .join("OpenAI")
            .join("Codex")
            .join("bin")
    ));
}

#[test]
#[serial_test::serial]
fn omp_search_paths_include_the_bun_global_bin() {
    // A GUI process does not inherit `~/.bun/bin`: without it a Bun global
    // omp is reported as not installed and updates lose their anchor.
    let home = crate::config::get_home_dir();
    let bun_bin = home.join(".bun").join("bin");
    assert!(build_tool_search_paths("omp").contains(&bun_bin));
    // Other Bun-installable tools keep their previous search set.
    assert!(!build_tool_search_paths("gemini").contains(&bun_bin));
}

#[cfg(target_os = "windows")]
#[test]
fn windows_path_lookup_ignores_same_named_file_in_current_directory() {
    let current_dir = tempfile::tempdir().expect("current directory should be created");
    let path_dir = tempfile::tempdir().expect("PATH directory should be created");
    std::fs::write(current_dir.path().join("codex.cmd"), "@echo current\r\n")
        .expect("current-directory shim should be created");
    let expected = path_dir.path().join("codex.cmd");
    std::fs::write(&expected, "@echo path\r\n").expect("PATH shim should be created");

    let effective_path = std::env::join_paths([path_dir.path()]).expect("test PATH should join");
    let output = windows_path_lookup_command("codex", &effective_path)
        .current_dir(current_dir.path())
        .output()
        .expect("where.exe should execute");
    let stderr = decode_command_output(&output.stderr);
    let matches = decode_command_output(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();

    assert!(output.status.success(), "where.exe failed: {stderr}");
    assert_eq!(matches.len(), 1);
    assert_eq!(
        std::fs::canonicalize(&matches[0]).expect("where.exe match should canonicalize"),
        std::fs::canonicalize(&expected).expect("expected PATH shim should canonicalize")
    );
}

#[test]
fn mise_node_search_paths_include_shims_and_installed_node_bins() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let home = temp.path();
    let node_bin = home
        .join(".local/share/mise/installs/node/25.8.0")
        .join("bin");
    std::fs::create_dir_all(&node_bin).expect("node bin should be created");

    let mut paths = IndexSet::new();
    extend_mise_node_search_paths(&mut paths, home);

    assert!(paths.contains(&home.join(".local/share/mise/shims")));
    assert!(paths.contains(&node_bin));
}

#[cfg(not(target_os = "windows"))]
#[test]
fn mise_shim_target_resolves_shim_through_mise_which() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("temp dir should be created");
    let root = temp.path();
    let node_bin = root.join("installs/node/22.12.0/bin");
    std::fs::create_dir_all(&node_bin).expect("node bin should be created");
    let target = node_bin.join("claude");
    std::fs::write(&target, "").expect("target should be written");

    let mise_dir = root.join("bin");
    std::fs::create_dir_all(&mise_dir).expect("mise dir should be created");
    let mise = mise_dir.join("mise");
    std::fs::write(
        &mise,
        format!(
            "#!/bin/sh\n[ \"$1\" = which ] && [ \"$2\" = claude ] && echo '{}'\n",
            target.display()
        ),
    )
    .expect("fake mise should be written");
    std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o755))
        .expect("fake mise should be executable");

    let shims = root.join("shims");
    std::fs::create_dir_all(&shims).expect("shims dir should be created");
    let shim = shims.join("claude");
    std::os::unix::fs::symlink(&mise, &shim).expect("shim symlink should be created");
    let shim_real = std::fs::canonicalize(&shim).expect("shim should canonicalize");

    assert_eq!(
        mise_shim_target("claude", &shim, &shim_real, None),
        Some(target.clone())
    );
    // 不是 mise shim 的候选不去问 mise。
    let target_real = std::fs::canonicalize(&target).expect("target should canonicalize");
    assert_eq!(
        mise_shim_target("claude", &target, &target_real, None),
        None
    );
    // mise 说不出路径（工具没装在当前版本里）时不猜。
    assert_eq!(mise_shim_target("codex", &shim, &shim_real, None), None);

    // 枚举安装时 shim 换成真身，和 installs 目录那处去重成同一处（#9515）；
    // shim 模式下默认入口是 mise 本体，也换成真身。
    let mut path_default = Some(shim_real.clone());
    let (path, real) = resolve_mise_shim_install(
        "claude",
        shim.clone(),
        shim_real.clone(),
        &mut path_default,
        None,
    );
    assert_eq!(path, target);
    assert_eq!(real, target_real);
    assert_eq!(path_default, Some(target_real.clone()));
    // 解析不了的 shim 原样保留，默认入口不动。
    let mut path_default = Some(shim_real.clone());
    let (path, real) = resolve_mise_shim_install(
        "codex",
        shim.clone(),
        shim_real.clone(),
        &mut path_default,
        None,
    );
    assert_eq!(path, shim);
    assert_eq!(real, shim_real);
    assert_eq!(path_default, Some(shim_real));
}

#[cfg(not(target_os = "windows"))]
#[test]
fn tool_executable_candidates_non_windows_uses_plain_binary_name() {
    let dir = PathBuf::from("/usr/local/bin");
    let candidates = tool_executable_candidates("opencode", &dir);

    assert_eq!(candidates, vec![PathBuf::from("/usr/local/bin/opencode")]);
}

#[test]
fn pathext_order_decides_whether_cmd_beats_exe() {
    assert!(!pathext_prefers_cmd_over_exe(None));
    assert!(!pathext_prefers_cmd_over_exe(Some(
        ".COM;.EXE;.BAT;.CMD;.VBS;.JS"
    )));
    assert!(pathext_prefers_cmd_over_exe(Some(".com;.cmd;.exe")));
    assert!(pathext_prefers_cmd_over_exe(Some(".COM;.CMD")));
    assert!(!pathext_prefers_cmd_over_exe(Some(".COM;.BAT")));
}

#[test]
fn same_dir_cmd_is_shadowed_by_exe_sibling_issue_8839() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let cmd = dir.path().join("hermes.cmd");
    let exe = dir.path().join("hermes.exe");
    std::fs::write(&cmd, "").expect("cmd launcher should be created");

    let default_pathext = Some(".COM;.EXE;.BAT;.CMD");
    // A lone launcher is the install itself.
    assert!(!shadowed_by_pathext_sibling(&cmd, default_pathext));

    std::fs::write(&exe, "").expect("exe launcher should be created");
    assert!(shadowed_by_pathext_sibling(&cmd, default_pathext));
    assert!(!shadowed_by_pathext_sibling(&exe, default_pathext));

    // A PATHEXT that lists .CMD first makes the .cmd the one that runs.
    let cmd_first = Some(".COM;.CMD;.EXE");
    assert!(!shadowed_by_pathext_sibling(&cmd, cmd_first));
    assert!(shadowed_by_pathext_sibling(&exe, cmd_first));

    // A different tool's launcher in the same directory never shadows.
    let other = dir.path().join("codex.cmd");
    std::fs::write(&other, "").expect("other launcher should be created");
    assert!(!shadowed_by_pathext_sibling(&other, default_pathext));
}

#[cfg(target_os = "windows")]
#[test]
fn tool_executable_candidates_windows_includes_cmd_exe_and_plain_name() {
    let dir = PathBuf::from("C:\\tools");
    let candidates = tool_executable_candidates("opencode", &dir);

    assert_eq!(
        candidates,
        vec![
            PathBuf::from("C:\\tools\\opencode.cmd"),
            PathBuf::from("C:\\tools\\opencode.exe"),
            PathBuf::from("C:\\tools\\opencode"),
        ]
    );
}

#[cfg(target_os = "windows")]
#[test]
fn tool_executable_candidates_windows_prefers_opencode_desktop_cli_sidecar() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let gui = dir.path().join("opencode.exe");
    let sidecar = dir.path().join("opencode-cli.exe");
    std::fs::write(&gui, "").expect("desktop executable should be created");
    std::fs::write(&sidecar, "").expect("CLI sidecar should be created");

    let candidates = tool_executable_candidates("opencode", dir.path());

    assert_eq!(
        candidates,
        vec![dir.path().join("opencode.cmd"), sidecar.clone()]
    );
    assert!(!candidates.contains(&gui));
    assert_eq!(
        windows_preferred_tool_executable("opencode", &gui),
        Some(sidecar)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn tool_executable_candidates_windows_prefers_opencode_resources_cli_sidecar() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let gui = dir.path().join("opencode.exe");
    let resources = dir.path().join("resources");
    let sidecar = resources.join("opencode-cli.exe");
    std::fs::create_dir(&resources).expect("resources dir should be created");
    std::fs::write(&gui, "").expect("desktop executable should be created");
    std::fs::write(resources.join("app.asar"), "").expect("app.asar should be created");
    std::fs::write(&sidecar, "").expect("CLI sidecar should be created");

    let candidates = tool_executable_candidates("opencode", dir.path());

    assert_eq!(
        candidates,
        vec![dir.path().join("opencode.cmd"), sidecar.clone()]
    );
    assert!(!candidates.contains(&gui));
    assert_eq!(
        windows_preferred_tool_executable("opencode", &gui),
        Some(sidecar)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn tool_executable_candidates_windows_skips_electron_desktop_without_cli_sidecar() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let gui = dir.path().join("opencode.exe");
    let resources = dir.path().join("resources");
    std::fs::create_dir(&resources).expect("resources dir should be created");
    std::fs::write(&gui, "").expect("desktop executable should be created");
    std::fs::write(resources.join("app.asar"), "").expect("app.asar should be created");

    let candidates = tool_executable_candidates("opencode", dir.path());

    assert_eq!(candidates, vec![dir.path().join("opencode.cmd")]);
    assert!(!candidates.contains(&gui));
    assert_eq!(windows_preferred_tool_executable("opencode", &gui), None);
}

#[cfg(target_os = "windows")]
#[test]
fn windows_preferred_tool_executable_keeps_standalone_opencode_exe() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let standalone = dir.path().join("opencode.exe");
    std::fs::write(&standalone, "").expect("standalone executable should be created");

    assert_eq!(
        windows_preferred_tool_executable("opencode", &standalone),
        Some(standalone)
    );
}

#[test]
fn foreign_pip_launcher_only_rejects_pi_exe_in_scripts_dir() {
    let pip_pi = Path::new(r"D:\soft\Python\Python311\Scripts\pi.exe");
    assert!(is_foreign_pip_launcher("pi", pip_pi));
    assert!(is_foreign_pip_launcher(
        "pi",
        Path::new(r"C:\venvs\tools\SCRIPTS\PI.EXE")
    ));
    assert!(!is_foreign_pip_launcher(
        "pi",
        Path::new(r"C:\Users\me\AppData\Roaming\npm\pi.cmd")
    ));
    // A custom npm prefix under a `Scripts` folder writes `.cmd` shims.
    assert!(!is_foreign_pip_launcher(
        "pi",
        Path::new(r"D:\Scripts\npm-global\pi.cmd")
    ));
    assert!(!is_foreign_pip_launcher(
        "pi",
        Path::new(r"D:\Scripts\pi.cmd")
    ));
    // Bun's global shims are `.exe` but live in `.bun\bin`.
    assert!(!is_foreign_pip_launcher(
        "pi",
        Path::new(r"C:\Users\me\.bun\bin\pi.exe")
    ));
    // Hermes really is installed by pip.
    assert!(!is_foreign_pip_launcher(
        "hermes",
        Path::new(r"D:\soft\Python\Python311\Scripts\hermes.exe")
    ));
}

#[test]
fn vscode_copilot_shim_is_rejected_but_real_copilot_is_kept() {
    assert!(is_vscode_copilot_shim(
        "copilot",
        Path::new(r"D:\Microsoft VS Code\bin\copilot-shim\copilot.exe")
    ));
    assert!(is_vscode_copilot_shim(
        "copilot",
        Path::new(r"C:\Program Files\Microsoft VS Code\bin\COPILOT-SHIM\copilot.cmd")
    ));
    // npm's global shim is the real CLI.
    assert!(!is_vscode_copilot_shim(
        "copilot",
        Path::new(r"C:\Users\me\AppData\Roaming\npm\copilot.cmd")
    ));
    // Only the copilot tool is affected.
    assert!(!is_vscode_copilot_shim(
        "codex",
        Path::new(r"D:\x\copilot-shim\codex.exe")
    ));
}

#[cfg(target_os = "windows")]
#[test]
fn windows_path_default_skips_pip_pi_and_uses_npm_pi() {
    let root = tempfile::tempdir().expect("temp dir should be created");
    let scripts = root.path().join("Python311").join("Scripts");
    std::fs::create_dir_all(&scripts).expect("Scripts dir should be created");
    let pip_pi = scripts.join("pi.exe");
    std::fs::write(&pip_pi, "").expect("pip launcher should be created");
    assert!(tool_executable_candidates("pi", &scripts)
        .iter()
        .all(|candidate| candidate != &pip_pi));

    let npm = root.path().join("npm");
    std::fs::create_dir_all(&npm).expect("npm dir should be created");
    let npm_pi = npm.join("pi.cmd");
    std::fs::write(&npm_pi, "").expect("npm shim should be created");

    let raw = format!("{}\r\n{}\r\n", pip_pi.display(), npm_pi.display());
    assert_eq!(windows_preferred_path_default("pi", &raw), Some(npm_pi));
    assert_eq!(
        windows_preferred_path_default("pi", &format!("{}\r\n", pip_pi.display())),
        None
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_path_default_skips_desktop_without_sidecar_and_uses_next_cli() {
    let desktop_dir = tempfile::tempdir().expect("desktop temp dir should be created");
    let resources = desktop_dir.path().join("resources");
    std::fs::create_dir(&resources).expect("resources dir should be created");
    let desktop = desktop_dir.path().join("opencode.exe");
    std::fs::write(&desktop, "").expect("desktop executable should be created");
    std::fs::write(resources.join("app.asar"), "").expect("app.asar should be created");

    let cli_dir = tempfile::tempdir().expect("CLI temp dir should be created");
    let cli = cli_dir.path().join("opencode.exe");
    std::fs::write(&cli, "").expect("standalone executable should be created");

    let raw = format!("{}\r\n{}\r\n", desktop.display(), cli.display());

    assert_eq!(windows_preferred_path_default("opencode", &raw), Some(cli));
}

#[cfg(target_os = "windows")]
#[test]
fn tool_executable_candidates_windows_skips_shadowed_npm_unix_shim() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let extensionless = dir.path().join("codex");
    let cmd = dir.path().join("codex.cmd");
    std::fs::write(&extensionless, "").expect("extensionless shim should be created");
    std::fs::write(&cmd, "").expect("cmd shim should be created");

    let candidates = tool_executable_candidates("codex", dir.path());

    assert_eq!(candidates, vec![cmd.clone(), dir.path().join("codex.exe")]);
    assert!(!candidates.contains(&extensionless));
}

#[cfg(target_os = "windows")]
#[test]
fn windows_runnable_sibling_prefers_cmd_over_extensionless_tool() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let extensionless = dir.path().join("codex");
    let cmd = dir.path().join("codex.cmd");
    std::fs::write(&extensionless, "").expect("extensionless shim should be created");
    std::fs::write(&cmd, "").expect("cmd shim should be created");

    let preferred = windows_runnable_sibling_for_extensionless_tool(&extensionless);

    assert_eq!(preferred.as_deref(), Some(cmd.as_path()));
}

#[cfg(target_os = "windows")]
#[test]
fn run_windows_tool_version_command_accepts_canonicalized_cmd_path() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let cmd = dir.path().join("codex.cmd");
    std::fs::write(&cmd, "@echo off\r\necho codex-cli 0.144.3\r\n")
        .expect("cmd shim should be created");
    let canonical = std::fs::canonicalize(&cmd).expect("cmd shim should canonicalize");
    assert!(
        canonical.to_string_lossy().starts_with(r"\\?\"),
        "Windows canonical paths should use the verbatim prefix: {}",
        canonical.display()
    );

    let current_path = std::env::var("PATH").unwrap_or_default();
    let output = run_windows_tool_version_command(&canonical, &current_path)
        .expect("canonicalized cmd shim should execute");
    let stderr = decode_command_output(&output.stderr);

    assert!(output.status.success(), "cmd shim failed: {stderr}");
    assert_eq!(
        decode_command_output(&output.stdout).trim(),
        "codex-cli 0.144.3"
    );
}

#[test]
fn opencode_desktop_sidecar_needs_the_gui_layout() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("opencode.exe"), b"").unwrap();
    for relative in ["opencode-cli.exe", "resources/opencode-cli.exe"] {
        let sidecar = dir.path().join(relative);
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        std::fs::write(&sidecar, b"").unwrap();
        assert_eq!(
            infer_install_source_for_tool("opencode", &sidecar, &sidecar),
            "desktop"
        );
        assert_ne!(
            infer_install_source_for_tool("codex", &sidecar, &sidecar),
            "desktop"
        );
    }
    // 不和 GUI 在一起的 opencode-cli.exe 不算 Desktop 自带
    let unrelated = dir.path().join("standalone/opencode-cli.exe");
    std::fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
    std::fs::write(&unrelated, b"").unwrap();
    assert_eq!(
        infer_install_source_for_tool("opencode", &unrelated, &unrelated),
        "system"
    );
}

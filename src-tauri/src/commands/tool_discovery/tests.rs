use super::*;

/// 探测 helper 正常路径：spawn（含 pre_exec setsid）能启动、输出能捕获。
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

/// 锚定落空后的兜底：认不出渠道的原生可执行文件不再退回裸 `npm i -g`（#7650），
/// 脚本 shim / node_modules 里的入口 / 定位不到默认那处仍保持静态命令。
mod unmanaged_native_update {
    use super::super::*;

    const MACH_O_64: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];

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
    fn resolve_update_command_guards_unmanaged_native_binaries() {
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

        let expected_static = UpdateCommand::Static(static_fallback_command("gemini"));
        let shim = installed(
            &dir,
            ".asdf/shims/gemini",
            b"#!/usr/bin/env bash\nexec asdf exec gemini \"$@\"\n",
            true,
        );
        assert_eq!(resolve_update_command("gemini", &[shim]), expected_static);

        let bundled = installed(
            &dir,
            "prefix/lib/node_modules/@google/gemini-cli/bin/gemini",
            &MACH_O_64,
            true,
        );
        assert_eq!(
            resolve_update_command("gemini", &[bundled]),
            expected_static
        );

        let a = installed(&dir, "a/bin/gemini", &MACH_O_64, false);
        let b = installed(&dir, "b/bin/gemini", &MACH_O_64, false);
        assert_eq!(resolve_update_command("gemini", &[a, b]), expected_static);
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

#[test]
fn infer_install_source_classifies_channels() {
    for (path, expected) in [
        ("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex", "homebrew"),
        ("/usr/local/Caskroom/codex/0.146.0/bin/codex", "homebrew"),
        ("/usr/local/bin/codex", "system"),
        ("/Users/me/.volta/bin/codex", "volta"),
        (r"C:\Users\me\AppData\Local\Volta\bin\codex.exe", "volta"),
        (r"C:\Users\me\AppData\Local\pnpm\codex.cmd", "pnpm"),
        (
            r"C:\Users\me\AppData\Roaming\nvm\v22.0.0\codex.cmd",
            "system",
        ),
        (r"C:\Users\me\scoop\shims\codex.cmd", "scoop"),
    ] {
        assert_eq!(infer_install_source(Path::new(path)), expected, "{path}");
    }

    // Intel Cask 的 PATH 入口在 `/usr/local/bin`，须按真身归入 homebrew；
    // nvm shim 仍按 launcher 归类，不能被真身路径抢走。
    assert_eq!(
        infer_install_source_for_install(
            Path::new("/usr/local/bin/codex"),
            Path::new("/usr/local/Caskroom/codex/0.146.0/bin/codex"),
        ),
        "homebrew"
    );
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
            real: PathBuf::from(path),
        }
    }

    #[test]
    fn anchored_command_by_install_channel() {
        let grok_native_default = format!(
            "npm_config_allow_scripts=@xai-official/grok npm_config_ignore_scripts=false npm_config_include=optional /Users/me/.grok/bin/grok update || {GROK_INSTALL_UNIX}"
        );
        let grok_native_custom = format!(
            "npm_config_allow_scripts=@xai-official/grok npm_config_ignore_scripts=false npm_config_include=optional /Users/me/bin/grok update || {GROK_INSTALL_UNIX}"
        );
        let mcode_native =
            format!("MCODE_INSTALL_DIR='/Users/me/.minimax-code' {MCODE_INSTALL_UNIX}");
        let codex_standalone_custom = format!(
            "CODEX_NON_INTERACTIVE=1 CODEX_INSTALL_DIR='/opt/tools/codex bin' CODEX_HOME='/data/codex-home' {CODEX_INSTALL_UNIX}"
        );

        let cases: &[(&str, &str, &str, Option<&str>)] = &[
            // Native self-update / official installer rerun
            (
                "claude",
                "/Users/me/.local/bin/claude",
                "/Users/me/.local/share/claude/versions/2.1.146",
                Some("/Users/me/.local/bin/claude update"),
            ),
            (
                "hermes",
                "/usr/local/bin/hermes",
                "/usr/local/bin/hermes",
                Some("/usr/local/bin/hermes update"),
            ),
            (
                "agy",
                "/opt/homebrew/bin/agy",
                "/opt/homebrew/Caskroom/antigravity-cli/1.3.4/antigravity",
                Some("/opt/homebrew/bin/agy update"),
            ),
            (
                "opencode",
                "/Users/me/.opencode/bin/opencode",
                "/Users/me/.opencode/bin/opencode",
                Some("/Users/me/.opencode/bin/opencode upgrade"),
            ),
            (
                "grok",
                "/Users/me/.grok/bin/grok",
                "/Users/me/.grok/downloads/grok-macos-aarch64",
                Some(grok_native_default.as_str()),
            ),
            (
                "grok",
                "/Users/me/bin/grok",
                "/Users/me/.grok/downloads/grok-macos-aarch64",
                Some(grok_native_custom.as_str()),
            ),
            (
                "mcode",
                "/Users/me/.local/bin/mcode",
                "/Users/me/.minimax-code/bin/mcode",
                Some(mcode_native.as_str()),
            ),
            // `codebuddy install` puts a native build outside node_modules: its own updater only
            (
                "codebuddy",
                "/Users/me/.local/bin/codebuddy",
                "/Users/me/.local/bin/codebuddy",
                Some("/Users/me/.local/bin/codebuddy update"),
            ),
            (
                "codex",
                "/opt/tools/codex bin/codex",
                "/data/codex-home/packages/standalone/releases/0.157.0-linux/bin/codex",
                Some(codex_standalone_custom.as_str()),
            ),
            // Homebrew formula & cask (Apple Silicon + Intel)
            (
                "gemini",
                "/opt/homebrew/bin/gemini",
                "/opt/homebrew/Cellar/gemini-cli/0.13.0/libexec/lib/node_modules/@google/gemini-cli/dist/index.js",
                Some("/opt/homebrew/bin/brew upgrade gemini-cli"),
            ),
            (
                "codex",
                "/usr/local/bin/codex",
                "/usr/local/Caskroom/codex/0.146.0/bin/codex",
                Some("/usr/local/bin/brew upgrade --cask codex"),
            ),
            (
                "claude",
                "/opt/homebrew/bin/claude",
                "/opt/homebrew/Caskroom/claude-code/2.1.274/claude",
                Some("/opt/homebrew/bin/brew upgrade --cask claude-code"),
            ),
            (
                "omp",
                "/opt/homebrew/bin/omp",
                "/opt/homebrew/Cellar/omp/14.1.0/bin/omp",
                Some("/opt/homebrew/bin/brew upgrade omp"),
            ),
            // Node managers: nvm / fnm / homebrew npm / volta / bun
            (
                "gemini",
                "/Users/me/.nvm/versions/node/v22.14.0/bin/gemini",
                "/Users/me/.nvm/versions/node/v22.14.0/lib/node_modules/@google/gemini-cli/dist/index.js",
                Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @google/gemini-cli@latest"),
            ),
            (
                "codex",
                "/Users/me/.local/share/fnm_multishells/12345_abc/bin/codex",
                "/Users/me/.local/share/fnm_multishells/12345_abc/lib/node_modules/@openai/codex/bin/codex.js",
                Some("PATH='/Users/me/.local/share/fnm_multishells/12345_abc/bin':\"$PATH\" /Users/me/.local/share/fnm_multishells/12345_abc/bin/npm i -g @openai/codex@latest"),
            ),
            (
                "mcode",
                "/Users/me/.nvm/versions/node/v22.19.0/bin/mcode",
                "/Users/me/.nvm/versions/node/v22.19.0/lib/node_modules/@minimax-ai/code/cli.js",
                Some("PATH='/Users/me/.nvm/versions/node/v22.19.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.19.0/bin/npm i -g @minimax-ai/code@latest --ignore-scripts=false --include=optional \"--allow-scripts=@minimax-ai/code,better-sqlite3\""),
            ),
            (
                "codebuddy",
                "/Users/me/.local/share/mise/installs/node/26.10.0/bin/codebuddy",
                "/Users/me/.local/share/mise/installs/node/26.10.0/lib/node_modules/@tencent-ai/codebuddy-code/bin/codebuddy",
                Some("/Users/me/.local/share/mise/installs/node/26.10.0/bin/codebuddy update || PATH='/Users/me/.local/share/mise/installs/node/26.10.0/bin':\"$PATH\" /Users/me/.local/share/mise/installs/node/26.10.0/bin/npm i -g @tencent-ai/codebuddy-code@latest"),
            ),
            (
                "openclaw",
                "/opt/homebrew/bin/openclaw",
                "/opt/homebrew/lib/node_modules/openclaw/openclaw.mjs",
                Some("/opt/homebrew/bin/openclaw update --yes || PATH='/opt/homebrew/bin':\"$PATH\" /opt/homebrew/bin/npm i -g openclaw@latest"),
            ),
            (
                "openclaw",
                "/Users/me/.volta/bin/openclaw",
                "/Users/me/.volta/tools/image/packages/openclaw/lib/node_modules/openclaw",
                Some("/Users/me/.volta/bin/openclaw update --yes || /Users/me/.volta/bin/volta install openclaw"),
            ),
            (
                "codex",
                "/Users/me/.volta/bin/codex",
                "/Users/me/.volta/tools/image/packages/codex/lib/node_modules/@openai/codex",
                Some("/Users/me/.volta/bin/volta install @openai/codex"),
            ),
            (
                "opencode",
                "/Users/me/.bun/bin/opencode",
                "/Users/me/.bun/install/global/node_modules/opencode-ai/bin/opencode",
                Some("/Users/me/.bun/bin/opencode upgrade || /Users/me/.bun/bin/bun add -g opencode-ai@latest"),
            ),
            // Scoped postinstall settings for npm-managed Claude / OpenCode (managed + system prefix)
            (
                "claude",
                "/Users/me/.local/share/mise/installs/node/22/bin/claude",
                "/Users/me/.local/share/mise/installs/node/22/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
                Some("npm_config_allow_scripts=@anthropic-ai/claude-code npm_config_ignore_scripts=false npm_config_include=optional /Users/me/.local/share/mise/installs/node/22/bin/claude update || PATH='/Users/me/.local/share/mise/installs/node/22/bin':\"$PATH\" /Users/me/.local/share/mise/installs/node/22/bin/npm i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code"),
            ),
            (
                "claude",
                "/usr/local/bin/claude",
                "/usr/local/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
                Some("npm_config_allow_scripts=@anthropic-ai/claude-code npm_config_ignore_scripts=false npm_config_include=optional /usr/local/bin/claude update"),
            ),
            (
                "opencode",
                "/Users/me/.nvm/versions/node/v22/bin/opencode",
                "/Users/me/.nvm/versions/node/v22/lib/node_modules/opencode-ai/bin/opencode.exe",
                Some("npm_config_allow_scripts=opencode-ai npm_config_ignore_scripts=false npm_config_include=optional /Users/me/.nvm/versions/node/v22/bin/opencode upgrade || PATH='/Users/me/.nvm/versions/node/v22/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22/bin/npm i -g opencode-ai@latest --ignore-scripts=false --include=optional --allow-scripts=opencode-ai"),
            ),
            // omp special rules: bun fallback only, no npm fallback, Nix unmanaged (None)
            (
                "omp",
                "/Users/me/.bun/bin/omp",
                "/Users/me/.bun/install/global/node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts",
                Some("/Users/me/.bun/bin/omp update || /Users/me/.bun/bin/bun add -g @oh-my-pi/pi-coding-agent@latest"),
            ),
            (
                "omp",
                "/Users/me/.nvm/versions/node/v22.0.0/bin/omp",
                "/Users/me/.nvm/versions/node/v22.0.0/lib/node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts",
                Some("/Users/me/.nvm/versions/node/v22.0.0/bin/omp update"),
            ),
            (
                "omp",
                "/Users/me/.nix-profile/bin/omp",
                "/nix/store/abc-omp-14.1.0/bin/omp",
                None,
            ),
            // Spaced paths quoted across channels
            (
                "claude",
                "/Users/my name/.local/bin/claude",
                "/Users/my name/.local/share/claude/versions/2.1.146",
                Some("'/Users/my name/.local/bin/claude' update"),
            ),
            (
                "codex",
                "/opt/my brew/bin/codex",
                "/opt/my brew/Caskroom/codex/0.146.0/bin/codex",
                Some("'/opt/my brew/bin/brew' upgrade --cask codex"),
            ),
            (
                "codex",
                "/Users/my name/.nvm/versions/node/v22/bin/codex",
                "/Users/my name/.nvm/versions/node/v22/lib/node_modules/@openai/codex/bin/codex.js",
                Some("PATH='/Users/my name/.nvm/versions/node/v22/bin':\"$PATH\" '/Users/my name/.nvm/versions/node/v22/bin/npm' i -g @openai/codex@latest"),
            ),
            (
                "opencode",
                "/Users/my name/.bun/bin/opencode",
                "/Users/my name/.bun/install/global/node_modules/opencode-ai/bin/opencode",
                Some("'/Users/my name/.bun/bin/opencode' upgrade || '/Users/my name/.bun/bin/bun' add -g opencode-ai@latest"),
            ),
        ];

        for &(tool, bin_path, real, expected) in cases {
            assert_eq!(
                anchored_command_from_paths(tool, bin_path, real).as_deref(),
                expected,
                "tool={tool} bin={bin_path} real={real}"
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
    fn brew_token_extraction() {
        assert_eq!(
            brew_formula_from_path("/opt/homebrew/Cellar/gemini-cli/0.13.0/bin/gemini").as_deref(),
            Some("gemini-cli")
        );
        assert_eq!(
            brew_formula_from_path("/opt/homebrew/lib/node_modules/openclaw/openclaw.mjs"),
            None
        );
        assert_eq!(
            brew_formula_from_path("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex"),
            None
        );
        assert_eq!(
            brew_cask_from_path("/usr/local/Caskroom/codex/0.146.0/bin/codex").as_deref(),
            Some("codex")
        );
        assert_eq!(
            brew_cask_from_path("/opt/homebrew/Cellar/codex/1.2.3/bin/codex"),
            None
        );
    }

    #[test]
    fn default_install_selection() {
        assert_eq!(
            default_install(&[
                inst("/opt/homebrew/bin/openclaw", false),
                inst("/Users/me/.nvm/versions/node/v22/bin/openclaw", true),
            ])
            .map(|i| i.path.as_str()),
            Some("/Users/me/.nvm/versions/node/v22/bin/openclaw")
        );
        assert_eq!(
            default_install(&[inst("/opt/homebrew/bin/gemini", false)]).map(|i| i.path.as_str()),
            Some("/opt/homebrew/bin/gemini")
        );
        assert!(default_install(&[
            inst("/opt/homebrew/bin/openclaw", false),
            inst("/Users/me/.nvm/versions/node/v22/bin/openclaw", false),
        ])
        .is_none());
    }

    #[test]
    fn codex_broken_self_heal_gate() {
        let make = |path: &str, real: &str, runnable: bool, exited_with_error: bool| {
            let mut i = inst(path, true);
            i.real = PathBuf::from(real);
            i.runnable = runnable;
            i.exited_with_error = exited_with_error;
            i
        };

        // 真正报错退出的 nvm 安装触发 uninstall + install 自愈。
        let nvm_bin = "/Users/me/.nvm/versions/node/v22.14.0/bin/codex";
        let nvm_real =
            "/Users/me/.nvm/versions/node/v22.14.0/lib/node_modules/@openai/codex/bin/codex.js";
        assert_eq!(
            installs_anchored_command("codex", &[make(nvm_bin, nvm_real, false, true)]).as_deref(),
            Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm uninstall -g @openai/codex || true; PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
        );

        // 探测超时（exited_with_error=false）与健康安装都只走普通 npm，不卸载旧版。
        let plain_npm = "PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest";
        for (runnable, exited_with_error) in [(false, false), (true, false)] {
            assert_eq!(
                installs_anchored_command(
                    "codex",
                    &[make(nvm_bin, nvm_real, runnable, exited_with_error)],
                )
                .as_deref(),
                Some(plain_npm)
            );
        }

        // 非 npm 来源（brew / volta / bun / standalone，即使装在 node 管理器 bin 目录下）
        // 损坏时必须交回各自渠道升级，绝不走 npm 重装。
        let standalone_real =
            "/Users/me/.codex/packages/standalone/releases/0.157.0-aarch64-apple-darwin/bin/codex";
        for (path, real, expected_exact, expected_suffix) in [
            (
                "/opt/homebrew/bin/codex",
                "/opt/homebrew/Cellar/codex/1.2.3/bin/codex",
                Some("/opt/homebrew/bin/brew upgrade codex"),
                None,
            ),
            (
                "/opt/homebrew/bin/codex",
                "/opt/homebrew/Caskroom/codex/0.146.0/bin/codex",
                Some("/opt/homebrew/bin/brew upgrade --cask codex"),
                None,
            ),
            (
                "/Users/me/.volta/bin/codex",
                "/Users/me/.volta/bin/codex",
                Some("/Users/me/.volta/bin/volta install @openai/codex"),
                None,
            ),
            (
                "/Users/me/.bun/bin/codex",
                "/Users/me/.bun/bin/codex",
                Some("/Users/me/.bun/bin/bun add -g @openai/codex@latest"),
                None,
            ),
            (
                "/Users/me/.local/bin/codex",
                standalone_real,
                None,
                Some(CODEX_INSTALL_UNIX),
            ),
            (
                "/opt/homebrew/bin/codex",
                standalone_real,
                None,
                Some(CODEX_INSTALL_UNIX),
            ),
            (
                "/Users/me/.nvm/versions/node/v22.19.0/bin/codex",
                standalone_real,
                None,
                Some(CODEX_INSTALL_UNIX),
            ),
        ] {
            let cmd = installs_anchored_command("codex", &[make(path, real, false, true)]).unwrap();
            assert!(!cmd.contains("npm"), "{path}: {cmd}");
            if let Some(exact) = expected_exact {
                assert_eq!(cmd, exact, "{path}");
            }
            if let Some(suffix) = expected_suffix {
                assert!(cmd.ends_with(suffix), "{path}: {cmd}");
            }
        }
    }

    #[test]
    fn shell_output_and_path_parsing() {
        assert_eq!(
            first_abs_path_line("🚀 Welcome back!\n/Users/me/.local/bin/claude\n"),
            Some("/Users/me/.local/bin/claude")
        );
        assert_eq!(first_abs_path_line("welcome\nbye\n"), None);

        let raw =
            "🚀 Welcome!\nSOME_SCRIPT=line1\nPATH=not-a-path\nPATH=/opt/homebrew/bin:/usr/bin\n";
        assert_eq!(
            path_line_from_env_output(raw),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(path_line_from_env_output("HOME=/Users/me\n"), None);

        assert_eq!(
            merge_path_segments(
                "/Users/me/.nvm/versions/node/v22/bin:/usr/bin:/bin",
                "/usr/bin:/bin:/usr/sbin"
            ),
            "/Users/me/.nvm/versions/node/v22/bin:/usr/bin:/bin:/usr/sbin"
        );
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
            real: PathBuf::from("/x"),
        };
        assert!(!is_conflicting(&[make(Some("1.0.0"), true)]));
        assert!(!is_conflicting(&[
            make(Some("1.0.0"), true),
            make(Some("1.0.0"), true)
        ]));
        assert!(is_conflicting(&[
            make(Some("1.0.0"), true),
            make(Some("2.0.0"), true)
        ]));
        assert!(is_conflicting(&[
            make(Some("1.0.0"), true),
            make(Some("1.0.0"), false)
        ]));
    }
}

#[cfg(unix)]
#[test]
fn install_probe_base_path_and_non_utf8_preservation() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let inherited = std::ffi::OsString::from_vec(b"/usr/bin:/tmp/\xff/bin".to_vec());
    assert_eq!(
        prepend_search_dir_to_path(Path::new("/candidate/bin"), &inherited)
            .as_os_str()
            .as_bytes(),
        b"/candidate/bin:/usr/bin:/tmp/\xff/bin"
    );
    assert_eq!(
        install_probe_base_path(Some("/opt/homebrew/bin:/usr/bin"), &inherited)
            .as_os_str()
            .as_bytes(),
        b"/opt/homebrew/bin:/usr/bin:/usr/bin:/tmp/\xff/bin"
    );
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
    let target_real = std::fs::canonicalize(&target).expect("target should canonicalize");
    assert_eq!(
        mise_shim_target("claude", &target, &target_real, None),
        None
    );
    assert_eq!(mise_shim_target("codex", &shim, &shim_real, None), None);

    // 枚举安装时 shim 换成真身，和 installs 目录那处去重成同一处（#9515）。
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
    assert_eq!(path_default, Some(target_real));

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

#[test]
fn same_dir_cmd_is_shadowed_by_pathext_sibling() {
    assert!(!pathext_prefers_cmd_over_exe(None));
    assert!(!pathext_prefers_cmd_over_exe(Some(
        ".COM;.EXE;.BAT;.CMD;.VBS;.JS"
    )));
    assert!(pathext_prefers_cmd_over_exe(Some(".com;.cmd;.exe")));
    assert!(pathext_prefers_cmd_over_exe(Some(".COM;.CMD")));
    assert!(!pathext_prefers_cmd_over_exe(Some(".COM;.BAT")));

    let dir = tempfile::tempdir().expect("temp dir should be created");
    let cmd = dir.path().join("hermes.cmd");
    let exe = dir.path().join("hermes.exe");
    std::fs::write(&cmd, "").unwrap();

    let default_pathext = Some(".COM;.EXE;.BAT;.CMD");
    assert!(!shadowed_by_pathext_sibling(&cmd, default_pathext));

    std::fs::write(&exe, "").unwrap();
    assert!(shadowed_by_pathext_sibling(&cmd, default_pathext));
    assert!(!shadowed_by_pathext_sibling(&exe, default_pathext));

    let cmd_first = Some(".COM;.CMD;.EXE");
    assert!(!shadowed_by_pathext_sibling(&cmd, cmd_first));
    assert!(shadowed_by_pathext_sibling(&exe, cmd_first));

    let other = dir.path().join("codex.cmd");
    std::fs::write(&other, "").unwrap();
    assert!(!shadowed_by_pathext_sibling(&other, default_pathext));
}

#[test]
fn foreign_shims_are_rejected() {
    assert!(is_foreign_pip_launcher(
        "pi",
        Path::new(r"D:\soft\Python\Python311\Scripts\pi.exe")
    ));
    assert!(is_foreign_pip_launcher(
        "pi",
        Path::new(r"C:\venvs\tools\SCRIPTS\PI.EXE")
    ));
    assert!(!is_foreign_pip_launcher(
        "pi",
        Path::new(r"D:\Scripts\pi.cmd")
    ));
    assert!(!is_foreign_pip_launcher(
        "pi",
        Path::new(r"C:\Users\me\.bun\bin\pi.exe")
    ));
    assert!(!is_foreign_pip_launcher(
        "hermes",
        Path::new(r"D:\soft\Python\Python311\Scripts\hermes.exe")
    ));

    assert!(is_vscode_copilot_shim(
        "copilot",
        Path::new(r"D:\Microsoft VS Code\bin\copilot-shim\copilot.exe")
    ));
    assert!(is_vscode_copilot_shim(
        "copilot",
        Path::new(r"C:\Program Files\Microsoft VS Code\bin\COPILOT-SHIM\copilot.cmd")
    ));
    assert!(!is_vscode_copilot_shim(
        "copilot",
        Path::new(r"C:\Users\me\AppData\Roaming\npm\copilot.cmd")
    ));
    assert!(!is_vscode_copilot_shim(
        "codex",
        Path::new(r"D:\x\copilot-shim\codex.exe")
    ));
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

#[cfg(target_os = "windows")]
#[test]
fn expand_env_chars_resolves_nested_vars_and_guards_cycles() {
    assert_eq!(
        expand_env_chars(r"C:\Program Files\nodejs"),
        r"C:\Program Files\nodejs"
    );
    assert_eq!(expand_env_chars(r"C:\path\%%\tail"), r"C:\path\%%\tail");

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
fn windows_path_lookup_ignores_same_named_file_in_current_directory() {
    let current_dir = tempfile::tempdir().expect("current directory should be created");
    let path_dir = tempfile::tempdir().expect("PATH directory should be created");
    std::fs::write(current_dir.path().join("codex.cmd"), "@echo current\r\n").unwrap();
    let expected = path_dir.path().join("codex.cmd");
    std::fs::write(&expected, "@echo path\r\n").unwrap();

    let effective_path = std::env::join_paths([path_dir.path()]).unwrap();
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
        std::fs::canonicalize(&matches[0]).unwrap(),
        std::fs::canonicalize(&expected).unwrap()
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_opencode_desktop_sidecar_resolution() {
    // 1. Sibling sidecar replaces desktop GUI executable.
    let dir = tempfile::tempdir().unwrap();
    let gui = dir.path().join("opencode.exe");
    let sidecar = dir.path().join("opencode-cli.exe");
    std::fs::write(&gui, "").unwrap();
    std::fs::write(&sidecar, "").unwrap();
    assert_eq!(
        tool_executable_candidates("opencode", dir.path()),
        vec![dir.path().join("opencode.cmd"), sidecar.clone()]
    );
    assert_eq!(
        windows_preferred_tool_executable("opencode", &gui),
        Some(sidecar)
    );

    // 2. Resources sidecar replaces Electron desktop app.
    let dir = tempfile::tempdir().unwrap();
    let gui = dir.path().join("opencode.exe");
    let resources = dir.path().join("resources");
    let sidecar = resources.join("opencode-cli.exe");
    std::fs::create_dir(&resources).unwrap();
    std::fs::write(&gui, "").unwrap();
    std::fs::write(resources.join("app.asar"), "").unwrap();
    std::fs::write(&sidecar, "").unwrap();
    assert_eq!(
        windows_preferred_tool_executable("opencode", &gui),
        Some(sidecar)
    );

    // 3. Electron desktop without sidecar is skipped in favor of next CLI on PATH.
    let desktop_dir = tempfile::tempdir().unwrap();
    let resources = desktop_dir.path().join("resources");
    std::fs::create_dir(&resources).unwrap();
    let desktop = desktop_dir.path().join("opencode.exe");
    std::fs::write(&desktop, "").unwrap();
    std::fs::write(resources.join("app.asar"), "").unwrap();
    assert_eq!(
        windows_preferred_tool_executable("opencode", &desktop),
        None
    );

    let cli_dir = tempfile::tempdir().unwrap();
    let standalone = cli_dir.path().join("opencode.exe");
    std::fs::write(&standalone, "").unwrap();
    assert_eq!(
        windows_preferred_tool_executable("opencode", &standalone),
        Some(standalone.clone())
    );
    let raw = format!("{}\r\n{}\r\n", desktop.display(), standalone.display());
    assert_eq!(
        windows_preferred_path_default("opencode", &raw),
        Some(standalone)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_executable_candidates_filter_foreign_and_shadowed_shims() {
    let root = tempfile::tempdir().unwrap();
    let scripts = root.path().join("Python311").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let pip_pi = scripts.join("pi.exe");
    std::fs::write(&pip_pi, "").unwrap();
    assert!(tool_executable_candidates("pi", &scripts)
        .iter()
        .all(|c| c != &pip_pi));

    let npm = root.path().join("npm");
    std::fs::create_dir_all(&npm).unwrap();
    let npm_pi = npm.join("pi.cmd");
    std::fs::write(&npm_pi, "").unwrap();
    let raw = format!("{}\r\n{}\r\n", pip_pi.display(), npm_pi.display());
    assert_eq!(windows_preferred_path_default("pi", &raw), Some(npm_pi));
    assert_eq!(
        windows_preferred_path_default("pi", &format!("{}\r\n", pip_pi.display())),
        None
    );

    // Shadowed extensionless npm unix shim is skipped in favor of `.cmd`.
    let dir = tempfile::tempdir().unwrap();
    let extensionless = dir.path().join("codex");
    let cmd = dir.path().join("codex.cmd");
    std::fs::write(&extensionless, "").unwrap();
    std::fs::write(&cmd, "").unwrap();
    assert_eq!(
        tool_executable_candidates("codex", dir.path()),
        vec![cmd.clone(), dir.path().join("codex.exe")]
    );
    assert_eq!(
        windows_runnable_sibling_for_extensionless_tool(&extensionless).as_deref(),
        Some(cmd.as_path())
    );
}

#[cfg(target_os = "windows")]
#[test]
fn run_windows_tool_version_command_accepts_canonicalized_cmd_path() {
    let dir = tempfile::tempdir().unwrap();
    let cmd = dir.path().join("codex.cmd");
    std::fs::write(&cmd, "@echo off\r\necho codex-cli 0.144.3\r\n").unwrap();
    let canonical = std::fs::canonicalize(&cmd).unwrap();
    assert!(canonical.to_string_lossy().starts_with(r"\\?\"));

    let current_path = std::env::var("PATH").unwrap_or_default();
    let output = run_windows_tool_version_command(&canonical, &current_path)
        .expect("canonicalized cmd shim should execute");
    assert!(
        output.status.success(),
        "cmd shim failed: {}",
        decode_command_output(&output.stderr)
    );
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

fn main() {
    tauri_build::build();

    build_widget_reload_shim();

    // Windows: Embed Common Controls v6 manifest for test binaries
    //
    // When running `cargo test`, the generated test executables don't include
    // the standard Tauri application manifest. Without Common Controls v6,
    // `tauri::test` calls fail with STATUS_ENTRYPOINT_NOT_FOUND.
    //
    // This workaround:
    // 1. Embeds the manifest into test binaries via /MANIFEST:EMBED
    // 2. Uses /MANIFEST:NO for the main binary to avoid duplicate resources
    //    (Tauri already handles manifest embedding for the app binary)
    #[cfg(target_os = "windows")]
    {
        let manifest_path = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
        )
        .join("common-controls.manifest");
        let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg={}", manifest_arg);
        // Avoid duplicate manifest resources in binary builds.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    }
}

/// macOS：把 `macos/WidgetReload.swift`（调 `WidgetCenter.reloadAllTimelines()`）编成静态库链进来，
/// 写完小组件快照后通知系统马上重读。没有 Xcode / swiftc 时不中断构建，只是小组件要等系统自己的
/// 刷新时间（`widget_reload_unavailable`）。
fn build_widget_reload_shim() {
    println!("cargo:rustc-check-cfg=cfg(widget_reload_unavailable)");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
    );
    let source = manifest_dir.join("macos").join("WidgetReload.swift");
    println!("cargo:rerun-if-changed={}", source.display());

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("missing OUT_DIR"));
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        other => {
            println!("cargo:warning=widget reload shim: unsupported arch {other:?}");
            println!("cargo:rustc-cfg=widget_reload_unavailable");
            return;
        }
    };
    // 和 Rust 那边用同一个最低系统版本，链接时才不会报「object built for newer macOS」
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    let deployment_target =
        std::env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "12.0".to_string());
    let lib = out_dir.join("libccswitch_widget_reload.a");
    let status = std::process::Command::new("xcrun")
        .args([
            "--sdk",
            "macosx",
            "swiftc",
            "-parse-as-library",
            "-emit-library",
            "-static",
        ])
        .args(["-module-name", "CCSwitchWidgetReload", "-O"])
        .arg("-target")
        .arg(format!("{arch}-apple-macos{deployment_target}"))
        .arg(&source)
        .arg("-o")
        .arg(&lib)
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        println!("cargo:warning=widget reload shim: swiftc failed ({status:?}); widgets will refresh on the system schedule only");
        println!("cargo:rustc-cfg=widget_reload_unavailable");
        return;
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=ccswitch_widget_reload");
    println!("cargo:rustc-link-lib=framework=WidgetKit");
    // Swift 运行时：系统自带的动态库，以及工具链里的兼容静态库
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    if let Ok(output) = std::process::Command::new("xcrun")
        .args(["--sdk", "macosx", "--find", "swiftc"])
        .output()
    {
        let swiftc = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some(toolchain_usr) = std::path::Path::new(&swiftc)
            .parent()
            .and_then(|p| p.parent())
        {
            let compat = toolchain_usr.join("lib/swift/macosx");
            println!("cargo:rustc-link-search=native={}", compat.display());
        }
    }
}

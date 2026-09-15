fn main() {
    tauri_build::build();

    // Windows: Embed Common Controls v6 manifest for test binaries
    //
    // When running `cargo test`, the generated test executables don't include
    // the standard Tauri application manifest. Without Common Controls v6,
    // comctl32.dll resolves to the legacy v5 export set and tests fail at
    // process startup with STATUS_ENTRYPOINT_NOT_FOUND.
    //
    // The embedding mechanism is target-env specific:
    // - MSVC: /MANIFEST:EMBED + /MANIFESTINPUT (MSVC link.exe 专有参数)
    // - GNU : 用 windres 把清单编译为 COFF 对象后直接链接（ld 不识别 /MANIFEST*）
    //
    // `/MANIFEST:NO` 用于避免与主二进制（Tauri 已嵌入清单）重复。
    #[cfg(target_os = "windows")]
    {
        let manifest_path = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
        )
        .join("common-controls.manifest");
        println!("cargo:rerun-if-changed={}", manifest_path.display());

        match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => {
                let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());
                println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
                println!("cargo:rustc-link-arg={}", manifest_arg);
                println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
            }
            Ok("gnu") => {
                let windres = "windres";
                let out_dir =
                    std::path::PathBuf::from(std::env::var("OUT_DIR").expect("missing OUT_DIR"));
                // RC 资源：type 24 = RT_MANIFEST，id 1 = 应用程序清单
                let rc_path = out_dir.join("common_controls.rc");
                let manifest_fwd = manifest_path
                    .display()
                    .to_string()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                std::fs::write(&rc_path, format!("1 24 \"{}\"\n", manifest_fwd))
                    .expect("write common_controls.rc");
                let obj_path = out_dir.join("common_controls.manifest.o");
                let status = std::process::Command::new(windres)
                    .args([
                        rc_path.to_string_lossy().as_ref(),
                        "-O",
                        "coff",
                        "-o",
                        obj_path.to_string_lossy().as_ref(),
                    ])
                    .status();
                match status {
                    Ok(s) if s.success() => {
                        println!("cargo:rustc-link-arg={}", obj_path.display());
                    }
                    _ => {
                        // windres 不可用（非 mingw 构建环境）：跳过清单嵌入，
                        // 仅影响 tauri::test 类测试二进制，不影响应用构建
                        println!("cargo:warning=windres not available; skip manifest embedding for gnu target");
                    }
                }
            }
            _ => {}
        }
    }
}

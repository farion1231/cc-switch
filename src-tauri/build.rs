fn main() {
    // The linker embeds the same manifest for the app and its test executables.
    // Keep Tauri's icon/version resources without embedding a second copy.
    #[cfg(target_os = "windows")]
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    #[cfg(not(target_os = "windows"))]
    let attributes = tauri_build::Attributes::new();
    tauri_build::try_build(attributes).expect("failed to run Tauri build script");

    // Windows: Embed Common Controls v6 manifest for test binaries
    //
    // When running `cargo test`, the generated test executables don't include
    // the standard Tauri application manifest. Without Common Controls v6,
    // `tauri::test` calls fail with STATUS_ENTRYPOINT_NOT_FOUND.
    //
    // Embed once through the linker for all targets, including library unit
    // tests. Cargo's rustc-link-arg-tests only covers integration-test targets.
    #[cfg(target_os = "windows")]
    {
        let manifest_path = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
        )
        .join("common-controls.manifest");
        let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg={}", manifest_arg);
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    }
}

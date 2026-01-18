fn main() {
    tauri_build::build();

    // Ensure Common Controls v6 is available at runtime on Windows.
    //
    // Some test executables may import `TaskDialogIndirect` from `comctl32.dll` (via dialog crates).
    // Without a Common Controls v6 manifest dependency, Windows may bind to the v5.82 DLL which
    // does not export that symbol, causing `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }
}

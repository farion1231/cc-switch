use std::path::Path;

fn main() {
    tauri_build::build();
    generate_webui_assets();

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

fn generate_webui_assets() {
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let dist_dir = manifest_dir.join("..").join("dist");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("missing OUT_DIR"));
    let output_path = out_dir.join("webui_assets.rs");

    println!("cargo:rerun-if-changed={}", dist_dir.display());

    let mut assets = Vec::new();
    if dist_dir.is_dir() {
        collect_assets(&dist_dir, &dist_dir, &mut assets);
    }
    assets.sort_by(|a, b| a.0.cmp(&b.0));

    let mut output = String::new();
    output.push_str(
        "pub struct WebUiAsset {\n    pub path: &'static str,\n    pub mime: &'static str,\n    pub bytes: &'static [u8],\n}\n\n",
    );
    output.push_str(&format!(
        "pub const ASSET_COUNT: usize = {};\n\n",
        assets.len()
    ));
    if assets.is_empty() {
        output.push_str("pub fn get(_path: &str) -> Option<WebUiAsset> {\n    None\n}\n");
    } else {
        output.push_str("pub fn get(path: &str) -> Option<WebUiAsset> {\n    match path {\n");

        for (relative_path, full_path) in &assets {
            output.push_str(&format!(
                "        {:?} => Some(WebUiAsset {{ path: {:?}, mime: {:?}, bytes: include_bytes!({:?}) }}),\n",
                relative_path,
                relative_path,
                mime_for_path(relative_path),
                full_path.to_string_lossy()
            ));
        }

        output.push_str("        _ => None,\n    }\n}\n");
    }

    let mut file = fs::File::create(output_path).expect("create webui_assets.rs");
    file.write_all(output.as_bytes())
        .expect("write webui_assets.rs");
}

fn collect_assets(
    root: &std::path::Path,
    dir: &std::path::Path,
    assets: &mut Vec<(String, std::path::PathBuf)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_assets(root, &path, assets);
            continue;
        }

        if !path.is_file() {
            continue;
        }

        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = normalize_asset_path(relative);
        assets.push((relative, path));
    }
}

fn normalize_asset_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn mime_for_path(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
}

//! 内置 API Key 脚本模板：预览、ZIP 和真实多文件 Node 执行保持一致。

use std::io::{Read, Write};

const EXPECTED_TEMPLATE_PATHS: [&str; 6] = [
    "index.mjs",
    "config.mjs",
    "lib/provider.mjs",
    "lib/contract.mjs",
    "README.zh-CN.md",
    "README.md",
];

use cc_switch_lib::api_key_vars::{self, template, ApiKeyVarInput};

#[path = "support.rs"]
#[allow(dead_code)]
mod support;
use support::{ensure_test_home, test_mutex};

struct EnvVarGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var(self.name, value);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

#[test]
fn template_zip_has_exactly_the_preview_files_and_contents() {
    let preview = template::files();
    let bytes = template::zip_bytes().expect("build template ZIP");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open template ZIP");

    assert_eq!(
        preview
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        EXPECTED_TEMPLATE_PATHS
    );
    assert_eq!(archive.len(), EXPECTED_TEMPLATE_PATHS.len());
    for expected in preview {
        let archive_path = format!("api-key-refresh-template/{}", expected.path);
        let mut file = archive
            .by_name(&archive_path)
            .expect("ZIP contains preview file");
        let mut actual = String::new();
        file.read_to_string(&mut actual).expect("read ZIP content");
        assert_eq!(
            actual, expected.content,
            "content differs for {archive_path}"
        );
    }
}

#[test]
fn unpacked_template_runs_from_a_unicode_space_path_and_masks_the_result() {
    let _guard = test_mutex().lock().expect("test mutex");
    let home = ensure_test_home();
    let temp = tempfile::Builder::new()
        .prefix("API Key 模板 空格 ")
        .tempdir_in(home)
        .expect("create template test directory");
    let destination = temp.path().join("api-key-refresh-template");
    let bytes = template::zip_bytes().expect("build template ZIP");
    let saved_zip = temp.path().join("下载 模板.zip");
    std::fs::write(&saved_zip, b"old template ZIP").expect("prepare existing ZIP");
    template::write_zip(&saved_zip, &bytes).expect("atomically save template ZIP");
    let saved_bytes = std::fs::read(&saved_zip).expect("read saved template ZIP");
    assert_eq!(saved_bytes, bytes);
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(saved_bytes)).expect("open saved template ZIP");

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("open ZIP entry");
        let enclosed = entry
            .enclosed_name()
            .expect("safe ZIP entry path")
            .to_owned();
        assert!(
            enclosed.starts_with("api-key-refresh-template"),
            "template ZIP entry is rooted correctly: {}",
            enclosed.display()
        );
        let path = temp.path().join(enclosed);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create extracted parent");
        }
        let mut output = std::fs::File::create(&path).expect("create extracted file");
        std::io::copy(&mut entry, &mut output).expect("extract template file");
        output.flush().expect("flush extracted file");
    }

    // Do not inherit a developer's HTTP template override. This fixture must
    // remain offline even when the parent process has real provider settings.
    let _demo_mode = EnvVarGuard::set("API_KEY_TEMPLATE_MODE", "demo");
    let _timeout = EnvVarGuard::set("API_KEY_REFRESH_TIMEOUT_SECONDS", "20");
    let input = ApiKeyVarInput {
        name: "TEMPLATE_DEMO_KEY".to_string(),
        script_dir: destination.to_string_lossy().into_owned(),
        enabled: true,
        timeout_secs: Some(10),
        lead_time_secs: None,
    };
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Node test runtime")
        .block_on(api_key_vars::test_script(&input))
        .expect("template demo should execute");

    assert_eq!(outcome.file, "index.mjs");
    assert!(outcome.api_key_masked.starts_with("demo"));
    assert!(outcome.api_key_masked.contains('…'));
    assert!(!outcome.api_key_masked.contains("not-a-real-key"));
    assert!(outcome.expires_at.expect("demo expiry") > chrono::Utc::now().timestamp_millis());
    assert!(
        outcome.refresh_after.expect("demo refresh schedule")
            > chrono::Utc::now().timestamp_millis()
    );
    assert_eq!(
        outcome.message.as_deref(),
        Some("Demo key only; configure HTTP mode before using a real provider.")
    );
}

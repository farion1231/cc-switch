//! API Key 变量脚本模板。
//!
//! 模板文本在编译期嵌入二进制，页面预览和下载的 ZIP 都从同一个清单生成，避免两份
//! 内容随着版本演进而漂移。

use std::io::{Cursor, Write};
use std::path::Path;

use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::DateTime;

const ZIP_ROOT: &str = "api-key-refresh-template";

/// 前端代码预览使用的单个模板文件。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiKeyVarTemplateFile {
    pub path: String,
    pub content: String,
}

struct EmbeddedTemplateFile {
    path: &'static str,
    content: &'static str,
}

const FILES: &[EmbeddedTemplateFile] = &[
    EmbeddedTemplateFile {
        path: "index.mjs",
        content: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/api-key-var-template/index.mjs"
        )),
    },
    EmbeddedTemplateFile {
        path: "config.mjs",
        content: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/api-key-var-template/config.mjs"
        )),
    },
    EmbeddedTemplateFile {
        path: "lib/provider.mjs",
        content: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/api-key-var-template/lib/provider.mjs"
        )),
    },
    EmbeddedTemplateFile {
        path: "lib/contract.mjs",
        content: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/api-key-var-template/lib/contract.mjs"
        )),
    },
    EmbeddedTemplateFile {
        path: "README.zh-CN.md",
        content: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/api-key-var-template/README.zh-CN.md"
        )),
    },
    EmbeddedTemplateFile {
        path: "README.md",
        content: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/api-key-var-template/README.md"
        )),
    },
];

/// Returns the exact source files used for both the UI preview and ZIP archive.
pub fn files() -> Vec<ApiKeyVarTemplateFile> {
    FILES
        .iter()
        .map(|file| ApiKeyVarTemplateFile {
            path: file.path.to_string(),
            content: file.content.to_string(),
        })
        .collect()
}

/// Builds a deterministic ZIP archive containing the same file manifest returned by [`files`].
pub fn zip_bytes() -> Result<Vec<u8>, String> {
    let mut output = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut output);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());

    for file in FILES {
        let zip_path = format!("{ZIP_ROOT}/{}", file.path);
        writer
            .start_file(zip_path, options)
            .map_err(|error| format!("创建模板 ZIP 条目失败: {error}"))?;
        writer
            .write_all(file.content.as_bytes())
            .map_err(|error| format!("写入模板 ZIP 条目失败: {error}"))?;
    }
    writer
        .finish()
        .map_err(|error| format!("完成模板 ZIP 失败: {error}"))?;
    Ok(output.into_inner())
}

/// Atomically writes a generated template archive to the user-selected path.
pub fn write_zip(path: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::config::atomic_write(path, bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_bytes_are_deterministic() {
        assert_eq!(
            zip_bytes().expect("build first template ZIP"),
            zip_bytes().expect("build second template ZIP")
        );
    }
}

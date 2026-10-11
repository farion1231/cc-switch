//! 供应商自定义图标资产存储（`<app_config>/assets/icons/`）。
//!
//! 供应商行通过 `icon_source = "local"` + `icon = <asset key>` 引用这里的文件；
//! 校验（magic bytes / 尺寸上限）、存储命名与路径安全都在本模块收敛，供
//! Tauri 命令与同步归档共用。issue #8117。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::get_app_config_dir;
use crate::error::AppError;

/// 光栅图体积上限（issue 提案：≤200 KB）。
pub const MAX_RASTER_BYTES: usize = 200 * 1024;
/// SVG 体积上限 — issue 未给出数值，按计划 §7 Q5 取 100 KB。
pub const MAX_SVG_BYTES: usize = 100 * 1024;
/// 光栅图边长上限（issue 提案：≤256×256，正方形优先）；SVG 豁免像素校验。
pub const MAX_DIMENSION: u32 = 256;

/// 同一供应商允许同时存在的资产文件数上限（防异常写入堆积）。
const MAX_ASSETS_PER_PROVIDER: usize = 8;

const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "svg"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconImageKind {
    Png,
    Jpeg,
    Webp,
    Svg,
}

impl IconImageKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Svg => "svg",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Svg => "image/svg+xml",
        }
    }

    fn max_bytes(self) -> usize {
        match self {
            Self::Svg => MAX_SVG_BYTES,
            _ => MAX_RASTER_BYTES,
        }
    }
}

/// 保存成功后返回给前端的元数据。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedProviderIcon {
    /// 存入 provider.icon（icon_source = "local"）的资产键。
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub byte_size: usize,
    pub mime_type: String,
}

// ─── 路径与命名 ──────────────────────────────────────────────

/// 图标资产目录（`<app_config>/assets/icons/`），不存在则创建。
pub fn icons_dir() -> Result<PathBuf, AppError> {
    let dir = get_app_config_dir().join("assets").join("icons");
    fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    Ok(dir)
}

/// 供应商 ID 收敛成安全文件名主干：仅保留字母数字与 `-_`，其余折叠为 `_`，
/// 空结果回落到 `provider`，并限制长度。
fn sanitize_provider_stem(provider_id: &str) -> String {
    let mut stem: String = provider_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if stem.is_empty() {
        stem = "provider".to_string();
    }
    stem.truncate(64);
    stem
}

/// 供应商资产的文件名前缀（`{stem}-`），delete/清理按它圈定范围。
fn provider_asset_prefix(provider_id: &str) -> String {
    format!("{}-", sanitize_provider_stem(provider_id))
}

/// 资产键即文件名：`{provider_stem}-{hash8}.{ext}`。同步归档恢复时也用它
/// 过滤非法条目。
pub fn is_valid_asset_key(key: &str) -> bool {
    let name = Path::new(key);
    if key.is_empty()
        || name.components().count() != 1
        || key.contains('/')
        || key.contains('\\')
        || key.starts_with('.')
    {
        return false;
    }
    let Some(dot) = key.rfind('.') else {
        return false;
    };
    let (stem, ext) = key.split_at(dot);
    let ext = &ext[1..];
    if !SUPPORTED_EXTENSIONS.contains(&ext) {
        return false;
    }
    // stem 必须形如 `{安全前缀}{8 位十六进制 hash}`
    let Some(hash) = stem.rsplit_once('-').map(|(_, h)| h) else {
        return false;
    };
    hash.len() == 8 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// 把资产键解析为图标目录内的安全路径；不合法的键返回 None（调用方静默回退）。
fn resolve_asset_path(key: &str) -> Option<PathBuf> {
    if !is_valid_asset_key(key) {
        return None;
    }
    Some(icons_dir().ok()?.join(key))
}

// ─── 内容校验 ────────────────────────────────────────────────

/// magic bytes 嗅探图片类型；不认识的类型返回 None。
pub fn detect_image_kind(bytes: &[u8]) -> Option<IconImageKind> {
    if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(IconImageKind::Png);
    }
    if bytes.len() >= 3 && bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(IconImageKind::Jpeg);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(IconImageKind::Webp);
    }
    if looks_like_svg(bytes) {
        return Some(IconImageKind::Svg);
    }
    None
}

/// SVG 内容嗅探：跳过 BOM 与空白后，首标签必须是 `<svg` 或 `<?xml`（且含 `<svg`）。
/// SVG 经 `<img>` 渲染不执行脚本，这是既定的 XSS 缓解措施（见计划 §7）。
fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let trimmed: &[u8] = {
        let mut t = head;
        if t.starts_with(b"\xEF\xBB\xBF") {
            t = &t[3..];
        }
        while let Some((first, rest)) = t.split_first() {
            if first.is_ascii_whitespace() {
                t = rest;
            } else {
                break;
            }
        }
        t
    };
    if trimmed.starts_with(b"<svg") {
        return true;
    }
    if trimmed.starts_with(b"<?xml") && find_subslice(trimmed, b"<svg").is_some() {
        return true;
    }
    false
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 轻量头部解析图片像素尺寸；解析不出返回 None（SVG 豁免，不调用本函数）。
pub fn probe_image_dimensions(kind: IconImageKind, bytes: &[u8]) -> Option<(u32, u32)> {
    match kind {
        IconImageKind::Svg => None,
        IconImageKind::Png => probe_png_dimensions(bytes),
        IconImageKind::Jpeg => probe_jpeg_dimensions(bytes),
        IconImageKind::Webp => probe_webp_dimensions(bytes),
    }
}

fn probe_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // 8 字节签名 + 4 字节长度 + "IHDR" + 4 宽 + 4 高
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

fn probe_jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2usize;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // 填充字节
        if marker == 0xFF {
            i += 1;
            continue;
        }
        // SOF0–SOF15（跳过 DHT/JPG/DAC）：含图像尺寸
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        let segment_len = u16::from_be_bytes(bytes[i + 2..i + 4].try_into().ok()?) as usize;
        if is_sof {
            if i + 9 > bytes.len() {
                return None;
            }
            let height = u16::from_be_bytes(bytes[i + 5..i + 7].try_into().ok()?) as u32;
            let width = u16::from_be_bytes(bytes[i + 7..i + 9].try_into().ok()?) as u32;
            return Some((width, height));
        }
        i += 2 + segment_len;
    }
    None
}

fn probe_webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"WEBP" {
        return None;
    }
    match &bytes[12..16] {
        b"VP8X" => {
            // 24 位小端 canvas 尺寸（减 1 后的实际值），偏移 24/27
            let read24 = |off: usize| -> u32 {
                bytes[off] as u32 | (bytes[off + 1] as u32) << 8 | (bytes[off + 2] as u32) << 16
            };
            Some((read24(24) + 1, read24(27) + 1))
        }
        b"VP8 " => {
            // 帧头：3 字节 chunk 长度 + 3 字节帧标签，宽高在 26/28 起（14 位小端）
            let width = (u16::from_le_bytes(bytes[26..28].try_into().ok()?) & 0x3FFF) as u32;
            let height = (u16::from_le_bytes(bytes[28..30].try_into().ok()?) & 0x3FFF) as u32;
            Some((width, height))
        }
        b"VP8L" => {
            // 位流：14 位宽 - 1、14 位高 - 1，从偏移 21 起按位读
            let b = |off: usize| bytes[off] as u32;
            let bits = b(21) | b(22) << 8 | b(23) << 16 | b(24) << 24;
            let width = (bits & 0x3FFF) + 1;
            let height = ((bits >> 14) & 0x3FFF) + 1;
            Some((width, height))
        }
        _ => None,
    }
}

/// 校验图标字节：类型嗅探、体积上限、光栅图尺寸上限。
/// 返回（类型, 尺寸）；SVG 尺寸为 None。
pub fn validate_icon_bytes(bytes: &[u8]) -> Result<(IconImageKind, Option<(u32, u32)>), AppError> {
    let Some(kind) = detect_image_kind(bytes) else {
        return Err(AppError::localized(
            "providerIcon.unsupportedType",
            "不支持的图片格式（仅支持 PNG / JPG / WEBP / SVG）",
            "Unsupported image type (only PNG / JPG / WEBP / SVG are supported)",
        ));
    };

    if bytes.len() > kind.max_bytes() {
        let max_kb = kind.max_bytes() / 1024;
        return Err(AppError::localized(
            "providerIcon.tooLarge",
            format!("图片超过大小上限（{max_kb} KB）"),
            format!("Image exceeds the size limit ({max_kb} KB)"),
        ));
    }

    if kind != IconImageKind::Svg {
        let Some((width, height)) = probe_image_dimensions(kind, bytes) else {
            return Err(AppError::localized(
                "providerIcon.unreadableImage",
                "无法读取图片尺寸，文件可能已损坏",
                "Unable to read image dimensions; the file may be corrupted",
            ));
        };
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(AppError::localized(
                "providerIcon.tooLargeDimensions",
                format!("图片尺寸超过上限（{MAX_DIMENSION}×{MAX_DIMENSION}）"),
                format!("Image dimensions exceed the limit ({MAX_DIMENSION}×{MAX_DIMENSION})"),
            ));
        }
        return Ok((kind, Some((width, height))));
    }

    Ok((kind, None))
}

// ─── 存取 ────────────────────────────────────────────────────

/// 把 picked 文件复制（非移动）进图标目录，校验后以
/// `{provider_stem}-{content_hash8}.{ext}` 命名，并清理该供应商的旧资产。
pub fn save_provider_icon(
    provider_id: &str,
    source_path: &Path,
) -> Result<SavedProviderIcon, AppError> {
    let bytes = fs::read(source_path).map_err(|e| AppError::io(source_path, e))?;
    let (kind, dimensions) = validate_icon_bytes(&bytes)?;

    let dir = icons_dir()?;
    let hash = sha256_hex8(&bytes);
    let key = format!(
        "{}-{}.{}",
        sanitize_provider_stem(provider_id),
        hash,
        kind.extension()
    );
    let dest = dir.join(&key);

    // 同内容已存在则幂等；否则先写临时名再改名，避免半写文件被当作资产。
    if !dest.exists() {
        let tmp = dir.join(format!(".{key}.tmp"));
        fs::write(&tmp, &bytes).map_err(|e| AppError::io(&tmp, e))?;
        fs::rename(&tmp, &dest).map_err(|e| AppError::io(&dest, e))?;
    }

    // 内容变化时同前缀旧文件成为孤儿，顺手清掉（保留当前 key）。
    cleanup_provider_assets(provider_id, &key);

    let (width, height) = dimensions.unwrap_or((0, 0));
    Ok(SavedProviderIcon {
        key,
        width,
        height,
        byte_size: bytes.len(),
        mime_type: kind.mime_type().to_string(),
    })
}

/// 删除一个供应商的全部图标资产（provider 删除 / 图标重置时调用，best-effort）。
pub fn delete_provider_icons(provider_id: &str) -> Result<(), AppError> {
    cleanup_provider_assets(provider_id, "");
    Ok(())
}

/// 按资产键精确删除单个文件（供应商删除时清掉实际引用的那份）。
pub fn delete_provider_asset_by_key(key: &str) -> Result<(), AppError> {
    if let Some(path) = resolve_asset_path(key) {
        if path.is_file() {
            fs::remove_file(&path).map_err(|e| AppError::io(&path, e))?;
        }
    }
    Ok(())
}

/// 读取资产并编码为 `data:` URL；文件缺失返回 None（渲染层静默回退）。
pub fn read_provider_icon_data_url(key: &str) -> Result<Option<String>, AppError> {
    let Some(path) = resolve_asset_path(key) else {
        return Ok(None);
    };
    let Ok(bytes) = fs::read(&path) else {
        return Ok(None);
    };
    let Some(kind) = detect_image_kind(&bytes) else {
        return Ok(None);
    };
    Ok(Some(format!(
        "data:{};base64,{}",
        kind.mime_type(),
        use_base64_encode(&bytes)
    )))
}

/// 删除同一供应商前缀下、除 `keep` 之外的资产文件；超出单供应商上限的
/// 历史堆积也一并收敛。keep 传空串表示全删。
fn cleanup_provider_assets(provider_id: &str, keep: &str) {
    let Ok(dir) = icons_dir() else {
        return;
    };
    let prefix = provider_asset_prefix(provider_id);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix) || !is_valid_asset_key(name) {
            continue;
        }
        if name == keep {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
        if removed >= MAX_ASSETS_PER_PROVIDER {
            break;
        }
    }
}

fn sha256_hex8(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{:x}", digest).chars().take(8).collect()
}

/// base64 标准编码（复用 workspace 已有 base64 crate）。
fn use_base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 文件系统相关测试串行化（CC_SWITCH_TEST_HOME 是进程级全局态）。
#[cfg(test)]
fn test_home_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

// ─── 测试 ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth, color type, …
        bytes.extend_from_slice(&[0, 0, 0, 0]); // 占位 CRC（校验不读 CRC）
        bytes
    }

    #[test]
    fn detect_rejects_unknown_magic() {
        // .txt 改名 .png 的常见用户错误
        assert_eq!(detect_image_kind(b"hello world"), None);
        assert_eq!(detect_image_kind(b""), None);
        assert_eq!(detect_image_kind(b"GIF89a...."), None);
    }

    #[test]
    fn detect_accepts_each_supported_class() {
        assert_eq!(
            detect_image_kind(&minimal_png(1, 1)),
            Some(IconImageKind::Png)
        );
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        jpeg.extend_from_slice(b"JFIF");
        assert_eq!(detect_image_kind(&jpeg), Some(IconImageKind::Jpeg));
        let mut webp = b"RIFF\x00\x00\x00\x00WEBPVP8 ".to_vec();
        webp.extend_from_slice(&[0u8; 20]);
        assert_eq!(detect_image_kind(&webp), Some(IconImageKind::Webp));
        assert_eq!(
            detect_image_kind(b"<svg xmlns=\"x\"></svg>"),
            Some(IconImageKind::Svg)
        );
        assert_eq!(
            detect_image_kind(b"<?xml version=\"1.0\"?><svg xmlns=\"x\"></svg>"),
            Some(IconImageKind::Svg)
        );
    }

    #[test]
    fn raster_size_cap_rejects_over_200kb() {
        let png = minimal_png(1, 1);
        let mut bytes = png.clone();
        bytes.extend(std::iter::repeat_n(0u8, MAX_RASTER_BYTES)); // 超限
        let err = validate_icon_bytes(&bytes).expect_err("oversized raster must be rejected");
        assert!(
            err.to_string().contains("KB") || err.to_string().contains("上限"),
            "unexpected error: {err}"
        );
        // 恰好在限额内应通过
        assert!(validate_icon_bytes(&png).is_ok());
    }

    #[test]
    fn svg_size_cap_is_100kb() {
        let mut svg = b"<svg xmlns=\"x\"><rect/></svg>".to_vec();
        svg.extend(std::iter::repeat_n(b' ', MAX_SVG_BYTES));
        assert!(
            validate_icon_bytes(&svg).is_err(),
            "oversized svg must be rejected"
        );
    }

    #[test]
    fn dimension_cap_rejects_over_256() {
        let bytes = minimal_png(257, 128);
        let err = validate_icon_bytes(&bytes).expect_err("over-dimension raster must be rejected");
        assert!(
            err.to_string().contains("256") || err.to_string().contains("上限"),
            "unexpected error: {err}"
        );
        assert!(validate_icon_bytes(&minimal_png(256, 256)).is_ok());
    }

    #[test]
    fn dimension_cap_reads_jpeg_and_webp_headers() {
        // JPEG SOF0: FFC0, len=17, precision=8, h=64, w=128, …
        let mut jpeg = vec![
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x40, 0x00, 0x80,
        ];
        jpeg.extend_from_slice(&[0u8; 12]);
        assert_eq!(
            probe_image_dimensions(IconImageKind::Jpeg, &jpeg),
            Some((128, 64))
        );

        // WEBP VP8X: 4 字节 flags + 3 字节宽-1 + 3 字节高-1，canvas 100×50
        let mut webp = b"RIFF\x00\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00".to_vec();
        webp.extend_from_slice(&[0x10, 0, 0, 0]);
        webp.extend_from_slice(&99u32.to_le_bytes()[..3]);
        webp.extend_from_slice(&49u32.to_le_bytes()[..3]);
        assert_eq!(
            probe_image_dimensions(IconImageKind::Webp, &webp),
            Some((100, 50))
        );
    }

    #[test]
    fn asset_key_validation_blocks_traversal() {
        assert!(is_valid_asset_key("myprovider-ab12cd34.png"));
        for bad in [
            "../evil.png",
            "a/b.png",
            "a\\b.png",
            ".hidden-ab12cd34.png",
            "provider-short.png",
            "provider-<script>.png",
            "",
            "provider-ab12cd34.exe",
            "nohash.png",
        ] {
            assert!(!is_valid_asset_key(bad), "{bad} must be rejected");
        }
    }

    #[test]
    fn sanitize_provider_stem_is_filename_safe() {
        assert_eq!(sanitize_provider_stem("a/b\\c d"), "a_b_c_d");
        assert_eq!(sanitize_provider_stem(""), "provider");
        let long = "x".repeat(100);
        assert_eq!(sanitize_provider_stem(&long).len(), 64);
    }

    #[test]
    fn save_and_read_round_trip_with_isolated_home() {
        let _guard = super::test_home_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("temp home");
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

        let source = home.path().join("picked.png");
        std::fs::write(&source, minimal_png(64, 64)).expect("write picked png");

        let saved = save_provider_icon("prov/1", &source).expect("save icon");
        assert!(saved.key.starts_with("prov_1-"));
        assert!(saved.key.ends_with(".png"));
        assert_eq!(saved.mime_type, "image/png");
        assert_eq!((saved.width, saved.height), (64, 64));

        let data_url = read_provider_icon_data_url(&saved.key)
            .expect("read")
            .expect("asset exists");
        assert!(data_url.starts_with("data:image/png;base64,"));

        // 删除后读取静默回落 None
        delete_provider_icons("prov/1").expect("delete icons");
        assert!(read_provider_icon_data_url(&saved.key)
            .expect("read after delete")
            .is_none());

        match previous {
            Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    fn replacement_cleans_old_assets_but_keeps_current() {
        let _guard = super::test_home_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("temp home");
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", home.path());

        let source_a = home.path().join("a.png");
        let source_b = home.path().join("b.png");
        std::fs::write(&source_a, minimal_png(32, 32)).expect("write a");
        std::fs::write(&source_b, minimal_png(48, 48)).expect("write b");

        let first = save_provider_icon("prov2", &source_a).expect("save a");
        let second = save_provider_icon("prov2", &source_b).expect("save b");
        assert_ne!(first.key, second.key);

        let dir = icons_dir().expect("icons dir");
        let remaining: Vec<String> = std::fs::read_dir(&dir)
            .expect("read dir")
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("prov2-"))
            .collect();
        assert_eq!(
            remaining,
            vec![second.key.clone()],
            "old asset must be cleaned"
        );

        match previous {
            Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}

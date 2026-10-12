//! 供应商的自定义图标。
//!
//! `providers.icon` 原本只存内置图标名，用户上传的图片也存在这一列：前端把图片缩成
//! 小尺寸 PNG，以 `data:image/png;base64,…` 的形式写进来，随行数据一起同步、复制。
//! 原始文件（SVG、带 EXIF 的 JPEG 等）不进库。保存时用 [`validate_icon`] 完整解码一遍；
//! SQL 导入和云同步不经过保存，由 [`strip_invalid_custom_icons`] 在暂存库里清理
//! （`providers` 表和 settings 里的统一供应商 JSON 都要清）。
//! 前端渲染时另有一道同样的长度、宽高检查兜底（`src/utils/customIcon.ts`）。

use std::io::Cursor;

use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppError;

/// 自定义图标的前缀，和前端 `src/utils/customIcon.ts` 的 `CUSTOM_ICON_PREFIX` 一致。
pub const CUSTOM_ICON_PREFIX: &str = "data:image/png;base64,";

/// 整个 data URI 的长度上限，和前端的 `MAX_CUSTOM_ICON_LENGTH` 一致。
/// 前端输出 96×96 的 PNG，最坏情况（不可压缩）base64 后约 50 KB。
pub const MAX_CUSTOM_ICON_LENGTH: usize = 64 * 1024;

/// PNG 宽高上限，和前端的 `MAX_CUSTOM_ICON_DIMENSION` 一致。前端只输出 96×96，留出余量即可。
const MAX_CUSTOM_ICON_DIMENSION: u32 = 256;

/// 解码时允许分配的内存：256×256 的 16 位 RGBA 是 512 KiB，留一倍余量
const DECODE_LIMIT_BYTES: usize = 1024 * 1024;

/// 是不是自定义图标（任何 `data:` 开头的值都算，交给 [`validate_icon`] 判断是否合法）。
pub fn is_custom_icon(icon: &str) -> bool {
    icon.starts_with("data:")
}

/// 保存供应商前校验 `icon`。
///
/// 只管 `data:` 开头的值；其余字符串照旧当图标名放行（找不到对应内置图标时前端显示首字母），
/// 否则库里已有的旧值会让用户连编辑都保存不了。
pub fn validate_icon(icon: Option<&str>) -> Result<(), AppError> {
    match icon {
        Some(icon) if is_custom_icon(icon) && !is_valid_custom_icon(icon) => {
            Err(AppError::localized(
                "provider.icon.invalid",
                "自定义图标无效，请重新上传",
                "The custom icon is invalid, please upload it again",
            ))
        }
        _ => Ok(()),
    }
}

/// 完整解码一遍：签名、块校验和、像素数据都要完好，宽高在上限内，解码内存有上限。
fn is_valid_custom_icon(icon: &str) -> bool {
    if icon.len() > MAX_CUSTOM_ICON_LENGTH {
        return false;
    }
    let Some(encoded) = icon.strip_prefix(CUSTOM_ICON_PREFIX) else {
        return false;
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    let decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes.as_slice()),
        png::Limits {
            bytes: DECODE_LIMIT_BYTES,
        },
    );
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };
    let (width, height) = reader.info().size();
    // 先看宽高再分配像素缓冲区
    if width == 0
        || height == 0
        || width > MAX_CUSTOM_ICON_DIMENSION
        || height > MAX_CUSTOM_ICON_DIMENSION
    {
        return false;
    }
    let Some(size) = reader.output_buffer_size() else {
        return false;
    };
    let mut buf = vec![0; size];
    reader.next_frame(&mut buf).is_ok() && reader.finish().is_ok()
}

/// SQL 导入、云同步在替换主库前调用：把暂存库里不合格的自定义图标清空（退回首字母），
/// 其余数据原样保留，不让一个坏图标挡住整份备份。返回清掉的个数。
///
/// 图标存在两处：`providers.icon` 列，和 settings 表 `universal_providers` 键下的 JSON
/// （统一供应商）。两处都要清，后者同步到应用时会原样复制给子供应商。
pub fn strip_invalid_custom_icons(conn: &Connection) -> Result<usize, AppError> {
    let db_err = |e: rusqlite::Error| AppError::Database(e.to_string());

    let rows: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, app_type, icon FROM providers WHERE icon LIKE 'data:%'")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(db_err)?
            .collect::<Result<_, _>>()
            .map_err(db_err)?;
        rows
    };
    let mut stripped = 0;
    for (id, app_type, icon) in rows {
        if is_valid_custom_icon(&icon) {
            continue;
        }
        conn.execute(
            "UPDATE providers SET icon = NULL, icon_color = NULL WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(db_err)?;
        stripped += 1;
    }

    stripped += strip_invalid_universal_icons(conn)?;

    if stripped > 0 {
        log::warn!("导入的数据里有 {stripped} 个无效的自定义图标，已清空");
    }
    Ok(stripped)
}

/// 按 JSON 原样改写，不经过 `UniversalProvider` 结构体：较新版本写进来的未知字段要保留。
/// JSON 解析不了就不动，交给读取它的地方报错。
fn strip_invalid_universal_icons(conn: &Connection) -> Result<usize, AppError> {
    let db_err = |e: rusqlite::Error| AppError::Database(e.to_string());
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'universal_providers'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)?;
    let Some(raw) = raw else {
        return Ok(0);
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Ok(0);
    };
    let Some(providers) = value.as_object_mut() else {
        return Ok(0);
    };
    let mut stripped = 0;
    for provider in providers.values_mut() {
        let Some(provider) = provider.as_object_mut() else {
            continue;
        };
        let invalid = provider
            .get("icon")
            .and_then(|icon| icon.as_str())
            .is_some_and(|icon| is_custom_icon(icon) && !is_valid_custom_icon(icon));
        if invalid {
            provider.remove("icon");
            provider.remove("iconColor");
            stripped += 1;
        }
    }
    if stripped > 0 {
        conn.execute(
            "UPDATE settings SET value = ?1 WHERE key = 'universal_providers'",
            params![value.to_string()],
        )
        .map_err(db_err)?;
    }
    Ok(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用真正的编码器生成一张 PNG
    fn encode_png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let pixels: Vec<u8> = (0..width * height * 4).map(|i| (i % 251) as u8).collect();
            writer.write_image_data(&pixels).unwrap();
        }
        out
    }

    fn data_uri(bytes: &[u8]) -> String {
        format!(
            "{CUSTOM_ICON_PREFIX}{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    #[test]
    fn accepts_builtin_names_and_empty() {
        assert!(validate_icon(None).is_ok());
        assert!(validate_icon(Some("claude")).is_ok());
        // 库里可能有旧版深链接写进来的任意字符串，编辑时不能因此保存失败
        assert!(validate_icon(Some("https://example.com/logo.png")).is_ok());
    }

    #[test]
    fn accepts_a_real_small_png() {
        assert!(validate_icon(Some(&data_uri(&encode_png(96, 96)))).is_ok());
    }

    #[test]
    fn rejects_truncated_or_header_only_png() {
        let png = encode_png(96, 96);
        // 只有签名 + IHDR，没有像素数据
        assert!(validate_icon(Some(&data_uri(&png[..33]))).is_err());
        // 像素数据截了一半
        assert!(validate_icon(Some(&data_uri(&png[..png.len() / 2]))).is_err());
        // 缺 IEND
        assert!(validate_icon(Some(&data_uri(&png[..png.len() - 12]))).is_err());
        // 改坏一个字节，块校验和对不上
        let mut corrupted = png.clone();
        let mid = corrupted.len() / 2;
        corrupted[mid] ^= 0xff;
        assert!(validate_icon(Some(&data_uri(&corrupted))).is_err());
    }

    #[test]
    fn rejects_other_data_uris() {
        let svg = format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode("<svg onload=\"alert(1)\"/>")
        );
        assert!(validate_icon(Some(&svg)).is_err());
        assert!(validate_icon(Some("data:image/png;base64,!!!not-base64")).is_err());
        assert!(validate_icon(Some(&data_uri(b"GIF89a-padding-padding-padding"))).is_err());
    }

    #[test]
    fn rejects_oversized_png() {
        assert!(validate_icon(Some(&data_uri(&encode_png(300, 10)))).is_err());

        let mut big = encode_png(96, 96);
        big.resize(MAX_CUSTOM_ICON_LENGTH, 0);
        assert!(validate_icon(Some(&data_uri(&big))).is_err());
    }

    #[test]
    fn strips_only_invalid_icons_from_imported_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE providers (id TEXT, app_type TEXT, icon TEXT, icon_color TEXT);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);",
        )
        .unwrap();
        let good = data_uri(&encode_png(96, 96));
        let too_long = format!("{CUSTOM_ICON_PREFIX}{}", "A".repeat(MAX_CUSTOM_ICON_LENGTH));
        for (id, icon) in [
            ("good", good.as_str()),
            ("long", too_long.as_str()),
            ("builtin", "claude"),
        ] {
            conn.execute(
                "INSERT INTO providers VALUES (?1, 'claude', ?2, '#fff')",
                params![id, icon],
            )
            .unwrap();
        }

        assert_eq!(strip_invalid_custom_icons(&conn).unwrap(), 1);

        let icon_of = |id: &str| -> Option<String> {
            conn.query_row(
                "SELECT icon FROM providers WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(icon_of("good"), Some(good.clone()));
        assert_eq!(icon_of("long"), None);
        assert_eq!(icon_of("builtin"), Some("claude".to_string()));
    }

    #[test]
    fn strips_invalid_icons_from_universal_providers_and_keeps_other_fields() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE providers (id TEXT, app_type TEXT, icon TEXT, icon_color TEXT);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);",
        )
        .unwrap();
        let good = data_uri(&encode_png(96, 96));
        let header_only = data_uri(&encode_png(96, 96)[..33]);
        let universal = serde_json::json!({
            "u1": { "id": "u1", "name": "Bad", "icon": header_only, "iconColor": "#fff", "futureField": 1 },
            "u2": { "id": "u2", "name": "Good", "icon": good },
            "u3": { "id": "u3", "name": "Builtin", "icon": "claude" },
        });
        conn.execute(
            "INSERT INTO settings VALUES ('universal_providers', ?1)",
            params![universal.to_string()],
        )
        .unwrap();

        assert_eq!(strip_invalid_custom_icons(&conn).unwrap(), 1);

        let raw: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'universal_providers'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let after: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(after["u1"].get("icon").is_none());
        assert!(after["u1"].get("iconColor").is_none());
        assert_eq!(after["u1"]["futureField"], 1);
        assert_eq!(after["u2"]["icon"], good);
        assert_eq!(after["u3"]["icon"], "claude");
    }
}

//! 供应商的自定义图标。
//!
//! `providers.icon` 原本只存内置图标名，用户上传的图片也存在这一列：前端把图片缩成
//! 小尺寸 PNG，以 `data:image/png;base64,…` 的形式写进来，随行数据一起同步、复制。
//! 原始文件（SVG、带 EXIF 的 JPEG 等）不进库。保存时用 [`validate_icon`] 完整解码一遍；
//! SQL 导入和云同步不经过保存，由 [`strip_invalid_custom_icons`] 在暂存库里清理
//! （`providers` 表、settings 里的统一供应商 JSON 和「我的图标」库都要清）。
//! 「我的图标」是用户上传过的图片，存在 settings 的 `custom_icons` 键下，供选择器复用。
//! 前端渲染时另有一道同样的长度、宽高检查兜底（`src/utils/customIcon.ts`）。

use std::io::Cursor;

use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};

use crate::database::{CustomIconEntry, Database, CUSTOM_ICONS_KEY};
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
/// 图标存在三处：`providers.icon` 列、settings 表 `universal_providers` 键下的 JSON
/// （统一供应商，同步到应用时会原样复制给子供应商），以及 `custom_icons` 键下的图标库
/// （选中后原样写进供应商）。三处都要清。
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
    stripped += strip_invalid_library_icons(conn)?;

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

/// 图标库里 `data` 不是合格自定义图标的条目整条删掉。和统一供应商一样按 JSON 改写，
/// 保留其余条目的未知字段；数组解析不了就不动（读取时按空库处理）。
fn strip_invalid_library_icons(conn: &Connection) -> Result<usize, AppError> {
    let db_err = |e: rusqlite::Error| AppError::Database(e.to_string());
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [CUSTOM_ICONS_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)?;
    let Some(raw) = raw else {
        return Ok(0);
    };
    let Ok(mut entries) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) else {
        return Ok(0);
    };
    let before = entries.len();
    entries.retain(|entry| {
        entry
            .get("data")
            .and_then(|data| data.as_str())
            .is_some_and(is_valid_custom_icon)
    });
    let stripped = before - entries.len();
    if stripped > 0 {
        conn.execute(
            "UPDATE settings SET value = ?1 WHERE key = ?2",
            params![
                serde_json::Value::Array(entries).to_string(),
                CUSTOM_ICONS_KEY
            ],
        )
        .map_err(db_err)?;
    }
    Ok(stripped)
}

// --- 我的图标（图标库） ---

/// 图标库条目数上限：每张 96×96 的 PNG 一般几 KB，最坏不到 64 KiB，整个库都存在一个
/// settings 值里，打开选择器时一次读出
pub const MAX_LIBRARY_ICONS: usize = 100;

/// 名称长度上限（按字符数）
pub const MAX_LIBRARY_NAME_CHARS: usize = 64;

/// [`add_library_icon`] 的结果：同一张图已经在库里时返回原条目，`existed` 为 true
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddLibraryIconResult {
    pub icon: CustomIconEntry,
    pub existed: bool,
}

fn normalize_library_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::localized(
            "customIcon.nameEmpty",
            "请给图标起个名字",
            "Please name the icon",
        ));
    }
    if name.chars().count() > MAX_LIBRARY_NAME_CHARS {
        return Err(AppError::localized(
            "customIcon.nameTooLong",
            format!("图标名称不能超过 {MAX_LIBRARY_NAME_CHARS} 个字符"),
            format!("The icon name cannot exceed {MAX_LIBRARY_NAME_CHARS} characters"),
        ));
    }
    Ok(name.to_string())
}

fn library_icon_not_found() -> AppError {
    AppError::localized(
        "customIcon.notFound",
        "这个图标已经不在「我的图标」里了",
        "This icon is no longer in My Icons",
    )
}

pub fn list_library_icons(db: &Database) -> Result<Vec<CustomIconEntry>, AppError> {
    db.get_custom_icons()
}

/// 存进图标库，新的排在最前面。图片和供应商保存时一样完整校验；同一张图已经在库里时
/// 不重复存，返回原条目（名称不改）。
pub fn add_library_icon(
    db: &Database,
    name: &str,
    data: &str,
) -> Result<AddLibraryIconResult, AppError> {
    let name = normalize_library_name(name)?;
    if !is_valid_custom_icon(data) {
        return Err(AppError::localized(
            "provider.icon.invalid",
            "自定义图标无效，请重新上传",
            "The custom icon is invalid, please upload it again",
        ));
    }
    let mut icons = db.get_custom_icons()?;
    if let Some(existing) = icons.iter().find(|icon| icon.data == data) {
        return Ok(AddLibraryIconResult {
            icon: existing.clone(),
            existed: true,
        });
    }
    if icons.len() >= MAX_LIBRARY_ICONS {
        return Err(AppError::localized(
            "customIcon.libraryFull",
            format!("「我的图标」最多存 {MAX_LIBRARY_ICONS} 个，请先删掉一些"),
            format!("My Icons holds up to {MAX_LIBRARY_ICONS} icons, please delete some first"),
        ));
    }
    let icon = CustomIconEntry {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        data: data.to_string(),
        created_at: chrono::Utc::now().timestamp_millis(),
        extra: serde_json::Map::new(),
    };
    icons.insert(0, icon.clone());
    db.save_custom_icons(&icons)?;
    Ok(AddLibraryIconResult {
        icon,
        existed: false,
    })
}

pub fn rename_library_icon(
    db: &Database,
    id: &str,
    name: &str,
) -> Result<CustomIconEntry, AppError> {
    let name = normalize_library_name(name)?;
    let mut icons = db.get_custom_icons()?;
    let icon = icons
        .iter_mut()
        .find(|icon| icon.id == id)
        .ok_or_else(library_icon_not_found)?;
    icon.name = name;
    let renamed = icon.clone();
    db.save_custom_icons(&icons)?;
    Ok(renamed)
}

/// 只删图标库里的条目，已经选了这张图的供应商自己存着图片，不受影响。
/// 条目已经不在（比如另一台设备删了又同步过来）时什么都不做。
pub fn delete_library_icon(db: &Database, id: &str) -> Result<(), AppError> {
    let mut icons = db.get_custom_icons()?;
    let before = icons.len();
    icons.retain(|icon| icon.id != id);
    if icons.len() != before {
        db.save_custom_icons(&icons)?;
    }
    Ok(())
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

    #[test]
    fn strips_invalid_entries_from_icon_library_and_keeps_others() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE providers (id TEXT, app_type TEXT, icon TEXT, icon_color TEXT);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);",
        )
        .unwrap();
        let good = data_uri(&encode_png(96, 96));
        let header_only = data_uri(&encode_png(96, 96)[..33]);
        let library = serde_json::json!([
            { "id": "a", "name": "Good", "data": good, "futureField": 1 },
            { "id": "b", "name": "Bad", "data": header_only },
            { "id": "c", "name": "Builtin", "data": "claude" },
            { "id": "d", "name": "No data" },
        ]);
        conn.execute(
            "INSERT INTO settings VALUES ('custom_icons', ?1)",
            params![library.to_string()],
        )
        .unwrap();

        assert_eq!(strip_invalid_custom_icons(&conn).unwrap(), 3);

        let raw: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'custom_icons'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            value,
            serde_json::json!([{ "id": "a", "name": "Good", "data": good, "futureField": 1 }])
        );
    }

    #[test]
    fn library_add_dedupes_validates_and_puts_newest_first() {
        let db = Database::memory().unwrap();
        let first = data_uri(&encode_png(96, 96));
        let second = data_uri(&encode_png(64, 64));

        let added = add_library_icon(&db, "  Relay  ", &first).unwrap();
        assert!(!added.existed);
        assert_eq!(added.icon.name, "Relay");

        // 同一张图不重复存，名称也不改
        let again = add_library_icon(&db, "Other", &first).unwrap();
        assert!(again.existed);
        assert_eq!(again.icon.id, added.icon.id);
        assert_eq!(again.icon.name, "Relay");

        add_library_icon(&db, "Second", &second).unwrap();
        let names: Vec<_> = list_library_icons(&db)
            .unwrap()
            .into_iter()
            .map(|icon| icon.name)
            .collect();
        assert_eq!(names, ["Second", "Relay"]);

        assert!(add_library_icon(&db, " ", &second).is_err());
        assert!(add_library_icon(&db, &"x".repeat(MAX_LIBRARY_NAME_CHARS + 1), &second).is_err());
        assert!(add_library_icon(&db, "Name", "claude").is_err());
        let header_only = data_uri(&encode_png(96, 96)[..33]);
        assert!(add_library_icon(&db, "Name", &header_only).is_err());
        assert_eq!(list_library_icons(&db).unwrap().len(), 2);
    }

    #[test]
    fn library_rename_and_delete_keep_unknown_fields() {
        let db = Database::memory().unwrap();
        let good = data_uri(&encode_png(96, 96));
        db.set_setting(
            CUSTOM_ICONS_KEY,
            &serde_json::json!([
                { "id": "a", "name": "Old", "data": good, "createdAt": 1, "futureField": true },
                { "id": "b", "name": "Gone", "data": good },
            ])
            .to_string(),
        )
        .unwrap();

        let renamed = rename_library_icon(&db, "a", "New").unwrap();
        assert_eq!(renamed.name, "New");
        assert!(rename_library_icon(&db, "missing", "New").is_err());
        assert!(rename_library_icon(&db, "a", "").is_err());

        delete_library_icon(&db, "b").unwrap();
        delete_library_icon(&db, "missing").unwrap();

        let raw = db.get_setting(CUSTOM_ICONS_KEY).unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            value,
            serde_json::json!([
                { "id": "a", "name": "New", "data": good, "createdAt": 1, "futureField": true }
            ])
        );
    }

    #[test]
    fn library_is_capped() {
        let db = Database::memory().unwrap();
        let entries: Vec<_> = (0..MAX_LIBRARY_ICONS)
            .map(|i| CustomIconEntry {
                id: i.to_string(),
                name: i.to_string(),
                data: format!("{CUSTOM_ICON_PREFIX}{i}"),
                created_at: 0,
                extra: serde_json::Map::new(),
            })
            .collect();
        db.save_custom_icons(&entries).unwrap();
        assert!(add_library_icon(&db, "One more", &data_uri(&encode_png(96, 96))).is_err());
    }
}

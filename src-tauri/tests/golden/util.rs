use std::path::{Path, PathBuf};

use serde_json::Value;

use cc_switch_lib::{AppState, AppType, McpApps, McpServer, Provider, ProviderMeta};

use crate::support::ensure_test_home;

const UPDATE_ENV: &str = "CC_SWITCH_UPDATE_GOLDEN";

/// 按字节比对 `snapshots/<name>`；设了 `CC_SWITCH_UPDATE_GOLDEN` 时改为写入。
///
/// 快照扩展名只用 `.json` / `.toml` / `.txt`：`.gitattributes` 对它们固定 `eol=lf`，
/// Windows 上 checkout 出来不会变成 CRLF。
pub fn assert_golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/snapshots")
        .join(name);
    if std::env::var_os(UPDATE_ENV).is_some() {
        std::fs::create_dir_all(path.parent().expect("snapshot dir")).expect("create snapshot dir");
        std::fs::write(&path, actual).expect("write snapshot");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run with {UPDATE_ENV}=1 to create it",
            path.display()
        )
    });
    if expected == actual {
        return;
    }
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let first_diff = expected_lines
        .iter()
        .zip(actual_lines.iter())
        .position(|(e, a)| e != a)
        .unwrap_or(expected_lines.len().min(actual_lines.len()));
    panic!(
        "snapshot {name} changed at line {}:\n  expected: {:?}\n  actual:   {:?}\n\
         (expected {} lines / {} bytes, actual {} lines / {} bytes)\n\
         --- actual ---\n{actual}",
        first_diff + 1,
        expected_lines.get(first_diff),
        actual_lines.get(first_diff),
        expected_lines.len(),
        expected.len(),
        actual_lines.len(),
        actual.len(),
    );
}

pub fn home() -> PathBuf {
    ensure_test_home().to_path_buf()
}

pub fn write_home_file(rel: &str, content: &str) -> PathBuf {
    let path = home().join(rel);
    std::fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent dir");
    std::fs::write(&path, content).expect("write home file");
    path
}

pub fn read_home_file(rel: &str) -> String {
    std::fs::read_to_string(home().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

pub fn read_home_json(rel: &str) -> Value {
    serde_json::from_str(&read_home_file(rel)).unwrap_or_else(|e| panic!("parse {rel}: {e}"))
}

/// `common_config`：`None` 表示 meta 里不写 `commonConfigEnabled`（旧行的形态）。
pub fn provider(id: &str, settings: Value, common_config: Option<bool>) -> Provider {
    let mut provider = Provider::with_id(id.to_string(), id.to_string(), settings, None);
    if common_config.is_some() {
        provider.meta = Some(ProviderMeta {
            common_config_enabled: common_config,
            ..Default::default()
        });
    }
    provider
}

pub fn official(id: &str, settings: Value) -> Provider {
    let mut provider = provider(id, settings, None);
    provider.category = Some("official".to_string());
    provider
}

/// 存入供应商并把 `current` 设为当前（DB 的 is_current；设备级设置在测试里为空）。
pub fn seed_providers(state: &AppState, app: &AppType, providers: &[Provider], current: &str) {
    for provider in providers {
        state
            .db
            .save_provider(app.as_str(), provider)
            .expect("save provider");
    }
    state
        .db
        .set_current_provider(app.as_str(), current)
        .expect("set current provider");
}

pub fn mcp_server(id: &str, server: Value, apps: &[AppType]) -> McpServer {
    let mut enabled = McpApps::default();
    for app in apps {
        enabled.set_enabled_for(app, true);
    }
    McpServer {
        id: id.to_string(),
        name: id.to_string(),
        server,
        apps: enabled,
        description: None,
        homepage: None,
        docs: None,
        tags: Vec::new(),
    }
}

/// 读 DB 里某个应用的供应商原始行（列里存的 JSON 文本原样输出）。
///
/// 锁原始文本而不是反序列化后的结构：降级后旧版读的就是这些字节。
/// `created_at` 是时间戳，不进快照；`id_filter` 用来把含时间戳的 id 换成稳定写法。
///
/// `settings_sort`：`settings_config` 里键序不稳定的对象（JSON 指针），比对前按键排序，
/// 见 [`sort_objects`]。
pub fn dump_provider_rows(
    app: &AppType,
    id_filter: impl Fn(&str) -> String,
    settings_sort: &[&str],
) -> String {
    let conn = open_db_read_only();
    let mut out = String::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, name, settings_config, website_url, category, sort_index, notes, icon,
                    icon_color, meta, is_current, in_failover_queue
             FROM providers WHERE app_type = ?1 ORDER BY id",
        )
        .expect("prepare providers query");
    let rows = stmt
        .query_map([app.as_str()], |row| {
            Ok([
                ("id", row.get::<_, String>(0)?),
                ("name", row.get::<_, String>(1)?),
                ("settings_config", row.get::<_, String>(2)?),
                ("website_url", opt_text(row.get::<_, Option<String>>(3)?)),
                ("category", opt_text(row.get::<_, Option<String>>(4)?)),
                ("sort_index", opt_text(row.get::<_, Option<i64>>(5)?)),
                ("notes", opt_text(row.get::<_, Option<String>>(6)?)),
                ("icon", opt_text(row.get::<_, Option<String>>(7)?)),
                ("icon_color", opt_text(row.get::<_, Option<String>>(8)?)),
                ("meta", row.get::<_, String>(9)?),
                ("is_current", row.get::<_, bool>(10)?.to_string()),
                ("in_failover_queue", row.get::<_, bool>(11)?.to_string()),
            ])
        })
        .expect("query providers");
    for row in rows {
        let row = row.expect("read provider row");
        for (column, value) in row {
            let value = match column {
                "id" => id_filter(&value),
                "settings_config" if !settings_sort.is_empty() => {
                    let mut parsed: Value =
                        serde_json::from_str(&value).expect("settings_config is JSON");
                    sort_objects(&mut parsed, settings_sort);
                    serde_json::to_string(&parsed).expect("serialize settings_config")
                }
                _ => value,
            };
            out.push_str(&format!("{column}: {value}\n"));
        }
        out.push('\n');
    }

    let mut stmt = conn
        .prepare(
            "SELECT provider_id, url FROM provider_endpoints WHERE app_type = ?1
             ORDER BY provider_id, url",
        )
        .expect("prepare endpoints query");
    let endpoints = stmt
        .query_map([app.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query endpoints");
    for endpoint in endpoints {
        let (provider_id, url) = endpoint.expect("read endpoint row");
        out.push_str(&format!("endpoint: {} {url}\n", id_filter(&provider_id)));
    }
    out
}

fn opt_text<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| "NULL".to_string(), |v| v.to_string())
}

fn open_db_read_only() -> rusqlite::Connection {
    let path = home().join(".cc-switch").join("cc-switch.db");
    rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap_or_else(|e| panic!("open {}: {e}", path.display()))
}

/// 从 TOML 文本里截出以 `header_prefix` 开头的那几张连续的表（含表头行）。
///
/// 用来单独比对 `[mcp_servers.*]` 这一段：切换时文件其余部分按设计会变，
/// 这一段的字节不应该变。
pub fn toml_section(text: &str, header_prefix: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            inside = trimmed.starts_with(header_prefix);
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

/// 把 `pointers` 指到的对象按键排序。
///
/// 只用在旧代码输出顺序本身不稳定的地方：MCP 投影按 `HashMap` 遍历写
/// `mcpServers`（`claude_mcp.rs` / `gemini_mcp.rs` 的 `set_mcp_servers_map`），
/// Gemini 的 `.env` 解析进 `HashMap` 后再存进 `env`。这几处每次运行顺序都可能不同，
/// 其余内容仍按原样比对。
pub fn sort_objects(value: &mut Value, pointers: &[&str]) {
    for pointer in pointers {
        if let Some(Value::Object(map)) = value.pointer_mut(pointer) {
            let mut entries: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            map.extend(entries);
        }
    }
}

/// 读 JSON 文件，锁住它是 `to_string_pretty` 的原样输出（格式、键序），
/// 再把 `pointers` 处排序后返回，供快照比对。
pub fn stable_json_file(rel: &str, pointers: &[&str]) -> String {
    let raw = read_home_file(rel);
    let mut parsed: Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
    assert_eq!(
        raw.trim_end(),
        serde_json::to_string_pretty(&parsed).expect("serialize"),
        "{rel} is no longer written as pretty-printed JSON"
    );
    sort_objects(&mut parsed, pointers);
    serde_json::to_string_pretty(&parsed).expect("serialize") + "\n"
}

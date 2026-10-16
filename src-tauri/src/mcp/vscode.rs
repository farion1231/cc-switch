//! VS Code MCP 服务器导入。
//!
//! VS Code 不是 AppType：用户级 `mcp.json` 只作为导入源，
//! 导入的条目按现有应用开关启用，cc-switch 不写回 VS Code 的配置。
//! `${input:...}` 占位符原样保留在 headers 里——没有任何受管应用
//! 解析 VS Code 的 input 变量，启用后按字面值发送。

use serde_json::Value;

use crate::app_config::{McpApps, McpServer};
use crate::error::AppError;
use crate::store::AppState;

/// 用户级 mcp.json 路径（VS Code 把它放在 settings.json 旁的用户数据目录）。
/// Windows 缺 `APPDATA` 时返回 None = 源不可用，统一导入继续读其余来源。
fn user_config_path() -> Option<std::path::PathBuf> {
    let user_dir = user_data_dir()?;
    Some(user_dir.join("Code").join("User").join("mcp.json"))
}

/// VS Code 的用户数据目录（按平台，与 settings.json 同目录的父目录）。
#[cfg(windows)]
fn user_data_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(std::path::PathBuf::from)
}

#[cfg(target_os = "macos")]
fn user_data_dir() -> Option<std::path::PathBuf> {
    Some(
        crate::config::get_home_dir()
            .join("Library")
            .join("Application Support"),
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn user_data_dir() -> Option<std::path::PathBuf> {
    // XDG_CONFIG_HOME 存在且为绝对路径时优先（与 Claude Desktop 的
    // linux_config_dir_from_home 同一先例）；否则回落 ~/.config。
    let config_home = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from);
    let home = crate::config::get_home_dir();
    let dir = match config_home {
        Some(dir) if dir.is_absolute() => dir,
        _ => home.join(".config"),
    };
    Some(dir)
}

/// `type` 缺省时按字段推断（与社区 .mcp.json 惯例一致）：有 url 无 command
/// 视为 http，有 command 视为 stdio。
fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    if spec.is_object() {
        let has_url = spec.get("url").is_some();
        let has_command = spec.get("command").is_some();
        match spec.get("type").and_then(Value::as_str) {
            Some("streamable-http") => spec["type"] = serde_json::json!("http"),
            Some(_) => {}
            None if has_url => spec["type"] = serde_json::json!("http"),
            None if has_command => spec["type"] = serde_json::json!("stdio"),
            None => {}
        }
    }
    spec
}

/// 连接定义比对用的 transport 投影：只保留连接相关字段（含 cwd——
/// 工作目录不同的 stdio 条目是不同连接）。先走 `unified_spec`（隐式
/// type 补成显式）再投影——两侧同样归一化，否则「库里存的隐式 stdio」
/// 和「新导入补成显式 stdio」会被误报成冲突。
fn transport_spec(spec: &Value) -> Value {
    const TRANSPORT_FIELDS: [&str; 7] = ["type", "command", "args", "url", "headers", "env", "cwd"];
    let spec = unified_spec(spec);
    let mut fields = serde_json::Map::new();
    if let Some(object) = spec.as_object() {
        for field in TRANSPORT_FIELDS {
            if let Some(value) = object.get(field) {
                fields.insert(field.into(), value.clone());
            }
        }
    }
    Value::Object(fields)
}

/// 从 VS Code 的用户级 `mcp.json` 导入。同 transport 的已有条目只报告冲突并跳过；
/// `inputs` 声明（`${input:...}` 占位符的解析来源）不导入——占位符原样保留在
/// headers 里，没有任何受管应用解析它，启用后按字面值发送。
pub fn import(state: &AppState) -> Result<usize, AppError> {
    // 源不可用（Windows 缺 APPDATA）= 不是错误：其余来源照常导入。
    let Some(path) = user_config_path() else {
        return Ok(0);
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(AppError::io(&path, error)),
    };
    // VS Code 的 mcp.json 允许注释和尾逗号（官方 schema allowComments/
    // allowTrailingCommas），严格 JSON 解析会把整份文件拒掉——按仓库读
    // Pi 配置的先例用 JSON5 读语义值。
    let document: Value = json5::from_str(&content)
        .map_err(|error| AppError::Config(format!("invalid {}: {error}", path.display())))?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document["servers"].as_object().into_iter().flatten() {
        let spec = unified_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        if let Some(server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            state.db.save_mcp_server(&server)?;
        } else {
            count += 1;
            let server = McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps::default(),
                description: None,
                homepage: None,
                docs: None,
                tags: vec![],
            };
            state.db.save_mcp_server(&server)?;
        }
    }
    if skipped.is_empty() {
        Ok(count)
    } else {
        Err(AppError::InvalidInput(format!(
            "Imported {count} VS Code MCP servers; skipped {}. The VS Code configuration was preserved.",
            skipped.join("; ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::store::AppState;
    use serial_test::serial;
    use std::sync::Arc;

    #[test]
    fn type_defaults_follow_field_presence() {
        assert_eq!(
            unified_spec(&serde_json::json!({"url": "https://x"}))["type"],
            "http"
        );
        assert_eq!(
            unified_spec(&serde_json::json!({"command": "npx"}))["type"],
            "stdio"
        );
        assert_eq!(
            unified_spec(&serde_json::json!({"streamable-http": true}))["type"],
            serde_json::json!(null)
        );
    }

    /// 测试夹具：把各平台的用户数据根重定向到 `root`，并给出 mcp.json
    /// 在该根下的真实路径（`resolved`——测试只往这里布景，不自己拼路径）。
    /// Windows 重定向 `APPDATA`；macOS/Linux 重定向
    /// `CC_SWITCH_TEST_HOME`（`get_home_dir` 走它）并清掉
    /// `XDG_CONFIG_HOME`（Linux 分支会先走它；XDG 优先的生产语义由
    /// `user_config_path_resolves_per_platform` 单独钉住）。Drop 恢复原值。
    struct RedirectedUserDir {
        resolved: std::path::PathBuf,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl RedirectedUserDir {
        fn to(root: &std::path::Path) -> Self {
            let mut saved: Vec<(&'static str, Option<std::ffi::OsString>)> = Vec::new();
            if cfg!(windows) {
                saved.push(("APPDATA", std::env::var_os("APPDATA")));
                std::env::set_var("APPDATA", root);
            } else {
                saved.push((
                    "CC_SWITCH_TEST_HOME",
                    std::env::var_os("CC_SWITCH_TEST_HOME"),
                ));
                std::env::set_var("CC_SWITCH_TEST_HOME", root);
                saved.push(("XDG_CONFIG_HOME", std::env::var_os("XDG_CONFIG_HOME")));
                std::env::remove_var("XDG_CONFIG_HOME");
            }
            let resolved = user_config_path().expect("resolve under redirected root");
            assert!(
                resolved.starts_with(root),
                "resolved path must stay under the redirected root: {}",
                resolved.display()
            );
            Self { resolved, saved }
        }
    }

    impl Drop for RedirectedUserDir {
        fn drop(&mut self) {
            for (name, previous) in self.saved.drain(..) {
                match previous {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    /// 真实结构的用户级 mcp.json：一个 stdio（playwright 型）+ 一个 http 带
    /// `${input:}` 占位符（context7 型）。导入后两条都在库里、开关全关、
    /// 占位符原样保留；再导一次不新增（幂等）。
    #[test]
    #[serial]
    fn import_absorbs_real_shaped_user_mcp_json() {
        let root = tempfile::tempdir().expect("temp user-data root");
        let _redirect = RedirectedUserDir::to(root.path());
        let user_dir = _redirect.resolved.parent().expect("user dir").to_path_buf();
        std::fs::create_dir_all(&user_dir).expect("create user dir");
        std::fs::write(
            _redirect.resolved.clone(),
            serde_json::json!({
                "servers": {
                    "microsoft/playwright-mcp": {
                        "type": "stdio",
                        "command": "npx",
                        "args": ["@playwright/mcp@latest"],
                        "gallery": "https://api.mcp.github.com",
                        "version": "0.0.1-seed"
                    },
                    "io.github.upstash/context7": {
                        "type": "http",
                        "url": "https://mcp.context7.com/mcp",
                        "headers": {
                            "Authorization": "${input:Authorization}"
                        },
                        "gallery": "https://api.mcp.github.com",
                        "version": "4.1.1"
                    }
                },
                "inputs": [
                    {
                        "id": "Authorization",
                        "type": "promptString",
                        "description": "API key for authentication.",
                        "password": true
                    }
                ]
            })
            .to_string(),
        )
        .expect("write user mcp.json");

        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        // 第一次导入：新增 2 条；transport 冲突为空，不该报错。
        let imported = import(&state).expect("import once");
        assert_eq!(imported, 2, "both servers are new");

        let servers = state.db.get_all_mcp_servers().expect("read servers");
        assert_eq!(servers.len(), 2, "both real-shaped servers imported");

        let playwright = &servers["microsoft/playwright-mcp"];
        assert_eq!(
            playwright
                .server
                .pointer("/command")
                .and_then(Value::as_str),
            Some("npx")
        );
        assert_eq!(
            playwright.server.pointer("/args/0").and_then(Value::as_str),
            Some("@playwright/mcp@latest")
        );

        let context7 = &servers["io.github.upstash/context7"];
        assert_eq!(
            context7.server.pointer("/url").and_then(Value::as_str),
            Some("https://mcp.context7.com/mcp")
        );
        // ${input:...} 占位符原样保留（只有支持 promptString 的应用能解析）
        assert_eq!(
            context7
                .server
                .pointer("/headers/Authorization")
                .and_then(Value::as_str),
            Some("${input:Authorization}")
        );

        // 导入源没有开关位：8 个应用的开关全部是关
        for (id, server) in &servers {
            for app in AppType::all() {
                assert!(
                    !server.apps.is_enabled_for(&app),
                    "{id} must stay disabled for {app:?}"
                );
            }
        }

        // 幂等：再导一次，数量不增长
        import(&state).expect("import twice");
        assert_eq!(
            state.db.get_all_mcp_servers().expect("count again").len(),
            2
        );
    }

    /// VS Code 的 mcp.json 允许注释和尾逗号（官方 schema allowComments/
    /// allowTrailingCommas）：带注释和尾逗号的配置要能正常导入。
    #[test]
    #[serial]
    fn import_reads_jsonc_user_mcp_json() {
        let root = tempfile::tempdir().expect("temp user-data root");
        let _redirect = RedirectedUserDir::to(root.path());
        std::fs::create_dir_all(_redirect.resolved.parent().expect("user dir"))
            .expect("create user dir");
        std::fs::write(
            _redirect.resolved.clone(),
            // 行注释 + 尾逗号：严格 JSON 解析会整份拒绝
            r#"{
                "servers": {
                    // filesystem access for the agent
                    "filesystem": {
                        "type": "stdio",
                        "command": "npx",
                        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/"],
                    },
                },
            }"#,
        )
        .expect("write user mcp.json");

        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let imported = import(&state).expect("import JSONC config");
        assert_eq!(imported, 1, "JSONC server imported");
        let servers = state.db.get_all_mcp_servers().expect("read servers");
        assert_eq!(
            servers["filesystem"]
                .server
                .pointer("/args/0")
                .and_then(Value::as_str),
            Some("-y")
        );
    }

    /// Windows 缺 APPDATA = 源不可用：导入返回 Ok(0) 而不是把
    /// 统一导入整体报成失败，其余来源照常工作。
    #[test]
    #[serial]
    fn import_without_appdata_reports_no_source() {
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let previous = std::env::var_os("APPDATA");
        std::env::remove_var("APPDATA");
        let imported = import(&state);
        match previous {
            Some(value) => std::env::set_var("APPDATA", value),
            None => std::env::remove_var("APPDATA"),
        }
        assert_eq!(imported.expect("no-source import"), 0);
    }

    /// 路径解析按平台走真实位置：Windows 用 APPDATA；macOS 用
    /// ~/Library/Application Support；Linux 优先走绝对路径的
    /// XDG_CONFIG_HOME、清掉后回落 ~/.config（经 CC_SWITCH_TEST_HOME
    /// 隔离真实 home——两条分支都钉住，不留暗病）。
    #[test]
    #[serial]
    fn user_config_path_resolves_per_platform() {
        let isolated_home = tempfile::tempdir().expect("isolated home");
        let saved_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let saved_xdg = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", isolated_home.path());
        std::env::remove_var("XDG_CONFIG_HOME");

        // 分支一：清掉 XDG 后各平台的用户数据目录。
        let resolved = user_config_path().expect("platform path resolves");
        let user_data_dir = resolved
            .parent()
            .and_then(|dir| dir.parent())
            .and_then(|dir| dir.parent())
            .expect("user data dir");
        let expected = if cfg!(windows) {
            std::env::var_os("APPDATA")
                .map(std::path::PathBuf::from)
                .expect("APPDATA set on Windows runners")
        } else if cfg!(target_os = "macos") {
            isolated_home
                .path()
                .join("Library")
                .join("Application Support")
        } else {
            isolated_home.path().join(".config")
        };
        assert_eq!(user_data_dir, expected);
        assert!(
            resolved.ends_with(std::path::Path::new("Code/User/mcp.json")),
            "resolved path keeps the VS Code layout: {}",
            resolved.display()
        );

        // 分支二（仅 Linux）：绝对路径的 XDG_CONFIG_HOME 优先于 ~/.config。
        let mut xdg_resolved: Option<std::path::PathBuf> = None;
        let mut xdg_root: Option<tempfile::TempDir> = None;
        if cfg!(all(unix, not(target_os = "macos"))) {
            let root = tempfile::tempdir().expect("temp XDG root");
            std::env::set_var("XDG_CONFIG_HOME", root.path());
            xdg_resolved = Some(user_config_path().expect("resolve under XDG root"));
            xdg_root = Some(root);
        }

        // 统一恢复：两个分支的环境变量一次清干净。
        std::env::remove_var("XDG_CONFIG_HOME");
        match saved_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match saved_xdg {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        if let (Some(resolved), Some(root)) = (xdg_resolved, xdg_root.as_ref()) {
            assert!(
                resolved.starts_with(root.path()),
                "absolute XDG_CONFIG_HOME must win: {}",
                resolved.display()
            );
        }
    }

    /// 库里存的隐式 stdio（缺 type，社区 .mcp.json 惯例）与 VS Code 里
    /// 同一定义（unified_spec 补成显式 stdio）是同一条连接：不该误报冲突。
    #[test]
    #[serial]
    fn import_treats_implicit_stdio_as_same_transport() {
        let root = tempfile::tempdir().expect("temp user-data root");
        let _redirect = RedirectedUserDir::to(root.path());
        std::fs::create_dir_all(_redirect.resolved.parent().expect("user dir"))
            .expect("create user dir");
        std::fs::write(
            _redirect.resolved.clone(),
            serde_json::json!({
                "servers": {
                    "shared": {
                        "type": "stdio",
                        "command": "node",
                        "args": ["server.js"]
                    }
                }
            })
            .to_string(),
        )
        .expect("write user mcp.json");

        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        // 库里已有同一条连接，type 是隐式的（导入路径原样保留）
        state
            .db
            .save_mcp_server(&mcp_server(
                "shared",
                serde_json::json!({"command": "node", "args": ["server.js"]}),
            ))
            .expect("seed existing server");

        // 同一条连接（隐式 stdio ≡ 显式 stdio）：Ok(0)、不报冲突、不新增条目。
        // 冲突路径返回 Err，这里 expect 就是最强的断言。
        let imported = import(&state).expect("same transport must not conflict");
        assert_eq!(imported, 0, "existing server, not a new entry");
        assert_eq!(
            state.db.get_all_mcp_servers().expect("read servers").len(),
            1,
            "no duplicate entry"
        );
    }

    /// 造一条只用于测试的 McpServer（开关全关，连接定义原样存）。
    fn mcp_server(id: &str, spec: Value) -> crate::app_config::McpServer {
        crate::app_config::McpServer {
            id: id.to_string(),
            name: id.to_string(),
            server: spec,
            apps: crate::app_config::McpApps::default(),
            description: None,
            homepage: None,
            docs: None,
            tags: vec![],
        }
    }
}

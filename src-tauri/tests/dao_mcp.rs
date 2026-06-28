use cc_switch_lib::{Database, McpApps, McpServer};
use serde_json::json;

fn make_mcp_server(id: &str, name: &str) -> McpServer {
    McpServer {
        id: id.to_string(),
        name: name.to_string(),
        server: json!({ "command": "npx", "args": [format!("-y @{id}/mcp")] }),
        apps: McpApps {
            claude: true,
            codex: false,
            gemini: false,
            opencode: false,
            hermes: false,
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    }
}

// === Empty State ===

#[test]
fn get_all_mcp_servers_empty_initially() {
    let db = Database::memory().expect("create memory db");
    let servers = db.get_all_mcp_servers().expect("get all servers");
    assert!(servers.is_empty());
}

#[test]
fn is_mcp_table_empty_true_initially() {
    let db = Database::memory().expect("create memory db");
    assert!(db.is_mcp_table_empty().expect("check empty"));
}

// === Save and Retrieve ===

#[test]
fn save_and_retrieve_mcp_server() {
    let db = Database::memory().expect("create memory db");
    let server = make_mcp_server("fetch", "MCP Fetch");
    db.save_mcp_server(&server).expect("save server");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    assert_eq!(servers.len(), 1);

    let retrieved = servers.get("fetch").expect("server exists");
    assert_eq!(retrieved.id, "fetch");
    assert_eq!(retrieved.name, "MCP Fetch");
}

#[test]
fn is_mcp_table_empty_false_after_insert() {
    let db = Database::memory().expect("create memory db");
    let server = make_mcp_server("fetch", "MCP Fetch");
    db.save_mcp_server(&server).expect("save server");
    assert!(!db.is_mcp_table_empty().expect("check empty"));
}

#[test]
fn server_config_json_preserved() {
    let db = Database::memory().expect("create memory db");
    let server_config = json!({
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem"],
        "env": { "ALLOWED_PATHS": "/tmp" }
    });
    let server = McpServer {
        id: "filesystem".to_string(),
        name: "Filesystem".to_string(),
        server: server_config.clone(),
        apps: McpApps::default(),
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    db.save_mcp_server(&server).expect("save server");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    let retrieved = servers.get("filesystem").expect("server exists");
    assert_eq!(retrieved.server, server_config);
}

// === Apps Flags ===

#[test]
fn mcp_apps_flags_all_true_preserved() {
    let db = Database::memory().expect("create memory db");
    let server = McpServer {
        id: "all-apps".to_string(),
        name: "All Apps".to_string(),
        server: json!({}),
        apps: McpApps {
            claude: true,
            codex: true,
            gemini: true,
            opencode: true,
            hermes: false,
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    db.save_mcp_server(&server).expect("save server");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    let retrieved = servers.get("all-apps").expect("server exists");
    assert!(retrieved.apps.claude);
    assert!(retrieved.apps.codex);
    assert!(retrieved.apps.gemini);
    assert!(retrieved.apps.opencode);
}

#[test]
fn mcp_apps_flags_all_false_preserved() {
    let db = Database::memory().expect("create memory db");
    let server = McpServer {
        id: "no-apps".to_string(),
        name: "No Apps".to_string(),
        server: json!({}),
        apps: McpApps {
            claude: false,
            codex: false,
            gemini: false,
            opencode: false,
            hermes: false,
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };
    db.save_mcp_server(&server).expect("save server");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    let retrieved = servers.get("no-apps").expect("server exists");
    assert!(!retrieved.apps.claude);
    assert!(!retrieved.apps.codex);
    assert!(!retrieved.apps.gemini);
    assert!(!retrieved.apps.opencode);
}

// === Optional Fields ===

#[test]
fn optional_fields_preserved() {
    let db = Database::memory().expect("create memory db");
    let server = McpServer {
        id: "rich".to_string(),
        name: "Rich Server".to_string(),
        server: json!({}),
        apps: McpApps::default(),
        description: Some("A rich MCP server".to_string()),
        homepage: Some("https://example.com".to_string()),
        docs: Some("https://docs.example.com".to_string()),
        tags: vec![],
    };
    db.save_mcp_server(&server).expect("save server");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    let retrieved = servers.get("rich").expect("server exists");
    assert_eq!(retrieved.description.as_deref(), Some("A rich MCP server"));
    assert_eq!(retrieved.homepage.as_deref(), Some("https://example.com"));
    assert_eq!(retrieved.docs.as_deref(), Some("https://docs.example.com"));
}

#[test]
fn tags_array_preserved() {
    let db = Database::memory().expect("create memory db");
    let server = McpServer {
        id: "tagged".to_string(),
        name: "Tagged Server".to_string(),
        server: json!({}),
        apps: McpApps::default(),
        description: None,
        homepage: None,
        docs: None,
        tags: vec![
            "filesystem".to_string(),
            "tools".to_string(),
            "io".to_string(),
        ],
    };
    db.save_mcp_server(&server).expect("save server");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    let retrieved = servers.get("tagged").expect("server exists");
    assert_eq!(retrieved.tags, vec!["filesystem", "tools", "io"]);
}

// === Update ===

#[test]
fn save_mcp_server_replaces_existing() {
    let db = Database::memory().expect("create memory db");
    let server = make_mcp_server("fetch", "MCP Fetch");
    db.save_mcp_server(&server).expect("save server");

    let updated = McpServer {
        id: "fetch".to_string(),
        name: "MCP Fetch Updated".to_string(),
        server: json!({ "command": "node", "args": ["fetch-server.js"] }),
        apps: McpApps {
            claude: false,
            codex: true,
            gemini: true,
            opencode: false,
            hermes: false,
        },
        description: Some("Updated description".to_string()),
        homepage: None,
        docs: None,
        tags: vec!["updated".to_string()],
    };
    db.save_mcp_server(&updated).expect("update server");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    assert_eq!(servers.len(), 1);
    let retrieved = servers.get("fetch").expect("server exists");
    assert_eq!(retrieved.name, "MCP Fetch Updated");
    assert!(!retrieved.apps.claude);
    assert!(retrieved.apps.codex);
    assert_eq!(
        retrieved.description.as_deref(),
        Some("Updated description")
    );
}

// === Delete ===

#[test]
fn delete_mcp_server_removes_it() {
    let db = Database::memory().expect("create memory db");
    let server = make_mcp_server("fetch", "MCP Fetch");
    db.save_mcp_server(&server).expect("save server");

    db.delete_mcp_server("fetch").expect("delete server");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    assert!(servers.is_empty());
}

#[test]
fn delete_nonexistent_server_does_not_error() {
    let db = Database::memory().expect("create memory db");
    db.delete_mcp_server("nonexistent")
        .expect("delete nonexistent server");
}

// === Multiple Servers ===

#[test]
fn multiple_servers_retrieved_ordered_by_name() {
    let db = Database::memory().expect("create memory db");
    db.save_mcp_server(&make_mcp_server("z-server", "Zebra Server"))
        .expect("save z");
    db.save_mcp_server(&make_mcp_server("a-server", "Alpha Server"))
        .expect("save a");
    db.save_mcp_server(&make_mcp_server("m-server", "Middle Server"))
        .expect("save m");

    let servers = db.get_all_mcp_servers().expect("get all servers");
    assert_eq!(servers.len(), 3);

    let names: Vec<&str> = servers.values().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Alpha Server", "Middle Server", "Zebra Server"]);
}

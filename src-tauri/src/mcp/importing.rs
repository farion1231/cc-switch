use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{AppType, McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::normalize_server_spec;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum McpImportIssueKind {
    Conflict,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpImportIssue {
    pub id: String,
    pub source_app: AppType,
    pub kind: McpImportIssueKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub existing_apps: Vec<AppType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpImportResult {
    pub added: usize,
    pub refreshed: usize,
    pub enabled_only: usize,
    pub conflicts: usize,
    pub invalid: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<McpImportIssue>,
}

impl McpImportResult {
    pub fn changed_count(&self) -> usize {
        self.added + self.refreshed + self.enabled_only
    }

    pub fn merge(&mut self, other: Self) {
        self.added += other.added;
        self.refreshed += other.refreshed;
        self.enabled_only += other.enabled_only;
        self.conflicts += other.conflicts;
        self.invalid += other.invalid;
        self.issues.extend(other.issues);
    }

    pub fn push_issue(&mut self, issue: McpImportIssue) {
        match issue.kind {
            McpImportIssueKind::Conflict => self.conflicts += 1,
            McpImportIssueKind::Invalid => self.invalid += 1,
        }
        self.issues.push(issue);
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedImport {
    pub servers: Vec<McpServer>,
    pub issues: Vec<McpImportIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImportMergeAction {
    Added,
    Refreshed,
    EnabledOnly,
    Unchanged,
    Conflict { existing_apps: Vec<AppType> },
}

pub(crate) fn invalid_issue(
    id: impl Into<String>,
    source_app: AppType,
    message: impl Into<String>,
) -> McpImportIssue {
    McpImportIssue {
        id: id.into(),
        source_app,
        kind: McpImportIssueKind::Invalid,
        message: message.into(),
        existing_apps: Vec::new(),
    }
}

pub(crate) fn conflict_issue(
    id: impl Into<String>,
    source_app: AppType,
    existing_apps: Vec<AppType>,
) -> McpImportIssue {
    let app_list = existing_apps
        .iter()
        .map(AppType::as_str)
        .collect::<Vec<_>>()
        .join(", ");

    McpImportIssue {
        id: id.into(),
        source_app,
        kind: McpImportIssueKind::Conflict,
        message: format!(
            "同一 ID 已被其他应用使用且配置不同，当前不会自动覆盖（现有应用: {app_list})"
        ),
        existing_apps,
    }
}

pub(crate) fn source_import_error_issue(
    source_app: AppType,
    message: impl Into<String>,
) -> McpImportIssue {
    McpImportIssue {
        id: format!("{}:import", source_app.as_str()),
        source_app,
        kind: McpImportIssueKind::Invalid,
        message: message.into(),
        existing_apps: Vec::new(),
    }
}

pub(crate) fn build_imported_server(
    id: impl Into<String>,
    source_app: AppType,
    server: Value,
) -> McpServer {
    let id = id.into();
    let mut apps = McpApps::default();
    apps.set_enabled_for(&source_app, true);

    McpServer {
        id: id.clone(),
        name: id,
        server,
        apps,
        description: None,
        homepage: None,
        docs: None,
        tags: Vec::new(),
    }
}

fn is_owned_by_source(server: &McpServer, source_app: &AppType) -> bool {
    let enabled_apps = server.apps.enabled_apps();
    !enabled_apps.is_empty() && enabled_apps.iter().all(|app| app == source_app)
}

pub(crate) fn reconcile_imported_server(
    servers: &mut HashMap<String, McpServer>,
    mut imported: McpServer,
    source_app: AppType,
) -> Result<ImportMergeAction, AppError> {
    imported.server = normalize_server_spec(&imported.server)?;

    match servers.get_mut(&imported.id) {
        None => {
            servers.insert(imported.id.clone(), imported);
            Ok(ImportMergeAction::Added)
        }
        Some(existing) => {
            let canonical_existing = normalize_server_spec(&existing.server)?;
            let same_spec = canonical_existing == imported.server;

            if same_spec {
                let mut changed = false;

                if canonical_existing != existing.server {
                    existing.server = canonical_existing;
                    changed = true;
                }

                if !existing.apps.is_enabled_for(&source_app) {
                    existing.apps.set_enabled_for(&source_app, true);
                    return Ok(if changed {
                        ImportMergeAction::Refreshed
                    } else {
                        ImportMergeAction::EnabledOnly
                    });
                }

                return Ok(if changed {
                    ImportMergeAction::Refreshed
                } else {
                    ImportMergeAction::Unchanged
                });
            }

            if is_owned_by_source(existing, &source_app) {
                existing.server = imported.server;
                existing.apps.set_enabled_for(&source_app, true);
                return Ok(ImportMergeAction::Refreshed);
            }

            Ok(ImportMergeAction::Conflict {
                existing_apps: existing.apps.enabled_apps(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::McpApps;
    use serde_json::json;

    fn server(id: &str, app: AppType, spec: Value) -> McpServer {
        build_imported_server(id.to_string(), app, spec)
    }

    #[test]
    fn reconcile_does_not_overwrite_disabled_entry() {
        let mut servers = HashMap::new();
        servers.insert(
            "shared".to_string(),
            McpServer {
                id: "shared".to_string(),
                name: "shared".to_string(),
                server: json!({
                    "type": "stdio",
                    "command": "local"
                }),
                apps: McpApps::default(),
                description: None,
                homepage: None,
                docs: None,
                tags: Vec::new(),
            },
        );

        let action = reconcile_imported_server(
            &mut servers,
            server(
                "shared",
                AppType::Codex,
                json!({
                    "type": "stdio",
                    "command": "external"
                }),
            ),
            AppType::Codex,
        )
        .expect("reconcile should succeed");

        assert_eq!(
            action,
            ImportMergeAction::Conflict {
                existing_apps: Vec::new()
            }
        );
        assert_eq!(
            servers["shared"].server.get("command").and_then(|v| v.as_str()),
            Some("local")
        );
        assert!(servers["shared"].apps.is_empty());
    }

    #[test]
    fn reconcile_surfaces_invalid_existing_spec() {
        let mut servers = HashMap::new();
        servers.insert(
            "shared".to_string(),
            McpServer {
                id: "shared".to_string(),
                name: "shared".to_string(),
                server: json!({
                    "type": "stdio"
                }),
                apps: McpApps {
                    claude: true,
                    codex: false,
                    gemini: false,
                    opencode: false,
                },
                description: None,
                homepage: None,
                docs: None,
                tags: Vec::new(),
            },
        );

        let err = reconcile_imported_server(
            &mut servers,
            server(
                "shared",
                AppType::Codex,
                json!({
                    "type": "stdio",
                    "command": "echo"
                }),
            ),
            AppType::Codex,
        )
        .expect_err("invalid existing spec should bubble up");

        assert!(
            err.to_string().contains("stdio 类型的 MCP 服务器缺少 command 字段"),
            "unexpected error: {err}"
        );
    }
}

pub(crate) fn apply_parsed_import(
    config: &mut MultiAppConfig,
    parsed: ParsedImport,
    source_app: AppType,
) -> Result<usize, AppError> {
    let servers = config
        .mcp
        .servers
        .get_or_insert_with(HashMap::<String, McpServer>::new);
    let mut changed = 0usize;

    for issue in &parsed.issues {
        log::warn!(
            "跳过 {} 导入的 MCP '{}': {}",
            issue.source_app.as_str(),
            issue.id,
            issue.message
        );
    }

    for server in parsed.servers {
        match reconcile_imported_server(servers, server, source_app.clone())? {
            ImportMergeAction::Added
            | ImportMergeAction::Refreshed
            | ImportMergeAction::EnabledOnly => changed += 1,
            ImportMergeAction::Unchanged | ImportMergeAction::Conflict { .. } => {}
        }
    }

    Ok(changed)
}

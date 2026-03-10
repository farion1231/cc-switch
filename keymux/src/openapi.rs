//! OpenAPI3 exoskeleton spec for discoverable agent gateway
//!
//! Every security agent/SSH-tree registers as a pluggable path.
//! Hot-swap without restart.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// OpenAPI3 Info object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiInfo {
    pub title: String,
    pub version: String,
    pub description: Option<String>,
}

/// OpenAPI3 Server object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiServer {
    pub url: String,
    pub description: Option<String>,
}

/// OpenAPI3 Path Item
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PathItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub put: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete: Option<Operation>,
}

/// OpenAPI3 Operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<Parameter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<RequestBody>,
    pub responses: HashMap<String, Response>,
}

/// OpenAPI3 Parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,
}

/// OpenAPI3 Request Body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
    pub content: HashMap<String, MediaType>,
}

/// OpenAPI3 Media Type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaType {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,
}

/// OpenAPI3 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<HashMap<String, MediaType>>,
}

/// OpenAPI3 Schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<serde_json::Value>,
}

/// OpenAPI3 Components
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Components {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schemas: Option<HashMap<String, Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_schemes: Option<HashMap<String, SecurityScheme>>,
}

/// OpenAPI3 Security Scheme
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScheme {
    #[serde(rename = "type")]
    pub scheme_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "in")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// OpenAPI3 Document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiDocument {
    pub openapi: String,
    pub info: OpenApiInfo,
    pub servers: Vec<OpenApiServer>,
    pub paths: HashMap<String, PathItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Components>,
}

/// Pluggable path registration
pub struct PathRegistry {
    paths: HashMap<String, PathItem>,
}

impl PathRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            paths: HashMap::new(),
        };

        // Register core paths
        registry.register_core_paths();

        registry
    }

    /// Register core API paths
    fn register_core_paths(&mut self) {
        // /v1/chat/completions
        self.paths.insert(
            "/v1/chat/completions".to_string(),
            PathItem {
                summary: Some("Chat completions".to_string()),
                description: Some(
                    "OpenAI-compatible chat completions endpoint with /provider/model routing"
                        .to_string(),
                ),
                post: Some(Operation {
                    summary: Some("Create chat completion".to_string()),
                    operation_id: Some("createChatCompletion".to_string()),
                    tags: Some(vec!["chat".to_string()]),
                    request_body: Some(RequestBody {
                        required: true,
                        content: HashMap::from([(
                            "application/json".to_string(),
                            MediaType {
                                schema: Some(Schema {
                                    schema_type: Some("object".to_string()),
                                    description: Some("Chat completion request".to_string()),
                                    ..Default::default()
                                }),
                            },
                        )]),
                    }),
                    responses: HashMap::from([(
                        "200".to_string(),
                        Response {
                            description: Some("Successful response".to_string()),
                            content: Some(HashMap::from([(
                                "application/json".to_string(),
                                MediaType {
                                    schema: Some(Schema {
                                        schema_type: Some("object".to_string()),
                                        ..Default::default()
                                    }),
                                },
                            )])),
                        },
                    )]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        // /v1/models
        self.paths.insert(
            "/v1/models".to_string(),
            PathItem {
                summary: Some("List models".to_string()),
                get: Some(Operation {
                    summary: Some("List available models".to_string()),
                    operation_id: Some("listModels".to_string()),
                    tags: Some(vec!["models".to_string()]),
                    responses: HashMap::from([(
                        "200".to_string(),
                        Response {
                            description: Some("List of models".to_string()),
                            ..Default::default()
                        },
                    )]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        // /v1/embeddings
        self.paths.insert(
            "/v1/embeddings".to_string(),
            PathItem {
                summary: Some("Create embeddings".to_string()),
                post: Some(Operation {
                    summary: Some("Create embeddings for text".to_string()),
                    operation_id: Some("createEmbeddings".to_string()),
                    tags: Some(vec!["embeddings".to_string()]),
                    responses: HashMap::from([(
                        "200".to_string(),
                        Response {
                            description: Some("Embedding vectors".to_string()),
                            ..Default::default()
                        },
                    )]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        // /feed (h3 realtime)
        self.paths.insert(
            "/feed".to_string(),
            PathItem {
                summary: Some("Realtime feed (h3)".to_string()),
                description: Some(
                    "Realtime agent events, radio state, tool calls via HTTP/3".to_string(),
                ),
                get: Some(Operation {
                    summary: Some("Subscribe to realtime feed".to_string()),
                    operation_id: Some("subscribeFeed".to_string()),
                    tags: Some(vec!["feed".to_string(), "realtime".to_string()]),
                    responses: HashMap::from([(
                        "200".to_string(),
                        Response {
                            description: Some("Server-sent events stream".to_string()),
                            content: Some(HashMap::from([(
                                "text/event-stream".to_string(),
                                MediaType {
                                    schema: Some(Schema {
                                        schema_type: Some("string".to_string()),
                                        description: Some(
                                            "SSE stream of FeedEvent objects".to_string(),
                                        ),
                                        ..Default::default()
                                    }),
                                },
                            )])),
                        },
                    )]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        // /health
        self.paths.insert(
            "/health".to_string(),
            PathItem {
                summary: Some("Health check".to_string()),
                get: Some(Operation {
                    summary: Some("Check service health".to_string()),
                    operation_id: Some("healthCheck".to_string()),
                    tags: Some(vec!["system".to_string()]),
                    responses: HashMap::from([(
                        "200".to_string(),
                        Response {
                            description: Some("Service is healthy".to_string()),
                            ..Default::default()
                        },
                    )]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        // /status
        self.paths.insert(
            "/status".to_string(),
            PathItem {
                summary: Some("Service status".to_string()),
                get: Some(Operation {
                    summary: Some("Get detailed service status".to_string()),
                    operation_id: Some("getStatus".to_string()),
                    tags: Some(vec!["system".to_string()]),
                    responses: HashMap::from([(
                        "200".to_string(),
                        Response {
                            description: Some(
                                "Detailed status including carrier metrics".to_string(),
                            ),
                            ..Default::default()
                        },
                    )]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    }

    /// Register a pluggable path (e.g., SSH-tree, skill, MCP server)
    pub fn register_path(&mut self, path: &str, item: PathItem) {
        self.paths.insert(path.to_string(), item);
    }

    /// Register a skill as a pluggable path
    pub fn register_skill(&mut self, skill_name: &str, description: &str) {
        let path = format!("/skills/{}", skill_name);
        self.paths.insert(
            path.clone(),
            PathItem {
                summary: Some(format!("Skill: {}", skill_name)),
                description: Some(description.to_string()),
                post: Some(Operation {
                    summary: Some(format!("Execute {}", skill_name)),
                    operation_id: Some(format!("executeSkill_{}", skill_name.replace('-', "_"))),
                    tags: Some(vec!["skills".to_string()]),
                    responses: HashMap::from([(
                        "200".to_string(),
                        Response {
                            description: Some("Skill execution result".to_string()),
                            ..Default::default()
                        },
                    )]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    }

    /// Register an MCP server as a pluggable path
    pub fn register_mcp(&mut self, server_name: &str, tools: &[&str]) {
        let base_path = format!("/mcp/{}", server_name);

        // Register base path
        self.paths.insert(
            base_path.clone(),
            PathItem {
                summary: Some(format!("MCP Server: {}", server_name)),
                description: Some(format!("MCP server with {} tools", tools.len())),
                ..Default::default()
            },
        );

        // Register each tool
        for tool in tools {
            let tool_path = format!("{}/tools/{}", base_path, tool);
            self.paths.insert(
                tool_path,
                PathItem {
                    summary: Some(format!("MCP Tool: {}", tool)),
                    post: Some(Operation {
                        summary: Some(format!("Execute {}", tool)),
                        operation_id: Some(format!(
                            "mcp_{}_{}",
                            server_name.replace('-', "_"),
                            tool.replace('-', "_")
                        )),
                        tags: Some(vec!["mcp".to_string(), server_name.to_string()]),
                        responses: HashMap::from([(
                            "200".to_string(),
                            Response {
                                description: Some("Tool execution result".to_string()),
                                ..Default::default()
                            },
                        )]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            );
        }
    }

    /// Register an SSH-tree as a pluggable path
    pub fn register_ssh_tree(&mut self, tree_name: &str, host: &str) {
        let path = format!("/ssh/{}", tree_name);
        self.paths.insert(
            path,
            PathItem {
                summary: Some(format!("SSH Tree: {}", tree_name)),
                description: Some(format!("SSH connection tree to {}", host)),
                post: Some(Operation {
                    summary: Some("Execute command via SSH tree"),
                    operation_id: Some(format!("sshTree_{}", tree_name.replace('-', "_"))),
                    tags: Some(vec!["ssh".to_string()]),
                    responses: HashMap::from([(
                        "200".to_string(),
                        Response {
                            description: Some("Command output".to_string()),
                            ..Default::default()
                        },
                    )]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    }

    /// Build the complete OpenAPI document
    pub fn build_document(&self, server_url: &str) -> OpenApiDocument {
        OpenApiDocument {
            openapi: "3.1.0".to_string(),
            info: OpenApiInfo {
                title: "KeyMux - Embodied Agent Gateway".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some("OpenAI-compatible LLM proxy with QUIC/h3, intelligent routing, and pluggable agent paths".to_string()),
            },
            servers: vec![
                OpenApiServer {
                    url: server_url.to_string(),
                    description: Some("KeyMux server".to_string()),
                },
            ],
            paths: self.paths.clone(),
            components: Some(Components {
                security_schemes: Some(HashMap::from([
                    ("BearerAuth".to_string(), SecurityScheme {
                        scheme_type: "http".to_string(),
                        name: Some("Authorization".to_string()),
                        location: Some("header".to_string()),
                        description: Some("Bearer token authentication".to_string()),
                    }),
                ])),
                ..Default::default()
            }),
        }
    }
}

impl Default for PathRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Feed event for h3 realtime streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedEvent {
    #[serde(rename = "type")]
    pub event_type: FeedEventType,
    pub timestamp: i64,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedEventType {
    /// Agent observation
    Observation,
    /// Tool call started/completed
    ToolCall,
    /// Radio state change
    RadioState,
    /// Carrier handoff
    CarrierHandoff,
    /// Key rotation
    KeyRotation,
    /// Provider failover
    Failover,
    /// Security event
    Security,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_openapi_document() {
        let registry = PathRegistry::new();
        let doc = registry.build_document("http://localhost:8888");

        assert_eq!(doc.openapi, "3.1.0");
        assert!(doc.paths.contains_key("/v1/chat/completions"));
        assert!(doc.paths.contains_key("/feed"));
    }

    #[test]
    fn test_register_skill() {
        let mut registry = PathRegistry::new();
        registry.register_skill("code-review", "Reviews code changes");

        assert!(registry.paths.contains_key("/skills/code-review"));
    }

    #[test]
    fn test_register_mcp() {
        let mut registry = PathRegistry::new();
        registry.register_mcp("filesystem", &["read_file", "write_file"]);

        assert!(registry.paths.contains_key("/mcp/filesystem"));
        assert!(registry
            .paths
            .contains_key("/mcp/filesystem/tools/read_file"));
    }

    #[test]
    fn test_serialize_document() {
        let registry = PathRegistry::new();
        let doc = registry.build_document("http://localhost:8888");

        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("\"openapi\":\"3.1.0\""));
    }
}

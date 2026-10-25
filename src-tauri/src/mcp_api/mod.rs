//! Opt-in HTTP API tools, independent of Tauri and the application database.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};

mod openapi;
mod runtime;
mod schema;
mod stdio;
pub use openapi::compile_document;
pub use runtime::{fetch_document, ApiRuntime};
pub use stdio::run_stdio;

pub const CONFIG_ENV: &str = "CC_SWITCH_MCP_API_CONFIG";
pub const HEADERS_ENV: &str = "CC_SWITCH_MCP_API_HEADERS";
pub const MAX_CONFIG: usize = 512 * 1024;
pub const MAX_ENCODED: usize = 24 * 1024;
pub const MAX_RESPONSE: usize = 2 * 1024 * 1024;
static PATH_PARAMETERS: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| regex::Regex::new(r"\{([^{}]+)\}").unwrap());

pub(crate) fn runtime_executable(
    current: std::path::PathBuf,
    appimage: Option<std::path::PathBuf>,
    flatpak: bool,
) -> Result<std::path::PathBuf, String> {
    if flatpak {
        return Err(
            "API MCP creation is not yet supported in Flatpak; use a native CC Switch installation"
                .into(),
        );
    }
    let path = appimage.unwrap_or(current);
    if !path.is_absolute() {
        return Err("CC Switch executable path must be absolute".into());
    }
    Ok(path)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTool {
    pub name: String,
    pub description: String,
    pub method: String,
    pub url: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
}

impl ApiTool {
    pub fn is_read_only(&self) -> bool {
        self.read_only
            .unwrap_or(matches!(self.method.as_str(), "GET" | "HEAD" | "OPTIONS"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProfile {
    pub version: u32,
    pub tools: Vec<ApiTool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiPreview {
    pub title: String,
    pub tools: Vec<ApiTool>,
    pub skipped: Vec<String>,
}

pub fn encode_profile(profile: &ApiProfile) -> Result<String, String> {
    use base64::Engine;
    validate_profile(profile)?;
    let bytes = serde_json::to_vec(profile).map_err(|_| "Cannot encode API profile")?;
    if bytes.len() > MAX_CONFIG {
        return Err("API profile exceeds 512 KiB; select fewer tools".into());
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&bytes)
        .map_err(|_| "Cannot compress API profile")?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(
        encoder
            .finish()
            .map_err(|_| "Cannot compress API profile")?,
    );
    if encoded.len() > MAX_ENCODED {
        return Err("API profile exceeds 24 KiB encoded; select fewer tools".into());
    }
    Ok(encoded)
}

pub fn decode_profile(text: &str) -> Result<ApiProfile, String> {
    use base64::Engine;
    if text.len() > MAX_ENCODED {
        return Err("API profile exceeds 24 KiB encoded".into());
    }
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| "Invalid API profile encoding")?;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&compressed[..])
        .take((MAX_CONFIG + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Invalid compressed API profile")?;
    if bytes.len() > MAX_CONFIG {
        return Err("API profile exceeds 512 KiB".into());
    }
    let profile: ApiProfile =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid API profile JSON")?;
    validate_profile(&profile)?;
    Ok(profile)
}

pub fn validate_target(text: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(text).map_err(|_| "Expected an absolute HTTP(S) URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Only HTTP(S) URLs without embedded credentials or fragments are supported".into(),
        );
    }
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let metadata = host == "metadata.google.internal"
        || host.ends_with(".metadata.google.internal")
        || host == "100.100.100.200"
        || host == "fd00:ec2::254";
    let link_local = host.parse::<std::net::IpAddr>().is_ok_and(|ip| match ip {
        std::net::IpAddr::V4(v4) => v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_link_local() || v4.to_string() == "100.100.100.200")
        }
    });
    if metadata || link_local {
        return Err("Cloud metadata and link-local endpoints are not supported".into());
    }
    Ok(url)
}

pub fn validate_headers(headers: &BTreeMap<String, String>) -> Result<(), String> {
    if headers.len() > 64 {
        return Err("At most 64 request headers are supported".into());
    }
    for (key, value) in headers {
        reqwest::header::HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| "Invalid header name")?;
        reqwest::header::HeaderValue::from_str(value).map_err(|_| "Invalid header value")?;
        if value.len() > 8192
            || matches!(
                key.to_ascii_lowercase().as_str(),
                "host"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "upgrade"
                    | "proxy-authorization"
                    | "proxy-connection"
                    | "te"
                    | "trailer"
                    | "expect"
            )
        {
            return Err(
                "Transport headers cannot be customized, or header value exceeds 8 KiB".into(),
            );
        }
    }
    Ok(())
}

pub fn validate_profile(profile: &ApiProfile) -> Result<(), String> {
    if profile.version != 1 || profile.tools.is_empty() || profile.tools.len() > 100 {
        return Err("API profile version must be 1 with 1–100 tools".into());
    }
    let mut names = std::collections::HashSet::new();
    for tool in &profile.tools {
        if tool.name.is_empty()
            || tool.name.len() > 64
            || !tool
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            || !names.insert(&tool.name)
        {
            return Err("Tool names must be unique and contain 1–64 letters, digits, dots, dashes or underscores".into());
        }
        if !matches!(
            tool.method.as_str(),
            "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
        ) {
            return Err("Unsupported HTTP method".into());
        }
        validate_target(&tool.url)?;
        if tool.url.len() > 4096
            || tool.description.len() > 4000
            || tool.input_schema["type"] != "object"
        {
            return Err("Invalid tool URL, description or object input schema".into());
        }
        schema::check_schema(&tool.input_schema, 0)?;
        let props = tool.input_schema["properties"]
            .as_object()
            .ok_or("Input schema must define properties")?;
        if props
            .keys()
            .any(|k| !matches!(k.as_str(), "path" | "query" | "headers" | "body"))
        {
            return Err("Arguments must be grouped into path, query, headers and body".into());
        }
        for group in ["path", "query", "headers"] {
            if let Some(schema) = props.get(group) {
                if schema["type"] != "object" || !schema["properties"].is_object() {
                    return Err("Path, query and headers groups must have object schemas".into());
                }
            }
        }
        let path_props = props.get("path").and_then(|p| p["properties"].as_object());
        let path = tool.url.split('?').next().unwrap_or_default();
        for capture in PATH_PARAMETERS.captures_iter(path) {
            if !path_props.is_some_and(|p| p.contains_key(&capture[1])) {
                return Err("URL path placeholder has no parameter definition".into());
            }
        }
        if tool.url.contains('?') || tool.url.contains('#') {
            return Err("Define query parameters in the input schema, not the URL".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

use super::*;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::json;
use std::{sync::Arc, time::Duration};

pub struct ApiRuntime {
    pub(super) profile: ApiProfile,
    client: reqwest::Client,
    headers: HeaderMap,
}
impl ApiRuntime {
    pub fn new(mut profile: ApiProfile, headers: BTreeMap<String, String>) -> Result<Self, String> {
        validate_profile(&profile)?;
        validate_headers(&headers)?;
        for tool in &mut profile.tools {
            let schema = &mut tool.input_schema;
            if let Some(required) = schema
                .get_mut("properties")
                .and_then(|properties| properties.get_mut("headers"))
                .and_then(|headers| headers.get_mut("required"))
                .and_then(Value::as_array_mut)
            {
                required.retain(|name| {
                    !headers.keys().any(|key| {
                        name.as_str()
                            .is_some_and(|name| name.eq_ignore_ascii_case(key))
                    })
                });
                if required.is_empty() {
                    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut)
                    {
                        required.retain(|name| name != "headers");
                    }
                }
            }
        }
        let mut map = HeaderMap::new();
        for (key, value) in headers {
            map.insert(
                HeaderName::from_bytes(key.as_bytes()).map_err(|_| "Invalid header name")?,
                HeaderValue::from_str(&value).map_err(|_| "Invalid header value")?,
            );
        }
        Ok(Self {
            profile,
            client: client()?,
            headers: map,
        })
    }

    /// Reject invalid arguments before HTTP; stdio reports them as tool errors.
    pub async fn call(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let tool = self
            .profile
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .ok_or("Unknown API tool")?;
        schema::validate(&tool.input_schema, &arguments)?;
        let mut endpoint = tool.url.clone();
        if let Some(path) = arguments["path"].as_object() {
            for (key, value) in path {
                let value = scalar(value)?;
                if matches!(value.as_str(), "." | "..") {
                    return Err("Dot path segments are not allowed".into());
                }
                endpoint = endpoint.replace(&format!("{{{key}}}"), &encode_segment(&value));
            }
        }
        if endpoint.contains(['{', '}']) {
            return Err("Missing path parameter".into());
        }
        let mut url = validate_target(&endpoint)?;
        let expected = validate_target(&tool.url)?;
        if url.origin() != expected.origin() {
            return Err("Path arguments cannot change the API origin".into());
        }
        let query = with_defaults(
            &tool.input_schema["properties"]["query"],
            arguments.get("query"),
        );
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                if let Some(items) = value.as_array() {
                    for item in items {
                        pairs.append_pair(&key, &scalar(item)?);
                    }
                } else {
                    pairs.append_pair(&key, &scalar(&value)?);
                }
            }
        }
        let dynamic = with_defaults(
            &tool.input_schema["properties"]["headers"],
            arguments.get("headers"),
        );
        let mut headers = BTreeMap::new();
        for (key, value) in dynamic {
            headers.insert(key, scalar(&value)?);
        }
        validate_headers(&headers)?;
        let mut all_headers = HeaderMap::new();
        for (key, value) in headers {
            all_headers.insert(
                HeaderName::from_bytes(key.as_bytes()).map_err(|_| "Invalid header name")?,
                HeaderValue::from_str(&value).map_err(|_| "Invalid header value")?,
            );
        }
        // HeaderMap canonicalizes case: a model can never override static auth.
        all_headers.extend(self.headers.clone());
        let method =
            reqwest::Method::from_bytes(tool.method.as_bytes()).map_err(|_| "Invalid method")?;
        let mut request = self.client.request(method, url).headers(all_headers);
        if let Some(body) = arguments.get("body") {
            request = request.json(body);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => return Ok(tool_error(network_error(&error))),
        };
        let status = response.status();
        let bytes = match read_limited(response, MAX_RESPONSE).await {
            Ok(bytes) => bytes,
            Err(error) => return Ok(tool_error(error)),
        };
        let body = serde_json::from_slice::<Value>(&bytes)
            .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
        let text = json!({"status":status.as_u16(),"body":body}).to_string();
        Ok(json!({"content":[{"type":"text","text":text}],"isError":!status.is_success()}))
    }
}

fn with_defaults(schema: &Value, value: Option<&Value>) -> BTreeMap<String, Value> {
    let mut result = BTreeMap::new();
    if let Some(props) = schema["properties"].as_object() {
        for (key, schema) in props {
            if let Some(value) = schema.get("default") {
                result.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(values) = value.and_then(Value::as_object) {
        result.extend(
            values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    result
}
fn scalar(value: &Value) -> Result<String, String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(_) | Value::Bool(_) => Ok(value.to_string()),
        _ => Err("Only primitive parameter values are supported".into()),
    }
}
fn encode_segment(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}
fn tool_error(message: String) -> Value {
    json!({"content":[{"type":"text","text":message}],"isError":true})
}
fn network_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "API request timed out".into()
    } else if error.is_connect() {
        "Cannot connect to API endpoint".into()
    } else {
        "API request failed".into()
    }
}
pub(super) async fn read_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(format!("Response exceeds {limit} bytes"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| network_error(&error))?
    {
        if bytes.len() + chunk.len() > limit {
            return Err(format!("Response exceeds {limit} bytes"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub async fn fetch_document(url: &str) -> Result<String, String> {
    validate_target(url)?;
    let response = client()?
        .get(url)
        .send()
        .await
        .map_err(|error| network_error(&error))?;
    if !response.status().is_success() {
        return Err(format!(
            "Document returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let bytes = read_limited(response, 5 * 1024 * 1024).await?;
    String::from_utf8(bytes).map_err(|_| "API document must be UTF-8".into())
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .dns_resolver(Arc::new(ApiResolver))
        .no_proxy()
        .build()
        .map_err(|_| "Cannot initialize API HTTP client".into())
}

/// Check actual DNS answers as well as literal URLs, including IPv4-mapped IPv6.
struct ApiResolver;
impl reqwest::dns::Resolve for ApiResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let addresses: Vec<_> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            for address in &addresses {
                let host = match address.ip() {
                    std::net::IpAddr::V4(ip) => ip.to_string(),
                    std::net::IpAddr::V6(ip) => format!("[{ip}]"),
                };
                if validate_target(&format!("http://{host}/")).is_err() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "Metadata/link-local DNS target blocked",
                    )
                    .into());
                }
            }
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

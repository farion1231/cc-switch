//! OpenCode gateway session affinity (`x-opencode-session`).

use super::*;

/// Whether the upstream belongs to the OpenCode gateway (`opencode.ai`).
///
/// That gateway (<https://opencode.ai/docs/go/>) expects clients to identify
/// themselves with a `User-Agent` and to carry a **stable per-conversation**
/// session id in `x-opencode-session`; a request without the session header is
/// rejected with `400 MissingSessionID`. Claude Code and Codex already send
/// headers the gateway recognises, Claude Desktop does not - the proxy fills
/// the gap (see the injection site in `forward`).
pub(super) fn is_opencode_upstream(upstream_host: Option<&str>) -> bool {
    upstream_host
        .map(|host| {
            let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
            host.trim_matches(['[', ']']).to_ascii_lowercase()
        })
        .is_some_and(|host| host == "opencode.ai" || host.ends_with(".opencode.ai"))
}

/// Derive a session id that stays **stable within one conversation**: a digest
/// of the system prompt plus the first user message.
///
/// The gateway uses the session id for upstream routing and prompt caching, so
/// it must not change between the turns of a conversation. Claude Desktop hands
/// CC Switch a fresh id on every request, which defeats that; hashing the
/// conversation prefix keeps the id stable while still separating conversations.
pub(super) fn conversation_fingerprint(body: &Value) -> Option<String> {
    let mut hasher = Sha256::new();
    let mut fed = false;
    if let Some(system) = body.get("system") {
        hasher.update(without_cache_control(system).to_string().as_bytes());
        fed = true;
    }
    if let Some(first_user) = body
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages
                .iter()
                .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
        })
    {
        hasher.update(without_cache_control(first_user).to_string().as_bytes());
        fed = true;
    }
    if !fed {
        return None;
    }
    let digest = format!("{:x}", hasher.finalize());
    Some(format!("ccsw-{}", &digest[..32]))
}

/// A stable session header the client already sends and the gateway recognises
/// natively (Claude Code, Codex). Checked before deriving an id of our own.
pub(super) fn native_session_header(headers: &http::HeaderMap) -> Option<&str> {
    [
        "x-claude-code-session-id",
        "session-id",
        "session_id",
        "x-session-id",
    ]
    .iter()
    .find_map(|name| {
        headers
            .get(*name)?
            .to_str()
            .ok()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

/// Pick the value for `x-opencode-session`, most stable identity first: a native
/// session header the client already sends, the client-provided session id, the
/// conversation fingerprint, then the generated id.
pub(super) fn pick_opencode_session<'a>(
    native: Option<&'a str>,
    client_provided: Option<&'a str>,
    fingerprint: Option<&'a str>,
    generated: &'a str,
) -> Option<String> {
    [native, client_provided, fingerprint, Some(generated)]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

/// Clone `value` without any `cache_control` key, at any depth.
///
/// Clients move that marker between messages as the conversation grows (Claude
/// Code puts it on the first user message, then on the last one), so hashing it
/// would change the session id once, mid-conversation.
pub(super) fn without_cache_control(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "cache_control")
                .map(|(key, value)| (key.clone(), without_cache_control(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_cache_control).collect()),
        other => other.clone(),
    }
}

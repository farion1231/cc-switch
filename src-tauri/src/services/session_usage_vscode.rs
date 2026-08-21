//! VS Code Copilot 会话使用追踪
//!
//! VS Code 会把聊天状态保存为 JSONL：首行是快照，后续行是字段替换或数组追加。
//! 本模块只提取模型标识、请求标识、时间与 token 数，不提取、保存或记录会话正文。

use crate::copilot_byok::CopilotByokGroup;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::usage_stats::find_model_pricing;
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::Metadata;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const APP_TYPE: &str = "copilot-byok";
const DATA_SOURCE: &str = "vscode_session";
const REQUEST_ID_PREFIX: &str = "vscode_session:";
const PROVIDER_ID: &str = "vscode-copilot";
const PROVIDER_NAME: &str = "VSCode Copilot";
const SESSION_PROVIDER_NAME: &str = "VS Code Copilot (Session)";
// v8 replays v7 rows once after VS Code session details became exempt from the
// generic 30-day rollup. Stable request IDs can now be upserted on catalog or
// JSONL changes without re-aggregating the same historical request.
const SYNC_VERSION: &str = "v8";
const CATALOG_SYNC_KEY: &str = "vscode_session:v8:catalog";
const MAX_SESSION_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SESSION_REQUESTS: usize = 10_000;

#[derive(Debug, Clone, Default)]
struct SelectedModel {
    identifier: String,
    vendor: String,
    name: String,
    provider_label: String,
    is_byok: bool,
}

impl SelectedModel {
    fn is_custom_endpoint(&self) -> bool {
        self.is_byok
            || self.vendor.eq_ignore_ascii_case("customendpoint")
            || self
                .identifier
                .to_ascii_lowercase()
                .starts_with("customendpoint/")
    }
}

#[derive(Debug, Clone, Default)]
struct VscodeRequest {
    request_id: String,
    response_id: String,
    model_id: String,
    timestamp_ms: i64,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    result_prompt_tokens: Option<u32>,
    result_output_tokens: Option<u32>,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    elapsed_ms: u64,
    resolved_model: String,
    resolved_model_name: String,
    model_state: Option<u8>,
    error_message: Option<String>,
    selected_model: SelectedModel,
    selected_at_send: bool,
}

impl VscodeRequest {
    fn raw_input_tokens(&self) -> u32 {
        self.prompt_tokens
            .unwrap_or(0)
            .max(self.result_prompt_tokens.unwrap_or(0))
    }

    fn input_tokens(&self) -> u32 {
        self.raw_input_tokens()
            .saturating_sub(self.cache_read_tokens)
            .saturating_sub(self.cache_creation_tokens)
    }

    fn output_tokens(&self) -> u32 {
        self.completion_tokens
            .unwrap_or(0)
            .max(self.result_output_tokens.unwrap_or(0))
    }

    fn has_usage(&self) -> bool {
        self.input_tokens() > 0
            || self.output_tokens() > 0
            || self.cache_read_tokens > 0
            || self.cache_creation_tokens > 0
    }

    fn is_in_progress(&self) -> bool {
        matches!(self.model_state, Some(0 | 4))
    }

    fn should_record(&self) -> bool {
        !self.is_in_progress()
            && (!self.request_id.is_empty()
                || !self.response_id.is_empty()
                || !self.effective_model_id().is_empty())
            && (self.has_usage() || self.model_state.is_some() || self.error_message.is_some())
    }

    fn is_custom_endpoint(&self) -> bool {
        // A qualified request model is the per-request source of truth. VS Code
        // can append `copilot/auto` while inputState.selectedModel still points
        // at the previously selected Custom Endpoint; treating that stale UI
        // state as authoritative would price subscription traffic as BYOK.
        if let Some((vendor, _)) = self.requested_model_id().split_once('/') {
            return vendor.trim().eq_ignore_ascii_case("customendpoint");
        }
        self.selected_at_send && self.selected_model.is_custom_endpoint()
    }

    fn status_code(&self) -> i64 {
        match self.model_state {
            Some(2) => 499,
            Some(3) => 500,
            _ if self.error_message.is_some() => 500,
            _ => 200,
        }
    }

    fn requested_model_id(&self) -> &str {
        if self.model_id.trim().is_empty() {
            self.selected_model.identifier.trim()
        } else {
            self.model_id.trim()
        }
    }

    fn effective_model_id(&self) -> &str {
        if self.resolved_model.trim().is_empty() {
            self.requested_model_id()
        } else {
            self.resolved_model.trim()
        }
    }
}

#[derive(Debug, Default)]
struct VscodeSession {
    session_id: String,
    creation_date_ms: i64,
    selected_model: SelectedModel,
    requests: Vec<VscodeRequest>,
    line_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogEntry {
    group_id: String,
    group_name: String,
    model_id: String,
    model_name: String,
    website_url: Option<String>,
    notes: Option<String>,
    icon: Option<String>,
    icon_color: Option<String>,
}

impl CatalogEntry {
    fn for_usage(mut self) -> Self {
        self.group_id = PROVIDER_ID.to_string();
        self.group_name = PROVIDER_NAME.to_string();
        self.website_url = Some("https://github.com/features/copilot".to_string());
        self.notes = None;
        self.icon = Some("vscode-copilot-byok".to_string());
        self.icon_color = None;
        self
    }
}

#[derive(Debug)]
struct ByokCatalog {
    by_model: HashMap<String, Vec<CatalogEntry>>,
    fingerprint: i64,
}

impl ByokCatalog {
    fn from_groups(groups: &[CopilotByokGroup]) -> Self {
        let mut by_model: HashMap<String, Vec<CatalogEntry>> = HashMap::new();
        let mut fingerprint_rows = Vec::new();

        for group in groups {
            for model in &group.models {
                let key = normalize_model_key(&model.model_id);
                if key.is_empty() {
                    continue;
                }
                let entry = CatalogEntry {
                    group_id: group.id.clone(),
                    group_name: group.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.name.clone(),
                    website_url: group.website_url.clone(),
                    notes: group.notes.clone(),
                    icon: group.icon.clone(),
                    icon_color: group.icon_color.clone(),
                };
                fingerprint_rows.push(format!(
                    "{}\0{}\0{}\0{}",
                    entry.group_id, entry.group_name, entry.model_id, entry.model_name
                ));
                by_model.entry(key.clone()).or_default().push(entry.clone());
                if let Some(short_key) = key.rsplit('/').next().filter(|part| *part != key) {
                    by_model
                        .entry(short_key.to_string())
                        .or_default()
                        .push(entry);
                }
            }
        }

        fingerprint_rows.sort();
        let mut hasher = Sha256::new();
        for row in fingerprint_rows {
            hasher.update(row.as_bytes());
            hasher.update(b"\n");
        }

        let digest = hasher.finalize();
        let mut marker = [0u8; 8];
        marker.copy_from_slice(&digest[..8]);
        Self {
            by_model,
            // session_log_sync stores signed 64-bit integers. Clearing the sign
            // bit gives us a stable catalog marker without creating one row per
            // historical catalog revision.
            fingerprint: i64::from_be_bytes(marker) & i64::MAX,
        }
    }

    fn resolve(&self, request: &VscodeRequest) -> Option<CatalogEntry> {
        let request_is_custom = request.is_custom_endpoint();

        if request_is_custom {
            let mut key = normalize_model_key(request.effective_model_id());
            if key.is_empty() {
                key = normalize_model_key(&request.selected_model.identifier);
            }
            let candidates = self.by_model.get(&key).or_else(|| {
                key.rsplit_once('/')
                    .and_then(|(_, short_key)| self.by_model.get(short_key))
            });
            if let Some(candidates) = candidates {
                // 只有增量记录或单请求快照能证明 selectedModel 就是发送时的选择。
                // 多请求快照只保留会话当前选择，不能用它反推旧请求属于哪个同模型供应商。
                if request.selected_at_send {
                    let provider_matches: Vec<&CatalogEntry> = candidates
                        .iter()
                        .filter(|entry| {
                            !request.selected_model.provider_label.is_empty()
                                && entry
                                    .group_name
                                    .eq_ignore_ascii_case(&request.selected_model.provider_label)
                        })
                        .collect();
                    if provider_matches.len() == 1 {
                        return Some(provider_matches[0].clone().for_usage());
                    }
                    let name_matches: Vec<&CatalogEntry> = candidates
                        .iter()
                        .filter(|entry| {
                            !request.selected_model.name.is_empty()
                                && entry
                                    .model_name
                                    .eq_ignore_ascii_case(&request.selected_model.name)
                        })
                        .collect();
                    if name_matches.len() == 1 {
                        return Some(name_matches[0].clone().for_usage());
                    }
                }
                if candidates.len() == 1 {
                    return Some(candidates[0].clone().for_usage());
                }
            }
        }

        session_catalog_entry(request)
    }
}

fn normalize_model_key(model_id: &str) -> String {
    let normalized = model_id.trim().to_ascii_lowercase();
    normalized
        .strip_prefix("customendpoint/")
        .unwrap_or(&normalized)
        .to_string()
}

fn session_catalog_entry(request: &VscodeRequest) -> Option<CatalogEntry> {
    let requested_model_id = request.requested_model_id();
    let effective_model_id = request.effective_model_id();
    if requested_model_id.is_empty() || effective_model_id.is_empty() {
        return None;
    }

    let model_vendor = requested_model_id
        .split_once('/')
        .map(|(vendor, _)| vendor.trim())
        .unwrap_or("");
    let selected_identifier_matches = request
        .selected_model
        .identifier
        .eq_ignore_ascii_case(requested_model_id);
    let selected_model_is_relevant = request.selected_at_send || selected_identifier_matches;
    let is_custom = request.is_custom_endpoint();
    let selected_vendor_matches = !model_vendor.is_empty()
        && request
            .selected_model
            .vendor
            .eq_ignore_ascii_case(model_vendor);
    let can_use_selected_metadata =
        selected_model_is_relevant || (!is_custom && selected_vendor_matches);

    let vendor = if !model_vendor.is_empty() {
        model_vendor
    } else {
        request.selected_model.vendor.trim()
    };
    let is_official_copilot = vendor.eq_ignore_ascii_case("copilot")
        || vendor.eq_ignore_ascii_case("github-copilot")
        || (selected_model_is_relevant
            && request
                .selected_model
                .provider_label
                .eq_ignore_ascii_case("GitHub Copilot"));
    // Ignore other language-model extensions. This importer is deliberately
    // scoped to GitHub Copilot and VS Code's Custom Endpoint integration.
    if !is_custom && !is_official_copilot {
        return None;
    }

    let model_id = if is_custom {
        normalize_model_key(effective_model_id)
    } else {
        effective_model_id.to_string()
    };
    if model_id.is_empty() {
        return None;
    }
    let requested_is_auto = requested_model_id
        .rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("auto"));
    let model_name = (!request.resolved_model_name.trim().is_empty())
        .then_some(request.resolved_model_name.trim())
        .or_else(|| {
            (can_use_selected_metadata && !requested_is_auto)
                .then_some(request.selected_model.name.trim())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or(&model_id)
        .to_string();

    Some(CatalogEntry {
        group_id: PROVIDER_ID.to_string(),
        group_name: PROVIDER_NAME.to_string(),
        model_id,
        model_name,
        website_url: Some("https://github.com/features/copilot".to_string()),
        notes: None,
        icon: Some("vscode-copilot-byok".to_string()),
        icon_color: None,
    })
}

fn value_u32(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .map(|number| number.min(u64::from(u32::MAX)) as u32)
}

fn value_i64(value: Option<&Value>) -> i64 {
    value
        .and_then(Value::as_i64)
        .or_else(|| {
            value
                .and_then(Value::as_u64)
                .map(|number| number.min(i64::MAX as u64) as i64)
        })
        .unwrap_or(0)
}

fn value_u64(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).unwrap_or(0)
}

fn value_string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_string()
}

fn max_u32(value: &Value, pointers: &[&str]) -> Option<u32> {
    pointers
        .iter()
        .filter_map(|pointer| value_u32(value.pointer(pointer)))
        .max()
}

fn parse_model_state(value: Option<&Value>) -> Option<u8> {
    let value = value?;
    value
        .pointer("/value")
        .or(Some(value))
        .and_then(Value::as_u64)
        .and_then(|state| u8::try_from(state).ok())
}

fn truncated_error_message(value: &Value) -> Option<String> {
    let message = [
        "/errorDetails/message",
        "/errorDetails/code",
        "/metadata/errorDetails/message",
        "/metadata/errorDetails/code",
    ]
    .into_iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))?
    .trim();
    if message.is_empty() {
        return None;
    }
    Some(message.chars().take(512).collect())
}

fn apply_response_metadata(request: &mut VscodeRequest, value: &Value) {
    let values = value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(value));
    for response in values {
        if let Some(model) = response
            .get("resolvedModel")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            request.resolved_model = model.to_string();
        }
        if let Some(name) = response
            .get("resolvedModelName")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            request.resolved_model_name = name.to_string();
        }
    }
}

fn parse_selected_model(value: &Value) -> SelectedModel {
    SelectedModel {
        identifier: value_string(value.get("identifier")),
        vendor: value_string(value.pointer("/metadata/vendor")),
        name: value_string(value.pointer("/metadata/name")),
        provider_label: value_string(value.pointer("/metadata/auth/providerLabel")),
        is_byok: value
            .pointer("/metadata/isBYOK")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn clear_result_metadata(request: &mut VscodeRequest) {
    request.result_prompt_tokens = None;
    request.result_output_tokens = None;
    request.cache_read_tokens = 0;
    request.cache_creation_tokens = 0;
    request.resolved_model.clear();
    request.resolved_model_name.clear();
    request.error_message = None;
}

fn clear_result_usage(request: &mut VscodeRequest) {
    request.result_prompt_tokens = None;
    request.result_output_tokens = None;
    request.cache_read_tokens = 0;
    request.cache_creation_tokens = 0;
    request.error_message = None;
}

fn apply_result_metadata(request: &mut VscodeRequest, result: &Value) {
    if let Some(prompt_tokens) = max_u32(
        result,
        &[
            "/metadata/promptTokens",
            "/usage/promptTokens",
            "/usage/inputTokens",
        ],
    ) {
        request.result_prompt_tokens = Some(prompt_tokens);
    }
    if let Some(output_tokens) = max_u32(
        result,
        &[
            "/metadata/outputTokens",
            "/metadata/completionTokens",
            "/usage/completionTokens",
            "/usage/outputTokens",
        ],
    ) {
        request.result_output_tokens = Some(output_tokens);
    }
    let direct_cache_read = max_u32(
        result,
        &[
            "/usage/prompt_tokens_details/cached_tokens",
            "/usage/promptTokensDetails/cachedTokens",
            "/metadata/usage/prompt_tokens_details/cached_tokens",
        ],
    );
    let summary_cache_read = result
        .pointer("/metadata/summaries")
        .and_then(Value::as_array)
        .and_then(|summaries| {
            let values: Vec<u32> = summaries
                .iter()
                .filter_map(|summary| {
                    max_u32(
                        summary,
                        &[
                            "/usage/prompt_tokens_details/cached_tokens",
                            "/usage/promptTokensDetails/cachedTokens",
                        ],
                    )
                })
                .collect();
            (!values.is_empty()).then(|| values.into_iter().fold(0u32, u32::saturating_add))
        });
    if let Some(cache_read_tokens) = [direct_cache_read, summary_cache_read]
        .into_iter()
        .flatten()
        .max()
    {
        request.cache_read_tokens = cache_read_tokens;
    }
    if let Some(cache_creation_tokens) = max_u32(
        result,
        &[
            "/usage/cache_creation_input_tokens",
            "/usage/cacheCreationInputTokens",
            "/metadata/usage/cache_creation_input_tokens",
        ],
    ) {
        request.cache_creation_tokens = cache_creation_tokens;
    }
    if let Some(resolved_model) = [
        "/metadata/resolvedModel",
        "/resolvedModel",
        "/metadata/modelId",
    ]
    .into_iter()
    .find_map(|pointer| result.pointer(pointer).and_then(Value::as_str))
    .map(str::trim)
    .filter(|value| !value.is_empty())
    {
        request.resolved_model = resolved_model.to_string();
    }
    if let Some(resolved_model_name) = ["/metadata/resolvedModelName", "/resolvedModelName"]
        .into_iter()
        .find_map(|pointer| result.pointer(pointer).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request.resolved_model_name = resolved_model_name.to_string();
    }
    if let Some(response) = result.get("response") {
        apply_response_metadata(request, response);
    }
    if result.get("errorDetails").is_some() || result.pointer("/metadata/errorDetails").is_some() {
        request.error_message = truncated_error_message(result);
    }
    if request.response_id.is_empty() {
        request.response_id = value_string(result.pointer("/metadata/responseId"));
    }
}

fn project_patch(path: &[Value], value: &Value) -> Option<Value> {
    let Some((head, tail)) = path.split_first() else {
        return Some(value.clone());
    };
    let child = project_patch(tail, value)?;
    if let Some(key) = head.as_str() {
        let mut object = serde_json::Map::new();
        object.insert(key.to_string(), child);
        Some(Value::Object(object))
    } else if let Some(index) = head
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < MAX_SESSION_REQUESTS)
    {
        let mut array = vec![Value::Null; index.saturating_add(1)];
        array[index] = child;
        Some(Value::Array(array))
    } else {
        None
    }
}

fn parse_request(
    value: &Value,
    selected_model: &SelectedModel,
    selected_at_send: bool,
) -> VscodeRequest {
    let mut request = VscodeRequest {
        request_id: value_string(value.get("requestId")),
        response_id: value_string(value.get("responseId")),
        model_id: value_string(value.get("modelId")),
        timestamp_ms: value_i64(value.get("timestamp")),
        prompt_tokens: value_u32(value.get("promptTokens")),
        completion_tokens: value_u32(value.get("completionTokens")),
        elapsed_ms: value_u64(value.get("elapsedMs")),
        model_state: parse_model_state(value.get("modelState")),
        selected_model: selected_model.clone(),
        selected_at_send,
        ..VscodeRequest::default()
    };
    if let Some(result) = value.get("result") {
        apply_result_metadata(&mut request, result);
    }
    if let Some(response) = value.get("response") {
        apply_response_metadata(&mut request, response);
    }
    request
}

fn parse_snapshot(value: &Value) -> VscodeSession {
    let selected_model = value
        .pointer("/inputState/selectedModel")
        .map(parse_selected_model)
        .unwrap_or_default();
    let requests = value.get("requests").and_then(Value::as_array);
    let selected_at_send = requests.is_some_and(|requests| requests.len() == 1);
    let requests = requests
        .map(|requests| {
            requests
                .iter()
                .take(MAX_SESSION_REQUESTS)
                .map(|request| parse_request(request, &selected_model, selected_at_send))
                .collect()
        })
        .unwrap_or_default();

    VscodeSession {
        session_id: value_string(value.get("sessionId")),
        creation_date_ms: value_i64(value.get("creationDate")),
        selected_model,
        requests,
        line_count: 0,
    }
}

fn ensure_request(session: &mut VscodeSession, index: usize) -> &mut VscodeRequest {
    let selected_model = session.selected_model.clone();
    if session.requests.len() <= index {
        session.requests.resize_with(index + 1, || VscodeRequest {
            selected_model: selected_model.clone(),
            selected_at_send: true,
            ..VscodeRequest::default()
        });
    }
    &mut session.requests[index]
}

fn apply_set_patch(session: &mut VscodeSession, path: &[Value], value: &Value) {
    if path.len() == 2
        && path[0].as_str() == Some("inputState")
        && path[1].as_str() == Some("selectedModel")
    {
        session.selected_model = parse_selected_model(value);
        return;
    }

    if path.first().and_then(Value::as_str) != Some("requests") {
        return;
    }
    let Some(index) = path
        .get(1)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|index| *index < MAX_SESSION_REQUESTS)
    else {
        return;
    };
    if path.len() == 2 {
        let selected = session.selected_model.clone();
        let parsed = parse_request(value, &selected, true);
        if session.requests.len() <= index {
            session.requests.resize(index + 1, VscodeRequest::default());
        }
        session.requests[index] = parsed;
        return;
    }

    let Some(field) = path.get(2).and_then(Value::as_str) else {
        return;
    };
    let request = ensure_request(session, index);
    match field {
        "requestId" => request.request_id = value_string(Some(value)),
        "responseId" => request.response_id = value_string(Some(value)),
        "modelId" => request.model_id = value_string(Some(value)),
        "timestamp" => request.timestamp_ms = value_i64(Some(value)),
        "promptTokens" => request.prompt_tokens = value_u32(Some(value)),
        "completionTokens" => request.completion_tokens = value_u32(Some(value)),
        "elapsedMs" => request.elapsed_ms = value_u64(Some(value)),
        "modelState" => request.model_state = parse_model_state(Some(value)),
        "result" => {
            if path.len() == 3 {
                // Preserve the best-known routed model across successive full
                // result snapshots; VS Code does not repeat resolvedModel in
                // every update.
                clear_result_usage(request);
                apply_result_metadata(request, value);
            } else if let Some(projected) = project_patch(&path[3..], value) {
                apply_result_metadata(request, &projected);
            }
        }
        "response" => {
            if path.len() == 3 {
                apply_response_metadata(request, value);
            } else if let Some(projected) = project_patch(&path[3..], value) {
                apply_response_metadata(request, &projected);
            }
        }
        _ => {}
    }
}

fn apply_append_patch(
    session: &mut VscodeSession,
    path: &[Value],
    insertion_index: Option<usize>,
    value: Option<&Value>,
) {
    if path.first().and_then(Value::as_str) != Some("requests") {
        return;
    }

    if path.len() == 1 {
        if let Some(index) = insertion_index {
            session.requests.truncate(index);
        }
        let Some(requests) = value.and_then(Value::as_array) else {
            return;
        };
        let available = MAX_SESSION_REQUESTS.saturating_sub(session.requests.len());
        for request in requests.iter().take(available) {
            session
                .requests
                .push(parse_request(request, &session.selected_model, true));
        }
        return;
    }

    let Some(index) = path
        .get(1)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|index| *index < MAX_SESSION_REQUESTS)
    else {
        return;
    };
    let Some(field) = path.get(2).and_then(Value::as_str) else {
        return;
    };
    if field == "response" {
        if let Some(value) = value {
            apply_response_metadata(ensure_request(session, index), value);
        }
    }
}

fn apply_delete_patch(session: &mut VscodeSession, path: &[Value]) {
    if path.len() == 2
        && path[0].as_str() == Some("inputState")
        && path[1].as_str() == Some("selectedModel")
    {
        session.selected_model = SelectedModel::default();
        return;
    }
    if path.first().and_then(Value::as_str) != Some("requests") {
        return;
    }
    let Some(index) = path
        .get(1)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|index| *index < MAX_SESSION_REQUESTS)
    else {
        return;
    };
    if path.len() == 2 {
        if index < session.requests.len() {
            session.requests.remove(index);
        }
        return;
    }
    let Some(request) = session.requests.get_mut(index) else {
        return;
    };
    match path.get(2).and_then(Value::as_str) {
        Some("requestId") => request.request_id.clear(),
        Some("responseId") => request.response_id.clear(),
        Some("modelId") => request.model_id.clear(),
        Some("timestamp") => request.timestamp_ms = 0,
        Some("promptTokens") => request.prompt_tokens = None,
        Some("completionTokens") => request.completion_tokens = None,
        Some("elapsedMs") => request.elapsed_ms = 0,
        Some("modelState") => request.model_state = None,
        Some("result") => {
            if path.len() == 3 {
                clear_result_metadata(request);
            } else {
                match path.last().and_then(Value::as_str) {
                    Some("promptTokens" | "inputTokens") => request.result_prompt_tokens = None,
                    Some("outputTokens" | "completionTokens") => {
                        request.result_output_tokens = None
                    }
                    Some("cached_tokens" | "cachedTokens") => request.cache_read_tokens = 0,
                    Some("cache_creation_input_tokens" | "cacheCreationInputTokens") => {
                        request.cache_creation_tokens = 0
                    }
                    Some("resolvedModel" | "modelId") => request.resolved_model.clear(),
                    Some("resolvedModelName") => request.resolved_model_name.clear(),
                    Some("message" | "code")
                        if path
                            .iter()
                            .any(|part| part.as_str() == Some("errorDetails")) =>
                    {
                        request.error_message = None
                    }
                    Some("responseId") => request.response_id.clear(),
                    _ => {}
                }
            }
        }
        Some("response") => match path.last().and_then(Value::as_str) {
            Some("resolvedModel") => request.resolved_model.clear(),
            Some("resolvedModelName") => request.resolved_model_name.clear(),
            _ if path.len() == 3 => {
                request.resolved_model.clear();
                request.resolved_model_name.clear();
            }
            _ => {}
        },
        _ => {}
    }
}

fn parse_session_file(path: &Path) -> Result<VscodeSession, AppError> {
    let metadata = session_file_metadata(path)?;
    if metadata.len() > MAX_SESSION_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "VS Code chat session exceeds {} MiB: {}",
            MAX_SESSION_FILE_BYTES / 1024 / 1024,
            path.display()
        )));
    }

    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let reader = BufReader::new(file);
    let mut session = VscodeSession::default();
    let mut saw_snapshot = false;

    for (index, line) in reader.lines().enumerate() {
        session.line_count = session.line_count.saturating_add(1);
        let line = line.map_err(|error| AppError::io(path, error))?;
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str::<Value>(&line).map_err(|error| {
            AppError::Config(format!(
                "Failed to parse VS Code chat session {} at JSONL line {}: {error}",
                path.display(),
                index.saturating_add(1)
            ))
        })?;
        match record.get("kind").and_then(Value::as_u64) {
            Some(0) => {
                if let Some(value) = record.get("v") {
                    let line_count = session.line_count;
                    session = parse_snapshot(value);
                    session.line_count = line_count;
                    saw_snapshot = true;
                }
            }
            Some(1) => {
                if let (Some(path), Some(value)) =
                    (record.get("k").and_then(Value::as_array), record.get("v"))
                {
                    apply_set_patch(&mut session, path, value);
                }
            }
            Some(2) => {
                if let Some(path) = record.get("k").and_then(Value::as_array) {
                    let insertion_index = record
                        .get("i")
                        .and_then(Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok())
                        .filter(|index| *index < MAX_SESSION_REQUESTS);
                    apply_append_patch(&mut session, path, insertion_index, record.get("v"));
                }
            }
            Some(3) => {
                if let Some(path) = record.get("k").and_then(Value::as_array) {
                    apply_delete_patch(&mut session, path);
                }
            }
            _ => {}
        }
    }

    if !saw_snapshot {
        return Err(AppError::Config(format!(
            "VS Code chat session has no initial snapshot: {}",
            path.display()
        )));
    }

    if session.session_id.is_empty() {
        session.session_id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string();
    }
    Ok(session)
}

fn session_file_metadata(path: &Path) -> Result<Metadata, AppError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::InvalidInput(format!(
            "VS Code chat session is not a regular file: {}",
            path.display()
        )));
    }
    Ok(metadata)
}

fn push_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"))
        {
            files.push(path);
        }
    }
}

fn collect_workspace_sessions(workspace_storage: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(workspace_storage) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            push_jsonl_files(&path.join("chatSessions"), files);
        }
    }
}

fn collect_chat_session_files(user_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_workspace_sessions(&user_dir.join("workspaceStorage"), &mut files);
    push_jsonl_files(
        &user_dir
            .join("globalStorage")
            .join("emptyWindowChatSessions"),
        &mut files,
    );

    // 某些便携版会把 profile 的存储放在 profile 目录内，固定下探两层即可。
    if let Ok(profiles) = fs::read_dir(user_dir.join("profiles")) {
        for profile in profiles.flatten() {
            let Ok(file_type) = profile.file_type() else {
                continue;
            };
            let path = profile.path();
            if !file_type.is_dir() {
                continue;
            }
            collect_workspace_sessions(&path.join("workspaceStorage"), &mut files);
            push_jsonl_files(
                &path.join("globalStorage").join("emptyWindowChatSessions"),
                &mut files,
            );
        }
    }

    files.sort();
    files.dedup();
    files
}

fn default_vscode_user_dirs() -> Vec<PathBuf> {
    let Some(config_dir) = dirs::config_dir() else {
        return Vec::new();
    };
    vec![
        config_dir.join("Code").join("User"),
        config_dir.join("Code - Insiders").join("User"),
    ]
}

fn sync_provider(db: &Database, entry: &CatalogEntry) -> Result<(), AppError> {
    let icon = entry
        .icon
        .clone()
        .or_else(|| Some("vscode-copilot-byok".to_string()));
    let existing = db.get_provider_by_id(&entry.group_id, APP_TYPE)?;
    let mut model_names = existing
        .as_ref()
        .and_then(|provider| provider.settings_config.get("modelNames"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if !entry.model_id.trim().is_empty() && !entry.model_name.trim().is_empty() {
        model_names.insert(entry.model_id.clone(), json!(entry.model_name));
    }
    let settings_config = json!({
        "source": DATA_SOURCE,
        "modelNames": model_names,
    });
    if existing.as_ref().is_some_and(|provider| {
        provider.name == SESSION_PROVIDER_NAME
            && provider.settings_config == settings_config
            && provider.category.as_deref() == Some("VS Code Copilot")
            && provider.icon == icon
            && provider.icon_color == entry.icon_color
            && provider.website_url == entry.website_url
            && provider.notes == entry.notes
    }) {
        return Ok(());
    }
    let mut provider = Provider::with_id(
        PROVIDER_ID.to_string(),
        SESSION_PROVIDER_NAME.to_string(),
        settings_config,
        None,
    );
    provider.category = Some("VS Code Copilot".to_string());
    provider.icon = icon;
    provider.icon_color = entry.icon_color.clone();
    provider.website_url = entry.website_url.clone();
    provider.notes = entry.notes.clone();
    db.save_provider(APP_TYPE, &provider)?;
    Ok(())
}

fn sync_copilot_provider(db: &Database) -> Result<u32, AppError> {
    sync_provider(
        db,
        &CatalogEntry {
            group_id: PROVIDER_ID.to_string(),
            group_name: PROVIDER_NAME.to_string(),
            model_id: String::new(),
            model_name: String::new(),
            website_url: Some("https://github.com/features/copilot".to_string()),
            notes: None,
            icon: Some("vscode-copilot-byok".to_string()),
            icon_color: None,
        },
    )?;

    // Normalize rows imported by earlier versions, including files that no
    // longer exist and therefore cannot be replayed during this run.
    let mut conn = lock_conn!(db.conn);
    let transaction = conn.transaction().map_err(|error| {
        AppError::Database(format!("开始规范化 VS Code Copilot 用量失败: {error}"))
    })?;
    let provider_updates = transaction
        .execute(
            "UPDATE proxy_request_logs
         SET provider_id = ?1
         WHERE app_type = ?2 AND data_source = ?3 AND provider_id != ?1",
            rusqlite::params![PROVIDER_ID, APP_TYPE, DATA_SOURCE],
        )
        .map_err(|error| {
            AppError::Database(format!("规范化 VS Code Copilot 供应商失败: {error}"))
        })?;
    let cost_updates = transaction
        .execute(
            "UPDATE proxy_request_logs
             SET cost_multiplier = '0',
                 input_cost_usd = '0',
                 output_cost_usd = '0',
                 cache_read_cost_usd = '0',
                 cache_creation_cost_usd = '0',
                 total_cost_usd = '0'
             WHERE app_type = ?1
               AND data_source = ?2
               AND (lower(trim(COALESCE(request_model, ''))) LIKE 'copilot/%'
                    OR lower(trim(COALESCE(request_model, ''))) LIKE 'github-copilot/%')
               AND (cost_multiplier != '0'
                    OR input_cost_usd != '0'
                    OR output_cost_usd != '0'
                    OR cache_read_cost_usd != '0'
                    OR cache_creation_cost_usd != '0'
                    OR total_cost_usd != '0')",
            rusqlite::params![APP_TYPE, DATA_SOURCE],
        )
        .map_err(|error| {
            AppError::Database(format!(
                "清理 VS Code Copilot 订阅流量估算成本失败: {error}"
            ))
        })?;
    transaction.commit().map_err(|error| {
        AppError::Database(format!("提交 VS Code Copilot 用量规范化失败: {error}"))
    })?;
    Ok(u32::try_from(provider_updates.saturating_add(cost_updates)).unwrap_or(u32::MAX))
}

fn session_request_id(session: &VscodeSession, request: &VscodeRequest, index: usize) -> String {
    let stable_id = if !request.request_id.is_empty() {
        request.request_id.as_str()
    } else if !request.response_id.is_empty() {
        request.response_id.as_str()
    } else {
        return format!("{REQUEST_ID_PREFIX}{}:{index}", session.session_id);
    };
    // VS Code request identifiers are only documented inside their owning chat
    // session. Include the session id so two histories using the same local
    // request id cannot overwrite each other in the global usage table.
    format!("{REQUEST_ID_PREFIX}{}:{stable_id}", session.session_id)
}

fn request_created_at(session: &VscodeSession, request: &VscodeRequest) -> i64 {
    let timestamp_ms = if request.timestamp_ms > 0 {
        request.timestamp_ms
    } else {
        session.creation_date_ms
    };
    if timestamp_ms > 0 {
        timestamp_ms / 1000
    } else {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
            .unwrap_or(0)
    }
}

fn insert_vscode_request(
    db: &Database,
    request_id: &str,
    session: &VscodeSession,
    request: &VscodeRequest,
    catalog_entry: &CatalogEntry,
) -> Result<bool, AppError> {
    sync_provider(db, catalog_entry)?;
    let conn = lock_conn!(db.conn);
    let input_tokens = request.input_tokens();
    let output_tokens = request.output_tokens();
    let created_at = request_created_at(session, request);

    let existing_source: Option<Option<String>> = conn
        .query_row(
            "SELECT data_source FROM proxy_request_logs WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| AppError::Database(format!("查询 VS Code 会话用量失败: {error}")))?;
    if existing_source
        .as_ref()
        .is_some_and(|source| source.as_deref() != Some(DATA_SOURCE))
    {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: request.cache_read_tokens,
        cache_creation_tokens: request.cache_creation_tokens,
        model: Some(catalog_entry.model_id.clone()),
        message_id: None,
    };
    let is_custom_endpoint = request.is_custom_endpoint();
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        match is_custom_endpoint
            .then(|| find_model_pricing(&conn, &catalog_entry.model_id))
            .flatten()
        {
            Some(pricing) => {
                let cost = CostCalculator::calculate(&usage, &pricing, Decimal::from(1));
                (
                    cost.input_cost.to_string(),
                    cost.output_cost.to_string(),
                    cost.cache_read_cost.to_string(),
                    cost.cache_creation_cost.to_string(),
                    cost.total_cost.to_string(),
                )
            }
            None => (
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
            ),
        };
    // Official Copilot subscription traffic has no per-request USD charge we
    // can infer from local history. Persist a zero multiplier so the generic
    // historical pricing backfill does not later turn those rows into retail
    // API cost estimates. Custom Endpoint traffic remains priceable.
    let cost_multiplier = if is_custom_endpoint { "1.0" } else { "0" };

    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source,
            input_token_semantics
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
        ON CONFLICT(request_id) DO UPDATE SET
            provider_id = excluded.provider_id,
            model = excluded.model,
            request_model = excluded.request_model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            latency_ms = excluded.latency_ms,
            status_code = excluded.status_code,
            error_message = excluded.error_message,
            session_id = excluded.session_id,
            cost_multiplier = excluded.cost_multiplier,
            created_at = excluded.created_at
        WHERE data_source = 'vscode_session'
          AND (provider_id != excluded.provider_id
           OR model != excluded.model
           OR input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR cache_creation_tokens != excluded.cache_creation_tokens
           OR status_code != excluded.status_code
           OR COALESCE(error_message, '') != COALESCE(excluded.error_message, '')
           OR request_model != excluded.request_model
           OR cost_multiplier != excluded.cost_multiplier
           OR input_cost_usd != excluded.input_cost_usd
           OR output_cost_usd != excluded.output_cost_usd
           OR cache_read_cost_usd != excluded.cache_read_cost_usd
           OR cache_creation_cost_usd != excluded.cache_creation_cost_usd
           OR total_cost_usd != excluded.total_cost_usd
           OR latency_ms != excluded.latency_ms
           OR created_at != excluded.created_at)",
        rusqlite::params![
            request_id,
            PROVIDER_ID,
            APP_TYPE,
            catalog_entry.model_id,
            request.requested_model_id(),
            input_tokens,
            output_tokens,
            i64::from(request.cache_read_tokens),
            i64::from(request.cache_creation_tokens),
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            request.elapsed_ms.min(i64::MAX as u64) as i64,
            Option::<i64>::None,
            request.status_code(),
            request.error_message.clone(),
            session.session_id,
            Some(DATA_SOURCE),
            1i64,
            cost_multiplier,
            created_at,
            DATA_SOURCE,
            INPUT_TOKEN_SEMANTICS_FRESH,
        ],
    )
    .map_err(|error| AppError::Database(format!("插入 VS Code 会话用量失败: {error}")))?;

    Ok(conn.changes() > 0)
}

fn delete_stale_session_requests(
    db: &Database,
    session_id: &str,
    keep_request_ids: &HashSet<String>,
) -> Result<u32, AppError> {
    if session_id.trim().is_empty() {
        return Ok(0);
    }

    let mut conn = lock_conn!(db.conn);
    let transaction = conn
        .transaction()
        .map_err(|error| AppError::Database(format!("开始清理 VS Code 会话旧用量失败: {error}")))?;
    let existing_request_ids = {
        let mut statement = transaction
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE app_type = ?1 AND data_source = ?2 AND session_id = ?3",
            )
            .map_err(|error| AppError::Database(format!("查询 VS Code 会话旧用量失败: {error}")))?;
        let request_ids = statement
            .query_map(
                rusqlite::params![APP_TYPE, DATA_SOURCE, session_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| AppError::Database(format!("读取 VS Code 会话旧用量失败: {error}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::Database(format!("读取 VS Code 会话旧用量失败: {error}")))?;
        request_ids
    };

    let mut deleted = 0u32;
    for request_id in existing_request_ids {
        if keep_request_ids.contains(&request_id) {
            continue;
        }
        let affected = transaction
            .execute(
                "DELETE FROM proxy_request_logs
                     WHERE request_id = ?1 AND app_type = ?2 AND data_source = ?3 AND session_id = ?4",
                rusqlite::params![request_id, APP_TYPE, DATA_SOURCE, session_id],
            )
            .map_err(|error| {
                AppError::Database(format!("清理 VS Code 会话旧用量失败: {error}"))
            })?;
        deleted = deleted.saturating_add(u32::try_from(affected).unwrap_or(u32::MAX));
    }
    transaction
        .commit()
        .map_err(|error| AppError::Database(format!("提交 VS Code 会话旧用量清理失败: {error}")))?;
    Ok(deleted)
}

fn delete_legacy_session_rollups(db: &Database) -> Result<u32, AppError> {
    // Copilot BYOK has no local-proxy data source, so every rollup under this
    // app type was produced from the VS Code session importer.
    let conn = lock_conn!(db.conn);
    let deleted = conn
        .execute(
            "DELETE FROM usage_daily_rollups WHERE app_type = ?1",
            [APP_TYPE],
        )
        .map_err(|error| {
            AppError::Database(format!("清理重复的 VS Code 会话历史汇总失败: {error}"))
        })?;
    Ok(u32::try_from(deleted).unwrap_or(u32::MAX))
}

fn sync_from_roots(
    db: &Database,
    roots: &[PathBuf],
    catalog: &ByokCatalog,
) -> Result<SessionSyncResult, AppError> {
    let (_, previous_catalog_fingerprint) = get_sync_state(db, CATALOG_SYNC_KEY)?;
    let force_catalog_replay = previous_catalog_fingerprint != catalog.fingerprint;
    let mut files = Vec::new();
    for root in roots {
        files.extend(collect_chat_session_files(root));
    }
    files.sort();
    files.dedup();

    let mut result = SessionSyncResult::default();
    for file_path in files {
        result.files_scanned = result.files_scanned.saturating_add(1);
        let metadata = match session_file_metadata(&file_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                result
                    .errors
                    .push(format!("{}: {error}", file_path.display()));
                continue;
            }
        };
        let modified = metadata_modified_nanos(&metadata);
        let sync_key = format!(
            "{DATA_SOURCE}:{SYNC_VERSION}:{}",
            file_path.to_string_lossy()
        );
        let (last_modified, _) = get_sync_state(db, &sync_key)?;
        if !force_catalog_replay && last_modified > 0 && modified <= last_modified {
            continue;
        }

        let session = match parse_session_file(&file_path) {
            Ok(session) => session,
            Err(error) => {
                result
                    .errors
                    .push(format!("{}: {error}", file_path.display()));
                continue;
            }
        };

        let mut file_failed = false;
        let mut current_request_ids = HashSet::new();
        for (index, request) in session.requests.iter().enumerate() {
            if !request.should_record() {
                continue;
            }
            let Some(catalog_entry) = catalog.resolve(request) else {
                continue;
            };
            let request_id = session_request_id(&session, request, index);
            current_request_ids.insert(request_id.clone());
            match insert_vscode_request(db, &request_id, &session, request, &catalog_entry) {
                Ok(true) => result.imported = result.imported.saturating_add(1),
                Ok(false) => result.skipped = result.skipped.saturating_add(1),
                Err(error) => {
                    result.errors.push(format!("{request_id}: {error}"));
                    file_failed = true;
                }
            }
        }

        if !file_failed {
            match delete_stale_session_requests(db, &session.session_id, &current_request_ids) {
                Ok(deleted) => {
                    result.imported = result.imported.saturating_add(deleted);
                    update_sync_state(db, &sync_key, modified, session.line_count)?;
                }
                Err(error) => result
                    .errors
                    .push(format!("{}: {error}", file_path.display())),
            }
        }
    }

    // A catalog replay is complete only when every discovered file was read and
    // reconciled successfully. Keeping the old fingerprint on any error makes
    // the next sync retry unchanged files instead of permanently retaining a
    // stale model mapping after a transient read/parse failure.
    if result.errors.is_empty() {
        if force_catalog_replay {
            let deleted = delete_legacy_session_rollups(db)?;
            if deleted > 0 {
                log::info!(
                    "[VSCODE-SESSION-SYNC] 已清理 {deleted} 条旧会话汇总；v8 明细将按稳定请求 ID 保留"
                );
            }
        }
        update_sync_state(db, CATALOG_SYNC_KEY, 0, catalog.fingerprint)?;
    }

    if result.imported > 0 {
        log::info!(
            "[VSCODE-SESSION-SYNC] 同步完成: 导入/更新 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }
    Ok(result)
}

/// 从 VS Code 的聊天会话历史导入 Copilot token 使用量。
///
/// 是否启用 CC Switch 本地代理不影响此流程；VS Code 未写入 token 的请求不会猜测补算。
pub fn sync_vscode_copilot_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let normalized = sync_copilot_provider(db)?;
    let groups = crate::copilot_byok::usage_catalog(db)?;
    let catalog = ByokCatalog::from_groups(&groups);
    let mut result = sync_from_roots(db, &default_vscode_user_dirs(), &catalog)?;
    result.imported = result.imported.saturating_add(normalized);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn group(id: &str, name: &str, model_id: &str, model_name: &str) -> CopilotByokGroup {
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "url": "https://example.com/v1/chat/completions",
            "apiKey": "test-only",
            "apiType": "chat-completions",
            "models": [{
                "id": format!("internal-{model_id}"),
                "modelId": model_id,
                "name": model_name
            }]
        }))
        .expect("test group")
    }

    fn custom_selected(model_id: &str, name: &str, provider: &str) -> Value {
        json!({
            "identifier": format!("customendpoint/{model_id}"),
            "metadata": {
                "vendor": "customendpoint",
                "name": name,
                "isBYOK": true,
                "auth": { "providerLabel": provider }
            }
        })
    }

    fn write_lines(path: &Path, lines: &[Value]) {
        let mut file = fs::File::create(path).expect("create fixture");
        for line in lines {
            writeln!(file, "{}", serde_json::to_string(line).unwrap()).unwrap();
        }
    }

    #[test]
    fn applies_snapshot_and_final_token_patches() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        write_lines(
            &path,
            &[
                json!({
                    "kind": 0,
                    "v": {
                        "sessionId": "s-1",
                        "creationDate": 1_700_000_000_000i64,
                        "inputState": { "selectedModel": custom_selected("gpt-test", "GPT Test", "Acme") },
                        "requests": [{
                            "requestId": "r-1",
                            "timestamp": 1_700_000_001_000i64,
                            "modelId": "customendpoint/gpt-test",
                            "promptTokens": 10,
                            "completionTokens": 2
                        }]
                    }
                }),
                json!({ "kind": 1, "k": ["requests", 0, "promptTokens"], "v": 40 }),
                json!({ "kind": 1, "k": ["requests", 0, "completionTokens"], "v": 9 }),
                json!({ "kind": 1, "k": ["requests", 0, "elapsedMs"], "v": 1234 }),
            ],
        );

        let session = parse_session_file(&path).unwrap();
        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.requests[0].input_tokens(), 40);
        assert_eq!(session.requests[0].output_tokens(), 9);
        assert_eq!(session.requests[0].elapsed_ms, 1234);
    }

    #[test]
    fn reads_token_usage_from_result_usage_shape() {
        let mut request = VscodeRequest::default();
        apply_result_metadata(
            &mut request,
            &json!({
                "usage": {
                    "promptTokens": 73,
                    "completionTokens": 11
                }
            }),
        );
        assert_eq!(request.input_tokens(), 73);
        assert_eq!(request.output_tokens(), 11);
    }

    #[test]
    fn appended_requests_keep_the_model_selected_at_send_time() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        write_lines(
            &path,
            &[
                json!({
                    "kind": 0,
                    "v": {
                        "sessionId": "s-2",
                        "inputState": { "selectedModel": {
                            "identifier": "copilot/auto",
                            "metadata": { "vendor": "copilot", "name": "Auto", "isBYOK": false }
                        }},
                        "requests": []
                    }
                }),
                json!({
                    "kind": 1,
                    "k": ["inputState", "selectedModel"],
                    "v": custom_selected("gpt-test", "GPT Test", "Acme")
                }),
                json!({
                    "kind": 2,
                    "k": ["requests"],
                    "v": [{
                        "requestId": "r-custom",
                        "modelId": "customendpoint/gpt-test",
                        "promptTokens": 100,
                        "completionTokens": 20
                    }]
                }),
                json!({
                    "kind": 1,
                    "k": ["inputState", "selectedModel"],
                    "v": {
                        "identifier": "copilot/gpt-test",
                        "metadata": { "vendor": "copilot", "name": "GPT Test", "isBYOK": false }
                    }
                }),
                json!({
                    "kind": 2,
                    "k": ["requests"],
                    "v": [{
                        "requestId": "r-official",
                        "modelId": "copilot/gpt-test",
                        "promptTokens": 200,
                        "completionTokens": 30
                    }]
                }),
            ],
        );

        let session = parse_session_file(&path).unwrap();
        let catalog = ByokCatalog::from_groups(&[group("g-1", "Acme", "gpt-test", "GPT Test")]);
        assert!(catalog.resolve(&session.requests[0]).is_some());
        let official = catalog
            .resolve(&session.requests[1])
            .expect("regular Copilot request");
        assert_eq!(official.group_id, PROVIDER_ID);
        assert_eq!(official.group_name, PROVIDER_NAME);
        assert_eq!(official.model_id, "copilot/gpt-test");
    }

    #[test]
    fn resolves_regular_copilot_and_unmanaged_custom_sessions_without_catalog() {
        let catalog = ByokCatalog::from_groups(&[]);
        let copilot = VscodeRequest {
            model_id: "copilot/auto".to_string(),
            selected_model: parse_selected_model(&json!({
                "identifier": "copilot/auto",
                "metadata": {
                    "vendor": "copilot",
                    "name": "Auto",
                    "isBYOK": false,
                    "auth": { "providerLabel": "GitHub Copilot" }
                }
            })),
            selected_at_send: true,
            ..VscodeRequest::default()
        };
        let copilot_entry = catalog.resolve(&copilot).expect("regular Copilot entry");
        assert_eq!(copilot_entry.group_id, PROVIDER_ID);
        assert_eq!(copilot_entry.group_name, PROVIDER_NAME);
        assert_eq!(copilot_entry.model_id, "copilot/auto");

        let copilot_after_custom_endpoint = VscodeRequest {
            model_id: "copilot/auto".to_string(),
            selected_model: parse_selected_model(&custom_selected("gpt-test", "GPT Test", "Acme")),
            selected_at_send: true,
            ..VscodeRequest::default()
        };
        assert!(!copilot_after_custom_endpoint.is_custom_endpoint());
        let stale_selection_entry = catalog
            .resolve(&copilot_after_custom_endpoint)
            .expect("qualified Copilot request wins over stale Custom Endpoint selection");
        assert_eq!(stale_selection_entry.group_id, PROVIDER_ID);
        assert_eq!(stale_selection_entry.model_id, "copilot/auto");

        let custom = VscodeRequest {
            model_id: "customendpoint/gpt-test".to_string(),
            selected_model: parse_selected_model(&custom_selected("gpt-test", "GPT Test", "Acme")),
            selected_at_send: true,
            ..VscodeRequest::default()
        };
        let custom_entry = catalog.resolve(&custom).expect("unmanaged custom entry");
        assert_eq!(custom_entry.group_name, PROVIDER_NAME);
        assert_eq!(custom_entry.model_id, "gpt-test");
    }

    #[test]
    fn normalizes_legacy_provider_and_official_cost_without_touching_custom_cost(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        assert_eq!(sync_copilot_provider(&db)?, 0);
        {
            let conn = lock_conn!(db.conn);
            // Simulate the pre-session-suffix provider row. The next sync must
            // rename it so existing detail and rollup joins display uniformly.
            conn.execute(
                "UPDATE providers SET name = ?1 WHERE id = ?2 AND app_type = ?3",
                rusqlite::params![PROVIDER_NAME, PROVIDER_ID, APP_TYPE],
            )?;
            for (request_id, request_model, multiplier, total_cost) in [
                ("official", "copilot/auto", "1.0", "0.0084"),
                (
                    "custom",
                    "customendpoint/MiniMax/MiniMax-M3",
                    "1.0",
                    "0.0110",
                ),
            ] {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                         request_id, provider_id, app_type, model, request_model,
                         input_tokens, output_tokens, input_cost_usd, output_cost_usd,
                         total_cost_usd, latency_ms, status_code, cost_multiplier,
                         created_at, data_source
                     ) VALUES (?1, 'legacy-vendor', ?2, 'resolved-model', ?3,
                               100, 10, ?4, '0', ?4, 0, 200, ?5, 1, ?6)",
                    rusqlite::params![
                        request_id,
                        APP_TYPE,
                        request_model,
                        total_cost,
                        multiplier,
                        DATA_SOURCE
                    ],
                )
                .unwrap();
            }
        }

        assert!(sync_copilot_provider(&db)? > 0);
        let conn = lock_conn!(db.conn);
        let rows = conn
            .prepare(
                "SELECT request_id, provider_id, cost_multiplier, total_cost_usd
                 FROM proxy_request_logs ORDER BY request_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "custom".to_string(),
                    PROVIDER_ID.to_string(),
                    "1.0".to_string(),
                    "0.0110".to_string(),
                ),
                (
                    "official".to_string(),
                    PROVIDER_ID.to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ),
            ]
        );
        let provider_name: String = conn.query_row(
            "SELECT name FROM providers WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![PROVIDER_ID, APP_TYPE],
            |row| row.get(0),
        )?;
        assert_eq!(provider_name, SESSION_PROVIDER_NAME);
        drop(conn);
        assert_eq!(sync_copilot_provider(&db)?, 0);
        Ok(())
    }

    #[test]
    fn provider_label_disambiguates_same_model_id() {
        let catalog = ByokCatalog::from_groups(&[
            group("g-1", "Acme", "shared", "Shared"),
            group("g-2", "Contoso", "shared", "Contoso Shared"),
        ]);
        let request = VscodeRequest {
            model_id: "customendpoint/shared".to_string(),
            selected_model: parse_selected_model(&custom_selected(
                "shared",
                "Contoso Shared",
                "Contoso",
            )),
            selected_at_send: true,
            ..VscodeRequest::default()
        };
        let resolved = catalog.resolve(&request).unwrap();
        assert_eq!(resolved.group_id, PROVIDER_ID);
        assert_eq!(resolved.model_name, "Contoso Shared");
    }

    #[test]
    fn qualified_custom_model_suffix_resolves_to_managed_provider() {
        let catalog = ByokCatalog::from_groups(&[group(
            "minimax",
            "MiniMax",
            "MiniMax-M2.7-highspeed",
            "MiniMax M2.7 Highspeed",
        )]);
        let request = VscodeRequest {
            model_id: "customendpoint/minimax/minimax-m2.7-highspeed".to_string(),
            selected_model: SelectedModel {
                identifier: "customendpoint/minimax/minimax-m2.7-highspeed".to_string(),
                vendor: "customendpoint".to_string(),
                is_byok: true,
                ..SelectedModel::default()
            },
            ..VscodeRequest::default()
        };

        let resolved = catalog.resolve(&request).expect("managed MiniMax provider");
        assert_eq!(resolved.group_id, PROVIDER_ID);
        assert_eq!(resolved.group_name, PROVIDER_NAME);
    }

    #[test]
    fn ambiguous_snapshot_uses_unified_provider_without_guessing_a_supplier() {
        let catalog = ByokCatalog::from_groups(&[
            group("g-1", "Acme", "shared", "Shared"),
            group("g-2", "Contoso", "shared", "Shared"),
        ]);
        let request = VscodeRequest {
            model_id: "customendpoint/shared".to_string(),
            selected_model: parse_selected_model(&custom_selected("shared", "Shared", "Contoso")),
            selected_at_send: false,
            ..VscodeRequest::default()
        };
        let resolved = catalog.resolve(&request).expect("unified Copilot provider");
        assert_eq!(resolved.group_id, PROVIDER_ID);
        assert_eq!(resolved.group_name, PROVIDER_NAME);
        assert_eq!(resolved.model_id, "shared");
    }

    #[test]
    fn imports_history_once_without_proxy() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let user_dir = temp.path().join("Code").join("User");
        let sessions = user_dir
            .join("workspaceStorage")
            .join("workspace")
            .join("chatSessions");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("session.jsonl");
        write_lines(
            &path,
            &[json!({
                "kind": 0,
                "v": {
                    "sessionId": "s-3",
                    "creationDate": 1_700_000_000_000i64,
                    "inputState": { "selectedModel": custom_selected("gpt-test", "GPT Test", "Acme") },
                    "requests": [{
                        "requestId": "r-import",
                        "timestamp": 1_700_000_001_000i64,
                        "modelId": "customendpoint/gpt-test",
                        "promptTokens": 321,
                        "completionTokens": 45,
                        "elapsedMs": 987
                    }]
                }
            })],
        );

        let db = Database::memory()?;
        let catalog = ByokCatalog::from_groups(&[group("g-1", "Acme", "gpt-test", "GPT Test")]);
        let first = sync_from_roots(&db, std::slice::from_ref(&user_dir), &catalog)?;
        assert_eq!(first.imported, 1);

        let second = sync_from_roots(&db, &[user_dir], &catalog)?;
        assert_eq!(second.imported, 0);

        let conn = lock_conn!(db.conn);
        let row: (String, String, i64, i64, String) = conn.query_row(
            "SELECT app_type, provider_id, input_tokens, output_tokens, data_source
             FROM proxy_request_logs WHERE request_id = 'vscode_session:s-3:r-import'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        assert_eq!(
            row,
            (
                APP_TYPE.to_string(),
                PROVIDER_ID.to_string(),
                321,
                45,
                DATA_SOURCE.to_string()
            )
        );
        drop(conn);
        let logs = db.get_request_logs(&Default::default(), 0, 10)?;
        assert_eq!(logs.data.len(), 1);
        assert_eq!(logs.data[0].model_display_name.as_deref(), Some("GPT Test"));
        Ok(())
    }

    #[test]
    fn successful_catalog_replay_replaces_legacy_session_rollups() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let user_dir = temp.path().join("Code").join("User");
        let sessions = user_dir
            .join("workspaceStorage")
            .join("workspace")
            .join("chatSessions");
        fs::create_dir_all(&sessions).unwrap();
        write_lines(
            &sessions.join("session.jsonl"),
            &[json!({
                "kind": 0,
                "v": {
                    "sessionId": "s-rollup",
                    "creationDate": 1_700_000_000_000i64,
                    "inputState": { "selectedModel": custom_selected("gpt-test", "GPT Test", "Acme") },
                    "requests": [{
                        "requestId": "r-rollup",
                        "timestamp": 1_700_000_001_000i64,
                        "modelId": "customendpoint/gpt-test",
                        "promptTokens": 100,
                        "completionTokens": 10
                    }]
                }
            })],
        );

        let db = Database::memory()?;
        let initial_catalog =
            ByokCatalog::from_groups(&[group("g-1", "Acme", "gpt-test", "GPT Test")]);
        sync_from_roots(&db, std::slice::from_ref(&user_dir), &initial_catalog)?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_model, pricing_model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (
                    '2023-11-14', ?1, ?2, 'gpt-test', 'customendpoint/gpt-test',
                    'gpt-test', 4, 4, 400, 40, 160, 0, '0', 0
                )",
                rusqlite::params![APP_TYPE, PROVIDER_ID],
            )?;
        }

        let replay_catalog = ByokCatalog::from_groups(&[
            group("g-1", "Acme", "gpt-test", "GPT Test"),
            group("g-2", "Other", "other-model", "Other model"),
        ]);
        sync_from_roots(&db, &[user_dir], &replay_catalog)?;

        let conn = lock_conn!(db.conn);
        let rollups: i64 = conn.query_row(
            "SELECT COUNT(*) FROM usage_daily_rollups WHERE app_type = ?1",
            [APP_TYPE],
            |row| row.get(0),
        )?;
        let details: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE request_id = 'vscode_session:s-rollup:r-rollup'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(rollups, 0);
        assert_eq!(details, 1);
        Ok(())
    }

    #[test]
    fn imports_regular_copilot_history_without_byok_catalog() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let user_dir = temp.path().join("Code").join("User");
        let sessions = user_dir
            .join("workspaceStorage")
            .join("workspace")
            .join("chatSessions");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("session.jsonl");
        write_lines(
            &path,
            &[
                json!({
                    "kind": 0,
                    "v": {
                        "sessionId": "s-copilot",
                        "creationDate": 1_700_000_000_000i64,
                        "inputState": { "selectedModel": {
                            "identifier": "copilot/auto",
                            "metadata": {
                                "vendor": "copilot",
                                "name": "Auto",
                                "isBYOK": false,
                                "auth": { "providerLabel": "GitHub Copilot" }
                            }
                        }},
                        "requests": [{
                            "requestId": "r-copilot",
                            "timestamp": 1_700_000_001_000i64,
                            "modelId": "copilot/auto"
                        }]
                    }
                }),
                json!({
                    "kind": 1,
                    "k": ["requests", 0, "result"],
                    "v": { "metadata": {
                        "promptTokens": 29148,
                        "outputTokens": 397,
                        "resolvedModel": "gpt-5-mini",
                        "resolvedModelName": "GPT-5 mini"
                    } }
                }),
            ],
        );

        let db = Database::memory()?;
        let catalog = ByokCatalog::from_groups(&[]);
        let result = sync_from_roots(&db, std::slice::from_ref(&user_dir), &catalog)?;
        assert_eq!(result.imported, 1);

        let conn = lock_conn!(db.conn);
        let row: (String, String, i64, i64, String, String) = conn.query_row(
            "SELECT provider_id, model, input_tokens, output_tokens,
                    cost_multiplier, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'vscode_session:s-copilot:r-copilot'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        let provider_name: String = conn.query_row(
            "SELECT name FROM providers WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![row.0, APP_TYPE],
            |provider_row| provider_row.get(0),
        )?;
        assert_eq!(row.1, "gpt-5-mini");
        assert_eq!(row.2, 29148);
        assert_eq!(row.3, 397);
        assert_eq!(row.0, PROVIDER_ID);
        assert_eq!(row.4, "0");
        assert_eq!(row.5, "0");
        assert_eq!(provider_name, SESSION_PROVIDER_NAME);
        let settings_config: String = conn.query_row(
            "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![PROVIDER_ID, APP_TYPE],
            |provider_row| provider_row.get(0),
        )?;
        let settings_config: Value = serde_json::from_str(&settings_config).unwrap();
        assert_eq!(
            settings_config.pointer("/modelNames/gpt-5-mini"),
            Some(&json!("GPT-5 mini"))
        );

        // v5 could let the generic pricing backfill assign retail API cost to
        // official Copilot subscription traffic. Simulate that persisted state
        // and prove the v7 cursor namespace forces a one-time corrective replay
        // even when the JSONL file itself has not changed.
        drop(conn);
        let logs = db.get_request_logs(&Default::default(), 0, 10)?;
        assert_eq!(logs.data.len(), 1);
        assert_eq!(
            logs.data[0].model_display_name.as_deref(),
            Some("GPT-5 mini")
        );
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE proxy_request_logs
                 SET cost_multiplier = '1.0',
                     input_cost_usd = '0.0075',
                     output_cost_usd = '0.0009',
                     total_cost_usd = '0.0084'
                 WHERE request_id = 'vscode_session:s-copilot:r-copilot'",
                [],
            )?;
            conn.execute("DELETE FROM session_log_sync", [])?;
        }
        let metadata = session_file_metadata(&path)?;
        let old_sync_key = format!("{DATA_SOURCE}:v5:{}", path.to_string_lossy());
        update_sync_state(&db, &old_sync_key, metadata_modified_nanos(&metadata), 2)?;
        update_sync_state(&db, "vscode_session:v5:catalog", 0, catalog.fingerprint)?;

        let replay = sync_from_roots(&db, &[user_dir], &catalog)?;
        assert_eq!(replay.imported, 1);
        let conn = lock_conn!(db.conn);
        let corrected_costs: (String, String, String, String) = conn.query_row(
            "SELECT cost_multiplier, input_cost_usd, output_cost_usd, total_cost_usd
             FROM proxy_request_logs
             WHERE request_id = 'vscode_session:s-copilot:r-copilot'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            corrected_costs,
            (
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string()
            )
        );
        Ok(())
    }

    #[test]
    fn removes_rows_deleted_by_later_jsonl_patches() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let user_dir = temp.path().join("Code").join("User");
        let sessions = user_dir
            .join("workspaceStorage")
            .join("workspace")
            .join("chatSessions");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("session.jsonl");
        write_lines(
            &path,
            &[json!({
                "kind": 0,
                "v": {
                    "sessionId": "s-cleanup",
                    "creationDate": 1_700_000_000_000i64,
                    "inputState": { "selectedModel": {
                        "identifier": "copilot/gpt-5-mini",
                        "metadata": { "vendor": "copilot", "name": "GPT-5 mini" }
                    }},
                    "requests": [
                        {
                            "requestId": "r-deleted",
                            "modelId": "copilot/gpt-5-mini",
                            "promptTokens": 10,
                            "completionTokens": 2
                        },
                        {
                            "requestId": "r-kept",
                            "modelId": "copilot/gpt-5-mini",
                            "promptTokens": 20,
                            "completionTokens": 4
                        }
                    ]
                }
            })],
        );

        let db = Database::memory()?;
        let initial_catalog = ByokCatalog::from_groups(&[]);
        assert_eq!(
            sync_from_roots(&db, std::slice::from_ref(&user_dir), &initial_catalog)?.imported,
            2
        );

        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::to_string(&json!({"kind": 3, "k": ["requests", 0]})).unwrap()
        )
        .unwrap();
        file.sync_all().unwrap();

        // A changed catalog fingerprint forces a replay independently of the
        // host file system's timestamp resolution.
        let replay_catalog =
            ByokCatalog::from_groups(&[group("unused", "Unused", "unused-model", "Unused model")]);
        assert_eq!(
            sync_from_roots(&db, &[user_dir], &replay_catalog)?.imported,
            1
        );

        let conn = lock_conn!(db.conn);
        let rows: Vec<String> = {
            let mut statement = conn.prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE app_type = ?1 AND data_source = ?2 AND session_id = 's-cleanup'
                 ORDER BY request_id",
            )?;
            let request_ids = statement
                .query_map(rusqlite::params![APP_TYPE, DATA_SOURCE], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            request_ids
        };
        assert_eq!(rows, vec!["vscode_session:s-cleanup:r-kept"]);
        Ok(())
    }

    #[test]
    fn request_ids_are_namespaced_by_chat_session() {
        let request = VscodeRequest {
            request_id: "request-1".to_string(),
            ..VscodeRequest::default()
        };
        let first = VscodeSession {
            session_id: "session-a".to_string(),
            ..VscodeSession::default()
        };
        let second = VscodeSession {
            session_id: "session-b".to_string(),
            ..VscodeSession::default()
        };

        assert_ne!(
            session_request_id(&first, &request, 0),
            session_request_id(&second, &request, 0)
        );
    }

    #[test]
    fn push_with_index_replaces_the_regenerated_request_tail() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        write_lines(
            &path,
            &[
                json!({
                    "kind": 0,
                    "v": {
                        "sessionId": "s-replay",
                        "requests": [
                            {"requestId": "r-0", "modelId": "copilot/auto"},
                            {"requestId": "r-old", "modelId": "copilot/auto"}
                        ]
                    }
                }),
                json!({
                    "kind": 2,
                    "k": ["requests"],
                    "i": 1,
                    "v": [{"requestId": "r-new", "modelId": "copilot/auto"}]
                }),
            ],
        );

        let session = parse_session_file(&path).unwrap();
        assert_eq!(session.requests.len(), 2);
        assert_eq!(session.requests[0].request_id, "r-0");
        assert_eq!(session.requests[1].request_id, "r-new");
    }

    #[test]
    fn push_without_value_truncates_and_delete_removes_requests() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        write_lines(
            &path,
            &[
                json!({
                    "kind": 0,
                    "v": {
                        "sessionId": "s-delete",
                        "requests": [
                            {"requestId": "r-0"},
                            {"requestId": "r-1"},
                            {"requestId": "r-2"}
                        ]
                    }
                }),
                json!({"kind": 2, "k": ["requests"], "i": 2}),
                json!({"kind": 3, "k": ["requests", 0]}),
            ],
        );

        let session = parse_session_file(&path).unwrap();
        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.requests[0].request_id, "r-1");
    }

    #[test]
    fn malformed_jsonl_keeps_catalog_replay_pending() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let user_dir = temp.path().join("Code").join("User");
        let sessions = user_dir
            .join("workspaceStorage")
            .join("workspace")
            .join("chatSessions");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("session.jsonl");
        write_lines(
            &path,
            &[json!({
                "kind": 0,
                "v": {"sessionId": "s-retry", "requests": []}
            })],
        );

        let db = Database::memory()?;
        let initial_catalog = ByokCatalog::from_groups(&[]);
        let initial = sync_from_roots(&db, std::slice::from_ref(&user_dir), &initial_catalog)?;
        assert!(initial.errors.is_empty());
        let (_, initial_fingerprint) = get_sync_state(&db, CATALOG_SYNC_KEY)?;
        assert_eq!(initial_fingerprint, initial_catalog.fingerprint);

        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{{truncated").unwrap();
        file.sync_all().unwrap();

        let changed_catalog =
            ByokCatalog::from_groups(&[group("changed", "Changed", "model", "Model")]);
        assert_ne!(changed_catalog.fingerprint, initial_catalog.fingerprint);
        let retry = sync_from_roots(&db, &[user_dir], &changed_catalog)?;
        assert_eq!(retry.errors.len(), 1);
        let (_, retained_fingerprint) = get_sync_state(&db, CATALOG_SYNC_KEY)?;
        assert_eq!(retained_fingerprint, initial_catalog.fingerprint);
        Ok(())
    }

    #[test]
    fn resolves_auto_model_and_separates_cached_input_tokens() {
        let mut request = VscodeRequest {
            model_id: "copilot/auto".to_string(),
            selected_model: parse_selected_model(&json!({
                "identifier": "copilot/auto",
                "metadata": {"vendor": "copilot", "name": "Auto"}
            })),
            selected_at_send: true,
            ..VscodeRequest::default()
        };
        apply_result_metadata(
            &mut request,
            &json!({
                "metadata": {
                    "promptTokens": 100,
                    "outputTokens": 20,
                    "resolvedModel": "gpt-5-mini",
                    "resolvedModelName": "GPT-5 mini"
                },
                "usage": {
                    "prompt_tokens_details": {"cached_tokens": 60}
                }
            }),
        );

        assert_eq!(request.input_tokens(), 40);
        assert_eq!(request.cache_read_tokens, 60);
        let entry = session_catalog_entry(&request).expect("Copilot Auto entry");
        assert_eq!(entry.model_id, "gpt-5-mini");
        assert_eq!(entry.model_name, "GPT-5 mini");
        assert_eq!(entry.group_name, PROVIDER_NAME);
    }

    #[test]
    fn result_usage_is_not_masked_by_zero_top_level_counters() {
        let request = VscodeRequest {
            prompt_tokens: Some(0),
            completion_tokens: Some(0),
            result_prompt_tokens: Some(100),
            result_output_tokens: Some(20),
            ..VscodeRequest::default()
        };

        assert_eq!(request.raw_input_tokens(), 100);
        assert_eq!(request.output_tokens(), 20);
    }

    #[test]
    fn zero_result_metadata_does_not_mask_nonzero_usage_fields() {
        let mut request = VscodeRequest::default();
        apply_result_metadata(
            &mut request,
            &json!({
                "metadata": {"promptTokens": 0, "outputTokens": 0},
                "usage": {
                    "inputTokens": 100,
                    "completionTokens": 20,
                    "prompt_tokens_details": {"cached_tokens": 0},
                    "promptTokensDetails": {"cachedTokens": 60}
                }
            }),
        );

        assert_eq!(request.input_tokens(), 40);
        assert_eq!(request.output_tokens(), 20);
        assert_eq!(request.cache_read_tokens, 60);
    }

    #[test]
    fn nested_auto_resolution_patch_preserves_existing_usage() {
        let mut session = VscodeSession {
            requests: vec![VscodeRequest {
                model_id: "copilot/auto".to_string(),
                result_prompt_tokens: Some(100),
                result_output_tokens: Some(20),
                cache_read_tokens: 60,
                ..VscodeRequest::default()
            }],
            ..VscodeSession::default()
        };

        apply_set_patch(
            &mut session,
            &[
                json!("requests"),
                json!(0),
                json!("result"),
                json!("metadata"),
                json!("resolvedModel"),
            ],
            &json!("gpt-5-mini"),
        );
        assert_eq!(session.requests[0].input_tokens(), 40);
        assert_eq!(session.requests[0].output_tokens(), 20);
        assert_eq!(session.requests[0].cache_read_tokens, 60);

        apply_set_patch(
            &mut session,
            &[json!("requests"), json!(0), json!("result")],
            &json!({"metadata": {"promptTokens": 120, "outputTokens": 25}}),
        );

        let request = &session.requests[0];
        assert_eq!(request.resolved_model, "gpt-5-mini");
        assert_eq!(request.input_tokens(), 120);
        assert_eq!(request.output_tokens(), 25);
        assert_eq!(request.cache_read_tokens, 0);
        assert_eq!(
            session_catalog_entry(request)
                .expect("resolved Auto model")
                .model_name,
            "gpt-5-mini"
        );
    }

    #[test]
    fn excludes_other_language_model_extensions_and_incomplete_requests() {
        let unrelated = VscodeRequest {
            model_id: "other-extension/model".to_string(),
            selected_model: SelectedModel {
                identifier: "other-extension/model".to_string(),
                vendor: "other-extension".to_string(),
                ..SelectedModel::default()
            },
            selected_at_send: true,
            prompt_tokens: Some(10),
            ..VscodeRequest::default()
        };
        assert!(session_catalog_entry(&unrelated).is_none());

        let unrelated_with_stale_copilot_selection = VscodeRequest {
            model_id: "ollama/llama3".to_string(),
            selected_model: parse_selected_model(&json!({
                "identifier": "copilot/auto",
                "metadata": {
                    "vendor": "copilot",
                    "name": "Auto",
                    "auth": {"providerLabel": "GitHub Copilot"}
                }
            })),
            selected_at_send: false,
            prompt_tokens: Some(10),
            ..VscodeRequest::default()
        };
        assert!(session_catalog_entry(&unrelated_with_stale_copilot_selection).is_none());

        let incomplete = VscodeRequest {
            request_id: "pending".to_string(),
            model_id: "copilot/auto".to_string(),
            model_state: Some(4),
            prompt_tokens: Some(10),
            ..VscodeRequest::default()
        };
        assert!(!incomplete.should_record());
    }

    #[test]
    fn ignores_sessions_without_recorded_tokens() {
        let request = VscodeRequest {
            model_id: "customendpoint/gpt-test".to_string(),
            selected_model: parse_selected_model(&custom_selected("gpt-test", "GPT Test", "Acme")),
            ..VscodeRequest::default()
        };
        assert!(!request.has_usage());
    }
}

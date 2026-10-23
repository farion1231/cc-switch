//! Exact counters exported by agy; absent models remain unknown (never use today's provider).
//! Amounts are the matching Gemini API prices, like every other session-log import.
use super::{session_usage::SessionSyncResult, sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH};
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::{
    database::{lock_conn, Database},
    error::AppError,
    session_manager::providers::agy,
};
use rusqlite::params;
use serde_json::Value;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
struct SubagentDescriptor {
    role: String,
    type_name: String,
}

impl SubagentDescriptor {
    fn from_metadata(metadata: &Value) -> Option<Self> {
        let descriptor = &metadata["subagentDescriptor"];
        Some(Self {
            role: descriptor["role"].as_str()?.to_string(),
            type_name: descriptor["typeName"].as_str()?.to_string(),
        })
    }

    fn matches(&self, argument: &Value) -> bool {
        argument["Role"].as_str() == Some(self.role.as_str())
            && argument["TypeName"].as_str() == Some(self.type_name.as_str())
    }
}

#[derive(Debug, Clone)]
struct SubagentChild {
    session_id: String,
    spawn_step: Option<u64>,
    descriptor: Option<SubagentDescriptor>,
}

#[derive(Default)]
struct SubagentGraph {
    children: HashMap<String, Vec<SubagentChild>>,
    incoming: HashMap<String, usize>,
    blocked: HashSet<String>,
    incomplete_roots: HashSet<PathBuf>,
    errors: Vec<String>,
}

fn session_directory(transcript: &Path) -> Option<&Path> {
    transcript.parent()?.parent()?.parent()
}

impl SubagentGraph {
    fn from_sessions(sessions: &BTreeMap<String, PathBuf>) -> Self {
        let mut graph = Self::default();
        for (parent, transcript) in sessions {
            let Some(parent_dir) = session_directory(transcript) else {
                continue;
            };
            let directory = parent_dir.join(".system_generated/subagents");
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    graph.directory_error(parent_dir, &directory, &error);
                    continue;
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        graph.directory_error(parent_dir, &directory, &error);
                        continue;
                    }
                };
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let Some(child) = path.file_stem().and_then(|id| id.to_str()) else {
                    continue;
                };
                let Some(child_path) = sessions.get(child) else {
                    continue;
                };
                // Never join sessions from different brain roots, including
                // ancestors recovered before the orphan fallback.
                if session_directory(child_path).and_then(Path::parent) != parent_dir.parent() {
                    continue;
                }
                let metadata = fs::read_to_string(&path)
                    .map_err(|error| error.to_string())
                    .and_then(|text| {
                        serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())
                    })
                    .and_then(|metadata| {
                        if metadata.is_object()
                            && metadata
                                .get("conversationId")
                                .is_none_or(|id| id.as_str() == Some(child))
                        {
                            Ok(metadata)
                        } else {
                            Err("Invalid subagent metadata".to_owned())
                        }
                    })
                    .unwrap_or_else(|error| {
                        // The filename still identifies a possible child, but
                        // unreadable metadata cannot supply its model. Keep the
                        // edge and defer this branch instead of inventing an orphan.
                        graph.blocked.insert(child.to_owned());
                        graph
                            .errors
                            .push(format!("Antigravity {}: {error}", path.display()));
                        Value::Null
                    });
                graph
                    .children
                    .entry(parent.clone())
                    .or_default()
                    .push(SubagentChild {
                        session_id: child.to_owned(),
                        spawn_step: metadata["spawnStepIndex"].as_u64(),
                        descriptor: SubagentDescriptor::from_metadata(&metadata),
                    });
                *graph.incoming.entry(child.to_owned()).or_default() += 1;
            }
        }
        for children in graph.children.values_mut() {
            children.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        }
        graph
    }

    fn directory_error(&mut self, parent_dir: &Path, path: &Path, error: &std::io::Error) {
        // With an incomplete directory listing we cannot identify the affected
        // children. Defer this brain root until the next sync can rebuild it.
        if let Some(root) = parent_dir.parent() {
            self.incomplete_roots.insert(root.to_owned());
        }
        self.errors
            .push(format!("Antigravity {}: {error}", path.display()));
    }

    fn recover_orphan_parents(sessions: &mut BTreeMap<String, PathBuf>) -> Self {
        let graph = Self::from_sessions(sessions);
        if !sessions.keys().any(|id| !graph.incoming.contains_key(id)) {
            return graph;
        }
        // The UI scanner can omit a parent whose transcript cannot be read.
        // Rebuild once from directory identities before importing any apparent
        // orphan; rebuilding from the filtered scanner list would lose it again.
        let roots: BTreeSet<_> = sessions
            .values()
            .filter_map(|path| session_directory(path)?.parent().map(Path::to_owned))
            .collect();
        let mut candidates = sessions.clone();
        let mut discovery_errors = Vec::new();
        let mut incomplete_roots = HashSet::new();
        for root in roots {
            let discovered = (|| -> std::io::Result<()> {
                let canonical_root = root.canonicalize()?;
                for entry in fs::read_dir(&root)? {
                    let entry = entry?;
                    if !entry.file_type()?.is_dir() {
                        continue;
                    }
                    let Some(id) = entry
                        .file_name()
                        .to_str()
                        .filter(|id| agy::valid_session_id(id))
                        .map(str::to_owned)
                    else {
                        continue;
                    };
                    let path = entry
                        .path()
                        .join(".system_generated/logs/transcript_full.jsonl");
                    // Preserve the scanner's selected copy of each session id.
                    // Missing transcripts remain candidates: their metadata can
                    // still prove that a child must wait for its parent.
                    if path.exists() && !path.canonicalize()?.starts_with(&canonical_root) {
                        continue;
                    }
                    candidates.entry(id).or_insert(path);
                }
                Ok(())
            })();
            if let Err(error) = discovered {
                discovery_errors.push(format!("Antigravity {}: {error}", root.display()));
                incomplete_roots.insert(root);
            }
        }
        let mut rebuilt = Self::from_sessions(&candidates);
        let mut parents: HashMap<&str, Vec<&str>> = HashMap::new();
        for (parent, children) in &rebuilt.children {
            for child in children {
                parents.entry(&child.session_id).or_default().push(parent);
            }
        }
        let mut needed: HashSet<String> = sessions.keys().cloned().collect();
        let mut pending: Vec<_> = needed.iter().cloned().collect();
        while let Some(child) = pending.pop() {
            for parent in parents.get(child.as_str()).into_iter().flatten() {
                if needed.insert((*parent).to_owned()) {
                    pending.push((*parent).to_owned());
                }
            }
        }
        candidates.retain(|id, _| needed.contains(id));
        rebuilt.children.retain(|parent, children| {
            children.retain(|child| needed.contains(&child.session_id));
            needed.contains(parent)
        });
        rebuilt.incoming.retain(|id, _| needed.contains(id));
        rebuilt.blocked.retain(|id| needed.contains(id));
        rebuilt.incomplete_roots.extend(incomplete_roots);
        rebuilt.errors.extend(discovery_errors);
        *sessions = candidates;
        rebuilt
    }
}

pub fn sync_agy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let sessions = agy::scan_sessions()
        .into_iter()
        .filter_map(|session| Some((session.session_id, PathBuf::from(session.source_path?))))
        .collect();
    Ok(sync_session_paths(db, sessions, agy::usage_records))
}

fn sync_session_paths(
    db: &Database,
    mut sessions: BTreeMap<String, PathBuf>,
    read_records: impl FnMut(&Path) -> Result<Vec<Value>, String>,
) -> SessionSyncResult {
    let graph = SubagentGraph::recover_orphan_parents(&mut sessions);
    let mut sync = AgyUsageSync {
        db,
        sessions: &sessions,
        graph: &graph,
        visited: HashSet::new(),
        read_records,
        result: SessionSyncResult {
            errors: graph.errors.clone(),
            ..Default::default()
        },
    };
    // Block before traversal, so neither an alternate root nor the fallback
    // pass can enter a branch whose parent evidence is unavailable.
    for session in sessions.keys().filter(|id| {
        graph.blocked.contains(*id)
            || session_directory(&sessions[*id])
                .and_then(Path::parent)
                .is_some_and(|root| graph.incomplete_roots.contains(root))
    }) {
        sync.visited.insert(session.clone());
        sync.skip_descendants(session);
    }
    // Start trees and ambiguous-parent nodes before their descendants.
    for session in sessions
        .keys()
        .filter(|id| graph.incoming.get(*id).copied().unwrap_or(0) != 1)
    {
        sync.visit_session(session, None);
    }
    // Import any remaining unvisited components, including rootless cycles, with
    // no inherited model. Their first session can be unknown if its transcript
    // also has no model evidence. Traversal from a known root instead propagates
    // that model downward; stopping a repeated edge does not clear it.
    for session in sessions.keys() {
        sync.visit_session(session, None);
    }
    sync.result
}

struct AgyUsageSync<'a, F> {
    db: &'a Database,
    sessions: &'a BTreeMap<String, PathBuf>,
    graph: &'a SubagentGraph,
    visited: HashSet<String>,
    read_records: F,
    result: SessionSyncResult,
}

impl<F: FnMut(&Path) -> Result<Vec<Value>, String>> AgyUsageSync<'_, F> {
    fn visit_session(&mut self, session: &str, initial_model: Option<&str>) {
        // Mark before recursion: a repeated edge stops this branch without
        // rereading, reimporting, or changing the session's resolved model.
        // Models flow only from parent to child; do not infer a parent's model
        // backward from the model it explicitly selected for a child.
        if !self.visited.insert(session.to_owned()) {
            return;
        }
        let Some(path) = self.sessions.get(session) else {
            return;
        };
        self.result.files_scanned += 1;
        let children = self
            .graph
            .children
            .get(session)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let resolved: Vec<_> = match (self.read_records)(path) {
            Ok(rows) => {
                let timeline = model_timeline(&rows, initial_model);
                match import_records_with_models(self.db, session, &rows, &timeline) {
                    Ok((imported, skipped)) => {
                        self.result.imported += imported;
                        self.result.skipped += skipped;
                    }
                    Err(error) => self
                        .result
                        .errors
                        .push(format!("Antigravity {session}: {error}")),
                }
                children
                    .iter()
                    .map(|child| (child, resolve_child_model(&rows, child, &timeline)))
                    .collect()
            }
            Err(error) => {
                self.result
                    .errors
                    .push(format!("Antigravity {session}: {error}"));
                // A failed read or JSON parse provides incomplete model evidence.
                // Stop its subtree, including the later fallback pass, rather than
                // overwriting retained descendant usage with unknown models.
                self.skip_descendants(session);
                return;
            }
        };
        // Drop the parent's transcript before loading any child transcript.
        for (child, model) in resolved {
            if self.graph.incoming.get(&child.session_id) == Some(&1) {
                self.visit_session(&child.session_id, model.as_deref());
            }
        }
    }

    fn skip_descendants(&mut self, session: &str) {
        let mut pending = vec![session.to_owned()];
        while let Some(parent) = pending.pop() {
            for child in self.graph.children.get(&parent).into_iter().flatten() {
                // Match recursive traversal: ambiguous-parent nodes are
                // processed independently and do not inherit from this parent.
                if self.graph.incoming.get(&child.session_id) == Some(&1)
                    && self.visited.insert(child.session_id.clone())
                {
                    pending.push(child.session_id.clone());
                }
            }
        }
    }
}

struct SubagentCreationResult {
    step: u64,
    ids: Vec<String>,
}

/// 规范化 invoke_subagent 中显式指定的模型简写。
fn normalize_explicit_subagent_model(model: &str) -> String {
    let m = model.trim();
    let low = m.to_ascii_lowercase();
    match low.as_str() {
        "flash_lite" | "flash-lite" | "gemini-flash-lite" => {
            "Gemini 3.1 Flash-Lite (High)".to_string()
        }
        "flash" | "gemini-flash" => "Gemini 3.8 Flash (High)".to_string(),
        "pro" | "gemini-pro" => "Gemini 3.1 Pro (High)".to_string(),
        _ => m.to_string(),
    }
}

/// 只解析原生创建结果中的 JSON，避免把消息正文里提到的 UUID 当成创建事件。
fn subagent_creation_result(row: &Value) -> Option<SubagentCreationResult> {
    if row["type"] != "GENERIC" || row["status"] != "DONE" {
        return None;
    }
    let content = row["content"].as_str()?;
    let (prefix, mut rest) = content
        .split_once("Created the following subagents:")
        .or_else(|| content.split_once("Created the following subagent:"))?;
    let header: Vec<_> = prefix.lines().collect();
    if !prefix.is_empty()
        && (!prefix.ends_with('\n')
            || header.len() != 2
            || !header[0].starts_with("Created At: ")
            || !header[1].starts_with("Completed At: "))
    {
        return None;
    }
    let mut ids: Vec<String> = Vec::new();
    loop {
        rest = rest.trim_start();
        if !rest.starts_with('{') && !rest.starts_with('[') {
            break;
        }
        let mut values = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
        let value = values.next()?.ok()?;
        rest = &rest[values.byte_offset()..];
        let objects = match &value {
            Value::Object(_) => std::slice::from_ref(&value),
            Value::Array(objects) => objects.as_slice(),
            _ => return None,
        };
        for object in objects {
            let id = object["conversationId"].as_str()?;
            if id.is_empty() || ids.iter().any(|known| known == id) {
                return None;
            }
            ids.push(id.to_string());
        }
    }
    if ids.is_empty() {
        return None;
    }
    Some(SubagentCreationResult {
        step: row["step_index"].as_u64()?,
        ids,
    })
}

/// 多次工具调用可以共用 spawnStepIndex；先看提示步，再按完整 UUID 搜索其他结果。
fn find_subagent_creation_result(
    rows: &[Value],
    session: &str,
    spawn_hint: Option<u64>,
) -> Option<SubagentCreationResult> {
    let hinted = rows
        .iter()
        .filter(|row| row["step_index"].as_u64() == spawn_hint);
    let others = rows
        .iter()
        .filter(|row| row["step_index"].as_u64() != spawn_hint);
    let mut found = None;
    for row in hinted.chain(others) {
        let Some(result) = subagent_creation_result(row) else {
            continue;
        };
        if result.ids.iter().any(|id| id == session) {
            if found.is_some() {
                return None;
            }
            found = Some(result);
        }
    }
    found
}

fn subagent_arguments(call: &Value) -> Option<Vec<&Value>> {
    let args = call.get("args").filter(|args| args.is_object())?;
    let arguments = match args.get("Subagents") {
        Some(value) => value.as_array()?.iter().collect::<Vec<_>>(),
        None => vec![args],
    };
    if arguments.is_empty() || arguments.iter().any(|arg| !arg.is_object()) {
        return None;
    }
    Some(arguments)
}

/// Match native creation evidence, then pass the resolved model down to the child.
fn resolve_child_model(
    rows: &[Value],
    child: &SubagentChild,
    timeline: &HashMap<u64, String>,
) -> Option<String> {
    let session = &child.session_id;
    let result = find_subagent_creation_result(rows, session, child.spawn_step)?;
    let call_row = rows
        .iter()
        .filter(|row| row["type"] == "PLANNER_RESPONSE")
        .filter(|row| {
            row["step_index"]
                .as_u64()
                .is_some_and(|step| step < result.step)
                && row["tool_calls"]
                    .as_array()
                    .is_some_and(|calls| calls.iter().any(|call| call["name"] == "invoke_subagent"))
        })
        .max_by_key(|row| row["step_index"].as_u64())?;
    let calls: Vec<_> = call_row["tool_calls"]
        .as_array()?
        .iter()
        .filter(|call| call["name"] == "invoke_subagent")
        .collect();
    let arguments = calls
        .iter()
        .map(|call| subagent_arguments(call))
        .collect::<Vec<_>>();
    // 保留无效调用的槽位，不能压缩后继续按序匹配；后备筛选仍可使用有效参数。
    let mut candidates: Vec<_> = arguments.iter().flatten().flatten().copied().collect();

    // 引擎保证同一轮独立 invoke_subagent 的创建结果按 tool_calls 顺序落盘，
    // 单次批量调用的 UUID 也按 Subagents 顺序返回。这里按 step_index 排序，
    // 不使用所有子代理可能共用的 spawnStepIndex，也不依赖 Role/TypeName 唯一。
    // 仅在每个调用都有完整创建结果时按序对应，避免缺失或失败的调用使后续错位。
    let call_step = call_row["step_index"].as_u64()?;
    let next_call_step = rows
        .iter()
        .filter(|row| {
            row["type"] == "PLANNER_RESPONSE"
                && row["tool_calls"]
                    .as_array()
                    .is_some_and(|calls| calls.iter().any(|call| call["name"] == "invoke_subagent"))
        })
        .filter_map(|row| row["step_index"].as_u64())
        .filter(|step| *step > call_step)
        .min();
    let mut results: Vec<_> = rows
        .iter()
        .filter_map(subagent_creation_result)
        .filter(|created| {
            created.step > call_step && next_call_step.is_none_or(|end| created.step < end)
        })
        .collect();
    results.sort_by_key(|created| created.step);
    if results.len() == calls.len()
        && results.iter().zip(&arguments).all(|(created, args)| {
            args.as_ref()
                .is_some_and(|args| created.ids.len() == args.len())
        })
    {
        let call_index = results
            .iter()
            .position(|created| created.step == result.step)?;
        let sub_index = result.ids.iter().position(|id| id == session)?;
        candidates = vec![arguments[call_index].as_ref()?[sub_index]];
    }
    // 先读匹配调用的 Model：显式值不依赖祖先模型；省略或空字符串沿用默认 inherit。
    // inherit 直接读取父代理在真实创建步的模型状态，不再向上查找祖先。
    let models = candidates
        .into_iter()
        .filter(|argument| {
            child
                .descriptor
                .as_ref()
                .is_none_or(|descriptor| descriptor.matches(argument))
        })
        .map(|argument| match argument.get("Model") {
            None => Some(None),
            Some(Value::String(model))
                if model.trim().is_empty() || model.trim().eq_ignore_ascii_case("inherit") =>
            {
                Some(None)
            }
            Some(Value::String(model)) => Some(Some(normalize_explicit_subagent_model(model))),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let inherited_model = timeline.get(&result.step);
    let mut agreed_model: Option<String> = None;
    for model in models {
        let model = model.or_else(|| inherited_model.cloned())?;
        if let Some(agreed) = &agreed_model {
            if model_id_of(agreed) != model_id_of(&model) {
                return None;
            }
        } else {
            agreed_model = Some(model);
        }
    }
    agreed_model
}

/// agy 把模型切换写进会话正文：`changed setting \`Model Selection\` from X to Y`。
/// 逐条响应本身没有模型字段，按 step 顺序继承最近一次切换即可还原当时的模型。
fn model_from_settings_change(content: &str) -> Option<String> {
    let start = content.find("Model Selection")?;
    let rest = &content[start..];
    let to = rest.find(" to ")? + 4;
    let tail = &rest[to..];
    // 模型名里本身有点（Gemini 3.8），只把「句号 + 空格」当成句子结束。
    let end = tail
        .find(". ")
        .or_else(|| tail.find('\n'))
        .or_else(|| tail.find('`'))
        .unwrap_or(tail.len());
    let model = tail[..end].trim().trim_matches('.').trim();
    if model.is_empty() || model.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(model.to_string())
    }
}

/// agy 记下的是显示名（`Gemini 3.8 Flash (High)`）；换成 `agy models` 第一列那样的 ID
/// （`gemini-3.8-flash-high`）才能对上定价表，查价时档位后缀会被剥掉。已经是 ID 的原样返回。
fn model_id_of(display: &str) -> String {
    if !display.contains(' ') {
        return display.to_string();
    }
    display
        .to_ascii_lowercase()
        .replace(" (", "-")
        .replace(['(', ')'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// 五个金额（输入、输出、缓存读、缓存写、合计）；查不到价格的记 0。
fn costs(conn: &rusqlite::Connection, model: &str, tokens: [i64; 4]) -> [String; 5] {
    let [input, output, cache_read, cache_write] =
        tokens.map(|v| u32::try_from(v).unwrap_or(u32::MAX));
    let usage = TokenUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_write,
        cache_creation_1h_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };
    match crate::services::usage_stats::find_model_pricing(conn, model) {
        Some(pricing) => {
            let cost = CostCalculator::calculate_for_app(
                "agy",
                &usage,
                &pricing,
                rust_decimal::Decimal::from(1),
            );
            [
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                cost.total_cost.to_string(),
            ]
        }
        None => std::array::from_fn(|_| "0".to_string()),
    }
}

fn model_timeline(rows: &[Value], initial_model: Option<&str>) -> HashMap<u64, String> {
    let mut ordered: Vec<&Value> = rows.iter().collect();
    ordered.sort_by_key(|row| row["step_index"].as_u64().unwrap_or(u64::MAX));
    let mut current_model = initial_model.map(str::to_owned);
    let mut model_by_step = HashMap::new();
    for row in ordered {
        let Some(step) = row["step_index"].as_u64() else {
            continue;
        };
        if row["type"] == "USER_INPUT" {
            if let Some(model) = row["content"].as_str().and_then(model_from_settings_change) {
                current_model = Some(model);
            }
        }
        // GENERIC creation results and PLANNER_RESPONSE rows share the same state.
        if let Some(model) = &current_model {
            model_by_step.insert(step, model.clone());
        }
    }
    model_by_step
}

#[cfg(test)]
fn import_records(db: &Database, session: &str, rows: &[Value]) -> Result<(u32, u32), AppError> {
    import_records_with_fallback(db, session, rows, None)
}

#[cfg(test)]
fn import_records_with_fallback(
    db: &Database,
    session: &str,
    rows: &[Value],
    initial_model: Option<&str>,
) -> Result<(u32, u32), AppError> {
    import_records_with_models(db, session, rows, &model_timeline(rows, initial_model))
}

fn import_records_with_models(
    db: &Database,
    session: &str,
    rows: &[Value],
    model_by_step: &HashMap<u64, String>,
) -> Result<(u32, u32), AppError> {
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let (mut imported, mut skipped) = (0, 0);
    for row in rows {
        if row["type"] != "PLANNER_RESPONSE" || row["status"] != "DONE" {
            continue;
        }
        let Some(step) = row["step_index"].as_u64() else {
            continue;
        };
        let token = |name: &str| {
            row[name]
                .as_u64()
                .and_then(|v| i64::try_from(v).ok())
                .unwrap_or(0)
        };
        let input = token("input_tokens");
        let output = token("output_tokens");
        let cache = token("cache_read_tokens");
        let write = token("cache_write_tokens");
        if input == 0 && output == 0 && cache == 0 && write == 0 {
            continue;
        }
        let id = format!("agy:{session}:{step}");
        // Persist a tombstone with the row so retention never imports pruned history again.
        let fresh = tx.execute("INSERT OR IGNORE INTO session_usage_dedup(data_source,request_id,semantic_id,has_entry_id) VALUES ('agy_session',?1,?1,1)", [&id])?;
        // `model` 存 ID（对得上定价和其它应用的统计），`request_model` 留显示名。
        let display = row["model"]
            .as_str()
            .map(str::to_string)
            .or_else(|| model_by_step.get(&step).cloned())
            .unwrap_or_else(|| "unknown".to_string());
        let model = model_id_of(&display);
        let [input_cost, output_cost, cache_cost, write_cost, total_cost] =
            costs(&tx, &model, [input, output, cache, write]);
        if fresh == 0 {
            // A rewritten completed step may correct counters; update retained detail only.
            // If retention already removed it, its tombstone still forbids resurrection.
            let changed = tx.execute("UPDATE proxy_request_logs SET model=?2,request_model=?7,input_tokens=?3,output_tokens=?4,cache_read_tokens=?5,cache_creation_tokens=?6,input_cost_usd=?8,output_cost_usd=?9,cache_read_cost_usd=?10,cache_creation_cost_usd=?11,total_cost_usd=?12 WHERE request_id=?1 AND (model!=?2 OR request_model!=?7 OR input_tokens!=?3 OR output_tokens!=?4 OR cache_read_tokens!=?5 OR cache_creation_tokens!=?6 OR total_cost_usd!=?12)", params![id,model,input,output,cache,write,display,input_cost,output_cost,cache_cost,write_cost,total_cost])?;
            imported += changed as u32;
            skipped += u32::from(changed == 0);
            continue;
        }
        // Native agy was verified with a local mock: input excludes cache, output includes
        // thinking. Never subtract cache or add thinking a second time. Missing historical
        // models remain unknown and unpriced; never fall back to the active provider.
        tx.execute("INSERT INTO proxy_request_logs (
            request_id,provider_id,app_type,model,request_model,input_tokens,output_tokens,
            cache_read_tokens,cache_creation_tokens,input_cost_usd,output_cost_usd,
            cache_read_cost_usd,cache_creation_cost_usd,total_cost_usd,latency_ms,status_code,
            session_id,provider_type,is_streaming,cost_multiplier,created_at,data_source,input_token_semantics
        ) VALUES (?1,'_agy_session','agy',?2,?10,?3,?4,?5,?6,?11,?12,?13,?14,?15,0,200,?7,'agy_session',1,'1',?8,'agy_session',?9)",
        params![id, model, input, output, cache, write, session, agy::timestamp(row).unwrap_or(0)/1000, INPUT_TOKEN_SEMANTICS_FRESH, display, input_cost, output_cost, cache_cost, write_cost, total_cost])?;
        imported += 1;
    }
    tx.commit()?;
    Ok((imported, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn agy_native_fixture_preserves_fresh_input_and_combined_thinking_output() {
        let db = Database::memory().unwrap();
        let rows = include_str!("../../../tests/fixtures/sessions/agy.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect::<Vec<Value>>();
        assert_eq!(import_records(&db, "fixture", &rows).unwrap(), (1, 0));
        let conn = db.conn.lock().unwrap();
        let got=conn.query_row("SELECT input_tokens,output_tokens,cache_read_tokens,input_token_semantics FROM proxy_request_logs",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?))).unwrap();
        assert_eq!(got, (80, 12, 20, INPUT_TOKEN_SEMANTICS_FRESH));
    }

    #[test]
    fn agy_rewritten_counters_update_retained_rows_without_reimporting_pruned_rows() {
        let db = Database::memory().unwrap();
        let mut row =
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10});
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (1, 0));
        row["input_tokens"] = json!(20);
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (1, 0));
        assert_eq!(import_records(&db, "s", &[row]).unwrap(), (0, 1));
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT input_tokens FROM proxy_request_logs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            20
        );
    }

    #[test]
    fn agy_usage_is_exact_deduplicated_and_survives_retention() {
        let db = Database::memory().unwrap();
        let rows = vec![
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":11830,"output_tokens":137,"cache_read_tokens":25}),
        ];
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (1, 0));
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (0, 1));
        let conn = db.conn.lock().unwrap();
        let got = conn.query_row("SELECT input_tokens,output_tokens,cache_read_tokens,model FROM proxy_request_logs WHERE app_type='agy'", [], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?, r.get::<_,i64>(2)?, r.get::<_,String>(3)?))).unwrap();
        assert_eq!(got, (11830, 137, 25, "unknown".into()));
        conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        drop(conn);
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (0, 1));
        assert_eq!(import_records(&db, "other", &rows).unwrap(), (1, 0));
    }
    #[test]
    fn model_selection_change_labels_later_responses() {
        let db = Database::memory().unwrap();
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","status":"DONE","content":"The user changed setting `Model Selection` from None to Gemini 3.8 Flash (High). No need to comment"}),
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10,"output_tokens":5}),
        ];
        assert_eq!(import_records(&db, "models", &rows).unwrap(), (1, 0));
        let conn = db.conn.lock().unwrap();
        let model: String = conn
            .query_row(
                "SELECT model FROM proxy_request_logs WHERE app_type='agy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model, "gemini-3.8-flash-high");
    }

    #[test]
    fn display_names_map_to_priced_model_ids() {
        assert_eq!(
            model_id_of("Gemini 3.8 Flash (High)"),
            "gemini-3.8-flash-high"
        );
        assert_eq!(model_id_of("Gemini 4 Argon (Low)"), "gemini-4-argon-low");
        assert_eq!(model_id_of("gemini-3.8-flash"), "gemini-3.8-flash");
        assert_eq!(model_id_of("unknown"), "unknown");

        let db = Database::memory().unwrap();
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","status":"DONE","content":"The user changed setting `Model Selection` from None to Gemini 3.8 Flash (High). No need to comment"}),
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":0}),
            json!({"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","model":"Gemini 3.1 Pro (High)","input_tokens":1_000_000}),
            json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"Mystery Model","input_tokens":5}),
        ];
        assert_eq!(import_records(&db, "priced", &rows).unwrap(), (3, 0));
        let conn = db.conn.lock().unwrap();
        let price = |model: &str| -> (String, String) {
            conn.query_row(
                "SELECT request_model, total_cost_usd FROM proxy_request_logs WHERE model=?1",
                [model],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let (display, flash) = price("gemini-3.8-flash-high");
        assert_eq!(display, "Gemini 3.8 Flash (High)");
        assert!(flash.parse::<f64>().unwrap() > 0.0, "{flash}");
        let (_, pro) = price("gemini-3.1-pro-high");
        assert!(pro.parse::<f64>().unwrap() > 0.0, "{pro}");
        assert_eq!(price("mystery-model").1, "0");
    }

    #[test]
    fn agy_incomplete_steps_do_not_consume_dedup_ids() {
        let db = Database::memory().unwrap();
        let mut row =
            json!({"step_index":0,"type":"PLANNER_RESPONSE","status":"RUNNING","input_tokens":10});
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (0, 0));
        row["status"] = json!("DONE");
        assert_eq!(import_records(&db, "s", &[row]).unwrap(), (1, 0));
    }

    fn native_creation(step: u64, ids: &[&str]) -> Value {
        let objects = ids
            .iter()
            .map(|id| json!({"conversationId": id}).to_string())
            .collect::<Vec<_>>()
            .join("\n");
        json!({
            "step_index": step, "type": "GENERIC", "status": "DONE",
            "content": format!(
                "Created At: 2026-10-23T01:22:23+08:00\nCompleted At: 2026-10-23T01:22:23+08:00\nCreated the following subagents:\n{objects}\nThe subagents will send you a message."
            )
        })
    }

    fn native_invocations(step: u64, arguments: &[Value]) -> Value {
        json!({
            "step_index": step, "type": "PLANNER_RESPONSE", "status": "DONE",
            "tool_calls": arguments.iter().map(|args| {
                json!({"name": "invoke_subagent", "args": args})
            }).collect::<Vec<_>>()
        })
    }

    #[test]
    fn command_output_replaying_a_creation_result_is_not_creation_evidence() {
        // Local snapshot: steps 408 and 414 contained the same UUID because run_command
        // printed step 408's content. Only the original tool result created the child.
        let mut creation = native_creation(408, &["test-alpha"]);
        creation["content"] = json!(creation["content"]
            .as_str()
            .unwrap()
            .replace("subagents:", "subagent:"));
        let replay = json!({
            "step_index": 414, "type": "GENERIC", "status": "DONE",
            "content": format!(
                "Created At: 2026-10-23T01:21:09+08:00\nCompleted At: 2026-10-23T01:21:18+08:00\nCommand exited with code 0.\n\nCommand output:\nCONTENT IN 408:\r\n{}",
                creation["content"].as_str().unwrap().replace('\n', "\r\n")
            )
        });
        assert!(subagent_creation_result(&replay).is_none());
        let rows = [
            native_invocations(
                407,
                &[json!({"Model":"flash","Role":"Test Subagent Alpha","TypeName":"research"})],
            ),
            creation,
            json!({"step_index":413,"type":"PLANNER_RESPONSE","tool_calls":[{"name":"run_command","args":{}}]}),
            replay,
        ];
        assert_eq!(
            resolve_test_subagent(
                &rows,
                "test-alpha",
                Some(408),
                Some(("Test Subagent Alpha", "research")),
            )
            .as_deref(),
            Some("Gemini 3.8 Flash (High)")
        );
        let mut replay_only = rows.to_vec();
        replay_only.remove(1);
        assert_eq!(
            resolve_test_subagent(&replay_only, "test-alpha", Some(408), None),
            None
        );
    }

    fn native_probe_rows() -> Vec<Value> {
        vec![
            native_invocations(
                420,
                &[
                    json!({"Role": "Probe Pro Agent", "TypeName": "research", "Model": "pro"}),
                    json!({"Role": "Probe Flash Agent", "TypeName": "research", "Model": "flash"}),
                ],
            ),
            native_creation(421, &["probe-pro"]),
            native_creation(422, &["probe-flash"]),
        ]
    }

    fn resolve_test_subagent(
        rows: &[Value],
        session: &str,
        hint: Option<u64>,
        descriptor: Option<(&str, &str)>,
    ) -> Option<String> {
        let metadata = descriptor.map(|(role, type_name)| {
            json!({"subagentDescriptor": {"role": role, "typeName": type_name}})
        });
        resolve_child_model(
            rows,
            &SubagentChild {
                session_id: session.to_owned(),
                spawn_step: hint,
                descriptor: metadata
                    .as_ref()
                    .and_then(SubagentDescriptor::from_metadata),
            },
            &model_timeline(rows, None),
        )
    }

    #[test]
    fn invalid_sibling_calls_do_not_hide_a_valid_explicit_model() {
        for invalid_args in [
            json!({"Subagents":[]}),
            json!({"Subagents":"invalid"}),
            Value::Null,
        ] {
            for invalid_first in [true, false] {
                let valid = json!({"Role":"Valid Worker","TypeName":"research","Model":"pro"});
                let calls = if invalid_first {
                    vec![invalid_args.clone(), valid]
                } else {
                    vec![valid, invalid_args.clone()]
                };
                let rows = [
                    native_invocations(10, &calls),
                    json!({"step_index":11,"type":"GENERIC","status":"DONE","content":"Error: invalid subagent arguments"}),
                    native_creation(12, &["valid-child"]),
                ];
                assert_eq!(
                    resolve_test_subagent(
                        &rows,
                        "valid-child",
                        Some(12),
                        Some(("Valid Worker", "research"))
                    )
                    .as_deref(),
                    Some("Gemini 3.1 Pro (High)")
                );
                assert!(resolve_test_subagent(
                    &rows,
                    "valid-child",
                    Some(12),
                    Some(("Other Worker", "research"))
                )
                .is_none());
            }
        }
    }

    #[test]
    fn independent_calls_with_a_shared_spawn_hint_resolve_their_own_models() {
        let mut rows = native_probe_rows();
        let pro = Some(("Probe Pro Agent", "research"));
        let flash = Some(("Probe Flash Agent", "research"));
        assert_eq!(
            resolve_test_subagent(&rows, "probe-pro", Some(422), pro).as_deref(),
            Some("Gemini 3.1 Pro (High)")
        );
        assert_eq!(
            resolve_test_subagent(&rows, "probe-flash", Some(422), flash).as_deref(),
            Some("Gemini 3.8 Flash (High)")
        );

        // 文件中的行顺序不影响按 step_index 关联，也允许缺失或过期的提示步。
        rows.swap(1, 2);
        assert_eq!(
            resolve_test_subagent(&rows, "probe-pro", None, pro).as_deref(),
            Some("Gemini 3.1 Pro (High)")
        );
        assert_eq!(
            resolve_test_subagent(&rows, "probe-flash", Some(999), flash).as_deref(),
            Some("Gemini 3.8 Flash (High)")
        );
    }

    #[test]
    fn identical_descriptors_resolve_when_effective_models_agree() {
        let rows = [
            native_invocations(
                10,
                &[
                    json!({"Role": "Parallel Worker", "TypeName": "research", "Model": "flash"}),
                    json!({"Role": "Parallel Worker", "TypeName": "research", "Model": "gemini-3.8-flash-high"}),
                ],
            ),
            native_creation(11, &["same-a"]),
            native_creation(12, &["same-b"]),
        ];
        for session in ["same-a", "same-b"] {
            assert_eq!(
                resolve_test_subagent(
                    &rows,
                    session,
                    Some(12),
                    Some(("Parallel Worker", "research")),
                )
                .map(|model| model_id_of(&model))
                .as_deref(),
                Some("gemini-3.8-flash-high")
            );
            assert_eq!(
                resolve_test_subagent(&rows, session, Some(12), None)
                    .map(|model| model_id_of(&model))
                    .as_deref(),
                Some("gemini-3.8-flash-high")
            );
        }
        // 即使结果不完整，只要剩余调用候选的实际模型一致，仍能安全解析。
        let incomplete = [rows[0].clone(), rows[2].clone()];
        assert_eq!(
            resolve_test_subagent(&incomplete, "same-b", Some(12), None)
                .map(|model| model_id_of(&model))
                .as_deref(),
            Some("gemini-3.8-flash-high")
        );
    }

    #[test]
    fn identical_descriptors_with_distinct_models_resolve_by_call_order() {
        for (first, second) in [("pro", "flash"), ("flash", "pro")] {
            let mut rows = vec![
                native_invocations(
                    10,
                    &[
                        json!({"Role":"Parallel Worker","TypeName":"research","Model":first}),
                        json!({"Role":"Parallel Worker","TypeName":"research","Model":second}),
                    ],
                ),
                native_creation(11, &["same-a"]),
                native_creation(12, &["same-b"]),
            ];
            // 创建结果在文件中的物理行位置不决定归属，step_index 才决定顺序。
            rows.swap(1, 2);
            for (session, model) in [("same-a", first), ("same-b", second)] {
                for descriptor in [Some(("Parallel Worker", "research")), None] {
                    assert_eq!(
                        resolve_test_subagent(&rows, session, Some(12), descriptor),
                        Some(normalize_explicit_subagent_model(model))
                    );
                }
            }
        }
    }

    #[test]
    fn ordered_calls_support_batches_and_stop_at_the_next_spawn_group() {
        let rows = [
            native_invocations(
                10,
                &[
                    json!({"Role":"Worker","TypeName":"research","Model":"pro"}),
                    json!({"Subagents":[
                        {"Role":"Worker","TypeName":"research","Model":"flash"},
                        {"Role":"Worker","TypeName":"research","Model":"flash_lite"}
                    ]}),
                ],
            ),
            json!({"step_index":11,"type":"GENERIC","status":"DONE","content":"Command output: unrelated result"}),
            native_creation(12, &["first"]),
            json!({"step_index":13,"type":"PLANNER_RESPONSE","status":"DONE"}),
            native_creation(14, &["second", "third"]),
            native_invocations(
                20,
                &[json!({"Role":"Worker","TypeName":"research","Model":"pro"})],
            ),
            native_creation(21, &["later"]),
        ];
        for (session, model) in [
            ("first", "pro"),
            ("second", "flash"),
            ("third", "flash_lite"),
            ("later", "pro"),
        ] {
            assert_eq!(
                resolve_test_subagent(&rows, session, Some(14), Some(("Worker", "research"))),
                Some(normalize_explicit_subagent_model(model))
            );
        }
    }

    #[test]
    fn incomplete_ordered_results_with_conflicting_models_preserve_unknown_usage() {
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","content":"Model Selection from None to Gemini 3.8 Flash (High)."}),
            native_invocations(
                10,
                &[
                    json!({"Role": "Parallel Worker", "TypeName": "research", "Model": "pro"}),
                    json!({"Role": "Parallel Worker", "TypeName": "research", "Model": "flash"}),
                ],
            ),
            // 缺少第一个创建结果时，不把剩余 UUID 错配给第一个调用。
            native_creation(12, &["same-b"]),
        ];
        let db = Database::memory().unwrap();
        for session in ["same-a", "same-b"] {
            let resolved = resolve_test_subagent(
                &rows,
                session,
                Some(12),
                Some(("Parallel Worker", "research")),
            );
            assert!(resolved.is_none());
            let usage = [
                json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":100,"output_tokens":20,"cache_read_tokens":30}),
            ];
            assert_eq!(
                import_records_with_fallback(&db, session, &usage, resolved.as_deref()).unwrap(),
                (1, 0)
            );
        }
        let conn = db.conn.lock().unwrap();
        let unknown: (i64, i64, i64, i64) = conn.query_row(
            "SELECT count(*),sum(input_tokens),sum(output_tokens),sum(cache_read_tokens) FROM proxy_request_logs WHERE model='unknown'",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).unwrap();
        assert_eq!(unknown, (2, 200, 40, 60));
    }

    #[test]
    fn inherit_candidates_use_the_model_at_the_actual_creation_step() {
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","content":"Model Selection from None to Gemini 3.8 Flash (High)."}),
            native_invocations(
                10,
                &[
                    json!({"Role": "Parallel Worker", "TypeName": "research", "Model": " inherit "}),
                    json!({"Role": "Parallel Worker", "TypeName": "research"}),
                ],
            ),
            native_creation(11, &["same-a"]),
            native_creation(12, &["same-b"]),
            json!({"step_index":13,"type":"USER_INPUT","content":"Model Selection from Gemini 3.8 Flash (High) to Gemini 3.1 Pro (High)."}),
        ];
        for session in ["same-a", "same-b"] {
            assert_eq!(
                resolve_test_subagent(
                    &rows,
                    session,
                    Some(13),
                    Some(("Parallel Worker", "research")),
                )
                .as_deref(),
                Some("Gemini 3.8 Flash (High)")
            );
        }
    }

    #[test]
    fn inherit_does_not_take_another_calls_explicit_model() {
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","content":"Model Selection from None to Gemini 4 Argon (Medium)."}),
            native_invocations(
                10,
                &[
                    json!({"Role": "Reviewer", "TypeName": "self", "Model": "inherit"}),
                    json!({"Role": "Reviewer", "TypeName": "research", "Model": "flash"}),
                ],
            ),
            native_creation(11, &["inherited"]),
            native_creation(12, &["explicit"]),
        ];
        assert_eq!(
            resolve_test_subagent(&rows, "inherited", Some(12), Some(("Reviewer", "self")))
                .as_deref(),
            Some("Gemini 4 Argon (Medium)")
        );
        assert_eq!(
            resolve_test_subagent(&rows, "explicit", Some(12), Some(("Reviewer", "research")))
                .as_deref(),
            Some("Gemini 3.8 Flash (High)")
        );
    }

    #[test]
    fn repeated_descriptors_in_other_groups_do_not_affect_the_model() {
        let rows = [
            native_invocations(
                10,
                &[json!({"Role":"Reviewer","TypeName":"self","Model":"pro"})],
            ),
            native_creation(11, &["old-review"]),
            native_invocations(
                20,
                &[json!({"Role":"Reviewer","TypeName":"self","Model":"flash"})],
            ),
            native_creation(21, &["new-review"]),
        ];
        let descriptor = Some(("Reviewer", "self"));
        assert_eq!(
            resolve_test_subagent(&rows, "old-review", Some(21), descriptor).as_deref(),
            Some("Gemini 3.1 Pro (High)")
        );
        assert_eq!(
            resolve_test_subagent(&rows, "new-review", Some(21), descriptor).as_deref(),
            Some("Gemini 3.8 Flash (High)")
        );
    }

    #[test]
    fn missing_or_invalid_creation_evidence_never_uses_the_first_argument() {
        let mut rows = native_probe_rows();
        assert!(resolve_test_subagent(&rows, "missing", Some(422), None).is_none());
        assert!(resolve_test_subagent(
            &rows,
            "probe-flash",
            Some(422),
            Some(("Wrong Role", "research")),
        )
        .is_none());

        // 完整结果可按跨调用顺序配对，即使没有描述信息也能识别第二个调用。
        assert_eq!(
            resolve_test_subagent(&rows, "probe-flash", Some(422), None).as_deref(),
            Some("Gemini 3.8 Flash (High)")
        );
        rows[2]["content"] =
            json!("Created the following subagent:\n{\"conversationId\":\"probe-flash\"");
        assert!(resolve_test_subagent(&rows, "probe-flash", Some(422), None).is_none());
        rows[2] = native_creation(422, &["probe-flash"]);
        rows[2]["type"] = json!("USER_INPUT");
        assert!(resolve_test_subagent(&rows, "probe-flash", Some(422), None).is_none());
        rows[2] = native_creation(422, &["probe-flash"]);
        rows.push(native_creation(423, &["probe-flash"]));
        assert!(resolve_test_subagent(&rows, "probe-flash", Some(422), None).is_none());
    }

    #[test]
    fn child_settings_and_explicit_response_models_override_the_initial_model() {
        let db = Database::memory().unwrap();
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","content":"Model Selection from None to Gemini 4 Argon (Medium)."}),
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10}),
            json!({"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10,"model":"Gemini 3.8 Flash (High)"}),
        ];
        import_records_with_fallback(&db, "child", &rows, Some("Gemini 3.1 Pro (High)")).unwrap();
        let conn = db.conn.lock().unwrap();
        let models: (String, String) = conn.query_row(
            "SELECT (SELECT model FROM proxy_request_logs WHERE request_id='agy:child:1'), (SELECT model FROM proxy_request_logs WHERE request_id='agy:child:2')",
            [], |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap();
        assert_eq!(
            models,
            (
                "gemini-4-argon-medium".into(),
                "gemini-3.8-flash-high".into()
            )
        );
    }

    #[test]
    fn corrected_initial_model_updates_known_rows_and_all_costs_idempotently() {
        let db = Database::memory().unwrap();
        let rows = [
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":50,"cache_read_tokens":100,"cache_write_tokens":20,"created_at":"2026-10-23T01:22:23+08:00"}),
        ];
        import_records_with_fallback(&db, "probe-flash", &rows, Some("Gemini 3.1 Pro (High)"))
            .unwrap();
        let initial_cost: String = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT total_cost_usd FROM proxy_request_logs", [], |r| {
                r.get(0)
            })
            .unwrap();
        let resolved = resolve_test_subagent(
            &native_probe_rows(),
            "probe-flash",
            Some(422),
            Some(("Probe Flash Agent", "research")),
        );
        assert_eq!(resolved.as_deref(), Some("Gemini 3.8 Flash (High)"));
        assert_eq!(
            import_records_with_fallback(&db, "probe-flash", &rows, resolved.as_deref()).unwrap(),
            (1, 0)
        );
        {
            let conn = db.conn.lock().unwrap();
            let (model, display, amounts): (String, String, [String; 5]) = conn.query_row(
                "SELECT model,request_model,input_cost_usd,output_cost_usd,cache_read_cost_usd,cache_creation_cost_usd,total_cost_usd FROM proxy_request_logs WHERE request_id='agy:probe-flash:1'",
                [], |r| Ok((r.get(0)?, r.get(1)?, [r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?])),
            ).unwrap();
            assert_eq!(model, "gemini-3.8-flash-high");
            assert_eq!(display, "Gemini 3.8 Flash (High)");
            assert_eq!(amounts, costs(&conn, &model, [1_000_000, 50, 100, 20]));
            assert_ne!(amounts[4], initial_cost);
            let retained: (String, i64, i64, i64, i64, i64) = conn.query_row(
                "SELECT session_id,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,created_at FROM proxy_request_logs",
                [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            ).unwrap();
            assert_eq!(
                retained,
                (
                    "probe-flash".into(),
                    1_000_000,
                    50,
                    100,
                    20,
                    agy::timestamp(&rows[0]).unwrap() / 1000
                )
            );
        }
        assert_eq!(
            import_records_with_fallback(&db, "probe-flash", &rows, resolved.as_deref()).unwrap(),
            (0, 1)
        );
        db.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM proxy_request_logs", [])
            .unwrap();
        assert_eq!(
            import_records_with_fallback(&db, "probe-flash", &rows, resolved.as_deref()).unwrap(),
            (0, 1)
        );
    }

    fn usage_row(step: u64) -> Value {
        json!({"step_index":step,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":100,"output_tokens":20,"cache_read_tokens":30})
    }

    fn settings_row(step: u64, model: &str) -> Value {
        json!({"step_index":step,"type":"USER_INPUT","content":format!("Model Selection from None to {model}.")})
    }

    fn write_session(brain: &Path, id: &str, rows: &[Value]) -> PathBuf {
        let logs = brain.join(id).join(".system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        let path = logs.join("transcript_full.jsonl");
        fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        path
    }

    fn write_child(brain: &Path, parent: &str, child: &str, hint: Option<u64>) {
        let dir = brain.join(parent).join(".system_generated/subagents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{child}.json")),
            json!({
                "conversationId":child, "spawnStepIndex":hint,
                "subagentDescriptor":{"role":"Worker","typeName":"research"},
            })
            .to_string(),
        )
        .unwrap();
    }

    fn session_models(db: &Database) -> BTreeMap<String, String> {
        db.conn
            .lock()
            .unwrap()
            .prepare("SELECT session_id, model FROM proxy_request_logs ORDER BY session_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn recursive_sync_passes_models_down_before_alphabetically_earlier_children() {
        // Reuse the reference branch's nested model cases through the real traversal.
        for (models, root_model, expected) in [
            (vec!["pro", "inherit", "inherit"], None, Some("pro")),
            (
                vec!["inherit", "inherit", "inherit"],
                Some("Gemini 3.8 Flash (High)"),
                Some("flash"),
            ),
            (
                vec!["pro", "flash", "inherit"],
                Some("Gemini 4 Argon (Medium)"),
                Some("flash"),
            ),
            (vec!["inherit", "flash", "inherit"], None, Some("flash")),
            (vec!["inherit", "inherit"], None, None),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let brain = dir.path().join("brain");
            let db = Database::memory().unwrap();
            let mut paths = BTreeMap::new();
            let mut parent = "z-root".to_string();
            for (depth, model) in models.iter().enumerate() {
                let child = format!("a-child-{depth}");
                let mut rows = vec![usage_row(5)];
                if depth == 0 {
                    if let Some(model) = root_model {
                        rows.push(settings_row(0, model));
                    }
                }
                rows.extend([
                    native_invocations(
                        10,
                        &[json!({"Role":"Worker","TypeName":"research","Model":model})],
                    ),
                    native_creation(11, &[&child]),
                ]);
                paths.insert(parent.clone(), write_session(&brain, &parent, &rows));
                write_child(&brain, &parent, &child, Some(11));
                parent = child;
            }
            paths.insert(
                parent.clone(),
                write_session(&brain, &parent, &[usage_row(1)]),
            );
            let mut reads = HashMap::new();
            let result = sync_session_paths(&db, paths.clone(), |path| {
                *reads.entry(path.to_owned()).or_insert(0) += 1;
                agy::records(path)
            });
            assert!(result.errors.is_empty(), "{:?}", result.errors);
            assert_eq!(result.files_scanned, paths.len() as u32);
            assert_eq!(result.imported, paths.len() as u32);
            assert_eq!(reads.len(), paths.len());
            assert!(reads.values().all(|count| *count == 1));
            let expected = expected
                .map(normalize_explicit_subagent_model)
                .map(|model| model_id_of(&model))
                .unwrap_or_else(|| "unknown".into());
            assert_eq!(session_models(&db)[&parent], expected, "models={models:?}");
            let repeated = sync_session_paths(&db, paths, agy::records);
            assert!(repeated.errors.is_empty());
            assert_eq!(repeated.imported, 0);
            assert_eq!(repeated.skipped, result.imported);
        }
    }

    #[test]
    fn recursive_sync_uses_local_settings_at_each_actual_creation_step() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path();
        let db = Database::memory().unwrap();
        let worker = json!({"Role":"Worker","TypeName":"research","Model":"inherit"});
        let mut paths = BTreeMap::new();
        paths.insert(
            "z-root".into(),
            write_session(
                brain,
                "z-root",
                &[
                    settings_row(0, "Gemini 3.8 Flash (High)"),
                    native_invocations(
                        10,
                        &[json!({"Role":"Worker","TypeName":"research","Model":"pro"})],
                    ),
                    native_creation(11, &["a-child"]),
                    settings_row(12, "Gemini 4 Argon (Low)"),
                ],
            ),
        );
        // Unsorted records and a per-response model do not change the settings timeline.
        let mut response = usage_row(5);
        response["model"] = json!("Gemini 3.8 Flash (High)");
        paths.insert(
            "a-child".into(),
            write_session(
                brain,
                "a-child",
                &[
                    settings_row(12, "Gemini 3.8 Flash (High)"),
                    native_creation(21, &["c-later"]),
                    native_invocations(20, &[worker.clone()]),
                    settings_row(0, "Gemini 4 Argon (Medium)"),
                    response,
                    native_invocations(10, &[worker]),
                    native_creation(11, &["b-earlier"]),
                ],
            ),
        );
        for child in ["b-earlier", "c-later"] {
            paths.insert(child.into(), write_session(brain, child, &[usage_row(1)]));
            write_child(brain, "a-child", child, Some(21));
        }
        write_child(brain, "z-root", "a-child", Some(11));
        let result = sync_session_paths(&db, paths, agy::records);
        assert!(result.errors.is_empty());
        let got = session_models(&db);
        assert_eq!(got["a-child"], "gemini-3.8-flash-high");
        assert_eq!(got["b-earlier"], "gemini-4-argon-medium");
        assert_eq!(got["c-later"], "gemini-3.8-flash-high");
    }

    #[test]
    fn recursive_sync_keeps_sibling_models_separate_in_a_batch() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path();
        let db = Database::memory().unwrap();
        let mut paths = BTreeMap::new();
        paths.insert(
            "z-root".into(),
            write_session(
                brain,
                "z-root",
                &[
                    settings_row(0, "Gemini 4 Argon (Medium)"),
                    native_invocations(
                        10,
                        &[json!({"Subagents":[
                            {"Role":"Worker","TypeName":"research","Model":"inherit"},
                            {"Role":"Worker","TypeName":"research","Model":"pro"},
                            {"Role":"Worker","TypeName":"research","Model":"flash_lite"},
                        ]})],
                    ),
                    native_creation(12, &["b-inherit", "a-pro", "c-lite"]),
                ],
            ),
        );
        for child in ["a-pro", "b-inherit", "c-lite"] {
            paths.insert(child.into(), write_session(brain, child, &[usage_row(1)]));
            write_child(brain, "z-root", child, Some(12));
        }
        let result = sync_session_paths(&db, paths, agy::records);
        assert!(result.errors.is_empty());
        assert_eq!(result.imported, 3);
        let got = session_models(&db);
        assert_eq!(got["a-pro"], "gemini-3.1-pro-high");
        assert_eq!(got["b-inherit"], "gemini-4-argon-medium");
        assert_eq!(got["c-lite"], "gemini-3.1-flash-lite-high");
    }

    #[test]
    fn recursive_sync_skips_failed_parent_subtree_but_imports_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path();
        let db = Database::memory().unwrap();
        let parent_path = write_session(brain, "z-unreadable", &[]);
        fs::remove_file(&parent_path).unwrap();
        let mut paths = BTreeMap::from([("z-unreadable".into(), parent_path)]);
        paths.insert(
            "b-child".into(),
            write_session(
                brain,
                "b-child",
                &[
                    settings_row(0, "Gemini 3.1 Pro (High)"),
                    usage_row(1),
                    native_invocations(10, &[json!({"Role":"Worker","TypeName":"research"})]),
                    native_creation(11, &["a-grandchild"]),
                ],
            ),
        );
        paths.insert(
            "a-grandchild".into(),
            write_session(brain, "a-grandchild", &[usage_row(1)]),
        );
        paths.insert(
            "c-orphan".into(),
            write_session(brain, "c-orphan", &[usage_row(1)]),
        );
        write_child(brain, "z-unreadable", "b-child", Some(11));
        write_child(brain, "b-child", "a-grandchild", None);
        let mut reads = Vec::new();
        let result = sync_session_paths(&db, paths.clone(), |path| {
            reads.push(path.to_owned());
            agy::records(path)
        });
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("z-unreadable"));
        assert_eq!(result.files_scanned, 2);
        assert_eq!(result.imported, 1);
        assert_eq!(
            reads,
            vec![paths["c-orphan"].clone(), paths["z-unreadable"].clone()]
        );
        let got = session_models(&db);
        assert_eq!(got, BTreeMap::from([("c-orphan".into(), "unknown".into())]));
    }

    #[test]
    fn recursive_sync_stops_rootless_cycles_and_keeps_explicit_models() {
        // This fixture has no root, so A starts without an inherited model.
        // A's explicit Pro selection applies to child B, not to A itself.
        // Returning from B to the visited A stops recursion and preserves A.
        for explicit in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let brain = dir.path();
            let db = Database::memory().unwrap();
            let mut paths = BTreeMap::new();
            for (parent, child, model) in [
                (
                    "a-cycle",
                    "b-cycle",
                    if explicit { "pro" } else { "inherit" },
                ),
                ("b-cycle", "a-cycle", "inherit"),
            ] {
                paths.insert(
                    parent.into(),
                    write_session(
                        brain,
                        parent,
                        &[
                            usage_row(1),
                            native_invocations(
                                10,
                                &[json!({"Role":"Worker","TypeName":"research","Model":model})],
                            ),
                            native_creation(11, &[child]),
                        ],
                    ),
                );
                write_child(brain, parent, child, Some(11));
            }
            let result = sync_session_paths(&db, paths, agy::records);
            assert!(result.errors.is_empty());
            assert_eq!(result.files_scanned, 2);
            assert_eq!(result.imported, 2);
            let got = session_models(&db);
            assert_eq!(got["a-cycle"], "unknown");
            assert_eq!(
                got["b-cycle"],
                if explicit {
                    "gemini-3.1-pro-high"
                } else {
                    "unknown"
                }
            );
        }
    }

    #[test]
    fn conflicting_parents_do_not_choose_the_first_model_or_lose_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path();
        let db = Database::memory().unwrap();
        let mut paths = BTreeMap::new();
        for (parent, model) in [("x-parent", "pro"), ("y-parent", "flash")] {
            paths.insert(
                parent.into(),
                write_session(
                    brain,
                    parent,
                    &[
                        native_invocations(
                            10,
                            &[json!({"Role":"Worker","TypeName":"research","Model":model})],
                        ),
                        native_creation(11, &["b-child"]),
                    ],
                ),
            );
            write_child(brain, parent, "b-child", Some(11));
        }
        paths.insert(
            "b-child".into(),
            write_session(
                brain,
                "b-child",
                &[
                    usage_row(1),
                    native_invocations(
                        10,
                        &[json!({"Role":"Worker","TypeName":"research","Model":"pro"})],
                    ),
                    native_creation(11, &["a-grandchild"]),
                ],
            ),
        );
        paths.insert(
            "a-grandchild".into(),
            write_session(brain, "a-grandchild", &[usage_row(1)]),
        );
        write_child(brain, "b-child", "a-grandchild", Some(11));
        let result = sync_session_paths(&db, paths, agy::records);
        assert!(result.errors.is_empty());
        let got = session_models(&db);
        assert_eq!(got["b-child"], "unknown");
        assert_eq!(got["a-grandchild"], "gemini-3.1-pro-high");
    }

    #[test]
    fn graph_uses_only_selected_paths_without_mixing_brain_roots() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("cli/brain");
        let ide = dir.path().join("ide/brain");
        let db = Database::memory().unwrap();
        let paths = BTreeMap::from([
            (
                "parent".into(),
                write_session(
                    &cli,
                    "parent",
                    &[
                        native_invocations(
                            10,
                            &[json!({"Role":"Worker","TypeName":"research","Model":"pro"})],
                        ),
                        native_creation(11, &["child"]),
                    ],
                ),
            ),
            (
                "child".into(),
                write_session(&ide, "child", &[usage_row(1)]),
            ),
        ]);
        write_child(&cli, "parent", "child", Some(11));
        // A second copy exists, but the scanner selected the IDE child.
        write_session(&cli, "child", &[usage_row(1)]);
        let graph = SubagentGraph::from_sessions(&paths);
        assert!(graph.incoming.is_empty());
        let mut reads = Vec::new();
        let result = sync_session_paths(&db, paths, |path| {
            reads.push(path.to_owned());
            agy::records(path)
        });
        assert!(result.errors.is_empty());
        assert_eq!(session_models(&db)["child"], "unknown");
        assert!(!reads.contains(&cli.join("child/.system_generated/logs/transcript_full.jsonl")));
    }

    #[test]
    fn malformed_metadata_defers_the_child() {
        for metadata in ["{", "null", r#"{"conversationId":"another-child"}"#] {
            let dir = tempfile::tempdir().unwrap();
            let brain = dir.path();
            let db = Database::memory().unwrap();
            let paths = BTreeMap::from([
                (
                    "parent".into(),
                    write_session(
                        brain,
                        "parent",
                        &[
                            native_invocations(
                                10,
                                &[json!({"Role":"Worker","TypeName":"research","Model":"pro"})],
                            ),
                            native_creation(11, &["child"]),
                        ],
                    ),
                ),
                (
                    "child".into(),
                    write_session(brain, "child", &[usage_row(1)]),
                ),
            ]);
            write_child(brain, "parent", "child", Some(11));
            fs::write(
                brain.join("parent/.system_generated/subagents/child.json"),
                metadata,
            )
            .unwrap();
            let result = sync_session_paths(&db, paths, agy::records);
            assert_eq!(result.errors.len(), 1);
            assert_eq!(result.files_scanned, 1);
            assert!(!session_models(&db).contains_key("child"));
        }
    }

    #[test]
    fn a_parent_import_error_does_not_stop_model_resolution_for_its_child() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path();
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute_batch(
            "CREATE TRIGGER fail_parent BEFORE INSERT ON proxy_request_logs WHEN NEW.session_id='parent' BEGIN SELECT RAISE(FAIL,'parent import failed'); END;"
        ).unwrap();
        let paths = BTreeMap::from([
            (
                "parent".into(),
                write_session(
                    brain,
                    "parent",
                    &[
                        usage_row(1),
                        native_invocations(
                            10,
                            &[json!({"Role":"Worker","TypeName":"research","Model":"pro"})],
                        ),
                        native_creation(11, &["child"]),
                    ],
                ),
            ),
            (
                "child".into(),
                write_session(brain, "child", &[usage_row(1)]),
            ),
        ]);
        write_child(brain, "parent", "child", Some(11));
        let result = sync_session_paths(&db, paths, agy::records);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("parent"));
        assert_eq!(result.imported, 1);
        assert_eq!(session_models(&db)["child"], "gemini-3.1-pro-high");
    }
    #[test]
    fn recursive_sync_revisits_imported_roots_when_child_links_arrive_later() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path();
        let db = Database::memory().unwrap();
        let mut paths = BTreeMap::new();
        for (session, child, model) in [
            ("z-root", Some("b-child"), "inherit"),
            ("b-child", Some("a-grandchild"), "flash"),
            ("a-grandchild", None, "inherit"),
        ] {
            let mut rows = vec![usage_row(1)];
            if session == "z-root" {
                rows.push(settings_row(0, "Gemini 3.1 Pro (High)"));
            }
            if let Some(child) = child {
                rows.extend([
                    native_invocations(
                        10,
                        &[json!({"Role":"Worker","TypeName":"research","Model":model})],
                    ),
                    native_creation(11, &[child]),
                ]);
            }
            paths.insert(session.into(), write_session(brain, session, &rows));
        }
        // Initially the exported usage exists, but the relationship files do not.
        let first = sync_session_paths(&db, paths.clone(), agy::records);
        assert!(first.errors.is_empty());
        assert_eq!(first.imported, 3);
        let before = session_models(&db);
        assert_eq!(before["z-root"], "gemini-3.1-pro-high");
        assert_eq!(before["b-child"], "unknown");
        assert_eq!(before["a-grandchild"], "unknown");

        write_child(brain, "z-root", "b-child", Some(11));
        write_child(brain, "b-child", "a-grandchild", Some(11));
        let repaired = sync_session_paths(&db, paths.clone(), agy::records);
        assert!(repaired.errors.is_empty());
        assert_eq!(repaired.imported, 2);
        assert_eq!(repaired.skipped, 1);
        let after = session_models(&db);
        assert_eq!(after["b-child"], "gemini-3.1-pro-high");
        assert_eq!(after["a-grandchild"], "gemini-3.8-flash-high");
        let repeated = sync_session_paths(&db, paths, agy::records);
        assert!(repeated.errors.is_empty());
        assert_eq!(repeated.imported, 0);
        assert_eq!(repeated.skipped, 3);
    }

    #[test]
    fn recursive_sync_parent_read_failure_preserves_resolved_descendant_models() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path();
        let db = Database::memory().unwrap();
        let mut paths = BTreeMap::new();
        for (session, child) in [
            ("z-root", Some("b-child")),
            ("b-child", Some("a-grandchild")),
            ("a-grandchild", None),
        ] {
            let mut rows = vec![usage_row(1)];
            if session == "z-root" {
                rows.push(settings_row(0, "Gemini 3.1 Pro (High)"));
            }
            if let Some(child) = child {
                rows.extend([
                    native_invocations(
                        10,
                        &[json!({"Role":"Worker","TypeName":"research","Model":"inherit"})],
                    ),
                    native_creation(11, &[child]),
                ]);
                write_child(brain, session, child, Some(11));
            }
            paths.insert(session.into(), write_session(brain, session, &rows));
        }
        let first = sync_session_paths(&db, paths.clone(), agy::records);
        assert!(first.errors.is_empty());
        let before = session_models(&db);
        assert!(before.values().all(|model| model == "gemini-3.1-pro-high"));
        let before_rows = snapshot_log_rows(&db);

        for (failed_session, expected_reads) in [
            ("z-root", vec![paths["z-root"].clone()]),
            (
                "b-child",
                vec![paths["z-root"].clone(), paths["b-child"].clone()],
            ),
        ] {
            let mut reads = Vec::new();
            let failed = sync_session_paths(&db, paths.clone(), |path| {
                reads.push(path.to_owned());
                if path == paths[failed_session] {
                    Err("Temporary transcript read failure".into())
                } else {
                    agy::records(path)
                }
            });
            assert_eq!(failed.errors.len(), 1);
            assert!(failed.errors[0].contains(failed_session));
            assert_eq!(failed.imported, 0);
            assert_eq!(failed.files_scanned, expected_reads.len() as u32);
            assert_eq!(reads, expected_reads);
            // All retained fields, including models, tokens and five costs,
            // survive failures at both the root and an intermediate parent.
            assert_eq!(snapshot_log_rows(&db), before_rows);
            let recovered = sync_session_paths(&db, paths.clone(), agy::records);
            assert!(recovered.errors.is_empty());
            assert_eq!(recovered.files_scanned, 3);
            assert_eq!(recovered.imported, 0);
            assert_eq!(recovered.skipped, 3);
            assert_eq!(session_models(&db), before);
        }
    }

    #[test]
    fn recursive_sync_child_read_failure_still_imports_its_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path();
        let db = Database::memory().unwrap();
        let mut paths = review_inherited_tree(brain);
        paths.insert(
            "z-root".into(),
            write_session(
                brain,
                "z-root",
                &[
                    settings_row(0, "Gemini 3.1 Pro (High)"),
                    native_invocations(
                        10,
                        &[json!({"Subagents":[
                            {"Role":"Worker","TypeName":"research","Model":"inherit"},
                            {"Role":"Worker","TypeName":"research","Model":"flash"},
                        ]})],
                    ),
                    native_creation(11, &["b-child", "c-sibling"]),
                ],
            ),
        );
        paths.insert(
            "c-sibling".into(),
            write_session(brain, "c-sibling", &[usage_row(1)]),
        );
        write_child(brain, "z-root", "c-sibling", Some(11));
        let mut reads = Vec::new();
        let result = sync_session_paths(&db, paths.clone(), |path| {
            reads.push(path.to_owned());
            if path == paths["b-child"] {
                Err("Temporary child read failure".into())
            } else {
                agy::records(path)
            }
        });
        assert_eq!(result.errors.len(), 1);
        assert_eq!(
            reads,
            vec![
                paths["z-root"].clone(),
                paths["b-child"].clone(),
                paths["c-sibling"].clone()
            ]
        );
        assert_eq!(result.files_scanned, 3);
        assert_eq!(result.imported, 1);
        assert_eq!(
            session_models(&db),
            BTreeMap::from([("c-sibling".into(), "gemini-3.8-flash-high".into())])
        );
    }

    fn review_inherited_tree(brain: &Path) -> BTreeMap<String, PathBuf> {
        let mut paths = BTreeMap::new();
        for (session, child) in [
            ("z-root", Some("b-child")),
            ("b-child", Some("a-grandchild")),
            ("a-grandchild", None),
        ] {
            let mut rows = vec![usage_row(1)];
            if session == "z-root" {
                rows.push(settings_row(0, "Gemini 3.1 Pro (High)"));
            }
            if let Some(child) = child {
                rows.extend([
                    native_invocations(
                        10,
                        &[json!({"Role":"Worker","TypeName":"research","Model":"inherit"})],
                    ),
                    native_creation(11, &[child]),
                ]);
                write_child(brain, session, child, Some(11));
            }
            paths.insert(session.into(), write_session(brain, session, &rows));
        }
        paths
    }

    #[test]
    fn review_metadata_read_failure_preserves_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let paths = review_inherited_tree(dir.path());
        let first = sync_session_paths(&db, paths.clone(), agy::records);
        assert!(first.errors.is_empty());
        let before = session_models(&db);
        assert!(before.values().all(|model| model == "gemini-3.1-pro-high"));
        let before_rows = snapshot_log_rows(&db);
        let metadata = dir
            .path()
            .join("z-root/.system_generated/subagents/b-child.json");
        // Invalid UTF-8 deterministically makes read_to_string return Err.
        let original = fs::read(&metadata).unwrap();
        fs::write(&metadata, [0xff]).unwrap();
        assert!(fs::read_to_string(&metadata).is_err());
        let mut reads = Vec::new();
        let failed = sync_session_paths(&db, paths.clone(), |path| {
            reads.push(path.to_owned());
            agy::records(path)
        });
        assert_eq!(failed.errors.len(), 1);
        assert_eq!(reads, vec![paths["z-root"].clone()]);
        assert_eq!(failed.imported, 0);
        assert_eq!(snapshot_log_rows(&db), before_rows);
        fs::write(metadata, original).unwrap();
        let recovered = sync_session_paths(&db, paths, agy::records);
        assert!(recovered.errors.is_empty());
        assert_eq!(recovered.files_scanned, 3);
        assert_eq!(recovered.imported, 0);
        assert_eq!(snapshot_log_rows(&db), before_rows);
    }

    #[test]
    #[ignore = "Requires a fresh isolated CC_SWITCH_TEST_HOME for the production scanner"]
    fn review_scanner_parent_read_failure_preserves_descendants() {
        let home = std::env::var_os("CC_SWITCH_TEST_HOME")
            .map(PathBuf::from)
            .expect("Set CC_SWITCH_TEST_HOME to a fresh isolated directory");
        assert!(home.is_absolute() && home.is_dir() && home.starts_with(std::env::temp_dir()));
        assert_eq!(crate::config::get_home_dir(), home);
        let brain = crate::agy_config::agy_dir().join("brain");
        assert!(brain.starts_with(&home) && !brain.exists());
        let paths = review_inherited_tree(&brain);
        let db = Database::memory().unwrap();
        let first = sync_agy_usage(&db).unwrap();
        assert!(first.errors.is_empty());
        assert_eq!(first.files_scanned, 3);
        let before = session_models(&db);
        assert!(before.values().all(|model| model == "gemini-3.1-pro-high"));
        let before_rows = snapshot_log_rows(&db);
        let original = fs::read(&paths["z-root"]).unwrap();
        let malformed_json = String::from_utf8(original.clone())
            .unwrap()
            .lines()
            .map(|line| {
                let row: Value = serde_json::from_str(line).unwrap();
                if row["type"] == "GENERIC" {
                    "{"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        for (corrupted, visible_in_list) in [(vec![0xff], false), (malformed_json, true)] {
            fs::write(&paths["z-root"], corrupted).unwrap();
            assert_eq!(agy::records(&paths["z-root"]).is_ok(), visible_in_list);
            let selected: Vec<_> = agy::scan_sessions()
                .into_iter()
                .map(|s| s.session_id)
                .collect();
            assert_eq!(selected.iter().any(|id| id == "z-root"), visible_in_list);
            let failed = sync_agy_usage(&db).unwrap();
            assert_eq!(failed.errors.len(), 1);
            assert!(failed.errors[0].contains("z-root"));
            if visible_in_list {
                assert!(failed.errors[0].contains("line 4"));
            }
            assert_eq!(failed.files_scanned, 1);
            assert_eq!(failed.imported, 0);
            assert_eq!(snapshot_log_rows(&db), before_rows);
            fs::write(&paths["z-root"], &original).unwrap();
            let recovered = sync_agy_usage(&db).unwrap();
            assert!(recovered.errors.is_empty());
            assert_eq!(recovered.files_scanned, 3);
            assert_eq!(recovered.imported, 0);
            assert_eq!(snapshot_log_rows(&db), before_rows);
        }
    }

    #[test]
    fn orphan_rebuild_recovers_multiple_ancestors_and_reads_each_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let paths = review_inherited_tree(dir.path());
        write_session(dir.path(), "unrelated", &[usage_row(1)]);
        // Only the grandchild survived the scanner. Rebuild both missing
        // ancestors, then reuse the normal root-to-child parser.
        let selected = BTreeMap::from([("a-grandchild".into(), paths["a-grandchild"].clone())]);
        let mut reads = Vec::new();
        let first = sync_session_paths(&db, selected.clone(), |path| {
            reads.push(path.to_owned());
            agy::records(path)
        });
        assert!(first.errors.is_empty());
        assert_eq!(
            reads,
            vec![
                paths["z-root"].clone(),
                paths["b-child"].clone(),
                paths["a-grandchild"].clone()
            ]
        );
        assert_eq!(first.imported, 3);
        assert!(session_models(&db)
            .values()
            .all(|model| model == "gemini-3.1-pro-high"));
        let before = snapshot_log_rows(&db);
        // Even a deleted transcript leaves a parent identity in the metadata.
        let original = fs::read(&paths["z-root"]).unwrap();
        fs::remove_file(&paths["z-root"]).unwrap();
        let mut reads = Vec::new();
        let failed = sync_session_paths(&db, selected.clone(), |path| {
            reads.push(path.to_owned());
            agy::records(path)
        });
        assert_eq!(failed.errors.len(), 1);
        assert_eq!(reads, vec![paths["z-root"].clone()]);
        assert_eq!(snapshot_log_rows(&db), before);
        fs::write(&paths["z-root"], original).unwrap();
        let recovered = sync_session_paths(&db, selected, agy::records);
        assert!(recovered.errors.is_empty());
        assert_eq!(recovered.files_scanned, 3);
        assert_eq!(recovered.imported, 0);
        assert_eq!(snapshot_log_rows(&db), before);
    }

    #[test]
    fn orphan_rebuild_defers_a_bad_ancestor_edge_but_imports_a_true_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let paths = review_inherited_tree(dir.path());
        let first = sync_session_paths(&db, paths.clone(), agy::records);
        assert!(first.errors.is_empty());
        let before = snapshot_log_rows(&db);
        fs::write(
            dir.path()
                .join("z-root/.system_generated/subagents/b-child.json"),
            [0xff],
        )
        .unwrap();
        let orphan = write_session(dir.path(), "orphan", &[usage_row(1)]);
        let selected = BTreeMap::from([
            ("a-grandchild".into(), paths["a-grandchild"].clone()),
            ("orphan".into(), orphan.clone()),
        ]);
        let mut reads = Vec::new();
        let failed = sync_session_paths(&db, selected, |path| {
            reads.push(path.to_owned());
            agy::records(path)
        });
        assert_eq!(failed.errors.len(), 1);
        assert_eq!(failed.imported, 1);
        assert_eq!(reads, vec![orphan, paths["z-root"].clone()]);
        let after = snapshot_log_rows(&db);
        for (id, row) in before.1 {
            assert_eq!(after.1[&id], row);
        }
        assert_eq!(session_models(&db)["orphan"], "unknown");
    }

    #[test]
    fn incomplete_relationship_directory_defers_only_its_brain_root() {
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path().join("cli/brain");
        let other = dir.path().join("ide/brain");
        let db = Database::memory().unwrap();
        let mut paths = review_inherited_tree(&brain);
        let first = sync_session_paths(&db, paths.clone(), agy::records);
        assert!(first.errors.is_empty());
        let before = session_models(&db);
        // A file in place of the directory gives a deterministic read_dir error.
        let metadata_dir = brain.join("z-root/.system_generated/subagents");
        fs::remove_dir_all(&metadata_dir).unwrap();
        fs::write(&metadata_dir, "unavailable").unwrap();
        paths.insert(
            "orphan".into(),
            write_session(&other, "orphan", &[usage_row(1)]),
        );
        let mut reads = Vec::new();
        let failed = sync_session_paths(&db, paths.clone(), |path| {
            reads.push(path.to_owned());
            agy::records(path)
        });
        assert_eq!(failed.errors.len(), 1);
        assert_eq!(reads, vec![paths["orphan"].clone()]);
        let models = session_models(&db);
        for (id, model) in before {
            assert_eq!(models[&id], model);
        }
        assert_eq!(models["orphan"], "unknown");
        fs::remove_file(&metadata_dir).unwrap();
        write_child(&brain, "z-root", "b-child", Some(11));
        let recovered = sync_session_paths(&db, paths, agy::records);
        assert!(recovered.errors.is_empty());
        assert_eq!(recovered.files_scanned, 4);
        assert_eq!(recovered.imported, 0);
    }

    #[test]
    fn parent_json_parse_failure_preserves_descendants_and_retries() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let paths = review_inherited_tree(dir.path());
        let first = sync_session_paths(&db, paths.clone(), agy::usage_records);
        assert!(first.errors.is_empty());
        let before = snapshot_log_rows(&db);
        for (parent, expected_line, expected_reads) in [
            ("z-root", "line 4", vec![paths["z-root"].clone()]),
            (
                "b-child",
                "line 3",
                vec![paths["z-root"].clone(), paths["b-child"].clone()],
            ),
        ] {
            let original = fs::read_to_string(&paths[parent]).unwrap();
            let corrupted = original
                .lines()
                .map(|line| {
                    let row: Value = serde_json::from_str(line).unwrap();
                    if row["type"] == "GENERIC" {
                        "{"
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(&paths[parent], corrupted).unwrap();
            // The list/message reader remains tolerant; usage must reject it.
            let rows = agy::records(&paths[parent]).unwrap();
            assert!(rows.iter().all(|row| row["type"] != "GENERIC"));
            let mut reads = Vec::new();
            let failed = sync_session_paths(&db, paths.clone(), |path| {
                reads.push(path.to_owned());
                agy::usage_records(path)
            });
            assert_eq!(failed.errors.len(), 1);
            assert!(failed.errors[0].contains(parent));
            assert!(failed.errors[0].contains(expected_line));
            assert_eq!(failed.imported, 0);
            assert_eq!(reads, expected_reads);
            assert_eq!(snapshot_log_rows(&db), before);
            fs::write(&paths[parent], original).unwrap();
            let recovered = sync_session_paths(&db, paths.clone(), agy::usage_records);
            assert!(recovered.errors.is_empty());
            assert_eq!(recovered.files_scanned, 3);
            assert_eq!(recovered.imported, 0);
            assert_eq!(snapshot_log_rows(&db), before);
        }
    }

    fn snapshot_database(source: &rusqlite::Connection) -> Database {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        rusqlite::backup::Backup::new(source, &mut conn)
            .unwrap()
            .run_to_completion(1000, std::time::Duration::ZERO, None)
            .unwrap();
        Database {
            conn: std::sync::Mutex::new(conn),
            log_count_cache: std::sync::Mutex::new(None),
        }
    }

    fn snapshot_log_rows(
        db: &Database,
    ) -> (Vec<String>, BTreeMap<String, Vec<rusqlite::types::Value>>) {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT * FROM proxy_request_logs ORDER BY request_id")
            .unwrap();
        let names = stmt
            .column_names()
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let count = stmt.column_count();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>("request_id")?,
                    (0..count)
                        .map(|i| row.get::<_, rusqlite::types::Value>(i))
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        (names, rows)
    }

    #[test]
    #[ignore = "Requires an explicit database snapshot, copied home, and reference oracle"]
    fn agy_snapshot_matches_reference_and_preserves_usage_and_retention() {
        let snapshot_path = std::env::var_os("CC_SWITCH_AGY_SNAPSHOT_DB")
            .map(PathBuf::from)
            .expect("Set CC_SWITCH_AGY_SNAPSHOT_DB to the source snapshot");
        let home = std::env::var_os("CC_SWITCH_TEST_HOME")
            .map(PathBuf::from)
            .expect("Set CC_SWITCH_TEST_HOME to the isolated transcript copy");
        let oracle_path = std::env::var_os("CC_SWITCH_AGY_SNAPSHOT_ORACLE")
            .map(PathBuf::from)
            .expect("Set CC_SWITCH_AGY_SNAPSHOT_ORACLE to the reference JSON");
        // All writes stay in memory or in this explicitly isolated home.
        assert!(home.is_absolute() && home.is_dir() && home.starts_with(std::env::temp_dir()));
        assert_eq!(crate::config::get_home_dir(), home);
        let source = rusqlite::Connection::open_with_flags(
            &snapshot_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let original = snapshot_database(&source);
        let legacy = snapshot_database(&source);
        let candidate = snapshot_database(&source);
        let oracle: Value = serde_json::from_slice(&fs::read(oracle_path).unwrap()).unwrap();
        let expected = oracle["models_by_request"].as_object().unwrap();
        assert!(!expected.is_empty());
        let unknown_count = |db: &Database| -> i64 {
            db.conn.lock().unwrap().query_row(
                "SELECT count(*) FROM proxy_request_logs WHERE app_type='agy' AND model='unknown'", [], |r| r.get(0),
            ).unwrap()
        };
        let unknown_before = unknown_count(&original);
        let mut exported = HashSet::new();
        // Establish a baseline with the original importer against the same frozen logs.
        let sessions = agy::scan_sessions();
        let selected: BTreeMap<_, _> = sessions
            .iter()
            .filter_map(|s| {
                s.source_path
                    .as_ref()
                    .map(|path| (s.session_id.clone(), path.clone()))
            })
            .collect();
        fs::write(
            home.join("selected-sessions.json"),
            serde_json::to_vec_pretty(&selected).unwrap(),
        )
        .unwrap();
        println!(
            "Selected {} readable sessions from {} total sessions",
            selected.len(),
            sessions.len()
        );
        for session in sessions {
            let Some(path) = session.source_path else {
                continue;
            };
            assert!(Path::new(&path).starts_with(&home));
            let rows = agy::records(Path::new(&path)).unwrap();
            for row in &rows {
                if row["type"] == "PLANNER_RESPONSE"
                    && row["status"] == "DONE"
                    && [
                        "input_tokens",
                        "output_tokens",
                        "cache_read_tokens",
                        "cache_write_tokens",
                    ]
                    .iter()
                    .any(|key| {
                        row[key]
                            .as_u64()
                            .is_some_and(|n| n > 0 && n <= i64::MAX as u64)
                    })
                {
                    exported.insert(format!(
                        "agy:{}:{}",
                        session.session_id,
                        row["step_index"].as_u64().unwrap()
                    ));
                }
            }
            import_records(&legacy, &session.session_id, &rows).unwrap();
        }
        assert_eq!(exported.len(), expected.len());
        assert!(exported.iter().all(|id| expected.contains_key(id)));
        let first = sync_agy_usage(&candidate).unwrap();
        assert!(first.errors.is_empty(), "{:?}", first.errors);
        let mut recovered_paths = selected
            .iter()
            .map(|(id, path)| (id.clone(), PathBuf::from(path)))
            .collect();
        SubagentGraph::recover_orphan_parents(&mut recovered_paths);
        recovered_paths.retain(|id, _| !selected.contains_key(id));
        let (names, before) = snapshot_log_rows(&legacy);
        let (_, after) = snapshot_log_rows(&candidate);
        assert_eq!(before.len(), after.len());
        let mutable = [
            "model",
            "request_model",
            "input_cost_usd",
            "output_cost_usd",
            "cache_read_cost_usd",
            "cache_creation_cost_usd",
            "total_cost_usd",
        ];
        let app_index = names.iter().position(|name| name == "app_type").unwrap();
        for (id, row) in &before {
            let actual = after
                .get(id)
                .expect("The candidate lost a baseline request");
            let is_agy = row[app_index] == rusqlite::types::Value::Text("agy".into());
            for (index, name) in names.iter().enumerate() {
                if !(is_agy && mutable.contains(&name.as_str())) {
                    assert!(
                        row[index] == actual[index],
                        "Unexpected change: request {id}, column {name}"
                    );
                }
            }
        }
        let conn = candidate.conn.lock().unwrap();
        let mut checked = 0;
        let mut model_mismatches = Vec::new();
        let mut retained_without_source = 0;
        for (id, model) in expected {
            let actual = conn.query_row(
                "SELECT model,request_model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,input_cost_usd,output_cost_usd,cache_read_cost_usd,cache_creation_cost_usd,total_cost_usd FROM proxy_request_logs WHERE request_id=?1",
                [id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                    [row.get::<_,i64>(2)?,row.get(3)?,row.get(4)?,row.get(5)?],
                    [row.get::<_,String>(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?])),
            );
            match actual {
                Ok((canonical, display, tokens, amounts)) => {
                    if canonical != model["model"].as_str().unwrap()
                        || display != model["display"].as_str().unwrap()
                    {
                        model_mismatches.push(json!({
                            "request_id": id, "actual": canonical, "expected": model["model"],
                            "actual_costs": amounts,
                            "expected_costs": costs(&conn, model["model"].as_str().unwrap(), tokens),
                        }));
                    }
                    assert_eq!(amounts, costs(&conn, &canonical, tokens), "request {id}");
                    checked += 1;
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    // An existing retention tombstone can suppress an exported request.
                    let tombstone: i64 = conn.query_row(
                        "SELECT count(*) FROM session_usage_dedup WHERE data_source='agy_session' AND request_id=?1", [id], |r| r.get(0),
                    ).unwrap();
                    assert_eq!(tombstone, 1, "Missing row without tombstone: {id}");
                    retained_without_source += 1;
                }
                Err(error) => panic!("Cannot read request {id}: {error}"),
            }
        }
        let mut query = conn.prepare("SELECT model,count(*) FROM proxy_request_logs WHERE app_type='agy' GROUP BY model ORDER BY model").unwrap();
        let distribution: BTreeMap<String, i64> = query
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        drop(query);
        let retention = snapshot_database(&conn);
        let candidate_file = home.join("candidate.db");
        conn.backup(rusqlite::DatabaseName::Main, &candidate_file, None)
            .unwrap();
        drop(conn);
        let second = sync_agy_usage(&candidate).unwrap();
        assert!(second.errors.is_empty());
        assert_eq!(second.imported, 0);
        assert!(
            snapshot_log_rows(&candidate).1 == after,
            "Repeated sync changed request logs"
        );
        let sample = expected.keys().find(|id| after.contains_key(*id)).unwrap();
        retention
            .conn
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM proxy_request_logs WHERE request_id=?1",
                [sample],
            )
            .unwrap();
        let retained = sync_agy_usage(&retention).unwrap();
        assert!(retained.errors.is_empty());
        assert_eq!(retained.imported, 0);
        assert!(!snapshot_log_rows(&retention).1.contains_key(sample));
        let report = json!({
            "reference":oracle["reference"], "source_snapshot":snapshot_path,
            "recovered_ancestor_paths":recovered_paths,
            "exported_requests":expected.len(), "checked_models_and_all_costs":checked,
            "exports_suppressed_by_retention":retained_without_source,
            "model_mismatches": model_mismatches,
            "unknown_before":unknown_before, "unknown_after_legacy":unknown_count(&legacy),
            "unknown_after_candidate":unknown_count(&candidate), "models":distribution,
            "first_sync":{"scanned":first.files_scanned,"imported_or_corrected":first.imported,"skipped":first.skipped},
            "second_sync":{"scanned":second.files_scanned,"imported_or_corrected":second.imported,"skipped":second.skipped},
            "non_model_fields_unchanged":true, "other_providers_unchanged":true,
            "retention_no_resurrection":true,
        });
        fs::write(
            home.join("snapshot-report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        let mut summary = report.clone();
        summary["model_mismatches"] = json!(model_mismatches.len());
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        assert!(
            model_mismatches.is_empty(),
            "{} requests differ from the reference; see snapshot-report.json",
            model_mismatches.len()
        );
    }
}

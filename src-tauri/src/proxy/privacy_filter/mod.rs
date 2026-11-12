//! 隐私过滤器：请求进入上游 LLM 前脱敏 PII 与密钥
//!
//! 移植自 [packyme/privacy-filter](https://github.com/packyme/privacy-filter)（MIT License,
//! Copyright (c) 2026 PackyMe）。纯正则、无模型、O(n)。
//!
//! 两层检测：
//! - 结构化 PII（`pii`）：邮箱、手机号、身份证、银行卡（Luhn + 卡组织前缀或卡号上下文）、
//!   IPv4（不含回环 / 未指定 / 广播地址）
//! - 密钥 / 凭证（`secrets`）：gitleaks 规则集（关键词预筛）+ 上下文口令 + 香农熵兜底
//!
//! 各层产出 `(start, end, label)` 区间 → 合并去重叠 → 单遍重建文本。占位符不可逆。
//!
//! 代理侧只改写用户输入、系统提示、工具定义的说明文字和搜索词，不碰 thinking 块、签名、
//! 工具调用参数和图片等二进制内容，避免破坏协议语义。
//!
//! 转发器里每次尝试过滤多遍（见 `forwarder::RequestForwarder::apply_privacy_filter`）：先在客户端
//! 格式的请求体上过滤，此时还分得清用户文本和工具结果；Copilot 清洗 / 合并工具结果后、供应商的
//! 本地 body 覆写合并前各补一遍；最后在定稿的上游请求体上过滤，兜住代理自己补进去的内容。替换结果与原文相同的命中（第一遍留下的
//! 同类占位符）不改写、不计数；原文里写着的占位符字样不享受豁免，照常检测和替换。
//!
//! 模型回复和工具结果默认也不碰：模型只拿到占位符时，Edit / apply_patch 对不上原文，
//! 整文件重写还会把占位符写回用户的真实文件。工具结果可由 `tool_results` 子开关单独打开。

mod cache;
mod pii;
mod secrets;

use once_cell::sync::Lazy;
use secrets::SecretDetector;
use serde_json::Value;

/// 内置 gitleaks 默认规则集（上游 gitleaks/gitleaks `config/gitleaks.toml`）
const GITLEAKS_TOML: &str = include_str!("gitleaks.toml");

/// 命中的实体类型，替换时按语言取占位符
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Email,
    Phone,
    IdCard,
    BankCard,
    Ip,
    Secret,
}

/// 一次脱敏命中的区间。`start` / `end` 为 UTF-8 字节偏移。
#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    kind: Kind,
}

/// 一套占位符，跟随界面语言
#[derive(Debug)]
pub struct Placeholders {
    email: &'static str,
    phone: &'static str,
    id_card: &'static str,
    bank_card: &'static str,
    ip: &'static str,
    secret: &'static str,
}

const PLACEHOLDERS_ZH: Placeholders = Placeholders {
    email: "[邮箱]",
    phone: "[电话]",
    id_card: "[身份证]",
    bank_card: "[银行卡]",
    ip: "[IP]",
    secret: "[密钥]",
};

const PLACEHOLDERS_ZH_TW: Placeholders = Placeholders {
    email: "[電子郵件]",
    phone: "[電話]",
    id_card: "[身分證]",
    bank_card: "[銀行卡]",
    ip: "[IP]",
    secret: "[金鑰]",
};

const PLACEHOLDERS_EN: Placeholders = Placeholders {
    email: "[EMAIL]",
    phone: "[PHONE]",
    id_card: "[ID_CARD]",
    bank_card: "[BANK_CARD]",
    ip: "[IP]",
    secret: "[SECRET]",
};

const PLACEHOLDERS_JA: Placeholders = Placeholders {
    email: "[メール]",
    phone: "[電話番号]",
    id_card: "[身分証]",
    bank_card: "[銀行カード]",
    ip: "[IP]",
    secret: "[シークレット]",
};

const PLACEHOLDERS_TR: Placeholders = Placeholders {
    email: "[E-POSTA]",
    phone: "[TELEFON]",
    id_card: "[KİMLİK_NO]",
    bank_card: "[BANKA_KARTI]",
    ip: "[IP]",
    secret: "[GİZLİ_ANAHTAR]",
};

impl Placeholders {
    /// 按界面语言码（en / zh / zh-TW / ja / tr）取占位符，未知语言用英文
    pub fn for_language(language: &str) -> &'static Self {
        match language {
            "zh" => &PLACEHOLDERS_ZH,
            "zh-TW" => &PLACEHOLDERS_ZH_TW,
            "ja" => &PLACEHOLDERS_JA,
            "tr" => &PLACEHOLDERS_TR,
            _ => &PLACEHOLDERS_EN,
        }
    }

    fn label(&self, kind: Kind) -> &'static str {
        match kind {
            Kind::Email => self.email,
            Kind::Phone => self.phone,
            Kind::IdCard => self.id_card,
            Kind::BankCard => self.bank_card,
            Kind::Ip => self.ip,
            Kind::Secret => self.secret,
        }
    }
}

/// 一次脱敏的结果（测试用；代理路径走 `redact_cow`，没有命中时不复制原文）
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct RedactResult {
    pub redacted: String,
    pub count: usize,
}

/// 编译好的过滤器。规则创建后只读；检测缓存内部加锁，可并发安全地复用。
pub struct Filter {
    secrets: SecretDetector,
    cache: cache::DetectionCache,
}

impl Filter {
    /// 用 gitleaks TOML 规则文本创建过滤器；规则为空或解析失败时退回内置兜底规则。
    fn from_gitleaks_toml(toml_text: &str) -> Self {
        Self {
            secrets: SecretDetector::from_gitleaks_toml(toml_text),
            cache: cache::DetectionCache::new(),
        }
    }

    /// 合并后的命中区间。长会话每轮重发全部历史，同一段文本的检测结果按原文缓存。
    ///
    /// 缓存属于这个 Filter 实例，规则在实例创建后不再变化，所以键里不需要规则版本。
    fn detect(&self, text: &str) -> std::sync::Arc<[Span]> {
        self.cache.get_or_detect(text, || {
            let mut spans = pii::detect(text);
            spans.extend(self.secrets.detect(text));
            merge_spans(spans)
        })
    }

    /// 检测并脱敏文本，命中处替换为 `placeholders` 里对应的占位符
    #[cfg(test)]
    pub fn redact(&self, text: &str, placeholders: &Placeholders) -> RedactResult {
        let (redacted, count) = self.redact_cow(text, placeholders);
        RedactResult {
            redacted: redacted.into_owned(),
            count,
        }
    }

    /// 检测并脱敏文本，没有命中时不复制原文
    fn redact_cow<'t>(
        &self,
        text: &'t str,
        placeholders: &Placeholders,
    ) -> (std::borrow::Cow<'t, str>, usize) {
        let merged = self.detect(text);

        // merged 已按起点升序且互不重叠，单遍重建。替换结果和原文一样的命中（例如第二遍过滤时
        // 「密码是 [密钥]」里的 [密钥] 又被上下文规则按密钥命中）不改写、不计数。只比较这一处
        // 替换结果，不按「像占位符」豁免：`password = [EMAIL]` 照样换成 [SECRET]。
        let mut out: Option<String> = None;
        let mut prev = 0;
        let mut count = 0;
        for s in merged.iter() {
            if placeholders.label(s.kind) == &text[s.start..s.end] {
                continue;
            }
            let out = out.get_or_insert_with(|| String::with_capacity(text.len()));
            out.push_str(&text[prev..s.start]);
            out.push_str(placeholders.label(s.kind));
            prev = s.end;
            count += 1;
        }
        match out {
            None => (std::borrow::Cow::Borrowed(text), 0),
            Some(mut out) => {
                out.push_str(&text[prev..]);
                (std::borrow::Cow::Owned(out), count)
            }
        }
    }
}

/// 丢弃无效区间并合并相交区间，确保每个命中的字节都被替换。
fn merge_spans(spans: Vec<Span>) -> Vec<Span> {
    let mut valid: Vec<Span> = spans.into_iter().filter(|s| s.start < s.end).collect();
    valid.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut merged: Vec<Span> = Vec::with_capacity(valid.len());
    for s in valid {
        if let Some(last) = merged.last_mut() {
            if s.start < last.end {
                last.end = last.end.max(s.end);
                continue;
            }
        }
        merged.push(s);
    }
    merged
}

static FILTER: Lazy<Filter> = Lazy::new(|| {
    let filter = Filter::from_gitleaks_toml(GITLEAKS_TOML);
    let (rules, skipped) = filter.secrets.stats();
    log::info!("[PrivacyFilter] Loaded {rules} gitleaks rules ({skipped} skipped)");
    filter
});

/// 全局过滤器（首次访问时编译规则）
pub fn global() -> &'static Filter {
    &FILTER
}

/// 内容派生标识（Copilot 确定性请求 ID、OpenCode 会话指纹）用的进程内随机密钥，不落盘、不外发。
static FINGERPRINT_KEY: Lazy<[u8; 32]> = Lazy::new(|| {
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key
});

/// 带密钥的内容摘要：`HMAC-SHA256(进程内随机密钥, parts 依次拼接)`。
///
/// 隐私过滤打开时，发往上游的内容派生标识改用它。普通 SHA-256 的输入里只要还有一处没被遮盖的
/// 短口令（转换后才拼出上下文、孤儿工具结果降级等），上游就能拿模板穷举反推；上游不知道这个
/// 密钥，就没法离线验证猜测。同一进程内相同输入结果相同，重启后会变。
pub fn keyed_digest(parts: &[&[u8]]) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(FINGERPRINT_KEY.as_slice())
        .expect("HMAC accepts any key length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// 预热：提前编译规则，避免开关打开后的第一个请求承担编译耗时
pub fn warm_up() {
    Lazy::force(&FILTER);
}

// ---------------------------------------------------------------------------
// 请求体改写
// ---------------------------------------------------------------------------

struct Redactor<'a> {
    filter: &'a Filter,
    placeholders: &'a Placeholders,
    /// 是否同时脱敏工具结果
    tool_results: bool,
    /// 见 [`RedactOptions::embedded_custom_tools`]
    embedded_custom_tools: bool,
}

/// 请求体脱敏选项
#[derive(Debug, Clone, Copy, Default)]
pub struct RedactOptions {
    /// 同时脱敏工具结果
    pub tool_results: bool,
    /// 请求体由 CC Switch 从 Responses 转成了 Chat / Anthropic：工具说明里可能嵌着它生成的
    /// custom tool 原定义（含 grammar），只处理定义里的说明文字。只有转发器出口那一遍、且确实
    /// 做过这种转换时才打开；客户端发来的说明一律按普通文本处理。
    pub embedded_custom_tools: bool,
}

/// 脱敏请求体里的文本内容，返回命中数。
///
/// 覆盖 Anthropic Messages / OpenAI Chat Completions / OpenAI Responses / Gemini
/// （含 countTokens 的 `generateContentRequest` 包装）四种格式，图片生成的 `prompt`，
/// Alpha Search 的搜索词，以及工具定义的说明文字。只改写用户输入、系统提示和说明文字，
/// 选项见 [`RedactOptions`]。
pub fn redact_request_body_with(
    body: &mut Value,
    placeholders: &Placeholders,
    options: RedactOptions,
) -> usize {
    let r = Redactor {
        filter: global(),
        placeholders,
        tool_results: options.tool_results,
        embedded_custom_tools: options.embedded_custom_tools,
    };
    redact_body(&r, body)
}

/// 同 [`redact_request_body_with`]，不识别嵌入的 custom tool 定义（测试用）
#[cfg(test)]
pub fn redact_request_body(
    body: &mut Value,
    placeholders: &Placeholders,
    tool_results: bool,
) -> usize {
    redact_request_body_with(
        body,
        placeholders,
        RedactOptions {
            tool_results,
            embedded_custom_tools: false,
        },
    )
}

fn redact_body(r: &Redactor, body: &mut Value) -> usize {
    let Some(obj) = body.as_object_mut() else {
        return 0;
    };
    let mut count = 0;

    // Gemini countTokens：完整的 GenerateContentRequest 包在这个字段里
    for key in ["generateContentRequest", "generate_content_request"] {
        if let Some(inner) = obj.get_mut(key) {
            count += redact_body(r, inner);
        }
    }

    // Anthropic: system（字符串或 text 块数组）
    if let Some(system) = obj.get_mut("system") {
        count += redact_content(r, system);
    }
    // Anthropic / OpenAI Chat: messages[].content
    if let Some(Value::Array(messages)) = obj.get_mut("messages") {
        for msg in messages {
            count += redact_message(r, msg);
        }
    }
    // OpenAI Responses: instructions + input
    if let Some(instructions) = obj.get_mut("instructions") {
        count += redact_string(r, instructions);
    }
    if let Some(input) = obj.get_mut("input") {
        count += redact_responses_input(r, input);
    }
    // Gemini: contents[].parts + systemInstruction.parts
    if let Some(Value::Array(contents)) = obj.get_mut("contents") {
        for content in contents {
            count += redact_gemini_content(r, content);
        }
    }
    for key in ["systemInstruction", "system_instruction"] {
        if let Some(si) = obj.get_mut(key) {
            count += redact_gemini_content(r, si);
        }
    }
    // 图片生成 / 编辑
    if let Some(prompt) = obj.get_mut("prompt") {
        count += redact_string(r, prompt);
    }
    // 工具定义的说明文字（MCP / 用户自定义工具里可能写着地址、账号）
    if let Some(Value::Array(tools)) = obj.get_mut("tools") {
        for tool in tools {
            count += redact_tool_descriptions(r, tool);
        }
    }
    // Codex Alpha Search：`commands.<命令>[].q`
    if let Some(Value::Object(commands)) = obj.get_mut("commands") {
        for command in commands.values_mut() {
            if let Value::Array(calls) = command {
                for call in calls {
                    if let Some(q) = call.get_mut("q") {
                        count += redact_string(r, q);
                    }
                }
            }
        }
    }
    count
}

/// 工具定义里所有键名为 `description` 的字符串：工具本身、函数声明和参数 schema 各层。
/// 名称、类型、`enum` / `default` 等会约束模型输出的值不改，否则模型照着占位符调用会出错。
fn redact_tool_descriptions(r: &Redactor, v: &mut Value) -> usize {
    match v {
        Value::Object(fields) => fields
            .iter_mut()
            .map(|(key, field)| match field {
                Value::String(_) if key == "description" => redact_description(r, field),
                // 「名称 → schema」字典：键是参数名或定义名（可能恰好叫 default / enum），值都是 schema
                Value::Object(named) if is_schema_map_key(key) => named
                    .values_mut()
                    .map(|schema| redact_tool_descriptions(r, schema))
                    .sum(),
                Value::Object(_) | Value::Array(_) if !is_schema_value_key(key) => {
                    redact_tool_descriptions(r, field)
                }
                _ => 0,
            })
            .sum(),
        Value::Array(items) => items
            .iter_mut()
            .map(|i| redact_tool_descriptions(r, i))
            .sum(),
        _ => 0,
    }
}

/// schema 里装的是取值而不是说明的键，不往里递归
fn is_schema_value_key(key: &str) -> bool {
    matches!(key, "enum" | "const" | "default" | "examples" | "example")
}

/// 值是「名称 → schema」字典的 JSON Schema 关键字
fn is_schema_map_key(key: &str) -> bool {
    matches!(
        key,
        "properties"
            | "patternProperties"
            | "dependentSchemas"
            | "dependencies"
            | "$defs"
            | "definitions"
    )
}

/// 工具的 `description`。Responses custom tool 转成 Chat / Anthropic 时，说明里嵌着完整的原工具
/// 定义（含 grammar 等约束），只处理定义里的说明文字，约束原样保留。只在
/// [`RedactOptions::embedded_custom_tools`] 打开时识别；这些定义在客户端那一遍已按结构处理过。
fn redact_description(r: &Redactor, v: &mut Value) -> usize {
    use crate::proxy::providers::transform_codex_chat::{
        parse_custom_tool_description, responses_custom_tool_description,
    };
    if r.embedded_custom_tools {
        if let Value::String(text) = v {
            if let Some(mut definition) = parse_custom_tool_description(text) {
                let count = redact_tool_descriptions(r, &mut definition);
                if count > 0 {
                    *text = responses_custom_tool_description(&definition);
                }
                return count;
            }
        }
    }
    redact_string(r, v)
}

fn redact_string(r: &Redactor, v: &mut Value) -> usize {
    let Value::String(s) = v else {
        return 0;
    };
    let (redacted, count) = r.filter.redact_cow(s, r.placeholders);
    if let std::borrow::Cow::Owned(redacted) = redacted {
        *s = redacted;
    }
    count
}

/// 消息是否出自模型（assistant / Gemini 的 model）。未知角色按用户文本处理，宁可多脱敏。
fn is_model_message(message: &Value) -> bool {
    matches!(
        message.get("role").and_then(Value::as_str),
        Some("assistant" | "model")
    )
}

/// Anthropic / OpenAI Chat 的一条消息。OpenAI Chat 的工具结果是 tool / function 角色的消息。
fn redact_message(r: &Redactor, msg: &mut Value) -> usize {
    let is_tool_result = matches!(
        msg.get("role").and_then(Value::as_str),
        Some("tool" | "function")
    );
    if is_model_message(msg) || (is_tool_result && !r.tool_results) {
        return 0;
    }
    msg.get_mut("content").map_or(0, |c| redact_content(r, c))
}

/// 内容字段：字符串，或内容块数组（Anthropic / OpenAI Chat / Responses 共用）。
fn redact_content(r: &Redactor, content: &mut Value) -> usize {
    match content {
        Value::String(_) => redact_string(r, content),
        Value::Array(blocks) => blocks.iter_mut().map(|b| redact_block(r, b)).sum(),
        _ => 0,
    }
}

/// 单个内容块。thinking / redacted_thinking（带签名）、tool_use、image 等一律不碰。
fn redact_block(r: &Redactor, block: &mut Value) -> usize {
    let Some(obj) = block.as_object_mut() else {
        return 0;
    };
    match obj.get("type").and_then(Value::as_str) {
        Some("text" | "input_text" | "output_text") => {
            obj.get_mut("text").map_or(0, |t| redact_string(r, t))
        }
        Some("tool_result") if r.tool_results => {
            obj.get_mut("content").map_or(0, |c| redact_content(r, c))
        }
        _ => 0,
    }
}

/// Responses API 的 input：字符串，或 item 数组。
fn redact_responses_input(r: &Redactor, input: &mut Value) -> usize {
    match input {
        Value::String(_) => redact_string(r, input),
        Value::Array(items) => items
            .iter_mut()
            .map(|item| {
                if is_model_message(item) {
                    return 0;
                }
                let Some(obj) = item.as_object_mut() else {
                    return 0;
                };
                match obj.get("type").and_then(Value::as_str) {
                    // 无 type 的 {role, content} 也是 message
                    None | Some("message") => {
                        obj.get_mut("content").map_or(0, |c| redact_content(r, c))
                    }
                    // CC Switch 自己包装的压缩摘要只是 base64，上游一解就是明文。
                    // 转成 Chat / Anthropic 时它会作为用户文字回放，这里保持同样的处理。
                    Some("compaction" | "compaction_summary" | "context_compaction") => {
                        redact_own_compaction_summary(r, obj)
                    }
                    Some("function_call_output" | "custom_tool_call_output") if r.tool_results => {
                        obj.get_mut("output").map_or(0, |o| redact_content(r, o))
                    }
                    // 写在 input 里的动态工具定义：说明文字按工具定义处理
                    Some("additional_tools") => obj
                        .get_mut("tools")
                        .map_or(0, |t| redact_tool_descriptions(r, t)),
                    // 工具搜索结果：带出来的工具定义同上，结果文本跟随子开关
                    Some("tool_search_output") => {
                        let tools = obj
                            .get_mut("tools")
                            .map_or(0, |t| redact_tool_descriptions(r, t));
                        let output = if r.tool_results {
                            obj.get_mut("output").map_or(0, |o| redact_content(r, o))
                        } else {
                            0
                        };
                        tools + output
                    }
                    _ => 0,
                }
            })
            .sum(),
        _ => 0,
    }
}

/// 解开 CC Switch 的压缩摘要包装，脱敏后重新包装。别家签发的真密文不碰。
fn redact_own_compaction_summary(r: &Redactor, item: &mut serde_json::Map<String, Value>) -> usize {
    use crate::proxy::providers::codex_compaction::{
        decode_compaction_summary, encode_compaction_summary,
    };
    let Some(Value::String(encrypted)) = item.get_mut("encrypted_content") else {
        return 0;
    };
    let Some(summary) = decode_compaction_summary(encrypted) else {
        return 0;
    };
    let (redacted, count) = r.filter.redact_cow(&summary, r.placeholders);
    if count > 0 {
        *encrypted = encode_compaction_summary(&redacted);
    }
    count
}

/// Gemini Content：`{ parts: [{ text } | { functionResponse }] }`。
/// 带 thought / thoughtSignature 的 part 不碰。
fn redact_gemini_content(r: &Redactor, content: &mut Value) -> usize {
    if is_model_message(content) {
        return 0;
    }
    let Some(Value::Array(parts)) = content.get_mut("parts") else {
        return 0;
    };
    parts
        .iter_mut()
        .map(|part| {
            let Some(obj) = part.as_object_mut() else {
                return 0;
            };
            let is_thought = obj.get("thought").and_then(Value::as_bool) == Some(true)
                || obj.contains_key("thoughtSignature")
                || obj.contains_key("thought_signature");
            if is_thought {
                return 0;
            }
            let text = obj.get_mut("text").map_or(0, |t| redact_string(r, t));
            if !r.tool_results {
                return text;
            }
            let response_key = if obj.contains_key("functionResponse") {
                "functionResponse"
            } else {
                "function_response"
            };
            text + obj
                .get_mut(response_key)
                .and_then(|f| f.get_mut("response"))
                .map_or(0, |v| redact_json_strings(r, v))
        })
        .sum()
}

/// `functionResponse.response` 是任意 JSON：逐个字符串叶子脱敏，键名不动。
/// 媒体在同级的 `functionResponse.parts` 里，不经过这里。
fn redact_json_strings(r: &Redactor, v: &mut Value) -> usize {
    match v {
        Value::String(_) => redact_string(r, v),
        Value::Array(items) => items.iter_mut().map(|i| redact_json_strings(r, i)).sum(),
        Value::Object(fields) => fields.values_mut().map(|f| redact_json_strings(r, f)).sum(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests;

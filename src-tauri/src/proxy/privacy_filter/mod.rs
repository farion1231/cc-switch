//! 隐私过滤器：请求进入上游 LLM 前脱敏 PII 与密钥
//!
//! 移植自 [packyme/privacy-filter](https://github.com/packyme/privacy-filter)（MIT License,
//! Copyright (c) 2026 PackyMe）。纯正则、无模型、O(n)。
//!
//! 两层检测：
//! - 结构化 PII（`pii`）：邮箱、手机号、身份证、银行卡（Luhn 校验）、IPv4
//! - 密钥 / 凭证（`secrets`）：gitleaks 规则集（关键词预筛）+ 上下文口令 + 香农熵兜底
//!
//! 各层产出 `(start, end, label)` 区间 → 合并去重叠 → 单遍重建文本。占位符不可逆。
//!
//! 代理侧只改写请求体里的文本内容（system / messages / input / contents 等），
//! 不碰 thinking 块、签名、工具调用参数和图片等二进制内容，避免破坏协议语义。

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

/// 一次脱敏的结果
#[derive(Debug, Clone)]
pub struct RedactResult {
    pub redacted: String,
    pub count: usize,
}

/// 编译好的过滤器。创建后只读，可并发安全地复用。
pub struct Filter {
    secrets: SecretDetector,
}

impl Filter {
    /// 用 gitleaks TOML 规则文本创建过滤器；规则为空或解析失败时退回内置兜底规则。
    fn from_gitleaks_toml(toml_text: &str) -> Self {
        Self {
            secrets: SecretDetector::from_gitleaks_toml(toml_text),
        }
    }

    /// 检测并脱敏文本，命中处替换为 `placeholders` 里对应的占位符
    pub fn redact(&self, text: &str, placeholders: &Placeholders) -> RedactResult {
        let mut spans = pii::detect(text);
        spans.extend(self.secrets.detect(text));

        let merged = merge_spans(spans);
        if merged.is_empty() {
            return RedactResult {
                redacted: text.to_string(),
                count: 0,
            };
        }

        // merged 已按起点升序且互不重叠，单遍重建
        let mut out = String::with_capacity(text.len());
        let mut prev = 0;
        for s in &merged {
            out.push_str(&text[prev..s.start]);
            out.push_str(placeholders.label(s.kind));
            prev = s.end;
        }
        out.push_str(&text[prev..]);
        RedactResult {
            redacted: out,
            count: merged.len(),
        }
    }
}

/// 丢弃无效与重叠区间：按起点升序、同起点取更长者，贪心保留互不重叠的区间。
fn merge_spans(spans: Vec<Span>) -> Vec<Span> {
    let mut valid: Vec<Span> = spans.into_iter().filter(|s| s.start < s.end).collect();
    valid.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut merged = Vec::with_capacity(valid.len());
    let mut last_end = 0;
    for s in valid {
        if merged.is_empty() || s.start >= last_end {
            last_end = s.end;
            merged.push(s);
        }
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
}

/// 脱敏请求体里的文本内容，返回命中数。
///
/// 覆盖 Anthropic Messages / OpenAI Chat Completions / OpenAI Responses / Gemini
/// 四种格式，以及图片生成的 `prompt`。只改写纯文本字段（含工具结果）。
pub fn redact_request_body(body: &mut Value, placeholders: &Placeholders) -> usize {
    let r = Redactor {
        filter: global(),
        placeholders,
    };
    let r = &r;
    let Some(obj) = body.as_object_mut() else {
        return 0;
    };
    let mut count = 0;

    // Anthropic: system（字符串或 text 块数组）
    if let Some(system) = obj.get_mut("system") {
        count += redact_content(r, system);
    }
    // Anthropic / OpenAI Chat: messages[].content
    if let Some(Value::Array(messages)) = obj.get_mut("messages") {
        for msg in messages {
            if let Some(content) = msg.get_mut("content") {
                count += redact_content(r, content);
            }
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
    count
}

fn redact_string(r: &Redactor, v: &mut Value) -> usize {
    let Value::String(s) = v else {
        return 0;
    };
    let res = r.filter.redact(s, r.placeholders);
    if res.count > 0 {
        *s = res.redacted;
    }
    res.count
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
        Some("tool_result") => obj.get_mut("content").map_or(0, |c| redact_content(r, c)),
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
                let Some(obj) = item.as_object_mut() else {
                    return 0;
                };
                match obj.get("type").and_then(Value::as_str) {
                    // 无 type 的 {role, content} 也是 message
                    None | Some("message") => {
                        obj.get_mut("content").map_or(0, |c| redact_content(r, c))
                    }
                    Some("function_call_output" | "custom_tool_call_output") => {
                        obj.get_mut("output").map_or(0, |o| redact_content(r, o))
                    }
                    _ => 0,
                }
            })
            .sum(),
        _ => 0,
    }
}

/// Gemini Content：`{ parts: [{ text }] }`。带 thought / thoughtSignature 的 part 不碰。
fn redact_gemini_content(r: &Redactor, content: &mut Value) -> usize {
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
            obj.get_mut("text").map_or(0, |t| redact_string(r, t))
        })
        .sum()
}

#[cfg(test)]
mod tests;

//! 出站可逆脱敏
//!
//! 把请求体里的敏感值替换成携带类型的占位符（形如 `{{PHONE_k3f001}}`），并在响应
//! 侧换回原文。与遮盖式脱敏（把值替换成等长 `*`）的区别是**可逆**：
//!
//! - 占位符带类型，模型知道「这里是个手机号」，能在回答里引用它；
//! - 同一实体在一次会话里始终映射到同一个占位符，多轮推理不串号；
//! - 响应回来时占位符被换回原文，用户侧无感。
//!
//! ## 跨轮一致性怎么成立
//!
//! [`MaskSession`] 是**每请求新建**的——常驻映射表会随会话无限增长，代理进程扛不住。
//! 跨轮一致靠的是确定性：占位符 = 会话盐（由 session id 派生）+ 文档序计数，而聊天
//! 类请求每轮都会带上完整历史，历史是追加的，遍历顺序稳定，于是同一实体在第 1 轮和
//! 第 10 轮会算出同一个占位符。
//!
//! Claude Code / Codex / Grok Build 都会带稳定的 session id（见 [`super::session`]）。
//! 客户端不提供时 session id 是每请求新生成的，跨轮占位符名会变——但每一轮内部仍然
//! 自洽（模型看到的是一套完整且一致的符号），不影响正确性。
//!
//! ## 为什么遮盖式不可逆
//!
//! `13800138000` 一旦变成 `***********`，原值在管线里不再有任何载体，还原在原理
//! 上就不成立；且两个不同的手机号会塌缩成同一串星号，模型会把它们当成同一个东西。
//!
//! ## 转义
//!
//! 占位符只含 `{}`、`_`、ASCII 字母数字，在 JSON 字符串里不会被转义，所以流式
//! 响应上可以直接做文本替换。**但还原回去的原文不一定 JSON 安全**（PEM 私钥含
//! 换行、连接串可能含引号），因此：
//!
//! - 流式路径按 JSON 字符串字面量替换（[`MaskSession::restore_stream_text`]）；
//! - 非流式路径解析 JSON 后在字符串值里替换，由 serde_json 负责转义
//!   （[`MaskSession::restore_json_value`]）。
//!
//! 写反了会产出非法 JSON，客户端直接解析失败——这是本模块最容易踩的坑。

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

use super::types::{MaskRuleKind, OutboundMaskConfig};

/// 占位符最长长度，用于流式还原时限制回退缓冲。
///
/// 形如 `{{CONNSTR_k3f001}}`：4 个大括号 + 类型名 + `_` + 6 位 id。类型名最长
/// 为 `CONNSTR`（7），留足余量取 48。超过这个长度的 `{{` 开头一定不是占位符，
/// 必须放行，否则用户文本里一个字面 `{{` 会把流永久卡住。
const MAX_PLACEHOLDER_LEN: usize = 48;

/// 不参与脱敏的结构性字段。
///
/// 这些键的值是协议字段而非用户内容，打码会直接破坏请求（例如把 `model` 换成
/// 占位符，上游会返回模型不存在）。
const SKIP_KEYS: &[&str] = &[
    "model",
    "type",
    "role",
    "stop_reason",
    "finish_reason",
    "object",
    "id",
];

/// 内置规则定义。
pub struct BuiltinRule {
    /// 稳定 id，前端用它做开关；写进配置后不可改名。
    pub id: &'static str,
    /// 占位符类型前缀，必须是大写字母/数字，且不含 `_`（`_` 是 id 分隔符）。
    pub label: &'static str,
    pattern: &'static str,
    /// 是否默认开启。误报代价高的规则默认关闭。
    pub default_on: bool,
}

/// 内置规则表。
///
/// 顺序即优先级：重叠命中时长匹配优先，长度相同则表中靠前的规则优先。把范围大的
/// 规则（PEM 块、连接串）放在前面，避免被 email / 私网 IP 这类子串规则切碎。
pub static BUILTIN_RULES: &[BuiltinRule] = &[
    BuiltinRule {
        id: "pem_private_key",
        label: "PRIVKEY",
        // `[\s\S]` 而非 `.`：私钥块跨行，默认 `.` 不匹配换行。
        pattern: r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
        default_on: true,
    },
    BuiltinRule {
        id: "conn_string",
        label: "CONNSTR",
        // scheme://user:pass@host —— 只认带密码的形式，避免把普通 URL 全部吃掉。
        pattern: r"[a-zA-Z][a-zA-Z0-9+.\-]*://[^\s:/@]+:[^\s:/@]+@[^\s/?#]+",
        default_on: true,
    },
    BuiltinRule {
        id: "jwt",
        label: "JWT",
        pattern: r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
        default_on: true,
    },
    BuiltinRule {
        id: "api_key",
        label: "APIKEY",
        // 只收敛前缀明确、几乎不可能误报的厂商密钥形态。
        pattern: r"\b(?:sk-[A-Za-z0-9_\-]{20,}|ghp_[A-Za-z0-9]{36}|gho_[A-Za-z0-9]{36}|ghs_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{50,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9\-]{10,}|AIza[A-Za-z0-9_\-]{35})\b",
        default_on: true,
    },
    BuiltinRule {
        id: "email",
        label: "EMAIL",
        pattern: r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b",
        default_on: true,
    },
    BuiltinRule {
        id: "cn_id_card",
        label: "IDCARD",
        // 18 位二代身份证：6 位地区 + 8 位生日 + 3 位顺序 + 1 位校验。
        pattern: r"\b[1-9]\d{5}(?:19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\d{3}[0-9Xx]\b",
        default_on: true,
    },
    BuiltinRule {
        id: "cn_phone",
        label: "PHONE",
        // `\b` 保证不在更长的数字串内部命中（身份证、订单号等）。
        pattern: r"\b1[3-9]\d{9}\b",
        default_on: true,
    },
    BuiltinRule {
        id: "private_ip",
        label: "PRIVIP",
        pattern: r"\b(?:10\.(?:\d{1,3}\.){2}\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3})\b",
        default_on: true,
    },
    BuiltinRule {
        id: "bank_card",
        label: "BANKCARD",
        // 纯数字串误报率高（订单号、时间戳拼接都可能命中），默认关闭。
        pattern: r"\b(?:4\d{12}(?:\d{3})?|5[1-5]\d{14}|6(?:011|5\d{2})\d{12}|62\d{14,17})\b",
        default_on: false,
    },
];

/// 编译后的内置正则，进程内只编译一次。
static BUILTIN_REGEXES: Lazy<Vec<Regex>> = Lazy::new(|| {
    BUILTIN_RULES
        .iter()
        .map(|r| {
            Regex::new(r.pattern).unwrap_or_else(|e| panic!("内置脱敏规则 {} 正则非法: {e}", r.id))
        })
        .collect()
});

/// 匹配占位符本身，用于还原。
static PLACEHOLDER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{[A-Z][A-Z0-9]*_[a-z0-9]{4,12}\}\}").expect("占位符正则非法"));

/// 一次脱敏的统计结果。日志只输出统计，永不输出原值。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MaskStats {
    /// 本次替换总次数。
    pub total: usize,
    /// 按规则 id 计数。
    pub per_rule: HashMap<String, usize>,
}

impl MaskStats {
    fn record(&mut self, rule_id: &str) {
        self.total += 1;
        *self.per_rule.entry(rule_id.to_string()).or_insert(0) += 1;
    }

    /// 供日志使用的紧凑摘要，形如 `3 replacements (email=2, cn_phone=1)`。
    pub fn summary(&self) -> String {
        if self.total == 0 {
            return "no replacements".to_string();
        }
        let mut parts: Vec<String> = self
            .per_rule
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        parts.sort();
        format!("{} replacements ({})", self.total, parts.join(", "))
    }
}

/// 编译后的规则，脱敏时按序尝试。
struct CompiledRule {
    id: String,
    label: String,
    regex: Regex,
}

/// 按配置编译出生效的规则集。
///
/// 自定义正则由用户输入，非法时不静默跳过——返回 `Err` 交给调用方按 `on_error`
/// 策略处理，否则用户会以为规则生效了而实际在裸奔。
fn compile_rules(config: &OutboundMaskConfig) -> Result<Vec<CompiledRule>, String> {
    let mut out = Vec::new();

    for (idx, rule) in BUILTIN_RULES.iter().enumerate() {
        let on = config
            .builtin
            .get(rule.id)
            .copied()
            .unwrap_or(rule.default_on);
        if on {
            out.push(CompiledRule {
                id: rule.id.to_string(),
                label: rule.label.to_string(),
                regex: BUILTIN_REGEXES[idx].clone(),
            });
        }
    }

    for (idx, rule) in config.custom_rules.iter().enumerate() {
        if !rule.enabled {
            continue;
        }
        if rule.pattern.is_empty() {
            continue;
        }
        let label = sanitize_label(&rule.label, idx);
        let regex = match rule.kind {
            MaskRuleKind::Regex => Regex::new(&rule.pattern)
                .map_err(|e| format!("自定义规则 #{} 正则非法: {e}", idx + 1))?,
            // 字面量走 escape，用户填的 `a.b` 不应该匹配 `axb`。
            MaskRuleKind::Literal => Regex::new(&regex::escape(&rule.pattern))
                .map_err(|e| format!("自定义规则 #{} 构造失败: {e}", idx + 1))?,
        };
        out.push(CompiledRule {
            id: format!("custom_{}", idx + 1),
            label,
            regex,
        });
    }

    Ok(out)
}

/// 把用户填的类型名规整成合法的占位符前缀。
///
/// 占位符格式依赖 `[A-Z][A-Z0-9]*_`，所以非法字符必须剔除；空值回退到
/// `CUSTOM{n}`，保证任何输入都能产出可还原的占位符。
fn sanitize_label(raw: &str, idx: usize) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    if cleaned.is_empty() || !cleaned.starts_with(|c: char| c.is_ascii_alphabetic()) {
        format!("CUSTOM{}", idx + 1)
    } else {
        cleaned
    }
}

/// 一次会话的占位符映射表。
///
/// 请求侧写入、响应侧读取，两侧通过 `Arc` 共享同一实例，所以内部用 `Mutex`
/// 而不是要求调用方持有可变引用。
#[derive(Debug)]
pub struct MaskSession {
    inner: Mutex<MaskSessionInner>,
}

#[derive(Debug)]
struct MaskSessionInner {
    /// 原文 → 占位符。保证同一实体多轮映射一致。
    forward: HashMap<String, String>,
    /// 占位符 → 原文。
    reverse: HashMap<String, String>,
    counter: u32,
    /// 会话盐，让不同会话产出不同占位符，避免跨会话串味。
    salt: String,
}

impl MaskSession {
    /// 用会话 id 派生盐。
    pub fn new(session_id: &str) -> Self {
        Self {
            inner: Mutex::new(MaskSessionInner {
                forward: HashMap::new(),
                reverse: HashMap::new(),
                counter: 0,
                salt: derive_salt(session_id),
            }),
        }
    }

    /// 映射表是否为空。为空说明本次请求没打过码，响应侧可以完全跳过还原。
    pub fn is_empty(&self) -> bool {
        self.inner
            .lock()
            .map(|g| g.reverse.is_empty())
            .unwrap_or(true)
    }

    /// 取得某个原文对应的占位符，不存在则分配一个。
    fn placeholder_for(&self, original: &str, label: &str) -> String {
        let mut g = match self.inner.lock() {
            Ok(g) => g,
            // 锁中毒说明别处 panic 过；此时宁可不打码也不能 panic 在代理热路径上，
            // 返回原文由调用方按原样发送（fail-open 仅限此极端分支）。
            Err(_) => return original.to_string(),
        };
        if let Some(existing) = g.forward.get(original) {
            return existing.clone();
        }
        g.counter += 1;
        let placeholder = format!("{{{{{}_{}{:03}}}}}", label, g.salt, g.counter);
        g.forward.insert(original.to_string(), placeholder.clone());
        g.reverse.insert(placeholder.clone(), original.to_string());
        placeholder
    }

    /// 查占位符对应的原文。
    fn original_for(&self, placeholder: &str) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.reverse.get(placeholder).cloned())
    }

    /// 还原一段**位于 JSON 字符串字面量内部**的文本（流式路径）。
    ///
    /// 原文按 JSON 字符串规则转义后代入，因此含换行的 PEM 私钥、含引号的连接串
    /// 都不会破坏外层 JSON。
    pub fn restore_stream_text(&self, text: &str) -> String {
        self.replace_placeholders(text, true)
    }

    /// 还原一段裸文本（不在 JSON 字符串内部），原文按原样代入。
    pub fn restore_plain_text(&self, text: &str) -> String {
        self.replace_placeholders(text, false)
    }

    fn replace_placeholders(&self, text: &str, json_escape: bool) -> String {
        if !text.contains("{{") {
            return text.to_string();
        }
        PLACEHOLDER_RE
            .replace_all(text, |caps: &regex::Captures| {
                let ph = &caps[0];
                match self.original_for(ph) {
                    // 模型可能凭空编出一个没见过的占位符，原样留着比错误还原安全。
                    None => ph.to_string(),
                    Some(original) if json_escape => json_escape_inner(&original),
                    Some(original) => original,
                }
            })
            .into_owned()
    }

    /// 在解析好的 JSON 值里递归还原（非流式路径）。
    ///
    /// 只动字符串值，转义交给 serde_json 序列化时处理。
    pub fn restore_json_value(&self, value: &mut Value) {
        match value {
            // 提前判掉不含占位符的字符串，避免整棵树都走一遍正则
            Value::String(s) if s.contains("{{") => {
                *s = self.restore_plain_text(s);
            }
            Value::Array(arr) => {
                for v in arr.iter_mut() {
                    self.restore_json_value(v);
                }
            }
            Value::Object(map) => {
                for (_, v) in map.iter_mut() {
                    self.restore_json_value(v);
                }
            }
            _ => {}
        }
    }
}

/// 由会话 id 派生 3 位小写字母数字盐。
fn derive_salt(session_id: &str) -> String {
    use sha2::{Digest, Sha256};
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let digest = Sha256::digest(session_id.as_bytes());
    digest
        .iter()
        .take(3)
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect()
}

/// 把字符串按 JSON 规则转义，但不带外层引号。
fn json_escape_inner(s: &str) -> String {
    let quoted = serde_json::Value::String(s.to_string()).to_string();
    // to_string() 一定产出带引号的形式，去掉首尾即可。
    quoted[1..quoted.len() - 1].to_string()
}

/// 对请求体做脱敏，就地修改。
///
/// 返回统计信息；`Err` 表示自定义规则非法，由调用方按 `on_error` 决定放行还是阻断。
pub fn mask_request_body(
    body: &mut Value,
    config: &OutboundMaskConfig,
    session: &MaskSession,
) -> Result<MaskStats, String> {
    let mut stats = MaskStats::default();
    if !config.enabled {
        return Ok(stats);
    }
    let rules = compile_rules(config)?;
    if rules.is_empty() {
        return Ok(stats);
    }
    mask_value(body, &rules, session, &mut stats, None);
    Ok(stats)
}

fn mask_value(
    value: &mut Value,
    rules: &[CompiledRule],
    session: &MaskSession,
    stats: &mut MaskStats,
    key: Option<&str>,
) {
    match value {
        Value::String(s) => {
            if key.is_some_and(|k| SKIP_KEYS.contains(&k)) {
                return;
            }
            if let Some(masked) = mask_text(s, rules, session, stats) {
                *s = masked;
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                // 数组元素继承父键，使 `messages[].content` 这类结构也受 SKIP_KEYS 保护。
                mask_value(v, rules, session, stats, key);
            }
        }
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let k = k.clone();
                mask_value(v, rules, session, stats, Some(&k));
            }
        }
        _ => {}
    }
}

/// 命中的区间。
struct Span {
    start: usize,
    end: usize,
    rule_idx: usize,
}

/// 对一段文本做脱敏，无命中时返回 `None` 以免无谓分配。
fn mask_text(
    text: &str,
    rules: &[CompiledRule],
    session: &MaskSession,
    stats: &mut MaskStats,
) -> Option<String> {
    let mut spans: Vec<Span> = Vec::new();
    for (rule_idx, rule) in rules.iter().enumerate() {
        for m in rule.regex.find_iter(text) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                rule_idx,
            });
        }
    }
    if spans.is_empty() {
        return None;
    }

    // 长匹配优先；同长则规则表靠前的优先。排序后线性扫描剔除重叠。
    spans.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.rule_idx.cmp(&b.rule_idx))
    });

    let mut kept: Vec<&Span> = Vec::new();
    let mut cursor = 0usize;
    for span in &spans {
        if span.start >= cursor {
            kept.push(span);
            cursor = span.end;
        }
    }

    // 从右往左替换，避免前面的替换移动后面区间的下标。
    let mut out = text.to_string();
    for span in kept.iter().rev() {
        let rule = &rules[span.rule_idx];
        let original = &text[span.start..span.end];
        let placeholder = session.placeholder_for(original, &rule.label);
        out.replace_range(span.start..span.end, &placeholder);
        stats.record(&rule.id);
    }
    Some(out)
}

/// 流式还原器。
///
/// SSE 下一个占位符可能被切在两个 chunk 里（`{{PHO` / `NE_k3f001}}`），直接逐块
/// 替换会漏掉跨界的那个。本结构把可能是占位符开头的尾巴扣下来，等下一块拼上再处理。
///
/// 这与 [`super::sse::append_utf8_safe`] 处理多字节字符跨块截断是同一个模式。
#[derive(Debug, Default)]
pub struct StreamRestorer {
    pending: String,
}

impl StreamRestorer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 吃进一块文本，吐出可以安全下发的部分。
    pub fn push(&mut self, chunk: &str, session: &MaskSession) -> String {
        self.pending.push_str(chunk);
        let restored = session.restore_stream_text(&self.pending);
        match holdback_index(&restored) {
            Some(idx) => {
                let out = restored[..idx].to_string();
                self.pending = restored[idx..].to_string();
                out
            }
            None => {
                self.pending.clear();
                restored
            }
        }
    }

    /// 流结束时把扣下的尾巴吐出去。
    ///
    /// 不再等待闭合——此时不可能有后续数据，扣着只会丢内容。
    pub fn flush(&mut self, session: &MaskSession) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        let out = session.restore_stream_text(&self.pending);
        self.pending.clear();
        out
    }
}

/// 找出需要扣下的起点：末尾那个尚未闭合、且仍可能长成占位符的 `{{`。
///
/// 返回 `None` 表示整段都能放行。
fn holdback_index(s: &str) -> Option<usize> {
    if let Some(idx) = s.rfind("{{") {
        // 尾部超长的 `{{` 一定不是占位符（用户文本里的字面量），必须放行，
        // 否则流会被永久卡住。
        if !s[idx..].contains("}}") && s.len() - idx <= MAX_PLACEHOLDER_LEN {
            return Some(idx);
        }
    }
    // 末尾孤立的 `{`，下一块可能补上第二个 `{`。
    if s.ends_with('{') {
        return Some(s.len() - 1);
    }
    None
}

/// 内置规则的对外描述，供前端渲染开关列表。
///
/// 只暴露 id / 类型名 / 默认值，**不暴露正则本体**——规则内容属于实现细节，
/// 前端展示它既无意义又会让用户误以为可以在界面上改。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinRuleInfo {
    pub id: String,
    pub label: String,
    pub default_on: bool,
}

/// 列出全部内置规则。
pub fn builtin_rule_infos() -> Vec<BuiltinRuleInfo> {
    BUILTIN_RULES
        .iter()
        .map(|r| BuiltinRuleInfo {
            id: r.id.to_string(),
            label: r.label.to_string(),
            default_on: r.default_on,
        })
        .collect()
}

/// 校验配置能否编译成规则集，保存前调用。
pub fn validate_config(config: &OutboundMaskConfig) -> Result<(), String> {
    compile_rules(config).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::types::{MaskCustomRule, MaskRuleKind, OutboundMaskConfig};
    use serde_json::json;

    fn cfg() -> OutboundMaskConfig {
        OutboundMaskConfig {
            enabled: true,
            ..Default::default()
        }
    }

    fn session() -> MaskSession {
        MaskSession::new("test-session")
    }

    #[test]
    fn masks_and_restores_round_trip() {
        let s = session();
        let mut body = json!({"messages":[{"content":"联系 13800138000 或 a@b.com"}]});
        let stats = mask_request_body(&mut body, &cfg(), &s).unwrap();
        assert_eq!(stats.total, 2);

        let masked = body["messages"][0]["content"].as_str().unwrap();
        assert!(!masked.contains("13800138000"));
        assert!(!masked.contains("a@b.com"));

        // 还原回原文
        let restored = s.restore_plain_text(masked);
        assert_eq!(restored, "联系 13800138000 或 a@b.com");
    }

    #[test]
    fn placeholder_is_stable_within_session() {
        let s = session();
        let mut a = json!({"c": "13800138000"});
        let mut b = json!({"c": "13800138000"});
        mask_request_body(&mut a, &cfg(), &s).unwrap();
        mask_request_body(&mut b, &cfg(), &s).unwrap();
        // 同一实体在多轮里必须是同一个占位符，否则模型会当成两个不同的东西
        assert_eq!(a["c"], b["c"]);
    }

    #[test]
    fn placeholders_are_stable_across_turns_with_fresh_sessions() {
        // 每轮请求都是新的 MaskSession，跨轮一致性完全靠「会话盐 + 文档序计数」
        // 的确定性。历史追加不应改变既有实体的占位符。
        let turn1 = MaskSession::new("conv-1");
        let turn2 = MaskSession::new("conv-1");
        let mut first = json!({"messages":[{"content":"联系 13800138000"}]});
        let mut second = json!({"messages":[
            {"content":"联系 13800138000"},
            {"content":"再确认一次 13800138000"}
        ]});
        mask_request_body(&mut first, &cfg(), &turn1).unwrap();
        mask_request_body(&mut second, &cfg(), &turn2).unwrap();
        assert_eq!(
            first["messages"][0]["content"],
            second["messages"][0]["content"]
        );
        // 同一实体在新追加的那条消息里也必须是同一个占位符
        assert_eq!(
            second["messages"][0]["content"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .last(),
            second["messages"][1]["content"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .last()
        );
    }

    #[test]
    fn distinct_values_get_distinct_placeholders() {
        let s = session();
        let mut body = json!({"c": "13800138000 和 13900139000"});
        mask_request_body(&mut body, &cfg(), &s).unwrap();
        let masked = body["c"].as_str().unwrap();
        let parts: Vec<&str> = masked.split(" 和 ").collect();
        // 遮盖式脱敏在这里会塌缩成两串一样的星号
        assert_ne!(parts[0], parts[1]);
    }

    #[test]
    fn skips_structural_keys() {
        let s = session();
        // 构造一个恰好长得像私网 IP 的模型名，确认不会被打码
        let mut body = json!({"model": "10.1.2.3", "content": "10.1.2.3"});
        mask_request_body(&mut body, &cfg(), &s).unwrap();
        assert_eq!(body["model"], "10.1.2.3");
        assert_ne!(body["content"], "10.1.2.3");
    }

    #[test]
    fn longer_match_wins_on_overlap() {
        let s = session();
        // 连接串里内嵌私网 IP：整条连接串应被当作一个整体
        let mut body = json!({"c": "mysql://root:pw@192.168.1.50:3306/db"});
        let stats = mask_request_body(&mut body, &cfg(), &s).unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.per_rule.get("conn_string"), Some(&1));
        assert_eq!(stats.per_rule.get("private_ip"), None);
    }

    #[test]
    fn phone_not_matched_inside_longer_digits() {
        let s = session();
        let mut body = json!({"c": "订单号 13800138000123456"});
        let stats = mask_request_body(&mut body, &cfg(), &s).unwrap();
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn disabled_config_is_noop() {
        let s = session();
        let mut body = json!({"c": "13800138000"});
        let config = OutboundMaskConfig::default();
        assert!(!config.enabled, "默认必须关闭，不能擅自改写用户请求");
        let stats = mask_request_body(&mut body, &config, &s).unwrap();
        assert_eq!(stats.total, 0);
        assert_eq!(body["c"], "13800138000");
    }

    #[test]
    fn bank_card_off_by_default() {
        let s = session();
        let mut body = json!({"c": "4111111111111111"});
        let stats = mask_request_body(&mut body, &cfg(), &s).unwrap();
        assert_eq!(stats.total, 0);

        let mut on = cfg();
        on.builtin.insert("bank_card".to_string(), true);
        let mut body2 = json!({"c": "4111111111111111"});
        let stats2 = mask_request_body(&mut body2, &on, &s).unwrap();
        assert_eq!(stats2.total, 1);
    }

    #[test]
    fn invalid_custom_regex_is_reported() {
        let s = session();
        let mut config = cfg();
        config.custom_rules.push(MaskCustomRule {
            enabled: true,
            kind: MaskRuleKind::Regex,
            label: "X".into(),
            // 未闭合的括号
            pattern: "(".into(),
        });
        let mut body = json!({"c": "x"});
        // 必须报错而不是静默跳过，否则用户以为规则生效了其实在裸奔
        assert!(mask_request_body(&mut body, &config, &s).is_err());
    }

    #[test]
    fn literal_rule_does_not_treat_input_as_regex() {
        let s = session();
        let mut config = cfg();
        config.custom_rules.push(MaskCustomRule {
            enabled: true,
            kind: MaskRuleKind::Literal,
            label: "TERM".into(),
            pattern: "a.b".into(),
        });
        let mut body = json!({"c": "a.b axb"});
        let stats = mask_request_body(&mut body, &config, &s).unwrap();
        assert_eq!(stats.total, 1, "字面量模式不应把 . 当通配符");
        assert!(body["c"].as_str().unwrap().contains("axb"));
    }

    #[test]
    fn custom_label_is_sanitized() {
        assert_eq!(sanitize_label("我的项目", 0), "CUSTOM1");
        assert_eq!(sanitize_label("", 3), "CUSTOM4");
        assert_eq!(sanitize_label("my-term", 0), "MYTERM");
        assert_eq!(sanitize_label("9lives", 0), "CUSTOM1");
    }

    #[test]
    fn unknown_placeholder_is_left_untouched() {
        let s = session();
        // 模型幻觉出一个没见过的占位符，原样保留比错误还原安全
        let text = "见 {{PHONE_zzz999}}";
        assert_eq!(s.restore_plain_text(text), text);
    }

    // --- 流式还原 ---

    #[test]
    fn stream_restores_placeholder_split_across_chunks() {
        let s = session();
        let mut body = json!({"c": "13800138000"});
        mask_request_body(&mut body, &cfg(), &s).unwrap();
        let ph = body["c"].as_str().unwrap().to_string();

        // 把占位符从中间劈开，模拟 SSE 跨 chunk 切片
        let mid = ph.len() / 2;
        let (head, tail) = ph.split_at(mid);

        let mut r = StreamRestorer::new();
        let mut out = String::new();
        out.push_str(&r.push(&format!("前缀 {head}"), &s));
        out.push_str(&r.push(&format!("{tail} 后缀"), &s));
        out.push_str(&r.flush(&s));

        assert_eq!(out, "前缀 13800138000 后缀");
    }

    #[test]
    fn stream_handles_byte_by_byte_delivery() {
        let s = session();
        let mut body = json!({"c": "a@b.com"});
        mask_request_body(&mut body, &cfg(), &s).unwrap();
        let ph = body["c"].as_str().unwrap().to_string();
        let full = format!("x{ph}y");

        let mut r = StreamRestorer::new();
        let mut out = String::new();
        // 逐字符喂入是最坏情况
        for ch in full.chars() {
            out.push_str(&r.push(&ch.to_string(), &s));
        }
        out.push_str(&r.flush(&s));
        assert_eq!(out, "xa@b.comy");
    }

    #[test]
    fn stream_does_not_stall_on_literal_braces() {
        let s = session();
        // 用户文本里的字面 `{{` 永远不会闭合，不能把流卡住
        let long = format!("{{{{{}", "x".repeat(MAX_PLACEHOLDER_LEN + 10));
        let mut r = StreamRestorer::new();
        let out = r.push(&long, &s);
        assert!(!out.is_empty(), "超长未闭合的 {{{{ 必须放行");
    }

    #[test]
    fn stream_passthrough_when_no_placeholders() {
        let s = session();
        let mut r = StreamRestorer::new();
        let out = r.push("普通文本，没有占位符", &s);
        assert_eq!(out, "普通文本，没有占位符");
    }

    // --- 转义 ---

    #[test]
    fn stream_restore_escapes_for_json_context() {
        let s = session();
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIabc\n-----END RSA PRIVATE KEY-----";
        let mut body = json!({"c": pem});
        mask_request_body(&mut body, &cfg(), &s).unwrap();
        let ph = body["c"].as_str().unwrap().to_string();

        // 流式路径：原文含换行，必须转义成 \n，否则外层 JSON 被破坏
        let sse_line = format!(r#"{{"text":"{ph}"}}"#);
        let restored = s.restore_stream_text(&sse_line);
        assert!(!restored.contains('\n'), "裸换行会破坏 SSE 的 JSON");
        assert!(restored.contains("\\n"));
        // 还原后仍是合法 JSON，且解析出来就是原文
        let parsed: Value = serde_json::from_str(&restored).expect("还原后必须仍是合法 JSON");
        assert_eq!(parsed["text"], pem);
    }

    #[test]
    fn non_streaming_restore_uses_raw_value() {
        let s = session();
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIabc\n-----END RSA PRIVATE KEY-----";
        let mut body = json!({"c": pem});
        mask_request_body(&mut body, &cfg(), &s).unwrap();

        // 非流式路径：在解析好的 Value 里还原，转义交给 serde_json
        let mut resp = json!({"choices":[{"text": body["c"].as_str().unwrap()}]});
        s.restore_json_value(&mut resp);
        assert_eq!(resp["choices"][0]["text"], pem);
        // 序列化后依然合法
        let text = serde_json::to_string(&resp).unwrap();
        let reparsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(reparsed["choices"][0]["text"], pem);
    }

    #[test]
    fn different_sessions_produce_different_placeholders() {
        let a = MaskSession::new("session-a");
        let b = MaskSession::new("session-b");
        let mut ba = json!({"c": "13800138000"});
        let mut bb = json!({"c": "13800138000"});
        mask_request_body(&mut ba, &cfg(), &a).unwrap();
        mask_request_body(&mut bb, &cfg(), &b).unwrap();
        assert_ne!(ba["c"], bb["c"]);
    }

    #[test]
    fn empty_session_short_circuits_restore() {
        let s = session();
        assert!(s.is_empty());
        let mut body = json!({"c": "13800138000"});
        mask_request_body(&mut body, &cfg(), &s).unwrap();
        assert!(!s.is_empty());
    }

    #[test]
    fn stats_summary_is_stable() {
        let mut st = MaskStats::default();
        st.record("email");
        st.record("email");
        st.record("cn_phone");
        assert_eq!(st.summary(), "3 replacements (cn_phone=1, email=2)");
        assert_eq!(MaskStats::default().summary(), "no replacements");
    }
}

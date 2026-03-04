//! 出站请求脱敏模块
//!
//! 在请求发送到上游之前对请求体进行本地脱敏，避免敏感信息外发。

use super::types::{OutboundRedactionConfig, RedactionErrorStrategy, RedactionMatchMethod};
use aho_corasick::AhoCorasick;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;

/// 脱敏统计信息（仅包含计数，不记录原始内容）
#[derive(Debug, Clone, Default)]
pub struct RedactionStats {
    pub total_replacements: usize,
    pub rule_replacements: BTreeMap<String, usize>,
}

impl RedactionStats {
    /// 返回可用于日志打印的简短统计信息
    pub fn to_log_summary(&self) -> String {
        if self.rule_replacements.is_empty() {
            return "none".to_string();
        }
        self.rule_replacements
            .iter()
            .map(|(rule, count)| format!("{rule}:{count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// 脱敏处理结果
#[derive(Debug, Clone)]
pub struct RedactionResult {
    pub body: Value,
    pub stats: RedactionStats,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct CompiledCustomRule {
    rule_index: usize,
    label: String,
    matcher: RuleMatcher,
}

#[derive(Debug, Clone)]
enum RuleMatcher {
    Regex(Regex),
    Literal(String),
}

#[derive(Debug, Default)]
struct RedactionState {
    stats: RedactionStats,
}

impl RedactionState {
    fn record_replacement(&mut self, label: &str) {
        self.stats.total_replacements += 1;
        *self
            .stats
            .rule_replacements
            .entry(label.to_string())
            .or_insert(0) += 1;
    }
}

#[derive(Debug, Clone)]
struct MatchSpan {
    start: usize,
    end: usize,
    rule_index: usize,
    rule_label: String,
}

#[derive(Debug)]
struct CompiledLiteralMatcher {
    ac: AhoCorasick,
    // Aho pattern slot -> custom_rules index
    rule_slots: Vec<usize>,
}

/// 对请求体执行出站脱敏。
///
/// 注意：
/// - 仅处理 JSON 字符串值
/// - 当前仅支持「自定义规则」
/// - 默认异常策略为 warn_and_bypass，不中断请求
pub fn redact_outbound_payload(
    body: Value,
    config: &OutboundRedactionConfig,
) -> Result<RedactionResult, String> {
    if !config.enabled {
        return Ok(RedactionResult {
            body,
            stats: RedactionStats::default(),
            warnings: Vec::new(),
        });
    }

    let mut warnings = Vec::new();
    let rules = compile_custom_rules(config, &mut warnings)?;
    let literal_matcher = build_literal_matcher(&rules);

    let mut state = RedactionState::default();
    let mut redacted_body = body;
    redact_value(
        &mut redacted_body,
        &rules,
        literal_matcher.as_ref(),
        &mut state,
    );

    Ok(RedactionResult {
        body: redacted_body,
        stats: state.stats,
        warnings,
    })
}

fn compile_custom_rules(
    config: &OutboundRedactionConfig,
    warnings: &mut Vec<String>,
) -> Result<Vec<CompiledCustomRule>, String> {
    let mut compiled = Vec::new();

    for (index, rule) in config.custom_rules.iter().enumerate() {
        if !rule.enabled {
            continue;
        }

        let rule_index = index;
        let label = format!("RULE_{}", index + 1);
        let matcher = match rule.match_method {
            RedactionMatchMethod::Regex => match Regex::new(&rule.pattern) {
                Ok(regex) => RuleMatcher::Regex(regex),
                Err(err) => {
                    let msg = format!("自定义脱敏规则 #{}, pattern 非法: {err}", index + 1);
                    handle_rule_error(config.on_error, warnings, msg)?;
                    continue;
                }
            },
            RedactionMatchMethod::StringMatch => {
                if rule.pattern.is_empty() {
                    let msg = format!(
                        "自定义脱敏规则 #{} 的 string_match pattern 不能为空",
                        index + 1
                    );
                    handle_rule_error(config.on_error, warnings, msg)?;
                    continue;
                }
                RuleMatcher::Literal(rule.pattern.clone())
            }
        };

        compiled.push(CompiledCustomRule {
            rule_index,
            label,
            matcher,
        });
    }

    Ok(compiled)
}

fn handle_rule_error(
    strategy: RedactionErrorStrategy,
    warnings: &mut Vec<String>,
    message: String,
) -> Result<(), String> {
    match strategy {
        RedactionErrorStrategy::WarnAndBypass => {
            warnings.push(message);
            Ok(())
        }
        RedactionErrorStrategy::BlockRequest => Err(message),
    }
}

fn redact_value(
    value: &mut Value,
    custom_rules: &[CompiledCustomRule],
    literal_matcher: Option<&CompiledLiteralMatcher>,
    state: &mut RedactionState,
) {
    match value {
        Value::Object(map) => {
            for child in map.values_mut() {
                redact_value(child, custom_rules, literal_matcher, state);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                redact_value(item, custom_rules, literal_matcher, state);
            }
        }
        Value::String(text) => {
            let redacted = redact_text(text, custom_rules, literal_matcher, state);
            *text = redacted;
        }
        _ => {}
    }
}

fn redact_text(
    input: &str,
    custom_rules: &[CompiledCustomRule],
    literal_matcher: Option<&CompiledLiteralMatcher>,
    state: &mut RedactionState,
) -> String {
    let mut all_matches = collect_matches(input, custom_rules, literal_matcher);
    if all_matches.is_empty() {
        return input.to_string();
    }

    all_matches.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| (b.end - b.start).cmp(&(a.end - a.start)))
            .then_with(|| a.rule_index.cmp(&b.rule_index))
    });

    let mut selected = Vec::new();
    let mut occupied_until = 0usize;
    for span in all_matches {
        if span.start < occupied_until || span.start >= span.end {
            continue;
        }
        occupied_until = span.end;
        selected.push(span);
    }

    if selected.is_empty() {
        return input.to_string();
    }

    let mut output = String::with_capacity(input.len());
    let mut cursor = 0usize;
    for span in selected {
        output.push_str(&input[cursor..span.start]);
        output.push_str(&mask_for_span(&input[span.start..span.end]));
        state.record_replacement(&span.rule_label);
        cursor = span.end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn build_literal_matcher(custom_rules: &[CompiledCustomRule]) -> Option<CompiledLiteralMatcher> {
    let mut patterns = Vec::new();
    let mut rule_slots = Vec::new();

    for (rule_slot, rule) in custom_rules.iter().enumerate() {
        if let RuleMatcher::Literal(literal) = &rule.matcher {
            patterns.push(literal.as_str());
            rule_slots.push(rule_slot);
        }
    }

    if patterns.is_empty() {
        return None;
    }

    match AhoCorasick::new(&patterns) {
        Ok(ac) => Some(CompiledLiteralMatcher { ac, rule_slots }),
        Err(_) => None,
    }
}

fn collect_matches(
    input: &str,
    custom_rules: &[CompiledCustomRule],
    literal_matcher: Option<&CompiledLiteralMatcher>,
) -> Vec<MatchSpan> {
    let mut matches = Vec::new();
    for rule in custom_rules {
        match &rule.matcher {
            RuleMatcher::Regex(regex) => {
                for found in regex.find_iter(input) {
                    if found.start() == found.end() {
                        continue;
                    }
                    matches.push(MatchSpan {
                        start: found.start(),
                        end: found.end(),
                        rule_index: rule.rule_index,
                        rule_label: rule.label.clone(),
                    });
                }
            }
            RuleMatcher::Literal(_) => {}
        }
    }
    if let Some(matcher) = literal_matcher {
        matches.extend(collect_literal_matches(input, custom_rules, matcher));
    } else {
        matches.extend(collect_literal_matches_fallback(input, custom_rules));
    }
    matches
}

fn collect_literal_matches(
    input: &str,
    custom_rules: &[CompiledCustomRule],
    literal_matcher: &CompiledLiteralMatcher,
) -> Vec<MatchSpan> {
    if literal_matcher.rule_slots.is_empty() {
        return Vec::new();
    }

    let mut last_end_by_rule = vec![0usize; literal_matcher.rule_slots.len()];
    let mut matches = Vec::new();

    for found in literal_matcher.ac.find_overlapping_iter(input) {
        if found.start() == found.end() {
            continue;
        }
        let rule_slot = found.pattern().as_usize();
        if found.start() < last_end_by_rule[rule_slot] {
            continue;
        }
        last_end_by_rule[rule_slot] = found.end();
        let rule = &custom_rules[literal_matcher.rule_slots[rule_slot]];
        matches.push(MatchSpan {
            start: found.start(),
            end: found.end(),
            rule_index: rule.rule_index,
            rule_label: rule.label.clone(),
        });
    }

    matches
}

fn collect_literal_matches_fallback(
    input: &str,
    custom_rules: &[CompiledCustomRule],
) -> Vec<MatchSpan> {
    let mut matches = Vec::new();
    for rule in custom_rules {
        let literal = match &rule.matcher {
            RuleMatcher::Literal(literal) => literal.as_str(),
            RuleMatcher::Regex(_) => continue,
        };
        for (start, end) in find_literal_matches(input, literal) {
            matches.push(MatchSpan {
                start,
                end,
                rule_index: rule.rule_index,
                rule_label: rule.label.clone(),
            });
        }
    }
    matches
}

fn find_literal_matches(input: &str, literal: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    if literal.is_empty() {
        return ranges;
    }

    let mut cursor = 0usize;
    while let Some(found) = input[cursor..].find(literal) {
        let start = cursor + found;
        let end = start + literal.len();
        ranges.push((start, end));
        cursor = end;
    }

    ranges
}

fn mask_for_span(input: &str) -> String {
    "*".repeat(input.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::types::{
        CustomRedactionRule, OutboundRedactionConfig, RedactionErrorStrategy, RedactionMatchMethod,
    };
    use serde_json::json;

    fn base_config() -> OutboundRedactionConfig {
        OutboundRedactionConfig {
            enabled: true,
            on_error: RedactionErrorStrategy::WarnAndBypass,
            custom_rules: vec![],
        }
    }

    #[test]
    fn custom_rule_redacts_email_and_phone() {
        let mut config = base_config();
        config.custom_rules = vec![
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"\b(?:\+?86[-\s]?)?1[3-9]\d{9}\b".to_string(),
            },
        ];

        let body = json!({
            "content": "email=alice@example.com phone=13800001234"
        });

        let result = redact_outbound_payload(body, &config).expect("redaction ok");
        let content = result.body["content"].as_str().unwrap();

        assert_eq!(content, "email=***************** phone=***********");
        assert_eq!(result.stats.rule_replacements.get("RULE_1"), Some(&1));
        assert_eq!(result.stats.rule_replacements.get("RULE_2"), Some(&1));
    }

    #[test]
    fn invalid_custom_rule_warns_and_bypasses_when_configured() {
        let mut config = base_config();
        config.custom_rules = vec![
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: "(".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b".to_string(),
            },
        ];

        let body = json!({
            "email": "alice@example.com"
        });
        let result = redact_outbound_payload(body, &config).expect("should not fail");
        assert!(!result.warnings.is_empty());
        assert_eq!(result.body["email"], "*****************");
    }

    #[test]
    fn invalid_custom_rule_blocks_request_when_configured() {
        let mut config = base_config();
        config.on_error = RedactionErrorStrategy::BlockRequest;
        config.custom_rules = vec![CustomRedactionRule {
            enabled: true,
            match_method: RedactionMatchMethod::Regex,
            pattern: "(".to_string(),
        }];

        let body = json!({
            "email": "alice@example.com"
        });
        let err = redact_outbound_payload(body, &config).expect_err("should fail");
        assert!(err.contains("#1"));
    }

    #[test]
    fn disabled_config_returns_original_body() {
        let mut config = base_config();
        config.enabled = false;
        let body = json!({
            "email": "alice@example.com"
        });

        let result = redact_outbound_payload(body.clone(), &config).expect("ok");
        assert_eq!(result.body, body);
        assert_eq!(result.stats.total_replacements, 0);
    }

    #[test]
    fn string_match_replaces_literal_content() {
        let mut config = base_config();
        config.custom_rules = vec![CustomRedactionRule {
            enabled: true,
            match_method: RedactionMatchMethod::StringMatch,
            pattern: "top-secret".to_string(),
        }];

        let body = json!({
            "content": "token=top-secret, again=top-secret"
        });
        let result = redact_outbound_payload(body, &config).expect("redaction ok");
        let content = result.body["content"].as_str().unwrap();
        assert_eq!(content, "token=**********, again=**********");
        assert_eq!(result.stats.rule_replacements.get("RULE_1"), Some(&2));
    }

    #[test]
    fn overlapping_string_match_prefers_longest_span() {
        let mut config = base_config();
        config.custom_rules = vec![
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::StringMatch,
                pattern: "abc".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::StringMatch,
                pattern: "abc123".to_string(),
            },
        ];

        let body = json!({
            "v1": "abc",
            "v2": "abc123",
        });

        let result = redact_outbound_payload(body, &config).expect("redaction ok");
        assert_eq!(result.body["v1"], "***");
        assert_eq!(result.body["v2"], "******");
        assert_eq!(result.stats.rule_replacements.get("RULE_1"), Some(&1));
        assert_eq!(result.stats.rule_replacements.get("RULE_2"), Some(&1));
    }

    #[test]
    fn tie_break_uses_rule_order_not_label_lexical_order() {
        let mut config = base_config();
        config.custom_rules = vec![
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"$^".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: "abc".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"$^".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"$^".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"$^".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"$^".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"$^".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"$^".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::Regex,
                pattern: r"$^".to_string(),
            },
            CustomRedactionRule {
                enabled: true,
                match_method: RedactionMatchMethod::StringMatch,
                pattern: "abc".to_string(),
            },
        ];

        let body = json!({ "v": "abc" });
        let result = redact_outbound_payload(body, &config).expect("redaction ok");

        assert_eq!(result.body["v"], "***");
        assert_eq!(result.stats.rule_replacements.get("RULE_2"), Some(&1));
        assert_eq!(result.stats.rule_replacements.get("RULE_10"), None);
    }
}

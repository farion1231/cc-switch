use serde::{Deserialize, Serialize};

use crate::orchestration::model_caller::ModelCaller;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationTool {
    StructuralCheck,
    PatternMatch,
    SchemaValidator,
    LLMJudge,
}

impl VerificationTool {
    pub fn name(&self) -> &'static str {
        match self {
            Self::StructuralCheck => "structural_check",
            Self::PatternMatch => "pattern_match",
            Self::SchemaValidator => "schema_validator",
            Self::LLMJudge => "llm_judge",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityGate {
    pub tools: Vec<VerificationTool>,
    pub pass_threshold: f64,
}

impl Default for QualityGate {
    fn default() -> Self {
        Self {
            tools: vec![
                VerificationTool::StructuralCheck,
                VerificationTool::PatternMatch,
            ],
            pass_threshold: 0.65,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityResult {
    pub passed: bool,
    pub score: f64,
    pub individual_scores: Vec<(String, f64)>,
}

// ---------------------------------------------------------------------------
// QualityGate implementation
// ---------------------------------------------------------------------------

impl QualityGate {
    pub fn new(tools: Vec<VerificationTool>, pass_threshold: f64) -> Self {
        Self {
            tools,
            pass_threshold,
        }
    }

    /// Run every configured verification tool against `content` and return
    /// an aggregated result.
    ///
    /// When `tools` is empty the result *vacuously passes* (score = 1.0,
    /// no individual scores).  This matches the design-doc semantics for a
    /// gate with nothing to check.
    pub async fn verify(
        &self,
        content: &str,
        json_schema: Option<&serde_json::Value>,
        model_caller: Option<&ModelCaller>,
        judge_model_key: Option<&str>,
    ) -> QualityResult {
        if self.tools.is_empty() {
            return QualityResult {
                passed: true,
                score: 1.0,
                individual_scores: vec![],
            };
        }

        let mut individual: Vec<(String, f64)> = Vec::with_capacity(self.tools.len());

        for tool in &self.tools {
            let score = match tool {
                VerificationTool::StructuralCheck => {
                    run_structural_check(content)
                }
                VerificationTool::PatternMatch => {
                    run_pattern_match(content)
                }
                VerificationTool::SchemaValidator => {
                    run_schema_validator(content, json_schema)
                }
                VerificationTool::LLMJudge => {
                    run_llm_judge(content, model_caller, judge_model_key).await
                }
            };
            individual.push((tool.name().to_string(), score));
        }

        let total: f64 = individual.iter().map(|(_, s)| *s).sum();
        let count = individual.len() as f64;
        let avg = if count > 0.0 { total / count } else { 1.0 };

        QualityResult {
            passed: avg >= self.pass_threshold,
            score: avg,
            individual_scores: individual,
        }
    }
}

// ---------------------------------------------------------------------------
// Structural check
// ---------------------------------------------------------------------------

/// Extract code blocks from markdown fences (``` ... ```), including an
/// optional language tag on the opening line.  Returns a vec of
/// `(language_option, code_body)`.
fn extract_code_blocks(content: &str) -> Vec<(Option<String>, String)> {
    let mut blocks: Vec<(Option<String>, String)> = Vec::new();
    let mut in_block = false;
    let mut current_lang: Option<String> = None;
    let mut current_body = String::new();

    for line in content.lines() {
        if !in_block {
            if let Some(rest) = line.strip_prefix("```") {
                in_block = true;
                let lang = rest.trim();
                current_lang = if lang.is_empty() {
                    None
                } else {
                    Some(lang.to_string())
                };
                current_body.clear();
            }
        } else if line.trim() == "```" {
            in_block = false;
            blocks.push((current_lang.take(), current_body.clone()));
        } else {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }

    // If still inside an unclosed fence, take what we have.
    if in_block && !current_body.is_empty() {
        blocks.push((current_lang, current_body));
    }

    blocks
}

/// Check bracket balance across the full text (code blocks only when
/// available, otherwise the whole content).
fn check_bracket_balance(text: &str) -> f64 {
    let mut paren: i32 = 0;
    let mut bracket: i32 = 0;
    let mut brace: i32 = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_triple_single = false;
    let mut in_triple_double = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut prev_ch: Option<char> = None;
    let mut prev_prev_ch: Option<char> = None;

    let chars: Vec<char> = text.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        // Skip characters inside comments
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            prev_prev_ch = prev_ch;
            prev_ch = Some(ch);
            continue;
        }

        if in_block_comment {
            if prev_ch == Some('*') && ch == '/' {
                in_block_comment = false;
            }
            prev_prev_ch = prev_ch;
            prev_ch = Some(ch);
            continue;
        }

        // Handle triple quotes (Python)
        if !in_double_quote && !in_single_quote {
            if ch == '\'' {
                // Check for triple quote opening '''
                if i + 2 < chars.len() && chars[i + 1] == '\'' && chars[i + 2] == '\'' {
                    if in_triple_single {
                        in_triple_single = false;
                    } else {
                        in_triple_single = true;
                    }
                    prev_prev_ch = prev_ch;
                    prev_ch = Some(ch);
                    continue;
                }
            }
            if ch == '"' {
                // Check for triple quote opening """
                if i + 2 < chars.len() && chars[i + 1] == '"' && chars[i + 2] == '"' {
                    if in_triple_double {
                        in_triple_double = false;
                    } else {
                        in_triple_double = true;
                    }
                    prev_prev_ch = prev_ch;
                    prev_ch = Some(ch);
                    continue;
                }
            }
        }

        if in_triple_single || in_triple_double {
            prev_prev_ch = prev_ch;
            prev_ch = Some(ch);
            continue;
        }

        // Handle regular strings
        if ch == '"' && prev_ch != Some('\\') {
            in_double_quote = !in_double_quote;
        } else if ch == '\'' && prev_ch != Some('\\') {
            in_single_quote = !in_single_quote;
        }

        if in_double_quote || in_single_quote {
            prev_prev_ch = prev_ch;
            prev_ch = Some(ch);
            continue;
        }

        // Handle comment starts
        if ch == '/' && prev_ch == Some('/') {
            in_line_comment = true;
            prev_prev_ch = prev_ch;
            prev_ch = Some(ch);
            continue;
        }
        if ch == '*' && prev_ch == Some('/') {
            in_block_comment = true;
            prev_prev_ch = prev_ch;
            prev_ch = Some(ch);
            continue;
        }
        // Python-style comment
        if ch == '#' {
            in_line_comment = true;
            prev_prev_ch = prev_ch;
            prev_ch = Some(ch);
            continue;
        }

        // Count brackets
        match ch {
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            _ => {}
        }

        prev_prev_ch = prev_ch;
        prev_ch = Some(ch);
    }

    // Score: start at 1.0, penalise each type of imbalance.
    let mut penalty = 0.0;
    if paren != 0 {
        penalty += 0.33;
    }
    if bracket != 0 {
        penalty += 0.33;
    }
    if brace != 0 {
        penalty += 0.34;
    }
    (1.0_f64 - penalty).max(0.0_f64)
}

/// Rust-specific: check that every `fn` declaration has a body `{ ... }`.
fn check_rust_fn_body(code: &str) -> f64 {
    let lines: Vec<&str> = code.lines().collect();
    let mut fn_count = 0usize;
    let mut fn_with_body = 0usize;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.contains("fn ") && !line.starts_with("//") && !line.starts_with('#') {
            // Check if this is a trait fn (no body) vs impl fn (has body)
            // Look ahead for `{` on same or subsequent lines
            let mut found_body = false;
            let mut j = i;
            let mut buffer = String::new();
            while j < lines.len().min(i + 5) {
                buffer.push_str(lines[j]);
                if buffer.contains('{') {
                    found_body = true;
                    break;
                }
                if buffer.contains(';') {
                    break;
                }
                j += 1;
            }
            if found_body {
                fn_with_body += 1;
            }
            fn_count += 1;
        }
        i += 1;
    }

    if fn_count == 0 {
        return 1.0;
    }
    (fn_with_body as f64 / fn_count as f64).min(1.0)
}

/// Python-specific: check that every `def` has at least one indented line
/// following it.
fn check_python_def_body(code: &str) -> f64 {
    let lines: Vec<&str> = code.lines().collect();
    let mut def_count = 0usize;
    let mut def_with_body = 0usize;

    for i in 0..lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("def ") && trimmed.ends_with(':') {
            def_count += 1;
            // Check next line for indentation
            if i + 1 < lines.len() {
                let next = lines[i + 1];
                if next.starts_with(' ') || next.starts_with('\t') {
                    let next_trimmed = next.trim();
                    if !next_trimmed.is_empty()
                        && !next_trimmed.starts_with("pass")
                        && !next_trimmed.starts_with("...")
                    {
                        def_with_body += 1;
                    }
                }
            }
        }
    }

    if def_count == 0 {
        return 1.0;
    }
    (def_with_body as f64 / def_count as f64).min(1.0)
}

/// Check for unclosed strings (simple heuristic: count unescaped quotes).
fn check_unclosed_strings(text: &str) -> f64 {
    let mut issues = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut prev: Option<char> = None;

    for ch in text.chars() {
        if ch == '\'' && prev != Some('\\') && !in_double {
            in_single = !in_single;
        } else if ch == '"' && prev != Some('\\') && !in_single {
            in_double = !in_double;
        }
        prev = Some(ch);
    }

    if in_single {
        issues += 1;
    }
    if in_double {
        issues += 1;
    }

    match issues {
        0 => 1.0,
        1 => 0.5,
        _ => 0.2,
    }
}

fn run_structural_check(content: &str) -> f64 {
    let blocks = extract_code_blocks(content);
    let text_to_check = if blocks.is_empty() {
        content
    } else {
        // Combine all code block bodies
        &blocks
            .iter()
            .map(|(_, body)| body.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };

    if text_to_check.trim().is_empty() {
        return 0.0;
    }

    let bracket_score = check_bracket_balance(text_to_check);
    let unclosed_string_score = check_unclosed_strings(text_to_check);

    // Language-specific checks
    let mut lang_specific_scores: Vec<f64> = Vec::new();
    for (lang, body) in &blocks {
        match lang.as_deref() {
            Some("rust") | Some("rs") => {
                lang_specific_scores.push(check_rust_fn_body(body));
            }
            Some("python") | Some("py") => {
                lang_specific_scores.push(check_python_def_body(body));
            }
            _ => {}
        }
    }

    let mut total = bracket_score + unclosed_string_score;
    let mut count = 2.0;

    for s in &lang_specific_scores {
        total += *s;
        count += 1.0;
    }

    (total / count).min(1.0).max(0.0)
}

// ---------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------

/// Anti-pattern definitions with associated penalty weights.
struct AntiPattern {
    name: &'static str,
    pattern: &'static str,
    penalty: f64, // per occurrence, applied to the final score
}

fn get_anti_patterns() -> Vec<AntiPattern> {
    vec![
        // Empty catch/except blocks
        AntiPattern {
            name: "empty_catch",
            pattern: r"(?ms)catch\s*\([^)]*\)\s*\{\s*\}",
            penalty: 0.15,
        },
        AntiPattern {
            name: "empty_except",
            pattern: r"(?m)except\s*.*:\s*\n(\s+pass\s*\n|\s*\n)",
            penalty: 0.15,
        },
        // Hardcoded paths
        AntiPattern {
            name: "hardcoded_path_unix",
            pattern: r#"(?m)(?:^|["'\s=])/usr/[^\s"']+"#,
            penalty: 0.10,
        },
        AntiPattern {
            name: "hardcoded_path_windows",
            pattern: r#"(?mi)(?:^|["'\s=])C:\\[^\s"']+"#,
            penalty: 0.10,
        },
        AntiPattern {
            name: "hardcoded_path_tmp",
            pattern: r#"(?m)(?:^|["'\s=])/tmp/[^\s"']+"#,
            penalty: 0.10,
        },
        // Hardcoded secrets
        AntiPattern {
            name: "hardcoded_password",
            pattern: r#"(?mi)password\s*=\s*"[^"]+""#,
            penalty: 0.20,
        },
        AntiPattern {
            name: "hardcoded_api_key",
            pattern: r#"(?mi)(?:api_?key|apikey)\s*=\s*"[^"]+""#,
            penalty: 0.20,
        },
        AntiPattern {
            name: "hardcoded_token",
            pattern: r#"(?mi)(?:secret_?token|auth_?token)\s*=\s*"[^"]+""#,
            penalty: 0.20,
        },
        // TODO/FIXME/HACK (minor penalty)
        AntiPattern {
            name: "todo_comment",
            pattern: r"(?mi)\b(?:TODO|FIXME|HACK)\b",
            penalty: 0.03,
        },
        // Bare throw without context (JS)
        AntiPattern {
            name: "bare_throw",
            pattern: r"(?m)^\s*throw\s*;\s*$",
            penalty: 0.10,
        },
    ]
}

fn run_pattern_match(content: &str) -> f64 {
    let patterns = get_anti_patterns();
    let mut total_penalty = 0.0_f64;

    for ap in &patterns {
        if let Ok(re) = regex::Regex::new(ap.pattern) {
            let matches = re.find_iter(content).count();
            if matches > 0 {
                // Cap per-pattern penalty at 0.5 so one bad pattern cannot
                // single-handedly zero the score.
                let capped = (ap.penalty * matches as f64).min(0.5);
                log::debug!(
                    "[PatternMatch] '{}' matched {} times (penalty {:.3})",
                    ap.name,
                    matches,
                    capped
                );
                total_penalty += capped;
            }
        }
    }

    // Total penalty is capped at 1.0
    (1.0 - total_penalty.min(1.0)).max(0.0)
}

// ---------------------------------------------------------------------------
// Schema validation
// ---------------------------------------------------------------------------

fn run_schema_validator(content: &str, schema: Option<&serde_json::Value>) -> f64 {
    let Some(schema) = schema else {
        // No schema provided, vacuously pass.
        return 1.0;
    };

    // Attempt to parse content as JSON.  It may be wrapped in markdown fences.
    let json_str = strip_json_fences(content);

    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&json_str);
    let Ok(value) = parsed else {
        log::debug!("[SchemaValidator] Content is not valid JSON");
        return 0.0;
    };

    validate_json_against_schema(&value, schema)
}

/// Strip optional markdown fences around JSON content.
fn strip_json_fences(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.starts_with("```") {
        let without_start = trimmed
            .trim_start_matches("```")
            .trim_start_matches("json")
            .trim_start_matches("JSON");
        if without_start.ends_with("```") {
            return without_start
                .trim_end_matches("```")
                .trim()
                .to_string();
        }
    }
    trimmed.to_string()
}

/// Lightweight JSON Schema validation supporting the subset used in v2 MVP:
///   - `type` (string, number, integer, boolean, array, object, null)
///   - `required` (array of required property names, for objects)
///   - `properties` (schema per property, for objects)
///   - `items` (schema for array items)
///   - `minLength` / `maxLength` (for strings)
///   - `minimum` / `maximum` (for numbers)
///
/// This is deliberately not a full JSON Schema implementation -- just enough
/// for the gateway's structured-output checks.
fn validate_json_against_schema(value: &serde_json::Value, schema: &serde_json::Value) -> f64 {
    let mut errors = 0usize;
    let mut checks = 0usize;

    validate_node(value, schema, &mut errors, &mut checks);

    if checks == 0 {
        return 1.0;
    }

    let pass_rate = 1.0 - (errors as f64 / checks as f64);
    pass_rate.max(0.0)
}

fn validate_node(
    value: &serde_json::Value,
    schema: &serde_json::Value,
    errors: &mut usize,
    checks: &mut usize,
) {
    // type check
    if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
        *checks += 1;
        let type_ok = match type_str {
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.is_i64() || value.is_u64(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            "null" => value.is_null(),
            _ => true,
        };
        if !type_ok {
            *errors += 1;
        }
    }

    // string length
    if let Some(s) = value.as_str() {
        if let Some(min) = schema.get("minLength").and_then(|v| v.as_u64()) {
            *checks += 1;
            if (s.len() as u64) < min {
                *errors += 1;
            }
        }
        if let Some(max) = schema.get("maxLength").and_then(|v| v.as_u64()) {
            *checks += 1;
            if (s.len() as u64) > max {
                *errors += 1;
            }
        }
    }

    // number range
    if value.is_number() {
        if let Some(min) = schema.get("minimum").and_then(|v| v.as_f64()) {
            *checks += 1;
            if value.as_f64().unwrap_or(f64::MAX) < min {
                *errors += 1;
            }
        }
        if let Some(max) = schema.get("maximum").and_then(|v| v.as_f64()) {
            *checks += 1;
            if value.as_f64().unwrap_or(f64::MIN) > max {
                *errors += 1;
            }
        }
    }

    // object: required + properties
    if let Some(obj) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for key in required {
                if let Some(key_str) = key.as_str() {
                    *checks += 1;
                    if !obj.contains_key(key_str) {
                        *errors += 1;
                    }
                }
            }
        }

        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            for (key, prop_schema) in props {
                if let Some(child) = obj.get(key) {
                    validate_node(child, prop_schema, errors, checks);
                }
            }
        }
    }

    // array: items
    if let Some(arr) = value.as_array() {
        if let Some(item_schema) = schema.get("items") {
            for item in arr {
                validate_node(item, item_schema, errors, checks);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// LLM Judge
// ---------------------------------------------------------------------------

async fn run_llm_judge(
    content: &str,
    model_caller: Option<&ModelCaller>,
    judge_model_key: Option<&str>,
) -> f64 {
    let (Some(caller), Some(key)) = (model_caller, judge_model_key) else {
        log::debug!("[LLMJudge] No model caller or key provided, skipping");
        return 0.7; // Neutral default when LLM judge is unavailable
    };

    let prompt = format!(
        r#"Rate this AI code response on a scale of 0.0 to 1.0:
- 0.0-0.3: Incorrect or harmful code
- 0.3-0.5: Partially correct, needs significant fixes
- 0.5-0.7: Mostly correct, minor issues
- 0.7-0.9: Correct and well-written
- 0.9-1.0: Excellent, production-ready

---BEGIN CONTENT---
{}
---END CONTENT---

Reply with ONLY a number between 0.0 and 1.0."#,
        content
    );

    match caller.call_prompt(key, "", &prompt, Some(0.0)).await {
        Ok(resp) => parse_llm_score(&resp.content),
        Err(e) => {
            log::warn!("[LLMJudge] Model call failed: {}", e);
            0.7 // Neutral fallback
        }
    }
}

/// Parse the LLM response to extract a score.  Looks for the first floating
/// point number in the text and clamps it to [0.0, 1.0].
fn parse_llm_score(text: &str) -> f64 {
    // Try the full text first
    let trimmed = text.trim();

    // Direct parse
    if let Ok(v) = trimmed.parse::<f64>() {
        return v.clamp(0.0, 1.0);
    }

    // Find first number-like token
    for token in trimmed.split_whitespace() {
        let clean = token.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
        if let Ok(v) = clean.parse::<f64>() {
            return v.clamp(0.0, 1.0);
        }
    }

    // Regex fallback for embedded numbers
    if let Ok(re) = regex::Regex::new(r"(\d+\.?\d*)") {
        if let Some(caps) = re.captures(trimmed) {
            if let Some(m) = caps.get(1) {
                if let Ok(v) = m.as_str().parse::<f64>() {
                    return v.clamp(0.0, 1.0);
                }
            }
        }
    }

    0.5 // Unknown score, give the benefit of the doubt
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Structural check: valid code ----

    #[test]
    fn structural_check_passes_valid_rust_code() {
        let code = r#"```rust
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let result = add(1, 2);
    println!("{}", result);
}
```"#;
        let score = run_structural_check(code);
        assert!(
            score >= 0.9,
            "Valid Rust code should score >= 0.9, got {}",
            score
        );
    }

    #[test]
    fn structural_check_passes_valid_python_code() {
        let code = "```python\ndef greet(name):\n    return f'Hello, {name}!'\n\nprint(greet('world'))\n```";
        let score = run_structural_check(code);
        assert!(
            score >= 0.8,
            "Valid Python code should score >= 0.8, got {}",
            score
        );
    }

    #[test]
    fn structural_check_passes_valid_js_code() {
        let code = "```javascript\nfunction hello() {\n  console.log('hi');\n}\nhello();\n```";
        let score = run_structural_check(code);
        assert!(
            score >= 0.8,
            "Valid JS code should score >= 0.8, got {}",
            score
        );
    }

    // ---- Structural check: invalid code ----

    #[test]
    fn structural_check_fails_unbalanced_brackets() {
        let code = "```javascript\nfunction broken() {\n  if (true {\n    return [1, 2, 3;\n  }\n}\n```";
        let score = run_structural_check(code);
        assert!(
            score < 0.7,
            "Unbalanced brackets should score < 0.7, got {}",
            score
        );
    }

    #[test]
    fn structural_check_fails_unclosed_brace() {
        let code = "```rust\nfn open() {\n    let x = 1;\n```";
        let score = run_structural_check(code);
        assert!(
            score < 0.7,
            "Unclosed brace should score < 0.7, got {}",
            score
        );
    }

    #[test]
    fn structural_check_returns_zero_for_empty_content() {
        let score = run_structural_check("");
        assert_eq!(score, 0.0, "Empty content should score 0.0");
    }

    #[test]
    fn structural_check_handles_no_code_blocks() {
        let content = "Here is some text without code blocks.";
        let score = run_structural_check(content);
        // No brackets to check, should pass
        assert!(
            score >= 0.9,
            "Plain text without brackets should score high, got {}",
            score
        );
    }

    // ---- Pattern matching: hardcoded paths ----

    #[test]
    fn pattern_match_detects_hardcoded_unix_path() {
        let code = r#"config.path = "/usr/local/bin/app""#;
        let score = run_pattern_match(code);
        assert!(
            score < 1.0,
            "Should detect hardcoded /usr/ path, got {}",
            score
        );
    }

    #[test]
    fn pattern_match_detects_hardcoded_tmp_path() {
        let code = r#"temp_dir = "/tmp/cache""#;
        let score = run_pattern_match(code);
        assert!(
            score < 1.0,
            "Should detect hardcoded /tmp/ path, got {}",
            score
        );
    }

    #[test]
    fn pattern_match_detects_hardcoded_windows_path() {
        let code = r#"log_path = "C:\\Users\\admin\\logs""#;
        let score = run_pattern_match(code);
        assert!(
            score < 1.0,
            "Should detect hardcoded C:\\ path, got {}",
            score
        );
    }

    // ---- Pattern matching: empty catch/except ----

    #[test]
    fn pattern_match_detects_empty_catch_js() {
        let code = "try {\n  doSomething();\n} catch (e) {\n}";
        let score = run_pattern_match(code);
        assert!(
            score < 0.9,
            "Should detect empty catch block, got {}",
            score
        );
    }

    #[test]
    fn pattern_match_detects_empty_except_python() {
        let code = "try:\n    risky()\nexcept Exception:\n    pass\n";
        let score = run_pattern_match(code);
        assert!(
            score < 0.9,
            "Should detect empty except with pass, got {}",
            score
        );
    }

    // ---- Pattern matching: hardcoded secrets ----

    #[test]
    fn pattern_match_detects_hardcoded_password() {
        let code = r#"db.password = "super_secret_123""#;
        let score = run_pattern_match(code);
        assert!(
            score < 0.9,
            "Should detect hardcoded password, got {}",
            score
        );
    }

    #[test]
    fn pattern_match_detects_hardcoded_api_key() {
        let code = r#"api_key = "sk-abc123xyz""#;
        let score = run_pattern_match(code);
        assert!(
            score < 0.9,
            "Should detect hardcoded api_key, got {}",
            score
        );
    }

    // ---- Pattern matching: clean code passes ----

    #[test]
    fn pattern_match_passes_clean_code() {
        let code = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let score = run_pattern_match(code);
        assert_eq!(
            score, 1.0,
            "Clean code should score 1.0, got {}",
            score
        );
    }

    // ---- QualityGate: empty tools vacuously pass ----

    #[tokio::test]
    async fn quality_gate_empty_tools_vacuously_passes() {
        let gate = QualityGate::new(vec![], 0.65);
        let result = gate.verify("anything", None, None, None).await;
        assert!(result.passed, "Empty tools should vacuously pass");
        assert_eq!(result.score, 1.0);
        assert!(result.individual_scores.is_empty());
    }

    // ---- QualityGate: score aggregation ----

    #[tokio::test]
    async fn quality_gate_aggregates_multiple_tool_scores() {
        let gate = QualityGate::new(
            vec![
                VerificationTool::StructuralCheck,
                VerificationTool::PatternMatch,
            ],
            0.65,
        );

        let code = r#"```rust
fn add(a: i32, b: i32) -> i32 {
    a + b
}
```"#;
        let result = gate.verify(code, None, None, None).await;

        assert_eq!(result.individual_scores.len(), 2);
        assert!(
            result.individual_scores[0].0 == "structural_check",
            "First tool should be structural_check"
        );
        assert!(
            result.individual_scores[1].0 == "pattern_match",
            "Second tool should be pattern_match"
        );
        assert!(
            result.passed,
            "Clean code should pass, score = {}",
            result.score
        );
    }

    #[tokio::test]
    async fn quality_gate_fails_below_threshold() {
        let gate = QualityGate::new(
            vec![VerificationTool::PatternMatch],
            0.9, // High threshold
        );

        let bad_code = r#"password = "hardcoded_secret"
api_key = "sk-12345"
path = "/usr/local/bad"
"#;
        let result = gate.verify(bad_code, None, None, None).await;
        assert!(
            !result.passed,
            "Code with anti-patterns should fail high threshold, score = {}",
            result.score
        );
    }

    // ---- Schema validation ----

    #[test]
    fn schema_validation_passes_valid_json() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "age"],
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }
        });
        let content = r#"{"name": "Alice", "age": 30}"#;
        let score = run_schema_validator(content, Some(&schema));
        assert!(
            score >= 0.9,
            "Valid JSON matching schema should pass, got {}",
            score
        );
    }

    #[test]
    fn schema_validation_fails_missing_required() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "age"],
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }
        });
        let content = r#"{"name": "Alice"}"#;
        let score = run_schema_validator(content, Some(&schema));
        assert!(
            score < 1.0,
            "Missing required field should reduce score, got {}",
            score
        );
    }

    #[test]
    fn schema_validation_returns_one_when_no_schema() {
        let content = "anything";
        let score = run_schema_validator(content, None);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn schema_validation_handles_json_in_fences() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["status"],
            "properties": {
                "status": {"type": "string"}
            }
        });
        let content = "```json\n{\"status\": \"ok\"}\n```";
        let score = run_schema_validator(content, Some(&schema));
        assert!(
            score >= 0.9,
            "JSON in fences should be parsed, got {}",
            score
        );
    }

    // ---- LLM score parsing ----

    #[test]
    fn parse_llm_score_extracts_direct_number() {
        assert_eq!(parse_llm_score("0.85"), 0.85);
    }

    #[test]
    fn parse_llm_score_extracts_from_text() {
        let result = parse_llm_score("I would rate this 0.75");
        assert!((result - 0.75).abs() < 0.01, "got {}", result);
    }

    #[test]
    fn parse_llm_score_clamps_to_range() {
        assert_eq!(parse_llm_score("1.5"), 1.0);
        assert_eq!(parse_llm_score("-0.5"), 0.0);
    }

    #[test]
    fn parse_llm_score_defaults_on_invalid() {
        assert_eq!(parse_llm_score("not a number"), 0.5);
    }

    // ---- Code block extraction ----

    #[test]
    fn extract_single_code_block() {
        let md = "Some text\n```rust\nfn main() {}\n```\nMore text";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0.as_deref(), Some("rust"));
        assert!(blocks[0].1.contains("fn main()"));
    }

    #[test]
    fn extract_multiple_code_blocks() {
        let md = "```python\nprint(1)\n```\n```rust\nfn f() {}\n```";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn extract_no_code_blocks() {
        let blocks = extract_code_blocks("just plain text");
        assert!(blocks.is_empty());
    }

    // ---- Bracket balance ----

    #[test]
    fn bracket_balance_perfect() {
        let code = "fn main() { let v = vec![1, 2]; }";
        let score = check_bracket_balance(code);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn bracket_balance_missing_closing_brace() {
        let code = "fn main() { let x = 1;";
        let score = check_bracket_balance(code);
        assert!(score < 0.7, "Missing closing brace, got {}", score);
    }

    #[test]
    fn bracket_balance_ignores_strings() {
        let code = r#"let s = "hello { world }"; let a = (1 + 2);"#;
        let score = check_bracket_balance(code);
        assert_eq!(score, 1.0, "Brackets in strings should be ignored");
    }

    #[test]
    fn bracket_balance_ignores_comments() {
        let code = "// function with ( brackets\nfn main() {}";
        let score = check_bracket_balance(code);
        assert_eq!(score, 1.0, "Brackets in comments should be ignored");
    }

    // ---- Rust fn body check ----

    #[test]
    fn rust_fn_body_check_passes_with_body() {
        let code = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}";
        let score = check_rust_fn_body(code);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn rust_fn_body_check_handles_empty() {
        let code = "no functions here";
        let score = check_rust_fn_body(code);
        assert_eq!(score, 1.0);
    }

    // ---- Python def body check ----

    #[test]
    fn python_def_body_check_passes_with_body() {
        let code = "def greet(name):\n    return f'Hello {name}'";
        let score = check_python_def_body(code);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn python_def_body_check_fails_with_pass() {
        let code = "def placeholder():\n    pass";
        let score = check_python_def_body(code);
        assert!(
            score < 1.0,
            "def with pass should not score 1.0, got {}",
            score
        );
    }

    #[test]
    fn python_def_body_check_handles_empty() {
        let code = "x = 1";
        let score = check_python_def_body(code);
        assert_eq!(score, 1.0);
    }

    // ---- Strip JSON fences ----

    #[test]
    fn strip_json_fences_basic() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        let result = strip_json_fences(input);
        assert_eq!(result, "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_json_fences_no_fences() {
        let input = r#"{"key": "value"}"#;
        let result = strip_json_fences(input);
        assert_eq!(result, input);
    }

    // ---- Full QualityGate with schema ----

    #[tokio::test]
    async fn quality_gate_with_schema_validation() {
        let gate = QualityGate::new(
            vec![VerificationTool::SchemaValidator],
            0.65,
        );
        let schema = serde_json::json!({
            "type": "object",
            "required": ["result"],
            "properties": {
                "result": {"type": "string", "minLength": 1}
            }
        });

        let valid_json = r#"{"result": "success"}"#;
        let result = gate.verify(valid_json, Some(&schema), None, None).await;
        assert!(result.passed, "Valid JSON should pass schema validation, score = {}", result.score);

        let invalid_json = r#"{"other": "field"}"#;
        let result2 = gate.verify(invalid_json, Some(&schema), None, None).await;
        assert!(!result2.passed, "Missing required field should fail, score = {}", result2.score);
    }

    // ---- Unclosed strings ----

    #[test]
    fn unclosed_strings_detects_problem() {
        let code = "let s = \"hello";
        let score = check_unclosed_strings(code);
        assert!(score < 1.0, "Unclosed string should reduce score, got {}", score);
    }

    #[test]
    fn unclosed_strings_passes_clean() {
        let code = r#"let s = "hello";"#;
        let score = check_unclosed_strings(code);
        assert_eq!(score, 1.0);
    }

    // ---- TODO/FIXME detection ----

    #[test]
    fn pattern_match_penalizes_todo_comments() {
        let code = "fn main() {\n    // TODO: implement this\n    // FIXME: broken\n}";
        let score = run_pattern_match(code);
        assert!(
            score < 1.0,
            "TODO/FIXME comments should incur minor penalty, got {}",
            score
        );
        // Should be a small penalty, not a failure
        assert!(
            score > 0.8,
            "TODO/FIXME penalty should be minor, got {}",
            score
        );
    }
}

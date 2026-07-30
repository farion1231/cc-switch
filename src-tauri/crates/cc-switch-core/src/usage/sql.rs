//! 本地与远程 Usage 聚合共享的 SQL 语义常量。

/// 存储 `input_tokens` 已包含 cache read/write 的应用；新增 OpenAI 风格应用必须同步加入。
pub const CACHE_INCLUSIVE_APP_TYPES: &[&str] = &["codex", "gemini", "grokbuild"];

pub const INPUT_TOKEN_SEMANTICS_LEGACY: i64 = 0;
pub const INPUT_TOKEN_SEMANTICS_TOTAL: i64 = 1;
pub const INPUT_TOKEN_SEMANTICS_FRESH: i64 = 2;

pub fn is_cache_inclusive_app(app_type: &str) -> bool {
    CACHE_INCLUSIVE_APP_TYPES.contains(&app_type)
}

/// 生成单行 fresh-input 表达式；所有汇总必须通过此函数避免缓存 token 重复计数。
pub fn fresh_input_sql(alias: &str) -> String {
    let prefix = if alias.is_empty() {
        String::new()
    } else {
        format!("{alias}.")
    };
    let apps = CACHE_INCLUSIVE_APP_TYPES
        .iter()
        .map(|app| format!("'{app}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CASE \
         WHEN {prefix}input_token_semantics = {INPUT_TOKEN_SEMANTICS_FRESH} THEN {prefix}input_tokens \
         WHEN {prefix}app_type IN ({apps}) \
              AND {prefix}input_token_semantics = {INPUT_TOKEN_SEMANTICS_TOTAL} \
              AND {prefix}input_tokens >= ({prefix}cache_read_tokens + {prefix}cache_creation_tokens) \
         THEN {prefix}input_tokens - {prefix}cache_read_tokens - {prefix}cache_creation_tokens \
         WHEN {prefix}app_type IN ({apps}) \
              AND {prefix}input_token_semantics = {INPUT_TOKEN_SEMANTICS_LEGACY} \
              AND {prefix}input_tokens >= {prefix}cache_read_tokens \
         THEN {prefix}input_tokens - {prefix}cache_read_tokens \
         ELSE {prefix}input_tokens END"
    )
}

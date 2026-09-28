//! 环境变量占位符展开工具
//!
//! 支持 `$NAME` 与 `${NAME}` 两种语法。用于把 API Key 等敏感字段从
//! `~/.zshenv` / 1Password / launchd environment 注入 cc-switch 的
//! settings_config / auth.json 路径，避免明文落盘。
//!
//! 设计要点：
//! - **占位符识别**：仅识别首字符为 `$` 的整段；无 `$` 则原样返回。
//! - **大小写敏感**：与 POSIX shell 一致（macOS/Linux 的 getenv 行为）。
//! - **空值处理**：环境变量已定义但为空字符串视为"未设置"，与 bash
//!   `${VAR-default}` 不一致——更严格，避免「export FOO=」误用导致密钥
//!   真空。调用方可以用 `expand_or_warn` 拿到 missing 列表写日志。
//! - **转义**：不支持 `\${NAME}`。Shell 引用与本函数正交，由调用方在
//!   拼字符串时决定是否带引号。
//! - **递归**：不支持 `${${A}_B}` 这类嵌套；超出本工具范围。
//! - **副作用**：无；纯函数，方便单测。

use std::env;

/// 展开结果：成功展开后的字符串 + 缺失变量列表（用于打日志/审计）。
///
/// `expanded` 是把全部占位符替换过的字符串（无 `$` 字符或剩余 `$` 非占位符）。
/// `missing` 是「字符串里出现但进程环境里没定义或为空的变量名」。
/// `referenced` 是「字符串里出现过的所有变量名」（去重），用于审计"引用过
/// 但未提供"的占位符。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandResult {
    pub expanded: String,
    pub missing: Vec<String>,
    pub referenced: Vec<String>,
}

/// 展开单个字符串内的所有 `$NAME` / `${NAME}` 占位符。
///
/// 返回 [`ExpandResult`]；`missing` 非空时 `expanded` 中对应位置会留空
/// 字符串——由调用方决定是否当作错误。
pub fn expand_env(value: &str) -> ExpandResult {
    let mut out = String::with_capacity(value.len());
    let mut referenced: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut missing: Vec<String> = Vec::new();
    let mut missing_seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            // 复制一个 UTF-8 字符（ASCII 1 字节，非 ASCII 继续到下一字节首）
            if bytes[i] < 0x80 {
                out.push(value[i..i + 1].chars().next().unwrap());
                i += 1;
            } else {
                let ch_start = i;
                i += 1;
                while i < bytes.len() && (bytes[i] & 0xC0) == 0x80 {
                    i += 1;
                }
                out.push_str(&value[ch_start..i]);
            }
            continue;
        }

        // 处理 $：先看是不是 ${...}
        let next = bytes.get(i + 1).copied();
        if next == Some(b'{') {
            // 找匹配的 '}'；没找到则当作字面 "${..."
            if let Some(end_rel) = find_closing_brace(bytes, i + 2) {
                let name = &value[i + 2..i + 2 + end_rel];
                if is_valid_var_name(name) {
                    record_ref(&mut referenced, &mut seen, name);
                    match env::var(name) {
                        Ok(v) if !v.is_empty() => out.push_str(&v),
                        _ => record_missing(&mut missing, &mut missing_seen, name),
                    }
                    i = i + 2 + end_rel + 1;
                    continue;
                }
                // 非合法 var 名，按字面量处理
                out.push('$');
                out.push('{');
                i += 2;
                continue;
            }
            out.push('$');
            out.push('{');
            i += 2;
            continue;
        }

        // $NAME 形式：从 $ 后开始取连续 [A-Za-z0-9_]，但首字符必须是字母或下划线
        // （POSIX 规则；不允许 $5 / $1foo 这种数字开头的「伪变量」）。
        let name_start = i + 1;
        if name_start >= bytes.len() || !is_var_name_start(bytes[name_start]) {
            // 孤立的 '$' 后跟非变量起始字符 → 当字面量
            out.push('$');
            i += 1;
            continue;
        }
        let mut name_end = name_start;
        while name_end < bytes.len() && is_var_char(bytes[name_end]) {
            name_end += 1;
        }
        let name = &value[name_start..name_end];
        record_ref(&mut referenced, &mut seen, name);
        match env::var(name) {
            Ok(v) if !v.is_empty() => out.push_str(&v),
            _ => record_missing(&mut missing, &mut missing_seen, name),
        }
        i = name_end;
    }

    ExpandResult {
        expanded: out,
        missing,
        referenced,
    }
}

/// 若字符串里出现任何占位符但全部展开成功 → 返回 `Ok(expanded)`。
/// 若任一占位符缺失 → 返回 `Err(missing)`，调用方可写日志或拒绝写入。
pub fn expand_required(value: &str) -> Result<String, Vec<String>> {
    let r = expand_env(value);
    if r.missing.is_empty() {
        Ok(r.expanded)
    } else {
        Err(r.missing)
    }
}

/// 是否含 `$` 起始的占位符。给前端做"密钥字段是不是走 env"判定。
pub fn contains_placeholder(value: &str) -> bool {
    if !value.contains('$') {
        return false;
    }
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let next = bytes.get(i + 1).copied();
            if next == Some(b'{') {
                if let Some(end_rel) = find_closing_brace(bytes, i + 2) {
                    let name = &value[i + 2..i + 2 + end_rel];
                    if is_valid_var_name(name) {
                        return true;
                    }
                }
            } else if next.is_some_and(is_var_name_start) {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ---- helpers ----

fn find_closing_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'}' {
            return Some(i - start);
        }
        i += 1;
    }
    None
}

fn is_var_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_var_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_valid_var_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    // 第一个字符必须是字母或下划线（与 POSIX 一致）
    let first = bytes[0];
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    bytes.iter().all(|&b| is_var_char(b))
}

fn record_ref(
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    name: &str,
) {
    if seen.insert(name.to_string()) {
        out.push(name.to_string());
    }
}

fn record_missing(
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    name: &str,
) {
    if seen.insert(name.to_string()) {
        out.push(name.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // 环境变量在并行测试里会互相干扰，必须串行化。PoisonError 时也
    // 强制拿锁——一个 case 的 panic 不该污染后续 case。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(pairs: &[(&str, &str)], f: F) {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 保存并清理
        let saved: Vec<(String, Option<String>)> = pairs
            .iter()
            .map(|(k, _)| (k.to_string(), env::var(k).ok()))
            .collect();
        for (k, _) in pairs {
            env::remove_var(k);
        }
        for (k, v) in pairs {
            env::set_var(k, v);
        }
        f();
        // 还原
        for (k, _) in pairs {
            env::remove_var(k);
        }
        for (k, v) in saved {
            if let Some(v) = v {
                env::set_var(&k, v);
            }
        }
    }

    #[test]
    fn plain_passthrough() {
        let r = expand_env("hello world");
        assert_eq!(r.expanded, "hello world");
        assert!(r.missing.is_empty());
        assert!(r.referenced.is_empty());
    }

    #[test]
    fn empty_string() {
        let r = expand_env("");
        assert_eq!(r.expanded, "");
        assert!(r.missing.is_empty());
    }

    #[test]
    fn dollar_only_literal() {
        // "$" 后面不是合法变量字符时按字面处理
        let r = expand_env("price: $5 and $!boom");
        assert_eq!(r.expanded, "price: $5 and $!boom");
        assert!(r.missing.is_empty());
    }

    #[test]
    fn simple_dollar_var() {
        with_env(&[("CC_SWITCH_TEST_FOO", "bar")], || {
            let r = expand_env("prefix-$CC_SWITCH_TEST_FOO-suffix");
            assert_eq!(r.expanded, "prefix-bar-suffix");
            assert_eq!(r.referenced, vec!["CC_SWITCH_TEST_FOO".to_string()]);
            assert!(r.missing.is_empty());
        });
    }

    #[test]
    fn brace_var() {
        with_env(&[("CC_SWITCH_TEST_BAR", "value42")], || {
            let r = expand_env("a${CC_SWITCH_TEST_BAR}b");
            assert_eq!(r.expanded, "avalue42b");
        });
    }

    #[test]
    fn multiple_occurrences_dedup_referenced() {
        with_env(&[("CC_SWITCH_TEST_X", "X")], || {
            let r = expand_env("$CC_SWITCH_TEST_X-$CC_SWITCH_TEST_X");
            assert_eq!(r.expanded, "X-X");
            // referenced 去重
            assert_eq!(r.referenced, vec!["CC_SWITCH_TEST_X".to_string()]);
        });
    }

    #[test]
    fn missing_var_leaves_empty_and_lists_missing() {
        let r = expand_env("a$CC_SWITCH_TEST_DEFINITELY_NOT_SET b");
        assert_eq!(r.expanded, "a b");
        assert_eq!(r.missing, vec!["CC_SWITCH_TEST_DEFINITELY_NOT_SET".to_string()]);
        assert_eq!(
            r.referenced,
            vec!["CC_SWITCH_TEST_DEFINITELY_NOT_SET".to_string()]
        );
    }

    #[test]
    fn empty_env_value_treated_as_missing() {
        with_env(&[("CC_SWITCH_TEST_EMPTY", "")], || {
            let r = expand_env("a$CC_SWITCH_TEST_EMPTY b");
            assert_eq!(r.expanded, "a b");
            assert_eq!(r.missing, vec!["CC_SWITCH_TEST_EMPTY".to_string()]);
        });
    }

    #[test]
    fn unclosed_brace_is_literal() {
        // "${CC_SWITCH_TEST_UNCLOSED" 没有 '}'，整个按字面处理。
        // 设计选择：未闭合 ${ 不展开（与 bash 的「按字面直到 EOF」不同，
        // 我们的目的是「明确写错就报错」，避免误把 ${ 当成 $NAME 吃字）。
        let r = expand_env("prefix-${CC_SWITCH_TEST_UNCLOSED no close");
        assert_eq!(r.expanded, "prefix-${CC_SWITCH_TEST_UNCLOSED no close");
    }

    #[test]
    fn invalid_var_name_brace_is_literal() {
        // "${1FOO}" 不是合法 var 名，整个按字面 "$" + "{" 处理
        let r = expand_env("x${1FOO}y");
        assert_eq!(r.expanded, "x${1FOO}y");
        assert!(r.referenced.is_empty());
    }

    #[test]
    fn underscore_var_name_ok() {
        with_env(&[("_CC_SWITCH_TEST_UNDER", "U")], || {
            let r = expand_env("${_CC_SWITCH_TEST_UNDER}");
            assert_eq!(r.expanded, "U");
        });
    }

    #[test]
    fn mixed_with_text() {
        with_env(
            &[("CC_SWITCH_TEST_K", "kvalue"), ("CC_SWITCH_TEST_V", "vvalue")],
            || {
                let r = expand_env("k=$CC_SWITCH_TEST_K&v=${CC_SWITCH_TEST_V}&end");
                assert_eq!(r.expanded, "k=kvalue&v=vvalue&end");
                assert_eq!(
                    r.referenced,
                    vec!["CC_SWITCH_TEST_K".to_string(), "CC_SWITCH_TEST_V".to_string()]
                );
                assert!(r.missing.is_empty());
            },
        );
    }

    #[test]
    fn expand_required_ok_when_no_missing() {
        with_env(&[("CC_SWITCH_TEST_REQ", "yes")], || {
            let r = expand_required("$CC_SWITCH_TEST_REQ");
            assert_eq!(r.unwrap(), "yes");
        });
    }

    #[test]
    fn expand_required_err_lists_missing() {
        let r = expand_required("$CC_SWITCH_TEST_NOPE_X");
        let err = r.unwrap_err();
        assert_eq!(err, vec!["CC_SWITCH_TEST_NOPE_X".to_string()]);
    }

    #[test]
    fn contains_placeholder_detects_dollar_var() {
        assert!(contains_placeholder("$FOO"));
        assert!(contains_placeholder("prefix-$FOO"));
        assert!(contains_placeholder("${FOO}"));
        assert!(!contains_placeholder("plain text"));
        assert!(!contains_placeholder("price: $5"));
        // "$" 后面接非法字符，不算占位符
        assert!(!contains_placeholder("$!boom"));
    }

    #[test]
    fn utf8_passthrough() {
        // 中文/emoji 路径不应被 byte 切割破坏
        let r = expand_env("中文-$CC_SWITCH_TEST_UTF8-🎉");
        // CC_SWITCH_TEST_UTF8 未设置，missing
        assert!(r.missing.contains(&"CC_SWITCH_TEST_UTF8".to_string()));
        assert_eq!(r.expanded, "中文--🎉");
    }

    #[test]
    fn chinese_var_name_rejected() {
        // 中文不是合法 POSIX var 名 → 字面处理
        let r = expand_env("$中文变量");
        assert_eq!(r.expanded, "$中文变量");
        assert!(r.missing.is_empty());
    }
}
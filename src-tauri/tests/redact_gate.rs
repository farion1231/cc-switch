//! 静态门禁：`src/remote/**` 里任何 HTTP 错误体都必须走脱敏出口。
//!
//! 背景：`/api/usage` 与 `/api/provider/test` 的错误字符串**逐字透传**上游响应体
//! 与 usage JS 脚本的异常文本，而这两条透传源都带密钥。上一轮修法是在单个分支
//! （`resolve_copilot_base_url_override` 的 `Err`）补一次 `redact_error`，结果被
//! 证明无效 —— 同一函数里还有十几处 `"error": …` 构造，今天补一处、明天新增路由
//! 又漏一处，而没有任何东西会红。
//!
//! 所以这道门禁的作用不是「检查某几处改对了」，而是**让漏网在门禁期就红**：
//! 凡是 `json!` 里形如 `"error": <expr>` 的字段，`<expr>` 不是字符串字面量时，
//! 就必须被 `error_json` / `error_json_body` / `remote_error` / `redact_error`
//! 之一包裹。
//!
//! 两条边界的说明：
//!
//! - **字符串字面量放行**：`"Server is stopped"` 这类常量是本仓库自己写的，不含
//!   上游数据，抹它只会让排障文本变噪声（见 `redact_error` 模块的反向约束）。
//!   但字面量里手写一个真 key 是能被 code review 看见的 —— 这条豁免买的是
//!   「不制造不可读的错误文本」，代价是常量正确性仍靠人看。
//! - **扫描整个 `src/remote/` 递归目录**，不是硬编码几个文件名：门禁的作用范围
//!   必须覆盖将来新增的子模块（含 `providers/*` 这类拆分），硬编码清单会让新
//!   文件默认在门外，等于把「漏网」重新变成默认值。

use std::fs;
use std::path::{Path, PathBuf};

/// 脱敏出口的函数名。命中任意一个即视为「这条错误文本过了闸门」。
///
/// 名单是封闭的：新增一个自称「我内部会脱敏」的包装函数，必须同时把自己加进
/// 这个名单 —— 加名字是一次有 diff 的、需要人看的动作，而漏包是零动作的。
const WRAPPERS: &[&str] = &[
    "error_json(",
    "error_json_body(",
    "remote_error(",
    "redact_error(",
];

/// `"error":` 在行内的位置。写成常量是为了让下面的解析器与夹具共用同一个串，
/// 少一次「门禁扫描的形状」与「门禁认得的形状」漂移的机会。
const ERROR_KEY: &str = "\"error\":";

/// `src/remote/` 的绝对路径。
///
/// 由 `CARGO_MANIFEST_DIR` 拼出：这个仓会被 clone 到很多台机器上，写死任何人的
/// 家目录都会让门禁在别人那里直接读不到文件（而「读不到」必须表现为红，不是绿）。
fn remote_source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/remote")
}

/// 递归收集目录下全部 `.rs` 文件，路径稳定排序。
///
/// 排序不是洁癖：违规报告按文件顺序打印，不排序的话同一份代码在两次运行里给出
/// 不同的报告顺序，review 时没法逐条对照。
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current)
            .unwrap_or_else(|e| panic!("无法读取 {}：{e}", current.display()));
        for entry in entries {
            let path = entry.expect("read_dir 条目不可读").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    out.sort();
    out
}

/// 从一行里取出所有 `"error": <expr>` 的 `<expr>`。
///
/// 取值靠一个极小的扫描器而不是「切到行尾再剥几个字符」，因为两种朴素写法都会
/// 在真实代码上切错：
///
/// - 切到行尾：`e.to_string()})).into_response(),` 会把 `.into_response()` 一起
///   当成值的一部分，字符串字面量的判定随之失效。
/// - 剥尾部 `)`：`redact::redact_error(&e.to_string())` 的两个右括号是表达式的
///   一部分；剥掉后判定虽然还成立，但值已经不是代码里真实的那个表达式了。
///
/// 扫描器只做一件事：跳过空白后，按括号深度与字符串字面量读到「深度归零的
/// `,` / `}` / `)`」为止。字符串内的 `{}`、`)` 一律跳过 —— `format!("供应商 {}
/// 不存在", id)` 里那个 `}` 不是结构。
///
/// 行尾就是值为空：`"error":` 后面换行写了值（rustfmt 对超长 json! 会这么排）。
/// 这种情况返回空串而不是跳过 —— 换行是门禁检测器最容易被绕过的排版手段，
/// 「解析不了」必须算违规，不能算通过。
fn error_values_in_line(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;

    while let Some(rel) = line[from..].find(ERROR_KEY) {
        let start = from + rel + ERROR_KEY.len();
        out.push(scan_value(&bytes[start..]));
        from = start;
    }

    out
}

/// 从 `bytes`（值所在位置起）读出完整的值表达式。
///
/// 分两种形态：字符串字面量（读到闭合引号为止）与表达式（读到深度归零的分隔符
/// 为止）。两者都原样返回，判定交给 [`is_redacted`]。
fn scan_value(bytes: &[u8]) -> String {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return String::new();
    }

    // 字符串字面量：连引号一起返回，让 is_redacted 能认出「这是常量」
    if bytes[i] == b'"' {
        let mut j = i + 1;
        while j < bytes.len() {
            match bytes[j] {
                b'\\' => j += 2,
                b'"' => return String::from_utf8_lossy(&bytes[i..=j]).to_string(),
                _ => j += 1,
            }
        }
        // 引号没闭合（跨行排版的字面量）：整段返回，判定必然是违规
        return String::from_utf8_lossy(&bytes[i..]).trim_end().to_string();
    }

    let mut depth = 0i32;
    let mut j = i;
    while j < bytes.len() {
        match bytes[j] {
            b'"' => {
                // 跳过整段字符串：里面的括号不是结构
                j += 1;
                while j < bytes.len() {
                    match bytes[j] {
                        b'\\' => j += 2,
                        b'"' => break,
                        _ => j += 1,
                    }
                }
                j += 1;
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                j += 1;
            }
            b')' | b']' | b'}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                j += 1;
            }
            b',' if depth == 0 => break,
            _ => j += 1,
        }
    }

    String::from_utf8_lossy(&bytes[i..j]).trim_end().to_string()
}

/// 一行是不是注释。注释里的 `"error":` 不构造任何东西。
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// 该值表达式是否已过脱敏闸门。
///
/// 判据两条且必须同时成立才算「豁免」：
///
/// - 值是完整的字符串字面量（首尾都是 `"`）→ 本仓库写死的常量，放行；
/// - 否则值里出现某个 [`WRAPPERS`] 名字 → 过了闸门。
///
/// 另有唯一的**非字符串**豁免：`None`。它是 `Option` 的空值，不携带任何文本，
/// 没有可泄漏的东西 —— 而 `ProviderUsageDto` 的成功路径必然要写它，不豁免就逼着
/// 人去改造型，白白把门禁的可信度换成一次噪音。
///
/// 其余一律违规，包括**空值**：空值意味着值在下一行，检测器读不到，按违规处理。
fn is_redacted(value: &str) -> bool {
    let trimmed = value.trim();
    let is_literal = trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"');
    let is_none = trimmed == "None";
    let is_some_literal = is_some_of_a_literal(trimmed);
    let is_fn_ref = trimmed.ends_with(")") && last_call_segment_is_a_wrapper(trimmed);
    is_literal
        || is_none
        || is_some_literal
        || is_fn_ref
        || WRAPPERS.iter().any(|w| trimmed.contains(w))
}

/// 值是不是恰好 `Some(<字符串字面量>)` 这个形状。
///
/// 窄到整体形状匹配，而不是「里面有引号」：后者会让 `Some(e.to_string())` 这类
/// 真实透传蒙混过关 —— 它内部确实没引号，但 `Some(format!("a{}",b))` 之类一旦被
/// 放行，豁免就等于没有。
fn is_some_of_a_literal(value: &str) -> bool {
    let Some(inner) = value
        .strip_prefix("Some(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    let inner = inner.trim();
    inner.len() >= 2 && inner.starts_with('"') && inner.ends_with('"')
}

/// 值的最后一个实参是不是一个脱敏函数的路径（作为函数引用被收下）。
///
/// 真实源码里 `error` 字段的脱敏有两种等价写法：直接调用
/// `redact_error(&e.to_string())`，以及把脱敏函数本身交给迭代器
/// （`.map(redact_error)`）。后者在文本上**不以函数名开头**，含子串判定会漏，
/// 但它确实过了闸门 —— 判据必须是「最后一对括号里收下的是脱敏函数」。
///
/// 实参还必须是**纯路径**（只含标识符字符、`.` 和 `:`）：放行任意表达式会让
/// `format!("{}", redact_error)` 之外的形状一起漏进来，而这类形状正是豁免该
/// 挡住的。
fn last_call_segment_is_a_wrapper(value: &str) -> bool {
    let Some(open) = value.rfind('(') else {
        return false;
    };
    let arg = value[open + 1..]
        .trim_end()
        .strip_suffix(')')
        .unwrap_or_default();
    if arg.is_empty()
        || !arg
            .chars()
            .all(|c| c.is_alphanumeric() || "_.:".contains(c))
    {
        return false;
    }
    WRAPPERS
        .iter()
        .map(|w| w.trim_end_matches('('))
        .any(|name| arg == name || arg.ends_with(&format!("::{name}")))
}

/// 扫描一份源码，返回全部违规行（`行号: 原文`）。
fn violations_in(source: &str) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !is_comment(line))
        .filter(|(_, line)| line.contains(ERROR_KEY))
        .filter(|(_, line)| error_values_in_line(line).iter().any(|v| !is_redacted(v)))
        .map(|(i, _)| i + 1)
        .collect()
}

/// 扫描一份源码里**结构体字面量**的 `error:` 字段，返回全部违规行号。
///
/// 与 [`violations_in`] 是两个检测器而不是一个，因为两者的字段形状根本不同：
/// `json!` 里是 `"error": <expr>`（带引号的 key），`#[derive(Serialize)]` 的
/// 响应体里是 `error: <expr>`（裸 key）。只留后者的话 `json!` 分支会失明，
/// 只留前者的话 DTO 分支会失明 —— 而后者正盖着 `/api/usage` 的整个响应。
fn struct_error_violations(source: &str) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !is_comment(line))
        .filter(|(_, line)| is_struct_error_field(line))
        .filter(|(_, line)| !is_redacted(struct_error_value(line)))
        .map(|(i, _)| i + 1)
        .collect()
}

/// 这一行是不是结构体字面量里的 `error:` 字段构造（而不是类型声明）。
///
/// 判据是**行首**裸 `error:`：结构体字面量的字段都在 `{ … }` 内部、缩进更深，
/// 且写法是 `error: <expr>,`；而类型声明是 `pub error: Option<String>,`，
/// 行首带 `pub`。按行首匹配而不是「行内出现 `error:`」是为了不把
/// `usage_result.error.clone()` 这类**字段读取**误当成响应体字段 —— 前者要判
/// 红，后者只是取值路径。
fn is_struct_error_field(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("error:") && !trimmed.starts_with("pub error:")
}

/// 取出结构体 `error:` 字段的值表达式。
///
/// 值一直取到行尾为止 —— 结构体字段是单行的，取到逗号为止和取到行尾在真实代码
/// 上等价，而取到逗号会把 `Some(redact_error(&e.to_string())),` 里的 `)` 提前切掉
/// 半个（`scan_value` 的深度归零规则在这里反而帮忙，但一致性更好的是直接取到
/// 行尾，让 [`is_redacted`] 看到完整的包裹形态）。
fn struct_error_value(line: &str) -> &str {
    line.trim_start()
        .strip_prefix("error:")
        .unwrap_or_default()
        .trim()
        .trim_end_matches(',')
        .trim()
}

// ---------------------------------------------------------------------------
// 主张 1b：结构体字面量里的 `error:` 字段同样要过闸门
// ---------------------------------------------------------------------------
//
// `#[derive(Serialize)]` 的响应体（`ProviderUsageDto` 就是 `/api/usage` 的整个
// 响应）是靠**结构体字段**构造的，不是 `json!` 字面量。检测器只看
// `"error":`（带引号），对它完全失明 —— 而这恰恰是第一个要覆盖的接口。
//
// 失明的后果不是「漏一处」，是**整个接口在门外**：这一条漏掉，`json!` 扫描全绿
// 也证明不了 `/api/usage` 的错误文本过了闸门。

/// 反向夹具：结构体字面量里的裸透传必须被判违规。
#[test]
fn fixture_with_struct_literal_error_field_is_rejected() {
    let fixture = r#"
fn build(usage_result: UsageResult) -> ProviderUsageDto {
    ProviderUsageDto {
        provider: "p1".to_string(),
        success: false,
        // 逐字透传脚本异常文本
        error: usage_result.error.clone(),
    }
}
"#;

    let bad = struct_error_violations(fixture);
    assert_eq!(
        bad.len(),
        1,
        "结构体字面量里的裸透传必须被判 1 处违规，实际 {bad:?}\n{fixture}"
    );
}

/// 结构体字面量里走了脱敏出口的写法必须判干净。
#[test]
fn fixture_with_redacted_struct_literal_field_is_clean() {
    let fixture = r#"
fn build(usage_result: UsageResult, e: anyhow::Error) -> ProviderUsageDto {
    ProviderUsageDto {
        provider: "p1".to_string(),
        success: false,
        error: Some(redact::redact_error(&e.to_string())),
    }
}

fn build2(usage_result: UsageResult) -> ProviderUsageDto {
    ProviderUsageDto {
        success: true,
        error: None,
    }
}
"#;

    assert_eq!(
        struct_error_violations(fixture),
        Vec::<usize>::new(),
        "已走脱敏出口 / None 的结构体字段被误判了"
    );
}

/// `None` 是唯一的裸值豁免：它不携带任何文本，没有可泄漏的东西。
#[test]
fn struct_field_none_is_the_only_bare_exemption() {
    for (value, want_violation) in [
        ("None", false),
        ("usage_result.error.clone()", true),
        ("e.to_string()", true),
        ("err.message", true),
        ("Some(redact_error(&e.to_string()))", false),
    ] {
        let line = format!("        error: {value},\n");
        assert_eq!(
            struct_error_violations(&line).is_empty(),
            !want_violation,
            "`{value}` 的判定与预期相反"
        );
    }
}

/// `pub error: Option<String>` 是类型声明，不是响应体，不该被判。
///
/// 判据是行首的 `pub`：响应体的字段构造都在结构体字面量内部，缩进更深且没有
/// `pub`。把声明一起判红会让这道门禁逼着人给声明加豁免，最后整条门禁被架空。
#[test]
fn struct_field_declaration_is_not_a_construction() {
    let fixture = "\npub struct ProviderUsageDto {\n    pub error: Option<String>,\n}\n";
    assert_eq!(
        struct_error_violations(fixture),
        Vec::<usize>::new(),
        "类型声明被误判成响应体构造"
    );
}

// ---------------------------------------------------------------------------
// 主张 1：真实源码零违规，且扫描确实覆盖到了东西
// ---------------------------------------------------------------------------

#[test]
fn every_remote_error_field_goes_through_a_redacting_exit() {
    let dir = remote_source_dir();
    assert!(
        dir.is_dir(),
        "扫描目录不存在：{}（门禁读不到文件时必须红，而不是静默通过）",
        dir.display()
    );

    let sources = rust_sources(&dir);
    assert!(
        sources.len() >= 3,
        "在 {} 下只找到 {} 个 .rs 文件，扫描范围明显不对",
        dir.display(),
        sources.len()
    );

    let mut total_fields = 0usize;
    let mut total_struct_fields = 0usize;
    let mut offenders = Vec::new();

    for path in &sources {
        let source =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("无法读取 {}：{e}", path.display()));
        total_fields += source
            .lines()
            .filter(|l| !is_comment(l) && l.contains(ERROR_KEY))
            .count();
        total_struct_fields += source
            .lines()
            .filter(|l| !is_comment(l) && is_struct_error_field(l))
            .count();

        let found: Vec<usize> = violations_in(&source)
            .into_iter()
            .chain(struct_error_violations(&source))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for line_no in found {
            let line = source.lines().nth(line_no - 1).unwrap_or_default();
            offenders.push(format!("{}:{}: {}", path.display(), line_no, line.trim()));
        }
    }

    assert!(
        total_fields > 0,
        "一次 error 字段都没扫到 —— 检测器空转了，绿色的门禁不代表任何东西"
    );
    // 只锁 `json!` 分支时这条会一直绿，而 `#[derive(Serialize)]` 的响应体
    // （`ProviderUsageDto` = `/api/usage` 的整个响应）整体在覆盖之外。
    assert!(
        total_struct_fields > 0,
        "一个结构体 `error:` 字段都没扫到 —— DTO 检测器空转了，\
         /api/usage 的响应体形状不在门禁覆盖内"
    );
    assert!(
        offenders.is_empty(),
        "以下错误字段的取值没有过脱敏闸门。\n\
         修法：改走 `remote_error(status, detail)`（脱敏在出口里强制发生），\
         不要自己拼 JSON。\n{}\n",
        offenders.join("\n")
    );
}

// ---------------------------------------------------------------------------
// 主张 2：反向夹具 —— 检测器必须真的会红
// ---------------------------------------------------------------------------

/// 反向夹具：裸透传（正是上一轮被判定无效的那种写法）必须被判违规。
///
/// 没有它，一个恒返回「零违规」的检测器能让门禁永远绿 —— 而门禁的全部价值就在
/// 于它会红。
#[test]
fn fixture_with_bare_passthrough_is_rejected() {
    let fixture = r#"
fn test_provider(provider_id: String, e: anyhow::Error) -> Response {
    // 逐字透传上游响应体 / 脚本异常
    return Json(json!({"success": false, "error": e.to_string()})).into_response();
}
"#;

    let bad = violations_in(fixture);
    assert_eq!(
        bad.len(),
        1,
        "夹具里的裸透传必须被判 1 处违规，实际 {bad:?}\n{fixture}"
    );
    assert!(
        fixture
            .lines()
            .nth(bad[0] - 1)
            .unwrap()
            .contains("e.to_string()"),
        "报出的行号指错了地方"
    );
}

/// 反向夹具：走了脱敏出口的写法必须被判干净。
///
/// 这一条防的是另一个方向 —— 检测器过宽时，它会把「把整个文件抹掉」也判绿，
/// 逼着后来的人去加豁免，最后整条门禁被架空。
#[test]
fn fixture_with_redacted_exit_is_clean() {
    let fixture = r#"
fn test_provider(provider_id: String, e: anyhow::Error) -> Response {
    return remote_error(StatusCode::OK, e);
}

fn missing_provider(id: String) -> Response {
    return remote_error(StatusCode::OK, format!("供应商 {id} 不存在"));
}

fn stopped() -> Response {
    // 常量错误文本不是上游数据，豁免
    return Json(json!({"error": "Server is stopped"})).into_response();
}

fn comment_is_not_a_field() {
    // "error": e.to_string(),
}
"#;

    assert_eq!(
        violations_in(fixture),
        Vec::<usize>::new(),
        "已走脱敏出口 / 常量 / 注释的夹具被误判了"
    );
}

/// `Some(<字符串字面量>)` 与「把脱敏函数当函数引用传」这两种形态都必须放行，
/// 否则真实源码（`pairing` 的状态码常量、`ProviderUsageDto` 的 `.map` 写法）
/// 会把门禁逼成噪音 —— 而一道天天误报的静态门禁最后一定是被加豁免关掉的。
///
/// 两条豁免都**窄到具体形态**：只放行 `Some("...")` 这个整体形状，和末段
/// 恰好是脱敏函数名的路径引用。放开一点点就会变成新的绕过手段。
#[test]
fn narrow_exemptions_do_not_become_a_bypass() {
    // 放行：常量包在 Some 里
    for value in [
        r#"Some("unauthorized")"#,
        r#"Some("rate_limited")"#,
        // 放行：脱敏函数作为函数引用被 .map 收下
        "usage_result.error.as_deref().map(redact::redact_error)",
        "usage_result.error.as_deref().map(redact_error)",
    ] {
        let line = format!("            error: {value},\n");
        assert_eq!(
            struct_error_violations(&line),
            Vec::<usize>::new(),
            "合法形态被误判：{value}"
        );
    }

    // 判红：同样的前缀，一换内容就越界
    for value in [
        r#"Some(e.to_string())"#,
        r#"Some(usage_result.error.clone())"#,
        r#"format!("x: {}", err)"#,
        // 名字出现在别处不等于过了闸门
        r#"redact_error_note"#,
        r#"self.redact_error"#,
    ] {
        let line = format!("            error: {value},\n");
        assert_eq!(
            struct_error_violations(&line),
            vec![1],
            "越界形态被放过：{value}"
        );
    }
}

/// 换行写的值不能成为绕过门禁的排版手段。
///
/// rustfmt 对超长 `json!` 会把值挪到下一行。如果解析器读到行尾就认为「值是空的、
/// 空的等于没问题」，那么把 `"error":` 后面的东西换一行就能让任何透传蒙混过关
/// —— 这正是门禁最容易被无意架空的方式。
#[test]
fn value_on_the_next_line_is_not_a_bypass() {
    let fixture =
        "    Json(json!({\"success\": false, \"error\":\n        e.to_string(),\n    }))\n";
    assert_eq!(violations_in(fixture), vec![1], "把值挪到下一行绕过了门禁");
}

// ---------------------------------------------------------------------------
// 检测器自身：形状漂移要红，而不是绿
// ---------------------------------------------------------------------------

#[test]
fn detector_recognizes_every_declared_wrapper() {
    for wrapper in WRAPPERS {
        let wrapped = format!("    Json(json!({{\"error\": {wrapper}\"boom\"}}))\n");
        assert_eq!(
            violations_in(&wrapped),
            Vec::<usize>::new(),
            "名单里的 {wrapper} 没被识别为脱敏出口 —— 加了名字却没接上检测器"
        );

        // 同一个值去掉包裹后必须立刻变红，否则「认得」只是碰巧
        let bare = "    Json(json!({\"error\": boom})).into_response()\n";
        assert_eq!(
            violations_in(bare),
            vec![1],
            "同一个值去掉包裹后竟然没变红，检测器对 {wrapper} 的判定不成立"
        );
    }
}

#[test]
fn non_string_literal_without_a_wrapper_is_rejected() {
    // 变量、`format!`、方法调用、字段读取：全部是运行时文本，一律要过闸门
    for value in [
        "detail",
        "format!(\"供应商 {} 不存在\", id)",
        "e.to_string()",
        "err.message",
        "usage_result.error",
        "response.body",
    ] {
        let line = format!("    Json(json!({{\"error\": {value}}}))\n");
        assert_eq!(
            violations_in(&line),
            vec![1],
            "`{value}` 是运行时文本却没被判违规"
        );
    }
}

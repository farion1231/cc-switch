//! 远程错误字符串的脱敏闸门
//!
//! 背景：`/api/usage` 与 `/api/provider/test` 的错误字符串**逐字透传**上游响应体
//! 与用户自定义 usage JS 脚本的异常文本，而这两条透传源都带密钥 —— 上游鉴权失败
//! 会回显请求里的 `Authorization` / `api_key` 片段；usage 脚本的上下文里注入了
//! 明文 `api_key`（见 `services/provider/usage.rs`），脚本一旦回显变量就原样吐出。
//! 远程服务一旦绑到 LAN，这两条就是凭据外泄口。
//!
//! 已在窄白名单上的 `/api/providers`、`/api/current` 不经过这里：那两条返回的是
//! 逐字段挑选过的 DTO，脱敏对它们是 no-op，也不该在这里「顺手」加一层。
//!
//! 反向约束同样重要：不含密钥的正常错误文本（超时、404、供应商不存在……）必须
//! 原样可读。全量抹成同一个占位符等于把远程排障能力一并删掉 —— 那等于把洞换了
//! 个位置，而不是堵上。

use once_cell::sync::Lazy;
use regex::Regex;

/// 脱敏后单条错误的最大字节数。超长错误原样下发会被拿来当数据通道用。
pub const MAX_ERROR_LEN: usize = 512;

/// 密钥占位符。
pub const REDACTED: &str = "[REDACTED:API_KEY]";

/// `sk-` 开头的 key：前缀加其后整段 token，一起替换。
///
/// 匹配到「前缀 + 至少 8 个 token 字符」为止：更短的 `sk-` 片段（文档里的
/// `sk-xxx` 占位示例）不是密钥，抹掉它只会让错误文本变得看不懂。
static SK_KEY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"sk-[A-Za-z0-9_\-]{8,}").expect("sk- 形态的 key 正则是常量，必须能编译")
});

/// `Authorization` 头的赋值：保留头名与 scheme，只替换 token 本身。
///
/// 保留前缀不是装饰 —— 「401 而且我们确实发了 Authorization 头」和
/// 「401 但没带头」排查动作完全不同，把头名一起抹掉等于把远程排障信息删了。
/// 头名的 JSON 形态（`"Authorization": "Bearer …"`）一并覆盖，因为上游回显
/// 通常把请求头序列化成 JSON 塞进错误体。
static AUTH_HEADER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)(authorization["']?\s*[:=]\s*["']?\s*)((?:bearer|basic|token)\s+)?([^\s"',}\]]+)"#,
    )
    .expect("Authorization 形态的正则是常量，必须能编译")
});

/// 无前缀的裸长十六进制串（32 位以上）。
///
/// 下限取 32：短于此的十六进制串是哈希片段或错误码，不是密钥。
static LONG_HEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b[0-9a-f]{32,}\b").expect("长十六进制正则必须能编译"));

/// 对错误字符串做脱敏 + 截断，使其可安全下发给远程调用方。
///
/// 顺序是**脱敏在前、截断在后**，不能反：反过来的话，一个跨过 512 字节边界的
/// key 会被切成半截，前半截原样下发 —— 密钥照样泄漏，只是短了一点。
///
/// 三条正则互不重叠的顺序也有讲究：`sk-<hex>` 同时命中 `SK_KEY` 和
/// `LONG_HEX` 里的那截十六进制，`SK_KEY` 先跑才能替换掉整个 key，而不是留下
/// 一个「sk- 后面原来有串东西」的标记。
pub fn redact_error(raw: &str) -> String {
    let masked = SK_KEY.replace_all(raw, REDACTED);
    let masked = AUTH_HEADER.replace_all(&masked, "${1}${2}".to_string() + REDACTED);
    let masked = LONG_HEX.replace_all(&masked, REDACTED);

    truncate_on_char_boundary(&masked)
}

/// 按 [`MAX_ERROR_LEN`] 截断，切点落在字符边界上。
fn truncate_on_char_boundary(s: &str) -> String {
    if s.len() <= MAX_ERROR_LEN {
        return s.to_string();
    }
    let mut end = MAX_ERROR_LEN;
    // UTF-8 多字节字符不能被从中间切开，`is_char_boundary` 向前回退到最近的边界。
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// 构造脱敏后的错误体（`serde_json::Value` 形态）。
///
/// 这是错误文本**唯一**的 JSON 出口：脱敏在这里强制发生，调用点拿到的已经是
/// 成品，因此「先拼好 JSON 再想起脱敏」这个顺序错误不可表达 —— 它根本没有拼
/// JSON 的入口。
///
/// 参数刻意只收 `Display` 而不收 `serde_json::Value`：收 `Value` 就能把
/// `json!({"error": e.to_string()})` 原样递进来，脱敏会作用在一个已经成型的
/// 结构上（要么漏掉嵌套里的错误文本，要么整段被当成一个字符串去抹）。
pub fn error_json_body(detail: impl std::fmt::Display) -> serde_json::Value {
    serde_json::json!({
        "success": false,
        "error": redact_error(&detail.to_string()),
    })
}

/// 构造脱敏后的错误 HTTP 响应（`axum::response::Response` 形态）。
///
/// 与 [`error_json_body`] 同源，只是多带一个状态码 —— handler 里绝大多数分支是
/// `return <构造器>(..)`，收 `Response` 才能让「忘记 `.into_response()`」变成
/// 编译错误而不是运行时形状错误。
///
/// `success: false` 恒定：调用方只需要一个「这个响应是错误」的判据，而不同
/// handler 各自拼的字段名（`error` / `message` / `action`）不能作为判据。
pub fn error_json(
    status: axum::http::StatusCode,
    detail: impl std::fmt::Display,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    (status, axum::Json(error_json_body(detail))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{error_json, error_json_body, redact_error, MAX_ERROR_LEN, REDACTED};

    /// `sk-` 前缀的 key 必须整段换成占位符，前后的普通文本保持可读。
    ///
    /// 这是脚本与上游回显里最常见的一种形态：key 本身是
    /// `sk-` + 一长串十六进制，整体替换而不是只替换十六进制那截，
    /// 否则占位符前面会留下一个「这串东西是 key」的标记。
    #[test]
    fn redacts_sk_prefixed_key() {
        let out =
            redact_error("upstream said: invalid key sk-deadbeef00112233445566778899aabbccddeeff");
        assert_eq!(out, format!("upstream said: invalid key {REDACTED}"));
        assert!(
            !out.contains("deadbeef00112233445566778899aabbccddeeff"),
            "sk- key 的字面量仍然出现在下发文本里：{out}"
        );
    }

    /// `Authorization` 头的 token 片段同样要替换，但 `Authorization: Bearer ` 这段
    /// 结构信息必须留着 —— 丢掉它就看不出「是鉴权头出的问题」。
    #[test]
    fn redacts_authorization_token() {
        let out = redact_error("401 from upstream, sent Authorization: Bearer abc.def-ghi_jkl");
        assert!(
            out.contains(REDACTED),
            "Authorization token 未被脱敏：{out}"
        );
        assert!(
            !out.contains("abc.def-ghi_jkl"),
            "token 字面量仍然出现在下发文本里：{out}"
        );
        assert!(
            out.contains("Authorization"),
            "Authorization 这个结构信息被抹掉了，排障信息丢失：{out}"
        );
    }

    /// 没有 `sk-` 前缀、但形态上就是密钥的裸长十六进制串也要替换。
    ///
    /// 取 32 是有意的：短于此的十六进制串（`deadbeef` 这类哈希片段、错误码）
    /// 不可能是密钥，抹掉它们只会把正常错误文本变成噪声。
    #[test]
    fn redacts_long_hex_run() {
        let out = redact_error("auth failed for digest 0123456789abcdef0123456789abcdef");
        assert_eq!(out, format!("auth failed for digest {REDACTED}"));
    }

    /// 反向要求：不含密钥的普通错误文本必须原样可见。
    ///
    /// 把这一条和 `redacts_*` 放在一起看才有意义：只证明「会脱敏」的测试，
    /// 「把全部文本都替换掉」也能通过。
    #[test]
    fn keeps_plain_error_readable() {
        for plain in [
            "request timed out after 30s",
            "供应商 anthropic 不存在",
            "HTTP 502 Bad Gateway from https://api.example.com/v1/messages",
            "connection refused: deadbeef cafe",
            "",
        ] {
            assert_eq!(
                redact_error(plain),
                plain,
                "不含密钥的错误文本被改写了：{plain}"
            );
        }
    }

    /// 超长错误必须被截断到长度上限内，不原样下发。
    ///
    /// 上游响应体没有上限，逐字透传等于给远程调用方开了一条数据通道。
    #[test]
    fn truncates_long_error() {
        let long = "x".repeat(MAX_ERROR_LEN * 3);
        let out = redact_error(&long);
        assert!(
            out.len() <= MAX_ERROR_LEN,
            "错误文本未截断，长度 {} > {MAX_ERROR_LEN}",
            out.len()
        );
        assert!(!out.is_empty(), "截断不应把错误抹成空串");
    }

    /// 截断必须切在字符边界上：中文错误文本按字节硬切会 panic 或产生乱码。
    #[test]
    fn truncates_on_char_boundary() {
        let long = "上游返回错误，".repeat(MAX_ERROR_LEN);
        let out = redact_error(&long);
        assert!(out.len() <= MAX_ERROR_LEN, "长度 {} 超过上限", out.len());
        assert!(out.chars().any(|c| c == '上'), "截断后中文内容不应消失");
    }

    /// 脱敏在截断之前：key 落在 512 字节之后时也不能靠「反正会被截掉」蒙混过关。
    #[test]
    fn redacts_before_truncating() {
        let filler = "a".repeat(MAX_ERROR_LEN);
        let raw = format!("{filler} sk-deadbeef00112233445566778899aabbccddeeff");
        let out = redact_error(&raw);
        assert!(!out.contains("deadbeef0011"), "截断前未脱敏：{out}");
    }

    /// 错误体构造器必须**自己**完成脱敏，而不是把文本原样塞进 JSON。
    ///
    /// 这一条是结构性收口的全部意义：调用点传进来的 `Display` 一旦成型就已经
    /// 过闸门，「忘了脱敏」不再是一个能写出来的代码形态。
    #[test]
    fn error_json_body_redacts_the_detail_it_is_given() {
        let body = error_json_body("upstream rejected sk-ant-api03-00112233445566778899aabbcc");

        assert_eq!(body["success"], serde_json::Value::Bool(false));
        let message = body["error"].as_str().expect("错误体必须有 error 字段");
        assert_eq!(message, format!("upstream rejected {REDACTED}"));
        assert!(
            !message.contains("0011223344556677"),
            "密钥字面量仍然出现在错误体里：{message}"
        );
    }

    /// 任何实现了 `Display` 的错误类型都要能用 —— 这是出口存在的理由：
    /// handler 里的 `e` 是 `anyhow::Error` / `rusqlite::Error` / `String`，
    /// 出口不接受它们就等于不存在。
    #[test]
    fn error_json_body_accepts_any_display_error() {
        let err = std::io::Error::other("boom sk-deadbeef00112233445566778899aabbccddeeff");

        let from_err = error_json_body(&err);
        let from_str = error_json_body(err.to_string());
        let from_format = error_json_body(format!("wrapped: {err}"));

        for (label, body) in [
            ("&io::Error", from_err),
            ("String", from_str),
            ("format!", from_format),
        ] {
            let message = body["error"]
                .as_str()
                .unwrap_or_else(|| panic!("{label} 没产出 error 字段"));
            assert!(
                !message.contains("deadbeef00112233"),
                "{label} 走出口后仍带密钥字面量：{message}"
            );
        }
    }

    /// 不含密钥的普通错误文本在出口上仍然原样可读 —— 出口不是「一律抹掉」。
    #[test]
    fn error_json_body_keeps_plain_detail_readable() {
        let body = error_json_body("request timed out after 30s");
        assert_eq!(body["error"], "request timed out after 30s");
    }

    /// `error_json` 带上状态码，且响应体是同一个脱敏后的错误体。
    ///
    /// 状态码要跟着走：远程调用方判「是不是错误」的第一依据是 HTTP 状态，
    /// 只把文案脱敏而把状态弄丢等于换了种方式静默失败。
    #[test]
    fn error_json_keeps_status_and_redacts_body() {
        let response = error_json(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "db down: sk-deadbeef00112233445566778899aabbccddeeff",
        );

        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );

        let body = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("构造单线程 runtime 以读取响应体")
            .block_on(async {
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("错误响应体可读");
                String::from_utf8_lossy(&bytes).to_string()
            });

        assert!(
            !body.contains("deadbeef0011"),
            "响应体里仍带密钥字面量：{body}"
        );
        assert!(body.contains(REDACTED), "响应体里没有脱敏占位符：{body}");
    }
}

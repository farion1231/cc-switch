//! 测试移植自上游 filter_test.go / cases_test.go，另补请求体改写的用例

use super::*;
use serde_json::json;

const ZH: &Placeholders = &PLACEHOLDERS_ZH;

fn redact(text: &str) -> String {
    global().redact(text, ZH).redacted
}

fn assert_hit(text: &str, label: &str) {
    let got = redact(text);
    assert!(
        got.contains(label),
        "expected {label} in {got:?} (input {text:?})"
    );
}

fn assert_no_hit(text: &str, label: &str) {
    let got = redact(text);
    assert!(
        !got.contains(label),
        "unexpected {label} in {got:?} (input {text:?})"
    );
}

// --- 结构化 PII ---

#[test]
fn pii_basic() {
    assert_hit("有事联系 test.user@example.com 谢谢", "[邮箱]");
    assert_hit("我的手机是 13812345678 随时打", "[电话]");
    assert_hit("身份证号 11010519900307743X 务必保密", "[身份证]");
    assert_hit("付款卡号 4111111111111111", "[银行卡]");
    assert_no_hit("订单编号 1234567890123456", "[银行卡]");
}

#[test]
fn ssh_targets_are_not_emails() {
    for input in [
        "ssh user@host.example.com",
        "ssh -i ~/.ssh/id_rsa user@host.example.com",
        "打开 ssh user@host.example.com",
        "scp file.txt user@host.example.com:/data/",
        "rsync -av /src/ user@host.example.com:/dst/",
    ] {
        assert_no_hit(input, "[邮箱]");
    }
    assert_hit("我的邮箱是 alice@example.com 请保密", "[邮箱]");
}

// --- 密钥层 ---

#[test]
fn gitleaks_rules_loaded() {
    let (rules, _) = global().secrets.stats();
    assert!(rules > 100, "only {rules} gitleaks rules loaded");
}

#[test]
fn builtin_fallback_when_rules_missing() {
    let f = Filter::from_gitleaks_toml("");
    assert!(f.secrets.stats().0 > 0);
    assert!(f
        .redact("key sk-proj-aB3xK9pLmN2qR7sT5vW1zYQwErTy", ZH)
        .redacted
        .contains("[密钥]"));
}

#[test]
fn secrets_basic() {
    assert_hit("我的密码是 Hunter2xyz", "[密钥]");
    assert_hit("配置里 api_key = aB3xK9pLmN2qR7sT", "[密钥]");
    assert_hit("临时凭证 aB3xK9pLmN2qR7sT5vW1zY 已生成", "[密钥]");
    assert_hit("请使用 token aB3xK9pLmN2qR7sT5vW1zY 调试", "[密钥]");
    assert_hit(
        "Authorization: Bearer abcDEF1234567890/xyzABC4567890==",
        "[密钥]",
    );
}

#[test]
fn entropy_fallback_false_positives() {
    for input in [
        "ls /home/user/AbCdEfGh1234567890XyZ",
        "curl https://api.example.com/v1/users/AbCdEfGh1234567890XyZ",
        "aws s3 cp s3://my-bucket/dir/AbCdEfGh1234567890XyZ .",
        "docker pull registry.io/img@sha256:9f86d081884c7d659a2feaa0c55ad015b1b8a3e6b1d2c4a5e9f8b7d6c5a4b3210",
        "host=long-subdomain-with-many-chars.example.com",
        "订单编号 aB3xK9pLmN2qR7sT5vW1zY 已记账",
        "api_key.example.com/AbCdEfGh1234567890XyZ",
    ] {
        assert_no_hit(input, "[密钥]");
    }
}

#[test]
fn post_validators() {
    for input in [
        "secret={{ API_KEY }} 或 token=${TOKEN}",
        "config: token=%{TOKEN}",
        "auth=<API_KEY>",
        "order_id=aB3xK9pLmN2qR7sT5vW1zY",
        "user_id=AbCdEfGh1234567890XyZQwErTyUiOp",
        "session_no=aB3xK9pLmN2qR7sT5vW1zY1234",
        "trace_id=550e8400-e29b-41d4-a716-446655440000",
        "md5: 9f86d081884c7d659a2feaa0c55ad015",
        "sha1: da39a3ee5e6b4b0d3255bfef95601890afd80709",
        "commit 9f86d081884c7d659a2feaa0c55ad015b1b8a3e6b1d2c4a5e9f8b7d6c5a4b3210",
    ] {
        assert_no_hit(input, "[密钥]");
    }
}

// --- 整体行为 ---

#[test]
fn clean_text_untouched() {
    let text = "今天天气不错，我们一起去公园散步吧。";
    let res = global().redact(text, ZH);
    assert_eq!(res.redacted, text);
    assert_eq!(res.count, 0);
}

#[test]
fn multiple_entities() {
    let res = global().redact("邮箱 a@b.com 手机 13900001111 密码是 Qwer1234", ZH);
    for label in ["[邮箱]", "[电话]", "[密钥]"] {
        assert!(res.redacted.contains(label), "{}", res.redacted);
    }
    assert!(res.count >= 3);
}

#[test]
fn placeholders_follow_language() {
    let text = "mail a@b.com phone 13900001111 password: Qwer1234";
    let cases = [
        ("zh", "mail [邮箱] phone [电话] password: [密钥]"),
        ("zh-TW", "mail [電子郵件] phone [電話] password: [金鑰]"),
        ("en", "mail [EMAIL] phone [PHONE] password: [SECRET]"),
        (
            "ja",
            "mail [メール] phone [電話番号] password: [シークレット]",
        ),
        (
            "tr",
            "mail [E-POSTA] phone [TELEFON] password: [GİZLİ_ANAHTAR]",
        ),
        // 未知语言回退英文
        ("fr", "mail [EMAIL] phone [PHONE] password: [SECRET]"),
    ];
    for (language, expected) in cases {
        let got = global()
            .redact(text, Placeholders::for_language(language))
            .redacted;
        assert_eq!(got, expected, "language {language}");
    }
}

#[test]
fn multibyte_lookback_does_not_panic() {
    // 回溯窗口落在多字节字符中间时不能 panic
    let text = "中文中文中文中文中文中文 aB3xK9pLmN2qR7sT5vW1zYQwErTyUiOp 密钥令牌凭证鉴权";
    let _ = global().redact(text, ZH);
}

/// 上游 testdata/cases.json 回归样本
#[test]
fn upstream_regression_cases() {
    #[derive(serde::Deserialize)]
    struct Case {
        id: u32,
        desc: String,
        input: String,
        expect_hit: bool,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("testdata/cases.json")).unwrap();
    assert!(!cases.is_empty());
    for c in cases {
        let res = global().redact(&c.input, ZH);
        assert_eq!(
            res.count > 0,
            c.expect_hit,
            "#{} {}: input={:?} out={:?}",
            c.id,
            c.desc,
            c.input,
            res.redacted
        );
    }
}

// --- 请求体改写 ---

const SECRET: &str = "我的密码是 Hunter2xyz";

#[test]
fn anthropic_body() {
    let mut body = json!({
        "model": "claude-sonnet-4-5",
        "system": [{ "type": "text", "text": SECRET }],
        "messages": [
            { "role": "user", "content": SECRET },
            { "role": "assistant", "content": [
                { "type": "thinking", "thinking": SECRET, "signature": "sig" },
                { "type": "text", "text": "邮箱 a@b.com" },
                { "type": "tool_use", "id": "t1", "name": "bash", "input": { "cmd": SECRET } }
            ]},
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "t1", "content": SECRET },
                { "type": "tool_result", "tool_use_id": "t2", "content": [{ "type": "text", "text": SECRET }] }
            ]}
        ]
    });
    assert_eq!(redact_request_body(&mut body, ZH), 5);
    assert_eq!(body["model"], "claude-sonnet-4-5");
    assert_eq!(body["system"][0]["text"], "我的密码是 [密钥]");
    assert_eq!(body["messages"][0]["content"], "我的密码是 [密钥]");
    // thinking 带签名，tool_use 是结构化参数：都不碰
    assert_eq!(body["messages"][1]["content"][0]["thinking"], SECRET);
    assert_eq!(body["messages"][1]["content"][1]["text"], "邮箱 [邮箱]");
    assert_eq!(body["messages"][1]["content"][2]["input"]["cmd"], SECRET);
    assert_eq!(
        body["messages"][2]["content"][0]["content"],
        "我的密码是 [密钥]"
    );
    assert_eq!(
        body["messages"][2]["content"][1]["content"][0]["text"],
        "我的密码是 [密钥]"
    );
}

#[test]
fn openai_chat_body() {
    let mut body = json!({
        "messages": [
            { "role": "system", "content": SECRET },
            { "role": "user", "content": [
                { "type": "text", "text": SECRET },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
            ]},
            { "role": "tool", "tool_call_id": "c1", "content": SECRET }
        ]
    });
    assert_eq!(redact_request_body(&mut body, ZH), 3);
    assert_eq!(
        body["messages"][1]["content"][0]["text"],
        "我的密码是 [密钥]"
    );
    assert_eq!(
        body["messages"][1]["content"][1]["image_url"]["url"],
        "data:image/png;base64,AAAA"
    );
}

#[test]
fn responses_body() {
    let mut body = json!({
        "instructions": SECRET,
        "input": [
            { "role": "user", "content": SECRET },
            { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": SECRET }] },
            { "type": "reasoning", "encrypted_content": SECRET },
            { "type": "function_call", "call_id": "c1", "arguments": SECRET },
            { "type": "function_call_output", "call_id": "c1", "output": SECRET }
        ]
    });
    assert_eq!(redact_request_body(&mut body, ZH), 4);
    assert_eq!(body["input"][2]["encrypted_content"], SECRET);
    assert_eq!(body["input"][3]["arguments"], SECRET);
    assert_eq!(body["input"][4]["output"], "我的密码是 [密钥]");

    let mut body = json!({ "input": SECRET });
    assert_eq!(redact_request_body(&mut body, ZH), 1);
    assert_eq!(body["input"], "我的密码是 [密钥]");
}

#[test]
fn gemini_body() {
    let mut body = json!({
        "systemInstruction": { "parts": [{ "text": SECRET }] },
        "contents": [
            { "role": "user", "parts": [{ "text": SECRET }] },
            { "role": "model", "parts": [
                { "text": SECRET, "thought": true },
                { "text": SECRET, "thoughtSignature": "sig" }
            ]}
        ]
    });
    assert_eq!(redact_request_body(&mut body, ZH), 2);
    assert_eq!(body["contents"][1]["parts"][0]["text"], SECRET);
    assert_eq!(body["contents"][1]["parts"][1]["text"], SECRET);
}

#[test]
fn non_object_body_is_noop() {
    let mut body = Value::Null;
    assert_eq!(redact_request_body(&mut body, ZH), 0);
}

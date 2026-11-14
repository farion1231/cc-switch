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

#[test]
fn local_addresses_are_not_ips() {
    for input in [
        "http://127.0.0.1:3000",
        "curl http://127.0.0.1:8080/health",
        "nameserver 127.0.0.53",
        "server listening on 0.0.0.0:8080",
        "allow from 0.0.0.0/0",
        "broadcast 255.255.255.255",
    ] {
        assert_no_hit(input, "[IP]");
    }
    assert_hit("服务器在 8.8.8.8 上", "[IP]");
    assert_hit("内网机器 192.168.1.10", "[IP]");
    assert_hit("http://10.0.0.5:3000", "[IP]");
}

#[test]
fn bank_card_needs_bin_or_context() {
    // 卡组织测试卡号，无上下文也要命中
    for card in [
        "4242424242424242", // Visa
        "5555555555554444", // Mastercard
        "2223003122003222", // Mastercard 2 系列
        "378282246310005",  // American Express
        "3566002020360505", // JCB
        "6011111111111117", // Discover
        "6200000000000005", // 银联
    ] {
        assert_hit(&format!("number {card}"), "[银行卡]");
    }
    // 过了 Luhn 但不在任何卡组织范围内：只有卡号上下文才算
    let card = "1234567812345670";
    assert_no_hit(&format!("订单 {card}"), "[银行卡]");
    assert_no_hit(&format!("card.createdAt: {card}"), "[银行卡]");
    for prefix in [
        "卡号 ",
        "信用卡：",
        "card number: ",
        "cardNo=",
        "bank card ",
    ] {
        assert_hit(&format!("{prefix}{card}"), "[银行卡]");
    }
}

#[test]
fn timestamps_are_not_bank_cards() {
    // 毫秒 / 微秒 / 纳秒时间戳，步长取质数以覆盖各种校验位
    for i in 0..1000u64 {
        let millis = 1_700_000_000_000 + i * 7_919;
        for ts in [millis, millis * 1_000, millis * 1_000_000] {
            assert_no_hit(&format!("createdAt: {ts}"), "[银行卡]");
        }
    }
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
fn entropy_fallback_preserves_assignment_names() {
    let value = "V6nQ2rZa8kTm3wBx9cLp5dFj7sHy4uGe";
    for prefix in ["SESSION_COOKIE=", "GITHUB_PAT=", "x=", "MY_VAR: "] {
        assert_eq!(
            redact(&format!("{prefix}{value}")),
            format!("{prefix}[密钥]")
        );
    }
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
        "https://example.com/?sig=V6nQ2rZa8kTm3wBx9cLp5dFj7sHy4uGe",
        "/var/data/x=V6nQ2rZa8kTm3wBx9cLp5dFj7sHy4uGe",
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

#[test]
fn keyword_crossing_candidate_start_does_not_panic() {
    // 关键词 `api key` 跨过了候选串 `key=...` 的起点
    assert_eq!(redact("api key=AbCdEfGh1234567890XyZ"), "api key=[密钥]");
    assert_eq!(
        redact("access key=AbCdEfGh1234567890XyZ"),
        "access key=[密钥]"
    );
    assert_hit("my api-key=AbCdEfGh1234567890XyZ", "[密钥]");
    assert_hit("中文 api key=AbCdEfGh1234567890XyZ", "[密钥]");
    // 关键词紧贴候选串、整个落在候选串里，仍按强上下文处理
    assert_hit("密钥AbCdEfGh1234567890/XyZ", "[密钥]");
    assert_hit("x token=AbCdEfGh1234567890XyZ", "[密钥]");
}

#[test]
fn short_context_secrets_are_redacted() {
    for (input, expected) in [
        ("我的密码是 hunter2", "我的密码是 [密钥]"),
        ("密码：abc123", "密码：[密钥]"),
        ("password: admin", "password: [密钥]"),
        ("pwd=qwer", "pwd=[密钥]"),
        ("token = 123456", "token = [密钥]"),
    ] {
        assert_eq!(redact(input), expected);
    }
}

#[test]
fn context_placeholders_are_kept() {
    for input in [
        // 占位值
        "password: null",
        "token = None",
        "api_key: xxxx",
        "password=xxx,",
        "secret: REPLACE_ME",
        "password=changeme",
        "password: ****",
        "token=...",
        // 类型标注
        "password: string",
        "password: String,",
        "fn login(password: &str) -> bool",
        "token: string[]",
        "secret: boolean",
        "api_key: SecretStr",
        // 值就是字段名或环境变量
        "token: token,",
        "password = password",
        "token=$TOKEN",
        "password=$DB_PASS",
        "set password=%DB_PASS%",
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
const REDACTED: &str = "我的密码是 [密钥]";

fn anthropic_body() -> Value {
    json!({
        "model": "claude-sonnet-4-5",
        "system": [{ "type": "text", "text": SECRET }],
        "messages": [
            { "role": "user", "content": SECRET },
            { "role": "assistant", "content": [
                { "type": "thinking", "thinking": SECRET, "signature": "sig" },
                { "type": "text", "text": SECRET },
                { "type": "tool_use", "id": "t1", "name": "bash", "input": { "cmd": SECRET } }
            ]},
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "t1", "content": SECRET },
                { "type": "tool_result", "tool_use_id": "t2", "content": [{ "type": "text", "text": SECRET }] },
                { "type": "text", "text": SECRET }
            ]}
        ]
    })
}

#[test]
fn anthropic_body_defaults_to_user_text_and_system() {
    let mut body = anthropic_body();
    assert_eq!(redact_request_body(&mut body, ZH, false), 3);
    assert_eq!(body["model"], "claude-sonnet-4-5");
    assert_eq!(body["system"][0]["text"], REDACTED);
    assert_eq!(body["messages"][0]["content"], REDACTED);
    assert_eq!(body["messages"][2]["content"][2]["text"], REDACTED);
    // 模型回复（thinking 带签名）和结构化的工具参数都不碰
    assert_eq!(body["messages"][1], anthropic_body()["messages"][1]);
    // 工具结果默认不碰
    assert_eq!(body["messages"][2]["content"][0]["content"], SECRET);
    assert_eq!(
        body["messages"][2]["content"][1]["content"][0]["text"],
        SECRET
    );
}

#[test]
fn anthropic_body_with_tool_results() {
    let mut body = anthropic_body();
    assert_eq!(redact_request_body(&mut body, ZH, true), 5);
    assert_eq!(body["messages"][1], anthropic_body()["messages"][1]);
    assert_eq!(body["messages"][2]["content"][0]["content"], REDACTED);
    assert_eq!(
        body["messages"][2]["content"][1]["content"][0]["text"],
        REDACTED
    );
}

fn openai_chat_body() -> Value {
    json!({
        "messages": [
            { "role": "system", "content": SECRET },
            { "role": "developer", "content": SECRET },
            { "role": "user", "content": [
                { "type": "text", "text": SECRET },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
            ]},
            { "role": "assistant", "content": SECRET },
            { "role": "tool", "tool_call_id": "c1", "content": SECRET },
            { "role": "function", "name": "f", "content": SECRET }
        ]
    })
}

#[test]
fn openai_chat_body_defaults_to_user_text_and_system() {
    let mut body = openai_chat_body();
    assert_eq!(redact_request_body(&mut body, ZH, false), 3);
    assert_eq!(body["messages"][0]["content"], REDACTED);
    assert_eq!(body["messages"][1]["content"], REDACTED);
    assert_eq!(body["messages"][2]["content"][0]["text"], REDACTED);
    assert_eq!(
        body["messages"][2]["content"][1]["image_url"]["url"],
        "data:image/png;base64,AAAA"
    );
    assert_eq!(body["messages"][3]["content"], SECRET);
    assert_eq!(body["messages"][4]["content"], SECRET);
    assert_eq!(body["messages"][5]["content"], SECRET);
}

#[test]
fn openai_chat_body_with_tool_results() {
    let mut body = openai_chat_body();
    assert_eq!(redact_request_body(&mut body, ZH, true), 5);
    assert_eq!(body["messages"][3]["content"], SECRET);
    assert_eq!(body["messages"][4]["content"], REDACTED);
    assert_eq!(body["messages"][5]["content"], REDACTED);
}

fn responses_body() -> Value {
    json!({
        "instructions": SECRET,
        "input": [
            { "role": "user", "content": SECRET },
            { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": SECRET }] },
            { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": SECRET }] },
            { "role": "assistant", "content": SECRET },
            { "type": "reasoning", "encrypted_content": SECRET },
            { "type": "function_call", "call_id": "c1", "arguments": SECRET },
            { "type": "function_call_output", "call_id": "c1", "output": SECRET },
            { "type": "custom_tool_call_output", "call_id": "c2", "output": [{ "type": "input_text", "text": SECRET }] }
        ]
    })
}

#[test]
fn responses_body_defaults_to_user_text_and_system() {
    let mut body = responses_body();
    assert_eq!(redact_request_body(&mut body, ZH, false), 3);
    assert_eq!(body["instructions"], REDACTED);
    assert_eq!(body["input"][0]["content"], REDACTED);
    assert_eq!(body["input"][1]["content"][0]["text"], REDACTED);
    assert_eq!(body["input"][2], responses_body()["input"][2]);
    assert_eq!(body["input"][3], responses_body()["input"][3]);
    assert_eq!(body["input"][4]["encrypted_content"], SECRET);
    assert_eq!(body["input"][5]["arguments"], SECRET);
    assert_eq!(body["input"][6]["output"], SECRET);
    assert_eq!(body["input"][7]["output"][0]["text"], SECRET);

    let mut body = json!({ "input": SECRET });
    assert_eq!(redact_request_body(&mut body, ZH, false), 1);
    assert_eq!(body["input"], REDACTED);
}

#[test]
fn responses_body_with_tool_results() {
    let mut body = responses_body();
    assert_eq!(redact_request_body(&mut body, ZH, true), 5);
    assert_eq!(body["input"][2], responses_body()["input"][2]);
    assert_eq!(body["input"][6]["output"], REDACTED);
    assert_eq!(body["input"][7]["output"][0]["text"], REDACTED);
}

fn gemini_body() -> Value {
    json!({
        "systemInstruction": { "parts": [{ "text": SECRET }] },
        "contents": [
            { "role": "user", "parts": [{ "text": SECRET }] },
            { "role": "model", "parts": [
                { "text": SECRET },
                { "functionCall": { "name": "read", "args": { "path": SECRET } } }
            ]},
            { "parts": [
                { "text": SECRET, "thought": true },
                { "text": SECRET, "thoughtSignature": "sig" }
            ]},
            { "role": "user", "parts": [
                { "functionResponse": {
                    "name": "read",
                    "response": {
                        "output": SECRET,
                        "nested": { "lines": [SECRET, 1, null] }
                    },
                    "parts": [{ "inlineData": { "mimeType": "image/png", "data": SECRET } }]
                }},
                { "function_response": { "name": "read", "response": { "output": SECRET } } }
            ]}
        ]
    })
}

#[test]
fn gemini_body_defaults_to_user_text_and_system() {
    let mut body = gemini_body();
    assert_eq!(redact_request_body(&mut body, ZH, false), 2);
    assert_eq!(body["systemInstruction"]["parts"][0]["text"], REDACTED);
    assert_eq!(body["contents"][0]["parts"][0]["text"], REDACTED);
    assert_eq!(body["contents"][1], gemini_body()["contents"][1]);
    assert_eq!(body["contents"][2], gemini_body()["contents"][2]);
    assert_eq!(body["contents"][3], gemini_body()["contents"][3]);
}

#[test]
fn gemini_body_with_tool_results() {
    let mut body = gemini_body();
    assert_eq!(redact_request_body(&mut body, ZH, true), 5);
    assert_eq!(body["contents"][1], gemini_body()["contents"][1]);
    assert_eq!(body["contents"][2], gemini_body()["contents"][2]);

    let response = &body["contents"][3]["parts"][0]["functionResponse"];
    assert_eq!(response["response"]["output"], REDACTED);
    assert_eq!(response["response"]["nested"]["lines"][0], REDACTED);
    assert_eq!(response["response"]["nested"]["lines"][1], 1);
    assert_eq!(response["name"], "read");
    // 媒体不是工具结果文本
    assert_eq!(response["parts"][0]["inlineData"]["data"], SECRET);
    assert_eq!(
        body["contents"][3]["parts"][1]["function_response"]["response"]["output"],
        REDACTED
    );
}

#[test]
fn non_object_body_is_noop() {
    let mut body = Value::Null;
    assert_eq!(redact_request_body(&mut body, ZH, true), 0);
}

#[test]
fn stored_config_without_sub_switch_keeps_tool_results_off() {
    let config: crate::proxy::types::PrivacyFilterConfig =
        serde_json::from_str(r#"{"enabled":true}"#).unwrap();
    assert!(config.enabled);
    assert!(!config.filter_tool_results);
}

// --- P1：关键词预筛与旧实现等价 ---

/// 对拍语料：现有用例 + 每个关键词的大小写 / Unicode 变体 + 非 ASCII 上下文
fn gate_corpus() -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Case {
        input: String,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("testdata/cases.json")).unwrap();
    let mut corpus: Vec<String> = cases.into_iter().map(|c| c.input).collect();
    corpus.extend(
        [
            "",
            "plain ascii text without anything",
            "中文 API_KEY=abc",
            "\u{212A}ey: Ab3dE5gH7jK9",
            "\u{212A}EY=x",
            "aP\u{130}_key", // İ 小写后是 i + 组合点
            "\u{130}",
            "ǅ ǈ ǋ ǲ ß ẞ ﬀ ﬁ",
            "\u{3A3}\u{3A3}",
        ]
        .map(String::from),
    );
    for keyword in global().secrets.all_keywords() {
        let upper = keyword.to_uppercase();
        let kelvin: String = keyword
            .chars()
            .map(|c| if c == 'k' { '\u{212A}' } else { c })
            .collect();
        let dotted: String = keyword
            .chars()
            .map(|c| if c == 'i' { '\u{130}' } else { c })
            .collect();
        for variant in [keyword.clone(), upper, kelvin, dotted] {
            corpus.push(format!("x {variant} y"));
            corpus.push(format!("中文{variant}中文"));
            corpus.push(variant);
        }
    }
    corpus
}

#[test]
fn keyword_gate_matches_naive_lowercase_contains() {
    for filter in [global(), &Filter::from_gitleaks_toml("")] {
        for text in gate_corpus() {
            assert_eq!(
                filter.secrets.active_rules(&text),
                filter.secrets.active_rules_naive(&text),
                "gate differs for {text:?}"
            );
        }
    }
}

#[test]
fn kelvin_sign_activates_keyword_rules() {
    // 旧实现先 to_lowercase，开尔文符号 `K`（U+212A）会变成 k，带含 k 关键词的规则
    // （如 snyk-api-token 的 snyk）随之参与匹配；只按 ASCII 忽略大小写的预筛会把它们漏掉
    let count = |text: &str| {
        global()
            .secrets
            .active_rules(text)
            .into_iter()
            .filter(|&on| on)
            .count()
    };
    assert!(count("sny\u{212A} value") > count("snyz value"));
    assert_eq!(count("sny\u{212A} value"), count("snyk value"));
}

// --- P2：新增的请求字段 ---

/// 工具定义整体不碰：说明文字是客户端自带的固定文字，示例地址会被模型照抄
/// （Claude Code 的 Bash 工具说明要求提交信息带 `Co-Authored-By: Claude <noreply@anthropic.com>`），
/// 句点前的 `?token=abc.` 也不能被吞掉。
#[test]
fn tool_definitions_are_left_untouched() {
    let coauthor = "Co-Authored-By: Claude <noreply@anthropic.com>";
    let url = "Fetch https://example.com/api?token=abc. Then continue.";
    let mut body = json!({
        "messages": [{ "role": "user", "content": "hi" }],
        "tools": [
            { "name": "Bash", "description": coauthor, "input_schema": {
                "type": "object",
                "properties": { "url": { "type": "string", "description": url } }
            }},
            { "type": "function", "function": { "name": "b", "description": SECRET } },
            { "functionDeclarations": [{ "name": "c", "description": SECRET }] }
        ]
    });
    let original = body.clone();
    assert_eq!(redact_request_body(&mut body, ZH, true), 0);
    assert_eq!(body, original);
}

#[test]
fn alpha_search_queries_are_redacted() {
    let mut body = json!({
        "id": "search-1",
        "commands": { "search_query": [{ "q": SECRET }, { "q": "rust regex" }] }
    });
    assert_eq!(redact_request_body(&mut body, ZH, false), 1);
    assert_eq!(body["commands"]["search_query"][0]["q"], REDACTED);
    assert_eq!(body["commands"]["search_query"][1]["q"], "rust regex");
}

#[test]
fn own_compaction_summaries_are_redacted_and_rewrapped() {
    use crate::proxy::providers::codex_compaction::{
        decode_compaction_summary, encode_compaction_summary,
    };
    let mut body = json!({
        "input": [
            { "type": "compaction", "encrypted_content": encode_compaction_summary(SECRET) },
            { "type": "compaction", "encrypted_content": "gAAAA-foreign-blob" },
            { "type": "compaction", "encrypted_content": encode_compaction_summary("nothing here") }
        ]
    });
    let clean = body["input"][2].clone();
    assert_eq!(redact_request_body(&mut body, ZH, false), 1);
    let summary =
        decode_compaction_summary(body["input"][0]["encrypted_content"].as_str().unwrap());
    assert_eq!(summary.as_deref(), Some(REDACTED));
    assert_eq!(body["input"][1]["encrypted_content"], "gAAAA-foreign-blob");
    // 没有命中的摘要逐字节不变，不重新包装
    assert_eq!(body["input"][2], clean);
}

// --- Codex 审核（P1/P2）后的回归用例 ---

/// 同类占位符的第二遍过滤不改写、不计数（跨类型的情况见 `second_pass_may_relabel_across_kinds_once`）
#[test]
fn second_pass_over_placeholders_is_a_noop() {
    for language in ["zh", "zh-TW", "en", "ja", "tr"] {
        let placeholders = Placeholders::for_language(language);
        let text = "我的密码是 Hunter2xyz，邮箱 a@b.com，password = Qwer1234";
        let first = global().redact(text, placeholders);
        assert!(first.count >= 3, "{language}: {}", first.redacted);
        let second = global().redact(&first.redacted, placeholders);
        assert_eq!(second.redacted, first.redacted, "{language}");
        assert_eq!(second.count, 0, "{language}: {}", second.redacted);
    }
}

/// Copilot 优化会把 user 消息里的文本并进 tool_result。转发器先在客户端请求体上过滤
/// （子开关关闭时也处理用户文本），再合并，用户文本里的密钥不会躲进工具结果。
#[test]
fn client_pass_runs_before_copilot_merge() {
    use crate::proxy::copilot_optimizer::{merge_tool_results, sanitize_orphan_tool_results};
    let mut body = json!({
        "messages": [
            { "role": "assistant", "content": [
                { "type": "tool_use", "id": "c1", "name": "lookup", "input": {} }
            ]},
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "c1", "content": "ok" },
                { "type": "text", "text": SECRET }
            ]}
        ]
    });
    assert_eq!(redact_request_body(&mut body, ZH, false), 1);
    let merged = merge_tool_results(sanitize_orphan_tool_results(body));
    let sent = merged.to_string();
    assert!(!sent.contains("Hunter2xyz"), "{sent}");
    assert!(sent.contains(REDACTED), "{sent}");
}

/// Copilot 确定性请求 ID 是 SHA-256(session ‖ 最后一条用户文本)。在过滤后的请求体上计算，
/// 只差在被遮盖口令上的两个请求得到同一个 ID，上游没法用它枚举口令。
#[test]
fn deterministic_request_id_does_not_fingerprint_redacted_secrets() {
    use crate::proxy::copilot_optimizer::deterministic_request_id;
    let id = |password: &str| {
        let mut body = json!({
            "messages": [{ "role": "user", "content": format!("password = {password}") }]
        });
        redact_request_body(&mut body, ZH, false);
        deterministic_request_id(&body, "known-session")
    };
    assert_eq!(id("4937"), id("4938"));
}

#[test]
fn gemini_count_tokens_wrapper_is_redacted() {
    for key in ["generateContentRequest", "generate_content_request"] {
        let mut body = json!({
            key: {
                "model": "models/gemini-x",
                "systemInstruction": { "parts": [{ "text": SECRET }] },
                "contents": [{ "role": "user", "parts": [{ "text": SECRET }] }],
                "tools": [{ "functionDeclarations": [{ "name": "f", "description": SECRET }] }]
            }
        });
        assert_eq!(redact_request_body(&mut body, ZH, false), 2, "{key}");
        let inner = &body[key];
        assert_eq!(inner["systemInstruction"]["parts"][0]["text"], REDACTED);
        assert_eq!(inner["contents"][0]["parts"][0]["text"], REDACTED);
        assert_eq!(
            inner["tools"][0]["functionDeclarations"][0]["description"],
            SECRET
        );
        assert_eq!(inner["model"], "models/gemini-x");
    }
}

/// 写在 input 里的工具定义（additional_tools、工具搜索带出来的定义）同样不碰；
/// 工具搜索的结果文本跟随子开关
#[test]
fn input_declared_tools_are_left_untouched() {
    let tools = json!([{ "type": "function", "name": "lookup", "description": SECRET,
        "parameters": { "type": "object", "properties": {} } }]);
    let body = || {
        json!({
            "input": [
                { "type": "additional_tools", "role": "developer", "tools": tools.clone() },
                { "type": "tool_search_output", "call_id": "s1", "tools": tools.clone(),
                  "output": [{ "type": "input_text", "text": SECRET }] }
            ]
        })
    };
    let mut off = body();
    assert_eq!(redact_request_body(&mut off, ZH, false), 0);
    assert_eq!(off, body());

    let mut on = body();
    assert_eq!(redact_request_body(&mut on, ZH, true), 1);
    assert_eq!(on["input"][1]["output"][0]["text"], REDACTED);
    assert_eq!(on["input"][0]["tools"], tools);
    assert_eq!(on["input"][1]["tools"], tools);
}

const LANGUAGES: [&str; 5] = ["zh", "zh-TW", "en", "ja", "tr"];

/// 原文里写着的占位符字样不豁免：`password = [EMAIL]` 仍按密钥替换
#[test]
fn literal_placeholders_in_source_text_are_not_exempt() {
    for language in LANGUAGES {
        let ph = Placeholders::for_language(language);
        for literal in [ph.email, ph.phone, ph.id_card, ph.bank_card, ph.ip] {
            let res = global().redact(&format!("password = {literal}"), ph);
            assert_eq!(
                res.redacted,
                format!("password = {}", ph.secret),
                "{language} {literal}"
            );
            assert_eq!(res.count, 1, "{language} {literal}");
        }
    }
    // 替换结果与原文相同：不改写、不计数
    let res = global().redact("我的密码是 [密钥]", ZH);
    assert_eq!(res.redacted, "我的密码是 [密钥]");
    assert_eq!(res.count, 0);
}

/// P1 验收：与旧预筛三层对拍。① Route 1–3 原始命中 ② 合并区间
/// ③ 各语言最终字节（与 v1 的重建算法比：每个合并区间都换成占位符）
#[test]
fn detection_and_output_match_the_naive_gate() {
    let filter = global();
    let filler = " 普通的对话内容 plain text".repeat(12);
    let mut corpus = gate_corpus();
    for language in LANGUAGES {
        let ph = Placeholders::for_language(language);
        corpus.push(format!("password = {}", ph.email));
        corpus.push(format!("密码是 {}", ph.secret));
    }
    // 一部分加长，命中落在长文本中间
    let long: Vec<String> = corpus
        .iter()
        .step_by(7)
        .map(|text| format!("{text}{filler}"))
        .collect();
    corpus.extend(long);
    corpus.sort();
    corpus.dedup();

    let triples = |spans: &[Span]| -> Vec<(usize, usize, Kind)> {
        spans.iter().map(|s| (s.start, s.end, s.kind)).collect()
    };
    for text in &corpus {
        let naive_raw = filter.secrets.detect_naive(text);
        assert_eq!(
            triples(&filter.secrets.detect(text)[..]),
            triples(&naive_raw[..]),
            "raw spans differ for {text:?}"
        );
        let mut spans = super::pii::detect(text);
        spans.extend(naive_raw);
        let naive_merged = merge_spans(spans);
        assert_eq!(
            triples(&filter.detect(text)[..]),
            triples(&naive_merged[..]),
            "merged spans differ for {text:?}"
        );
        for language in LANGUAGES {
            let ph = Placeholders::for_language(language);
            let mut expected = String::with_capacity(text.len());
            let mut prev = 0;
            for s in &naive_merged {
                expected.push_str(&text[prev..s.start]);
                expected.push_str(ph.label(s.kind));
                prev = s.end;
            }
            expected.push_str(&text[prev..]);
            assert_eq!(
                filter.redact(text, ph).redacted,
                expected,
                "{language} output differs for {text:?}"
            );
        }
    }
}

/// 预筛的边界：重复关键词、互相包含的关键词、空关键词、无关键词、大写关键词，
/// 以及自动机不可用时的回退路径
#[test]
fn keyword_gate_edge_cases_match_naive() {
    use super::secrets::SecretDetector;
    const EDGE_TOML: &str = r#"
[[rules]]
id = "nested-and-duplicate"
regex = '''aaa[0-9]{3}'''
keywords = ["key", "apikey", "key"]

[[rules]]
id = "empty-keyword"
regex = '''bbb[0-9]{3}'''
keywords = [""]

[[rules]]
id = "no-keyword"
regex = '''ccc[0-9]{3}'''

[[rules]]
id = "upper-keyword"
regex = '''ddd[0-9]{3}'''
keywords = ["KEY", "snyk"]
"#;
    let edge = SecretDetector::from_gitleaks_toml(EDGE_TOML);
    assert_eq!(edge.stats(), (4, 0));
    assert_eq!(
        edge.active_rules("nothing here"),
        [false, true, true, false]
    );
    assert_eq!(edge.active_rules("my APIKEY"), [true, true, true, true]);
    assert_eq!(edge.active_rules("sny\u{212A}"), [false, true, true, true]);

    let corpus = [
        "",
        "nothing here",
        "key",
        "my APIKEY",
        "x apikey y",
        "sny\u{212A}",
        "\u{212A}EY",
        "中文key中文",
        "SNYK and KEY",
    ];
    for detector in [
        SecretDetector::from_gitleaks_toml(EDGE_TOML),
        SecretDetector::from_gitleaks_toml(EDGE_TOML).without_automaton(),
    ] {
        for text in corpus {
            assert_eq!(
                detector.active_rules(text),
                detector.active_rules_naive(text),
                "{text:?}"
            );
        }
    }
    let fallback = SecretDetector::from_gitleaks_toml(GITLEAKS_TOML).without_automaton();
    for text in gate_corpus() {
        assert_eq!(
            fallback.active_rules(&text),
            fallback.active_rules_naive(&text),
            "fallback gate differs for {text:?}"
        );
    }
}

/// 第二遍只省掉「同类占位符」的无效重写：第一遍按邮箱替换的值，第二遍被上下文规则按密钥
/// 命中，会改成密钥占位符并计数；再来一遍就不变了。原值不会因此重新出现。
#[test]
fn second_pass_may_relabel_across_kinds_once() {
    let en = Placeholders::for_language("en");
    let first = global().redact("password = alice@example.com", en);
    assert_eq!(first.redacted, "password = [EMAIL]");
    let second = global().redact(&first.redacted, en);
    assert_eq!(second.redacted, "password = [SECRET]");
    assert_eq!(second.count, 1);
    let third = global().redact(&second.redacted, en);
    assert_eq!(third.redacted, second.redacted);
    assert_eq!(third.count, 0);
}

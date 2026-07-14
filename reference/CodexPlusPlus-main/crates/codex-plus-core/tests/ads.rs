use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use codex_plus_core::ads::{
    DEFAULT_AD_LIST_URLS, cache_busted_ad_url, fetch_ad_list_from_urls, normalize_ad_payload,
};
use serde_json::json;

#[test]
fn default_ad_urls_match_legacy_helper_sources() {
    assert_eq!(
        DEFAULT_AD_LIST_URLS,
        [
            "https://raw.githubusercontent.com/BigPizzaV3/Ad-List/main/ads.json",
            "https://cdn.jsdelivr.net/gh/BigPizzaV3/Ad-List@main/ads.json",
        ]
    );
}

#[test]
fn cache_busted_ad_url_appends_version_query_to_plain_url() {
    assert_eq!(
        cache_busted_ad_url("https://example.test/ads.json", 1779035222758),
        "https://example.test/ads.json?v=1779035222758"
    );
}

#[test]
fn cache_busted_ad_url_preserves_existing_query() {
    assert_eq!(
        cache_busted_ad_url("https://example.test/ads.json?source=cdn", 1779035222758),
        "https://example.test/ads.json?source=cdn&v=1779035222758"
    );
}

#[test]
fn normalizes_remote_ads_for_plugin_and_manager_rendering() {
    let payload = normalize_ad_payload(json!({
        "version": 1,
        "ads": [
            {
                "id": "sponsor",
                "type": "sponsor",
                "title": "赞助商",
                "description": "推荐内容",
                "url": "https://example.test",
                "highlights": ["稳定"]
            },
            {
                "id": "normal",
                "type": "normal",
                "title": "普通推荐",
                "description": "推荐内容",
                "url": "https://example.org"
            },
            {
                "id": "broken",
                "type": "normal",
                "title": "",
                "description": "missing title",
                "url": "https://example.invalid"
            }
        ]
    }));

    assert_eq!(payload["version"], json!(1));
    assert_eq!(payload["ads"].as_array().unwrap().len(), 4);
    assert_eq!(payload["ads"][0]["type"], json!("sponsor"));
    assert_eq!(payload["ads"][1]["id"], json!("cubence"));
    assert_eq!(payload["ads"][1]["type"], json!("sponsor"));
    assert_eq!(payload["ads"][2]["id"], json!("ergou-api"));
    assert_eq!(payload["ads"][2]["type"], json!("sponsor"));
    assert_eq!(payload["ads"][3]["type"], json!("normal"));
}

#[test]
fn builtin_sponsors_are_appended_after_remote_sponsors_with_ergou_last() {
    let payload = normalize_ad_payload(json!({
        "version": 1,
        "ads": [
            {
                "id": "remote-sponsor",
                "type": "sponsor",
                "title": "远端赞助商",
                "description": "远端推荐内容",
                "url": "https://example.test"
            },
            {
                "id": "remote-normal",
                "type": "normal",
                "title": "普通推荐",
                "description": "普通推荐内容",
                "url": "https://example.org"
            }
        ]
    }));
    let ads = payload["ads"].as_array().unwrap();

    assert_eq!(ads[0]["id"], json!("remote-sponsor"));
    assert_eq!(ads[1]["id"], json!("cubence"));
    assert_eq!(ads[1]["title"], json!("Cubence"));
    assert_eq!(
        ads[1]["url"],
        json!("https://cubence.com?source=codexplusplus")
    );
    assert_eq!(ads[1]["expires_at"], json!("2026-08-02T23:59:59+08:00"));
    assert!(
        ads[1]["image"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert_eq!(ads[2]["id"], json!("ergou-api"));
    assert_eq!(ads[2]["title"], json!("二狗 API"));
    assert_eq!(
        ads[2]["url"],
        json!("https://ergouapi.com/r/gh-codexplusplus")
    );
    assert_eq!(ads[2]["expires_at"], json!("2026-08-02T23:59:59+08:00"));
    assert!(
        ads[2]["image"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert_eq!(ads[3]["id"], json!("remote-normal"));
}

#[test]
fn normalizes_known_remote_sponsors_with_local_logos() {
    let payload = normalize_ad_payload(json!({
        "version": 1,
        "ads": [
            {
                "id": "volcengine-ark-agent-plan",
                "type": "sponsor",
                "title": "火山方舟",
                "description": "远端推荐内容",
                "url": "https://example.test/volcengine"
            },
            {
                "id": "0029-token-bridge",
                "type": "sponsor",
                "title": "PackyCode",
                "description": "远端推荐内容",
                "url": "https://example.test/0029"
            },
            {
                "id": "0055-token-bridge",
                "type": "sponsor",
                "title": "Token 云桥",
                "description": "远端推荐内容",
                "url": "https://example.test/0055"
            },
            {
                "id": "apikey-fun-ai-relay",
                "type": "sponsor",
                "title": "APIKEY.FUN",
                "description": "远端推荐内容",
                "url": "https://example.test/apikey"
            },
            {
                "id": "rawchat-codex-relay",
                "type": "sponsor",
                "title": "RawChat",
                "description": "远端推荐内容",
                "url": "https://example.test/rawchat"
            },
            {
                "id": "runapi-openrouter-alternative",
                "type": "sponsor",
                "title": "RunAPI",
                "description": "远端推荐内容",
                "url": "https://example.test/runapi"
            },
            {
                "id": "baikewei-ai",
                "type": "sponsor",
                "title": "百可为AI",
                "description": "远端推荐内容",
                "url": "https://example.test/baikewei"
            },
            {
                "id": "jojocode-codex-relay",
                "type": "sponsor",
                "title": "JOJO Code",
                "description": "远端推荐内容",
                "url": "https://example.test/jojocode",
                "image": "https://example.test/logo.png"
            }
        ]
    }));
    let ads = payload["ads"].as_array().unwrap();

    for id in [
        "volcengine-ark-agent-plan",
        "0029-token-bridge",
        "apikey-fun-ai-relay",
        "runapi-openrouter-alternative",
    ] {
        let ad = ads.iter().find(|ad| ad["id"] == json!(id)).unwrap();
        assert!(
            ad["image"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,"),
            "{id}"
        );
    }
    let baikewei = ads
        .iter()
        .find(|ad| ad["id"] == json!("baikewei-ai"))
        .unwrap();
    assert!(
        baikewei["image"]
            .as_str()
            .unwrap()
            .starts_with("data:image/jpeg;base64,")
    );
    for id in ["0055-token-bridge", "rawchat-codex-relay"] {
        let ad = ads.iter().find(|ad| ad["id"] == json!(id)).unwrap();
        assert!(
            ad["image"]
                .as_str()
                .unwrap()
                .starts_with("data:image/svg+xml;base64,"),
            "{id}"
        );
    }
    let jojocode = ads
        .iter()
        .find(|ad| ad["id"] == json!("jojocode-codex-relay"))
        .unwrap();
    assert_eq!(jojocode["image"], json!("https://example.test/logo.png"));
}

#[tokio::test]
async fn fetch_ad_list_tries_backup_url_when_primary_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let thread = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "client closed before sending complete headers");
                request.extend_from_slice(&buffer[..read]);
                assert!(request.len() <= 16 * 1024, "request headers are too large");
            }
            let request = String::from_utf8_lossy(&request);
            if request.starts_with("GET /primary.json?") {
                stream
                    .write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .unwrap();
            } else {
                assert!(request.starts_with("GET /backup.json?"), "{request}");
                let body = json!({
                    "version": 1,
                    "ads": [{
                        "id": "backup-ad",
                        "type": "normal",
                        "title": "Backup",
                        "description": "Loaded from backup",
                        "url": "https://example.test",
                        "highlights": []
                    }]
                })
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            stream.flush().unwrap();
        }
    });

    let payload = fetch_ad_list_from_urls(&[
        format!("http://127.0.0.1:{port}/primary.json"),
        format!("http://127.0.0.1:{port}/backup.json"),
    ])
    .await
    .unwrap();
    thread.join().unwrap();

    let ads = payload["ads"].as_array().unwrap();
    assert!(ads.iter().any(|ad| ad["id"] == json!("ergou-api")));
    assert!(ads.iter().any(|ad| ad["id"] == json!("backup-ad")));
}

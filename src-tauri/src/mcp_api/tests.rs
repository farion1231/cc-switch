use super::*;
use serde_json::json;

fn document() -> String {
    json!({"openapi":"3.0.3","info":{"title":"Weather"},"servers":[{"url":"https://api.example.com/v1"}],
      "components":{"schemas":{"City":{"type":"string","enum":["Paris","Tokyo"]}}},
      "paths":{"/weather/{city}":{"parameters":[{"in":"path","name":"city","required":true,"schema":{"$ref":"#/components/schemas/City"}}],
        "get":{"operationId":"weather","summary":"Read weather","parameters":[{"in":"query","name":"days","schema":{"type":"integer"}}]},
        "post":{"operationId":"updateWeather","requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","properties":{"temp":{"type":"number"}},"required":["temp"]}}}}}},
        "/upload":{"post":{"requestBody":{"content":{"multipart/form-data":{"schema":{"type":"object"}}}}}}}}
    ).to_string()
}

#[test]
fn mcp_api_compiles_paths_refs_and_json_body_and_reports_unsupported_upload() {
    let preview = compile_document(&document(), None, None).unwrap();
    assert_eq!(preview.title, "Weather");
    assert_eq!(preview.tools.len(), 2);
    assert_eq!(
        preview.tools[0].url,
        "https://api.example.com/v1/weather/{city}"
    );
    assert_eq!(
        preview.tools[0].input_schema["properties"]["path"]["properties"]["city"]["enum"],
        json!(["Paris", "Tokyo"])
    );
    assert_eq!(preview.tools[0].input_schema["required"], json!(["path"]));
    assert_eq!(
        preview.tools[1].input_schema["required"],
        json!(["path", "body"])
    );
    assert_eq!(preview.skipped.len(), 1);
}

#[test]
fn mcp_api_supports_swagger_yaml_and_relative_server() {
    let preview = compile_document("swagger: '2.0'\ninfo:\n  title: Legacy\nhost: api.example.com\nbasePath: /v2\nschemes: [https]\npaths:\n  /items:\n    get:\n      operationId: items\n", None, None).unwrap();
    assert_eq!(preview.tools[0].url, "https://api.example.com/v2/items");
    let spec = r#"{"openapi":"3.1.0","servers":[{"url":"../api"}],"paths":{"/items":{"get":{}}}}"#;
    let preview = compile_document(spec, Some("https://example.com/docs/spec.json"), None).unwrap();
    assert_eq!(preview.tools[0].url, "https://example.com/api/items");
}

#[test]
fn mcp_api_review_swagger_uses_document_origin_defaults() {
    let mut spec = json!({"swagger":"2.0","basePath":"/v1","paths":{"/items":{"get":{}}}});
    let source = Some("http://[::1]:8123/docs/swagger.json");
    let preview = compile_document(&spec.to_string(), source, None).unwrap();
    assert!(preview.skipped.is_empty(), "{:?}", preview.skipped);
    assert_eq!(preview.tools[0].url, "http://[::1]:8123/v1/items");
    spec["host"] = json!("api.example.com:8080");
    let preview = compile_document(&spec.to_string(), source, None).unwrap();
    assert_eq!(preview.tools[0].url, "http://api.example.com:8080/v1/items");
    assert!(compile_document(&spec.to_string(), None, None)
        .unwrap()
        .tools
        .is_empty());
    spec["schemes"] = json!(["https"]);
    let preview = compile_document(&spec.to_string(), source, None).unwrap();
    assert_eq!(
        preview.tools[0].url,
        "https://api.example.com:8080/v1/items"
    );
}

#[test]
fn mcp_api_review_swagger_preserves_parameter_constraints() {
    let spec = json!({"swagger":"2.0","host":"api.example.com","schemes":["https"],"paths":{"/items":{"get":{"parameters":[
        {"in":"query","name":"n","type":"integer","minimum":0,"exclusiveMinimum":true},
        {"in":"query","name":"tag","type":"array","items":{"type":"string"},"collectionFormat":"multi","minItems":1,"maxItems":2,"uniqueItems":true}
    ]}}}});
    let preview = compile_document(&spec.to_string(), None, None).unwrap();
    let schema = &preview.tools[0].input_schema;
    assert_eq!(
        schema["properties"]["query"]["properties"]["n"]["exclusiveMinimum"],
        0
    );
    for invalid in [
        json!({"n":0}),
        json!({"tag":[]}),
        json!({"tag":["a","a"]}),
        json!({"tag":["a","b","c"]}),
    ] {
        assert!(super::schema::validate(schema, &json!({"query":invalid})).is_err());
    }
    assert!(super::schema::validate(schema, &json!({"query":{"n":1,"tag":["a","b"]}})).is_ok());
    let mut unsupported = spec;
    unsupported["paths"]["/items"]["get"]["parameters"][0]["multipleOf"] = json!(2);
    let preview = compile_document(&unsupported.to_string(), None, None).unwrap();
    assert!(preview.tools.is_empty());
    assert!(preview.skipped[0].contains("multipleOf"));
}

#[test]
fn mcp_api_review_runtime_does_not_add_absent_header_schema() {
    let mut profile = ApiProfile {
        version: 1,
        tools: compile_document(&document(), None, None).unwrap().tools,
    };
    profile.tools.push(ApiTool {
        name: "optional_headers".into(), description: "Optional headers".into(), method: "GET".into(),
        url: "https://api.example.com/items".into(), read_only: None,
        input_schema: json!({"type":"object","properties":{"headers":{"type":"object","properties":{"X-Optional":{"type":"string"}},"required":[]}}}),
    });
    let original = profile.tools.clone();
    let runtime = ApiRuntime::new(profile, BTreeMap::new()).unwrap();
    for (tool, expected) in runtime.profile.tools.iter().zip(original) {
        assert_eq!(tool.input_schema, expected.input_schema);
        super::schema::check_schema(&tool.input_schema, 0).unwrap();
    }
}

#[test]
fn mcp_api_config_roundtrip_and_target_guards() {
    let profile = ApiProfile {
        version: 1,
        tools: compile_document(&document(), None, None).unwrap().tools,
    };
    let encoded = encode_profile(&profile).unwrap();
    assert_eq!(decode_profile(&encoded).unwrap().tools.len(), 2);
    for url in [
        "file:///etc/passwd",
        "https://example-user@example.com",
        "http://169.254.169.254/latest",
        "http://metadata.google.internal/",
        "http://100.100.100.200/",
    ] {
        assert!(validate_target(url).is_err(), "{url}");
    }
    assert!(validate_target("http://127.0.0.1:8080/v1").is_ok());
    assert!(validate_headers(&BTreeMap::from([("Host".into(), "other.example".into())])).is_err());
    assert!(validate_headers(&BTreeMap::from([(
        "Authorization".into(),
        "Bearer synthetic".into()
    )]))
    .is_ok());
}

#[tokio::test]
async fn mcp_api_maps_real_http_parameters_json_and_static_auth() {
    use axum::{
        extract::{Path, Query},
        http::HeaderMap,
        routing::get,
        Json, Router,
    };
    async fn read(
        Path(city): Path<String>,
        Query(query): Query<BTreeMap<String, String>>,
        headers: HeaderMap,
    ) -> Json<Value> {
        Json(
            json!({"city":city,"days":query.get("days"),"auth":headers.get("authorization").unwrap().to_str().unwrap()}),
        )
    }
    async fn update(Json(body): Json<Value>) -> Json<Value> {
        Json(body)
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/weather/:city", get(read).post(update)),
        )
        .await
        .unwrap()
    });
    let preview =
        compile_document(&document(), None, Some(&format!("http://{address}/v1"))).unwrap();
    let rt = ApiRuntime::new(
        ApiProfile {
            version: 1,
            tools: preview.tools,
        },
        BTreeMap::from([("Authorization".into(), "Bearer synthetic".into())]),
    )
    .unwrap();
    let result = rt
        .call(
            "weather",
            json!({"path":{"city":"Paris"},"query":{"days":3}}),
        )
        .await
        .unwrap();
    assert_eq!(result["isError"], false);
    let response: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(response["status"], 200);
    assert_eq!(
        response["body"],
        json!({"city":"Paris","days":"3","auth":"Bearer synthetic"})
    );
    let result = rt
        .call(
            "updateWeather",
            json!({"path":{"city":"Tokyo"},"body":{"temp":18}}),
        )
        .await
        .unwrap();
    assert_eq!(result["isError"], false);
    assert!(rt
        .call(
            "weather",
            json!({"path":{"city":"Paris"},"query":{"days":"wrong"}})
        )
        .await
        .is_err());
    assert!(rt
        .call("weather", json!({"path":{"city":"../admin"}}))
        .await
        .is_err());
    task.abort();
}

#[test]
fn mcp_api_reports_external_circular_and_unsupported_parameter_schemas() {
    let mut doc: Value = serde_json::from_str(&document()).unwrap();
    doc["components"]["schemas"]["City"] = json!({"$ref":"https://other.example.com/schema.json"});
    let preview = compile_document(&doc.to_string(), None, None).unwrap();
    assert!(preview.tools.is_empty());
    assert!(preview
        .skipped
        .iter()
        .any(|reason| reason.contains("External or circular")));
    doc["components"]["schemas"]["City"] = json!({"$ref":"#/components/schemas/City"});
    assert!(compile_document(&doc.to_string(), None, None)
        .unwrap()
        .tools
        .is_empty());
}

#[tokio::test]
async fn mcp_api_stdio_lifecycle_tools_and_protocol_errors() {
    let rt = std::sync::Arc::new(
        ApiRuntime::new(
            ApiProfile {
                version: 1,
                tools: compile_document(&document(), None, None).unwrap().tools,
            },
            BTreeMap::new(),
        )
        .unwrap(),
    );
    let mut session = stdio::Session::new(rt);
    let result = session
        .handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .await
        .unwrap();
    assert_eq!(result["error"]["code"], -32002);
    let result=session.handle(json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).await.unwrap();
    assert_eq!(result["result"]["protocolVersion"], "2025-11-25");
    assert!(session
        .handle(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await
        .is_none());
    let result = session
        .handle(json!({"jsonrpc":"2.0","id":"tools","method":"tools/list"}))
        .await
        .unwrap();
    assert_eq!(result["result"]["tools"].as_array().unwrap().len(), 2);
    assert_eq!(
        result["result"]["tools"][0]["annotations"]["readOnlyHint"],
        true
    );
    let result = session
        .handle(json!({"jsonrpc":"2.0","id":3,"method":"unknown"}))
        .await
        .unwrap();
    assert_eq!(result["error"]["code"], -32601);
    let result=session.handle(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"weather","arguments":{}}})).await.unwrap();
    assert_eq!(result["result"]["isError"], true);
    assert!(result.get("error").is_none());
    let result=session.handle(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"missing","arguments":{}}})).await.unwrap();
    assert_eq!(result["error"]["code"], -32602);
}

#[tokio::test]
async fn mcp_api_escapes_paths_query_arrays_and_protects_static_headers() {
    use axum::{extract::OriginalUri, http::HeaderMap, routing::get, Json, Router};
    async fn echo(OriginalUri(uri): OriginalUri, headers: HeaderMap) -> Json<Value> {
        Json(json!({"uri":uri.to_string(),"auth":headers["authorization"].to_str().unwrap()}))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/items/:id", get(echo)))
            .await
            .unwrap()
    });
    let doc = json!({"openapi":"3.1.0","servers":[{"url":format!("http://{address}")}],"paths":{"/items/{id}":{"get":{"operationId":"items","parameters":[
        {"in":"path","name":"id","schema":{"type":"string"}},
        {"in":"query","name":"tag","schema":{"type":"array","items":{"type":"string"}}},
        {"in":"query","name":"limit","schema":{"type":"integer","default":5}},
        {"in":"header","name":"authorization","schema":{"type":"string"}}
    ]}}}});
    let rt = ApiRuntime::new(
        ApiProfile {
            version: 1,
            tools: compile_document(&doc.to_string(), None, None)
                .unwrap()
                .tools,
        },
        BTreeMap::from([("Authorization".into(), "Bearer fixed".into())]),
    )
    .unwrap();
    let result=rt.call("items",json!({"path":{"id":"a/b?next=http://evil.test/#"},"query":{"tag":["a&b","c"]},"headers":{"authorization":"Bearer attacker"}})).await.unwrap();
    let body: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        body["body"]["uri"],
        "/items/a%2Fb%3Fnext%3Dhttp%3A%2F%2Fevil.test%2F%23?limit=5&tag=a%26b&tag=c"
    );
    assert_eq!(body["body"]["auth"], "Bearer fixed");
    assert!(rt.call("items", json!({"path":{"id":".."}})).await.is_err());
    task.abort();
}

#[test]
fn mcp_api_rejects_invalid_schema_and_normalizes_openapi_numeric_exclusivity() {
    let doc = json!({"openapi":"3.0.3","servers":[{"url":"https://api.example.com"}],"paths":{"/items":{"post":{"operationId":"create","requestBody":{"content":{"application/json":{"schema":{"type":"number","minimum":1,"exclusiveMinimum":true}}}}}}}});
    let preview = compile_document(&doc.to_string(), None, None).unwrap();
    assert_eq!(preview.tools.len(), 1);
    assert!(schema::validate(&preview.tools[0].input_schema, &json!({"body":1})).is_err());
    let mut invalid = doc;
    invalid["paths"]["/items"]["post"]["requestBody"]["content"]["application/json"]["schema"] =
        json!({"type":"string","minLength":"bad"});
    assert!(compile_document(&invalid.to_string(), None, None)
        .unwrap()
        .tools
        .is_empty());
}

#[test]
fn mcp_api_profile_rejects_wrong_group_shape_and_decompression_bomb() {
    let tool = ApiTool {
        read_only: None,
        name: "broken".into(),
        description: "test".into(),
        method: "GET".into(),
        url: "https://example.com".into(),
        input_schema: json!({"type":"object","properties":{"query":{"type":"string"}}}),
    };
    assert!(encode_profile(&ApiProfile {
        version: 1,
        tools: vec![tool]
    })
    .is_err());
    use base64::Engine;
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&vec![b' '; MAX_CONFIG + 1]).unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap());
    assert!(decode_profile(&encoded).is_err());
}

#[test]
fn mcp_api_unique_array_handles_large_input_and_object_order() {
    let s = json!({"type":"array","uniqueItems":true});
    let values = json!((0..100_000).collect::<Vec<_>>());
    let start = std::time::Instant::now();
    assert!(schema::validate(&s, &values).is_ok());
    eprintln!("100000 unique integers: {:?}", start.elapsed());
    let duplicates: Value = serde_json::from_str(r#"[{"x":1,"y":2},{"y":2,"x":1}]"#).unwrap();
    assert!(schema::validate(&s, &duplicates).is_err());
}

#[tokio::test]
async fn mcp_api_static_auth_satisfies_required_headers_without_exposing_its_value() {
    let doc = json!({"openapi":"3.0.3","servers":[{"url":"http://127.0.0.1:1"}],"paths":{"/items":{"get":{"operationId":"items","parameters":[{"in":"header","name":"X-API-Key","required":true,"schema":{"type":"string"}}]}}}});
    let rt = std::sync::Arc::new(
        ApiRuntime::new(
            ApiProfile {
                version: 1,
                tools: compile_document(&doc.to_string(), None, None)
                    .unwrap()
                    .tools,
            },
            BTreeMap::from([("x-api-key".into(), "synthetic-secret".into())]),
        )
        .unwrap(),
    );
    let result = rt.call("items", json!({})).await;
    assert!(
        result.is_ok(),
        "static auth must satisfy required header without model arguments"
    );
    assert!(!rt.profile.tools[0]
        .input_schema
        .to_string()
        .contains("synthetic-secret"));
    assert_eq!(rt.profile.tools[0].input_schema["required"], json!([]));
}

#[test]
fn mcp_api_request_body_omits_readonly_fields_and_their_required_entries() {
    let doc = json!({"openapi":"3.0.3","servers":[{"url":"https://example.com"}],"paths":{"/items":{"post":{"requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","properties":{"id":{"type":"integer","readOnly":true},"name":{"type":"string"}},"required":["id","name"]}}}}}}}});
    let preview = compile_document(&doc.to_string(), None, None).unwrap();
    let schema = &preview.tools[0].input_schema;
    assert!(schema::validate(schema, &json!({"body":{"name":"synthetic"}})).is_ok());
    assert_eq!(schema["properties"]["body"]["required"], json!(["name"]));
    assert!(schema["properties"]["body"]["properties"]
        .get("id")
        .is_none());
}

#[test]
fn mcp_api_uses_persistent_appimage_path_and_rejects_flatpak() {
    let executable = std::env::temp_dir().join("cc-switch-synthetic");
    let image = std::env::temp_dir().join("CC-Switch.AppImage");
    assert_eq!(
        runtime_executable(executable.clone(), Some(image.clone()), false).unwrap(),
        image
    );
    assert!(runtime_executable(executable, None, true).is_err());
}

#[test]
fn mcp_api_deepseek_example_is_importable_and_constrains_search_arguments() {
    let source = include_str!("../../../docs/config/mcp-api/deepseek-search-fetch.openapi.json");
    let preview = compile_document(source, None, None).unwrap();
    assert_eq!(preview.tools.len(), 2);
    assert!(preview.skipped.is_empty());
    let search = preview
        .tools
        .iter()
        .find(|tool| tool.name == "deepseek_search")
        .unwrap();
    assert_eq!(search.url, "https://api.deepseek.com/v1/tools/search");
    assert_eq!(search.method, "POST");
    assert!(search.is_read_only());
    assert!(schema::validate(
        &search.input_schema,
        &json!({"body":{"query":"Rust MCP","count":5}})
    )
    .is_ok());
    assert!(schema::validate(&search.input_schema, &json!({"body":{"query":"   "}})).is_err());
    assert!(schema::validate(
        &search.input_schema,
        &json!({"body":{"query":"Rust MCP","count":51}})
    )
    .is_err());
}

#[tokio::test]
async fn mcp_api_returns_http_errors_blocks_redirects_and_bounds_responses() {
    use axum::{
        http::{header, StatusCode},
        routing::get,
        Router,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let reached = std::sync::Arc::new(AtomicUsize::new(0));
    let count = reached.clone();
    let router = Router::new()
        .route(
            "/error",
            get(|| async { (StatusCode::TOO_MANY_REQUESTS, "slow down") }),
        )
        .route(
            "/redirect",
            get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/target")], "") }),
        )
        .route(
            "/target",
            get(move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    "target"
                }
            }),
        )
        .route("/large", get(|| async { "x".repeat(MAX_RESPONSE + 1) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let tools = ["error", "redirect", "large"].map(|name| ApiTool {
        read_only: None,
        name: name.into(),
        description: name.into(),
        method: "GET".into(),
        url: format!("http://{address}/{name}"),
        input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
    });
    let rt = ApiRuntime::new(
        ApiProfile {
            version: 1,
            tools: tools.to_vec(),
        },
        BTreeMap::new(),
    )
    .unwrap();
    for name in ["error", "redirect", "large"] {
        assert_eq!(
            rt.call(name, json!({})).await.unwrap()["isError"],
            true,
            "{name}"
        );
    }
    assert_eq!(reached.load(Ordering::SeqCst), 0);
    task.abort();
}

#[test]
#[ignore = "local conversion benchmark"]
fn mcp_api_benchmark_conversion() {
    for count in [1, 25, 100, 1000] {
        let mut paths = serde_json::Map::new();
        for i in 0..count {
            paths.insert(format!("/items{i}"),json!({"get":{"operationId":format!("items{i}"),"parameters":[{"in":"query","name":"q","schema":{"type":"string"}}]}}));
        }
        let text =
            json!({"openapi":"3.1.0","servers":[{"url":"https://api.example.com"}],"paths":paths})
                .to_string();
        let start = std::time::Instant::now();
        for _ in 0..10 {
            assert_eq!(
                compile_document(&text, None, None).unwrap().tools.len(),
                count
            );
        }
        eprintln!(
            "{count} operations, {} document bytes, mean compile {:?}",
            text.len(),
            start.elapsed() / 10
        );
    }
}

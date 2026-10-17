//! Phase 1a — OpenAPI 3.1 spec + swagger-ui（SHOULD R-010）。
//!
//! 手写 spec（零上游类型派生，保持零触碰）。作为 `routes-map` 的活文档校验源。
//! `GET /control/v1/openapi.json` 与 `GET /control/v1/docs` 均**免鉴权**（文档 + UI 本体不含敏感数据；
//! 真正的 API 调用仍经 Bearer 中间件）。

use axum::response::{Html, IntoResponse};
use axum::Json;
use serde_json::{json, Value};

/// app 查询参数（大量 GET 端点共用）。
fn q_app() -> Value {
    json!({
        "name": "app", "in": "query", "required": true, "schema": {"type": "string"},
        "description": "应用类型：claude | claude-desktop | codex"
    })
}

/// 通用 200 JSON 响应占位（不耦合上游类型；活文档后续可精化 schema）。
fn ok_resp(desc: &str) -> Value {
    json!({
        "200": { "description": desc, "content": { "application/json": { "schema": { "type": "object" } } } },
        "401": { "description": "未授权（Bearer token 缺失/错误）" },
        "500": { "description": "服务端错误" }
    })
}

/// JSON body 占位。
fn body_op(desc: &str) -> Value {
    json!({
        "description": desc, "required": true,
        "content": { "application/json": { "schema": { "type": "object" } } }
    })
}

/// 构造 OpenAPI 3.1 文档。
pub fn spec() -> Value {
    let q = q_app();
    let ok = ok_resp("成功");
    let okb = ok_resp("成功（返回 {ok:true}）");

    // (method, path, summary, parameters[Vec], requestBody: Option<Value>, responses: Value)
    #[allow(clippy::type_complexity)]
    let ops: Vec<(&str, &str, &str, Vec<Value>, Option<Value>, Value)> = vec![
        // ---- providers ----
        (
            "get",
            "/control/v1/providers",
            "列出 provider",
            vec![q.clone()],
            None,
            ok.clone(),
        ),
        (
            "post",
            "/control/v1/providers",
            "新增 provider",
            vec![],
            Some(body_op("provider + addToLive + editorSave")),
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/providers/current",
            "当前 provider id",
            vec![q.clone()],
            None,
            ok.clone(),
        ),
        (
            "post",
            "/control/v1/providers/switch",
            "切换 provider",
            vec![],
            Some(body_op("{app, provider_id}")),
            ok.clone(),
        ),
        (
            "patch",
            "/control/v1/providers/sort",
            "更新排序",
            vec![],
            Some(body_op("{app, updates}")),
            okb.clone(),
        ),
        (
            "post",
            "/control/v1/providers/editor-view",
            "编辑器视图",
            vec![],
            Some(body_op("{app, settingsConfig, category, providerId}")),
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/providers/{id}",
            "更新 provider",
            vec![json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}})],
            Some(body_op("{app, provider, editorSave}")),
            okb.clone(),
        ),
        (
            "delete",
            "/control/v1/providers/{id}",
            "删除 provider",
            vec![
                json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}}),
                q.clone(),
            ],
            None,
            okb.clone(),
        ),
        (
            "get",
            "/control/v1/providers/{id}/custom-endpoints",
            "列出自定义端点",
            vec![
                json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}}),
                q.clone(),
            ],
            None,
            ok.clone(),
        ),
        (
            "post",
            "/control/v1/providers/{id}/custom-endpoints",
            "新增自定义端点",
            vec![json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}})],
            Some(body_op("{app, url}")),
            okb.clone(),
        ),
        (
            "delete",
            "/control/v1/providers/{id}/custom-endpoints",
            "移除自定义端点",
            vec![json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}})],
            Some(body_op("{app, url}")),
            okb.clone(),
        ),
        (
            "post",
            "/control/v1/providers/test-endpoints",
            "测试端点延迟",
            vec![],
            Some(body_op("{urls, timeoutSecs}")),
            ok.clone(),
        ),
        // ---- settings (blob) ----
        (
            "get",
            "/control/v1/settings",
            "全量 AppSettings（脱敏）",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/settings",
            "保存全量 AppSettings（合并保护）",
            vec![],
            Some(body_op("AppSettings")),
            okb.clone(),
        ),
        (
            "get",
            "/control/v1/config-dir",
            "应用配置目录路径",
            vec![q.clone()],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/app-config-dir-override",
            "app_config_dir 覆盖路径（无则 null）",
            vec![],
            None,
            ok.clone(),
        ),
        // ---- misc（只读） ----
        (
            "get",
            "/control/v1/installed-skills",
            "已安装 skills 列表",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/migration-result",
            "迁移结果（一次性标记）",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/skills-migration-result",
            "skills 迁移结果（一次性）",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/rest-config",
            "REST 控制面配置（只读）",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/pi-current-state",
            "pi 集成当前状态",
            vec![],
            None,
            ok.clone(),
        ),
        // ---- proxy ----
        (
            "get",
            "/control/v1/proxy/status",
            "代理运行状态",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "post",
            "/control/v1/proxy/restart",
            "重启代理（幂等）",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/proxy/config",
            "应用代理配置（整体 ProxyConfig）",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/proxy/config",
            "更新应用代理配置（整体）",
            vec![],
            Some(body_op("ProxyConfig")),
            okb.clone(),
        ),
        (
            "get",
            "/control/v1/proxy/config/{app}",
            "应用级代理配置（AppProxyConfig，per-app）",
            vec![json!({"name":"app","in":"path","required":true,"schema":{"type":"string"}})],
            None,
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/proxy/config/{app}",
            "更新应用级代理配置（per-app）",
            vec![json!({"name":"app","in":"path","required":true,"schema":{"type":"string"}})],
            Some(body_op("AppProxyConfig")),
            okb.clone(),
        ),
        (
            "get",
            "/control/v1/proxy/takeover",
            "接管总状态",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "post",
            "/control/v1/proxy/takeover/{app}",
            "切换应用接管",
            vec![json!({"name":"app","in":"path","required":true,"schema":{"type":"string"}})],
            Some(body_op("{enabled}")),
            okb.clone(),
        ),
        (
            "get",
            "/control/v1/proxy/health/{app}",
            "provider 健康状态",
            vec![
                json!({"name":"app","in":"path","required":true,"schema":{"type":"string"}}),
                json!({"name":"provider_id","in":"query","required":true,"schema":{"type":"string"}}),
            ],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/proxy/circuit-breaker/config",
            "熔断配置",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/proxy/circuit-breaker/config",
            "更新熔断配置",
            vec![],
            Some(body_op("CircuitBreakerConfig")),
            okb.clone(),
        ),
        (
            "post",
            "/control/v1/proxy/circuit-breaker/reset",
            "重置熔断器",
            vec![],
            Some(body_op("{provider_id, app_type}")),
            okb.clone(),
        ),
        (
            "get",
            "/control/v1/proxy/circuit-breaker/stats",
            "熔断统计（镜像 command stub，恒 null）",
            vec![
                json!({"name":"provider_id","in":"query","required":true,"schema":{"type":"string"}}),
                json!({"name":"app","in":"query","required":true,"schema":{"type":"string"}}),
            ],
            None,
            ok.clone(),
        ),
        // ---- global-proxy ----
        (
            "get",
            "/control/v1/global-proxy/url",
            "全局出站代理 URL",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/global-proxy/url",
            "设置全局出站代理 URL",
            vec![],
            Some(body_op("{url}")),
            okb.clone(),
        ),
        (
            "post",
            "/control/v1/global-proxy/test",
            "测试代理连通性",
            vec![],
            Some(body_op("{url}")),
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/global-proxy/status",
            "上游状态快照",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "post",
            "/control/v1/global-proxy/scan",
            "扫描本机代理",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/global-proxy/config",
            "全局代理配置（GlobalProxyConfig）",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/global-proxy/config",
            "更新全局代理配置",
            vec![],
            Some(body_op("GlobalProxyConfig")),
            okb.clone(),
        ),
        // ---- failover ----
        (
            "get",
            "/control/v1/failover/queue",
            "故障转移队列",
            vec![q.clone()],
            None,
            ok.clone(),
        ),
        (
            "post",
            "/control/v1/failover/queue",
            "入队",
            vec![],
            Some(body_op("{app, provider_id}")),
            okb.clone(),
        ),
        (
            "delete",
            "/control/v1/failover/queue/{id}",
            "出队",
            vec![
                json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}}),
                q.clone(),
            ],
            None,
            okb.clone(),
        ),
        (
            "get",
            "/control/v1/failover/available",
            "可用 provider（故障转移候选）",
            vec![q.clone()],
            None,
            ok.clone(),
        ),
        (
            "get",
            "/control/v1/failover/auto",
            "自动故障转移开关",
            vec![q.clone()],
            None,
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/failover/auto",
            "设置自动故障转移",
            vec![],
            Some(body_op("{app_type, enabled}")),
            okb.clone(),
        ),
        // ---- profile ----
        (
            "get",
            "/control/v1/profiles",
            "profile 列表（ProfilesResponse）",
            vec![],
            None,
            ok.clone(),
        ),
        (
            "post",
            "/control/v1/profiles",
            "创建 profile",
            vec![],
            Some(body_op("{name, scope}")),
            ok.clone(),
        ),
        (
            "put",
            "/control/v1/profiles/{id}",
            "更新 profile",
            vec![json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}})],
            Some(body_op("{name, resnapshot, scope}")),
            ok.clone(),
        ),
        (
            "delete",
            "/control/v1/profiles/{id}",
            "删除 profile",
            vec![json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}})],
            None,
            okb.clone(),
        ),
        (
            "post",
            "/control/v1/profiles/{id}/apply",
            "应用 profile",
            vec![
                json!({"name":"id","in":"path","required":true,"schema":{"type":"string"}}),
                json!({"name":"scope","in":"query","required":true,"schema":{"type":"string"}}),
            ],
            None,
            ok.clone(),
        ),
        (
            "delete",
            "/control/v1/profiles/current/{scope}",
            "清除当前 profile",
            vec![json!({"name":"scope","in":"path","required":true,"schema":{"type":"string"}})],
            None,
            okb.clone(),
        ),
    ];

    let mut paths: serde_json::Map<String, Value> = serde_json::Map::new();
    // 收集手写路由 path+method（用于跳过自动路由重复）
    let hand: std::collections::HashSet<(String, &str)> = ops
        .iter()
        .map(|(m, p, _, _, _, _)| (p.to_string(), *m))
        .collect();
    for (method, path, summary, params, body, responses) in ops {
        let mut op = serde_json::Map::new();
        op.insert("summary".into(), json!(summary));
        op.insert("parameters".into(), json!(params));
        if let Some(b) = body {
            op.insert("requestBody".into(), b);
        }
        op.insert("responses".into(), responses);
        op.insert("tags".into(), json!(tag_for(path)));
        let entry = paths.entry(path.to_string()).or_insert(json!({}));
        entry
            .as_object_mut()
            .unwrap()
            .insert(method.to_string(), json!(op));
    }

    // —— 合并宏自动生成的路由（#[command_api] inventory 收集）——
    // 手写路由已有详细 schema，自动路由仅加 minimal entry；重复 path+method 跳过。
    for meta in crate::rest_registry::route_table() {
        // :param → {param}
        let mut oa_path = meta.path.to_string();
        for p in meta.path_params {
            oa_path = oa_path.replace(&format!(":{p}"), &format!("{{{p}}}"));
        }
        let method_lower = meta.method.to_lowercase();
        if hand.contains(&(oa_path.clone(), method_lower.as_str())) {
            continue; // 手写路由已覆盖
        }
        let params: Vec<Value> = meta.path_params.iter().map(|p| {
            json!({"name": p, "in": "path", "required": true, "schema": {"type": "string"}})
        }).collect();
        let mut op = serde_json::Map::new();
        op.insert("summary".into(), json!(meta.cmd));
        op.insert("tags".into(), json!(["auto-generated"]));
        op.insert("parameters".into(), json!(params));
        if meta.method != "GET" {
            op.insert("requestBody".into(), body_op("JSON body (camelCase)"));
        }
        op.insert("responses".into(), ok_resp("成功"));
        let entry = paths.entry(oa_path).or_insert(json!({}));
        entry
            .as_object_mut()
            .unwrap()
            .insert(method_lower, json!(op));
    }

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "cc-switch REST Control Plane",
            "version": "1.0.0",
            "description": "feature-gated (`rest_api`) axum 控制面。Bearer token 鉴权（除 /control/v1/openapi.json 与 /control/v1/docs）。"
        },
        "servers": [{ "url": "http://127.0.0.1:8787/control/v1" }],
        "components": {
            "securitySchemes": {
                "bearerAuth": { "type": "http", "scheme": "bearer" }
            }
        },
        "security": [{ "bearerAuth": [] }],
        "paths": Value::Object(paths)
    })
}

fn tag_for(path: &str) -> Vec<&'static str> {
    let seg = path
        .trim_start_matches("/control/v1/")
        .split('/')
        .next()
        .unwrap_or("");
    match seg {
        "providers" => vec!["providers"],
        "profiles" => vec!["profiles"],
        "proxy" => vec!["proxy"],
        "global-proxy" => vec!["global-proxy"],
        "failover" => vec!["failover"],
        _ => vec!["other"],
    }
}

pub async fn openapi_json() -> impl IntoResponse {
    Json(spec())
}

pub async fn swagger_ui() -> impl IntoResponse {
    Html(SWAGGER_HTML)
}

/// 最小 swagger-ui 页面（CDN，零本地资源）。
const SWAGGER_HTML: &str = r##"<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8"/>
<title>cc-switch REST Control Plane</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css"/>
</head>
<body>
<div id="swagger-ui"></div>
<script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>
window.onload = () => {
  SwaggerUIBundle({
    url: "/control/v1/openapi.json",
    dom_id: "#swagger-ui",
    deepLinking: true,
    presets: [SwaggerUIBundle.presets.apis],
    layout: "BaseLayout"
  });
};
</script>
</body>
</html>"##;

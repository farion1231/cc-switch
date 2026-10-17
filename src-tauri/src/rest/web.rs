//! Web 控制台静态资源 serve（feature-gated，R-005）。
//!
//! 用 `rust-embed` 在编译期嵌入 `src/web/dist/`（由 `pnpm build:web` 产出）。
//! 构建顺序：先 `pnpm build:web`，再 `cargo build --features rest_api`。
//! dist 为空（仅有 .gitkeep）时仍可编译，`GET /` 返回占位提示。
//!
//! `GET /` 与静态资源**免鉴权**（HTML 本体不含敏感数据；API 调用仍经 Bearer）。

use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../src/web/dist/"]
struct WebAssets;

pub fn routes() -> Router {
    Router::new()
        .route("/", get(index_handler))
        .fallback(asset_handler)
}

async fn index_handler() -> Response {
    serve_named("index.html")
}

/// 按路径查嵌入资源；命中返回带 mime 的文件，未命中回退 index.html（SPA）。
/// dist 为空时回退占位页。
async fn asset_handler(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    if path.is_empty() {
        return serve_named("index.html");
    }
    serve_named(path)
}

fn serve_named(name: &str) -> Response {
    if let Some(file) = WebAssets::get(name) {
        let mime = mime_for(name);
        let mut resp = (StatusCode::OK, file.data.into_owned()).into_response();
        resp.headers_mut()
            .insert(header::CONTENT_TYPE, mime.parse().unwrap());
        return resp;
    }
    // 回退：index.html（SPA）或占位
    if name != "index.html" {
        if let Some(file) = WebAssets::get("index.html") {
            return Html(file.data.into_owned()).into_response();
        }
    }
    Html(PLACEHOLDER).into_response()
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

const PLACEHOLDER: &str = r#"<!DOCTYPE html><html><body style="font-family:system-ui;padding:2em">
<h2>cc-switch Web Console</h2>
<p>web bundle 未构建。请在项目根目录运行：</p>
<pre>pnpm build:web</pre>
<p>然后重新 <code>cargo build --features rest_api</code>。</p>
</body></html>"#;

// (EmbeddedFile 细节由 rust-embed 宏处理)

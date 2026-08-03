use crate::deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, parse_deeplink_url, DeepLinkImportRequest,
};
use crate::store::AppState;
use tauri::State;

const DEFAULT_TICKET_URL: &str = "https://store.tu-zi.com/portal/api/codexImport/exchange";
const TICKET_PATH: &str = "/portal/api/codexImport/exchange";
const TICKET_CLIENT: &str = "cc";

fn ticket_url_allowed(url: &reqwest::Url) -> bool {
    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    if url.scheme() == "https"
        && url.host_str() == Some("store.tu-zi.com")
        && url.port_or_known_default() == Some(443)
        && url.path() == TICKET_PATH
    {
        return true;
    }

    #[cfg(debug_assertions)]
    {
        let host = url.host_str().unwrap_or_default();
        return url.scheme() == "http"
            && matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
            && url.port() == Some(8081)
            && url.path() == TICKET_PATH;
    }

    #[cfg(not(debug_assertions))]
    false
}

async fn exchange_ticket(request: DeepLinkImportRequest) -> Result<DeepLinkImportRequest, String> {
    let Some(ticket) = request
        .usage_user_id
        .as_deref()
        .and_then(|v| v.strip_prefix("codex-ticket:"))
    else {
        return Ok(request);
    };
    let endpoint = request
        .usage_base_url
        .as_deref()
        .and_then(|v| v.strip_prefix("codex-ticket-url:"))
        .unwrap_or(DEFAULT_TICKET_URL);
    let url = reqwest::Url::parse(endpoint).map_err(|_| "配置票据地址无效".to_string())?;
    if !ticket_url_allowed(&url) {
        return Err("配置票据地址不在客户端安全白名单中".to_string());
    }
    let requested_name = request.name.clone();
    let requested_model = request.model.clone();
    let response = reqwest::Client::builder()
        // 票据只能发送到上面校验过的固定主机；禁止 307/308 把 POST
        // 与一次性票据原样转发到重定向目标。
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "配置服务初始化失败".to_string())?
        .post(url)
        .form(&[("ticket", ticket), ("client", TICKET_CLIENT)])
        .send()
        .await
        .map_err(|_| "无法连接配置服务，请检查网络后重试".to_string())?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "配置服务返回格式无效".to_string())?;
    if !status.is_success() || body.get("code").and_then(|v| v.as_i64()) != Some(200) {
        return Err(body
            .get("error")
            .or_else(|| body.get("msg"))
            .and_then(|v| v.as_str())
            .unwrap_or("配置票据兑换失败")
            .to_string());
    }
    let mut resolved: DeepLinkImportRequest =
        serde_json::from_value(body.get("data").cloned().unwrap_or_default())
            .map_err(|_| "配置服务返回内容无效".to_string())?;
    if requested_name
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty())
    {
        resolved.name = requested_name;
    }
    if requested_model
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty())
    {
        resolved.model = requested_model;
    }
    Ok(resolved)
}

#[cfg(test)]
mod ticket_url_tests {
    use super::ticket_url_allowed;

    #[test]
    fn accepts_only_the_exact_production_exchange_endpoint() {
        let valid =
            reqwest::Url::parse("https://store.tu-zi.com/portal/api/codexImport/exchange").unwrap();
        assert!(ticket_url_allowed(&valid));

        for value in [
            "http://store.tu-zi.com/portal/api/codexImport/exchange",
            "https://store.tu-zi.com/portal/api/codexImport/other",
            "https://store.tu-zi.com:444/portal/api/codexImport/exchange",
            "https://store.tu-zi.com/portal/api/codexImport/exchange?redirect=1",
            "https://evil.example/portal/api/codexImport/exchange",
        ] {
            assert!(
                !ticket_url_allowed(&reqwest::Url::parse(value).unwrap()),
                "{value}"
            );
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_build_accepts_loopback_exchange_endpoints() {
        for value in [
            "http://127.0.0.1:8081/portal/api/codexImport/exchange",
            "http://localhost:8081/portal/api/codexImport/exchange",
            "http://[::1]:8081/portal/api/codexImport/exchange",
        ] {
            assert!(
                ticket_url_allowed(&reqwest::Url::parse(value).unwrap()),
                "{value}"
            );
        }

        for value in [
            "http://127.0.0.1:8080/portal/api/codexImport/exchange",
            "http://localhost/portal/api/codexImport/exchange",
            "http://[::1]:3000/portal/api/codexImport/exchange",
        ] {
            assert!(
                !ticket_url_allowed(&reqwest::Url::parse(value).unwrap()),
                "{value}"
            );
        }
    }
}

/// Parse a deep link URL and return the parsed request for frontend confirmation
#[tauri::command]
pub fn parse_deeplink(url: String) -> Result<DeepLinkImportRequest, String> {
    log::info!("Parsing deep link URL: {}", crate::url_for_log(&url));
    parse_deeplink_url(&url).map_err(|e| e.to_string())
}

/// Merge configuration from Base64/URL into a deep link request
/// This is used by the frontend to show the complete configuration in the confirmation dialog
#[tauri::command]
pub fn merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, String> {
    log::info!("Merging config for deep link request: {:?}", request.name);
    crate::deeplink::parse_and_merge_config(&request).map_err(|e| e.to_string())
}

/// Import a provider from a deep link request (legacy, kept for compatibility)
#[tauri::command]
pub fn import_from_deeplink(
    state: State<AppState>,
    request: DeepLinkImportRequest,
) -> Result<String, String> {
    log::info!(
        "Importing provider from deep link: {:?} for app {:?}",
        request.name,
        request.app
    );

    let provider_id = import_provider_from_deeplink(&state, request).map_err(|e| e.to_string())?;

    log::info!("Successfully imported provider with ID: {provider_id}");

    Ok(provider_id)
}

/// Import resource from a deep link request (unified handler)
#[tauri::command]
pub async fn import_from_deeplink_unified(
    state: State<'_, AppState>,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, String> {
    let request = exchange_ticket(request).await?;
    log::info!("Importing {} resource from deep link", request.resource);

    match request.resource.as_str() {
        "provider" => {
            let provider_id =
                import_provider_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "provider",
                "id": provider_id
            }))
        }
        "prompt" => {
            let prompt_id =
                import_prompt_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "prompt",
                "id": prompt_id
            }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            // Add type field to the result
            Ok(serde_json::json!({
                "type": "mcp",
                "importedCount": result.imported_count,
                "importedIds": result.imported_ids,
                "failed": result.failed
            }))
        }
        "skill" => {
            let skill_key =
                import_skill_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "skill",
                "key": skill_key
            }))
        }
        _ => Err(format!("Unsupported resource type: {}", request.resource)),
    }
}

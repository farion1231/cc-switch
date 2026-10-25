use crate::{error::AppError, services::model_fetch::FetchedModel};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use std::collections::BTreeSet;

const CONTROL_API: &str = "https://codeagentcli.rnd.huawei.com/codeAgentPro";
// Match the installed CodeAgent CLI's service protocol version.
const PLUGIN_VERSION: &str = "cli-1.2605.05.01";

async fn json_response(request: RequestBuilder, label: &str) -> Result<Value, AppError> {
    let response = request
        .send()
        .await
        .map_err(|_| AppError::Message(format!("{label}连接失败，请检查华为网络连接")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Message(format!(
            "{label}失败 (HTTP {})",
            status.as_u16()
        )));
    }
    response
        .json()
        .await
        .map_err(|_| AppError::Message(format!("{label}返回了无效 JSON")))
}

async fn user_detail(client: &Client, token: &str) -> Result<Value, AppError> {
    let value = json_response(
        client
            .post(format!("{CONTROL_API}/auth/internal/getUserDetail"))
            .header("x-auth-token", token)
            .header("Agent-Type", "AgentCenter")
            .json(&serde_json::json!({})),
        "华为账号信息查询",
    )
    .await?;
    if value["code"] != 200 || !value["data"].is_object() {
        return Err(AppError::Message("华为账号信息查询失败，请重新登录".into()));
    }
    Ok(value["data"].clone())
}

fn area(detail: &Value) -> &'static str {
    if matches!(detail["region"].as_str(), Some("y" | "yellow")) {
        "yellow"
    } else {
        "green"
    }
}

pub(super) async fn fetch_models(
    client: &Client,
    token: &str,
) -> Result<Vec<FetchedModel>, AppError> {
    let detail = user_detail(client, token).await?;
    let value = json_response(
        client
            .get(format!(
                "{CONTROL_API}/chat/modles?checkUserPermission=TRUE"
            ))
            .header("x-auth-token", token)
            .header("Agent-Type", "AgentCenter")
            .header("plugin-version", PLUGIN_VERSION)
            .header("area", area(&detail))
            .header(
                "depart",
                detail["departmentPath"].as_str().unwrap_or_default(),
            ),
        "华为模型列表查询",
    )
    .await?;
    parse_models(&value)
}

fn parse_models(value: &Value) -> Result<Vec<FetchedModel>, AppError> {
    fn collect(rows: &[Value], ids: &mut BTreeSet<String>) -> Result<(), AppError> {
        for row in rows {
            // Auto groups need CLI-specific routing. Offer their concrete model IDs to the gateway.
            if let Some(routes) = row["routModels"].as_array().filter(|rows| !rows.is_empty()) {
                collect(routes, ids)?;
                continue;
            }
            let id = row["modelId"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| row["name"].as_str().filter(|s| !s.trim().is_empty()))
                .ok_or_else(|| AppError::Message("华为模型列表包含缺少模型 ID 的条目".into()))?;
            ids.insert(id.to_string());
        }
        Ok(())
    }
    let rows = value
        .as_array()
        .ok_or_else(|| AppError::Message("华为模型列表响应格式不正确".into()))?;
    let mut ids = BTreeSet::new();
    collect(rows, &mut ids)?;
    if ids.is_empty() {
        return Err(AppError::Message(
            "当前华为账号未返回可用模型，请检查账号权限".into(),
        ));
    }
    Ok(ids
        .into_iter()
        .map(|id| FetchedModel {
            id,
            owned_by: Some("Huawei CodeAgent".into()),
        })
        .collect())
}

fn quota_url(zone: &str, model_id: Option<&str>) -> Result<reqwest::Url, AppError> {
    let mut url = reqwest::Url::parse(&format!(
        "https://snapengine.cida.cce.prod-szv-{zone}.dragon.tools.huawei.com/api/v1/"
    ))
    .map_err(|_| AppError::Message("华为额度查询地址无效".into()))?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| AppError::Message("华为额度查询地址无效".into()))?;
        path.pop_if_empty();
        if let Some(model_id) = model_id {
            if model_id.trim().is_empty() || matches!(model_id, "." | "..") {
                return Err(AppError::Message("请选择要查询用量的模型".into()));
            }
            // Treat the model ID as one encoded path segment, never as a URL/path.
            path.push("models").push(model_id);
        }
        path.push("quota").push("usage");
    }
    Ok(url)
}

pub(super) async fn fetch_quota(
    client: &Client,
    token: &str,
    model_id: Option<&str>,
) -> Result<Value, AppError> {
    let detail = user_detail(client, token).await?;
    let zone = if area(&detail) == "yellow" { "y" } else { "g" };
    let value = json_response(
        client
            .get(quota_url(zone, model_id)?)
            .header("x-auth-token", token)
            .header("plugin-version", PLUGIN_VERSION)
            .header(
                "depart",
                detail["fullDepartmentNamePathUrlEncode"]
                    .as_str()
                    .unwrap_or_default(),
            ),
        "华为额度用量查询",
    )
    .await?;
    if !value["quotas"].is_array() {
        return Err(AppError::Message("华为额度用量响应格式不正确".into()));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn huawei_catalog_flattens_routes_and_deduplicates_real_ids() {
        let models = parse_models(&json!([
            {"modelId":"auto", "routModels":[{"modelId":"glm"},{"modelId":"small"},{"modelId":"small"}]},
            {"modelId":"deepseek", "name":"Display name"}, {"modelId":"", "name":"legacy-id"}
        ])).unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["deepseek", "glm", "legacy-id", "small"]
        );
    }

    #[test]
    fn huawei_catalog_rejects_empty_errors_and_cli_output_without_fallback() {
        for value in [
            json!([]),
            json!({"code":401}),
            json!("Update available"),
            json!([{}]),
        ] {
            assert!(parse_models(&value).is_err());
        }
    }

    #[test]
    fn huawei_catalog_respects_user_network_region() {
        assert_eq!(area(&json!({"region":"y"})), "yellow");
        assert_eq!(area(&json!({"region":"g"})), "green");
    }
    #[test]
    fn huawei_model_quota_url_encodes_model_id_and_keeps_account_endpoint() {
        assert_eq!(quota_url("g", None).unwrap().path(), "/api/v1/quota/usage");
        let url = quota_url("y", Some("vendor/model?x#y")).unwrap();
        assert_eq!(
            url.path(),
            "/api/v1/models/vendor%2Fmodel%3Fx%23y/quota/usage"
        );
        assert!(url.host_str().unwrap().contains("prod-szv-y"));
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        for id in ["", " ", ".", ".."] {
            assert!(quota_url("g", Some(id)).is_err());
        }
    }
}

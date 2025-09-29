use crate::cloud_sync::{CloudSyncResult, CloudSyncError};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const GITHUB_API_BASE: &str = "https://api.github.com";
const TIMEOUT_SECONDS: u64 = 60;

#[derive(Debug, Serialize, Deserialize)]
struct GistFile {
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GistRequest {
    description: String,
    public: bool,
    files: std::collections::HashMap<String, GistFile>,
}

#[derive(Debug, Deserialize)]
struct GistResponse {
    #[allow(dead_code)]
    id: String,
    html_url: String,
    files: std::collections::HashMap<String, GistFile>,
}

pub struct GitHubClient {
    #[allow(dead_code)]
    token: String,
    client: reqwest::Client,
    #[allow(dead_code)]
    headers: HeaderMap,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("cc-switch-cloud-sync/1.0"),
        );

        let headers_clone = headers.clone();
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(TIMEOUT_SECONDS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { token, client, headers: headers_clone }
    }

    pub async fn validate_token(&self) -> CloudSyncResult<Value> {
        let url = format!("{}/user", GITHUB_API_BASE);

        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| CloudSyncError::Network(format!("Failed to validate token: {}", e)))?;

        // 保存状态码和headers以便后续使用
        let status = response.status();
        let is_rate_limited = response.headers().get("x-ratelimit-remaining") == Some(&HeaderValue::from_static("0"));

        if status.is_success() {
            let response_text = response.text().await
                .map_err(|e| CloudSyncError::Parse(format!("Failed to read response: {}", e)))?;

            let user_info: Value = serde_json::from_str(&response_text)
                .map_err(|e| CloudSyncError::Parse(format!("Failed to parse user info: {}", e)))?;

            Ok(user_info)
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unable to read error response".to_string());

            if status == 401 {
                Err(CloudSyncError::Authentication("Invalid GitHub token".into()))
            } else if status == 403 && is_rate_limited {
                Err(CloudSyncError::RateLimit("GitHub API rate limit exceeded".into()))
            } else {
                Err(CloudSyncError::Api(format!("Token validation failed: {} - {}", status, error_text)))
            }
        }
    }

    pub async fn create_gist(&self, content: &str) -> CloudSyncResult<String> {
        let url = format!("{}/gists", GITHUB_API_BASE);

        let mut files = std::collections::HashMap::new();
        files.insert(
            "cc-switch-config.json".to_string(),
            GistFile { content: content.to_string() },
        );

        let gist_request = GistRequest {
            description: "CC-Switch Configuration Backup".to_string(),
            public: false,
            files,
        };

        let response = self.client
            .post(&url)
            .json(&gist_request)
            .send()
            .await
            .map_err(|e| CloudSyncError::Network(format!("Failed to create gist: {}", e)))?;

        // 保存状态码和headers以便后续使用
        let status = response.status();
        let is_rate_limited = response.headers().get("x-ratelimit-remaining") == Some(&HeaderValue::from_static("0"));

        if status.is_success() {
            let response_text = response.text().await
                .map_err(|e| CloudSyncError::Parse(format!("Failed to read response: {}", e)))?;

            let gist: GistResponse = serde_json::from_str(&response_text)
                .map_err(|e| CloudSyncError::Parse(format!("Failed to parse gist response: {}", e)))?;
            Ok(gist.html_url)
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());

            if status == 401 {
                Err(CloudSyncError::Authentication("Unauthorized to create gist".into()))
            } else if status == 403 && is_rate_limited {
                Err(CloudSyncError::RateLimit("GitHub API rate limit exceeded".into()))
            } else {
                Err(CloudSyncError::Api(format!("Failed to create gist: {}", error_text)))
            }
        }
    }

    pub async fn update_gist_by_url(&self, gist_url: &str, content: &str) -> CloudSyncResult<()> {
        // Extract gist ID from URL
        let gist_id = self.extract_gist_id(gist_url)?;

        // Call the existing update_gist method
        self.update_gist(&gist_id, content).await
    }

    pub async fn update_gist(&self, gist_id: &str, content: &str) -> CloudSyncResult<()> {
        let url = format!("{}/gists/{}", GITHUB_API_BASE, gist_id);

        let mut files = std::collections::HashMap::new();
        files.insert(
            "cc-switch-config.json".to_string(),
            GistFile { content: content.to_string() },
        );

        let update_request = json!({
            "files": files
        });

        let response = self.client
            .patch(&url)
            .json(&update_request)
            .send()
            .await
            .map_err(|e| CloudSyncError::Network(format!("Failed to update gist: {}", e)))?;

        // 保存状态码和headers以便后续使用
        let status = response.status();
        let is_rate_limited = response.headers().get("x-ratelimit-remaining") == Some(&HeaderValue::from_static("0"));

        if status.is_success() {
            Ok(())
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());

            if status == 401 {
                Err(CloudSyncError::Authentication("Unauthorized to update gist".into()))
            } else if status == 404 {
                Err(CloudSyncError::NotFound(format!("Gist {} not found", gist_id)))
            } else if status == 403 && is_rate_limited {
                Err(CloudSyncError::RateLimit("GitHub API rate limit exceeded".into()))
            } else {
                Err(CloudSyncError::Api(format!("Failed to update gist: {}", error_text)))
            }
        }
    }

    pub async fn get_gist(&self, gist_url: &str) -> CloudSyncResult<String> {
        // Extract gist ID from URL
        let gist_id = self.extract_gist_id(gist_url)?;
        let url = format!("{}/gists/{}", GITHUB_API_BASE, gist_id);

        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| CloudSyncError::Network(format!("Failed to get gist: {}", e)))?;

        // 保存状态码和headers以便后续使用
        let status = response.status();
        let is_rate_limited = response.headers().get("x-ratelimit-remaining") == Some(&HeaderValue::from_static("0"));

        if status.is_success() {
            let response_text = response.text().await
                .map_err(|e| CloudSyncError::Parse(format!("Failed to read response: {}", e)))?;

            let gist: GistResponse = serde_json::from_str(&response_text)
                .map_err(|e| CloudSyncError::Parse(format!("Failed to parse gist response: {}", e)))?;

            // Get the cc-switch-config.json file content
            gist.files.get("cc-switch-config.json")
                .map(|file| file.content.clone())
                .ok_or_else(|| CloudSyncError::NotFound("cc-switch-config.json not found in gist".into()))
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unable to read error response".to_string());

            if status == 401 {
                Err(CloudSyncError::Authentication("Unauthorized to access gist".into()))
            } else if status == 404 {
                Err(CloudSyncError::NotFound(format!("Gist {} not found", gist_id)))
            } else if status == 403 && is_rate_limited {
                Err(CloudSyncError::RateLimit("GitHub API rate limit exceeded".into()))
            } else {
                Err(CloudSyncError::Api(format!("Failed to get gist: {}", error_text)))
            }
        }
    }

    fn extract_gist_id(&self, gist_url: &str) -> CloudSyncResult<String> {
        // Handle various GitHub Gist URL formats
        // https://gist.github.com/username/gist_id
        // https://gist.github.com/gist_id

        let url = gist_url.trim_end_matches('/');
        let parts: Vec<&str> = url.split('/').collect();

        if parts.len() >= 4 && parts[2] == "gist.github.com" {
            let last_part = parts.last().unwrap();
            // Remove any file extensions or anchors
            let gist_id = last_part.split('.').next().unwrap_or(last_part);
            let gist_id = gist_id.split('#').next().unwrap_or(gist_id);

            if !gist_id.is_empty() {
                return Ok(gist_id.to_string());
            }
        }

        Err(CloudSyncError::Validation(format!("Invalid Gist URL: {}", gist_url)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_gist_id() {
        let client = GitHubClient::new("test_token".to_string());

        // Test various URL formats
        assert_eq!(
            client.extract_gist_id("https://gist.github.com/user/abc123").unwrap(),
            "abc123"
        );
        assert_eq!(
            client.extract_gist_id("https://gist.github.com/abc123").unwrap(),
            "abc123"
        );
        assert_eq!(
            client.extract_gist_id("https://gist.github.com/user/abc123/").unwrap(),
            "abc123"
        );
        assert_eq!(
            client.extract_gist_id("https://gist.github.com/user/abc123.git").unwrap(),
            "abc123"
        );
        assert_eq!(
            client.extract_gist_id("https://gist.github.com/user/abc123#file-config-json").unwrap(),
            "abc123"
        );

        // Test invalid URLs
        assert!(client.extract_gist_id("not-a-url").is_err());
        assert!(client.extract_gist_id("https://github.com/user/repo").is_err());
    }
}
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
    id: String,
    html_url: String,
    files: std::collections::HashMap<String, GistFile>,
}

pub struct GitHubClient {
    token: String,
    client: reqwest::Client,
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

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(TIMEOUT_SECONDS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { token, client }
    }

    pub async fn validate_token(&self) -> CloudSyncResult<Value> {
        let response = self.client
            .get(format!("{}/user", GITHUB_API_BASE))
            .send()
            .await
            .map_err(|e| CloudSyncError::Network(format!("Failed to validate token: {}", e)))?;

        if response.status().is_success() {
            let user_info: Value = response.json().await
                .map_err(|e| CloudSyncError::Parse(format!("Failed to parse user info: {}", e)))?;
            Ok(user_info)
        } else if response.status() == 401 {
            Err(CloudSyncError::Authentication("Invalid GitHub token".into()))
        } else if response.status() == 403 && response.headers().get("x-ratelimit-remaining") == Some(&HeaderValue::from_static("0")) {
            Err(CloudSyncError::RateLimit("GitHub API rate limit exceeded".into()))
        } else {
            Err(CloudSyncError::Api(format!("Token validation failed: {}", response.status())))
        }
    }

    pub async fn create_gist(&self, content: &str) -> CloudSyncResult<String> {
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
            .post(format!("{}/gists", GITHUB_API_BASE))
            .json(&gist_request)
            .send()
            .await
            .map_err(|e| CloudSyncError::Network(format!("Failed to create gist: {}", e)))?;

        if response.status().is_success() {
            let gist: GistResponse = response.json().await
                .map_err(|e| CloudSyncError::Parse(format!("Failed to parse gist response: {}", e)))?;
            Ok(gist.html_url)
        } else if response.status() == 401 {
            Err(CloudSyncError::Authentication("Unauthorized to create gist".into()))
        } else if response.status() == 403 && response.headers().get("x-ratelimit-remaining") == Some(&HeaderValue::from_static("0")) {
            Err(CloudSyncError::RateLimit("GitHub API rate limit exceeded".into()))
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(CloudSyncError::Api(format!("Failed to create gist: {}", error_text)))
        }
    }

    pub async fn update_gist(&self, gist_id: &str, content: &str) -> CloudSyncResult<()> {
        let mut files = std::collections::HashMap::new();
        files.insert(
            "cc-switch-config.json".to_string(),
            GistFile { content: content.to_string() },
        );

        let update_request = json!({
            "files": files
        });

        let response = self.client
            .patch(format!("{}/gists/{}", GITHUB_API_BASE, gist_id))
            .json(&update_request)
            .send()
            .await
            .map_err(|e| CloudSyncError::Network(format!("Failed to update gist: {}", e)))?;

        if response.status().is_success() {
            Ok(())
        } else if response.status() == 401 {
            Err(CloudSyncError::Authentication("Unauthorized to update gist".into()))
        } else if response.status() == 404 {
            Err(CloudSyncError::NotFound(format!("Gist {} not found", gist_id)))
        } else if response.status() == 403 && response.headers().get("x-ratelimit-remaining") == Some(&HeaderValue::from_static("0")) {
            Err(CloudSyncError::RateLimit("GitHub API rate limit exceeded".into()))
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(CloudSyncError::Api(format!("Failed to update gist: {}", error_text)))
        }
    }

    pub async fn get_gist(&self, gist_url: &str) -> CloudSyncResult<String> {
        // Extract gist ID from URL
        let gist_id = self.extract_gist_id(gist_url)?;

        let response = self.client
            .get(format!("{}/gists/{}", GITHUB_API_BASE, gist_id))
            .send()
            .await
            .map_err(|e| CloudSyncError::Network(format!("Failed to get gist: {}", e)))?;

        if response.status().is_success() {
            let gist: GistResponse = response.json().await
                .map_err(|e| CloudSyncError::Parse(format!("Failed to parse gist response: {}", e)))?;

            // Get the cc-switch-config.json file content
            gist.files.get("cc-switch-config.json")
                .map(|file| file.content.clone())
                .ok_or_else(|| CloudSyncError::NotFound("cc-switch-config.json not found in gist".into()))
        } else if response.status() == 401 {
            Err(CloudSyncError::Authentication("Unauthorized to access gist".into()))
        } else if response.status() == 404 {
            Err(CloudSyncError::NotFound(format!("Gist {} not found", gist_id)))
        } else if response.status() == 403 && response.headers().get("x-ratelimit-remaining") == Some(&HeaderValue::from_static("0")) {
            Err(CloudSyncError::RateLimit("GitHub API rate limit exceeded".into()))
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(CloudSyncError::Api(format!("Failed to get gist: {}", error_text)))
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
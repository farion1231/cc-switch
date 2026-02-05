use chrono::Utc;
use reqwest::{Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::AppError;
use crate::proxy::http_client;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavBackupRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavBackupResult {
    pub remote_url: String,
    pub file_name: String,
    pub size_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct WebDavDownloadResult {
    pub file_name: String,
    pub content: String,
}

pub struct WebDavBackupService;

impl WebDavBackupService {
    pub async fn test_connection(config: &WebDavBackupRequest) -> Result<(), AppError> {
        let url = build_probe_url(config)?;
        let client = http_client::get();
        let method = Method::from_bytes(b"PROPFIND")
            .map_err(|e| AppError::InvalidInput(format!("WebDAV 方法无效: {e}")))?;

        let mut request = client
            .request(method, url)
            .header("Depth", "0")
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS));

        if let Some((user, pass)) = auth_parts(config) {
            request = request.basic_auth(user, pass);
        }

        let response = request
            .send()
            .await
            .map_err(|e| AppError::Message(format!("WebDAV 连接失败: {e}")))?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        match status {
            StatusCode::UNAUTHORIZED => Err(AppError::Message("WebDAV 认证失败".to_string())),
            StatusCode::FORBIDDEN => Err(AppError::Message("WebDAV 权限不足".to_string())),
            StatusCode::NOT_FOUND => Err(AppError::Message(
                "WebDAV 路径不存在，请检查远程路径".to_string(),
            )),
            _ => Err(AppError::Message(format!(
                "WebDAV 连接失败: HTTP {} {}",
                status.as_u16(),
                body
            ))),
        }
    }

    pub async fn upload_backup(
        config: &WebDavBackupRequest,
        sql_content: String,
    ) -> Result<WebDavBackupResult, AppError> {
        let base_url = parse_base_url(&config.url)?;
        let remote_path = normalize_remote_path(config.remote_path.as_deref());
        let has_file_name = has_explicit_file_name(config);
        let treat_as_dir = infer_treat_as_dir(&remote_path, has_file_name);
        let file_name = effective_file_name(config, &remote_path, treat_as_dir)?;
        let dir_path = if remote_path.is_empty() {
            ""
        } else if treat_as_dir {
            remote_path.trim_end_matches('/')
        } else {
            remote_path
                .rsplit_once('/')
                .map(|(dir, _)| dir)
                .unwrap_or("")
        };

        let size_bytes = sql_content.len();
        let client = http_client::get();
        let auth = auth_parts(config);
        ensure_remote_directories(&client, &base_url, dir_path, &auth).await?;
        let target_url = build_target_url(config, &file_name)?;
        let mut request = client
            .put(target_url.clone())
            .header("Content-Type", "application/sql")
            .body(sql_content)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS));

        if let Some((ref user, ref pass)) = auth {
            request = request.basic_auth(user, pass.as_deref());
        }

        let response = request
            .send()
            .await
            .map_err(|e| AppError::Message(format!("上传失败: {e}")))?;

        if response.status().is_success() {
            log::info!("[WebDAV] Backup uploaded successfully to: {}", target_url);
            return Ok(WebDavBackupResult {
                remote_url: target_url.to_string(),
                file_name,
                size_bytes,
            });
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        log::error!(
            "[WebDAV] PUT failed: status={}, url={}, body={}",
            status.as_u16(),
            target_url,
            body
        );

        match status {
            StatusCode::UNAUTHORIZED => Err(AppError::Message("WebDAV 认证失败".to_string())),
            StatusCode::FORBIDDEN => Err(AppError::Message(format!(
                "WebDAV 无权限上传到 {}。\
坚果云不允许直接上传到根目录 /dav/；请确保远程路径指向一个已存在的同步文件夹。",
                target_url
            ))),
            StatusCode::NOT_FOUND => Err(AppError::Message(format!(
                "WebDAV 目标路径不存在: {}。请确保远程路径指向一个已存在的文件夹。",
                target_url
            ))),
            StatusCode::CONFLICT => Err(AppError::Message(format!(
                "WebDAV 上传失败（409 Conflict），目标目录可能不存在: {}。\
若使用坚果云，请先在坚果云网页/客户端创建对应的文件夹，再重试。",
                target_url
            ))),
            _ => Err(AppError::Message(format!(
                "上传失败: HTTP {} {}",
                status.as_u16(),
                body
            ))),
        }
    }

    /// Download the latest backup from WebDAV
    pub async fn download_latest(
        config: &WebDavBackupRequest,
    ) -> Result<WebDavDownloadResult, AppError> {
        let base_url = parse_base_url(&config.url)?;
        let remote_path = normalize_remote_path(config.remote_path.as_deref());
        let client = http_client::get();
        let auth = auth_parts(config);

        // Build the directory URL to list files
        let mut list_url = base_url.clone();
        if !remote_path.is_empty() {
            push_segments(&mut list_url, &remote_path)?;
        }
        ensure_trailing_slash(&mut list_url);

        // Use PROPFIND with Depth 1 to list directory contents
        let propfind_method = Method::from_bytes(b"PROPFIND")
            .map_err(|e| AppError::InvalidInput(format!("WebDAV 方法无效: {e}")))?;

        let mut request = client
            .request(propfind_method, list_url.clone())
            .header("Depth", "1")
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS));

        if let Some((ref user, ref pass)) = auth {
            request = request.basic_auth(user, pass.as_deref());
        }

        let response = request
            .send()
            .await
            .map_err(|e| AppError::Message(format!("WebDAV 列出文件失败: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Message(format!(
                "WebDAV 列出文件失败: HTTP {} {}",
                status.as_u16(),
                body
            )));
        }

        let body = response.text().await.unwrap_or_default();

        // Parse the PROPFIND response to find .sql files
        let files = parse_propfind_response(&body, &list_url)?;
        let sql_files: Vec<_> = files.into_iter().filter(|f| f.ends_with(".sql")).collect();

        if sql_files.is_empty() {
            return Err(AppError::Message(
                "WebDAV 远程目录中没有找到 .sql 备份文件".to_string(),
            ));
        }

        // Sort by name (which includes timestamp) and get the latest
        let mut sorted_files = sql_files;
        sorted_files.sort();
        let latest_file = sorted_files.last().unwrap();

        // Build download URL
        let mut download_url = base_url;
        if !remote_path.is_empty() {
            push_segments(&mut download_url, &remote_path)?;
        }
        push_segments(&mut download_url, latest_file)?;

        log::info!("[WebDAV] Downloading latest backup: {}", download_url);

        // Download the file
        let mut get_request = client
            .get(download_url.clone())
            .timeout(Duration::from_secs(300)); // 5 minutes for large files

        if let Some((ref user, ref pass)) = auth {
            get_request = get_request.basic_auth(user, pass.as_deref());
        }

        let get_response = get_request
            .send()
            .await
            .map_err(|e| AppError::Message(format!("WebDAV 下载文件失败: {e}")))?;

        if !get_response.status().is_success() {
            let status = get_response.status();
            let body = get_response.text().await.unwrap_or_default();
            return Err(AppError::Message(format!(
                "WebDAV 下载文件失败: HTTP {} {}",
                status.as_u16(),
                body
            )));
        }

        let content = get_response
            .text()
            .await
            .map_err(|e| AppError::Message(format!("读取文件内容失败: {e}")))?;

        log::info!(
            "[WebDAV] Downloaded {} ({} bytes)",
            latest_file,
            content.len()
        );

        Ok(WebDavDownloadResult {
            file_name: latest_file.clone(),
            content,
        })
    }
}

fn auth_parts(config: &WebDavBackupRequest) -> Option<(String, Option<String>)> {
    let user = config.username.as_ref()?.trim();
    if user.is_empty() {
        return None;
    }
    let pass = config.password.as_ref().map(|s| s.to_string());
    Some((user.to_string(), pass))
}

fn has_explicit_file_name(config: &WebDavBackupRequest) -> bool {
    config
        .file_name
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .is_some()
}

fn infer_treat_as_dir(remote_path: &str, has_explicit_file_name: bool) -> bool {
    if remote_path.is_empty() || remote_path.ends_with('/') || has_explicit_file_name {
        return true;
    }
    !remote_path.ends_with(".sql")
}

fn effective_file_name(
    config: &WebDavBackupRequest,
    remote_path: &str,
    treat_as_dir: bool,
) -> Result<String, AppError> {
    if treat_as_dir {
        return build_file_name(config);
    }

    let name = remote_path.rsplit('/').next().unwrap_or("").trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput(
            "远程路径无效：缺少文件名".to_string(),
        ));
    }
    Ok(name.to_string())
}

fn parse_base_url(raw: &str) -> Result<Url, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("WebDAV 地址不能为空".to_string()));
    }

    let url =
        Url::parse(trimmed).map_err(|e| AppError::InvalidInput(format!("WebDAV 地址无效: {e}")))?;

    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(AppError::InvalidInput(
            "WebDAV 仅支持 http/https 地址".to_string(),
        )),
    }
}

fn build_file_name(config: &WebDavBackupRequest) -> Result<String, AppError> {
    if let Some(name) = config.file_name.as_ref() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Ok(default_file_name());
        }
        if trimmed.contains('/') || trimmed.contains('\\') {
            return Err(AppError::InvalidInput(
                "文件名不能包含路径分隔符".to_string(),
            ));
        }
        return Ok(trimmed.to_string());
    }
    Ok(default_file_name())
}

fn default_file_name() -> String {
    let stamp = Utc::now().format("%Y%m%d_%H%M%S");
    format!("cc-switch-export-{stamp}.sql")
}

fn build_target_url(config: &WebDavBackupRequest, file_name: &str) -> Result<Url, AppError> {
    let mut url = parse_base_url(&config.url)?;
    let remote_path = normalize_remote_path(config.remote_path.as_deref());
    let has_file_name = has_explicit_file_name(config);

    let treat_as_dir = infer_treat_as_dir(&remote_path, has_file_name);
    if treat_as_dir {
        let dir_path = remote_path.trim_end_matches('/');
        push_segments(&mut url, dir_path)?;
        push_segments(&mut url, file_name)?;
    } else {
        push_segments(&mut url, remote_path.as_str())?;
    }

    Ok(url)
}

fn build_probe_url(config: &WebDavBackupRequest) -> Result<Url, AppError> {
    let mut url = parse_base_url(&config.url)?;
    let remote_path = normalize_remote_path(config.remote_path.as_deref());
    if remote_path.is_empty() {
        return Ok(url);
    }

    let has_file_name = has_explicit_file_name(config);
    let treat_as_dir = infer_treat_as_dir(&remote_path, has_file_name);
    if treat_as_dir {
        let dir_path = remote_path.trim_end_matches('/');
        push_segments(&mut url, dir_path)?;
        return Ok(url);
    }

    if let Some((dir, _)) = remote_path.rsplit_once('/') {
        push_segments(&mut url, dir)?;
    }

    Ok(url)
}

fn normalize_remote_path(raw: Option<&str>) -> String {
    raw.unwrap_or("").trim().replace('\\', "/").to_string()
}

fn ensure_trailing_slash(url: &mut Url) {
    let path = url.path();
    if path.ends_with('/') {
        return;
    }
    url.set_path(&format!("{path}/"));
}

async fn ensure_remote_directories(
    client: &reqwest::Client,
    base_url: &Url,
    dir_path: &str,
    auth: &Option<(String, Option<String>)>,
) -> Result<(), AppError> {
    if dir_path.is_empty() {
        return Ok(());
    }

    let mkcol_method = Method::from_bytes(b"MKCOL")
        .map_err(|e| AppError::InvalidInput(format!("WebDAV 方法无效: {e}")))?;
    let propfind_method = Method::from_bytes(b"PROPFIND")
        .map_err(|e| AppError::InvalidInput(format!("WebDAV 方法无效: {e}")))?;

    let mut current_url = base_url.clone();
    let mut created_path = String::new();
    for segment in dir_path.split('/').filter(|s| !s.is_empty()) {
        push_segments(&mut current_url, segment)?;
        let mut check_url = current_url.clone();
        ensure_trailing_slash(&mut check_url);

        if created_path.is_empty() {
            created_path.push_str(segment);
        } else {
            created_path.push('/');
            created_path.push_str(segment);
        }

        // First check if directory exists using PROPFIND
        let mut propfind_request = client
            .request(propfind_method.clone(), check_url.clone())
            .header("Depth", "0")
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        if let Some((ref user, ref pass)) = auth {
            propfind_request = propfind_request.basic_auth(user, pass.as_deref());
        }

        let propfind_response = propfind_request.send().await;
        if let Ok(resp) = propfind_response {
            if resp.status().is_success() {
                // Directory already exists, skip MKCOL
                continue;
            }
        }

        // Directory doesn't exist, try to create it with MKCOL
        let check_url_str = check_url.to_string();
        let mut mkcol_request = client
            .request(mkcol_method.clone(), check_url)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        if let Some((ref user, ref pass)) = auth {
            mkcol_request = mkcol_request.basic_auth(user, pass.as_deref());
        }

        let response = mkcol_request
            .send()
            .await
            .map_err(|e| AppError::Message(format!("创建 WebDAV 目录失败: {e}")))?;

        let status = response.status();
        // Success: 201 Created
        if status.is_success() {
            log::info!("[WebDAV] MKCOL created directory: {}", check_url_str);
            continue;
        }

        // 405 Method Not Allowed might mean:
        // 1. Directory already exists (some servers)
        // 2. Server doesn't support MKCOL at all
        // Since we already checked with PROPFIND and it said dir doesn't exist,
        // 405 here likely means server doesn't support creating directories
        if status == StatusCode::METHOD_NOT_ALLOWED {
            log::warn!(
                "[WebDAV] MKCOL returned 405 for {}, server may not support directory creation. Path: /{created_path}",
                check_url_str
            );
            // We'll continue and let the PUT fail with a clearer error if the dir really doesn't exist
            continue;
        }

        let body = response.text().await.unwrap_or_default();
        match status {
            StatusCode::UNAUTHORIZED => {
                return Err(AppError::Message("WebDAV 认证失败".to_string()))
            }
            StatusCode::FORBIDDEN => {
                return Err(AppError::Message(format!(
                    "WebDAV 无权限创建目录 /{created_path}（403 Forbidden）。\
坚果云不允许在根目录 /dav/ 下直接创建文件夹；\
请先在坚果云网页端或客户端创建同步文件夹（如 cc-switch-backup），\
然后在「远程路径」填写该文件夹名。"
                )))
            }
            StatusCode::CONFLICT => {
                return Err(AppError::Message(format!(
                    "WebDAV 无法创建目录 /{created_path}（409 Conflict）。\
部分 WebDAV 服务（例如坚果云）不允许通过 WebDAV 创建顶层文件夹；\
请先在服务端（网页/客户端）手动创建目录 /{} ，再重试。",
                    dir_path
                        .split('/')
                        .find(|s| !s.is_empty())
                        .unwrap_or(&created_path),
                )))
            }
            _ => {
                return Err(AppError::Message(format!(
                    "创建 WebDAV 目录失败: HTTP {} {}",
                    status.as_u16(),
                    body
                )))
            }
        }
    }

    Ok(())
}

fn push_segments(url: &mut Url, path: &str) -> Result<(), AppError> {
    if path.is_empty() {
        return Ok(());
    }

    let mut segments = url
        .path_segments_mut()
        .map_err(|_| AppError::InvalidInput("WebDAV 地址格式不支持追加路径".to_string()))?;

    // Remove trailing empty segment caused by trailing slash (e.g., /dav/ -> /dav)
    // This prevents double slashes like /dav//file.sql
    segments.pop_if_empty();

    for segment in path.split('/').filter(|s| !s.is_empty()) {
        segments.push(segment);
    }
    Ok(())
}

/// Parse WebDAV PROPFIND response to extract file names
fn parse_propfind_response(xml: &str, base_url: &Url) -> Result<Vec<String>, AppError> {
    let mut files = Vec::new();
    let base_path = base_url.path();

    // Extract all href values from XML (handles single-line compressed XML)
    let hrefs = extract_all_hrefs(xml);

    for href in hrefs {
        // Skip the directory itself (ends with /)
        if href.ends_with('/') {
            continue;
        }

        // Extract just the filename from the path
        let path = if href.starts_with("http://") || href.starts_with("https://") {
            if let Ok(url) = Url::parse(&href) {
                url.path().to_string()
            } else {
                href.to_string()
            }
        } else {
            href.to_string()
        };

        // Get the filename (last segment)
        if let Some(name) = path.rsplit('/').next() {
            let name = name.trim();
            if !name.is_empty()
                && name
                    != base_path
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
            {
                let decoded = simple_url_decode(name);
                files.push(decoded);
            }
        }
    }

    log::debug!(
        "[WebDAV] Parsed PROPFIND: found {} file(s) from response",
        files.len()
    );

    Ok(files)
}

/// Extract all href values from XML string
/// Handles compressed single-line XML by scanning for all href tag occurrences
fn extract_all_hrefs(xml: &str) -> Vec<String> {
    let mut hrefs = Vec::new();
    let patterns: &[(&str, &str)] = &[
        ("<D:href>", "</D:href>"),
        ("<d:href>", "</d:href>"),
        ("<href>", "</href>"),
        ("<lp1:href>", "</lp1:href>"),
    ];

    for &(start_tag, end_tag) in patterns {
        let mut search_from = 0;
        while let Some(start) = xml[search_from..].find(start_tag) {
            let abs_start = search_from + start + start_tag.len();
            if let Some(end) = xml[abs_start..].find(end_tag) {
                let href = &xml[abs_start..abs_start + end];
                hrefs.push(href.to_string());
                search_from = abs_start + end + end_tag.len();
            } else {
                break;
            }
        }
        // If we found hrefs with this pattern, no need to try others
        if !hrefs.is_empty() {
            break;
        }
    }

    hrefs
}

/// Percent-decode a URL path segment (UTF-8)
///
/// WebDAV PROPFIND `href` values commonly percent-encode UTF-8 bytes for non-ASCII filenames.
/// This function decodes `%XX` into raw bytes and then interprets the result as UTF-8.
fn simple_url_decode(s: &str) -> String {
    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    let mut out = Vec::<u8>::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h1), Some(h2)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h1 << 4) | h2);
                i += 3;
                continue;
            }
        }

        out.push(bytes[i]);
        i += 1;
    }

    match String::from_utf8(out) {
        Ok(s) => s,
        Err(err) => String::from_utf8_lossy(&err.into_bytes()).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::simple_url_decode;

    #[test]
    fn simple_url_decode_decodes_ascii() {
        assert_eq!(simple_url_decode("hello%20world.sql"), "hello world.sql");
    }

    #[test]
    fn simple_url_decode_decodes_utf8_percent_sequences() {
        assert_eq!(simple_url_decode("%E6%B5%8B%E8%AF%95.sql"), "测试.sql");
        assert_eq!(
            simple_url_decode("%E3%81%93%E3%82%93%E3%81%AB%E3%81%A1%E3%81%AF.sql"),
            "こんにちは.sql"
        );
    }

    #[test]
    fn simple_url_decode_keeps_invalid_sequences() {
        assert_eq!(simple_url_decode("%ZZ.sql"), "%ZZ.sql");
        assert_eq!(simple_url_decode("a%2.sql"), "a%2.sql");
    }
}

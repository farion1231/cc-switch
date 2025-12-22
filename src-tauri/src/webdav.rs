//! WebDAV 云端备份服务
//!
//! 提供 WebDAV 协议支持，允许用户将配置备份到云端存储

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use webdav_request::DavClient;
use reqwest;

/// WebDAV 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub remote_path: String, // 云端存储路径，例如 "/cc-switch/backups/"
}

/// WebDAV 客户端包装器
pub struct WebDavClient {
    config: WebDavConfig,
    client: DavClient,
}

impl WebDavClient {
    /// 创建新的 WebDAV 客户端
    pub fn new(config: WebDavConfig) -> Result<Self, AppError> {
        let client = DavClient::new(&config.username, &config.password)
            .map_err(|e| AppError::Config(format!("创建 WebDAV 客户端失败: {e}")))?;

        Ok(Self { config, client })
    }

    /// 构建完整的远程路径
    fn build_remote_url(&self, filename: &str) -> String {
        let base = self.config.url.trim_end_matches('/');
        let path = self.config.remote_path.trim_matches('/');
        if path.is_empty() {
            format!("{}/{}", base, filename)
        } else {
            format!("{}/{}/{}", base, path, filename)
        }
    }

    /// 获取列表路径
    fn get_list_path(&self) -> String {
        let base = self.config.url.trim_end_matches('/');
        let path = self.config.remote_path.trim_matches('/');
        if path.is_empty() {
            format!("{}/", base)
        } else {
            format!("{}/{}/", base, path)
        }
    }

    /// 确保远程目录存在
    async fn ensure_remote_dir(&self) -> Result<(), AppError> {
        let path = self.config.remote_path.trim_matches('/');
        if path.is_empty() {
            return Ok(()); // 根目录总是存在
        }

        let base_url = self.config.url.trim_end_matches('/');
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AppError::Config(format!("创建 HTTP 客户端失败: {e}")))?;

        // 递归创建所有父目录
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_path = String::new();

        for part in parts {
            current_path.push('/');
            current_path.push_str(part);

            let dir_url = format!("{}{}/", base_url, current_path);
            log::info!("尝试创建目录: {}", dir_url);

            let response = client
                .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &dir_url)
                .basic_auth(&self.config.username, Some(&self.config.password))
                .send()
                .await
                .map_err(|e| AppError::Config(format!("创建远程目录失败: {e}")))?;

            let status = response.status();
            // 201 Created = 成功创建
            // 405 Method Not Allowed = 目录已存在
            if status.is_success() || status.as_u16() == 405 {
                log::info!("目录创建成功或已存在: {}", dir_url);
            } else {
                log::warn!("创建目录返回状态码 {}: {}", status.as_u16(), dir_url);
            }
        }

        Ok(())
    }

    /// 测试连接
    pub async fn test_connection(&self) -> Result<(), AppError> {
        // 使用 reqwest 测试连接，使用 PROPFIND 方法（WebDAV 标准）
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AppError::Config(format!("创建 HTTP 客户端失败: {e}")))?;

        let response = client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &self.config.url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .header("Depth", "0")
            .send()
            .await
            .map_err(|e| AppError::Config(format!("WebDAV 连接测试失败: {e}")))?;

        let status = response.status();
        if !status.is_success() && status.as_u16() != 207 {
            // 207 Multi-Status 也是成功的响应
            let error_body = response.text().await.unwrap_or_default();
            return Err(AppError::Config(format!(
                "WebDAV 连接失败 (状态码: {}): {}",
                status.as_u16(),
                if error_body.is_empty() {
                    "请检查服务器地址、用户名和密码是否正确。坚果云需要使用应用密码而非账号密码。"
                } else {
                    &error_body
                }
            )));
        }

        Ok(())
    }

    /// 上传文件到 WebDAV 服务器
    pub async fn upload_file(
        &self,
        local_path: &PathBuf,
        remote_filename: &str,
    ) -> Result<(), AppError> {
        // 确保远程目录存在
        self.ensure_remote_dir().await?;

        // 读取本地文件
        let file_data = tokio::fs::read(local_path)
            .await
            .map_err(|e| AppError::io(local_path, e))?;

        // 构建远程路径
        let remote_url = self.build_remote_url(remote_filename);

        // 使用 reqwest 上传文件
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AppError::Config(format!("创建 HTTP 客户端失败: {e}")))?;

        client
            .put(&remote_url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .body(file_data)
            .send()
            .await
            .map_err(|e| AppError::Config(format!("上传文件到 WebDAV 失败: {e}")))?;

        Ok(())
    }

    /// 从 WebDAV 服务器下载文件
    pub async fn download_file(
        &self,
        remote_filename: &str,
        local_path: &PathBuf,
    ) -> Result<(), AppError> {
        let remote_url = self.build_remote_url(remote_filename);
        log::info!("开始下载文件: {} -> {:?}", remote_url, local_path);

        // 确保本地目录存在
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::io(parent, e))?;
            log::info!("本地目录已创建: {:?}", parent);
        }

        // 使用 reqwest 直接下载文件
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AppError::Config(format!("创建 HTTP 客户端失败: {e}")))?;

        log::info!("发送 GET 请求到: {}", remote_url);
        let response = client
            .get(&remote_url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .send()
            .await
            .map_err(|e| AppError::Config(format!("从 WebDAV 下载文件失败: {e}")))?;

        let status = response.status();
        log::info!("下载响应状态码: {}", status.as_u16());

        if !status.is_success() {
            return Err(AppError::Config(format!(
                "下载文件失败，状态码: {}",
                status.as_u16()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::Config(format!("读取 WebDAV 响应失败: {e}")))?;

        log::info!("下载了 {} 字节", bytes.len());

        // 写入本地文件
        tokio::fs::write(local_path, bytes)
            .await
            .map_err(|e| AppError::io(local_path, e))?;

        log::info!("文件已写入本地: {:?}", local_path);
        Ok(())
    }

    /// 列出 WebDAV 服务器上的文件
    pub async fn list_files(&self) -> Result<Vec<String>, AppError> {
        let list_path = self.get_list_path();
        log::info!("列出 WebDAV 文件，URL: {}", list_path);

        // 尝试列出文件,如果目录不存在则返回空列表
        let items = match self.client.list(&list_path).await {
            Ok(items) => items,
            Err(e) => {
                log::warn!("列出 WebDAV 文件失败(可能目录不存在): {}", e);
                // 如果是目录不存在的错误,返回空列表而不是失败
                return Ok(Vec::new());
            }
        };

        log::info!("WebDAV 返回 {} 个项目", items.len());

        let mut files = Vec::new();
        for item in items {
            log::info!("WebDAV 项目: name={}, is_dir={}", item.name, item.is_dir);
            // 跳过目录
            if !item.is_dir {
                files.push(item.name);
            }
        }

        log::info!("过滤后的文件列表: {:?}", files);
        Ok(files)
    }

    /// 删除 WebDAV 服务器上的文件
    pub async fn delete_file(&self, remote_filename: &str) -> Result<(), AppError> {
        let remote_url = self.build_remote_url(remote_filename);

        // 使用 reqwest 删除文件
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AppError::Config(format!("创建 HTTP 客户端失败: {e}")))?;

        client
            .delete(&remote_url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .send()
            .await
            .map_err(|e| AppError::Config(format!("删除 WebDAV 文件失败: {e}")))?;

        Ok(())
    }

    /// 清理旧备份（保留最新的 N 个）
    #[allow(dead_code)]
    pub async fn cleanup_old_backups(&self, keep_count: usize) -> Result<(), AppError> {
        let mut files = self.list_files().await?;

        // 按文件名排序（假设文件名包含时间戳）
        files.sort();
        files.reverse();

        if files.len() <= keep_count {
            return Ok(());
        }

        // 删除超出保留数量的文件
        for file in files.iter().skip(keep_count) {
            if let Err(e) = self.delete_file(file).await {
                log::warn!("删除旧备份文件失败 {}: {}", file, e);
            }
        }

        Ok(())
    }
}

/// 导出配置到 WebDAV
pub async fn export_to_webdav(
    db_path: &PathBuf,
    config: &WebDavConfig,
    filename: &str,
) -> Result<(), AppError> {
    let client = WebDavClient::new(config.clone())?;

    // 测试连接
    client.test_connection().await?;

    // 创建临时 SQL 文件
    let temp_sql_path = db_path.parent()
        .ok_or_else(|| AppError::Config("无效的数据库路径".to_string()))?
        .join(format!("temp_{}", filename));

    log::info!("创建临时 SQL 文件: {:?}", temp_sql_path);

    // 打开数据库连接并导出 SQL
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| AppError::Database(e.to_string()))?;
    let db = crate::database::Database {
        conn: std::sync::Mutex::new(conn),
    };

    // 导出为 SQL 文本文件
    db.export_sql(&temp_sql_path)
        .map_err(|e| AppError::Config(format!("导出 SQL 失败: {e}")))?;

    log::info!("SQL 导出成功，大小: {} 字节", temp_sql_path.metadata().map(|m| m.len()).unwrap_or(0));

    // 上传 SQL 文件
    client.upload_file(&temp_sql_path, filename).await?;

    // 清理临时文件
    std::fs::remove_file(&temp_sql_path)
        .map_err(|e| AppError::io(&temp_sql_path, e))?;

    log::info!("SQL 文件上传完成，临时文件已清理");

    Ok(())
}

/// 从 WebDAV 导入配置
pub async fn import_from_webdav(
    local_path: &PathBuf,
    config: &WebDavConfig,
    remote_filename: &str,
) -> Result<(), AppError> {
    let client = WebDavClient::new(config.clone())?;

    // 测试连接
    client.test_connection().await?;

    // 下载文件
    client.download_file(remote_filename, local_path).await?;

    Ok(())
}

/// 生成带时间戳的备份文件名
pub fn generate_backup_filename(prefix: &str) -> String {
    let now = chrono::Utc::now();
    format!(
        "{}-{}.sql",
        prefix,
        now.format("%Y%m%d_%H%M%S")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_backup_filename() {
        let filename = generate_backup_filename("cc-switch-export");
        assert!(filename.starts_with("cc-switch-export-"));
        assert!(filename.ends_with(".sql"));
        assert!(filename.len() > 20);
    }
}

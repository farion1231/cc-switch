use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::Engine;
use once_cell::sync::OnceCell;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;

use crate::error::AppError;

const IDEA_LOGIN_URL: &str =
    "https://rnd-idea-api.huawei.com/ideaclientservice/login/v4/secureLogin";
const TOKEN_BUFFER_SECS: u64 = 300;

const ENCRYPTION_SALT: &str = "cc-switch-idea-token-v1";

pub fn derive_encryption_key() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("NAME"))
        .unwrap_or_default();
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    let machine_id = format!("{}:{}", hostname, username);
    let mut hasher = Sha256::new();
    hasher.update(machine_id.as_bytes());
    hasher.update(ENCRYPTION_SALT.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

pub fn encrypt_password(password: &str) -> Result<String, AppError> {
    let key = derive_encryption_key();
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| AppError::Message(format!("创建加密器失败: {}", e)))?;
    let nonce_bytes = random_nonce();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| AppError::Message(format!("加密失败: {}", e)))?;
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(base64::engine::general_purpose::STANDARD.encode(&combined))
}

pub fn decrypt_password(encrypted: &str) -> Result<String, AppError> {
    let key = derive_encryption_key();
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| AppError::Message(format!("Base64 解码失败: {}", e)))?;
    if combined.len() < 12 {
        return Err(AppError::Message("加密数据格式无效".to_string()));
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| AppError::Message(format!("创建解密器失败: {}", e)))?;
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Message("解密失败（可能密钥不匹配）".to_string()))?;
    String::from_utf8(plaintext).map_err(|e| AppError::Message(format!("UTF-8 解码失败: {}", e)))
}

/// Generate a fresh 12-byte AES-GCM nonce from a cryptographically-secure
/// random source (`OsRng`).
///
/// Aes256Gcm 的 nonce 不必保密，但同一密钥下每次加密必须唯一 ——
/// 否则 GCM 的安全性会彻底崩塌（重用 nonce 会泄露明文的异或）。
/// 此前实现从 `SystemTime::now().as_nanos()` 派生 nonce，而 Windows
/// 时钟分辨率约 15ms，相邻两次加密（如登录后立即刷新）会生成完全
/// 相同的 nonce，导致 GCM 唯一性不变量被破坏、明文 XOR 泄露。
fn random_nonce() -> [u8; 12] {
    use rand::RngCore;
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    nonce
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HuaweiCodeAgentToken {
    pub account_id: String,
    pub user: String,
    pub token: String,
    pub expires_at: i64,
    #[serde(default)]
    pub encrypted_password: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HuaweiCodeAgentAuthStatus {
    pub authenticated: bool,
    pub user: Option<String>,
    pub expires_at: Option<i64>,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HuaweiCodeAgentAccount {
    pub id: String,
    pub user: String,
    pub authenticated_at: i64,
    pub is_default: bool,
    #[serde(default)]
    pub requires_reauth: bool,
}

pub struct HuaweiCodeAgentAuthManager {
    config_dir: PathBuf,
    tokens: RwLock<HashMap<String, HuaweiCodeAgentToken>>,
    client: OnceCell<Client>,
    refreshing: AtomicBool,
}

impl HuaweiCodeAgentAuthManager {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            tokens: RwLock::new(HashMap::new()),
            client: OnceCell::new(),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn get_client(&self) -> &Client {
        self.client.get_or_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(30))
                .danger_accept_invalid_certs(true)
                .no_proxy()
                .build()
                .expect("failed to build reqwest client for IDEA auth")
        })
    }

    pub async fn login(
        &self,
        user: &str,
        password: &str,
    ) -> Result<HuaweiCodeAgentToken, AppError> {
        let client = self.get_client();
        let body = serde_json::json!({
            "requireUserInfo": true,
            "user": user,
            "password": password,
        });

        let resp = client
            .post(IDEA_LOGIN_URL)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Message(format!("IDEA 登录请求失败: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Message(format!(
                "IDEA 登录失败 (HTTP {}): {}",
                status,
                text.chars().take(200).collect::<String>()
            )));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Message(format!("IDEA 登录响应解析失败: {}", e)))?;

        let token_str = json
            .pointer("/cloudDragonTokens/authToken")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Message("IDEA 登录响应中未找到 authToken".to_string()))?;

        let expires_in = json
            .pointer("/cloudDragonTokens/expiresIn")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let account_id = format!("idea_{}", user.replace('@', "_at_"));
        let encrypted_pwd = encrypt_password(password).ok();

        let is_first_account = {
            let tokens = self.tokens.read().await;
            tokens.is_empty()
        };

        let token = HuaweiCodeAgentToken {
            account_id: account_id.clone(),
            user: user.to_string(),
            token: token_str.to_string(),
            expires_at: now + expires_in as i64,
            encrypted_password: encrypted_pwd,
            is_default: is_first_account,
        };

        self.save_token(&token).await?;
        self.persist_tokens().await?;

        Ok(token)
    }

    pub async fn get_token_with_refresh(
        self: &Arc<Self>,
        account_id: &str,
    ) -> Result<String, AppError> {
        let tokens = self.tokens.read().await;
        let token = tokens
            .get(account_id)
            .ok_or_else(|| AppError::Message(format!("未找到 IDEA 账号: {}", account_id)))?
            .clone();
        drop(tokens);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let remaining = token.expires_at - now;

        if remaining > TOKEN_BUFFER_SECS as i64 {
            return Ok(token.token.clone());
        }

        if remaining > 0 {
            if !self.refreshing.load(Ordering::Relaxed) {
                self.start_async_refresh(account_id);
            }
            return Ok(token.token.clone());
        }

        self.sync_refresh(account_id).await
    }

    fn start_async_refresh(self: &Arc<Self>, account_id: &str) {
        if self.refreshing.swap(true, Ordering::Relaxed) {
            return;
        }
        let account_id = account_id.to_string();
        let manager = Arc::clone(self);

        tokio::spawn(async move {
            let result = async {
                let tokens = manager.tokens.read().await;
                let token = tokens
                    .get(&account_id)
                    .ok_or_else(|| AppError::Message(format!("未找到 IDEA 账号: {}", account_id)))?
                    .clone();
                drop(tokens);

                let password =
                    decrypt_password(token.encrypted_password.as_deref().ok_or_else(|| {
                        AppError::Message("无加密密码，无法自动刷新".to_string())
                    })?)?;
                manager.login(&token.user, &password).await
            }
            .await;
            manager.refreshing.store(false, Ordering::Relaxed);
            let _ = result;
        });
    }

    async fn sync_refresh(&self, account_id: &str) -> Result<String, AppError> {
        if self.refreshing.swap(true, Ordering::Relaxed) {
            let tokens = self.tokens.read().await;
            let token = tokens
                .get(account_id)
                .ok_or_else(|| AppError::Message(format!("未找到 IDEA 账号: {}", account_id)))?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            if token.expires_at > now {
                return Ok(token.token.clone());
            }
            return Err(AppError::Message(
                "IDEA Token 正在刷新中，请稍后重试".to_string(),
            ));
        }

        let tokens = self.tokens.read().await;
        let token = tokens
            .get(account_id)
            .ok_or_else(|| {
                self.refreshing.store(false, Ordering::Relaxed);
                AppError::Message(format!("未找到 IDEA 账号: {}", account_id))
            })?
            .clone();
        drop(tokens);

        let encrypted_password = token.encrypted_password.clone();
        let user = token.user.clone();

        let password = match encrypted_password {
            Some(ref enc) => decrypt_password(enc).map_err(|e| {
                self.refreshing.store(false, Ordering::Relaxed);
                e
            })?,
            None => {
                self.refreshing.store(false, Ordering::Relaxed);
                return Err(AppError::Message(
                    "IDEA Token 已过期且无存储密码，请重新登录".to_string(),
                ));
            }
        };

        let new_token = self.login(&user, &password).await.map_err(|e| {
            self.refreshing.store(false, Ordering::Relaxed);
            e
        })?;

        self.refreshing.store(false, Ordering::Relaxed);
        Ok(new_token.token.clone())
    }

    pub async fn refresh_token(&self, account_id: &str) -> Result<HuaweiCodeAgentToken, AppError> {
        self.sync_refresh(account_id).await?;
        let tokens = self.tokens.read().await;
        tokens
            .get(account_id)
            .cloned()
            .ok_or_else(|| AppError::Message(format!("未找到 IDEA 账号: {}", account_id)))
    }

    pub async fn get_status(&self) -> HuaweiCodeAgentAuthStatus {
        let tokens = self.tokens.read().await;
        let default = tokens
            .values()
            .find(|t| t.is_default)
            .or_else(|| tokens.values().find(|t| t.account_id.starts_with("idea_")));
        match default {
            Some(token) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                HuaweiCodeAgentAuthStatus {
                    authenticated: token.expires_at > now,
                    user: Some(token.user.clone()),
                    expires_at: Some(token.expires_at),
                    account_id: Some(token.account_id.clone()),
                }
            }
            None => HuaweiCodeAgentAuthStatus {
                authenticated: false,
                user: None,
                expires_at: None,
                account_id: None,
            },
        }
    }

    pub async fn logout(&self) -> Result<(), AppError> {
        let mut tokens = self.tokens.write().await;
        tokens.clear();
        drop(tokens);
        self.persist_tokens().await?;
        Ok(())
    }

    pub async fn list_accounts(&self) -> Vec<HuaweiCodeAgentAccount> {
        let tokens = self.tokens.read().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        tokens
            .values()
            .map(|t| HuaweiCodeAgentAccount {
                id: t.account_id.clone(),
                user: t.user.clone(),
                authenticated_at: t.expires_at - 3600,
                is_default: t.is_default,
                requires_reauth: if t.expires_at <= now {
                    t.encrypted_password.is_none()
                } else {
                    false
                },
            })
            .filter(|a| a.authenticated_at > 0)
            .collect()
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), AppError> {
        let mut tokens = self.tokens.write().await;
        tokens.remove(account_id);
        drop(tokens);
        self.persist_tokens().await?;
        Ok(())
    }

    pub async fn set_default_account(&self, account_id: &str) -> Result<(), AppError> {
        let mut tokens = self.tokens.write().await;
        for token in tokens.values_mut() {
            token.is_default = token.account_id == account_id;
        }
        drop(tokens);
        self.persist_tokens().await?;
        Ok(())
    }

    async fn save_token(&self, token: &HuaweiCodeAgentToken) -> Result<(), AppError> {
        let mut tokens = self.tokens.write().await;
        tokens.insert(token.account_id.clone(), token.clone());
        Ok(())
    }

    async fn persist_tokens(&self) -> Result<(), AppError> {
        let tokens = self.tokens.read().await;
        let path = self.config_dir.join("huawei_codeagent_2_tokens.json");
        let json = serde_json::to_string_pretty(&*tokens)
            .map_err(|e| AppError::Message(format!("序列化 W3 Token 失败: {}", e)))?;
        tokio::fs::write(&path, json)
            .await
            .map_err(|e| AppError::IoContext {
                context: "写入 IDEA Token 文件".to_string(),
                source: e,
            })?;
        Ok(())
    }

    pub async fn load_tokens(&self) -> Result<(), AppError> {
        let path = self.resolve_token_path();
        let Some(path) = path else {
            return Ok(());
        };
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::IoContext {
                context: "读取 W3 Token 文件".to_string(),
                source: e,
            })?;
        let map: HashMap<String, HuaweiCodeAgentToken> = serde_json::from_str(&content)
            .map_err(|e| AppError::Message(format!("解析 W3 Token 文件失败: {}", e)))?;
        let mut tokens = self.tokens.write().await;
        *tokens = map;
        Ok(())
    }

    pub fn load_tokens_sync(&self) -> Result<(), AppError> {
        let path = self.resolve_token_path();
        let Some(path) = path else {
            return Ok(());
        };
        let content = std::fs::read_to_string(&path).map_err(|e| AppError::IoContext {
            context: "读取 W3 Token 文件".to_string(),
            source: e,
        })?;
        let map: HashMap<String, HuaweiCodeAgentToken> = serde_json::from_str(&content)
            .map_err(|e| AppError::Message(format!("解析 W3 Token 文件失败: {}", e)))?;
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| AppError::Message(format!("创建 Tokio runtime 失败: {}", e)))?;
        let mut tokens = rt.block_on(self.tokens.write());
        *tokens = map;
        Ok(())
    }

    fn resolve_token_path(&self) -> Option<std::path::PathBuf> {
        let path = self.config_dir.join("huawei_codeagent_2_tokens.json");
        if path.exists() {
            return Some(path);
        }
        let legacy_w3_path = self.config_dir.join("huawei_w3_tokens.json");
        if legacy_w3_path.exists() {
            return Some(legacy_w3_path);
        }
        let legacy_idea_path = self.config_dir.join("huawei_idea_tokens.json");
        if legacy_idea_path.exists() {
            return Some(legacy_idea_path);
        }
        None
    }
}

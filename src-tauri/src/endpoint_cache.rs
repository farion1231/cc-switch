use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

/// 端点缓存条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointCacheEntry {
    /// 成功的端点URL
    pub endpoint: String,
    /// 使用的认证变体索引
    pub auth_variant_index: usize,
    /// HTTP方法 (GET, POST, HEAD)
    pub http_method: String,
    /// 最后成功时间戳
    pub last_success_timestamp: u64,
    /// 成功次数
    pub success_count: u32,
    /// 平均延迟(毫秒)
    pub avg_latency_ms: u128,
}

/// 端点缓存管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointCache {
    /// 缓存数据: provider_id -> base_url -> EndpointCacheEntry
    cache: HashMap<String, HashMap<String, EndpointCacheEntry>>,
}

impl EndpointCache {
    /// 创建新的缓存实例
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// 从文件加载缓存
    pub fn load_from_file(path: &PathBuf) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let content = fs::read_to_string(path)
            .map_err(|e| format!("读取缓存文件失败: {}", e))?;

        serde_json::from_str(&content)
            .map_err(|e| format!("解析缓存文件失败: {}", e))
    }

    /// 保存缓存到文件
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), String> {
        // 确保父目录存在
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建缓存目录失败: {}", e))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化缓存失败: {}", e))?;

        fs::write(path, content)
            .map_err(|e| format!("写入缓存文件失败: {}", e))
    }

    /// 获取缓存的成功端点
    pub fn get_cached_endpoint(&self, provider_id: &str, base_url: &str) -> Option<&EndpointCacheEntry> {
        self.cache
            .get(provider_id)
            .and_then(|provider_cache| provider_cache.get(base_url))
    }

    /// 记录成功的端点
    pub fn record_success(
        &mut self,
        provider_id: &str,
        base_url: &str,
        endpoint: String,
        auth_variant_index: usize,
        http_method: String,
        latency_ms: u128,
    ) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let provider_cache = self.cache
            .entry(provider_id.to_string())
            .or_insert_with(HashMap::new);

        if let Some(entry) = provider_cache.get_mut(base_url) {
            // 更新现有条目
            entry.last_success_timestamp = timestamp;
            entry.success_count += 1;
            // 计算移动平均延迟
            entry.avg_latency_ms = (entry.avg_latency_ms * (entry.success_count - 1) as u128 + latency_ms) 
                / entry.success_count as u128;
        } else {
            // 创建新条目
            provider_cache.insert(
                base_url.to_string(),
                EndpointCacheEntry {
                    endpoint,
                    auth_variant_index,
                    http_method,
                    last_success_timestamp: timestamp,
                    success_count: 1,
                    avg_latency_ms: latency_ms,
                },
            );
        }
    }

    /// 清除过期的缓存条目（超过30天未使用）
    #[allow(dead_code)]
    pub fn cleanup_expired(&mut self, max_age_days: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let max_age_secs = max_age_days * 24 * 60 * 60;

        for provider_cache in self.cache.values_mut() {
            provider_cache.retain(|_, entry| {
                now - entry.last_success_timestamp < max_age_secs
            });
        }

        // 移除空的 provider 缓存
        self.cache.retain(|_, provider_cache| !provider_cache.is_empty());
    }

    /// 获取缓存统计信息
    #[allow(dead_code)]
    pub fn get_stats(&self) -> CacheStats {
        let total_providers = self.cache.len();
        let total_endpoints: usize = self.cache.values().map(|p| p.len()).sum();
        
        CacheStats {
            total_providers,
            total_endpoints,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct CacheStats {
    pub total_providers: usize,
    pub total_endpoints: usize,
}

impl Default for EndpointCache {
    fn default() -> Self {
        Self::new()
    }
}

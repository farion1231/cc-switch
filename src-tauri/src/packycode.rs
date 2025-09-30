use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackyCodeEndpoint {
    pub name: String,
    pub url: String,
    pub latency: Option<u128>, // 延迟（毫秒）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackyCodeService {
    pub name: String,
    pub endpoints: Vec<PackyCodeEndpoint>,
}

/// 获取所有 PackyCode 服务配置
pub fn get_packycode_services() -> Vec<PackyCodeService> {
    vec![
        PackyCodeService {
            name: "滴滴车".to_string(),
            endpoints: vec![
                PackyCodeEndpoint {
                    name: "默认节点".to_string(),
                    url: "https://share-api.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "香港CN2".to_string(),
                    url: "https://share-api-hk-cn2.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "香港G口".to_string(),
                    url: "https://share-api-hk-g.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "美国CN2".to_string(),
                    url: "https://share-api-us-cn2.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "Cloudflare Pro".to_string(),
                    url: "https://share-api-cf-pro.packycode.com".to_string(),
                    latency: None,
                },
            ],
        },
        PackyCodeService {
            name: "公交车".to_string(),
            endpoints: vec![
                PackyCodeEndpoint {
                    name: "默认节点".to_string(),
                    url: "https://api.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "香港CN2".to_string(),
                    url: "https://api-hk-cn2.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "香港G口".to_string(),
                    url: "https://api-hk-g.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "美国CN2".to_string(),
                    url: "https://api-us-cn2.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "Cloudflare Pro".to_string(),
                    url: "https://api-cf-pro.packycode.com".to_string(),
                    latency: None,
                },
            ],
        },
        PackyCodeService {
            name: "Codex".to_string(),
            endpoints: vec![
                PackyCodeEndpoint {
                    name: "默认节点".to_string(),
                    url: "https://codex-api.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "香港CN2".to_string(),
                    url: "https://codex-api-hk-cn2.packycode.com".to_string(),
                    latency: None,
                },
                PackyCodeEndpoint {
                    name: "香港CDN".to_string(),
                    url: "https://codex-api-hk-cdn.packycode.com".to_string(),
                    latency: None,
                },
            ],
        },
    ]
}

/// 测试单个端点的延迟（使用 HTTP HEAD 请求）
pub async fn test_endpoint_latency(url: &str) -> Result<u128, String> {
    test_endpoint_latency_with_timeout(url, 10).await
}

/// 测试单个端点的延迟，支持自定义超时时间
pub async fn test_endpoint_latency_with_timeout(url: &str, timeout_secs: u64) -> Result<u128, String> {
    let start = Instant::now();
    
    // 创建 HTTP 客户端，设置超时时间（默认10秒，放宽阈值）
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    
    // 发送 HEAD 请求测试连通性
    match client.head(url).send().await {
        Ok(_response) => {
            let elapsed = start.elapsed().as_millis();
            Ok(elapsed)
        }
        Err(e) => {
            if e.is_timeout() {
                Err("请求超时".to_string())
            } else if e.is_connect() {
                Err("连接失败".to_string())
            } else {
                Err(format!("请求失败: {}", e))
            }
        }
    }
}

/// 为指定服务选择最佳端点（并发测速）
pub async fn select_best_endpoint(service_name: &str) -> Result<PackyCodeEndpoint, String> {
    let services = get_packycode_services();
    
    let service = services
        .into_iter()
        .find(|s| s.name == service_name)
        .ok_or_else(|| format!("未找到服务: {}", service_name))?;
    
    // 并发测试所有端点
    let mut tasks = Vec::new();
    for endpoint in service.endpoints {
        let url = endpoint.url.clone();
        tasks.push(async move {
            let latency = test_endpoint_latency(&url).await.ok();
            PackyCodeEndpoint {
                name: endpoint.name,
                url: endpoint.url,
                latency,
            }
        });
    }
    
    // 等待所有任务完成
    let results = futures::future::join_all(tasks).await;
    
    // 找出延迟最低的端点
    let mut best_endpoint: Option<PackyCodeEndpoint> = None;
    let mut min_latency = u128::MAX;
    
    for endpoint in results {
        if let Some(latency) = endpoint.latency {
            if latency < min_latency {
                min_latency = latency;
                best_endpoint = Some(endpoint);
            }
        }
    }
    
    best_endpoint.ok_or_else(|| "所有端点都不可用".to_string())
}

/// 批量测试端点（并发）
pub async fn test_endpoints_batch(urls: Vec<String>) -> Vec<(String, Option<u128>)> {
    let tasks = urls.into_iter().map(|url| {
        let url_clone = url.clone();
        async move {
            let latency = test_endpoint_latency(&url_clone).await.ok();
            (url, latency)
        }
    });
    
    futures::future::join_all(tasks).await
}
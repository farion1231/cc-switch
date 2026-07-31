//! 主动健康检查器
//!
//! 在 [`crate::proxy::circuit_breaker::CircuitBreaker`] 进入 Open 状态后，
//! 不再被动等待 `timeout_seconds` + 真实流量触发 HalfOpen 探测，而是由本模块
//! 后台主动调用 Provider 的 `/v1/models` 端点探测可达性，可达时立即
//! [`crate::proxy::circuit_breaker::CircuitBreaker::force_half_open`]，
//! 让下一次真实请求直接走恢复路径，显著缩短故障 provider 的恢复延迟。

use crate::app_config::AppType;
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::CircuitState;
use crate::proxy::log_codes::cb as log_cb;
use crate::proxy::provider_router::ProviderRouter;
use reqwest::Client;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

/// 主动探测循环间隔（秒）
///
/// 选 30s 是健康检查的常见业界值：足够频繁以快速发现恢复，
/// 又不会对上游造成明显压力（每次仅 GET /v1/models）。
const PROBE_INTERVAL_SECS: u64 = 30;

/// 单次探测超时
///
/// 健康探测不应长时间占用连接；3s 已足够区分"可达但鉴权失败"与"完全不可达"。
const PROBE_TIMEOUT_SECS: u64 = 3;

/// 健康检查器
///
/// 持有共享的 [`ProviderRouter`]（用于读取熔断状态、触发 `force_half_open`）
/// 与 [`Database`]（用于读取每个 app 的故障转移开关）。`reqwest::Client` 由调用方注入，
/// 生产环境复用 [`crate::proxy::http_client::get()`]（继承全局代理设置），
/// 测试时可用独立客户端避免污染全局状态。
pub struct HealthChecker {
    router: Arc<ProviderRouter>,
    db: Arc<Database>,
    client: Client,
}

impl HealthChecker {
    pub fn new(router: Arc<ProviderRouter>, db: Arc<Database>, client: Client) -> Self {
        Self { router, db, client }
    }

    /// 后台循环：按固定间隔扫描所有 app_type 的故障转移队列，
    /// 对处于 Open 状态的 provider 主动 probe，成功则 `force_half_open`。
    ///
    /// 通过 `oneshot::Receiver` 接收关闭信号，与 [`crate::proxy::server::ProxyServer`] 的
    /// 生命周期绑定：代理停止时发送信号，本循环立即退出。
    pub async fn run_loop(&self, mut shutdown: oneshot::Receiver<()>) {
        let mut interval = tokio::time::interval(Duration::from_secs(PROBE_INTERVAL_SECS));
        // 第一次 tick 立即返回（启动后先扫一次，避免等到下一个周期才发现恢复）
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.scan_once().await;
                }
                _ = &mut shutdown => {
                    log::info!("[{}] 健康检查器已停止", log_cb::OPEN_TO_HALF_OPEN);
                    break;
                }
            }
        }
    }

    /// 单次扫描：遍历所有 app_type，对 Open 状态的 provider 主动 probe。
    ///
    /// 仅扫描启用自动故障转移的 app（未启用故障转移的 app 不需要主动恢复——
    /// 单 provider 场景下半开也无意义）。`pub(crate)` 暴露以便单元测试直接调用，
    /// 避开 30s 间隔的等待。
    pub(crate) async fn scan_once(&self) {
        for app_type in AppType::all() {
            let app_type_str = app_type.as_str();

            // 只对启用自动故障转移的 app 检查
            let auto_failover_enabled = match self.db.get_proxy_config_for_app(app_type_str).await {
                Ok(config) => config.auto_failover_enabled,
                Err(_) => continue,
            };
            if !auto_failover_enabled {
                continue;
            }

            let providers = match self.router.list_providers_with_state(app_type_str).await {
                Ok(list) => list,
                Err(e) => {
                    log::debug!("[Health] 读取 {app_type_str} provider 列表失败: {e}");
                    continue;
                }
            };

            for (provider, state) in providers {
                if state != CircuitState::Open {
                    continue;
                }
                if self.probe(&provider, app_type_str).await {
                    let circuit_key = format!("{app_type_str}:{}", provider.id);
                    if let Some(breaker) = self.router.get_breaker(&circuit_key).await {
                        breaker.force_half_open().await;
                    }
                }
            }
        }
    }

    /// 探测单个 provider 是否可达
    ///
    /// 端点选择策略：
    /// - `base_url` 以 `/v1` 结尾：直接附加 `/models`（Codex/OpenAI 风格的 base_url 已含版本）
    /// - 否则：附加 `/v1/models`（Anthropic/Gemini 风格的 base_url 仅含 origin）
    ///
    /// 可达判定：
    /// - 2xx：正常可达
    /// - 401/403：鉴权失败但**网络层可达**——熔断目的是隔离网络不可达，而非认证错误；
    ///   这种情况下 provider 实际可服务（用户重试时鉴权错会被上层处理），视为可达
    /// - 其他状态码 / 网络错误：不可达
    async fn probe(&self, provider: &Provider, app_type: &str) -> bool {
        let app_type_enum = match AppType::from_str(app_type) {
            Ok(t) => t,
            Err(_) => return false,
        };
        let (base_url, api_key) = provider.resolve_usage_credentials(&app_type_enum);
        if base_url.is_empty() {
            return false;
        }

        let url = if base_url.ends_with("/v1") {
            format!("{base_url}/models")
        } else {
            format!("{base_url}/v1/models")
        };

        let mut req = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS));
        if !api_key.is_empty() {
            req = req.bearer_auth(&api_key);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                // 熔断器隔离的是"网络不可达/服务宕机"，而非"端点语义错误"。
                // 任何 < 500 的 HTTP 响应（含 400 鉴权/参数错、404 端点未实现、405 方法不允许）
                // 都说明网络层与应用层在正常处理请求 → 视为可达，放行真实流量探测恢复；
                // 仅 5xx（服务端自身故障/网关错误）视为不可达。日志用 info 级确保默认可见。
                let reachable = status < 500;
                if reachable {
                    log::info!(
                        "[Health] {app_type}/{} probe 可达 (HTTP {status})",
                        provider.id
                    );
                } else {
                    log::info!(
                        "[Health] {app_type}/{} probe 不可达 (HTTP {status})",
                        provider.id
                    );
                }
                reachable
            }
            Err(e) => {
                log::info!("[Health] {app_type}/{} probe 失败: {e}", provider.id);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::circuit_breaker::CircuitBreakerConfig;
    use crate::proxy::provider_router::ProviderRouter;
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use tempfile::TempDir;

    /// 复用 provider_router 测试的 TempHome 模式：隔离 HOME / CC_SWITCH_TEST_HOME，
    /// 避免 settings 全局状态串扰。
    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    /// 起一个只服务一次连接的本地 HTTP server，返回固定状态码 + 空 body。
    /// 返回 `(base_url, JoinHandle)`，base_url 已去掉 trailing slash 方便测试用例拼接。
    fn spawn_once_server(status_code: u16) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
        let port = listener.local_addr().expect("local addr").port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {status_code} OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                );
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    /// 构造独立的 reqwest Client（不依赖全局 http_client::init）。
    fn test_client() -> Client {
        Client::builder()
            .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
            .build()
            .expect("build test client")
    }

    fn claude_provider_with(base_url: &str) -> Provider {
        Provider::with_id(
            "p1".to_string(),
            "P".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": "sk-test",
                }
            }),
            None,
        )
    }

    #[tokio::test]
    async fn probe_returns_true_on_200() {
        let _home = TempHome::new();
        let (base_url, handle) = spawn_once_server(200);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&base_url);
        let reachable = checker.probe(&provider, "claude").await;
        assert!(reachable);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn probe_returns_true_on_401() {
        let _home = TempHome::new();
        // 401 表示鉴权失败但网络可达，视为 reachable（用户重试时由上层处理鉴权）
        let (base_url, handle) = spawn_once_server(401);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&base_url);
        let reachable = checker.probe(&provider, "claude").await;
        assert!(reachable);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn probe_returns_true_on_403() {
        let _home = TempHome::new();
        let (base_url, handle) = spawn_once_server(403);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&base_url);
        let reachable = checker.probe(&provider, "claude").await;
        assert!(reachable);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn probe_returns_false_on_500() {
        let _home = TempHome::new();
        let (base_url, handle) = spawn_once_server(500);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&base_url);
        let reachable = checker.probe(&provider, "claude").await;
        assert!(!reachable);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn probe_returns_true_on_400_anthropic_style_bad_request() {
        // Anthropic 端点用 Bearer 而非 x-api-key → 400；网络可达，鉴权交给真实流量
        let _home = TempHome::new();
        let (base_url, handle) = spawn_once_server(400);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&base_url);
        let reachable = checker.probe(&provider, "claude").await;
        assert!(reachable);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn probe_returns_true_on_404_endpoint_not_implemented() {
        // 第三方中转常未实现 GET /v1/models → 404；网络层可达即应放行恢复
        let _home = TempHome::new();
        let (base_url, handle) = spawn_once_server(404);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&base_url);
        let reachable = checker.probe(&provider, "claude").await;
        assert!(reachable);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn probe_returns_true_on_405_method_not_allowed() {
        // 端点存在但不允许 GET → 405；应用层在响应，视为可达
        let _home = TempHome::new();
        let (base_url, handle) = spawn_once_server(405);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&base_url);
        let reachable = checker.probe(&provider, "claude").await;
        assert!(reachable);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn probe_returns_false_on_503_service_unavailable() {
        // 5xx = 服务端自身故障/网关错误，不可达（与 500 一致）
        let _home = TempHome::new();
        let (base_url, handle) = spawn_once_server(503);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&base_url);
        let reachable = checker.probe(&provider, "claude").await;
        assert!(!reachable);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn probe_returns_false_when_connection_refused() {
        let _home = TempHome::new();
        // 绑定后立刻释放端口 → 连接被拒
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);

        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router.clone(), db, test_client());

        let provider = claude_provider_with(&format!("http://127.0.0.1:{port}"));
        let reachable = checker.probe(&provider, "claude").await;
        assert!(!reachable);
    }

    #[tokio::test]
    #[serial]
    async fn scan_once_forces_half_open_on_successful_probe() {
        let _home = TempHome::new();
        let (base_url, handle) = spawn_once_server(200);

        let db = Arc::new(Database::memory().unwrap());
        // 1 次失败即熔断
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            // 长 timeout：证明 force_half_open 由主动探测触发，而非 timeout 到期
            timeout_seconds: 3600,
            ..Default::default()
        })
        .await
        .unwrap();

        let provider = claude_provider_with(&base_url);
        db.save_provider("claude", &provider).unwrap();
        db.add_to_failover_queue("claude", &provider.id).unwrap();

        // 启用自动故障转移
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = Arc::new(ProviderRouter::new(db.clone()));
        // 触发熔断：1 次失败即 Open
        router
            .record_result(
                &provider.id,
                "claude",
                false,
                false,
                Some("boom".to_string()),
            )
            .await
            .unwrap();

        let checker = HealthChecker::new(router.clone(), db, test_client());

        // 探测前：Open
        let circuit_key = format!("claude:{}", provider.id);
        let breaker = router
            .get_breaker(&circuit_key)
            .await
            .expect("breaker exists");
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        checker.scan_once().await;

        // 探测成功后：force_half_open → HalfOpen
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
        handle.join().expect("server thread");
    }

    #[tokio::test]
    #[serial]
    async fn scan_once_skips_apps_without_auto_failover() {
        let _home = TempHome::new();
        // 不需要 mock server：scan_once 会跳过 probe，因为没有启用 auto_failover
        let db = Arc::new(Database::memory().unwrap());
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 3600,
            ..Default::default()
        })
        .await
        .unwrap();

        let provider = claude_provider_with("http://127.0.0.1:1");
        db.save_provider("claude", &provider).unwrap();
        db.add_to_failover_queue("claude", &provider.id).unwrap();
        // 注意：不启用 auto_failover（默认即为 false）

        let router = Arc::new(ProviderRouter::new(db.clone()));
        router
            .record_result(
                &provider.id,
                "claude",
                false,
                false,
                Some("boom".to_string()),
            )
            .await
            .unwrap();

        let checker = HealthChecker::new(router.clone(), db, test_client());
        checker.scan_once().await;

        // 未启用故障转移 → 跳过，状态仍为 Open
        let circuit_key = format!("claude:{}", provider.id);
        let breaker = router
            .get_breaker(&circuit_key)
            .await
            .expect("breaker exists");
        assert_eq!(breaker.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn run_loop_exits_on_shutdown_signal() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let checker = HealthChecker::new(router, db, test_client());

        let (tx, rx) = oneshot::channel::<()>();
        // 后台 spawn run_loop，立即发送 shutdown 信号
        let task = tokio::spawn(async move { checker.run_loop(rx).await });

        // 给 run_loop 一个 tick 的机会启动 interval.tick()
        tokio::time::sleep(Duration::from_millis(50)).await;
        tx.send(()).expect("send shutdown");

        // run_loop 应在 1s 内退出
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("run_loop should exit within 1s")
            .expect("task should not panic");
    }
}

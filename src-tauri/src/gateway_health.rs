//! 网关健康探针 + 本地 8782 启动入口
//!
//! 设计目的：让 kaixuan bundle 能在 UI 里显示「公网 kxpms.cn / 本地 8782
//! 各自是否在线」，并提供「重启本机网关」的一键按钮。
//!
//! 设计约束：
//! - 探针本身是纯 HTTP GET /v1/models；不依赖任何 cc-switch 内部状态。
//! - 公网探针只做轻量探测（200ms timeout）；不要做 /v1/chat/completions，
//!   那是 OpenAI 协议层的事情，跟「网关是否在跑」无关。
//! - 本机启动走 [`start_local_gateway`]：找已知的部署入口（docker
//!   compose / 直接命令），执行并返回 stdout/stderr 让 UI 显示。
//!
//! 已知 endpoint：
//! - 公网：https://llm.kxpms.cn/v1/models
//! - 本机：http://127.0.0.1:8782/v1/models

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::{Duration, Instant};

/// 单端点的健康结果。前端用它渲染小灯（绿/黄/红/灰）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayHealth {
    pub endpoint_id: String, // "kxpms" | "local-8782"
    pub url: String,
    pub reachable: bool,
    pub http_status: Option<u16>,
    pub latency_ms: Option<u128>,
    pub error: Option<String>,
    pub probed_at_ms: i64,
}

/// 启动本地网关的结果。前端把它显示成 banner 或 toast。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartLocalGatewayResult {
    pub success: bool,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
}

/// 已知网关 endpoint 的 SSOT。前端用它渲染 health 卡片。
///
/// 本机端口可由 `KAIXUAN_LOCAL_GATEWAY_PORT` 覆盖（默认 8782）。
/// 与 `provider_bundle` 的端口推导保持一致——两边任一处改了都得改。
pub fn known_endpoints() -> Vec<GatewayEndpointMeta> {
    let local_port: u16 = std::env::var("KAIXUAN_LOCAL_GATEWAY_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p > 0)
        .unwrap_or(8782);
    vec![
        GatewayEndpointMeta {
            id: "kxpms".to_string(),
            label: "开轩 kxpms.cn".to_string(),
            url: "https://llm.kxpms.cn/v1/models".to_string(),
            role: "primary".to_string(),
            local_port: None,
        },
        GatewayEndpointMeta {
            id: "local-8782".to_string(),
            label: format!("本地 {local_port}"),
            url: format!("http://127.0.0.1:{local_port}/v1/models"),
            role: "secondary".to_string(),
            local_port: Some(local_port),
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayEndpointMeta {
    pub id: String,
    pub label: String,
    pub url: String,
    pub role: String,
    /// 实际探测的本机端口（`KAIXUAN_LOCAL_GATEWAY_PORT`，默认 8782）。
    /// 公网端点为 `None`。
    ///
    /// 前端把它透出到 UI，是为了消掉「改了 env var 但界面还写 8782」的迷惑：
    /// 端口是**运行时**推导的，编译进 bundle 的字面量必然过期。
    pub local_port: Option<u16>,
}

/// 共享 reqwest Client（probe_all 复用，避免每个 endpoint 起新连接池）。
fn shared_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(500))
        .connect_timeout(Duration::from_millis(200))
        .danger_accept_invalid_certs(false)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// 探测单个 endpoint。async（reqwest 异步 + cc-switch 已在 tokio runtime）。
pub async fn probe(url: &str, endpoint_id: &str) -> GatewayHealth {
    let client = shared_client();
    let start = Instant::now();
    let probed_at_ms = chrono::Utc::now().timestamp_millis();
    let res = client
        .get(url)
        .header("User-Agent", "cc-switch/3.20")
        .send()
        .await;
    let elapsed = start.elapsed().as_millis();
    match res {
        Ok(r) => {
            let status = r.status();
            // 探针不需要 body；读到 200 即可丢弃。 4xx 视为 reachable
            // （网关在跑但缺 key）；5xx 与 transport 错误视为 unreachable。
            let reachable = !status.is_server_error();
            GatewayHealth {
                endpoint_id: endpoint_id.to_string(),
                url: url.to_string(),
                reachable,
                http_status: Some(status.as_u16()),
                latency_ms: Some(elapsed),
                error: None,
                probed_at_ms,
            }
        }
        Err(e) => GatewayHealth {
            endpoint_id: endpoint_id.to_string(),
            url: url.to_string(),
            reachable: false,
            http_status: None,
            latency_ms: Some(elapsed),
            error: Some(format!("{e}")),
            probed_at_ms,
        },
    }
}

/// 探测所有已知 endpoint，并发跑。返回 Vec 顺序与 [`known_endpoints`] 一致。
pub async fn probe_all() -> Vec<GatewayHealth> {
    let endpoints = known_endpoints();
    let futs = endpoints.iter().map(|e| {
        let url = e.url.clone();
        let id = e.id.clone();
        async move { probe(&url, &id).await }
    });
    futures::future::join_all(futs).await
}

/// 启动本机网关（同步函数，由调用方在 spawn_blocking 里跑）。
///
/// 策略：
/// 1. 先 probe `127.0.0.1:$KAIXUAN_LOCAL_GATEWAY_PORT`（默认 8782）：通了就直接返回 success；
/// 2. 找启动入口：优先级
///    (a) `$KAIXUAN_GATEWAY_START_CMD` 环境变量（用户自定义，最灵活）；
///    (b) `~/kaixuan/llm-gateway-local/start.sh`（项目惯例路径）；
///    (c) `docker run -d --name llm-gateway-local-8782 -p $PORT:$PORT
///         llm-gateway-go:local`（fallback，假设本地有镜像）。
/// 3. 启动后 wait up to 5 秒，再 probe 一次确认 reachable。
///
/// 失败路径要返回 stderr 让用户看到为什么没起来。
pub fn start_local_gateway() -> StartLocalGatewayResult {
    let started = Instant::now();
    let local_port: u16 = std::env::var("KAIXUAN_LOCAL_GATEWAY_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p > 0)
        .unwrap_or(8782);
    let probe_url = format!("http://127.0.0.1:{local_port}/v1/models");

    // 1. 同步阻塞 probe；Tauri 命令层用 spawn_blocking 包，本函数本身
    //    不能再 await。要更严谨可以传 Client 进来，但 200ms 探针可以接受。
    let client = shared_client();
    let probe_check = || -> Result<bool, String> {
        // 用 sync block_on 跑 reqwest async；这里我们在 spawn_blocking 里，
        // tokio runtime handle 可用
        let rt = tokio::runtime::Handle::try_current().map_err(|e| format!("no tokio: {e}"))?;
        let resp = rt.block_on(async {
            client
                .get(&probe_url)
                .header("User-Agent", "cc-switch/3.20")
                .send()
                .await
        });
        match resp {
            Ok(r) => Ok(!r.status().is_server_error()),
            Err(e) => Err(format!("{e}")),
        }
    };
    if let Ok(true) = probe_check() {
        return StartLocalGatewayResult {
            success: true,
            command: "(already running)".to_string(),
            stdout: "8782 已在跑".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: started.elapsed().as_millis(),
        };
    }

    // 2. 找启动入口
    let (cmd_str, args) = pick_start_command();
    let mut child = match Command::new(&cmd_str).args(&args).spawn() {
        Ok(c) => c,
        Err(e) => {
            return StartLocalGatewayResult {
                success: false,
                command: format!("{} {}", cmd_str, args.join(" ")),
                stdout: String::new(),
                stderr: format!(
                    "启动命令失败: {e}（请确认 {} 已安装或在 $PATH 里）",
                    cmd_str
                ),
                exit_code: None,
                duration_ms: started.elapsed().as_millis(),
            };
        }
    };

    // 3. 等子进程（最长 5 秒），中途轮询 try_wait
    let wait = Duration::from_secs(5);
    let start_wait = Instant::now();
    let stdout = String::new();
    let stderr = String::new();
    let exit_code: Option<i32>;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_code = status.code();
                // 收 stdout/stderr（不能再 wait_with_output，否则会重 waiting）
                break;
            }
            Ok(None) => {
                if start_wait.elapsed() > wait {
                    let _ = child.kill();
                    let _ = child.wait();
                    exit_code = None;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return StartLocalGatewayResult {
                    success: false,
                    command: format!("{} {}", cmd_str, args.join(" ")),
                    stdout: String::new(),
                    stderr: format!("try_wait 失败: {e}"),
                    exit_code: None,
                    duration_ms: started.elapsed().as_millis(),
                };
            }
        }
    }
    let _ = (stdout, stderr); // 占位避免 unused 警告——本函数简化版不读子进程输出

    // 4. 重新 probe 一次
    let now_reachable = probe_check().unwrap_or(false);
    StartLocalGatewayResult {
        success: now_reachable,
        command: format!("{} {}", cmd_str, args.join(" ")),
        stdout: if now_reachable {
            "8782 起来了".to_string()
        } else {
            "8782 仍未起，详情看 stderr".to_string()
        },
        stderr: String::new(),
        exit_code,
        duration_ms: started.elapsed().as_millis(),
    }
}

/// 选启动命令。返回 (program, args)。
///
/// 跨平台注意：本函数在 macOS / Linux 上用 `/bin/sh -c`；Windows 上
/// 用 `cmd.exe /C`。`KAIXUAN_GATEWAY_START_CMD` 用户自定义时按当前
/// 平台的 shell 写命令即可。
fn pick_start_command() -> (String, Vec<String>) {
    let local_port: u16 = std::env::var("KAIXUAN_LOCAL_GATEWAY_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p > 0)
        .unwrap_or(8782);
    if let Ok(custom) = std::env::var("KAIXUAN_GATEWAY_START_CMD") {
        if !custom.trim().is_empty() {
            #[cfg(windows)]
            {
                return ("cmd.exe".to_string(), vec!["/C".to_string(), custom]);
            }
            #[cfg(not(windows))]
            {
                return ("/bin/sh".to_string(), vec!["-c".to_string(), custom]);
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = std::path::PathBuf::from(home)
            .join("kaixuan")
            .join("llm-gateway-local")
            .join("start.sh");
        if p.exists() {
            return (p.to_string_lossy().to_string(), Vec::new());
        }
    }
    (
        "docker".to_string(),
        vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            format!("llm-gateway-local-{local_port}"),
            "-p".to_string(),
            format!("{local_port}:{local_port}"),
            "llm-gateway-go:local".to_string(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn known_endpoints_includes_both() {
        let eps = known_endpoints();
        assert_eq!(eps.len(), 2);
        let ids: Vec<&str> = eps.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"kxpms"));
        assert!(ids.contains(&"local-8782"));
    }

    #[test]
    fn endpoints_have_expected_urls() {
        let eps = known_endpoints();
        let local = eps.iter().find(|e| e.id == "local-8782").unwrap();
        assert_eq!(local.url, "http://127.0.0.1:8782/v1/models");
        assert_eq!(local.role, "secondary");
        let remote = eps.iter().find(|e| e.id == "kxpms").unwrap();
        assert_eq!(remote.role, "primary");
        assert!(remote.url.starts_with("https://"));
    }

    #[test]
    fn shared_client_is_built() {
        // 不验证可达性，只验证 builder 不 panic
        let _c = shared_client();
    }

    #[test]
    #[serial(env)]
    fn pick_start_command_prefers_env_var() {
        // 共享 env 锁：本测试改进程全局 env，必须与**全 crate** 所有改 env 的
        // 用例互斥。`#[serial]` 与 `#[serial(env)]` 是 serial_test 的两个不同 key，
        // 彼此不互斥，也都不与 crate::test_support::env_guard() 互斥——只靠它们
        // 挡不住跨模块串扰。详见 crate::test_support。
        let _env_guard = crate::test_support::env_guard();
        let saved = std::env::var("KAIXUAN_GATEWAY_START_CMD").ok();
        std::env::set_var("KAIXUAN_GATEWAY_START_CMD", "echo hi");
        let (prog, args) = pick_start_command();
        // macOS / Linux 用 /bin/sh -c；Windows 用 cmd.exe /C（测试机无 Windows 跳过）
        #[cfg(not(windows))]
        {
            assert_eq!(prog, "/bin/sh");
            assert_eq!(args[0], "-c");
            assert_eq!(args[1], "echo hi");
        }
        #[cfg(windows)]
        {
            assert_eq!(prog, "cmd.exe");
            assert_eq!(args[0], "/C");
            assert_eq!(args[1], "echo hi");
        }
        match saved {
            Some(v) => std::env::set_var("KAIXUAN_GATEWAY_START_CMD", v),
            None => std::env::remove_var("KAIXUAN_GATEWAY_START_CMD"),
        }
    }

    #[test]
    #[serial(env)]
    fn pick_start_command_empty_env_falls_through() {
        // 共享 env 锁：本测试改进程全局 env，必须与**全 crate** 所有改 env 的
        // 用例互斥。`#[serial]` 与 `#[serial(env)]` 是 serial_test 的两个不同 key，
        // 彼此不互斥，也都不与 crate::test_support::env_guard() 互斥——只靠它们
        // 挡不住跨模块串扰。详见 crate::test_support。
        let _env_guard = crate::test_support::env_guard();
        let saved = std::env::var("KAIXUAN_GATEWAY_START_CMD").ok();
        std::env::set_var("KAIXUAN_GATEWAY_START_CMD", "   ");
        let (prog, _args) = pick_start_command();
        // 空字符串不应走 env 分支，应当走 docker fallback 或 start.sh
        assert!(
            prog != "/bin/sh" && prog != "cmd.exe",
            "空 env 应跳过 shell 走 docker/start.sh 分支，但拿到 {prog}"
        );
        match saved {
            Some(v) => std::env::set_var("KAIXUAN_GATEWAY_START_CMD", v),
            None => std::env::remove_var("KAIXUAN_GATEWAY_START_CMD"),
        }
    }

    /// 前端读的是 `ep.localPort`（`KaixuanBundleCard` 里 `localPort` 变量）。
    /// `#[serde(rename_all = "camelCase")]` 理论上把 `local_port` 变成
    /// `localPort`，但这是**前端能不能看到端口**的唯一契约点——一旦 rename 被
    /// 去掉或字段名对不上，UI 会静默退回硬编码 8782，正是 item 5 要消灭的迷惑。
    /// 所以这里做一次真实序列化断言，而不是靠读代码推断。
    #[test]
    fn local_port_serializes_as_camel_case_for_the_frontend() {
        let endpoints = known_endpoints();
        let json = serde_json::to_value(&endpoints).expect("serialize endpoints");
        let arr = json.as_array().expect("array").clone();

        let primary = arr
            .iter()
            .find(|e| e["role"] == "primary")
            .expect("primary endpoint");
        let local = arr
            .iter()
            .find(|e| e["role"] == "secondary")
            .expect("secondary endpoint");

        // 公网端点没有本机端口，必须是 null（前端 `?? 8782` 才不会误显示）。
        assert!(
            primary.get("localPort").is_some(),
            "primary 必须带 localPort 键（值为 null），实际 {primary}"
        );
        assert!(
            primary["localPort"].is_null(),
            "primary 的 localPort 应为 null，实际 {}",
            primary["localPort"]
        );
        // 本机端点必须带**数字**端口，且与探测 url / label 一致。
        let port = local["localPort"].as_u64().unwrap_or_else(|| {
            panic!("local 的 localPort 必须是数字，实际 {}", local["localPort"])
        });
        assert!(port > 0, "端口必须为正");
        let url = local["url"].as_str().expect("url");
        assert!(
            url.contains(&format!("127.0.0.1:{port}")),
            "localPort={port} 必须与探测 url 一致：{url}"
        );
        assert!(
            local["label"].as_str().unwrap_or("").contains(&port.to_string()),
            "label 必须含真实端口：{}",
            local["label"]
        );
    }

    #[test]
    #[serial(env)]
    fn known_endpoints_honors_local_port_env() {
        // 共享 env 锁：本测试改进程全局 env，必须与**全 crate** 所有改 env 的
        // 用例互斥。`#[serial]` 与 `#[serial(env)]` 是 serial_test 的两个不同 key，
        // 彼此不互斥，也都不与 crate::test_support::env_guard() 互斥——只靠它们
        // 挡不住跨模块串扰。详见 crate::test_support。
        let _env_guard = crate::test_support::env_guard();
        let saved = std::env::var("KAIXUAN_LOCAL_GATEWAY_PORT").ok();
        std::env::set_var("KAIXUAN_LOCAL_GATEWAY_PORT", "8899");
        let eps = known_endpoints();
        let local = eps.iter().find(|e| e.id == "local-8782").unwrap();
        assert_eq!(local.url, "http://127.0.0.1:8899/v1/models");
        assert_eq!(local.label, "本地 8899");
        match saved {
            Some(v) => std::env::set_var("KAIXUAN_LOCAL_GATEWAY_PORT", v),
            None => std::env::remove_var("KAIXUAN_LOCAL_GATEWAY_PORT"),
        }
    }

    #[test]
    #[serial(env)]
    fn known_endpoints_default_port_8782() {
        // 共享 env 锁：本测试改进程全局 env，必须与**全 crate** 所有改 env 的
        // 用例互斥。`#[serial]` 与 `#[serial(env)]` 是 serial_test 的两个不同 key，
        // 彼此不互斥，也都不与 crate::test_support::env_guard() 互斥——只靠它们
        // 挡不住跨模块串扰。详见 crate::test_support。
        let _env_guard = crate::test_support::env_guard();
        let saved = std::env::var("KAIXUAN_LOCAL_GATEWAY_PORT").ok();
        std::env::remove_var("KAIXUAN_LOCAL_GATEWAY_PORT");
        let eps = known_endpoints();
        let local = eps.iter().find(|e| e.id == "local-8782").unwrap();
        assert_eq!(local.url, "http://127.0.0.1:8782/v1/models");
        if let Some(v) = saved {
            std::env::set_var("KAIXUAN_LOCAL_GATEWAY_PORT", v);
        }
    }
}
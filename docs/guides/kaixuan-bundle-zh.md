# kaixuan bundle 一键接入指南（zh）

> 适用版本：v3.20.4+（feat: kaixuan bundle）
> 一句话：装好 `~/.codex` 之后，cc-switch UI 顶部出现「kaixuan bundle」卡，点一下「安装/重装」就同时把开轩 kxpms.cn 和本机 8782 接进 Codex，自动开启故障转移。

## 一、部署前置

### 1.1 公网网关（开轩 kxpms.cn）

远端网关由 kx-citus-pg17 + llm-gateway-go 容器组成，已部署在 47.97.111.154（NPS+VPN）后面。用户只需保证 `https://llm.kxpms.cn/v1/models` 可达：

```bash
curl -s -o /dev/null -w "%{http_code}\n" --max-time 5 https://llm.kxpms.cn/v1/models
# 期望：401（网关在跑但缺 key） 或 200
```

### 1.2 本机网关（127.0.0.1:8782）

本机跑 llm-gateway-go 容器。三种部署方式（按优先级）：

| 方式                    | 路径                                                                            | 触发                                                                         |
| ----------------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| **(a) 自定义命令**      | `KAIXUAN_GATEWAY_START_CMD` 环境变量                                            | cc-switch 在「重启 8782」按钮按下时执行 `sh -c "$KAIXUAN_GATEWAY_START_CMD"` |
| **(b) 项目惯例脚本**    | `~/kaixuan/llm-gateway-local/start.sh`                                          | 文件存在即执行；缺权限请 `chmod +x`                                          |
| **(c) Docker fallback** | `docker run -d --name llm-gateway-local-8782 -p 8782:8782 llm-gateway-go:local` | 上两条都没找到时执行；要求本地有 `llm-gateway-go:local` 镜像                 |

### 1.3 验证本机网关

```bash
curl -s -o /dev/null -w "%{http_code}\n" --max-time 3 http://127.0.0.1:8782/v1/models
# 期望：200
```

## 二、cc-switch 端操作

### 2.1 一键安装

1. 打开 cc-switch → 切到 **Codex** 应用 Tab
2. 顶部出现「kaixuan bundle」卡片
3. 在「主端点 API Key」输入框填：
   - 占位符形式：`$KAIXUAN_KXPMS_KEY`
   - 或明文形式：`sk-live-xxxxx`（不推荐，存 db）
4. 「备用端点 API Key」可留空（用同一把 key）或填独立 key
5. 点「安装/重装」

后端会：

- 写两条 provider：`kaixuan-kxpms`（主）+ `kaixuan-local-8782`（备）
- 两条都加入故障转移队列，`sort_index` 递增
- 开启该 AppType 的代理接管 + `auto_failover_enabled=true`
- 切到 P1 = `kaixuan-kxpms`

### 2.2 状态显示

卡片上有两盏灯：

| 端点         | 健康 =                 | 健康 ≠                |
| ------------ | ---------------------- | --------------------- |
| `kxpms`      | <Wifi/> + `200 · 35ms` | <WifiOff/> + 错误信息 |
| `local-8782` | 同上                   | 同上                  |

每 15 秒轮询一次，窗口聚焦时立即刷新。8782 不可达时点 8782 行右侧的 ▶ 按钮一键启动。

## 三、故障排查

### 3.1 bundle install 失败：bundle unknown

```bash
# 看后端是否注册
sqlite3 ~/.cc-switch/cc-switch.db "SELECT 1;"
# 重新打开 cc-switch 让 startup 跑过；或重启应用
```

### 3.2 健康灯一直红，但 curl 能通

检查 cc-switch 进程能否继承到 env：

```bash
ps eww -p $(pgrep -f "cc-switch") | tr ' ' '\n' | grep -i "kaixuan\|http_proxy"
# macOS GUI 应用默认不读 ~/.zshenv；用 launchctl setenv（见 env-vars-zh 文档）
```

### 3.3 健康灯黄/红，但实际可用

reqwest 默认 200ms connect timeout / 500ms 全 timeout，公网拥塞时偶发。可调整 `src-tauri/src/gateway_health.rs::shared_client` 的 timeout：

```rust
Client::builder()
    .timeout(Duration::from_millis(2000))  // 改大
    .connect_timeout(Duration::from_millis(1000))
```

### 3.4 启动本机网关后仍不可达

1. 看 cc-switch 启动命令是否真执行：`start_local_gateway` 返回的 `stdout` / `stderr`
2. 看 docker 容器：`docker ps | grep llm-gateway-local-8782`
3. 看 docker 日志：`docker logs llm-gateway-local-8782`

## 四、扩展：再加一个 bundle

后端 `src-tauri/src/provider_bundle.rs::bundles()` 里加行即可：

```rust
pub fn bundles() -> Vec<BundleSpec> {
    vec![kaixuan_bundle(), my_other_bundle()]
}
```

UI 端 `useKaixuanBundles()` 已经返回所有 bundle，无需改动。

## 五、端口覆盖与跨平台

### 5.1 本机网关端口非 8782

```bash
# 启动 cc-switch 前 export；kxpms 端不受影响
export KAIXUAN_LOCAL_GATEWAY_PORT=8899
launchctl setenv KAIXUAN_LOCAL_GATEWAY_PORT 8899  # macOS GUI 也要设
open -a cc-switch
```

bundle 安装时会用 `http://127.0.0.1:8899/v1` 作为 base_url；启动入口也会用
8899 端口。

### 5.2 Windows

`start_local_gateway` 走的是 shell；Windows 上自动用 `cmd.exe /C` 而不是
`/bin/sh`。但 `~/kaixuan/llm-gateway-local/start.sh` 路径在 Windows 上
无效——要 Windows 用户用 `KAIXUAN_GATEWAY_START_CMD` 自定义命令，或
直接走 docker fallback（`docker run -d --name llm-gateway-local-8782
-p 8782:8782 llm-gateway-go:local`，前提是装了 docker desktop）。

## 六、与已有「双独立预设」的关系

|          | 旧方式                                                             | 新 kaixuan bundle                                           |
| -------- | ------------------------------------------------------------------ | ----------------------------------------------------------- |
| UI 操作  | 选「开轩 LLM 网关」预设 + 选「本地 LLM 网关 (8782)」预设，两次点击 | 点「安装/重装」一次                                         |
| 故障转移 | 需要手动把两个都加入队列                                           | 自动入队 + 开启 auto_failover                               |
| 默认模型 | 两个独立 `default` 切换                                            | bundle 装好后 P1 = kxpms                                    |
| 配置覆盖 | 重装会覆盖用户编辑的 api key                                       | 同 id 已存在时保留用户 settings_config（只刷新 membership） |

旧的两个独立预设（`codexProviderPresets.ts` 里的 `开轩 LLM 网关` / `本地 LLM 网关 (8782)`）保留，可继续单条使用；不冲突。

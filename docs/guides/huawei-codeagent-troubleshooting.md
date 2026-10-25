# Huawei CodeAgent 与 Claude Code 排查记录（2026-10-24）

本文记录这次实测结论，供后续排查直接使用。历史设计文档 `docs/huawei-codeagent-3-design.md` 中的 8888 连通性代理方案已被本次修改取代；不要按旧图恢复它。构建步骤见 [Windows Tauri 构建](windows-build.md)。

## 当前请求路径

- Claude Code（Code 模式）使用本机 `ANTHROPIC_BASE_URL=http://127.0.0.1:15721`；`15721` 是 cc-switch 的本机网关，**不是 Huawei 实际上游地址**。上游地址由当前供应商配置及网关转发逻辑决定，不能从报错里的 `requestUrl` 直接推断。
- Cowork/Claude Desktop 与 Claude Code CLI 的配置和进程环境不同。Cowork 可用不等于 CLI 的环境代理或本机请求路径正确；排查时应分别验证。
- 原先的 `127.0.0.1:8888` 是项目为 CLI 连通性检查引入的本地兼容代理，不是 Claude Code 官方固定要求的端口。本次用户决定先移除它：预设不再注入 `HTTP_PROXY`、`HTTPS_PROXY` 与 `NODE_TLS_REJECT_UNAUTHORIZED=0`，Rust hook 不再启动 8888 监听器；保留 loopback 的 `NO_PROXY` 和 15721 网关。若以后确需兼容代理，应以可复现的失败为依据重新设计。
- CLI 的请求获供应商认证后，经本机网关转发；不能把 Claude Code 与 Claude Desktop 当成同一个客户端实现。

## 这次出现过的问题及定位

| 现象 | 已确认的原因或判别方法 | 处理 |
| --- | --- | --- |
| Code 模式返回 `504 HIS Proxy Notification`，响应体是 Huawei 企业代理 HTML | 本机 loopback 请求被企业代理接管；该 HTML 不是模型 API 正常返回，也不能仅据此判断模型额度或权限 | 检查 Windows 代理绕过列表和 CLI 进程环境。此次移除绕过列表中的 `<-loopback>` 后重启 Claude Desktop，Code 模式恢复；先备份原值再更改系统配置 |
| Cowork 正常，Code 模式失败 | 两种模式走不同客户端及环境，Code CLI 曾沿用企业代理环境 | 分别检查 `ANTHROPIC_BASE_URL`、`HTTP_PROXY`、`HTTPS_PROXY`、`NO_PROXY`，验证 `127.0.0.1:15721` 直达 |
| 供应商页面显示 xAI 认证或“未识别用户认证信息” | Huawei 供应商元数据在表单默认值与保存路径中丢失会导致识别错误 | 本次修改 `ProviderForm.tsx` 合并预设默认元数据，并补回归测试；确认保存后的 `providerType`、`appTarget` |
| 测试请求显示 `probedModel: claude-haiku-4-5` | 这是连通性探测使用的 Claude 模型名，不足以证明 Huawei 实际使用 Haiku；应检查模型映射与最终转发请求 | 在当前供应商中核对 `ANTHROPIC_DEFAULT_HAIKU_MODEL`、`ANTHROPIC_MODEL` 等配置。若上游返回明确的模型无权限/未注册，再按实际上游模型 ID 处理 |
| “Session usage sync completed with 5 error(s)”或用量缺少模型维度 | 需区分用量同步接口失败、供应商是否提供额度接口，以及网关请求统计；模型列表正确不代表额度接口可用 | 先查对应同步日志和该供应商的用量实现；不要把模型列表当作可用额度证明 |

## 重新验证顺序

1. 检查当前运行的是新构建的 `cc-switch.exe`，且 15721 在监听、8888 不再由本程序监听。
2. 检查 `~/.claude/settings.json` 中的本机网关地址及 loopback 的 `NO_PROXY`；排除遗留的 8888 `HTTP_PROXY`/`HTTPS_PROXY` 和 `NODE_TLS_REJECT_UNAUTHORIZED=0`。此文件是用户设置，不要直接写入仓库或日志。
3. 用 Claude Code CLI 发一条最小请求，同时观察网关日志。此前在保留企业代理环境、移除 8888 设置后，一次最小请求已成功（约 4.8 秒）。完整应用重启后的验证以本轮构建结果为准。
4. 如果仍有 504，区分响应是 Huawei HIS HTML，还是上游模型 API 的 JSON 错误；同时记录模型映射后的 ID、HTTP 状态和当前运行程序版本，避免把不同故障合并处理。

## 代码入口

- CLI 供应商预设：`src/huawei/huaweiCodeAgent3Presets.ts`
- 供应商表单元数据：`src/components/providers/forms/ProviderForm.tsx`
- Huawei 网关 hook：`src-tauri/src/huawei/proxy_hooks.rs`
- 模型映射：`src-tauri/src/proxy/model_mapper.rs`
- 本机构建：`scripts/build-windows-tauri.cmd`

本文不记录账号、token、个人配置内容或企业内网实际域名；它们必须从当前授权配置核实。

## 本轮最终验证（2026-10-24）

新 Release 进程监听 127.0.0.1:15721，8888 无监听；Claude Code 设置中的 ANTHROPIC_BASE_URL 为本机 15721、NO_PROXY 包含 loopback，且无 8888 HTTP_PROXY/HTTPS_PROXY 和 TLS 校验绕过。以 claude.cmd -p 'Reply with exactly OK.' --max-turns 1 --output-format text 发送最小请求，约 12 秒正常返回 OK（退出码 0）。这确认该时间点的 Code CLI 路径可用。

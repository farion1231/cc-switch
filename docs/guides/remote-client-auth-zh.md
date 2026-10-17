# 远程访问客户端凭据契约（中文）

> 本文是对**第三方接入方**的对外契约。英文版见
> [remote-client-auth-en.md](./remote-client-auth-en.md)；两版内容一致。

## 这份契约管什么

CC Switch 的远程模式把本机 API 暴露给局域网/Tailscale 上的其他设备。任何想调用
这些 API 的第三方客户端，都必须自己保管一份长期凭据。

**服务端一半**由本仓实现并由测试覆盖：配对端点签发凭据，中间件只按摘要比对。
**客户端一半**在本仓没有任何代码 —— 本仓没有 `[[bin]]`，`src-tauri/src/bin` 不存在，
全仓也没有任何 keychain / keyring 读写。因此客户端一半**只以本文档形式交付，
明确不做代码验证**：本仓不实现、不编译、不测试客户端的存储行为，只断言本文的固定措辞
（见 `src-tauri/tests/contract_docs_gate.rs`）。

本文不提供参考客户端，也不提供只做正确性校验的参考片段 —— 后者会让读者以为它能安全地
照抄，而一个只校验正确性、不负责安全存储的片段恰恰是最容易被照抄的那种。

## 两类客户端载体

契约按**载体能不能自己设 HTTP 头**分成两类，两类的存储策略不同。

### 一、程序化客户端（CLI / fetch / curl / 服务端程序）

这类载体能自己设 `Authorization` 头，凭据走：

```
Authorization: Bearer <clientToken>
```

服务端只认这一个方案。配对期间的一次性轮询凭据则走另一个方案
`Authorization: Pairing <pollToken>`，两者不可互换。

**存储策略**：明文 token 一旦签发，在客户端进程内存之外必须落到安全存储里。
在 macOS 上应把明文 token 存入 Keychain，实现方式为调用 `/usr/bin/security`
（本仓不引入 keyring 类 crate，故不提供该实现）。其他平台的等价做法由接入方自行选择，
本契约不作规定。

### 二、浏览器上下文

浏览器不具 Keychain 能力：网页拿不到 macOS Keychain，也没有任何一条浏览器 API
能把东西写进去。而且凭据本来就不该进页面 —— 内嵌 Web UI 是第三方页面，把 token 交给它
等于交给它的一切脚本。这类载体走服务端下发的**会话 cookie**：

- 浏览器导航、子资源、`EventSource` 会话由浏览器自动携带，无需页面脚本介入；
- cookie 带 `HttpOnly`（页面脚本读不到）、`SameSite=Strict`、`Path=/`、`Max-Age`；
- cookie 的判据是凭据库里那条记录还在不在，`POST /pairing/revoke` 摘掉记录即失效。
- 这条 cookie 在**所有**受保护路由上都被接受，不只是浏览器专用的那几条
  （`/api/events`、`/api/icon` 一类）：远端浏览器打开内嵌 Web UI 之后，页面里的
  `fetch('/api/providers')`、`fetch('/api/switch')` 与程序化客户端带 Bearer token
  打的是同一档凭据。同一发请求里**通道之间不回落** —— `Authorization` 头一旦出现，
  就必须由它验过；带着无效的 Bearer 再挂一条有效 cookie，答案仍是 401。

因此**浏览器载体不适用 Keychain 策略，也不应被要求执行它**：浏览器上下文里的凭据由
cookie 机制承载，接入方在浏览器侧无需、也不应尝试持久化明文 token。

## 禁止事项（两类载体通用）

任何客户端都不得放进 URL、提交进仓库或写进 shell history。

具体地说，以下三类位置在本契约下都是凭据泄漏：

- **URL**：查询参数会让凭据进浏览器历史、代理日志与 `Referer`。配对端点只认
  `Authorization` 头，查询参数不参与鉴权 —— URL 里出现 token 时它已经泄了。
- **仓库**：`.env`、配置文件、示例代码、提交历史。
- **shell history**：命令行参数与内联环境变量赋值都会留档。

凭据应当只在内存与上述安全存储之间流动。日志、错误信息、崩溃报告同样不得包含明文 token。

## 绑定范围与两段式确认

服务监听在哪些地址上由三档绑定范围决定（取值按从窄到宽排列）：

| 档位 | 监听地址 | 谁能连上 |
|---|---|---|
| `loopback` | `127.0.0.1:<port>` | 只有本机 |
| `tailscale` | `127.0.0.1:<port>` + Tailscale IP | 本机与该 tailnet |
| `allInterfaces` | `0.0.0.0:<port>` | 任何能到达本机 LAN / Tailscale 地址的设备 |

**收窄**（往左走）立即生效。**放宽**（往右走）必须两段式：

1. `POST /api/remote/config/bind-scope/request`（body `{ "scope": "..." }`）
   只登记一条待确认请求，返回 `{ "confirmationId": "...", "expiresAt": <unix 秒> }`；
   配置不变、不落盘；
2. `POST /api/remote/config/bind-scope/confirm`（body `{ "confirmationId": "..." }`）
   才兑现、落盘并重绑 listener。`POST /api/remote/config/bind-scope/reject` 撤销待确认请求。

三条端点都是 `Protected`：**非回环来源仍然必须持有配对凭证**。确认 id 一次性、
5 分钟后过期，并且只能由**发起申请的那个已认证会话**兑现（Bearer 与 cookie 是两个
不同的会话摘要）。服务端**没有**「直接设置绑定范围」的端点。

诚实的边界声明：对「已被完全攻陷、且已配对的客户端」，两段式是**提高门槛 + 留痕**，
不是硬边界 —— 它能挡住渲染层一行代码就想放宽的路径，挡不住一枚合法凭据背后的攻击者
自己走完申请与确认两步。硬边界只有「必须回到那台机器上点原生弹窗」，本设计没有选它。

## 边界

- 本文描述的是**接入方该做什么**，不是服务端行为说明；服务端行为以代码与
  `ROUTE_SCOPE_TABLE` 为准。
- 撤销（`POST /pairing/revoke`）只摘掉服务端凭据库里的记录。客户端侧的明文副本由接入方
  自己清理 —— 本契约不提供远程清除客户端存储的能力，也没有任何端点能做这件事。
- 本仓不验证客户端是否真的执行了上述策略。接入方违反本文不会被服务端拒绝请求以外的任何
  机制发现，因此存储与传输安全由接入方自负。

## 相关

- 服务端实现：`src-tauri/src/remote/auth/`、`src-tauri/src/remote/pairing/`
- 本契约的静态门禁：`src-tauri/tests/contract_docs_gate.rs`
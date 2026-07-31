# Security Policy / 安全策略

## Supported Versions / 支持的版本

Only the latest release of CC Switch receives security updates.

仅最新版本的 CC Switch 会收到安全更新。

| Version / 版本 | Supported / 是否支持 |
|----------------|---------------------|
| Latest 3.x     | ✅ Yes / 是          |
| < 3.0          | ❌ No / 否           |

## Threat Model / 威胁模型

CC Switch is a local desktop application. It manages configuration files for AI coding CLIs on the user's own machine. There is no project-operated cloud backend, no multi-user model, and no privilege separation from the user who runs it.

CC Switch 是一个本地桌面应用，用于管理本机上各 AI 编程 CLI 的配置文件。本项目不运营任何云端后端，没有多用户模型，也不与运行它的用户之间存在权限隔离。

It does, however, run a **local HTTP proxy** whose listen address and port are user-configurable and **may be bound to a non-loopback interface**. Requests arriving at that listener are untrusted input and are in scope — see Scope below.

但它会启动一个**本地 HTTP 代理**，其监听地址与端口可由用户配置，**可能绑定到非 loopback 接口**。抵达该监听端口的请求属于不可信输入，在范围内——见下方「范围」。

### The bundled renderer is inside the trust boundary / 打包的渲染进程属于信任边界之内

The bundled WebView renderer is treated as a trusted component. This is a **scoping decision, supported by the facts below rather than derived from them** — the facts are what make the decision checkable, and if any ceases to hold the decision must be revisited. Verified against v3.18.0:

打包的 WebView 渲染进程被视为可信组件。这是一项**范围划定决策，由下列事实支撑，而非从中必然推出**——这些事实的作用是让该决策可被核验；一旦任一条不再成立，该决策必须重新评估。已针对 v3.18.0 核实：

1. **No remote executable content is loaded.** `frontendDist` is bundled at build time (`src-tauri/tauri.conf.json`); the codebase contains no `<iframe>`, no `<webview>`, and no remote script or stylesheet URL. The application *does* retrieve remote **data** — model pricing JSON and provider avatars — which CSP permits via `connect-src`/`img-src`; such data is treated as untrusted input, not as content.
   前端资源在构建期打包，代码库中不存在 `<iframe>`、`<webview>` 或远程脚本/样式地址。应用**确实**会获取远程**数据**（模型定价 JSON、供应商头像），CSP 经 `connect-src`/`img-src` 允许之；此类数据按不可信输入对待，不作为内容。
2. **CSP restricts script execution to bundled assets** — `script-src 'self'` (`src-tauri/tauri.conf.json`).
   CSP 将脚本执行限制在打包资源内。
3. **No dynamic code evaluation.** There is no `eval()` or `new Function()` anywhere under `src/`.
   `src/` 下不存在 `eval()` 或 `new Function()`。
4. **The single HTML sink never receives user-controlled content.** `src/components/ProviderIcon.tsx` uses `dangerouslySetInnerHTML` exactly once; the user-supplied `icon` field is used **solely as a lookup key** into a build-time icon table, never as content.
   全库唯一的 `dangerouslySetInnerHTML` 位于 `src/components/ProviderIcon.tsx`；用户提供的 `icon` 字段**仅作为查表的键**用于构建期图标表，永不作为内容。

**What this excludes — and what it does not.** Out of scope: reports whose only path to the IPC surface is **direct invocation from DevTools or from a locally modified frontend**. Someone in that position controls the machine already.

**它排除什么、不排除什么。** 不在范围内的是：抵达 IPC 接口的**唯一途径**为**从 DevTools 或本地改造过的前端直接调用**的报告。处于该位置的人已经控制了这台机器。

**Still in scope:** any complete, demonstrable chain in which an *untrusted* source — a `ccswitch://` deep link, a remote sync payload, remote data, an inbound proxy request, or an XSS — reaches a high-privilege IPC command. The trust placed in the renderer covers the code we ship, not arbitrary values that flow through it.

**仍在范围内**：任何完整、可演示的利用链，其中**不可信来源**——`ccswitch://` deeplink、远程同步载荷、远程数据、代理入站请求或 XSS——抵达高权限 IPC 命令。对渲染进程的信任覆盖的是我们发布的代码，而非流经其中的任意值。

### Invalidation triggers / 声明失效条件

The scoping decision above is void the moment any of the following becomes true. Reports demonstrating any of these are **always in scope**, and we ask to be told about them:

一旦下列任一条件成立，上述范围划定立即作废。证明下列任一情形的报告**始终在范围内**，也欢迎报告：

- Remote **executable or navigable** content is loaded into the WebView — iframe, webview, remote script, remote stylesheet, or navigation to a remote origin
  远程**可执行或可导航**内容被载入 WebView——iframe、webview、远程脚本、远程样式表，或导航至远程源
- `script-src` is relaxed beyond `'self'`
  `script-src` 被放宽到 `'self'` 之外
- `eval()` or `new Function()` is introduced
  引入 `eval()` 或 `new Function()`
- Any user-controlled string reaches an HTML sink as content
  任何用户可控字符串作为内容进入 HTML sink
- The IPC surface is exposed to a non-bundled origin
  IPC 接口暴露给非打包来源

## Scope / 范围

### In scope / 在范围内

Inputs that genuinely cross a trust boundary:

真正跨越信任边界的输入：

- `ccswitch://` deep link payloads / deeplink 载荷（由第三方构造，经浏览器抵达）
- **Inbound requests to the local HTTP proxy**, including from other hosts when it is configured to bind a non-loopback address / **抵达本地 HTTP 代理的入站请求**，包括配置为绑定非 loopback 地址时来自其他主机的请求
- Remote sync payloads restored from WebDAV / S3 / 从 WebDAV、S3 还原的同步数据
- Imported files: SQL import/export, provider and MCP config import / 导入文件：SQL 导入导出、供应商与 MCP 配置导入
- Upstream API responses processed by the local proxy (`src-tauri/src/proxy/`) / 本地代理处理的上游 API 响应
- Remote data rendered or acted upon by the renderer (model pricing, avatars) / 渲染进程展示或据以行动的远程数据（模型定价、头像）
- Live config files on disk that a third party can write / 磁盘上可被第三方写入的 live 配置文件
- Any path by which credentials (API keys, tokens) reach logs, telemetry, or shared config snippets / 凭据（API Key、令牌）进入日志、遥测或共享配置片段的任何路径
- The build, release, signing and updater pipeline / 构建、发布、签名与更新链路

### Out of scope / 不在范围内

- Findings whose only path to the IPC surface is direct invocation from DevTools or a locally modified frontend — see Threat Model
  抵达 IPC 接口的唯一途径为从 DevTools 或本地改造过的前端直接调用的问题——见威胁模型
- **Ordinary file operations the user directs.** Reading or writing a file whose path *and* content the user chose through the local UI, with no untrusted input participating.
  **用户主动指示的常规文件操作。** 读写路径**与**内容均由用户经本地界面选定、且无不可信输入参与的文件。
  → Not excluded: cases where a deep link, sync payload, proxy request or other untrusted source controls the path or the content. Having the same filesystem permissions as the user does not make it the user's decision — that is a confused-deputy attack and is **in scope**.
  → 不属豁免：路径或内容由 deeplink、同步载荷、代理请求等不可信来源控制的情形。攻击者与用户拥有相同的文件系统权限，并不等于该操作出自用户的决定——那是 confused deputy 攻击，**在范围内**。
- **User-authored integrations executing by design.** MCP servers, terminal launch and usage scripts run commands because that is their purpose. Where the user typed the command themselves and enabled it themselves, execution is the feature, not the bug.
  **用户亲手编写的集成按设计执行命令。** MCP server、终端启动、用量脚本执行命令是其本职。命令由用户自己输入、并由用户自己启用时，执行本身是功能而非缺陷。
  → Not excluded: the same integrations when they **arrive through import or a deep link**. There the required security property is *informed consent*, and the following are **in scope**: the command, arguments, environment or script body being hidden, truncated or misrepresented in the confirmation UI; and any integration carrying executable content being enabled without an explicit user decision.
  → 不属豁免：同样的集成**经导入或 deeplink 抵达**时。此时所要求的安全属性是**知情同意**，下列情形**在范围内**：确认界面隐藏、截断或错误展示命令、参数、环境变量或脚本正文；以及任何携带可执行内容的集成在缺少用户明确决定的情况下被启用。
- Denial of service against the user's own local instance
  针对用户自己本地实例的拒绝服务
- Automated scanner output without a demonstrated exploitation path on a currently supported release
  未在受支持版本上给出可行利用路径的自动化扫描结果
- Findings against unsupported versions — see Supported Versions
  针对不受支持版本的问题——见支持的版本

## Reporting a Vulnerability / 报告漏洞

**Please do NOT report security vulnerabilities through public GitHub issues.**

**请不要通过公开的 GitHub Issue 报告安全漏洞。**

Instead, please report them through [GitHub Security Advisories](https://github.com/farion1231/cc-switch/security/advisories/new).

请通过 [GitHub 安全公告](https://github.com/farion1231/cc-switch/security/advisories/new) 进行报告。

When reporting, please include:

报告时请包含以下信息：

- A description of the vulnerability / 漏洞描述
- **The untrusted source of the input, and the full data path from that source to the affected code** / **输入的不可信来源，以及从该来源到相关代码的完整数据路径**
- Steps to reproduce against a currently supported release / 在受支持版本上的复现步骤
- Potential impact / 潜在影响
- Affected versions / 受影响版本

The data path matters more than the sink. We assess severity by **who controls the input**, not by which API the value eventually reaches — the same function call can be critical or harmless depending entirely on where its argument came from.

数据路径比 sink 更重要。我们按**谁能控制输入**来评估严重度，而非按该值最终抵达哪个 API——同一处调用是严重还是无害，完全取决于其参数的来源。

## Response Timeline / 响应时间

- **Acknowledgment / 确认**: within 48 hours / 48 小时内
- **Initial assessment / 初步评估**: within 7 days / 7 天内
- **Fix for critical issues / 关键问题修复**: within 14 days / 14 天内

## Disclosure Policy / 披露政策

We follow a coordinated disclosure process:

我们遵循协调披露流程：

1. The reporter submits the vulnerability privately. / 报告者私下提交漏洞。
2. We confirm and work on a fix. / 我们确认并修复漏洞。
3. A patch release is published. / 发布修复版本。
4. The vulnerability is publicly disclosed. / 公开披露漏洞详情。

Reporters will be credited in the release notes unless they prefer to remain anonymous.

除非报告者希望匿名，否则将在发布说明中致谢。

### CVE identifiers / CVE 编号

For eligible vulnerabilities, the maintainer may request a CVE ID through a GitHub Security Advisory once a fix is available. Being in scope for this policy and meeting GitHub's CVE eligibility criteria are separate questions, and the second is decided by GitHub as CNA, not by this project.

对于符合条件的漏洞，维护者可在修复就绪后通过 GitHub 安全公告申请 CVE 编号。「属于本策略范围」与「符合 GitHub 的 CVE 分配条件」是两个不同的问题，后者由作为 CNA 的 GitHub 判定，而非本项目。

Severity is scored with CVSS (v3.1 or v4.0), and the vector will reflect any required user interaction or prior local access.

严重度采用 CVSS（v3.1 或 v4.0）评分，向量将如实反映所需的用户交互或前置本地访问条件。

## Security Updates / 安全更新

Security fixes are released as patch versions and announced via [GitHub Releases](https://github.com/farion1231/cc-switch/releases). We recommend always updating to the latest version.

安全修复通过补丁版本发布，并通过 [GitHub Releases](https://github.com/farion1231/cc-switch/releases) 通知。建议始终更新到最新版本。

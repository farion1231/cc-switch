<div align="center">

# CC Switch

### Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build、OpenCode、OpenClaw、Hermes Agent、Pi、MiniMax Code 的全方位管理工具

**一键切换 API 供应商，统一管理 MCP、Skills 与提示词，不用再手改 JSON / TOML / YAML 配置文件。**

[![Version](https://img.shields.io/github/v/release/farion1231/cc-switch?color=blue&label=version)](https://github.com/farion1231/cc-switch/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/farion1231/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![Downloads](https://img.shields.io/github/downloads/farion1231/cc-switch/total)](https://github.com/farion1231/cc-switch/releases/latest)

<a href="https://trendshift.io/repositories/15372" target="_blank"><img src="https://trendshift.io/api/badge/repositories/15372" alt="farion1231%2Fcc-switch | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>
<a href="https://www.star-history.com/#farion1231/cc-switch&Date"><picture><source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/badge?repo=farion1231/cc-switch&theme=dark" /><img alt="Star History Rank" src="https://api.star-history.com/badge?repo=farion1231/cc-switch" width="196" height="55" /></picture></a>

### 🌐 唯一官方网站：**[ccswitch.io](https://ccswitch.io)**

[English](README.md) | 中文 | [日本語](README_JA.md) | [Deutsch](README_DE.md) | [更新日志](CHANGELOG.md)

**[下载安装](#下载安装) · [快速开始](#快速开始) · [功能特性](#功能特性) · [常见问题](#常见问题) · [用户手册](docs/user-manual/zh/README.md)**

</div>

## ❤️赞助商

> [想出现在这里？](mailto:farion1231@gmail.com)

<details open>
<summary>点击折叠</summary>

[![Kimi K2.7 Code](https://gcdn.moonshot.cn/growth-cdn/sponsor/kimi-zh.png)](https://platform.kimi.com?track_id=track-6840233b42274ab4bcfd283e2bdd2aee&aff=cc-switch)

Kimi K3 是 Moonshot AI 迄今能力最强的模型，也是全球首个开源 3T 级模型。K3 拥有 2.8T 参数、原生视觉能力与 100 万 Token 上下文，在长程编码、知识工作和推理任务中展现前沿性能。使用 CC Switch，可以在各类 Agent 工具中便捷配置和切换 Kimi。

主要进行编程工作？可以试试 **Kimi Code 订阅**（[中文站](https://www.kimi.com/code?aff=cc-switch) | [Global](https://www.kimi.ai/code?aff=cc-switch)），或通过 Kimi 开放平台使用 **API**（[中文站](https://platform.kimi.com?track_id=track-6840233b42274ab4bcfd283e2bdd2aee&aff=cc-switch) | [Global](https://platform.kimi.ai?track_id=track-20d65732f0aa45dcb1df9691a15610af&aff=cc-switch)）。

**新用户首充福利**：通过上方 API 链接注册并完成首次充值，即可获赠充值金额 10% 的 API 额度，最高赠送 ¥1,000。

---

<table>
<tr>
<td width="180"><a href="https://www.packyapi.ai/register?aff=cc-switch"><img src="assets/partners/logos/packycode.png" alt="PackyCode" width="150"></a></td>
<td>感谢 PackyCode 赞助了本项目！PackyCode 是一家稳定、高效的API中转服务商，提供 Claude Code、Codex、Gemini 等多种中转服务。PackyCode 为本软件的用户提供了特别优惠，使用<a href="https://www.packyapi.ai/register?aff=cc-switch">此链接</a>注册并在充值时填写"cc-switch"优惠码，首次充值可以享受9折优惠！</td>
</tr>

<tr>
<td width="180"><a href="https://zetaapi.ai/go/u117"><img src="assets/partners/logos/zetaapi-banner.png" alt="ZetaAPI" width="150"></a></td>
<td>感谢 ZetaAPI 赞助本项目！ZetaAPI 主打模型不掺水、保真不降智、价格低至官方价 35 折，平台不混量、不暗中替换低质量模型、不做虚假路由，支持 Claude Code、Codex、Gemini、ChatGPT 等主流模型接入，帮助用户在保证模型质量的同时大幅降低 API 使用成本。同时，ZetaAPI 提供企业级 SLA 稳定性保障、标准接口兼容、一个 Key 接入多模型、快速集成、按量计费等能力，适用于 AI 产品、代码生成、企业内部工具、客服系统、内容生产和自动化流程等场景。若经验证发现模型质量与标称不符，ZetaAPI 承诺假一赔十，让用户用得更稳定、更透明、更放心。通过<a href="https://zetaapi.ai/go/u117">此链接</a>注册，并在首次充值时使用优惠码 CC-SWITCH，即可享受 CC Switch 用户专属的首次充值九折优惠！</td>
</tr>

<tr>
<td width="180"><a href="https://apinebula.ai/VjM74M"><img src="assets/partners/logos/apinebula_banner.png" alt="APINebula" width="150"></a></td>
<td>感谢 APINEBULA 赞助本项目！APINEBULA 是银河录像局旗下的企业级 AI 聚合平台，背靠大平台资源，面向开发者、团队与企业用户提供稳定、高性价比的大模型 API 接入服务。平台聚合 Claude、GPT、Gemini 等主流满血模型，一个接口，接入全球顶尖 AI 大模型，各大模型价格低至 1 折起，支持企业级高并发、正式合同、对公打款与开票服务，适合 AI 编程、Agent 开发、业务系统集成等多种场景！使用<a href="https://apinebula.ai/VjM74M">此链接</a>注册并在充值时填写 <strong>"ccswitch"</strong> 优惠码可享<strong>九折优惠</strong>！</td>
</tr>

<tr>
<td width="180"><a href="https://www.aicodemirror.ai/register?invitecode=9915W3"><img src="assets/partners/logos/aicodemirror.jpg" alt="AICodeMirror" width="150"></a></td>
<td>感谢 AICodeMirror 赞助了本项目！AICodeMirror 提供 Claude Code / Codex / Gemini CLI 官方高稳定中转服务，支持企业级高并发、极速开票、7×24 专属技术支持。
Claude Code / Codex / Gemini 官方渠道低至 3.8 / 0.2 / 0.9 折，充值更有折上折！AICodeMirror 为 CCSwitch 的用户提供了特别福利，通过<a href="https://www.aicodemirror.ai/register?invitecode=9915W3">此链接</a>注册的用户，可享受首充8折，企业客户最高可享 7.5 折！</td>
</tr>

<tr>
<td width="180"><a href="https://pateway.ai/?ch=etzpm8&aff=WB6M6F67#/"><img src="assets/partners/logos/pateway.png" alt="PatewayAI" width="150"></a></td>
<td>感谢 PatewayAI 赞助了本项目！PatewayAI 是一家面向重度 AI 开发者、专注官方直连高品质模型 API 中转服务商。提供 Claude 全系列与 Codex 系列模型，100% 官方源直供，不掺假不注水，欢迎检验。计费透明，Token 级账单可逐笔核验。
同时支持企业级高并发，并为企业客户提供了专业的管理平台，企业客户可签订正式合同并开具发票，更多详情进入官网获取联系方式。
现在通过<a href="https://pateway.ai/?ch=etzpm8&aff=WB6M6F67#/">此链接</a>注册即送 $3 试用额度，用户充值低至 6 折，邀请好友双向赠送，邀请奖励可达 $150！</td>
</tr>

<tr>
<td width="180"><a href="https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL"><img src="assets/partners/logos/fenno-banner.png" alt="Fenno.ai" width="150"></a></td>
<td>感谢 Fenno.ai 赞助了本项目！Fenno.ai 是一家稳定、高效的 API 中转服务商，目前主要提供 Codex 中转服务，兼容 OpenAI 及 Anthropic 协议，可灵活接入 Codex、Claude Code、OpenCode 等主流编程工具，可稳定支撑千亿 Token/日的企业级调用需求，支持国内及海外主体公对公结算、开票。Fenno.ai 为 CC Switch 的用户提供了专属福利：通过<a href="https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL">此链接</a>即可订阅 1.99 刀/50 刀额度的 Trial 计划（有效期 7 天），邀请好友最高可享 20% 奖励，多邀多得！</td>
</tr>

<tr>
<td width="180"><a href="https://runapi.host/register?aff=iOKB"><img src="assets/partners/logos/runapi.jpg" alt="RunAPI" width="150"></a></td>
<td>感谢 RunAPI 赞助本项目！RunAPI 是高效稳定的 AI 模型 API 中转平台，一个 API Key 即可访问 OpenAI、Claude、Gemini、DeepSeek、Grok 等 150+ 主流模型，低至 1 折，极其稳定，可以无缝兼容 Claude Code、OpenClaw 等工具。RunAPI为CC switch的用户提供了特别福利，使用<a href="https://runapi.host/register?aff=iOKB">此链接</a>注册并首次充值即可享受10%的优惠折扣！</td>
</tr>

<tr>
<td width="180"><a href="https://www.shengsuanyun.com/?from=CH_4HHXMRYF"><img src="assets/partners/logos/shengsuanyun.png" alt="Shengsuanyun" width="150"></a></td>
<td>感谢胜算云赞助了本项目！胜算云是专为AI Native Teams服务的超级工厂，工业级AI任务并行执行平台，模型商城集采直供聚合接入了Claude、Chatgpt、Gemini等海内外LLM及图片视频多媒体模型算力，绝无逆向掺水、全站模型SLA可用性高达99.7%、<a href="https://watch.shengsuanyun.com/status/shengsuanyun">监测接口</a>日常全绿。更有企业级专属定制网关，实现团队精细化成本与权限管控，智能路由+安全防护+BYOK企业自带密钥托管。平台按量及tokens plan（即将上线）计费，可开票，使用<a href="https://www.shengsuanyun.com/?from=CH_4HHXMRYF">此链接</a>注册新用户可获10元模力及首充10%赠送。</td>
</tr>

<tr>
<td width="180"><a href="https://aigocode.app/invite/CC-SWITCH"><img src="assets/partners/logos/aigocode.png" alt="AIGoCode" width="150"></a></td>
<td>感谢 AIGoCode 赞助了本项目！AIGoCode 是一个集成了 Claude Code、Codex 以及 Gemini 最新模型的一站式平台，为你提供稳定、高效且高性价比的AI编程服务。本站提供灵活的订阅计划，零封号风险，国内直连，无需魔法，极速响应。AIGoCode 为 CC Switch 的用户提供了特别福利，通过<a href="https://aigocode.app/invite/CC-SWITCH">此链接</a>注册的用户首次充值可以获得额外10%奖励额度！</td>
</tr>

<tr>
<td width="180"><a href="https://aicoding.inc/i/CCSWITCH"><img src="assets/partners/logos/aicoding.jpg" alt="AICoding" width="150"></a></td>
<td>感谢 AICoding 赞助了本项目！AICoding —— 全球大模型 API 超值中转服务！Claude Code 1.9 折，GPT 0.1 折，已为数百家企业提供高性价比 AI 服务。支持 Claude Code、GPT、Gemini 及国内主流模型，企业级高并发、极速开票、7×24 专属技术支持，通过<a href="https://aicoding.inc/i/CCSWITCH">此链接</a> 注册的 CC Switch 用户，首充可享受九折优惠！</td>
</tr>

<tr>
<td width="180"><a href="https://subrouter.ai/register?aff=l3ri"><img src="assets/partners/logos/subrouter-banner.png" alt="SubRouter" width="150"></a></td>
<td>感谢 SubRouter 赞助本项目！SubRouter 是面向 AI 服务经营者的公开市场与智能路由平台。商家可快速开通独立经营站，发布套餐、管理用户与模型价格；用户可在市场发现服务，并通过统一 API 获得稳定高效的模型调用。通过<a href="https://subrouter.ai/register?aff=l3ri">此链接</a>注册！</td>
</tr>

<tr>
<td width="180"><a href="https://agentmarket.fluxapay.xyz/marketplace/tokenplans"><img src="assets/partners/logos/fluxa-banner.png" alt="FluxA TokenPlan" width="150"></a></td>
<td>感谢 FluxA &amp; Baidu AI Cloud 赞助了本项目！ FluxA 与百度智能云联合推出 AgenticPlan，为 AI Agent 提供自主购买/管理/使用模型、API、工具的能力。内含百度千帆 TokenPlan，低至6折，可使用 DeepSeek V4、GLM 5.2、Kimi 等旗舰模型，并获赠 FluxA AgentMarket API的调用额度，解锁搜索、数据抓取、社交媒体、金融、加密、生图、视频等 13000+ 付费 API。<br>在用户授权下，AI Agent 还可借助官方的Visa卡支付自主采购资源、管理 API Key、监控用量并规划续费，帮助 Agent 从「自主完成任务」升级为真正能够「自主规划预算，完成任务」。通过<a href="https://agentmarket.fluxapay.xyz/marketplace/tokenplans">此链接</a>购买！</td>
</tr>

<tr>
<td width="180"><a href="https://apikey.fan/register?aff=CCSwitch"><img src="assets/partners/logos/apikey_banner.png" alt="APIKEY.FUN" width="150"></a></td>
<td>感谢 APIKEY.FUN 赞助本项目！APIKEY.FUN 是一家专业的企业级 AI 中转站，致力于为企业和个人开发者提供稳定、高效、低成本的 AI 模型 API 接入服务。平台支持 Claude、OpenAI、Gemini 等主流热门模型，价格低至官方原价的 7%。通过本项目<a href="https://apikey.fan/register?aff=CCSwitch">专属链接</a>注册，还可享受最高 <strong>充值永久 95 折</strong> 专属优惠。</td>
</tr>

<tr>
<td width="180"><a href="https://9527.codes/register?aff=e5zI"><img src="assets/partners/logos/9527-banner.png" alt="9527CODE" width="150"></a></td>
<td>感谢 9527CODE 赞助本项目！9527 CODE 是专注于稳定性与服务质量的企业级满血 AI 中转平台，为 Claude Code、Codex 等主流 AI 模型提供高速、稳定、高质量的中转服务。更重视长期稳定、线路质量与持续可用性。面向企业及高强度 AI 用户，提供稳定、高效的一站式模型接入方案。持续运营一年，服务稳定性达 99.9%，提供 7×24 小时人工技术支持。通过本项目<a href="https://9527.codes/register?aff=e5zI">专属链接</a>注册，新人福利联系客服赠送体验额度，拉新返现无上限！</td>
</tr>

<tr>
<td width="180"><a href="https://console.apito.ai/agent/register/pQBql2buaqiX3dDS"><img src="assets/partners/logos/claudeapi.png" alt="ClaudeAPI" width="150"></a></td>
<td>本项目由 <a href="https://console.apito.ai/agent/register/pQBql2buaqiX3dDS">Claude API</a> 赞助。Claude API 直连，三分钟接入 Claude Code 与 Agent 应用 新用户可领取测试额度。基于 Anthropic 官方 Key + AWS Bedrock 官方渠道，非逆向、非降智，支持 Opus / Sonnet / Haiku 全系列模型，保留 Tool Use、1M 上下文等官方能力。适合 Claude Code 深度用户、Agent 工程师与企业技术团队，支持开票和团队对接。点击<a href="https://console.apito.ai/agent/register/pQBql2buaqiX3dDS">这里</a>注册！</td>
</tr>

<tr>
<td width="180"><a href="https://code0.ai/agent/register/B2XHxGjGmRvqgznY"><img src="assets/partners/logos/code0.png" alt="code0.ai" width="150"></a></td>
<td>感谢 <a href="https://code0.ai/agent/register/B2XHxGjGmRvqgznY">code0.ai</a> 赞助本项目！code0.ai 是专为开发者打造的 AI 编程服务平台，支持 Claude Code、Codex、Gemini 等主流 AI 编程能力，帮助个人开发者和团队更稳定、更高效地使用 AI Agent 完成代码开发、调试与自动化任务。ccswitch 用户可通过 <a href="https://code0.ai/agent/register/B2XHxGjGmRvqgznY">code0.ai 官网</a> 联系客服领取测试额度，体验高效稳定的 AI 编程服务！</td>
</tr>

<tr>
<td width="180"><a href="https://teamorouter.cn/zh?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory"><img src="assets/partners/logos/TeamoRouter-banner.png" alt="TeamoRouter" width="150"></a></td>
<td>感谢 TeamoRouter 赞助本项目！TeamoRouter 是一款面向开发者、AI 团队和企业的企业级 Agentic LLM 网关。无需任何订阅，你就可以通过一个统一 API 访问 Claude Code、Codex、Gemini CLI、OpenAI Codex 以及其他热门 AI Agent，同时享受最高可达 90% 折扣的 API 价格。
不同于常见的 API 中转服务，TeamoRouter 聚合了数百家官方模型提供商和可信基础设施合作伙伴，包括 OpenAI、Anthropic、Vertex、Azure 和 AWS Bedrock。每个提供商都经过验证，确保 100% 兼容 Agent 协议，并具备可靠的缓存性能和请求可追踪性，从而提供稳定质量，而不是反向工程或缩水后的接口。平台提供接近官方水平的 TTFT、99.6% SLA、最高 5,000 QPM 的企业级吞吐量，以及行业领先的缓存命中率，可大幅降低长时间运行的 Agent 工作流中的 token 成本。
TeamoRouter 还提供企业级功能，包括集中账单、团队管理、BYOK、智能路由、用量分析、动态提供商优化和专属支持。为了获得更简单的使用体验，Teamo Desktop 支持你一键使用 Claude Code、Codex、Gemini CLI 和其他热门 AI Agent，无需管理 API Key，也无需手动配置网关。新用户通过<a href="https://teamorouter.cn/zh?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory">此链接</a>注册，首次充值可享受 10% 折扣。</td>
</tr>

<tr>
<td width="180"><a href="https://ppio.com/activity/ccswitch"><img src="assets/partners/logos/ppio-banner.png" alt="PPIO" width="150"></a></td>
<td>感谢 PPIO 赞助了本项目！PPIO 是国内领先的独立 Agentic Cloud 服务商，由 PPTV 创始人姚欣和前 PPTV 首席架构师王闻宇于 2018 年联合创立。PPIO 支持一个 API key 调用 DeepSeek-V4-Flash、Kimi-K3、GLM-5.2、MiniMax-M3 等所有旗舰开源模型；企业 Token Plan 最低 6 折接入，支持 200 席位；Fusion 融合模型以 1/10 的价格比肩 Fable5。通过<a href="https://ppio.com/activity/ccswitch">此链接</a>注册并完成实名认证，即可获得 ¥10 元代金券，邀请好友充值还可获得最高 15% 充值返利。</td>
</tr>

<tr>
<td width="180"><a href="https://www.newapi.ai/"><img src="assets/partners/logos/newapi-banner.png" alt="new-api" width="150"></a></td>
<td>感谢开源 AI 基础设施项目 <a href="https://www.newapi.ai/">new-api</a> 对本项目的鼎力支持！new-api 是由 QuantumNous（锟腾科技）推出的开源 AI 基础设施项目，也是活跃度与使用规模领先的大模型统一接入与分发项目之一，专注于帮助开发者、团队和企业以更低成本构建可管理、可扩展的 AI 服务平台。作为同样扎根开源生态的项目，new-api 希望通过赞助支持更多优秀开源项目持续发展。🌟 欢迎 Star 支持 new-api：<a href="https://github.com/QuantumNous/new-api">https://github.com/QuantumNous/new-api</a>，官网：<a href="https://www.newapi.ai/">https://www.newapi.ai/</a>。</td>
</tr>

<tr>
<td width="180"><a href="https://claudecn.ai/register?aff=HEL9"><img src="assets/partners/logos/claudecn.jpg" alt="ClaudeCN" width="150"></a></td>
<td>感谢 ClaudeCN 赞助本项目！ClaudeCN 由是一家实体企业运营的企业级AI中转平台。平台可提供高可用性的商用API服务，提供Claude、GPT、Deepseek等热门模型，支持企业采购流程，可对公打款、签约，服务合规有保障。点击<a href="https://claudecn.ai/register?aff=HEL9">此链接</a>注册！</td>
</tr>

<tr>
<td width="180"><a href="https://www.volcengine.com/activity/ai618?utm_campaign=hw&utm_content=hw&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch"><img src="assets/partners/logos/huoshan.png" alt="HuoShan" width="150"></a></td>
<td>感谢火山方舟 Agent Plan 模型赞助了本项目！方舟 Agent Plan 模型订阅套餐集成了包含 Doubao-Seed、Doubao-Seedance、Doubao-Seedream 等在内的字节跳动自研 SOTA 级模型，覆盖文本、代码、图像、视频等多模态任务。最新支持 MiniMax-M3、DeepSeek-V4 系列、GLM-5.1、Doubao-Seed-2.0 系列、Kimi-K2.6 等模型，工具不限。超全模态模型与 Harness 升级一步到位，深度支持 Agent 框架与 AI 编程工具。一次订阅，可以为不同任务切换合适的 AI 引擎。方舟 Coding Plan 为 CC Switch 的用户提供了专属福利：通过<a href="https://www.volcengine.com/activity/ai618?utm_campaign=hw&utm_content=hw&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch">此链接</a>订阅方舟 Coding Plan，新客户首两个月享 2.5 折优惠，再用专属邀请码 6J6FV5N2 领取奖励叠加 9.5 折，低至 9.4 元/月！<a href="https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch">>>For developers outside Mainland China, please click here</a></td>
</tr>

<tr>
<td width="180"><a href="https://cloud.siliconflow.cn/i/YflgU2Ve"><img src="assets/partners/logos/silicon_zh.jpg" alt="SiliconFlow" width="150"></a></td>
<td>感谢硅基流动赞助了本项目！硅基流动是一个高性能 AI 基础设施与模型 API 平台，一站式提供语言、语音、图像、视频等多模态模型的快速、可靠访问。平台支持按量计费、丰富的多模态模型选择、高速推理和企业级稳定性，帮助开发者和团队更高效地构建和扩展 AI 应用。通过<a href="https://cloud.siliconflow.cn/i/YflgU2Ve">此链接</a>注册并完成实名认证，即可获得 ¥16 奖励金，可在平台内跨模型使用。硅基流动现已兼容 OpenClaw，用户可接入硅基流动 API Key 免费调用主流 AI 模型。</td>
</tr>

<tr>
<td width="180"><a href="https://a6api.com/register?aff=AqNr"><img src="assets/partners/logos/a6-banner-zh.jpg" alt="A6API" width="150"></a></td>
<td>感谢 <a href="https://a6api.com/register?aff=AqNr">A6API</a> 赞助本项目！A6API 是一站式 AI 模型 API 聚合平台，覆盖 Claude、GPT、Gemini、Codex 等主流模型，支持多商家入驻供货，同一个模型可由多个上游商家竞争报价。平台通过智能路由自动优选更稳定、更低价的可用线路，并支持失败自动切换，帮助用户减少请求失败、降低调用成本、提升使用稳定性。无论你是开发者、AI 产品团队还是工作室，都可以通过统一接口快速接入，兼容所有格式，迁移成本低，使用更省心。新用户通过 <a href="https://a6api.com/register?aff=AqNr">此链接</a> 注册即可获得免费体验额度，先试再用，低价开用。</td>
</tr>

<tr>
<td width="180"><a href="https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch"><img src="assets/partners/logos/ucloud.png" alt="优云智算" width="150"></a></td>
<td>感谢优云智算赞助了本项目！优云智算是UCloud旗下AI云平台，提供稳定、全面的国内外模型API，仅一个key即可调用。主打包月、按次的高性价比 国模Coding Plan套餐，同时提供官转稳定海外模型。支持接入 Claude Code、Codex 及 API 调用。支持企业高并发、7*24技术支持、自助开票。通过<a href="https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch">此链接</a>注册的用户，可得免费5元平台体验金！</td>
</tr>

<tr>
<td width="180"><a href="https://www.ccsub.net/register?ref=Y6Z8DXEA"><img src="assets/partners/logos/ccsub.svg" alt="CCSub" width="150"></a></td>
<td>感谢 CCSub 赞助本项目！CCSub 是稳定、实惠的 AI API 中转平台，是 Claude Code 官方订阅的超强平替。一个 API Key 即可调用 Claude Opus 4.8、Sonnet 4.6、Haiku 4.5、GPT-5、Gemini、DeepSeek 全系列模型，价格约为官方直连的 1/3，全球直连无需梯子。兼容 Claude Code、Codex、Cursor、Cline、Continue、Windsurf 等所有主流 AI 编程工具。通过<a href="https://www.ccsub.net/register?ref=Y6Z8DXEA">此链接</a>注册即送 $5 体验额度！</td>
</tr>

<tr>
<td width="180"><a href="https://sssaicodeapi.com/register?ref=DCP0SM"><img src="assets/partners/logos/sssaicode.png" alt="SSSAiCode" width="150"></a></td>
<td>感谢 SSSAiCode 赞助了本项目！SSSAiCode 是一家稳定可靠的API中转站，致力于提供稳定、可靠、平价的Claude、CodeX模型服务，支持当日快速开票，SSSAiCode为本软件的用户提供特别优惠，使用<a href="https://sssaicodeapi.com/register?ref=DCP0SM">此链接</a>注册每次充值均可享受10$的额外奖励！</td>
</tr>

<tr>
<td width="180"><a href="https://soleapi.com/r/ccswitch"><img src="assets/partners/logos/soleapi-banner.png" alt="SoleAPI" width="150"></a></td>
<td>感谢 SoleAPI 对本项目的支持！SoleAPI 是一个面向开发者与企业的 AI 模型网关，一个 API Key 直达 Claude、GPT、Gemini 等 30+ 主流模型。原生兼容 OpenAI 与 Anthropic 协议，只需替换 Base URL 即可无缝接入 Claude Code、Codex、Cursor 等编码工具与现有 SDK。平台基于延迟与健康度实时智能选路，上游异常毫秒级自动切换备用渠道，服务可用性 99.99%，并提供 7×24 小时技术支持；所有调用一张账单，逐请求的 Token 与费用透明可查。通过本项目<a href="https://soleapi.com/r/ccswitch">专属链接</a>注册即送体验额度，邀请好友充值可持续获得返现奖励！</td>
</tr>

<tr>
<td width="180"><a href="https://www.micuapi.ai/register?aff=aOYQ"><img src="assets/partners/logos/mikubanner.svg" alt="Micu" width="150"></a></td>
<td>感谢 米醋API 赞助了本项目！米醋API 是一家致力于提供极致性价比与高稳定性的全球大模型中转服务商。米醋API 背后有实体企业做核心保障，杜绝跑路风险，支持极速正规开票！我们主打“试错零成本”：1 元起充低门槛，0 手续费随时退款！米醋API 为本软件的用户提供了特别优惠，使用<a href="https://www.micuapi.ai/register?aff=aOYQ">此链接</a>注册并在充值时填写"ccswitch"优惠码可享九折优惠！</td>
</tr>

<tr>
<td width="180"><a href="https://www.rightapi.ai/register?aff=CCSWITCH"><img src="assets/partners/logos/rightcode.jpg" alt="RightCode" width="150"></a></td>
<td>感谢 Right Code 赞助了本项目！Right Code 稳定提供 Claude Code、Codex、Gemini 等模型的中转服务，并可选按量、包月两种计费模式。充值即可开票，企业、团队用户一对一对接。同时为 CC Switch 的用户提供了特别优惠：通过<a href="https://www.rightapi.ai/register?aff=CCSWITCH">此链接</a>注册，每次充值均可获得实付金额5%的按量额度！</td>
</tr>

<tr>
<td width="180"><a href="https://etok.ai"><img src="assets/partners/logos/etok.png" alt="ETok" width="150"></a></td>
<td>感谢 ETok.ai 赞助了本项目！ETok.ai 致力于打造一站式 AI 编程工具服务平台。我们提供 Claude Code 专业套餐及技术社群服务，同时支持 Google Gemini 和 OpenAI Codex。通过精心设计的套餐方案和专业的技术社群，为开发者提供稳定的服务保障和持续的技术支持，让 AI 辅助编程真正成为开发者的生产力工具。点击<a href="https://etok.ai">这里</a>注册！</td>
</tr>

<tr>
<td width="180"><a href="https://cubence.com/signup?code=CCSWITCH&source=ccs"><img src="assets/partners/logos/cubence.png" alt="Cubence" width="150"></a></td>
<td>感谢 Cubence 赞助本项目！Cubence 是一家可靠高效的 API 中继服务提供商，提供对 Claude Code、Codex、Gemini 等模型的中继服务，并提供按量、包月等灵活的计费方式。Cubence 为 CC Switch 的用户提供了特别优惠：使用 <a href="https://cubence.com/signup?code=CCSWITCH&source=ccs">此链接</a> 注册，并在充值时输入 "CCSWITCH" 优惠码，每次充值均可享受九折优惠！</td>
</tr>

<tr>
<td width="180"><a href="https://crazyrouter.com/register?aff=OZcm&ref=cc-switch"><img src="assets/partners/logos/crazyrouter.png" alt="Crazyrouter" width="150"></a></td>
<td>感谢 Crazyrouter 赞助了本项目！Crazyrouter 是一个高性能 AI API 聚合平台——一个 API Key 即可访问 300+ 模型，包括 Claude Code、Codex、Gemini CLI 等。全部模型低至官方定价的 55%，支持自动故障转移、智能路由和无限并发。Crazyrouter 为 CC Switch 用户提供了专属优惠：通过<a href="https://crazyrouter.com/register?aff=OZcm&ref=cc-switch">此链接</a>注册后联系客服即可领取 <strong>$2 免费额度</strong>，首次充值时输入优惠码 `CCSWITCH` 还可获得额外 <strong>30% 奖励额度</strong>！</td>
</tr>

<tr>
<td width="180"><a href="https://www.dmxapi.cn/register?aff=bUHu"><img src="assets/partners/logos/dmx-zh.jpeg" alt="DMXAPI" width="150"></a></td>
<td>感谢 DMXAPI（大模型API）赞助了本项目！ DMXAPI，一个Key用全球大模型。
为200多家企业用户提供全球大模型API服务。· 充值即开票 ·当天开票 ·并发不限制  ·1元起充 ·  7x24 在线技术辅导，GPT/Claude/Gemini全部6.8折，国内模型5~8折，Claude Code 专属模型3.4折进行中！<a href="https://www.dmxapi.cn/register?aff=bUHu">点击这里注册</a></td>
</tr>

</table>

</details>

## 为什么选择 CC Switch？

Claude Code、Codex、Gemini CLI 等 AI 编程工具各有各的配置格式。换一个 API 供应商，就得手动改 JSON、TOML、YAML 或 `.env` 文件；MCP、Skills 和提示词也要在每个工具里分别维护。

**CC Switch** 把这些工作集中到一个桌面应用里：选一个预设、填入 Key，一键即可切换，原有配置不会丢失。

- **一个应用，十个工具** — Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build、OpenCode、OpenClaw、Hermes、Pi、MiniMax Code
- **告别手动编辑** — 90+ 供应商预设，包括 AWS Bedrock、NVIDIA NIM 和社区中转服务
- **在 Claude Code 里用 GPT，在 Codex 里用 Claude** — 内置本地路由，自动转换 Anthropic、OpenAI、Gemini 的接口格式，并支持自动故障转移
- **MCP、Skills 与提示词集中管理** — MCP 和 Skills 添加一次，按工具勾选同步；提示词按工具分别维护
- **用量与额度一目了然** — 不开本地路由也能统计 Token 用量和花费，供应商卡片和托盘上直接显示订阅额度与余额
- **跨平台** — 基于 Tauri 2 构建的原生桌面应用，支持 Windows、macOS 和 Linux

## 界面预览

|                  主界面                   |                  添加供应商                  |
| :---------------------------------------: | :------------------------------------------: |
| ![主界面](assets/screenshots/main-zh.png) | ![添加供应商](assets/screenshots/add-zh.png) |

## 下载安装

### 系统要求

- **Windows**：Windows 10 及以上
- **macOS**：macOS 12 (Monterey) 及以上
- **Linux**：x86_64 或 ARM64，需要 glibc 2.35+ 和 WebKitGTK 4.1，例如 Ubuntu 22.04+、Debian 12+ 及较新的 Fedora；RHEL / Rocky / Alma 8–9 暂不支持

### Windows 用户

从 [Releases](../../releases) 页面下载最新版本的 `CC-Switch-v{版本号}-Windows.msi` 安装包或 `CC-Switch-v{版本号}-Windows-Portable.zip` 绿色版。ARM 版 Windows 请下载 `CC-Switch-v{版本号}-Windows-arm64.msi` 或 `CC-Switch-v{版本号}-Windows-arm64-Portable.zip`。

### macOS 用户

**方式一：通过 Homebrew 安装（推荐）**

```bash
brew install --cask cc-switch
```

更新：

```bash
brew upgrade --cask cc-switch
```

**方式二：手动下载**

从 [Releases](../../releases) 页面下载 `CC-Switch-v{版本号}-macOS.dmg`（推荐）或 `.zip`。这是 Universal 通用包，Apple Silicon 和 Intel Mac 均可原生运行。

> **注意**：CC Switch macOS 版本已通过 Apple 代码签名和公证，可直接安装打开。

### Arch Linux 用户

**通过 paru 安装（推荐）**

```bash
paru -S cc-switch-bin
```

### Linux 用户

从 [Releases](../../releases) 页面下载最新版本的 Linux 安装包：

- `CC-Switch-v{版本号}-Linux-x86_64.deb` / `-Linux-arm64.deb`（Debian/Ubuntu）
- `CC-Switch-v{版本号}-Linux-x86_64.rpm` / `-Linux-arm64.rpm`（Fedora 等提供 WebKitGTK 4.1 的 RPM 发行版）
- `CC-Switch-v{版本号}-Linux-x86_64.AppImage` / `-Linux-arm64.AppImage`（满足上述系统要求的发行版）

> **Flatpak**：官方 Release 不包含 Flatpak 包。如需使用，可从 `.deb` 自行构建 — 参见 [`flatpak/README.md`](flatpak/README.md)。

## 快速开始

### 基本使用

1. **添加供应商**：点击工具栏的“添加新供应商”（+ 按钮）→ 选择预设或创建自定义配置
2. **切换供应商**：
   - 主界面：选择供应商 → 点击“启用”（OpenCode、OpenClaw、Hermes、MiniMax Code 的按钮为“添加”；这四个工具和 Pi 是共存式工具，可以同时添加多个供应商）
   - 系统托盘：直接点击供应商名称（支持 Claude Code、Codex、Gemini CLI、Grok Build）
3. **生效方式**：Claude Code 无需重启；Codex、Gemini CLI、Grok Build 需重启终端或对应的 CLI 工具；Claude Desktop 需重启应用本身（详见常见问题）
4. **恢复官方登录**：切换到列表中自带的官方供应商（如“Claude Official”），重启工具后按照其登录/OAuth 流程操作
5. **本地路由（可选）**：想在 Claude Code 里使用 OpenAI 或 Gemini 格式的供应商，或在 Codex 里使用 Claude，需要开启本地路由。做法是在「设置 → 路由 → 本地路由」里打开“路由总开关”，再在“路由启用”里打开对应的工具。想在主页顶部直接开关，可以打开“在主页面显示本地路由开关”

### MCP、提示词、Skills、项目与会话

- **MCP**：点击“MCP 管理”按钮 → 通过模板或自定义配置添加服务器（或“导入已有”）→ 切换各工具的同步开关
- **提示词**：点击“提示词” → 使用 Markdown 编辑器创建提示词 → 启用后写入该工具的提示词文件
- **Skills**：点击“Skills” →“发现技能” → 搜索 skills.sh 或浏览 GitHub 仓库 → 一键安装到支持的工具
- **项目**：在 Claude Code、Claude Desktop 或 Codex 页面，打开主页顶部的项目切换器 →“新建项目”，把当前配置保存下来，之后从切换器里选择即可整套切换
- **会话**：点击“会话管理” → 浏览、搜索和恢复各工具的会话历史

> **注意**：首次启动时，CC Switch 会自动把 Claude Code、Codex、Gemini CLI、Grok Build 的现有配置导入为名为 `default` 的供应商，并为这几个工具和 Claude Desktop 添加官方供应商，原有配置不会丢失。

各项功能的详细用法请查阅 **[用户手册](docs/user-manual/zh/README.md)**，涵盖供应商管理、MCP/提示词/Skills、本地路由与故障转移等全部功能。

## 功能特性

[完整更新日志](CHANGELOG.md) | [发布说明](docs/release-notes/v3.20.4-zh.md)

### 各工具支持的功能

| 工具 | 供应商 | 本地路由 | 托盘切换 | MCP | Skills | 提示词 | 会话 | 用量统计 |
| --- | --- | :---: | :---: | :---: | :---: | --- | :---: | :---: |
| Claude Code | 切换 | ✓ | ✓ | ✓ | ✓ | CLAUDE.md | ✓ | ✓ |
| Claude Desktop | 切换 | 模型映射时 | – | – | – | – | – | 模型映射时 |
| Codex | 切换 | ✓ | ✓ | ✓ | ✓ | AGENTS.md | ✓ | ✓ |
| Gemini CLI | 切换 | ✓ | ✓ | ✓ | ✓ | GEMINI.md | ✓ | ✓ |
| Grok Build | 切换 | ✓ | ✓ | ✓ | ✓ | AGENTS.md | ✓ | ✓ |
| OpenCode | 共存 | – | – | ✓ | ✓ | AGENTS.md | ✓ | ✓ |
| OpenClaw | 共存 | – | – | – | – | 工作区编辑器 | ✓ | – |
| Hermes | 共存 | – | – | ✓ | ✓ | 记忆管理 | ✓ | – |
| Pi | 共存 | – | – | – | ✓ | AGENTS.md、SYSTEM.md、提示词模板 | ✓ | ✓ |
| MiniMax Code | 共存 | – | – | ✓ | ✓ | AGENTS.md | ✓ | ✓ |

- **切换**：同一时间只启用一个供应商；**共存**：多个供应商同时写入工具自身的配置，在工具里选择使用。
- **本地路由**：由 CC Switch 在本机转发请求并转换接口格式，见下方[本地路由与故障转移](#本地路由与故障转移)。Claude Desktop 的供应商可选“直连”或“模型映射”，选“模型映射”时经本地路由转发。
- **会话**：浏览、搜索会话历史，复制恢复命令继续对话（OpenClaw、Hermes 的会话暂不支持恢复）。Hermes 的会话需要在会话管理里选择“全部”查看。
- **用量统计**：不开本地路由时，从各工具的本地会话记录统计；经本地路由的请求也会计入。
- 在 Claude Desktop 页面打开的 MCP、Skills、提示词和会话面板，作用于 Claude Code。

### 供应商管理

- **90+ 供应商预设** — 选择预设、填入 Key 即可添加，也可以创建自定义配置
- **项目** — 把 Claude Code 或 Codex 当前的供应商、MCP、Skills 和提示词文件保存为一个项目（Claude Desktop 只保存供应商），之后在主页顶部的项目切换器或托盘里一键整套切换；切到其他项目时，当前状态会自动存回原项目
- **OAuth 认证中心（Beta）** — 在「设置 → 认证」里登录多个 GitHub Copilot、ChatGPT、xAI（Grok）账号，把订阅当作供应商用在 Claude Code、Claude Desktop 和 Codex 中（除 Codex 的 OpenAI Official 外，都需要开启本地路由）。在官方客户端以外使用订阅可能违反厂商的服务条款，请自行评估风险
- **Claude Desktop 接入第三方** — 可以直连 Anthropic 兼容端点；非 Claude 模型选“模型映射”，经本地路由把 Sonnet、Opus、Haiku 等档位映射到供应商的实际模型
- **通用供应商** — 一份配置同步到 Claude Code、Codex 和 Gemini CLI
- 一键切换、系统托盘快速切换（Claude Code、Codex、Gemini CLI、Grok Build）、拖拽排序、导入导出

### 本地路由与故障转移

- **接口格式转换** — 本地路由在 Anthropic Messages、OpenAI Chat Completions、OpenAI Responses 和 Gemini Native 之间转换请求格式：Claude Code 和 Claude Desktop 可以使用 OpenAI 或 Gemini 格式的供应商，Codex 和 Grok Build 可以使用 Chat Completions 或 Anthropic Messages 格式的供应商
- **按工具开启** — Claude Code、Codex、Gemini CLI、Grok Build 可以分别开启本地路由；开启后，切换供应商会立即作用于后续请求（如果切换改变了模型，Codex 和 Grok Build 仍可能需要重启）
- **自动故障转移** — 为每个工具配置故障转移队列，请求失败时按队列顺序自动改用下一个供应商，配合熔断器和供应商健康监控
- **整流器** — 自动修正部分上游不兼容的请求（如 Thinking 签名、不支持图片时降级）
- 官方供应商（如 Claude Official）不能走本地路由（Codex 的 OpenAI Official 除外）
- 使用攻略：[在 Claude Code 中使用 GPT](docs/guides/claude-codex-routing-guide-zh.md) · [在 Codex 中使用 Claude](docs/guides/codex-claude-routing-guide-zh.md)

### MCP、提示词与 Skills

- **统一 MCP 面板** — 一处管理所有 MCP 服务器，按工具勾选同步，支持从各工具导入现有配置，支持 Deep Link 导入
- **提示词** — 按工具分别管理的提示词库，使用 Markdown 编辑器；启用后写入该工具的提示词文件（CLAUDE.md / AGENTS.md / GEMINI.md），启用前会先把文件里原有的内容存回提示词库，不会丢失。Pi 还可以编辑 SYSTEM.md、APPEND_SYSTEM.md 和提示词模板
- **Skills** — 搜索 skills.sh，或从 GitHub 仓库、ZIP 文件一键安装；检查更新并一键全部更新；通过软链接或文件复制同步到各工具，存储位置可选 `~/.agents/skills`
- 三个面板都支持搜索，MCP 和 Skills 还可以按工具一键全部启用或停用

### 用量与成本追踪

- **用量仪表盘** — 不开本地路由也能统计：默认自动扫描各工具的本地会话记录，按供应商和模型统计请求数、Token、缓存命中率和花费，提供趋势图和逐条请求日志
- **额度与余额** — 供应商卡片和托盘上直接显示官方订阅额度（Claude、ChatGPT、Gemini、SuperGrok）、Coding Plan 的 5 小时 / 周 / 月额度（Kimi、智谱 GLM、MiniMax、火山方舟等）和账户余额（DeepSeek、OpenRouter、硅基流动等），部分需要先在供应商卡片的“配置用量查询”里开启；其他供应商可以写自定义用量脚本
- **自定义定价** — 按模型设置单价，可以从 models.dev 导入

### 会话管理器与工作区

- **会话管理器** — 浏览、搜索各工具的会话历史，复制恢复命令继续对话；macOS 上可以一键在终端中恢复
- **工作区编辑器**（OpenClaw）— 编辑 Agent 文件（AGENTS.md、SOUL.md 等）和每日记忆
- **记忆管理**（Hermes）— 编辑 Hermes 的 MEMORY.md 和 USER.md

### 系统与平台

- **云同步** — 通过 WebDAV（坚果云、Nextcloud、群晖 NAS 等）或 S3 兼容存储（AWS S3、Cloudflare R2、阿里云 OSS、腾讯云 COS 等）在多台设备之间同步；也可以把 CC Switch 配置目录放到 Dropbox、OneDrive、iCloud 等网盘文件夹中
- **CLI 工具管理** — 在「关于」页查看 Claude Code、Codex 等命令行工具的当前版本和最新版本，一键安装、升级或全部升级，并诊断重复安装；Windows 上还能管理 WSL 里的工具（见常见问题）
- **Deep Link**（`ccswitch://`）— 通过链接一键导入供应商、MCP 服务器和提示词，或添加技能仓库
- **小工具** — 跳过 Claude Code 初次安装确认、隐藏 AI 署名、让 VS Code 的 Claude Code 插件随本软件切换供应商等
- 深色 / 浅色 / 跟随系统主题、开机自启、自动更新、原子写入、自动备份、国际化（简中/繁中/英/日）

## 常见问题

<details>
<summary><strong>CC Switch 支持哪些 AI 工具？</strong></summary>

CC Switch 支持十个工具：**Claude Code**、**Claude Desktop**、**Codex**、**Gemini CLI**、**Grok Build**、**OpenCode**、**OpenClaw**、**Hermes**、**Pi**、**MiniMax Code**。每个工具都有专属的供应商预设和配置管理，各自支持哪些功能见[各工具支持的功能](#各工具支持的功能)。

</details>

<details>
<summary><strong>切换供应商后需要重启终端吗？</strong></summary>

视工具而定：

- **Claude Code**：支持供应商数据的热切换，无需重启。
- **Codex、Gemini CLI、Grok Build**：需要重启终端或 CLI 工具才能生效（Codex 和 Grok Build 切换成功后会有提示）。开启本地路由后，请求会立即转发到新供应商；但如果切换改变了模型，Codex 和 Grok Build 仍可能需要重启。
- **Claude Desktop**：需要完全退出并重新打开 Claude Desktop；使用“模型映射”时，还需要保持 CC Switch 运行。
- **OpenCode、OpenClaw、Hermes、Pi、MiniMax Code**：这些是共存式工具，点击“添加”（Pi 为“启用”）会把供应商写入工具自身的配置、与其他供应商共存，之后在工具里选择要使用的模型即可。

</details>

<details>
<summary><strong>切换供应商之后我的插件配置怎么不见了？</strong></summary>

CC Switch 使用“通用配置片段”功能（Claude Code、Codex、Gemini CLI 均支持），在不同的供应商之间共享 Key、请求地址和模型之外的配置，例如插件、Hook、环境变量。编辑供应商时点击“编辑通用配置”→“从编辑内容提取”，即可把这些通用部分保存到通用配置片段中；新建供应商时保持勾选“应用通用配置”（默认勾选），片段就会合并到新的供应商配置中。对于勾选了“应用通用配置”的 Claude Code 和 Codex 供应商，切走时 CC Switch 会自动从当前配置重新提取通用部分，你在工具里新装的插件会自动带到下一个供应商。你的原始配置都保存在第一次运行本软件时导入的默认供应商里，不会丢失。

</details>

<details>
<summary><strong>为什么总有一个正在激活中的供应商无法删除？</strong></summary>

本软件的设计原则是“最小侵入性”，即使卸载本软件，也不会影响应用的正常使用。

所以对于同一时间只启用一个供应商的工具（Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build），系统总会保留一个正在激活中的配置，因为如果将所有配置全部删除，该应用将无法正常使用。OpenCode、OpenClaw、Hermes、Pi、MiniMax Code 等共存式工具不受此限制，可以直接删除任意供应商。如果你不常用某个工具，可以在设置中关掉它的显示。如果你想切换回官方登录，可以参考下条。

</details>

<details>
<summary><strong>如何切换回官方登录？</strong></summary>

Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build 的供应商列表里都自带一个官方供应商（**Claude Official**、**Claude Desktop Official**、**OpenAI Official**、**Google Official**、**Grok Official**），如果删掉了，可以从预设里重新添加。切换到官方供应商后，按照工具自身的登录流程操作（如 Claude Code 的 `/login`、Codex 的 `codex login`），之后便可以在官方供应商和第三方供应商之间随意切换。

Codex 还可以在 CC Switch 里用“使用 ChatGPT 登录”登录多个 ChatGPT 账号，再为每张 **OpenAI Official** 卡片选择“使用的账号”，多个 Plus、Pro 或 Team 账号之间一键切换；选择“跟随 Codex 登录”的卡片则沿用 Codex CLI 自己的登录。

注意：开启本地路由时不能切换到官方供应商，Codex 的 OpenAI Official 卡片除外。

</details>

<details>
<summary><strong>开启本地路由后，配置文件里的地址为什么变成了 127.0.0.1？</strong></summary>

开启本地路由后，工具的请求会先发到 CC Switch 的本地路由（默认 `http://127.0.0.1:15721`），再由 CC Switch 转发给你选中的供应商。所以工具的配置文件里只有本地地址和占位密钥 `PROXY_MANAGED`；Claude Code 的模型名还会写成 `claude-sonnet-5` 之类的固定别名（`/model` 菜单里仍显示真实模型名）。真实的供应商地址、密钥和模型都保存在 CC Switch 里。

在「设置 → 使用统计 → 请求日志」里可以看到每条请求的“请求模型 → 实际模型”。关闭本地路由后，配置文件会恢复为当前供应商的真实配置。

</details>

<details>
<summary><strong>能在 Claude Code 里使用 OpenAI 兼容接口、Gemini 或本地模型吗？</strong></summary>

可以，但需要开启本地路由。编辑供应商时，在“高级选项”的“上游格式”里选择和供应商一致的接口格式：只提供 Chat Completions 接口的服务（很多本地模型服务都是这样）选“OpenAI Chat Completions”，提供 Responses 接口的选“OpenAI Responses API”，Gemini 选“Gemini Native generateContent”。然后按[快速开始](#快速开始)第 5 步为 Claude Code 开启本地路由。格式选错或没有开启本地路由，通常会报 404 或 405 错误。

反过来，在 Codex 或 Grok Build 的“上游格式”里选“Anthropic Messages”，就能使用 Claude 格式的供应商，同样需要开启本地路由。详见[在 Claude Code 中使用 GPT](docs/guides/claude-codex-routing-guide-zh.md) 和 [在 Codex 中使用 Claude](docs/guides/codex-claude-routing-guide-zh.md)。

</details>

<details>
<summary><strong>“检测连通”通过了，为什么请求还是失败？</strong></summary>

供应商卡片上的“检测连通”只检查供应商地址能不能连上，不会发送真实的模型请求，所以验证不了 API Key 和模型名是否正确。请求失败时，请检查 Key、模型名和上游格式；开启本地路由时，还可以在「设置 → 使用统计 → 请求日志」里查看具体报错。

</details>

<details>
<summary><strong>我的数据存储在哪里？</strong></summary>

默认都在用户主目录下的 `.cc-switch` 文件夹（Windows 为 `C:\Users\<用户名>\.cc-switch`）：

- **数据库**：`cc-switch.db`（SQLite — 供应商、MCP、提示词、Skills、项目、用量记录等）
- **本地设置**：`settings.json`（设备级设置，如各工具的配置目录、备份策略、云同步连接信息）
- **备份**：`backups/`（默认每 24 小时自动备份一次、保留最近 10 个，可在「设置 → 高级 → 备份与恢复」中调整）
- **Skills**：`skills/`（可在设置中改为 `~/.agents/skills`），默认通过软链接同步到各工具，失败时改为复制
- **技能备份**：`skill-backups/`（卸载或更新技能前自动创建，保留最近 20 个）
- **OAuth 登录凭据**：`copilot_auth.json`、`codex_oauth_auth.json`、`xai_oauth_auth.json`
- **日志**：`logs/cc-switch.log` 和 `crash.log`，反馈问题时请附上

在「设置 → 高级 → 配置文件目录」里修改“CC Switch 配置目录”后，除 `settings.json` 以外的上述文件都改为存放在新目录。CC Switch 不会自动搬运已有文件，需要先手动复制过去。

</details>

<details>
<summary><strong>在 Windows 上怎么管理 WSL 里的工具？</strong></summary>

CC Switch 不会自动识别 WSL。请在「设置 → 高级 → 配置文件目录 → 配置目录覆盖（高级）」里，把对应工具的目录改成 WSL 里的路径，例如 `\\wsl.localhost\Ubuntu\home\<用户名>\.claude`，保存后 CC Switch 就会读写 WSL 里的配置（Claude Code、Codex、Gemini CLI、Grok Build、OpenCode、OpenClaw、Hermes、Pi 支持设置）。设置之后，「关于」页也会在对应的 WSL 发行版里检测和升级该工具。

注意：本地路由写入配置的地址是 `127.0.0.1`。WSL2 默认的 NAT 网络模式下，WSL 里的 `127.0.0.1` 连不到 Windows 上的本地路由，需要改用 WSL 的 mirrored 网络模式。

</details>

<details>
<summary><strong>有命令行版本或无界面版本吗？</strong></summary>

CC Switch 本身只提供需要图形界面的桌面版（系统要求见[下载安装](#下载安装)）。在服务器、SSH 远程或没有桌面环境的机器上，推荐使用社区维护的 **[CC Switch CLI](https://github.com/SaladDay/cc-switch-cli)**：它提供交互式终端界面（TUI）和命令行两种用法，支持 Claude Code、Codex、Gemini CLI、OpenCode、OpenClaw、Hermes、Pi，可以通过 Homebrew（`brew install cc-switch-cli`）或安装脚本安装。

CC Switch CLI 默认与桌面版共用数据目录 `~/.cc-switch`，也兼容桌面版的 WebDAV 同步。两个项目分别发版，CLI 版支持的数据库版本有时会落后于桌面版；遇到“数据库版本过新”的提示时，请升级 CLI 版，或等它跟进更新。

</details>

<details>
<summary><strong>Linux（Wayland + NVIDIA）：网页内容点不动、缩放后黑屏</strong></summary>

AppImage 会强制 `GDK_BACKEND=x11`（走 XWayland）以规避历史上的原生 Wayland 崩溃。但在较新的 Wayland + NVIDIA 环境下，这会导致网页内容区点不动（标题栏按钮仍可点）、窗口缩放后黑屏。可用内置的逃生开关切回原生 Wayland：

```bash
CC_SWITCH_GDK_BACKEND=wayland ./CC-Switch-*.AppImage
```

如果你是从桌面图标启动的，请把它写进 `.desktop` 的 `Exec=` 行（如 `env CC_SWITCH_GDK_BACKEND=wayland /path/to/AppImage`），或在会话环境中设置。该变量是通用的：在 tiling Wayland 合成器（sway/Hyprland）下若出现点击失效，可反过来设 `CC_SWITCH_GDK_BACKEND=x11`。不设置则保持默认行为。

</details>

更多问题请查看用户手册中的[常见问题](docs/user-manual/zh/5-faq/5.2-questions.md)。

## 贡献

欢迎提交 Issue 反馈问题和建议！新功能开发前，请先开 Issue 讨论实现方案，不适合项目的功能性 PR 有可能会被关闭。

开发环境、提交前检查和架构说明见 [CONTRIBUTING.md](CONTRIBUTING.md#贡献指南)；使用问题请先看 [SUPPORT.md](SUPPORT.md)；安全漏洞请按 [SECURITY.md](SECURITY.md) 私下报告。

**技术栈**：Tauri 2 · Rust · React 18 · TypeScript · SQLite

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=farion1231/cc-switch&type=Date)](https://www.star-history.com/#farion1231/cc-switch&Date)

## License

MIT © Jason Young

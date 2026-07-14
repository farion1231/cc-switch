# Codex++

<p align="center">
  <img src="docs/images/codex-plus-plus.png" alt="Codex++ 图标" width="160">
</p>

<p align="center">
  中文 | <a href="README_EN.md">English</a>
</p>

<p align="center">
  <img alt="Release" src="https://img.shields.io/github/v/release/BigPizzaV3/CodexPlusPlus">
  <img alt="Stars" src="https://img.shields.io/github/stars/BigPizzaV3/CodexPlusPlus">
  <img alt="License" src="https://img.shields.io/github/license/BigPizzaV3/CodexPlusPlus">
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.85%2B-orange">
  <img alt="Tauri" src="https://img.shields.io/badge/tauri-2.x-24C8DB">
</p>

Codex++ 是面向 OpenAI Codex / ChatGPT 桌面应用的外部启动器与管理工具。它通过 Chromium DevTools Protocol 和本地辅助服务提供供应商切换、协议转换、会话管理与界面增强，不修改官方应用的 `app.asar`，也不向安装目录写入补丁文件。

## 快速使用

从 [GitHub Releases](https://github.com/BigPizzaV3/CodexPlusPlus/releases) 下载最新版安装包：

- Windows：`CodexPlusPlus-*-windows-x64-setup.exe`
- macOS Intel：`CodexPlusPlus-*-macos-x64.dmg`
- macOS Apple Silicon：`CodexPlusPlus-*-macos-arm64.dmg`

安装后会有两个入口：

- `Codex++`：静默启动官方桌面应用，并加载已保存的供应商配置与增强功能。
- `Codex++ 管理工具`：管理供应商、模型、工具插件、会话、增强功能、脚本、更新和诊断。

首次使用建议先打开管理工具，确认应用路径和运行状态，再配置供应商与增强功能，最后从 `Codex++` 入口启动。Windows 安装包会创建桌面和开始菜单快捷方式；macOS DMG 会安装 `/Applications/Codex++.app` 和 `/Applications/Codex++ 管理工具.app`。

## 赞助商

<p align="center">
  <a href="https://jojocode.com/">
    <img src="docs/images/sponsor-jojocode.png" alt="JOJO Code" height="110">
  </a>
</p>
<p align="center">
  <a href="https://jojocode.com/"><strong>JOJO Code｜Codex++ 官方中转站</strong></a><br>
  Codex++ 官方中转站，主打稳定接入和划算价格，支持 GPT-5.6 全系列、Fable 5、Sonnet 5、GPT-5.5、GPT-5.4、Claude Opus 4.8、Claude Opus 4.7、gpt-image-2 等模型与图像能力，适合日常开发、团队协作和长期项目工作流。
</p>

<a href="mailto:1727532@qq.com">想显示在下方？</a>
<p align="center">
</p>
<table>
  <tr>
    <th width="180">🏆 赞助商 🏆</th>
    <th>介绍</th>
  </tr>
  <tr>
    <td align="center">
      <a href="https://jojocode.com/">
        <img src="docs/images/sponsor-jojocode.png" alt="JOJO Code" height="80">
      </a>
    </td>
    <td><a href="https://jojocode.com/"><strong>JOJO Code｜Codex++ 官方中转站</strong></a><br>感谢 JOJO Code 赞助本项目。JOJO Code 是 Codex++ 官方中转站，提供价格划算、稳定易接入的 Codex API 中转服务，支持 GPT-5.6 全系列、Fable 5、Sonnet 5、GPT-5.5、GPT-5.4、Claude Opus 4.8、Claude Opus 4.7、gpt-image-2 等模型与图像能力，适合日常开发、快速配置、团队协作和长期使用。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://aigocode.com/invite/CodexPlusPlus">
        <img src="docs/images/sponsor-aigocode.png" alt="AIGoCode" height="80">
      </a>
    </td>
    <td><a href="https://aigocode.com/invite/CodexPlusPlus"><strong>AIGoCode</strong></a><br>感谢 AIGoCode 赞助了本项目！AIGoCode 是一个集成了 Claude Code、Codex 以及 Gemini 最新模型的一站式平台，为你提供稳定、高效且高性价比的AI编程服务。本站提供灵活的订阅计划，支持多风险，国内直连，无需魔法，极速响应。AIGoCode 为 CodexPlusPlus 的用户提供了特别福利，通过<a href="https://aigocode.com/invite/CodexPlusPlus">此链接注册</a>的用户首次充值可以获得额外10%奖励额度！</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://apikey.fun/register?aff=CODEX">
        <img src="docs/images/sponsor-apikey-fun.png" alt="APIKEY.FUN" height="80">
      </a>
    </td>
    <td><a href="https://apikey.fun/register?aff=CODEX"><strong>APIKEY.FUN</strong></a><br>感谢 APIKEY.FUN 赞助了本项目！APIKEY.FUN 是一家致力于提供开放、稳定、高性价比的全球主流大模型的 AI 中转站。平台支持 Claude、OpenAI、Gemini 等热门模型的 API 中转服务，价格低至官方原价的 7%。通过专属链接<a href="https://apikey.fun/register?aff=CODEX">注册 APIKEY</a>，可享受最高充值永久 95 折优惠。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://runapi.co/register?aff=AWJq">
        <img src="docs/images/sponsor-runapi.png" alt="RunAPI" height="80">
      </a>
    </td>
    <td><a href="https://runapi.co/register?aff=AWJq"><strong>RunAPI</strong></a><br>感谢 RunAPI 赞助了本项目！RunAPI 是高效稳定的 API OpenRouter 平替平台，一个 API Key 即可访问 OpenAI、Claude、Gemini、DeepSeek、Grok 等 150+ 主流模型，低至 1 折，极其稳定，可以无缝兼容 Claude Code、OpenClaw 等工具。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://cubence.com?source=codexplusplus">
        <img src="docs/images/sponsor-cubence.png" alt="Cubence" height="80">
      </a>
    </td>
    <td><a href="https://cubence.com?source=codexplusplus"><strong>Cubence</strong></a><br>感谢 Cubence 对本项目的支持。Cubence 是一家致力为客户提供稳定、高效的 API 中转服务商。从 25 年 9 月运营至今，提供了 Claude Code、Codex、Gemini 等多种模型支持。Cubence 为本开源项目多用户提供了特别的专属优惠 <code>CODEXPLUSPLUS</code>，在首次购买时享受 8.8 折优惠！</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://www.0029.org/?promo=AFF11F">
        <img src="docs/images/sponsor-0029.svg" alt="0029 云桥" height="80">
      </a>
    </td>
    <td><a href="https://www.0029.org/?promo=AFF11F"><strong>0029云桥｜codex api中转站(gpt5.5 gpt-image-2)</strong></a><br>支持个人和企业接入。包月套餐/按量计费，Pro/Plus 号池，全站接口稳定可用，7×24 小时技术支持！</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://xc.y1yun.net/">
        <img src="docs/images/sponsor-yiyun-tech.jpg" alt="屹芸科技" height="80">
      </a>
    </td>
    <td><a href="https://xc.y1yun.net/"><strong>屹芸科技</strong></a><br>屹芸科技旗下拥有九五云商发卡网、屹芸付支付系统等面向 AI 聚合赛道的收付产品，支持微信、支付宝、银联、云闪付等通道，提供低费率、D1/D0 结算、7×24 小时技术支持和企微客户专属服务群。平台通道费率稳定、结算准时，并提供高强度网站防护，帮助商户稳定开展线上销售。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://sui-xiang.com/">
        <img src="docs/images/sponsor-sui-xiang-ai-gateway.jpg" alt="随想AI网关" width="150">
      </a>
    </td>
    <td><a href="https://sui-xiang.com/"><strong>随想AI网关</strong></a><br>感谢随想AI网关对本项目的赞助！随想AI网关是一家可靠高效的 API 中继服务提供商，提供 Claude、Codex、Gemini 等中继服务，注重隐私，承诺无数据倒卖、无模型掺水，并提供透明、快速的售后支持。新账户注册每日签到送 0.5 元测试额度，充值额度 1:1，无需订阅，按量付费。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://dis.chatdesks.cn/chatdesk/hsyqCodexPlusPlus.html">
        <img src="docs/images/sponsor-volcengine.png" alt="火山引擎" height="80">
      </a>
    </td>
    <td><a href="https://dis.chatdesks.cn/chatdesk/hsyqCodexPlusPlus.html"><strong>火山引擎｜方舟 Agent Plan</strong></a><br>感谢火山引擎赞助本项目！方舟 Agent Plan 模型订阅套餐集成了 Doubao-Seed、Doubao-Seedance、Doubao-Seedream 等字节跳动自研 SOTA 级模型，覆盖文本、代码、图像、视频等多模态任务。最新支持 MiniMax-M3、DeepSeek-V4 系列、GLM-5.2、Doubao-Seed-2.0 系列、Kimi-K2.7 等模型，工具不限。超全模态模型与 Harness 升级一步到位，深度支持 Agent 框架与 AI 编程工具。一次订阅，可以为不同任务切换合适的 AI 引擎。方舟 Agent Plan 限时 2.5 折订阅，<a href="https://dis.chatdesks.cn/chatdesk/hsyqCodexPlusPlus.html">点击链接抢购</a>，名额有限，先到先得。<a href="https://www.byteplus.com/en/product/modelark?utm_campaign=hw&amp;utm_content=CodexPlusPlus&amp;utm_medium=devrel_tool_web&amp;utm_source=OWO&amp;utm_term=CodexPlusPlus">For developers outside Mainland China, please click here</a>。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://smallice.xyz/register?aff=FSNMGR2THBLN">
        <img src="docs/images/sponsor-smallice.png" alt="Smallice" height="80">
      </a>
    </td>
    <td><a href="https://smallice.xyz/register?aff=FSNMGR2THBLN"><strong>Smallice｜AI 中转站</strong></a><br>感谢 Smallice 赞助本项目！Smallice 是一把钥匙，通往所有值得调用的语言模型。一个统一的 endpoint，作为你应用之下、无需多言的基础层。无论你召唤的是 Claude、GPT、Gemini 还是 DeepSeek，调用的形式从此恒等。通过<a href="https://smallice.xyz/register?aff=FSNMGR2THBLN">此链接注册</a>即可开始使用。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://ergouapi.com/r/gh-codexplusplus">
        <img src="docs/images/sponsor-ergou-api.png" alt="二狗 API" height="80">
      </a>
    </td>
    <td><a href="https://ergouapi.com/r/gh-codexplusplus"><strong>二狗 API</strong></a><br>二狗，稳如老狗的 AI API 中转站。全站 0.1x~0.2x 超低倍率，提供 Claude/GPT/Gemini 等多个国内外 100% 纯血大模型接口，顶级 IPLC 线路 + 住宅双 ISP 冗余，确保全国范围稳定低延迟访问。欢迎各位开发者、工作室注册使用。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://aihub.top/register?aff=ZYD8UJV274HD">
        <img src="docs/images/sponsor-aihub.jpg" alt="AIHub" height="80">
      </a>
    </td>
    <td><a href="https://aihub.top/register?aff=ZYD8UJV274HD"><strong>AIHub</strong></a><br>AIHub 是一家面向个人开发者和企业团队的高可用 AI 模型 API 中转平台。支持 Codex / ClaudeCode，价格约为官方 1 折不到。我们不生产 Token，我们是 Token 搬运工！通过<a href="https://aihub.top/register?aff=ZYD8UJV274HD">此链接注册</a>并使用优惠码 <code>CODEXPLUSPLUS</code>，即可获得 3$ 测试额度。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://hb-api.online/register?aff=8KA2ZKWNHND8">
        <img src="docs/images/sponsor-baikewei-ai.jpg" alt="百可为AI" height="80">
      </a>
    </td>
    <td><a href="https://hb-api.online/register?aff=8KA2ZKWNHND8"><strong>百可为AI</strong></a><br>百可为AI 是面向开发者、团队和 AI 工具用户的一站式大模型 API 服务平台，支持 Claude、OpenAI、Gemini、Codex 等主流模型能力接入。平台提供稳定中转、灵活计费、用量统计、余额管理和多场景 API 调用能力，适合 Claude Code、Codex、AI 生图、自动化脚本和各类智能应用长期使用。新用户注册可领取免费额度，开发者可快速接入、即开即用，让 AI 能力更稳定、更高效、更省心。</td>
  </tr>
  <tr>
    <td align="center">
      <a href="https://api.sublyx.org/register?aff=JMNUFYR8XAH6">
        <img src="docs/images/sponsor-sublyx.png" alt="Sublyx" width="220">
      </a>
    </td>
    <td><a href="https://api.sublyx.org/register?aff=JMNUFYR8XAH6"><strong>Sublyx｜AI API Gateway</strong></a><br>感谢 Sublyx 赞助本项目！Sublyx 是面向开发者与团队的 AI API 聚合网关，一个 API Key 即可统一接入 OpenAI、Claude、Gemini 等主流模型服务，支持 OpenAI compatible 和 Anthropic Messages 等兼容接口，可用于 Codex、Claude Code、Cherry Studio、OpenAI SDK 等常见开发工具。平台提供统一控制台、用量管理、稳定线路与按需付费能力，适合个人开发、团队协作和 Agent 工作流。Codex++ 用户通过<a href="https://api.sublyx.org/register?aff=JMNUFYR8XAH6">此链接注册 Sublyx</a>并使用优惠码 <code>CDXPP</code>，可额外领取 $10 使用额度。</td>
  </tr>
</table>

## 交流与支持

欢迎加入 Codex++ 交流群（QQ群：830629290），反馈问题、交流使用体验或提出新功能建议。

微信群：<a href="https://docs.qq.com/doc/DQ2VOanZTTFZJcUpZ#">点击这里获取最新微信群二维码</a>。

<img src="docs/images/discussion-group-qr.jpg" alt="Codex++ 微信群二维码" width="260">

Telegram 频道：<https://t.me/CodexPlusPlus>

友情链接：<a href="https://linux.do">LINUX DO</a>

如果 Codex++ 帮到了你，可以请我喝杯咖啡，或者随手赞赏支持一下继续维护。

<p align="center">
  <img src="docs/images/sponsor-alipay.jpg" alt="支付宝赞赏码" width="220">
  <img src="docs/images/sponsor-wechat.jpg" alt="微信赞赏码" width="220">
</p>

## 当前功能

| 模块 | 功能 |
| --- | --- |
| 供应商配置 | 官方登录、官方登录混入 API、纯 API、聚合供应商；Responses / Chat Completions；模型测试、模型列表、Provider Doctor、cc-switch 与链接导入 |
| 模型与上下文 | 每模型上下文窗口、自动压缩阈值、`model_catalog_json`、通用配置，以及按供应商选择 MCP、Skill 和 Plugin |
| 会话管理 | 扫描本地会话、批量删除、Markdown 导出、Token 用量历史、Provider metadata 同步与备份 |
| Codex 增强 | 插件市场与模型白名单、会话操作、粘贴修复、中文界面、快速启动、会话宽度与滚动恢复、服务层级控制、Goals、Stepwise、图片覆盖层 |
| 开发工作流 | 项目移动、Upstream worktree、线程 ID、Zed Remote 项目识别与打开 |
| 脚本与维护 | 用户脚本安装与启停、应用检测、快捷方式、Watcher、环境冲突、日志诊断、健康检查和 Release 更新 |

所有界面增强都可以单独关闭。关闭“Codex 增强”总开关后，Codex++ 仍可作为供应商和启动管理工具使用。

## 供应商模式

Codex++ 将官方登录、混入 API 和纯 API 分开保存和切换：

| 模式 | 用途 | 认证边界 |
| --- | --- | --- |
| 官方登录 | 只使用 ChatGPT / Codex 官方账号 | 清理自定义 provider 和 API Key，保留官方登录状态 |
| 官方登录 + API | 保留官方账号与插件入口，模型请求走兼容 API | API Key 写入 provider bearer token，不写入纯 API 的 `auth.json` |
| 纯 API | 不依赖官方账号，完全使用自定义 Base URL / Key | 独立保存 `config.toml` 与 API Key，不混入官方认证 |
| 聚合供应商 | 在多个普通 API 供应商之间路由 | 支持故障转移、按会话轮转、按请求轮转和权重轮转 |

每个供应商可配置 Responses 或 Chat Completions 协议、模型列表、测试模型、User-Agent、上下文窗口、自动压缩阈值，以及该供应商启用的 MCP Server、Skill 和 Plugin。Chat Completions 可通过本地代理转换为 Codex 使用的 Responses 协议。

每模型窗口支持 `1M`、`200K` 或纯数字。Codex++ 会生成独立 `model_catalog_json`，让 Codex 按当前模型使用对应窗口。

切换供应商时会先保存当前配置，再写入目标配置。真实 API Key 只保存在本机，请勿放入日志、截图或 issue。

## Codex 界面增强

- 会话删除、批量删除、Markdown 导出和项目移动。
- 插件市场解锁、插件自动展开和模型白名单处理。
- 富文本粘贴转纯文本、强制中文、启动加速和原生菜单本地化。
- 会话宽度、滚动位置恢复、线程 ID、服务层级切换和 Goals。
- Stepwise 下一步建议，可单独配置 API、模型、建议数量与超时。
- Upstream worktree、Zed Remote、自定义图片覆盖层和用户脚本。

依赖注入脚本的设置通常需要保存后重新启动 Codex++ 才会生效。

## 自动更新与安装包

Codex++ 通过 GitHub Release 发布安装包。Windows 会生成 NSIS 安装程序，macOS 会生成 Intel x64 和 Apple Silicon arm64 两个 DMG。

管理工具的“关于”页可以检查并启动更新。静默启动器发现新版本时会拉起管理工具并进入更新提示。

## 数据位置

- Codex 配置：`~/.codex/config.toml`
- Codex 登录状态：`~/.codex/auth.json`
- Codex 本地数据库：优先读取 `~/.codex/sqlite/*.db`，旧版回退到 `~/.codex/state_5.sqlite`
- Codex++ 状态与日志：`~/.codex-session-delete/`
- Provider 同步备份：`~/.codex/backups_state/provider-sync`

## 常见问题

### Codex++ 菜单没出现

确认从 `Codex++` 入口启动，而不是直接打开官方应用。然后在管理工具的“安装维护”和“关于”页面检查应用路径、启动状态与诊断日志。

### 切换供应商后请求失败

先在供应商详情中运行模型测试或 Provider Doctor，并确认协议、Base URL、Key 和测试模型匹配。纯 API 与官方混入模式使用不同的认证位置，不要手工复制两种模式的 `auth.json`。

### Upstream worktree 和 Codex 原生创建有什么区别

Codex++ 的 Upstream worktree 功能等价于先更新远端分支，再执行：

```bash
git worktree add -b <new-branch> <worktree-path> upstream/<base-branch>
```

这样新 worktree 从最新的远端跟踪分支开始，而不是从当前会话所在的本地 HEAD 开始。如果 Codex++ 无法安全识别当前 Codex 版本的原生 worktree 创建表单，请从 Codex++ 菜单中手动填写仓库路径、分支名、worktree 路径、remote 和 base branch。

### macOS 提示无法打开或已损坏

当前安装包未签名/未公证时，macOS Gatekeeper 可能拦截，出现“已损坏，无法打开”的提示：

![macOS 提示 Codex++ 管理工具已损坏](docs/images/macos-damaged-warning.png)

如果遇到该提示，可以在终端执行下面两条命令，解除苹果系统的安全隔离限制：

```bash
sudo xattr -rd com.apple.quarantine /Applications/Codex++\ 管理工具.app
sudo xattr -rd com.apple.quarantine /Applications/Codex++.app
```

执行后重新打开 `Codex++` 或 `Codex++ 管理工具` 即可。

### macOS Intel 能用吗

可以。Release 会分别提供 `macos-x64.dmg` 和 `macos-arm64.dmg`。Intel Mac 下载 x64 包，Apple Silicon 下载 arm64 包。

## 开发

```bash
# 前端检查
cd apps/codex-plus-manager
npm ci
npm run check
npm run vite:build

# Rust 检查
cd ../..
cargo fmt --all -- --check
cargo test
cargo build --release
```

主要结构：

```text
apps/
  codex-plus-launcher/          静默启动入口
  codex-plus-manager/           Tauri 管理工具
assets/inject/
  renderer-inject.js            注入到 Codex 渲染端的增强脚本
crates/
  codex-plus-core/              启动、注入、配置、更新、安装、桥接等核心逻辑
  codex-plus-data/              会话数据、导出、Provider 同步
scripts/installer/
  windows/CodexPlusPlus.nsi     Windows NSIS 安装包
  macos/package-dmg.sh          macOS DMG 打包
```

## 开源协议

Copyright (C) 2026 BigPizzaV3

CodexPlusPlus 采用 [GNU Affero General Public License v3.0](LICENSE)，SPDX 标识为 `AGPL-3.0-only`。修改并分发本项目，或通过网络提供修改后的版本时，需要按 AGPLv3 提供对应源代码。

许可证只覆盖 CodexPlusPlus 自身代码，不授予 OpenAI、ChatGPT、Codex 的商标、应用资源或其他第三方内容的权利。

## 兼容性说明

Codex++ 依赖官方桌面应用的页面结构、CDP 和本地数据格式。官方应用更新后，部分注入功能可能需要跟随适配；修改供应商配置或本地会话数据前应保留备份。

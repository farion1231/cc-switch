<div align="center">

# CC Switch · 多账号用量监控版

### 基于 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的个人增强 Fork,聚焦多账号、用量可视化与 Codex 工作流

[![Version](https://img.shields.io/badge/version-3.16.5-blue.svg)](https://github.com/ajia1206/cc-switch/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg)](https://github.com/ajia1206/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

中文 | [English](README_EN.md) | [上游项目](https://github.com/farion1231/cc-switch)

</div>

---

## 📌 关于本 Fork

这是 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的个人增强版本。原项目是一款优秀的 Claude Code / Codex / Gemini CLI / OpenCode / OpenClaw 账号切换管理工具,本 Fork 在其基础上**专注解决多账号用户的用量可视化与 Codex 工作流痛点**。

如果你管理多个 Codex 官方账号、经常被 5 小时窗口或 7 天周期限流、需要随时知道哪个账号还能用,或者希望把 CLI 会话用量、缓存命中与真实 Token 消耗沉淀成仪表盘 —— 这个 Fork 就是为你准备的。

---

## ✨ 相比上游的核心增强

### 功能速览

| 场景 | 上游 | 本 Fork | 入口 |
|------|:----:|:-------:|------|
| Codex 官方账号快照切换 | ✅ | ✅ 增强 | Codex 标签页 |
| 多账号并发用量查询 | ❌ | ✅ | Codex 官方账号快照 |
| 每账号 5h / 7d 剩余额度与重置时间 | ❌ | ✅ | Codex 账号卡片 |
| 全局 Codex 额度摘要 | ❌ | ✅ | 首页 / Codex 用量面板 |
| 年度用量热力图 | ❌ | ✅ | 用量统计 |
| Provider / 模型维度真实 Tokens 统计 | 基础统计 | ✅ 计入缓存命中与缓存创建 | 用量统计 |
| Notch 浮层用量摘要 | ❌ | ✅ | Notch overlay 窗口 |
| Codex proxy takeover 恢复一致性 | 基础行为 | ✅ 增强 | 后台服务 |

### 1. Codex 多账号实时用量监控

- **🔄 全账号并发查询** — 一次性遍历所有 Codex 快照账号,无需切换账号即可查看每个账号的官方用量
- **📊 卡片化额度显示** — 每个账号独立展示 `5h` 与 `7d` 窗口的剩余额度、重置时间与状态颜色
- **⏱ 可配置刷新间隔** — 在设置中配置用量刷新频率,也可以在账号面板手动立即刷新
- **💾 查询缓存与事件同步** — 后端缓存官方用量结果,托盘或后台刷新后会同步到前端 React Query 缓存

### 2. 用量统计增强

- **📈 真实 Tokens 口径** — Provider / 模型统计中的 `Tokens` 使用 `fresh input + output + cache read + cache creation`,避免缓存命中场景被低估
- **🟩 年度热力图** — 用 GitHub contribution 风格展示最近一年的活跃天数、会话数与真实 Token 消耗
- **🧮 多来源去重** — 对 Codex / Gemini / Claude / OpenCode 的 proxy 日志与 session 日志做有效用量去重,避免重复计数
- **🗄 历史 rollup** — 旧明细日志会汇总到每日 rollup,减少数据库膨胀,同时保持趋势与统计可查询

### 3. Codex 工作流细节增强

- **🔐 官方账号快照管理** — 支持保存、切换、恢复多个 Codex 官方账号快照
- **🔁 可靠重启 Codex** — 切换账号后自动重启相关 Codex 进程,让凭据尽快生效
- **🧭 proxy takeover 一致性** — Codex proxy 接管/恢复时保留关键 live config 与 OAuth 信息,减少切换后配置漂移
- **⌁ Notch 用量摘要** — 轻量浮层显示当前 Codex 账号组里可用额度最好的摘要信息

---

## 📦 安装

### 方式一:下载 DMG(macOS Apple Silicon)

从 [Releases](https://github.com/ajia1206/cc-switch/releases) 页面下载最新的 `CC Switch` macOS 安装包。

```bash
# 双击 dmg → 拖到 Applications 即可
# 首次运行如提示"无法验证开发者",请到「系统设置 → 隐私与安全性」点击「仍要打开」
```

### 方式二:源码构建

```bash
# 1. 克隆仓库
git clone https://github.com/ajia1206/cc-switch.git
cd cc-switch

# 2. 安装依赖(需要 Node 22+ 和 Rust 工具链)
pnpm install

# 3. 开发模式运行
pnpm dev

# 4. 打包发布
pnpm tauri build
# 产物位于 src-tauri/target/release/bundle/
```

**前置依赖:**
- Node.js 22.12+(推荐用 [fnm](https://github.com/Schniz/fnm) 管理)
- Rust 1.85+
- pnpm 9+(`npm i -g pnpm`)
- macOS: Xcode Command Line Tools

---

## 🎮 使用说明

### 查看 Codex 多账号用量

1. 打开 CC Switch → 顶部切换到 **Codex** 标签
2. 点击 **「Codex 官方账号快照」**
3. 每个账号卡片下方自动显示用量信息:

```text
┌─────────────────────────────────────┐
│ 📦 我的主账号        [使用中]       │
│  ⏱ 5h 剩余: 73%  · 2 小时后重置     │
│  📅 7d 剩余: 45%  · 5 天后重置      │
└─────────────────────────────────────┘
```

### 查看全局用量统计

1. 打开 **用量统计**
2. 在顶部选择应用范围与时间范围
3. 通过 **Provider 统计** / **模型统计** 查看计入缓存后的真实 Tokens
4. 在热力图中查看最近一年的活跃天数、会话数与 Tokens 峰值

### 调整刷新策略

- **设置 → 用量统计**:配置刷新间隔
- **账号面板刷新按钮**:立即触发所有 Codex 快照账号并发查询

### 切换账号

点击任意账号卡片的「切换到此账号」按钮,会自动:
1. 备份当前 `~/.codex/auth.json` 到当前激活账号的快照
2. 把目标账号的快照恢复到 `~/.codex/auth.json`
3. 重启 Codex 相关进程使凭据生效

---

## 🛠 技术实现

| 模块 | 文件 | 说明 |
|------|------|------|
| 多账号查询 | `src-tauri/src/codex_accounts.rs` | `get_all_account_quotas` 并发查询所有快照 |
| Tauri 命令 | `src-tauri/src/commands/codex_accounts.rs` | `get_all_codex_quotas` 暴露给前端 |
| 用量缓存 | `src-tauri/src/services/subscription.rs` | 每账号独立 TTL 缓存 |
| 统计聚合 | `src-tauri/src/services/usage_stats.rs` | 真实 Tokens、去重、Provider / 模型统计 |
| 历史汇总 | `src-tauri/src/database/dao/usage_rollup.rs` | 明细日志 rollup 与保留策略 |
| 设置持久化 | `src-tauri/src/settings.rs` | `codex_quota_refresh_interval` 字段 |
| 前端查询 | `src/lib/query/subscription.ts` | Codex 额度 React Query hooks |
| 账号面板 | `src/components/codex/CodexAccountsPanel.tsx` | 卡片用量渲染 + 刷新控件 |
| 用量热力图 | `src/components/usage/UsageActivityHeatmap.tsx` | 年度活跃与 Tokens 可视化 |
| Notch 浮层 | `src/components/NotchOverlay.tsx` | Codex 用量轻量摘要 |

---

## 🙏 致谢

- 上游项目: [@farion1231/cc-switch](https://github.com/farion1231/cc-switch) by Jason Young
- 上游完整功能(Claude/Codex/Gemini/OpenCode/OpenClaw 多 Provider 切换)请参考[原项目文档](https://github.com/farion1231/cc-switch/blob/main/README_ZH.md)
- 本 Fork 的用量查询实现思路参考了 [ericjypark/codex-island](https://github.com/ericjypark/codex-island)

---

## 📄 协议

本项目继承上游 [MIT License](LICENSE),版权归 Jason Young 及各贡献者所有。

---

## 🔗 相关链接

- 本 Fork: [github.com/ajia1206/cc-switch](https://github.com/ajia1206/cc-switch)
- 上游仓库: [github.com/farion1231/cc-switch](https://github.com/farion1231/cc-switch)
- Releases: [github.com/ajia1206/cc-switch/releases](https://github.com/ajia1206/cc-switch/releases)
- Issues: [github.com/ajia1206/cc-switch/issues](https://github.com/ajia1206/cc-switch/issues)

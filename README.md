<div align="center">

# CC Switch · 多账号用量监控版

### 基于 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的个人增强 Fork,新增 Codex 多账号实时用量监控

[![Version](https://img.shields.io/badge/version-3.16.2-blue.svg)](https://github.com/ajia1206/cc-switch/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg)](https://github.com/ajia1206/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

中文 | [English](README_EN.md) | [上游项目](https://github.com/farion1231/cc-switch)

</div>

---

## 📌 关于本 Fork

这是 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的个人增强版本。原项目是一款优秀的 Claude Code / Codex / Gemini CLI / OpenCode / OpenClaw 账号切换管理工具,本 Fork 在其基础上**专注解决多账号用户的用量可视化痛点**。

如果你管理多个 Codex 官方账号、经常被 5 小时窗口或 7 天周期限流、需要随时知道哪个账号还能用 —— 这个 Fork 就是为你准备的。

---

## ✨ 相比上游的核心增强

### 🎯 Codex 多账号实时用量监控

| 功能 | 上游 | 本 Fork |
|------|:----:|:-------:|
| 单账号用量查询 | ✅ | ✅ |
| **多账号并发查询** | ❌ | ✅ |
| **每账号独立显示 5h / 7d 用量** | ❌ | ✅ |
| **重置倒计时** | ❌ | ✅ |
| **可配置刷新间隔** | ❌ | ✅ |
| **手动立即刷新** | ❌ | ✅ |

### 详细特性

- **🔄 全账号并发查询** — 后端 `get_all_account_quotas` 命令,一次性遍历所有 Codex 快照账号,并发查询用量。无需切换账号即可查看
- **📊 卡片化显示** — 每个账号卡片下方显示 `5h 剩余: XX% · X 小时后重置` 与 `7d 剩余: XX% · X 天后重置`
- **🎨 用量分级颜色** — 剩余 ≥30% 绿色 / ≥10% 橙色 / <10% 红色
- **⏱ 可配置刷新间隔** — 下拉菜单选择 `1 / 5 / 30 / 60 分钟`,默认 5 分钟
- **⚡ 立即刷新按钮** — 想看实时数据时一键手动同步
- **💾 设置持久化** — 刷新间隔保存到 `~/.cc-switch/settings.json`,重启不丢失

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

### 调整刷新策略

- **顶部下拉菜单**:选择刷新间隔(1 / 5 / 30 / 60 分钟)
- **🔄 立即刷新按钮**:点击立即触发并发查询所有账号

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
| 设置持久化 | `src-tauri/src/settings.rs` | `codex_quota_refresh_interval` 字段 |
| 前端查询 | `src/lib/query/subscription.ts` | `useAllCodexQuotas` Hook |
| 账号面板 | `src/components/codex/CodexAccountsPanel.tsx` | 卡片用量渲染 + 刷新控件 |

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

# CC Switch v3.9.0

> 本地 API 代理、自动故障切换、统一供应商与多应用工作流增强

**[English →](release-note-v3.9.0-en.md) | [日本語版 →](release-note-v3.9.0-ja.md)**

---

## 概览

CC Switch v3.9.0 是 v3.9 测试版序列（`3.9.0-1`、`3.9.0-2`、`3.9.0-3`）的稳定版。
本次更新带来本地 API 代理（支持按应用接管）、自动故障切换、统一供应商（Universal Provider），并对 Claude Code / Codex / Gemini CLI 的稳定性与使用体验做了大量改进。

**发布日期**：2026-01-07

---

## 重点内容

- 本地 API 代理：Claude Code / Codex / Gemini CLI 统一接入
- 自动故障切换：熔断保护 + 每个应用独立的 failover 队列
- 统一供应商：一份配置可同步到多个应用（适合 NewAPI 等网关）
- Skills 相关增强：支持多应用、管理架构统一（SSOT + React Query）
- 通用配置片段：支持从编辑器内容或当前供应商提取可复用片段
- MCP 导入：支持从已安装应用导入 MCP servers
- 用量增强：自动刷新、缓存命中/创建指标、时区修复
- Linux 打包：新增 RPM 与 Flatpak 制品

---

## 主要功能

### 本地 API 代理（Local API Proxy）

- 运行一个本地高性能 HTTP 代理服务（基于 Axum）
- 统一代理 Claude Code、Codex、Gemini CLI 的 API 请求
- 按应用接管：你可以分别控制每个应用是否走本地代理
- Live 配置接管：启用接管时，会备份并重定向 CLI 的 live 配置到本地代理
- 监控能力：记录请求日志与用量统计，便于排错与成本分析
- 错误请求日志：代理会记录失败请求的详细信息，便于定位问题（#401，感谢 @yovinchen）

### 自动故障切换（Auto Failover / 熔断）

- 自动检测供应商异常并触发熔断保护
- 当前供应商不可用时自动切换到备用供应商
- 每个应用维护独立的 failover 队列，并实时追踪健康状态
- 当关闭故障切换时，超时/重试相关配置不会影响正常请求流程

### Skills 管理

- Skills 支持 Claude Code 与 Codex 多应用使用，并提供旧结构到新结构的平滑迁移（#365、#378，感谢 @yovinchen）
- Skills 管理架构统一（SSOT + React Query），状态刷新与数据一致性更稳定
- 发现（Discovery）体验与性能改进：
  - 扫描时跳过隐藏目录
  - Discoverable skills 使用长生命周期缓存提升性能
  - 增加加载状态提示，导入/刷新等操作入口更显眼
  - 修复 Skills 仓库分支配置错误（#505，感谢 @kjasn）

### 统一供应商（Universal Provider）

- 新增“跨应用共享”的供应商配置，可同步到 Claude/Codex/Gemini（#348，感谢 @Calcium-Ion）
- 适配支持多协议的 API 网关（例如 NewAPI）
- 同一个供应商下可按应用分别设置默认模型映射

### 通用配置片段（Claude/Codex/Gemini）

- 维护一段“通用配置片段”，并将其合并/追加到启用该功能的供应商配置中
- 新增“提取通用配置片段”工作流：
  - 优先从编辑器当前内容提取（你正在编辑的内容）
  - 若未提供编辑器内容，则从当前激活的供应商提取
- Codex 场景提取更安全：
  - 自动移除 `model_provider`、`model` 以及整个 `model_providers` 表等供应商相关内容
  - 会保留 `[mcp_servers.*]` 下的 `base_url`，避免误伤 MCP 配置

### MCP 管理

- 支持从已安装应用导入 MCP servers
- 同步更稳健：目标 CLI 未安装则跳过；无效的 Codex `config.toml` 可更优雅处理（#461，感谢 @majiayu000）
- Windows 兼容性：MCP 导出相关的 npx/npm 调用使用 `cmd /c` 包裹

### 用量与计费数据

- 用量与计费增强：自动刷新、缓存命中/创建指标、时区修复，以及内置价格表更新（#508，感谢 @yovinchen）
- 深链支持：可通过 deeplink 导入用量查询配置（#400，感谢 @qyinter）
- 用量统计支持提取模型信息（#455，感谢 @yovinchen）
- 用量查询凭证支持从供应商配置回退（#360，感谢 @Sirhexs）

---

## 体验优化

- 供应商搜索过滤：按名称快速查找（#435，感谢 @TinsFox）
- 供应商图标颜色：支持为供应商图标设置自定义颜色，便于快速区分（#385，感谢 @yovinchen）
- 快捷键：`Cmd/Ctrl + ,` 打开设置（#436，感谢 @TinsFox）
- 可跳过 Claude Code 首次确认弹窗（可选）
- Toast 通知可关闭：切换提示与成功提示都支持关闭按钮（#350，感谢 @ForteScarlet）
- 点击更新徽章会自动跳转到 About 标签页
- 设置页 Tab 样式改进（#342，感谢 @wenyuanw）
- 更顺滑的切换动效：应用/视图淡入淡出与面板退出动画
- 代理接管激活时应用翡翠绿主题，便于一眼识别当前状态
- 深色模式可读性增强（表单与标签对比度等）
- FullScreenPanel 的窗口拖拽区域优化（#525，感谢 @zerob13）

---

## 平台说明

### Windows

- 版本检查不再弹出终端窗口
- 改进窗口尺寸默认值（最小宽高）
- 修复部分设备启动黑屏问题（使用系统标题栏方案）
- 兼容旧 WebView：为 `crypto.randomUUID()` 增加降级方案

### macOS

- 自启动使用 `.app bundle` 路径，避免弹出终端窗口（#462，感谢 @majiayu000）
- 托盘与标题栏相关体验优化

---

## 打包

- Linux：新增 RPM 与 Flatpak 打包目标，用于生成发布制品

---

## 说明与注意事项

- 安全增强：修复 JavaScript 执行器与用量脚本相关的安全问题（#151，感谢 @luojiyin1987）。
- 为降低导入风险，SQL 导入被限制为仅允许导入 CC Switch 自己导出的备份。
- Proxy 接管会修改 CLI 的 live 配置；CC Switch 会在重定向前自动备份 live 配置。如需回退，可关闭接管/停止代理，并在必要时从备份恢复。

## 特别感谢

特别感谢 @xunyu @deijing @su-fen 做出的支持和贡献，没有你们就没有这个版本！

## 下载与安装

访问 [Releases](https://github.com/farion1231/cc-switch/releases/latest) 下载对应版本。

### 系统要求

| 系统    | 最低版本                      | 架构                                |
| ------- | ----------------------------- | ----------------------------------- |
| Windows | Windows 10 及以上             | x64                                 |
| macOS   | macOS 10.15 (Catalina) 及以上 | Intel (x64) / Apple Silicon (arm64) |
| Linux   | 见下表                        | x64                                 |

### Windows

| 文件                                    | 说明                                |
| --------------------------------------- | ----------------------------------- |
| `CC-Switch-v3.9.0-Windows.msi`          | **推荐** - MSI 安装包，支持自动更新 |
| `CC-Switch-v3.9.0-Windows-Portable.zip` | 便携版，解压即用，不写入注册表      |

### macOS

| 文件                            | 说明                                                      |
| ------------------------------- | --------------------------------------------------------- |
| `CC-Switch-v3.9.0-macOS.zip`    | **推荐** - 解压后拖入 Applications 即可，Universal Binary |
| `CC-Switch-v3.9.0-macOS.tar.gz` | 用于 Homebrew 安装和自动更新                              |

> **注意**：由于作者没有苹果开发者账号，首次打开可能出现"未知开发者"警告，请先关闭，然后前往"系统设置" → "隐私与安全性" → 点击"仍要打开"，之后便可以正常打开

### Homebrew（MacOS）

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

更新：

```bash
brew upgrade --cask cc-switch
```

### Linux

| 发行版                                  | 推荐格式    | 安装方式                                                               |
| --------------------------------------- | ----------- | ---------------------------------------------------------------------- |
| Ubuntu / Debian / Linux Mint / Pop!\_OS | `.deb`      | `sudo dpkg -i CC-Switch-*.deb` 或 `sudo apt install ./CC-Switch-*.deb` |
| Fedora / RHEL / CentOS / Rocky Linux    | `.rpm`      | `sudo rpm -i CC-Switch-*.rpm` 或 `sudo dnf install ./CC-Switch-*.rpm`  |
| openSUSE                                | `.rpm`      | `sudo zypper install ./CC-Switch-*.rpm`                                |
| Arch Linux / Manjaro                    | `.AppImage` | 添加执行权限后直接运行，或使用 AUR                                     |
| 其他发行版 / 不确定                     | `.AppImage` | `chmod +x CC-Switch-*.AppImage && ./CC-Switch-*.AppImage`              |
| 沙箱隔离需求                            | `.flatpak`  | `flatpak install CC-Switch-*.flatpak`                                  |

# CC Switch v3.7.0

> 从供应商切换器到 AI CLI 一体化管理平台

**[English Version →](release-note-v3.7.0-en.md)**

---

## 概览

CC Switch v3.7.0 新增六大核心功能，新增超过 18,000 行代码。

**发布日期**：2025-11-19
**提交数量**：从 v3.6.0 开始 85 个提交
**代码变更**：152 个文件，+18,104 / -3,732 行

---

## 新增功能

### Gemini CLI 集成

完整支持 Google Gemini CLI，成为第三个支持的应用（Claude Code、Codex、Gemini）。

**核心能力**：

- **双文件配置** - 同时支持 `.env` 和 `settings.json` 格式
- **自动检测** - 自动检测 `GOOGLE_GEMINI_BASE_URL`、`GEMINI_MODEL` 等环境变量
- **完整 MCP 支持** - 为 Gemini 提供完整的 MCP 服务器管理
- **深度链接集成** - 通过 `ccswitch://` 协议导入配置
- **系统托盘** - 从托盘菜单快速切换

**供应商预设**：

- **Google Official** - 支持 OAuth 认证
- **PackyCode** - 合作伙伴集成
- **自定义** - 完全自定义支持

**技术实现**：

- 新增后端模块：`gemini_config.rs`（20KB）、`gemini_mcp.rs`
- 表单与环境编辑器同步
- 双文件原子写入

---

### MCP v3.7.0 统一架构

MCP 管理系统完整重构，实现跨应用统一管理。

**架构改进**：

- **统一管理面板** - 单一界面管理 Claude/Codex/Gemini MCP 服务器
- **SSE 传输类型** - 新增 Server-Sent Events 支持
- **智能解析器** - 容错性 JSON 解析
- **格式修正** - 自动修复 Codex `[mcp_servers]` 格式
- **扩展字段** - 保留自定义 TOML 字段

**用户体验**：

- 表单中的默认应用选择
- JSON 格式化器用于验证
- 改进的视觉层次
- 更好的错误消息

**导入/导出**：

- 统一从三个应用导入
- 双向同步
- 状态保持

---

### Claude Skills 管理系统

**约 2,000 行代码** - 完整的技能生态平台。

**GitHub 集成**：

- 从 GitHub 仓库自动扫描技能
- 预配置仓库：
  - `ComposioHQ/awesome-claude-skills` - 精选集合
  - `anthropics/skills` - Anthropic 官方技能
  - `cexll/myclaude` - 社区贡献
- 添加自定义仓库
- 子目录扫描支持（`skillsPath`）

**生命周期管理**：

- **发现** - 自动检测 `SKILL.md` 文件
- **安装** - 一键安装到 `~/.claude/skills/`
- **卸载** - 安全移除并跟踪状态
- **更新** - 检查更新（基础设施已就绪）

**技术架构**：

- **后端**：`SkillService`（526 行）集成 GitHub API
- **前端**：SkillsPage、SkillCard、RepoManager
- **UI 组件**：Badge、Card、Table（shadcn/ui）
- **状态**：持久化存储在 `skills.json`
- **国际化**：47+ 个翻译键

---

### Prompts 管理系统

**约 1,300 行代码** - 完整的系统提示词管理。

**多预设管理**：

- 创建无限数量的提示词预设
- 快速在预设间切换
- 同时只能激活一个提示词
- 活动提示词删除保护

**跨应用支持**：

- **Claude**：`~/.claude/CLAUDE.md`
- **Codex**：`~/.codex/AGENTS.md`
- **Gemini**：`~/.gemini/GEMINI.md`

**Markdown 编辑器**：

- 完整的 CodeMirror 6 集成
- 语法高亮
- 暗色主题（One Dark）
- 实时预览

**智能同步**：

- **自动写入** - 立即写入 live 文件
- **回填保护** - 切换前保存当前内容
- **自动导入** - 首次启动从 live 文件导入
- **修改保护** - 保留手动修改

**技术实现**：

- **后端**：`PromptService`（213 行）
- **前端**：PromptPanel（177）、PromptFormModal（160）、MarkdownEditor（159）
- **Hooks**：usePromptActions（152 行）
- **国际化**：41+ 个翻译键

---

### 深度链接协议（ccswitch://）

通过 URL 方案一键导入供应商配置。

**功能特性**：

- 所有平台的协议注册
- 从共享链接导入
- 生命周期集成
- 安全验证

---

### 环境变量冲突检测

智能检测和管理配置冲突。

**检测范围**：

- **Claude & Codex** - 跨应用冲突
- **Gemini** - 自动发现
- **MCP** - 服务器配置冲突

**管理功能**：

- 可视化冲突指示器
- 解决建议
- 覆盖警告
- 更改前备份

---

## 改进优化

### 供应商管理

**新增预设**：

- **DouBaoSeed** - 字节跳动的豆包
- **Kimi For Coding** - 月之暗面
- **BaiLing** - 百灵 AI
- **移除 AnyRouter** - 避免误导

**增强功能**：

- Codex 和 Gemini 的模型名称配置
- 供应商备注字段用于组织
- 增强的预设元数据

### 配置管理

- **通用配置迁移** - 从 localStorage 迁移到 `config.json`
- **统一持久化** - 跨所有应用共享
- **自动导入** - 首次启动配置导入
- **回填优先级** - 正确处理 live 文件

### UI/UX 改进

**设计系统**：

- **macOS 原生** - 与系统对齐的配色方案
- **窗口居中** - 默认居中位置
- **视觉优化** - 改进的间距和层次

**交互优化**：

- **密码输入** - 修复 Edge/IE 显示按钮
- **URL 溢出** - 修复卡片溢出
- **错误复制** - 可复制到剪贴板的错误
- **托盘同步** - 实时拖放同步

---

## Bug 修复

### 关键修复

- **用量脚本验证** - 边界检查
- **Gemini 验证** - 放宽约束
- **TOML 解析** - CJK 引号处理
- **MCP 字段** - 自定义字段保留
- **白屏** - FormLabel 崩溃修复

### 稳定性

- **托盘安全** - 模式匹配替代 unwrap
- **错误隔离** - 托盘失败不阻塞操作
- **导入分类** - 正确的类别分配

### UI 修复

- **模型占位符** - 移除误导性提示
- **Base URL** - 第三方供应商自动填充
- **拖拽排序** - 托盘菜单同步

---

## 技术改进

### 架构

**MCP v3.7.0**：

- 移除遗留代码（约 1,000 行）
- 统一初始化结构
- 保持向后兼容性
- 全面的代码格式化

**平台兼容性**：

- Windows winreg API 修复（v0.52）
- 安全模式匹配（无 `unwrap()`）
- 跨平台托盘处理

### 配置

**同步机制**：

- 跨所有应用的 MCP 同步
- Gemini 表单-编辑器同步
- 双文件读取（.env + settings.json）

**验证增强**：

- 输入边界检查
- TOML 引号规范化（CJK）
- 自定义字段保留
- 增强的错误消息

### 代码质量

**类型安全**：

- 完整的 TypeScript 覆盖
- Rust 类型改进
- API 契约验证

**测试**：

- 简化的断言
- 更好的测试覆盖
- 集成测试更新

**依赖项**：

- Tauri 2.8.x
- Rust：`anyhow`、`zip`、`serde_yaml`、`tempfile`
- 前端：CodeMirror 6 包
- winreg 0.52（Windows）

---

## 技术统计

```
总体变更：
- 提交数：85
- 文件数：152 个文件变更
- 新增：+18,104 行
- 删除：-3,732 行

新增模块：
- Skills 管理：2,034 行（21 个文件）
- Prompts 管理：1,302 行（20 个文件）
- Gemini 集成：约 1,000 行
- MCP 重构：约 3,000 行重构

代码分布：
- 后端（Rust）：约 4,500 行新增
- 前端（React）：约 3,000 行新增
- 配置：约 1,500 行重构
- 测试：约 500 行
```

---

## 战略定位

### 从工具到平台

v3.7.0 代表了 CC Switch 定位的转变：

| 方面     | v3.6           | v3.7.0                  |
| -------- | -------------- | ----------------------- |
| **身份** | 供应商切换器   | AI CLI 管理平台         |
| **范围** | 配置管理       | 生态系统管理            |
| **应用** | Claude + Codex | Claude + Codex + Gemini |
| **能力** | 切换配置       | 扩展能力（Skills）      |
| **定制** | 手动编辑       | 可视化管理（Prompts）   |
| **集成** | 孤立应用       | 统一管理（MCP）         |

### AI CLI 管理六大支柱

1. **配置管理** - 供应商切换和管理
2. **能力扩展** - Skills 安装和生命周期
3. **行为定制** - 系统提示词预设
4. **生态集成** - 深度链接和共享
5. **多 AI 支持** - Claude/Codex/Gemini
6. **智能检测** - 冲突预防

---

## 下载与安装

### 系统要求

- **Windows**：Windows 10+
- **macOS**：macOS 10.15（Catalina）+
- **Linux**：Ubuntu 22.04+ / Debian 11+ / Fedora 34+

### 下载链接

访问 [Releases](https://github.com/farion1231/cc-switch/releases/latest) 下载：

- **Windows**：`CC-Switch-v3.7.0-Windows.msi` 或 `-Portable.zip`
- **macOS**：`CC-Switch-v3.7.0-macOS.tar.gz` 或 `.zip`
- **Linux**：`CC-Switch-v3.7.0-Linux.AppImage` 或 `.deb`

### Homebrew（macOS）

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

更新：

```bash
brew upgrade --cask cc-switch
```

---

## 迁移说明

### 从 v3.6.x 升级

**自动迁移** - 无需任何操作，配置完全兼容

### 从 v3.1.x 或更早版本升级

**需要两步迁移**：

1. 首先升级到 v3.2.x（执行一次性迁移）
2. 然后升级到 v3.7.0

### 新功能

- **Skills**：无需迁移，全新开始
- **Prompts**：首次启动时从 live 文件自动导入
- **Gemini**：需要单独安装 Gemini CLI
- **MCP v3.7.0**：与之前的配置向后兼容

---

## 致谢

### 贡献者

感谢所有让这个版本成为可能的贡献者：

- [@YoVinchen](https://github.com/YoVinchen) - Skills & Prompts & Geimini 集成实现
- [@farion1231](https://github.com/farion1231) - 从开发沦为 issue 回复机
- 社区成员的测试和反馈

### 赞助商

**Z.ai** - GLM CODING PLAN 赞助商
[通过此链接获得 10% 折扣](https://z.ai/subscribe?ic=8JVLJQFSKB)

**PackyCode** - API 中继服务合作伙伴
[使用 "cc-switch" 代码注册可享受 10% 折扣](https://www.packyapi.com/register?aff=cc-switch)

---

## 反馈与支持

- **问题反馈**：[GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- **讨论**：[GitHub Discussions](https://github.com/farion1231/cc-switch/discussions)
- **文档**：[README](../README_ZH.md)
- **更新日志**：[CHANGELOG.md](../CHANGELOG.md)

---

## 未来展望

**v3.8.0 预览**（暂定）：

- 本地代理功能

敬请期待更多更新！

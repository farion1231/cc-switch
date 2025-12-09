# CC Switch v3.8.0

> 持久化架构升级，为云同步奠定基础

**[English Version →](release-note-v3.8.0-en.md)**

---

## 概览

CC Switch v3.8.0 是一次重大的架构升级版本，重构了数据持久化层和用户界面，为未来的云同步和本地代理功能奠定基础。

**发布日期**：2025-11-28
**提交数量**：从 v3.7.1 开始 51 个提交
**代码变更**：207 个文件，+17,297 / -6,870 行

---

## 重大更新

### 持久化架构升级

从单一 JSON 文件存储迁移到 SQLite + JSON 双层架构，实现数据分层管理。

**架构变更**：

```
v3.7.x (旧)                      v3.8.0 (新)
┌─────────────────┐              ┌─────────────────────────────────┐
│  config.json    │              │  SQLite (可同步数据)             │
│  ┌───────────┐  │              │  ├─ providers     供应商配置     │
│  │ providers │  │              │  ├─ mcp_servers   MCP 服务器     │
│  │ mcp       │  │     ──>      │  ├─ prompts       提示词         │
│  │ prompts   │  │              │  ├─ skills        技能           │
│  │ settings  │  │              │  └─ settings      通用设置       │
│  └───────────┘  │              ├─────────────────────────────────┤
└─────────────────┘              │  JSON (设备级数据)               │
                                 │  └─ settings.json 本地设置       │
                                 │     ├─ 窗口位置                  │
                                 │     ├─ 路径覆盖                  │
                                 │     └─ 当前选中供应商 ID          │
                                 └─────────────────────────────────┘
```

**双层结构设计**：

| 层级     | 存储方式 | 数据类型                     | 同步策略   |
| -------- | -------- | ---------------------------- | ---------- |
| 云同步层 | SQLite   | 供应商、MCP、Prompts、Skills | 未来可同步 |
| 设备层   | JSON     | 窗口状态、本地路径、当前选择 | 保持本地   |

**技术实现**：

- **Schema 版本管理** - 支持数据库结构升级迁移
- **SQL 导入导出** - `backup.rs` 支持 SQL dump，便于云端存储
- **事务支持** - SQLite 原生事务保证数据一致性
- **自动迁移** - 首次启动自动从 `config.json` 迁移数据

**模块化重构**：

```
database/
├── mod.rs              核心 Database 结构体和初始化
├── schema.rs           表结构定义、Schema 版本迁移
├── backup.rs           SQL 导入导出、二进制快照备份
├── migration.rs        JSON → SQLite 数据迁移引擎
└── dao/                数据访问对象层
    ├── providers.rs    供应商 CRUD
    ├── mcp.rs          MCP 服务器 CRUD
    ├── prompts.rs      提示词 CRUD
    ├── skills.rs       Skills CRUD
    └── settings.rs     键值对设置存储
```

---

### 全新用户界面

完整重构的 UI 设计，提供更现代化的视觉体验。

**视觉改进**：

- 重新设计的界面布局
- 统一的组件样式
- 更流畅的过渡动画
- 优化的视觉层次

**交互优化**：

- Header toolbar 重新设计
- ConfirmDialog 样式统一
- 禁用主视图 overscroll 弹跳效果
- 改进的表单验证反馈

**兼容性调整**：

- Tailwind CSS 从 v4 降级到 v3.4，提升浏览器兼容性

---

### 日语支持

新增日语（日本語）界面支持，国际化语言扩展到三种。

**支持语言**：

- 简体中文
- English
- 日本語（新增）

---

## 新增功能

### Skills 递归扫描

Skills 管理系统支持递归扫描仓库目录，自动发现嵌套的技能文件。

**改进内容**：

- 支持多层目录结构
- 自动发现所有 `SKILL.md` 文件
- 允许不同仓库的同名技能（使用完整路径去重）

---

### 供应商图标配置

供应商预设支持自定义图标配置。

**功能特性**：

- 预设供应商包含默认图标
- 复制供应商时保留图标设置
- 图标颜色自定义

---

### 表单验证增强

供应商表单新增必填字段验证，提供更友好的错误提示。

**改进内容**：

- 必填字段实时校验
- 统一使用 Toast 通知显示验证错误
- 更清晰的错误信息

---

### 开机自启

新增开机自动启动功能，支持 Windows、macOS 和 Linux 三个平台。

**功能特性**：

- 在设置中一键开启/关闭
- 使用平台原生 API 实现
- Windows 使用注册表、macOS 使用 LaunchAgent、Linux 使用 XDG autostart

---

### 新增供应商预设

- **MiniMax** - 官方合作伙伴

---

## Bug 修复

### 关键修复

**自定义端点丢失问题**

修复更新供应商时自定义请求地址意外丢失的问题。

- 根因：`INSERT OR REPLACE` 在 SQLite 底层执行 `DELETE + INSERT`，触发外键级联删除
- 修复：改用 `UPDATE` 语句更新已存在的供应商

**Gemini 配置问题**

- 修复自定义供应商环境变量未正确写入 `.env` 文件
- 修复安全认证配置错误写入到其他配置文件

**供应商验证问题**

- 修复当前供应商 ID 不存在时的验证错误
- 修复供应商复制时图标字段丢失

### 平台兼容性

**Linux**

- 解决 WebKitGTK DMA-BUF 渲染问题
- 保留用户 `.desktop` 文件自定义

### 其他修复

- 修复切换应用时的冗余用量查询
- 修复 DMXAPI 预设使用错误的认证令牌字段
- 修复深链接组件缺少翻译键
- 修复用量脚本模板初始化逻辑

---

## 技术改进

### 架构重构

**供应商服务模块化**：

```
services/provider/
├── mod.rs          核心服务 - add/update/delete/switch/validate
├── live.rs         Live 配置文件操作
├── gemini_auth.rs  Gemini 认证类型检测
├── endpoints.rs    自定义端点管理
└── usage.rs        用量脚本执行
```

**深链接模块化**：

```
deeplink/
├── mod.rs       模块导出
├── parser.rs    URL 解析
├── provider.rs  供应商导入逻辑
├── mcp.rs       MCP 导入逻辑
├── prompt.rs    提示词导入
├── skill.rs     Skills 导入
└── utils.rs     工具函数
```

### 代码质量

**清理工作**：

- 移除 JSON 时代遗留的导入导出死代码
- 移除未使用的 MCP 类型导出
- 统一错误处理方式

**测试更新**：

- 迁移测试到 SQLite 数据库架构
- 更新组件测试匹配当前实现
- 修复 MSW handlers 适配新 API

---

## 技术统计

```
总体变更：
- 提交数：51
- 文件数：207 个文件变更
- 新增：+17,297 行
- 删除：-6,870 行
- 净增：+10,427 行

提交类型分布：
- fix：25 个（Bug 修复）
- refactor：11 个（代码重构）
- feat：9 个（新功能）
- test：1 个（测试）
- 其他：5 个

改动区域分布：
- 前端源码：112 个文件
- Rust 后端：63 个文件
- 测试文件：20 个文件
- 国际化文件：3 个文件
```

---

## 迁移说明

### 从 v3.7.x 升级

**自动迁移** - 首次启动时自动执行：

1. 检测 `config.json` 是否存在
2. 在事务中迁移所有数据到 SQLite
3. 设备级设置迁移到 `settings.json`
4. 显示迁移成功通知

**数据安全**：

- 原 `config.json` 文件保留不删除
- 迁移失败时显示错误对话框，保留`config.json`
- 支持 Dry-run 模式验证迁移逻辑

---

## 下载与安装

### 系统要求

- **Windows**：Windows 10+
- **macOS**：macOS 10.15（Catalina）+
- **Linux**：Ubuntu 22.04+ / Debian 11+ / Fedora 34+

### 下载链接

访问 [Releases](https://github.com/farion1231/cc-switch/releases/latest) 下载：

- **Windows**：`CC-Switch-v3.8.0-Windows.msi` 或 `-Portable.zip`
- **macOS**：`CC-Switch-v3.8.0-macOS.tar.gz` 或 `.zip`
- **Linux**：`CC-Switch-v3.8.0-Linux.AppImage` 或 `.deb`

### Homebrew（macOS）

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

更新：

```bash
brew upgrade --cask cc-switch
```

## 致谢

### 贡献者

感谢所有让这个版本成为可能的贡献者：

- [@YoVinchen](https://github.com/YoVinchen) - UI 和数据库重构
- [@farion1231](https://github.com/farion1231) - BUG 修复和功能增强
- 社区成员的测试和反馈

### 赞助商

**智谱AI** - GLM CODING PLAN 赞助商
[使用此链接购买可享九折优惠](https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII)

**PackyCode** - API 中转服务合作伙伴
[使用 "cc-switch" 优惠码注册享 9 折优惠](https://www.packyapi.com/register?aff=cc-switch)

**闪电说** - 本地优先的 AI 语音输入法
[免费下载](https://shandianshuo.cn) Mac/Win 双平台

**MiniMax** - MiniMax M2 CODING PLAN 赞助商
[黑五优惠进行中，套餐9.9元起](https://platform.minimaxi.com/subscribe/coding-plan)

---

## 反馈与支持

- **问题反馈**：[GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- **讨论**：[GitHub Discussions](https://github.com/farion1231/cc-switch/discussions)
- **文档**：[README](../README_ZH.md)
- **更新日志**：[CHANGELOG.md](../CHANGELOG.md)

---

## 未来展望

**v3.9.0 预览**（暂定）：

- 本地代理功能

敬请期待更多更新！

---

**Happy Coding!**

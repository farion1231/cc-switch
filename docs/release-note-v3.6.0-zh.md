# CC Switch v3.6.0

> 全栈架构重构，增强配置同步与数据保护

**[English Version →](../release-note-v3.6.0.md)**

---

## 新增功能

### 编辑模式与供应商管理

- **供应商复制功能** - 一键快速复制现有供应商配置，轻松创建变体配置
- **手动排序功能** - 通过拖拽对供应商进行重新排序，带有视觉推送效果动画
- **编辑模式切换** - 显示/隐藏拖拽手柄，优化编辑体验

### 自定义端点管理

- **多端点配置** - 支持聚合类供应商的多 API 端点配置
- **端点输入可见性** - 为所有非官方供应商自动显示端点字段

### 自定义配置目录（云同步）

- **自定义存储位置** - 自定义 CC Switch 的配置存储目录
- **云同步支持** - 指定到云同步文件夹（Dropbox、OneDrive、iCloud Drive、坚果云等）即可实现跨设备配置自动同步
- **独立管理** - 通过 Tauri Store 管理，更好的隔离性和可靠性

### 使用量查询增强

- **自动刷新间隔** - 配置定时自动使用量查询，支持自定义间隔时间
- **测试脚本 API** - 在执行前验证 JavaScript 使用量查询脚本
- **增强模板系统** - 自定义空白模板，支持 access token 和 user ID 参数

### 配置目录切换（WSL 支持）

- **目录变更自动同步** - 切换 Claude/Codex 配置目录（如 WSL 环境）时，自动同步当前供应商到新目录，无需手动操作
- **后置同步工具** - 统一的 `postChangeSync.ts` 工具，优雅处理错误而不阻塞主流程
- **导入配置自动同步** - 配置导入后自动同步，确保立即生效
- **智能冲突解决** - 区分"完全成功"和"部分成功"状态，提供精确的用户反馈

### 配置编辑器改进

- **JSON 格式化按钮** - 配置编辑器中一键 JSON 格式化
- **实时 TOML 验证** - Codex 配置的实时语法验证，带有错误高亮

### 编辑时加载 Live 配置

- **保护手动修改** - 编辑当前激活的供应商时，优先显示来自 live 文件的实际生效配置
- **双源策略** - 活动供应商自动从 live 配置加载，非活动供应商从 SSOT 加载

### Claude 配置数据结构增强

- **细粒度模型配置** - 从双键系统升级到四键系统，以匹配官方最新数据结构
  - 新增字段：`ANTHROPIC_DEFAULT_HAIKU_MODEL`、`ANTHROPIC_DEFAULT_SONNET_MODEL`、`ANTHROPIC_DEFAULT_OPUS_MODEL`、`ANTHROPIC_MODEL`
  - 替换旧版 `ANTHROPIC_SMALL_FAST_MODEL`，支持自动迁移
  - 后端在首次读写时自动规范化旧配置，带有智能回退链
  - UI 从 2 个模型输入字段扩展到 4 个，具有智能默认值
- **ANTHROPIC_API_KEY 支持** - 供应商现可使用 `ANTHROPIC_API_KEY` 字段（除 `ANTHROPIC_AUTH_TOKEN` 外）
- **模板变量系统** - 支持动态配置替换（如 KAT-Coder 的 `ENDPOINT_ID` 参数）
- **端点候选列表** - 预定义端点列表，用于速度测试和端点管理
- **视觉主题配置** - 供应商卡片自定义图标和颜色

### 供应商模型更新

- **Kimi k2** - 更新到最新的 `kimi-k2-thinking` 模型

### 新增供应商预设

新增 5 个供应商预设：

- **DMXAPI** - 多模型聚合服务
- **Azure Codex** - 微软 Azure OpenAI 端点
- **AnyRouter** - API 路由服务
- **AiHubMix** - AI 模型集合
- **MiniMax** - 国产 AI 模型提供商

### 合作伙伴推广机制

- 支持生态合作伙伴推广（智谱 GLM Z.ai）
- README 中集成赞助商横幅

---

## 改进优化

### 配置与同步

- **统一错误处理** - 后端全面使用 AppError 与国际化错误消息
- **修复 apiKeyUrl 优先级** - 修正 API key URL 解析的优先级顺序
- **修复 MCP 同步问题** - 解决同步到另一端功能失效的问题
- **导入配置同步** - 修复配置导入后的同步问题
- **配置错误处理** - 配置错误时强制退出，防止静默回退和数据丢失

### UI/UX 增强

- **独特的供应商图标** - 每个供应商卡片现在都有独特的图标和颜色识别
- **统一边框系统** - 所有组件采用一致的边框设计
- **拖拽交互** - 推送效果动画和改进的拖拽手柄图标
- **增强视觉反馈** - 更好的当前供应商视觉指示
- **对话框标准化** - 统一的对话框尺寸和布局一致性
- **表单改进** - 优化模型占位符，简化供应商提示，分类特定提示
- **使用量内联显示** - 使用量信息移至启用按钮旁边，更好地利用空间

### 完整国际化

- **错误消息国际化** - 所有后端错误消息支持中英文
- **托盘菜单国际化** - 系统托盘菜单完全国际化
- **UI 组件国际化** - 所有面向用户的组件 100% 覆盖

---

## Bug 修复

### 配置管理

- 修复 `apiKeyUrl` 优先级问题
- 修复 MCP 同步到另一端功能失效
- 修复配置导入后的同步问题
- 修复 Codex API Key 自动同步
- 修复端点速度测试功能
- 修复供应商复制插入位置（现在插入到原供应商旁边）
- 修复编辑模式下自定义端点保留问题
- 防止配置错误时的静默回退和数据丢失

### 使用量查询

- 修复自动查询间隔时间问题
- 确保刷新按钮点击时显示加载动画

### UI 问题

- 修复名称冲突错误（`get_init_error` 命令）
- 修复保存成功后语言设置回滚
- 修复语言切换状态重置（依赖循环）
- 修复编辑模式按钮对齐

### 启动问题

- 配置错误时强制退出（不再静默回退）
- 消除导致初始化错误的代码重复

---

## 架构重构

### 后端（Rust）- 5 阶段重构

1. **阶段 1**：统一错误处理（`AppError` + 国际化错误消息）
2. **阶段 2**：命令层按领域拆分（`commands/{provider,mcp,config,settings,plugin,misc}.rs`）
3. **阶段 3**：集成测试和事务机制（配置快照 + 失败回滚）
4. **阶段 4**：提取 Service 层（`services/{provider,mcp,config,speedtest}.rs`）
5. **阶段 5**：并发优化（`RwLock` 替代 `Mutex`，作用域 guard 避免死锁）

### 前端（React + TypeScript）- 4 阶段重构

1. **阶段 1**：测试基础设施（vitest + MSW + @testing-library/react）
2. **阶段 2**：提取自定义 hooks（`useProviderActions`、`useMcpActions`、`useSettings`、`useImportExport` 等）
3. **阶段 3**：组件拆分和业务逻辑提取
4. **阶段 4**：代码清理和格式化统一

### 测试体系

- **Hooks 单元测试** - 所有自定义 hooks 100% 覆盖
- **集成测试** - 关键流程覆盖（App、SettingsDialog、MCP 面板）
- **MSW 模拟** - 后端 API 模拟确保测试独立性
- **测试基础设施** - vitest + MSW + @testing-library/react

### 代码质量

- **统一参数格式** - 所有 Tauri 命令迁移到 camelCase（Tauri 2 规范）
- **语义清晰** - `AppType` 重命名为 `AppId` 以获得更好的语义
- **集中解析** - 使用 `FromStr` trait 统一 `app` 参数解析
- **DRY 违规清理** - 消除整个代码库中的代码重复
- **死代码移除** - 移除未使用的 `missing_param` 辅助函数、废弃的 `tauri-api.ts`、冗余的 `KimiModelSelector`

---

## 内部优化（用户无感知）

### 移除遗留迁移逻辑

v3.6.0 移除了 v1 配置自动迁移和副本文件扫描逻辑：

- **影响**：提升启动性能，代码更简洁
- **兼容性**：v2 格式配置完全兼容，无需任何操作
- **注意**：从 v3.1.0 或更早版本升级的用户，请先升级到 v3.2.x 或 v3.5.x 进行一次性迁移，然后再升级到 v3.6.0

### 命令参数标准化

后端统一使用 `app` 参数（取值：`claude` 或 `codex`）：

- **影响**：代码更规范，错误提示更友好
- **兼容性**：前端已完全适配，用户无需关心此变更

---

## 依赖更新

- 更新到 **Tauri 2.8.x**
- 更新到 **TailwindCSS 4.x**
- 更新到 **TanStack Query v5.90.x**
- 保持 **React 18.2.x** 和 **TypeScript 5.3.x**

---

## 安装方式

### macOS

**通过 Homebrew 安装（推荐）：**

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

**手动下载：**

- 从下方 [Assets](#assets) 下载 `CC-Switch-v3.6.0-macOS.zip`

> **注意**：由于作者没有苹果开发者账号，首次打开可能出现"未知开发者"警告。请前往"系统设置" → "隐私与安全性" → 点击"仍要打开"

### Windows

- **安装包**：`CC-Switch-v3.6.0-Windows.msi`
- **便携版**：`CC-Switch-v3.6.0-Windows-Portable.zip`

### Linux

- **AppImage**：`CC-Switch-v3.6.0-Linux.AppImage`
- **Debian**：`CC-Switch-v3.6.0-Linux.deb`

---

## 文档

- [中文文档](https://github.com/farion1231/cc-switch/blob/main/README_ZH.md)
- [English Documentation](https://github.com/farion1231/cc-switch/blob/main/README.md)
- [完整更新日志](https://github.com/farion1231/cc-switch/blob/main/CHANGELOG.md)

---

## 致谢

特别感谢**智谱 AI** 通过 GLM CODING PLAN 赞助本项目！

---

**完整变更记录**: https://github.com/farion1231/cc-switch/compare/v3.5.1...v3.6.0

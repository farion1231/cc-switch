# CC Switch 用户手册

> Claude Code / Claude Desktop / Codex / Gemini CLI / Grok Build / OpenCode / OpenClaw / Hermes / Pi / MiniMax Code 全方位辅助工具

## 目录结构

```
📚 CC Switch 用户手册
│
├── 1. 快速入门
│   ├── 1.1 软件介绍
│   ├── 1.2 安装指南
│   ├── 1.3 界面概览
│   ├── 1.4 快速上手
│   └── 1.5 个性化配置
│
├── 2. 供应商管理
│   ├── 2.1 添加供应商
│   ├── 2.2 切换供应商
│   ├── 2.3 编辑供应商
│   ├── 2.4 排序与复制
│   ├── 2.5 用量查询
│   └── 2.6 Claude Desktop
│
├── 3. 扩展功能
│   ├── 3.1 MCP 服务器管理
│   ├── 3.2 Prompts 提示词管理
│   ├── 3.3 Skills 技能管理
│   ├── 3.4 会话管理器
│   └── 3.5 工作区文件与每日记忆
│
├── 4. 本地路由与高可用
│   ├── 4.1 本地路由服务
│   ├── 4.2 应用路由
│   ├── 4.3 故障转移
│   ├── 4.4 用量统计
│   └── 4.5 连通检测
│
└── 5. 常见问题
    ├── 5.1 配置文件说明
    ├── 5.2 FAQ
    ├── 5.3 深度链接协议
    └── 5.4 环境变量冲突
```

## 文件列表

### 1. 快速入门

| 文件 | 内容 |
|------|------|
| [1.1-introduction.md](./1-getting-started/1.1-introduction.md) | 软件介绍、核心功能、支持平台 |
| [1.2-installation.md](./1-getting-started/1.2-installation.md) | Windows/macOS/Linux 安装指南 |
| [1.3-interface.md](./1-getting-started/1.3-interface.md) | 界面布局、导航栏、供应商卡片说明 |
| [1.4-quickstart.md](./1-getting-started/1.4-quickstart.md) | 5 分钟快速上手教程 |
| [1.5-settings.md](./1-getting-started/1.5-settings.md) | 语言、主题、目录、云同步配置 |

### 2. 供应商管理

| 文件 | 内容 |
|------|------|
| [2.1-add.md](./2-providers/2.1-add.md) | 使用预设、自定义配置、统一供应商 |
| [2.2-switch.md](./2-providers/2.2-switch.md) | 主界面切换、托盘切换、生效方式 |
| [2.3-edit.md](./2-providers/2.3-edit.md) | 编辑配置、修改 API Key、回填机制 |
| [2.4-sort-duplicate.md](./2-providers/2.4-sort-duplicate.md) | 拖拽排序、复制供应商、删除 |
| [2.5-usage-query.md](./2-providers/2.5-usage-query.md) | 用量查询、剩余额度、多套餐显示 |
| [2.6-claude-desktop.md](./2-providers/2.6-claude-desktop.md) | Claude Desktop 第三方供应商、直连与模型映射 |

### 3. 扩展功能

| 文件 | 内容 |
|------|------|
| [3.1-mcp.md](./3-extensions/3.1-mcp.md) | MCP 协议、添加服务器、应用绑定 |
| [3.2-prompts.md](./3-extensions/3.2-prompts.md) | 创建预设、激活切换、智能回填 |
| [3.3-skills.md](./3-extensions/3.3-skills.md) | 发现技能、安装卸载、仓库管理 |
| [3.4-sessions.md](./3-extensions/3.4-sessions.md) | 会话浏览、搜索过滤、恢复与删除 |
| [3.5-workspace.md](./3-extensions/3.5-workspace.md) | OpenClaw 工作区文件、每日记忆 |

### 4. 本地路由与高可用

| 文件 | 内容 |
|------|------|
| [4.1-service.md](./4-proxy/4.1-service.md) | 启动本地路由、配置项、接口格式转换 |
| [4.2-routing.md](./4-proxy/4.2-routing.md) | 应用路由、配置修改、状态指示 |
| [4.3-failover.md](./4-proxy/4.3-failover.md) | 故障转移队列、熔断器、健康状态 |
| [4.4-usage.md](./4-proxy/4.4-usage.md) | 用量统计、趋势图表、定价配置 |
| [4.5-model-test.md](./4-proxy/4.5-model-test.md) | 连通检测、检测参数 |

### 5. 常见问题

| 文件 | 内容 |
|------|------|
| [5.1-config-files.md](./5-faq/5.1-config-files.md) | CC Switch 存储、CLI 配置文件格式 |
| [5.2-questions.md](./5-faq/5.2-questions.md) | 常见问题解答 |
| [5.3-deeplink.md](./5-faq/5.3-deeplink.md) | 深度链接协议、生成和使用方法 |
| [5.4-env-conflict.md](./5-faq/5.4-env-conflict.md) | 环境变量冲突检测与处理 |

## 快速链接

- **新用户**：从 [1.1 软件介绍](./1-getting-started/1.1-introduction.md) 开始
- **安装问题**：查看 [1.2 安装指南](./1-getting-started/1.2-installation.md)
- **配置供应商**：查看 [2.1 添加供应商](./2-providers/2.1-add.md)
- **使用 Claude Desktop**：查看 [2.6 Claude Desktop](./2-providers/2.6-claude-desktop.md)
- **使用本地路由**：查看 [4.1 本地路由服务](./4-proxy/4.1-service.md)
- **遇到问题**：查看 [5.2 FAQ](./5-faq/5.2-questions.md)

## 版本信息

- 文档版本：v3.20.4
- 最后更新：2026-09-26
- 适用于 CC Switch v3.20.4+

### 近期主要变化

- **新增受管应用**：Grok Build（v3.18.0）、Pi（v3.20.0）、MiniMax Code（v3.20.4），受管应用共 10 个 — 详见 [1.1 软件介绍](./1-getting-started/1.1-introduction.md)
- **Codex 官方预设改为原生 Responses 直连**：DeepSeek、智谱 GLM（v3.20.2）和 Kimi（v3.20.3）等不再需要为协议转换开启本地路由— 详见 [2.1 添加供应商](./2-providers/2.1-add.md)
- **Codex 切换只写 config.toml**：第三方 API Key 不再写入 `auth.json`（v3.20.1）— 详见 [1.5 个性化配置 → Codex 应用增强](./1-getting-started/1.5-settings.md#codex-应用增强)
- **「上游格式」取代「需要本地路由映射」开关**（v3.16.5）— 详见 [2.1 添加供应商](./2-providers/2.1-add.md#codex--grok-build-的上游格式与模型映射)
- **连通检测取代模型检查**：只探测地址是否可达，不再发送真实模型请求（v3.16.3）— 详见 [4.5 连通检测](./4-proxy/4.5-model-test.md)
- **不开本地路由也能统计用量**：从各工具的本地会话记录导入 — 详见 [4.4 用量统计](./4-proxy/4.4-usage.md)
- **云同步支持 S3 兼容存储** — 详见 [1.5 个性化配置](./1-getting-started/1.5-settings.md)

## 贡献

欢迎提交 Issue 或 PR 改进文档：

- [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- [GitHub Repository](https://github.com/farion1231/cc-switch)

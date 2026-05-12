# CC Switch 用户手册

> Claude Code / Codex / Gemini CLI / OpenCode / OpenClaw 全方位辅助工具

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
│   └── 2.5 用量查询
│
├── 3. 扩展功能
│   ├── 3.1 MCP 服务器管理
│   ├── 3.2 Prompts 提示词管理
│   ├── 3.3 Skills 技能管理
│   ├── 3.4 会话管理器
│   └── 3.5 工作区文件与每日记忆
│
├── 4. 代理与高可用
│   ├── 4.1 代理服务
│   ├── 4.2 应用接管
│   ├── 4.3 故障转移
│   ├── 4.4 用量统计
│   └── 4.5 模型检查
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

### 3. 扩展功能

| 文件 | 内容 |
|------|------|
| [3.1-mcp.md](./3-extensions/3.1-mcp.md) | MCP 协议、添加服务器、应用绑定 |
| [3.2-prompts.md](./3-extensions/3.2-prompts.md) | 创建预设、激活切换、智能回填 |
| [3.3-skills.md](./3-extensions/3.3-skills.md) | 发现技能、安装卸载、仓库管理 |
| [3.4-sessions.md](./3-extensions/3.4-sessions.md) | 会话浏览、搜索过滤、恢复与删除 |
| [3.5-workspace.md](./3-extensions/3.5-workspace.md) | OpenClaw 工作区文件、每日记忆 |

### 4. 代理与高可用

| 文件 | 内容 |
|------|------|
| [4.1-service.md](./4-proxy/4.1-service.md) | 启动代理、配置项、运行状态 |
| [4.2-routing.md](./4-proxy/4.2-routing.md) | 应用路由、配置修改、状态指示 |
| [4.3-failover.md](./4-proxy/4.3-failover.md) | 故障转移队列、熔断器、健康状态 |
| [4.4-usage.md](./4-proxy/4.4-usage.md) | 用量统计、趋势图表、定价配置 |
| [4.5-model-test.md](./4-proxy/4.5-model-test.md) | 模型检查、健康检测、延迟测试 |

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
- **使用代理**：查看 [4.1 代理服务](./4-proxy/4.1-service.md)
- **遇到问题**：查看 [5.2 FAQ](./5-faq/5.2-questions.md)

## 版本信息

- 文档版本：v3.13.0
- 最后更新：2026-04-08
- 适用于 CC Switch v3.13.0+

### v3.13.0 亮点

- **轻量模式**：退出到托盘时销毁主窗口，空闲占用接近零 — 详见 [1.5 个性化配置](./1-getting-started/1.5-settings.md)
- **配额与余额展示**：官方订阅类（Claude/Codex/Gemini/Copilot/Codex OAuth）自动展示剩余额度；Token Plan 和第三方余额通过内置模板一键启用 — 详见 [2.5 用量查询](./2-providers/2.5-usage-query.md)
- **Codex OAuth 反向代理**：用 ChatGPT 账号在 Claude Code 中复用 Codex 服务 — 详见 [2.1 添加供应商](./2-providers/2.1-add.md)
- **托盘按应用分级菜单**：五应用独立子菜单，防止菜单溢出 — 详见 [2.2 切换供应商](./2-providers/2.2-switch.md)
- **Skills 发现与批量更新**：SHA-256 更新检测、批量更新、skills.sh 公共注册表搜索 — 详见 [3.3 Skills 技能管理](./3-extensions/3.3-skills.md)
- **完整 URL 端点模式**：高级选项支持将 base_url 视作完整上游端点 — 详见 [2.1 添加供应商](./2-providers/2.1-add.md)
- **OpenCode / OpenClaw 流式检测全覆盖**：Stream Check 面板扩展到全部五个应用 — 详见 [4.5 模型检查](./4-proxy/4.5-model-test.md)

## 贡献

欢迎提交 Issue 或 PR 改进文档：

- [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- [GitHub Repository](https://github.com/farion1231/cc-switch)

# Quickstart: Pi CLI 配置管理

**Feature**: specs/001-pi-cli-support
**Date**: 2026-05-23

## 前置条件

- CC Switch v3.15.0+ 已安装
- Pi CLI 已安装：`npm install -g @earendil-works/pi-coding-agent` 或 `curl -fsSL https://pi.dev/install.sh | sh`
- 至少一个 LLM 提供商的 API 密钥

## 5 分钟快速上手

### 1. 启用 Pi 选项卡

1. 打开 CC Switch → 设置 → "可见应用"
2. 开启 **Pi** 的开关（默认已启用）
3. 返回主界面，侧边栏出现 **Pi** 选项卡

### 2. 添加第一个提供商

1. 点击 **Pi** 选项卡 → **添加提供商**
2. 从预设列表中选择（如 "Anthropic (API Key)"）
3. 填入 API 密钥（如 `sk-ant-...`）
4. 如需中继/代理，修改 Base URL
5. 点击 **保存**

### 3. 切换并验证

1. 在提供商卡片上点击 **设为当前**
2. 打开终端，运行 `pi`
3. Pi 应使用刚配置的提供商启动
4. 验证方式：`pi` 启动后检查 `/model` 显示的当前模型

## 功能一览

| 功能 | 操作路径 |
|------|---------|
| 添加提供商 | Pi 选项卡 → 添加提供商 → 选择预设 → 填入 API Key → 保存 |
| 切换提供商 | Pi 选项卡 → 点击提供商的 "设为当前" 按钮 |
| 编辑提供商 | Pi 选项卡 → 点击提供商卡片 → 修改配置 → 保存 |
| 删除提供商 | Pi 选项卡 → 点击提供商卡片 → 删除 |
| 修改设置 | Pi 选项卡 → 设置子页 → 调整选项 → 保存 |
| 管理 Skills | Skills 面板 → 勾选 Pi 为目标应用 → 安装/卸载 |
| 编辑 AGENTS.md | Prompt 面板 → 选择 Pi → 编辑 → 保存 |

## 提供商类型说明

| 类型 | 使用场景 | 示例 |
|------|---------|------|
| **内置 + API Key** | 使用官方 API，自备密钥 | Anthropic API Key、OpenAI API Key |
| **中继/代理** | 通过第三方中继服务访问 | 自定义 Base URL 的中继服务 |
| **本地模型** | 自托管的 Ollama/vLLM 等 | `http://localhost:11434/v1` |

## 故障排查

### Pi 未检测到配置

- 检查 `~/.pi/agent/models.json` 是否存在且包含 CC Switch 写入的提供商（`cc-switch-` 前缀）
- 检查 `~/.pi/agent/settings.json` 中 `defaultProvider` 是否指向正确的提供商 ID
- 重启 Pi CLI

### API Key 不生效

- 确保 API Key 以正确的环境变量名填入（如 `ANTHROPIC_API_KEY`、`OPENAI_API_KEY`）
- Pi 会从环境变量读取 API Key；确保环境变量已设置或 CC Switch 已将 Key 写入 `.env` 文件

### 自定义模型不显示

- Pi 的 `/model` 命令只显示内置模型 + models.json 中定义的模型
- 确保 `models.json` 中该提供商的 `models` 数组包含你需要的模型 ID

# Copilot CLI 模型联动 VS Code Copilot

在「设置 → 应用配置 → Copilot CLI」打开「应用到 VS Code Copilot 插件」。
设置采用与 Claude Code 插件集成相同的单行开关，说明放在旁边的问号中。
供应商在 Copilot CLI 页面添加和编辑，已添加到 CLI 原生配置的模型会参与同步。

- 开启时自动识别 VS Code Stable / Insiders 的默认和命名 Profile，并同步到全部检测到的配置。继承默认模型配置的 Profile 共用一个目标文件，重新开启时刷新同步目标。
- 开启联动立即同步，在 CC Switch 中添加、编辑、移除 CLI 供应商时继续同步。在其他程序中编辑 CLI 配置后，可重新开启开关触发同步。
- VS Code 使用 Custom Endpoint 配置格式，本功能把 CLI 的 `providers.json` 转换写入所选的 `chatLanguageModels.json`。
- 联动内容为供应商、模型和请求所需的凭据。

## 数据与撤销

源文件遵循上游已有的 `COPILOT_HOME` 和 `COPILOT_PROVIDERS_CONFIG` 路径规则。
终端沿用 `COPILOT_PROVIDER_*` 环境变量时，可把 `COPILOT_PROVIDERS_CONFIG` 显式指向该文件，让 CLI 优先读取原生配置。重新启动终端后生效。
目标只更新带 `ccSwitchCopilotCli` 标记的 Custom Endpoint 分组，保留其他分组及 JSONC 注释。
API Key、Bearer Token 和请求头会随配置写入目标文件，Unix 下写入权限为 `0600`。
模型使用明确的鉴权请求头传递凭据。OpenAI 使用 `Authorization` Bearer，Azure 使用 `api-key`，这两种协议的 `bearerToken` 优先于 `apiKey`。Anthropic 同时保留 `x-api-key` 和 `Authorization` Bearer，自定义请求头按名称覆盖对应项。
第一次修改已有目标时，在旁边保存 `.cc-switch-copilot-cli.bak` 原始备份。

关闭联动会清理本功能写入的分组，保留其他配置和后续的手工修改。
CLI 原生配置和数据库先完成保存，再尝试自动联动。VS Code 同步失败时，CLI 的保存结果继续生效，提示中显示原因并提供「重试」。
待同步期间，VS Code 可能继续读取上次成功写入的配置。
开关设置和目标配置共同提交，失败时保留上次生效的设置及目标文件。多目标写入失败时回滚本次 VS Code 写入，检测到外部修改时保留对方内容。
目标发现、手动同步、状态检查和设置写入在后台阻塞线程池执行。

## 支持范围

支持 OpenAI Chat Completions、Responses、Anthropic Messages，以及 Azure OpenAI 的无版本 v1 接口。
转换保留请求模型名 `wireModel`、显示名称、API Key / Bearer Token、自定义请求头、显式 token 上限及常用能力。
`modelId` 用于 CLI 的模型识别，`wireModel` 用于请求模型名。
VS Code 要求显式 token 上限。缺省时使用输入 128000、输出 8192。已设置上下文窗口但未设置输出时，输出取 8192 与窗口四分之一的较小值。
显式填写的 CLI 上限优先，可在 CLI 模型配置中填写实际模型的上限。本功能使用固定兼容值补齐缺省项。

WebSocket、带 `apiVersion` 的 Azure 部署接口、SDK 动态凭据、模型级 `systemMessage`，以及同组重复 `wireModel` 暂不支持联动。
遇到这些配置时，VS Code 联动会显示失败原因，CLI 的保存结果保留。

## 参考

- [Copilot CLI 原生配置](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference)
- [VS Code 语言模型配置](https://code.visualstudio.com/docs/agent-customization/language-models)
- [VS Code Custom Endpoint 实现](https://github.com/microsoft/vscode/blob/main/extensions/copilot/src/extension/byok/vscode-node/customEndpointProvider.ts)

## 实测与回归

本机使用 VS Code 1.145.0 和自带的 Copilot 0.73.0，在独立的扩展开发宿主中验证。
Rust 测试从 CLI 原生注册表生成目标配置，实际 Copilot 加载这些模型并向本地 HTTP 测试服务发送请求。
验证覆盖 Chat Completions、Responses、Anthropic Messages 和 Azure v1，检查请求路径、`wireModel`、鉴权头和流式文本。鉴权回归包括 OpenAI 主凭据与网关请求头组合，以及 Anthropic 双凭据。
接口返回固定测试响应，验证产物存放在命令输出的临时目录。

    node scripts/test-copilot-vscode.mjs

Windows 默认检测用户级 VS Code 安装，也可通过 `--code` 和 `--copilot-extension` 指定实际路径。
脚本为用户数据、共享数据、扩展目录和工作区分配独立路径。
Copilot 在单元测试模式下跳过模型提供程序，因此验证脚本使用其正常的扩展开发模式。

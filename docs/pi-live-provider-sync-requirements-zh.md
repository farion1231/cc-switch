# Pi 显式供应商同步需求

## 背景

Pi 会读取 `~/.pi/agent/models.json` 中的 `providers`，并把同名显式供应商配置与自己的内置供应商、内置模型合并。当前 CC Switch 会跳过 `anthropic`、`openai`、`deepseek` 等 Pi 内置供应商 ID，导致 Pi 已经加载了显式配置，但 CC Switch 页面没有对应的已启用卡片，也无法正常移除、编辑或重新启用。

这类供应商是否属于 CC Switch 可管理范围，应只由配置来源决定，不能再由供应商 ID 或认证类型决定。

## 目标架构

整体行为与 OpenCode 的累加式供应商管理保持一致：

| OpenCode                            | Pi                                  |
| ----------------------------------- | ----------------------------------- |
| `opencode.json.provider`            | `models.json.providers`             |
| 显式 provider 同步为 CC Switch 卡片 | 显式 provider 同步为 CC Switch 卡片 |
| `/connect` 认证由 OpenCode 管理     | `/login` 与 `auth.json` 由 Pi 管理  |
| 添加和移除 provider 节点            | 启用和移除 provider 节点            |
| 数据库保留未添加供应商              | 数据库保留未启用供应商              |
| 不管理当前供应商                    | 不管理默认供应商和默认模型          |

Pi 额外需要支持同名内置供应商的显式覆盖：只要供应商节点存在于 `models.json.providers`，无论 ID 是否为 `anthropic`、`openai`、`deepseek` 或其他 Pi 内置 ID，都必须按普通显式供应商处理。

## 显式供应商同步规则

- `models.json.providers` 是 Pi 当前启用的显式供应商集合。
- 任意精确 provider ID 都可被同步，不使用内置供应商集合过滤。
- 数据库中没有对应记录时，按精确 provider ID 自动导入并 upsert。
- 数据库中已有记录时，用 Pi 当前的显式配置同步对应记录。
- provider 节点存在于 `models.json` 时，卡片显示为已启用。
- provider 只存在于 CC Switch 数据库时，卡片保留并显示为未启用。
- 已启用卡片沿用 OpenCode 的蓝色高亮。
- 外部修改 `models.json` 后，刷新供应商页面即自动同步，不要求二次确认。
- 不再出现“不是 CC Switch 管理的值”或“配置刚发生变化，请确认后重试”等 ownership 冲突提示。
- `PI_BUILTIN_PROVIDER_KEYS` 或等价集合不能参与配置归属判断，也不能用于跳过 live provider 同步。

## 认证边界

边界按配置来源划分：

- `models.json.providers` 中的显式配置由 CC Switch 管理。
- Pi `/login` 写入 `auth.json` 的凭证始终由 Pi 管理，无论其类型为 OAuth 还是 API Key。
- CC Switch 不读取、复制、修改或删除 `auth.json` 中的凭证。
- CC Switch 不刷新 OAuth Token。
- 只有 `auth.json` 凭证、没有 `models.json.providers` 节点的供应商，不生成 CC Switch 卡片。
- 环境变量中的凭证不导入 CC Switch。
- 同一供应商同时存在显式配置和 Pi 登录时，CC Switch 只管理显式配置。
- 移除显式配置不退出 Pi 登录，也不删除 `auth.json`。
- 用户在 CC Switch 中创建的 API Key 供应商仍由 CC Switch 保存并写入 `models.json`；用户通过 Pi `/login` 保存的 API Key 仍属于 Pi 原生认证状态。

## 启用与移除

### 启用

- 将 CC Switch 数据库中保存的完整配置写入 `models.json.providers.<providerId>`。
- 成功后卡片变为蓝色已启用状态。
- 不修改 Pi 默认供应商、默认模型或 `auth.json`。

### 移除

- 只删除 `models.json.providers.<providerId>`。
- 不删除 CC Switch 数据库记录。
- 卡片继续保留并变为未启用，之后可以再次启用。
- 不修改 `auth.json`、环境变量或 Pi 原生登录状态。
- 不因供应商当前正被 Pi 使用而阻止移除。
- 不提供“设为默认”操作；删除当前默认供应商后的回退行为由 Pi 处理。

所有配置写入复用现有原子文件更新能力，并保留与目标供应商无关的内容。

## Pi 内置模型合并语义

- 接受 Pi 对同名内置供应商的原生合并行为。
- 供应商卡片只根据显式 provider 节点判断启用状态。
- 编辑页面只展示该 provider 在 `models.json` 中显式保存的模型。
- 不把 Pi 的内置模型复制到 CC Switch 数据库或 `models.json`。
- 不根据 Pi 运行时出现的模型扩充供应商预设。
- 模型 ID 按精确字符串处理，不做大小写或相似名称合并。
- 同名内置供应商的显式配置必须完整保留。
- 读取、同步、编辑和保存后，未知 JSON 字段不能丢失。

`piProviderPresets`、`models.json.providers` 和 Pi 内置模型目录是三个独立数据源，不互相替代。

## 实现范围

优先复用 OpenCode 已有能力：

- live provider 导入与 upsert；
- 累加模式下的启用和移除；
- `live_config_managed` 或等价状态；
- 供应商列表刷新；
- 原子配置文件写入；
- 未知字段保留。

主要修改范围：

- Pi live provider 配置读取；
- Pi 供应商列表同步；
- 内置 provider ID 过滤规则；
- 启用和移除操作；
- 页面刷新后的状态更新。

不增加数据库 Schema，也不建立新的 ownership 状态。

明确不做：

- OAuth 或 `/login` 凭证管理；
- `auth.json` 管理；
- 环境变量导入；
- 默认供应商或默认模型管理；
- 路由与故障转移；
- 插件管理；
- 自动重写 Pi 内置模型。

## 真实 Pi 验收

1. 在 `models.json.providers` 中分别加入带 API Key 的 `anthropic`、`openai` 和 `deepseek`，刷新后均出现已启用卡片。
2. Pi `/model` 能显示并使用这些显式配置激活的模型。
3. 点击移除后，仅删除对应 provider 节点；数据库卡片保留并变为未启用。
4. 在没有 `auth.json` 凭证和相关环境变量时重启 Pi，被显式配置激活的模型不再可用。
5. 再次启用后完整配置恢复，Pi 可以继续使用。
6. 在 CC Switch 外部新增、修改或删除 provider，刷新页面后自动同步。
7. Pi `/login` 写入的 OAuth 或 API Key 凭证不被读取或修改。
8. 移除供应商前后 `auth.json` 内容与文件状态完全不变。
9. 自定义 Header、模型能力字段和未知 JSON 字段在同步、编辑、保存、移除与重新启用后不丢失。
10. 所有操作前后 Pi 当前默认供应商和默认模型保持不变。
11. 模型 ID 大小写按原值保存，不进行任何模糊合并。

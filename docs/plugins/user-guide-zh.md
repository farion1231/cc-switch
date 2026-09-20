# cc-switch 插件系统使用指南

> 本文档面向 cc-switch 用户，介绍插件系统是什么、如何安装和调试自定义插件。
> 文档内容以当前代码实现为准。开发/维护者视角请看
> [developer-notes-zh.md](./developer-notes-zh.md)；
> 内部设计契约见 [docs/dev/plugin-system-contract.md](../dev/plugin-system-contract.md)。

## 目录

1. [插件系统简介](#1-插件系统简介)
2. [三个执行时机（Stage）](#2-三个执行时机stage)
3. [fail-open：插件永远不影响转发](#3-fail-open插件永远不影响转发)
4. [内置插件](#4-内置插件)
5. [安装用户插件](#5-安装用户插件)
6. [plugin.json 字段表](#6-pluginjson-字段表)
7. [进程调用协议](#7-进程调用协议)
8. [示例插件](#8-示例插件)
9. [优先级与启用/禁用](#9-优先级与启用禁用)
10. [常见问题排查](#10-常见问题排查)
11. [安全性提示](#11-安全性提示)

---

## 1. 插件系统简介

cc-switch 本地代理在转发 Claude / Codex / Gemini 请求时，会在固定时机对请求/响应 JSON
做改写。插件系统把这些改写点开放出来：

- **内置插件**：随应用分发（如 Cache 断点注入、Thinking 优化），不可卸载，只可开关/调序；
- **用户插件**：任意语言编写的**外部脚本/可执行程序**，通过 `plugin.json` 清单 +
  stdin/stdout JSON 协议接入。

插件在一个统一的管理面板中配置：**设置 → 高级 → 插件**（`PluginSettingsPanel`），
支持启用/禁用、调整优先级、重新加载、打开插件目录。

## 2. 三个执行时机（Stage）

| Stage 值 | 时机 | 频率 | 说明 |
|---|---|---|---|
| `pre_request` | 收到客户端请求、body 解析成功后 | 每个客户端请求一次 | `provider` 为 `null`（尚未选择供应商） |
| `pre_send` | 请求即将发往某个供应商之前 | **每个供应商尝试一次**（故障转移时会重跑） | `provider` 已提供 |
| `post_response` | 非流式响应返回客户端之前 | 每个成功的非流式响应一次 | `provider` 已提供 |
| `sse_chunk` | 流式（SSE）响应逐事件 | 每个含 data 的 SSE 事件一次 | 仅常驻模式（`mode: "persistent"`）外部插件支持，见第 7 节 |

> **注意**：`sse_chunk` 是流式响应的执行时机，仅**常驻模式**外部插件支持——
> 每事件拉起一次进程的 oneshot 模式无法承载逐事件调用
> （见 [第 4 节](#4-内置插件)）——外部插件清单中声明 `sse_chunk` 仍会在加载时被拒绝；
> 没有启用的 SseChunk 插件时，SSE 流式响应按原始字节逐块透传（行为与没有插件系统时完全一致）。

## 3. fail-open：插件永远不影响转发

插件系统的核心语义是 **fail-open（失败放行）**：

- 插件脚本**崩溃**（进程非零退出）、**超时**、**输出非法**（stdout 不是合法 JSON）、
  甚至是宿主内插件逻辑 **panic**——都只会记录一条警告日志（带 `[PLUGIN]` 前缀），
  然后**跳过该插件，请求照常转发**；
- 插件失败时**不会破坏原始请求体**：改写只有在插件明确返回新 body 且解析成功时才应用；
- PostResponse 阶段改写后如果序列化失败，同样回退为原始响应。

也就是说，卸掉或禁用所有插件，代理行为与没有插件系统时完全一致；装了坏插件，
最坏结果也只是"这个插件没生效"，不会断网、不会 5xx。

## 4. 内置插件

cc-switch 目前有 3 个内置插件：两个优化器插件工作在 `pre_send` 阶段且**只对 Bedrock
供应商生效**（非 Bedrock 或 provider 信息缺失时插件直接跳过，即使配置全开）。它们的开关是
cc-switch "优化器"设置（总开关 + 子开关），每次转发时实时读取：

| 插件 ID | 默认优先级 | 生效条件（需全部满足） |
|---|---|---|
| `builtin:thinking-optimizer` | 100 | provider 为 Bedrock；优化器总开关开启；`thinking_optimizer` 子开关开启 |
| `builtin:cache-injector` | 200 | provider 为 Bedrock；优化器总开关开启；`cache_injection` 子开关开启 |

执行顺序：先 thinking 优化（100），后 cache 注入（200），与历史行为一致。

两个内置插件也可以在插件面板中单独禁用（override 优先于默认启用状态）。

## 5. 安装用户插件## 5. 安装用户插件

### 目录位置

插件目录为 **应用配置目录下的 `plugins/` 子目录**：

- Windows 默认：`C:\Users\<你的用户名>\.cc-switch\plugins`
- macOS / Linux 默认：`~/.cc-switch/plugins`

（若在设置中自定义过配置目录，则为 `<自定义配置目录>/plugins`。）

在插件面板点击"打开插件目录"可以直接创建并打开它。

### 目录结构

每个插件是一个**独立子目录**，目录里必须有一个 `plugin.json` 清单文件：

```
plugins/
└── my-plugin/            # 目录名随意（加载失败时会用它展示）
    ├── plugin.json       # 必需：插件清单
    ├── index.js          # 脚本本体（语言不限）
    └── ...               # 其他资源文件
```

### 安装步骤

方式一（推荐）：打开 cc-switch 设置 → 高级 → 插件，点击**导入插件**，在文件夹
选择器中选中插件所在目录（目录内需含 `plugin.json`），cc-switch 会校验清单、
把整个目录拷入 `plugins/` 并自动重载；同名目录已存在时会提示重命名后重试。

方式二：在 `plugins/` 下手动新建插件目录，写入 `plugin.json` 和脚本（可参考
[第 8 节](#8-示例插件)直接复制），然后点击**重新加载**；
3. 面板中出现该插件即为加载成功；如果显示为"加载失败"，错误信息会列在条目上，
   常见原因见 [第 10 节](#10-常见问题排查)。

重启 cc-switch 也会自动重新扫描插件目录。

## 6. plugin.json 字段表

清单为 UTF-8 JSON 对象，**未知字段会被忽略**（向后兼容）：

| 字段 | 类型 | 必填 | 默认值 | 约束说明 |
|---|---|---|---|---|
| `id` | string | 是 | — | 插件唯一 ID，仅允许字母/数字/`_`/`-`（正则 `[a-zA-Z0-9_-]{1,64}`），长度 1–64。注册后实际 ID 前缀为 `user:`（如 `my-plugin` → `user:my-plugin`） |
| `name` | string | 是 | — | 显示名（面板中展示） |
| `version` | string | 否 | `null` | 版本号，仅展示用 |
| `description` | string | 否 | `null` | 描述，仅展示用 |
| `stages` | string[] | 是 | — | 声明插件参与的时机，只能取 `"pre_request"` / `"pre_send"` / `"post_response"`；不能为空；`"sse_chunk"` 仅 `mode: "persistent"` 下支持 |
| `mode` | string | 否 | `"oneshot"` | 进程模式：`"oneshot"`（每次调用拉起新进程）或 `"persistent"`（常驻进程，按行交换 JSON，支持 `"sse_chunk"`），见第 7 节 |
| `priority` | number | 否 | `500` | 优先级，**数字越小越先执行**，可为负数 |
| `command` | string[] | 是 | — | 启动命令（argv 数组）。第一个元素是可执行文件：纯命令名（如 `node`）按 PATH 查找；**含路径分隔符的相对路径**（如 `./run.sh`）相对于插件目录解析；绝对路径原样使用。**插件进程的工作目录 = 插件所在目录**，因此脚本参数等相对路径（如 `["node", "index.js"]` 中的 `index.js`）也按插件目录解析 |
| `timeout_ms` | number | 否 | `10000` | 单次调用超时（毫秒），必须为 1–60000 |
| `enabled` | bool | 否 | `true` | 默认是否启用（面板中的开关可再覆盖） |
| `settings` | 任意 JSON | 否 | `null` | 任意配置，**原样透传**给插件进程（见协议输入的 `settings` 字段） |

最小合法清单示例：

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "stages": ["pre_request"],
  "command": ["node", "index.js"]
}
```

## 7. 进程调用协议

进程模式分两种（清单 `mode` 字段）：

- **oneshot（缺省）**：每当请求走到插件声明的 stage，cc-switch **启动一次插件进程**，
  通过 stdin/stdout 以 JSON 通信，进程退出后结束。每次调用都是全新进程，无跨调用状态；
- **persistent（常驻）**：首次调用时拉起进程并**保持存活**，之后每次调用通过
  stdin/stdout **按行交换 JSON**（写一行请求、读一行响应）。调用由核心逐条串行投递，
  插件进程可自带跨调用状态（例如流式处理的 per-stream 缓冲）。进程崩溃或单次调用
  超时时，核心自动重启进程并重试一次；插件被重载/删除/禁用后进程随之终止。

请求/响应的 JSON 形状在两种模式下**完全一致**；`sse_chunk` stage 仅 persistent
模式支持，请求额外携带 `event` / `data` 字段，响应为 `{"body": {"data": "…"}}`
（替换事件负载）或 `{}`（不修改）。

### 输入（cc-switch → 插件 stdin）

一个 JSON 对象，写完即关闭 stdin：

```json
{
  "stage": "pre_request",
  "app_type": "claude",
  "session_id": "sess-xxxx",
  "request_model": "claude-sonnet-4-5",
  "provider": null,
  "settings": null,
  "body": { "model": "claude-sonnet-4-5", "messages": [ ... ] }
}
```

| 字段 | 含义 |
|---|---|
| `stage` | 当前时机：`"pre_request"` / `"pre_send"` / `"post_response"` |
| `app_type` | 应用类型：`"claude"` / `"codex"` / `"gemini"` 等 |
| `session_id` | 会话 ID（无会话信息时可能为空字符串） |
| `request_model` | 请求中的 `model` 字段（缺失时为空字符串） |
| `provider` | 当前供应商信息；`pre_request` 阶段恒为 `null`；其余阶段为 `{"id": "...", "name": "...", "is_bedrock": false}` |
| `settings` | 插件清单里 `settings` 字段的原样拷贝（未配置时为 `null`） |
| `body` | 待改写的请求体（`post_response` 阶段为上游响应体） |

### 输出（插件 stdout → cc-switch）

stdout 必须**恰好输出一个 JSON 对象**（末尾换行无所谓），二选一：

- `{"body": { ...修改后的完整请求体... }}` —— 应用改写；
- `{}` —— 不修改，原样继续。

其余任何 stdout 输出（打印日志、进度等）都会导致解析失败——**日志一律写到
stderr**，stderr 内容不会被解析，仅用于调试。

### 失败处理

以下情况视为插件失败，cc-switch 记录警告后**跳过该插件**（fail-open），转发不受影响：

- 进程退出码非 0；
- 超过 `timeout_ms`（进程会被终止）；
- stdout 为空 / 不是合法 JSON / 不是 JSON 对象 / 含多余输出 / 非 `{}` 却缺 `body` 字段。

想自己写插件？[plugin-dev-guide-zh.md](./plugin-dev-guide-zh.md)
是面向开发者的完整指南（清单字段、两种进程协议、配置界面声明、调试与示例）。

## 8. 示例插件

两个示例功能相同：在请求的 `metadata` 中打一个 `via-plugin` 标记。复制即可运行。

### Node.js 示例

目录：`plugins/demo-node/`，需要本机安装 Node.js。

`plugin.json`：

```json
{
  "id": "demo-node",
  "name": "Node 示例插件",
  "version": "0.1.0",
  "description": "在 metadata 中打标（pre_request）",
  "stages": ["pre_request"],
  "priority": 500,
  "command": ["node", "index.js"],
  "timeout_ms": 10000,
  "enabled": true,
  "settings": {}
}
```

`index.js`：

```js
// 从 stdin 读取 cc-switch 传入的 JSON 输入
let raw = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (raw += chunk));
process.stdin.on("end", () => {
  let input;
  try {
    input = JSON.parse(raw);
  } catch (err) {
    // 日志必须写 stderr，绝不能 print 到 stdout
    console.error("[demo-node] 输入不是合法 JSON:", err);
    process.exit(1); // 非零退出 → cc-switch fail-open 跳过本插件
    return;
  }

  const body = input.body && typeof input.body === "object" ? input.body : {};

  // 示例改写：在 metadata 中打标
  body.metadata = Object.assign({}, body.metadata, { "via-plugin": "demo-node" });

  // stdout 恰好输出一个 JSON 对象；不需要修改时输出 {} 即可
  console.log(JSON.stringify({ body }));
});
```

### Python 示例

目录：`plugins/demo-python/`，需要本机安装 Python 3。

`plugin.json`：

```json
{
  "id": "demo-python",
  "name": "Python 示例插件",
  "version": "0.1.0",
  "description": "在 metadata 中打标（pre_request）",
  "stages": ["pre_request"],
  "priority": 500,
  "command": ["python", "plugin.py"],
  "timeout_ms": 10000,
  "enabled": true,
  "settings": {}
}
```

`plugin.py`：

```python
#!/usr/bin/env python3
"""cc-switch 示例插件：在请求 metadata 中打标。"""
import json
import sys


def main() -> None:
    try:
        input_data = json.load(sys.stdin)
    except ValueError as err:
        # 日志必须写 stderr，绝不能 print 到 stdout
        print(f"[demo-python] 输入不是合法 JSON: {err}", file=sys.stderr)
        sys.exit(1)  # 非零退出 → cc-switch fail-open 跳过本插件
        return

    body = input_data.get("body")
    if not isinstance(body, dict):
        body = {}

    # 示例改写：在 metadata 中打标
    metadata = body.get("metadata")
    if not isinstance(metadata, dict):
        metadata = {}
    metadata["via-plugin"] = "demo-python"
    body["metadata"] = metadata

    # stdout 恰好输出一个 JSON 对象；不需要修改时输出 {} 即可
    print(json.dumps({"body": body}))


if __name__ == "__main__":
    main()
```

> Windows 提示：若 `python` 不在 PATH 中，可把 `command` 改为
> `["C:\\Path\\To\\python.exe", "plugin.py"]`（含路径分隔符的相对路径会相对于插件
> 目录解析，绝对路径原样使用）。

## 9. 优先级与启用/禁用

- **优先级**：同一 stage 的插件按优先级**升序**执行（数字越小越先），同优先级保持
  注册顺序。内置插件约定占用 100–899 段位（`builtin:thinking-optimizer` = 100、
  `builtin:cache-injector` = 200），用户插件默认
  500——即默认在三个内置插件**之后**执行。可以在插件面板中修改任意插件的优先级（支持负数）。
- **三层开关**（从上到下，任何一层不通过该插件就不执行）：
  1. **全局开关**：插件面板顶部的总开关（对应配置 `plugins_config.enabled`），
     关闭后所有插件（含内置）都不参与管线；
  2. **插件开关**：清单中的 `enabled` / 面板中每个插件的启用开关；
  3. **业务开关**：内置插件还受优化器总开关和子开关控制（见第 4 节）。
- 开关与优先级的修改即时持久化（应用内即时生效，无需重启）；`plugin_reload` 只重扫
  用户插件目录与配置，不重载代理。

## 10. 常见问题排查

**插件不生效？按这个清单排查：**

1. **目录对不对**：插件必须是 `<配置目录>/plugins/<插件目录>/plugin.json`，
   不是直接把 `plugin.json` 放在 `plugins/` 根下；点击面板"打开插件目录"确认位置。
2. **重新加载了吗**：放入新插件后需点击面板"重新加载"（或重启应用）。
3. **清单校验失败**：面板条目会显示错误信息。常见：`id` 含非法字符/超 64 字符、
   `stages` 拼错（只能是 `pre_request`/`pre_send`/`post_response`）、
   `command` 缺失、`timeout_ms` 不在 1–60000 之间、JSON 本身语法错误。
4. **全局/插件开关**：检查面板顶部总开关和该插件的启用开关；全局开关关闭时，
   `plugin_list` 里所有条目都显示为禁用。
5. **内置插件"不生效"是正常的**：内置优化插件只对 **Bedrock 供应商**生效，且需要
   优化器总开关和对应子开关都打开。
6. **stage 声明错了**：想改请求却把 `stages` 写成了 `["post_response"]`，
   插件在 `pre_request` 阶段不会被调用。
7. **stdout 被日志污染**：插件往 stdout 打印了 `console.log("loaded...")` 之类的
   调试输出，导致整段 stdout 无法解析为单一 JSON。日志一律改用
   `console.error` / `print(..., file=sys.stderr)`。
8. **超时**：脚本冷启动慢（如 Python 首次导入大库）或处理耗时超过 `timeout_ms`
   会被杀掉。可在清单中调大 `timeout_ms`（上限 60000）。
9. **改了但被覆盖**：同一 stage 有更高优先级（数字更小）的插件在它之后运行会把
   `body` 再次改写；用面板调整优先级验证。

**SSE 流式响应与插件**：流式（stream=true）响应体是逐 chunk 传输的。`post_response`
只对**非流式**且上游返回成功、响应体为 JSON 的响应执行。外部插件中仅**常驻模式**
（`mode: "persistent"`）可通过 `sse_chunk` 挂点参与流式响应（逐事件调用，
插件进程自行维护跨事件状态，见第 7 节）；没有启用的 SseChunk 插件时流式响应字节级透传。

**插件报错去哪看日志**：cc-switch 应用日志中搜索 `[PLUGIN]` 前缀的警告行，
包含插件 ID 与失败原因（超时/退出码/解析错误等）。插件以非零码退出时，
失败原因会附带其 stderr 输出摘要（截断至 500 字符），可直接看到脚本报错；
超时或 stdout 协议错误不携带 stderr，可先在终端手动回放 stdin 验证。

## 11. 安全性提示

- 插件就是**以你的用户权限执行的任意本地命令**：cc-switch 会按清单 `command`
  直接 spawn 进程，脚本可以做任何你本人能做的事（读写文件、联网……）。
  **只安装来源可信的插件**，安装前阅读其源码与清单。
- `body` 包含完整的请求/响应内容（可能含对话内容、API 上下文），插件脚本可以
  随意读取并外传，请勿安装不明来源的插件。
- 插件目录与 cc-switch 配置目录同盘存放，注意备份；`plugin.json` 解析失败不会
  影响其他插件（失败条目只会在面板中提示）。

---

## 相关文档

- 开发者/维护者视角：[developer-notes-zh.md](./developer-notes-zh.md)
- 内部设计契约：[docs/dev/plugin-system-contract.md](../dev/plugin-system-contract.md)

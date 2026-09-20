# cc-switch 插件开发指南

> 面向插件开发者：如何为 cc-switch 编写外部插件——任何能读写 stdin/stdout 的语言都行
> （Python / Node / Shell / Go / Rust…）。用户视角（安装、开关、优先级）见
> [user-guide-zh.md](./user-guide-zh.md)；框架内部实现见
> [developer-notes-zh.md](./developer-notes-zh.md)；协议权威定义见
> [../dev/plugin-system-contract.md](../dev/plugin-system-contract.md)。
> 一个把所有特性（persistent、sse_chunk、config_schema 表格）用全的真实参考实现：
> [privacy 插件](https://github.com/)（独立仓库，`plugins-available` 有同步副本）。

## 0. 三分钟上手

一个插件 = 一个文件夹 + `plugin.json` + 一个可执行入口。

```
my-plugin/
├── plugin.json
└── plugin.py        （或 plugin.js / plugin.sh …语言不限）
```

`plugin.json`：

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "stages": ["pre_request"],
  "command": ["python", "plugin.py"],
  "timeout_ms": 10000
}
```

`plugin.py`：读一行 JSON（请求体），原样输出一行 JSON（改写后的 body 或 `{}` 表示不改）：

```python
import json, sys

for line in sys.stdin:
    req = json.loads(line)
    body = req.get("body", {})
    # ...在这里做你的改写...
    print(json.dumps({"body": body}, ensure_ascii=False), flush=True)
```

把文件夹放进 `<配置目录>/plugins/`（或面板"导入插件"选目录）→ 点"重新加载" → 出现在面板里。

## 1. 插件能做什么、红线是什么

cc-switch 本地代理在转发 Claude / Codex / Gemini 请求的固定时机调用你的插件，把
JSON 交给你改写：

| Stage | 时机 | 你能改 |
|---|---|---|
| `pre_request` | 收到客户端请求后 | 请求体（如改写提示词、注入内容、脱敏） |
| `pre_send` | 发往上游前（含 Bedrock 门控信息） | 最终请求体 |
| `post_response` | 上游返回后（仅非流式） | 响应体 |
| `sse_chunk` | 流式响应逐事件（仅 persistent 模式） | 事件 data 负载 |

**红线（不可妥协）**：插件任何错误、超时、panic 都只记一条 `[PLUGIN]` 警告然后跳过
你——请求照常转发（fail-open）。你的插件永远不能弄坏主链路；相应地，你的 bug 也不会
弄坏它。写插件时不要依赖"下一次调用进程还在"（oneshot 模式每次都是新进程）。

## 2. plugin.json 字段参考

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "description": "一句话说明",
  "stages": ["pre_request", "post_response"],
  "mode": "persistent",
  "priority": 500,
  "command": ["python", "plugin.py"],
  "timeout_ms": 10000,
  "enabled": true,
  "settings": { "任意": "JSON，随每次调用透传给你" },
  "config_schema": []
}
```

| 字段 | 必填 | 默认 | 说明 |
|---|---|---|---|
| `id` | ✓ | — | `[a-zA-Z0-9_-]{1,64}`，注册后前缀 `user:` |
| `name` | ✓ | — | 面板显示名 |
| `stages` | ✓ | — | 参与的时机；`sse_chunk` 仅 `mode:"persistent"` 允许 |
| `command` | ✓ | — | argv 数组；首元素含路径分隔符且为相对路径时，相对插件目录解析 |
| `mode` | | `"oneshot"` | `"oneshot"` 每次调用拉起新进程；`"persistent"` 常驻（见 §3.2） |
| `priority` | | 500 | 数字越小越先执行；同 stage 的插件按此排序 |
| `timeout_ms` | | 10000 | 单次调用超时（上限 60000） |
| `enabled` | | true | 初始启用状态（用户可在面板覆盖） |
| `settings` | | null | 任意 JSON，随每次调用透传（改它需改清单并重载） |
| `config_schema` | | `[]` | 声明式配置界面（见 §4） |

工作目录 = 插件目录；`command` 其余参数中的相对路径同样按插件目录解析。

## 3. 进程协议

两种模式的 **JSON 形状完全一致**，区别只在进程生命周期与传输帧。

### 3.1 oneshot（缺省）：一次调用 = 一个进程

stdin 收到**一个** JSON 对象（写完即关闭）：

```json
{
  "stage": "pre_request",
  "app_type": "claude",
  "session_id": "sess-…",
  "request_model": "claude-sonnet-4-5",
  "provider": null,
  "settings": {},
  "body": { "...完整的请求体...": "" }
}
```

- `provider`：`pre_request` 恒为 null；其余阶段为 `{"id","name","is_bedrock"}`。
- stdout 输出**恰好一个** JSON 对象，二选一：
  - `{"body": {…修改后的完整 body…}}` —— 应用改写；
  - `{}` —— 不修改。
- **任何日志都必须写 stderr**；stdout 混入日志 = 解析失败 = 该次调用被跳过。

以下情况会被核心跳过（fail-open，不影响转发）：退出码非 0、超时（进程被杀）、
stdout 为空/非法 JSON/缺 `body`。

### 3.2 persistent（常驻）：一次拉起，按行交换

适合：SSE 流处理、需要跨调用缓存/状态的插件、启动慢的运行时（只冷启动一次）。

- 首次调用时核心拉起你的进程并**保持存活**；之后每次调用：stdin 写**一行**请求
  JSON（追加 `\n`），期望 stdout 回**一行** JSON（不带换行）。
- 请求/响应形状与 oneshot 一致；调用由核心**逐条串行**投递——你不需要处理并发。
- 单次超时或进程崩溃：核心自动 kill 并重启进程、重试一次；再失败按错误跳过。
- 插件被禁用/删除/重载时进程被终止；重新启用后下次调用会重新拉起。
- stderr 随便写（核心后台排空并记 debug 日志），不会堵。
- 退出时机：进程收到 stdin 关闭（cc-switch 退出）或自身崩溃——把主循环写成
  "逐行读、读到 EOF 就退出"即可。

选择建议：只在 `pre_request`/`post_response` 做无状态改写 → oneshot 足够；
要处理 `sse_chunk` 或想跨调用缓存 → persistent。

### 3.3 sse_chunk（仅 persistent）

每个 SSE 事件调用一次，请求额外携带：

```json
{ "stage": "sse_chunk", "event": "content_block_delta", "data": "{\"index\":0,…}", … }
```

- `data` 是该事件的原始负载字符串；响应为 `{"body": {"data": "改写后整串"}}` 或 `{}`。
- 流是**逐步达的**：一个标记/一段文本可能被拆到多个事件里。需要"完整才处理"的语义时，
  在你的进程里为每条流维护缓冲（按 `session_id` + 事件里的 `index` 区分流与块），
  尾部疑似不完整的片段先扣下不发，与下一段拼接后再处理；
  `content_block_stop` / `message_delta` / `message_stop` 时清理缓冲。
- 核心**不会**替你管理流状态——状态完全在你自己的进程里（这正是 persistent 模式
  存在的意义）。

### 3.4 config：让面板渲染你的设置（stage=config）

在 plugin.json 声明 `config_schema` 后，面板会为你的插件显示"设置"按钮，渲染一个
通用表单；用户点"保存"时核心把**整份配置文档**经协议交给你校验与落盘：

```json
{
  "stage": "config",
  "op": "get"
}
```

→ 你回一行：`{"config": {"config.json": {…}, "rules.json": {…}}}`
（键 = 配置文件名，值 = 该文件的完整 JSON 文档；需要几个文件就返回几个）。

```json
{ "stage": "config", "op": "set", "config": { "config.json": {…}, … } }
```

→ 你校验后写盘（建议原子写：临时文件 + rename），回：
- `{}` —— 保存成功；
- `{"error": "人类可读的拒绝原因"}` —— 校验失败，**这条会原样显示在面板弹窗里**，
  把错误写清楚（如 `rules[0].pattern 正则无效: …`）。

schema 字段项的完整形状（与面板渲染器一一对应）：

```json
{
  "type": "toggle | text | number | select | textarea | table",
  "key":  "字段键（展示/排序用）",
  "file": "config.json",
  "path": "文档内的点路径，如 hash.algorithm；缺省 = key",
  "label": "显示名",
  "description": "说明文字（可省）",
  "options": ["select 的可选项"],
  "columns": [ { "key": "列键", "type": "text|number|toggle|textarea|select",
                 "label": "列名", "options": ["…"] } ]
}
```

- `file` 必须是插件目录内的相对 `.json` 路径（允许子目录如 `json/config.json`，
  禁止 `..` 逃逸）；核心**不读不写**这些文件——读写全部经协议由你完成。
- `table` 的 value 是对象数组，面板渲染成可增删行的表格（每列按 column.type 渲染），
  保存时把整个数组交回给你。
- 落盘后按你的引擎热重载机制生效即可（cc-switch 不缓存你的配置）。

## 4. 调试与日志

- **你的 stderr** → cc-switch 应用日志（`~/.cc-switch/logs/cc-switch.log`），带
  `[PLUGIN]` 前缀的行是核心对插件的警告（超时/崩溃/解析失败）。
- 面板"重新加载"= 杀掉 persistent 进程重建、重读所有清单——改完 plugin.json /
  代码后点它即可，无需重启 cc-switch。
- 常见坑：
  1. **Windows 管道编码**：Python 里对 stdin/stdout/stderr `reconfigure(encoding="utf-8")`，
     否则非 ASCII（中文/特殊符号）会被本地编码写坏；
  2. **stdout 污染**：print 调试信息到 stdout = 协议失败，调试信息一律走 stderr；
  3. **stderr 堆积**：oneshot 模式核心已并发排空，persistent 模式由核心后台排空，
     你只管写；
  4. **超时杀进程**：oneshot 超时进程被杀——慢启动（如 Python 首次导入大库）在
     persistent 模式下只发生一次；
  5. **面板没出现"设置"按钮**：`config_schema` 为空数组 = 无配置界面。

## 5. 最小示例集

**Python（oneshot，改写模型名）**：

```python
import json, sys
for line in sys.stdin:
    req = json.loads(line)
    body = req.get("body", {})
    body["model"] = "claude-sonnet-4-5"
    print(json.dumps({"body": body}, ensure_ascii=False), flush=True)
```

**Node（oneshot，读取 settings）**：

```js
let raw = "";
process.stdin.on("data", (d) => (raw += d));
process.stdin.on("end", () => {
  const req = JSON.parse(raw);
  const tag = req.settings?.tag ?? "via-plugin";
  const body = req.body;
  body.metadata = Object.assign({}, body.metadata, { via: tag });
  process.stdout.write(JSON.stringify({ body }) + "\n");
});
```

**Python（persistent 骨架）**：

```python
import json, sys
for line in sys.stdin:                # 读到 EOF（cc-switch 退出/禁用）就结束
    req = json.loads(line)
    # 你的状态可以放在模块级变量里（按 req["session_id"] 区分流）
    resp = {}                         # 或 {"body": {...}}
    print(json.dumps(resp, ensure_ascii=False), flush=True)
```

## 6. 打包与发布

- 插件目录即发行物：把文件夹（含 plugin.json 与全部脚本，**不含**运行时数据如映射表、
  密钥缓存）打包为 zip，或让用户"导入插件"选目录；
- 多份配置示例可以随目录附带（如 `json/` 子目录），config_schema 里只声明你要暴露的；
- 升级：改版本号 → 用户重新导入/覆盖目录 → 面板"重新加载"。

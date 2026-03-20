# Proxy Data 日志结构说明

本文档说明 `cc-switch` 代理 `data` 模式下的新日志结构、字段含义，以及这次改造解决了什么问题。

## 目标

新的 `data` 日志不再按“每个底层请求一条扁平记录”写入，而是按 `session` 聚合，便于：

- 还原用户真正看到的对话轮次
- 查看某一轮背后实际触发了哪些 Claude Code 后台请求
- 为后续模型路由、数据分析和质量评估保留更稳定的基础数据

日志文件默认写入：

```text
~/.cc-switch/logs/cc-switch-data/<session_id>.json
```

## 顶层结构

```json
{
  "session_id": "sess_xxx",
  "app": "Claude",
  "started_at": "2026-03-20T11:50:38+08:00",
  "updated_at": "2026-03-20T11:50:42+08:00",
  "summary": {
    "turn_count": 1,
    "internal_request_count": 3
  },
  "turns": []
}
```

字段说明：

- `session_id`: 当前会话标识
- `app`: 来源应用，例如 `Claude`、`Codex`、`Gemini`
- `started_at`: 会话首次写入时间
- `updated_at`: 最近一次写入时间
- `summary.turn_count`: 用户可见轮次数量
- `summary.internal_request_count`: 当前 session 内记录到的内部请求数量

## turns 的定义

`turns` 只表示用户真正看到的对话轮次。

每个 `turn` 包含：

```json
{
  "turn_index": 1,
  "user": {
    "text": "请解释什么是 Transformer",
    "timestamp": "2026-03-20T11:50:38+08:00"
  },
  "assistant": {
    "text": "Transformer 是一种基于自注意力机制的模型架构。",
    "timestamp": "2026-03-20T11:50:41+08:00"
  },
  "internal_requests": []
}
```

这里有两个重要约束：

- `turn.user.text` 和 `turn.assistant.text` 只保留用户可见内容
- 注入类内容例如 `<system-reminder>...</system-reminder>` 不应污染 `turns`

## internal_requests 的定义

`internal_requests` 表示当前 `turn` 相关的 Claude Code 后台 API 请求记录。

当前结构示例：

```json
{
  "trace_id": "b1def808-5d8b-431b-8168-a805dd479d9f",
  "kind": "main_response",
  "source": "proxy_data",
  "model": "claude-opus-4-5-20251101",
  "request_messages": [],
  "response": {
    "role": "assistant",
    "content": "Transformer 是一种基于自注意力机制的模型架构。"
  },
  "status": "success",
  "latency_ms": 1530,
  "usage": {
    "input_tokens": 1200,
    "output_tokens": 180,
    "cache_read_tokens": 0,
    "cache_creation_tokens": 0
  },
  "timestamp": "2026-03-20T11:50:41+08:00"
}
```

字段说明：

- `trace_id`: 单次底层请求唯一标识
- `kind`: 请求类型
- `source`: 记录来源，目前为 `proxy_data`
- `model`: 实际解析到的模型名，能拿到时才写入
- `request_messages`: 该底层请求的消息输入
- `response`: 提取后的响应内容
- `status`: 当前请求状态，例如 `success`
- `latency_ms`: 请求耗时，能拿到时才写入
- `usage`: token 使用量，能拿到时才写入
- `error`: 错误信息，失败时才写入
- `timestamp`: 该内部请求记录时间

## kind 的用途

`kind` 用来区分不同后台请求，便于过滤和分析。

当前已覆盖的典型类型包括：

- `main_response`
- `warmup`
- `topic_detection`
- `title_generation`

## 本次改造解决的问题

这次日志改造主要修复了以下问题：

1. 旧结构把每个底层请求都当成一条扁平日志，不利于还原真实对话
2. `warmup`、`topic_detection` 这类内部请求曾错误地生成可见 `turn`
3. 注入内容会污染用户可见文本
4. 下一轮开始前发生的内部请求，可能被错误挂到上一轮
5. 流式响应路径最初没有把 `model / latency_ms / usage` 一起写入

改造后：

- `turns` 回到“用户真正看到的轮次”
- `internal_requests` 聚合到底层请求级别
- 主响应前出现的内部请求会先缓存，再归到正确的后续轮次

## 当前边界

当前日志结构已经适合作为“代理层事实数据”基础，但仍建议把它理解为：

- 对话可见层：`turns`
- 后台请求层：`internal_requests`

如果未来要继续做模型路由训练，还可以在每个 `turn` 上继续补充：

- `routing`
- `outcome`
- 离线质量标签

这些字段本次没有伪造写入，因为代理层当前并不直接拥有这部分事实数据。

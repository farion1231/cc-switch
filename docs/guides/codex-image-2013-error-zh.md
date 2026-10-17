# Codex attach 图片报 2013 误报 "context window exceeds limit" 的来龙去脉

> 适用 CC Switch v3.16.4 及更早版本。已提上游 issue #4820。

## 这篇文档要解决什么

用 CC Switch 把 Codex CLI 路由到 MiniMax 系 provider (`MiniMax-M3`,
`MiniMax-M2.7` 等) 时, 部分用户在 Codex 对话框里 attach 图片会撞到这个错:

```
CC Switch local proxy failed while handling Codex endpoint /responses.
Provider: MiniMax; model: MiniMax-M3;
upstream_status: HTTP 400;
cause: invalid params, context window exceeds limit (2013)
```

报错信息说 **"context window"**, 会让用户去清历史/换模型, 走错路。

## 真因

MiniMax API 用同一个错误码 **2013** 表示多种不同的失败:

```json
// 真的 context window 溢出 (200K tokens 超了)
{"error":{"message":"prompt is too long (2013)",
          "code":"invalid_prompt"}}

// 单张图片 > 10MB
{"error":{"message":"media exceeds size limit: max 10485760 bytes (2013)",
          "code":"invalid_prompt"}}
```

CC Switch 上游错误包装器现在把 **所有** 2013 一律翻译成 "context window exceeds limit", 两种 case 完全没区分。所以用户看到同一条消息, 真实原因可能是上下文长度, 也可能是图片大小。

直接 `curl` 打 `https://api.minimaxi.com/v1/responses` 验证:

```bash
$ curl .../v1/responses -d '{ "model": "MiniMax-M3",
                              "input": [...10MB+ 大小的 base64 图片...] }'
{"error":{"message":"media exceeds size limit: max 10485760 bytes (2013)",
          "code":"invalid_prompt"}}
```

`MiniMax-M3` 实际 context window 是 200K+。一个 token 数很少但图片很大的请求, 失败原因绝对是 media size, 不是 context。

## 怎么判断是哪种 case

| 现象 | 大概率是 |
|---|---|
| 长对话历史 + 图片 | 真 context overflow (少见) |
| 单张 > 10MB 的图 (视网膜屏内容复杂的网页 / PDF 页面) | media size (常见) |
| 对话不到 1K tokens + 图片就失败 | 几乎肯定是 media size |

最快确认: 直接 `curl` 把同样 payload 打上游 `api.minimaxi.com`, 看返回的 `error.message` 原文。

## 建议的上游修法 (issue #4820 已提)

1. CC Switch 收到 MiniMax 系 provider 的 `2013` 时, 直接把上游 `error.message` 透传给 Codex UI, 不要替换成人话字符串。上游 message 已经写明 "media" 还是 "prompt", 不用翻译。
2. 可选: 在 forwarder 错误映射层加一个小的分类查表 (`2013` + `"media"` → "图片超过 10MB 上限", `2013` + `"prompt"` → "对话太长")。

这是 forwarder 错误映射层的小改动, 不需要 provider 侧任何改动。

## 临时 workaround (用户侧)

上游修法发版前, 用 [Konsn666/codex-img-prep](https://github.com/Konsn666/codex-img-prep)
这个客户端工具预处理图片, 保证都在 MiniMax 的 10MB 上限内。一行安装:

```bash
curl -fsSL https://raw.githubusercontent.com/Konsn666/codex-img-prep/main/install-remote.sh | bash
```

装完后:

- 截图复制到剪贴板 → 2 秒内自动 resize (最长边 1920px, JPEG q85, ≤ 8MB)
- Finder 右键任意图片 → **Quick Actions → Compress for Codex**
- Shell aliases: `prep-clip` / `prep-img` / `prep-watch`

另外 installer 还会把 Codex provider 的 `meta.apiFormat` 从 `openai_chat` 改成
`openai_responses`, 这样 forwarder 不再做 lossy `/responses → /chat/completions` 翻译,
日志也能跟实际请求路径对上。

## 环境

- CC Switch: v3.16.4
- Codex CLI: 最新, `wire_api = "responses"`
- MiniMax-M3 via `api.minimaxi.com`
- macOS 15.4 (Apple Silicon) — workaround 只支持 macOS 因为用了系统内置的 `sips`; Linux/Windows 用户需要换成 ImageMagick

## 相关

- 上游 issue: https://github.com/farion1231/cc-switch/issues/4820
- Workaround repo: https://github.com/Konsn666/codex-img-prep
- 端到端验证: 38.64MB 截图 → pipeline 后 1.04MB → CC Switch → MiniMax → 200 OK, 视觉识别正确
---

## 英文版 / English version

完整的英文版见 [Konsn666/codex-img-prep README](https://github.com/Konsn666/codex-img-prep/blob/main/README.md),
英文 issue 稿见 [ISSUE.md](https://github.com/Konsn666/codex-img-prep/blob/main/ISSUE.md)。

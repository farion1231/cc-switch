# Codex 微信助手 PoC

这是一个最小可跑的 HermesClaw 风格接入：

```text
微信 iLink -> 长轮询收消息 -> codex app-server -> sendmessage 发回微信
```

它是独立工具，不会修改 Codex++ 管理工具、Codex 配置或桌面注入逻辑。

## 安全默认值

- 默认 `allow_all=false`，没有加入 `allow_user_ids` 的用户不会触发 Codex。
- 默认 `require_prefix=true`，授权用户也需要发送 `/codex 你的问题`。
- 默认 Codex 使用 `app-server` 常驻后端，权限为 `read-only` / `never`。
- `/id` 默认允许未授权用户使用，方便拿到自己的 iLink user_id；如果不需要，改成 `allow_id_command_for_unauthorized=false`。

不要把包含 `token` 的配置文件提交到 Git。

## 使用方式

复制配置样例：

```powershell
Copy-Item tools/codex-wechat/config.example.json tools/codex-wechat/config.local.json
```

扫码登录并写入 token：

```powershell
python tools/codex-wechat/codex_wechat.py login --output tools/codex-wechat/config.local.json
```

先让目标微信用户发送：

```text
/id
```

把返回的 `iLink user_id` 填进 `allow_user_ids`：

```json
{
  "allow_user_ids": ["这里填 user_id"]
}
```

启动：

```powershell
python tools/codex-wechat/codex_wechat.py run --config tools/codex-wechat/config.local.json
```

授权用户发送：

```text
/codex 帮我总结一下当前项目结构
```

## 配置说明

- `base_url`：iLink base URL，扫码登录后可能会写入重定向后的地址。
- `token`：iLink bot token，也可用环境变量 `WEIXIN_TOKEN` 或 `ILINK_TOKEN` 覆盖。
- `account_id`：机器人账号 ID，用于过滤自己发出的消息。
- `allow_user_ids`：允许调用 Codex 的 iLink 用户 ID。
- `prefix`：触发前缀，默认 `/codex`。
- `codex.backend`：`app-server` 或 `exec`。默认 `app-server`，失败时会回退到 `exec`。
- `codex.workspace`：Codex 工作目录。
- `codex.model`：可选，传给 `--model`。
- `codex.profile`：可选，传给 `--profile`。
- `codex.app_server_command`：可选，指定原生 `codex.exe` 路径；留空时会尝试从 `codex.cmd` 自动定位。
- `codex.extra_args`：仅 `exec` 回退路径使用的额外参数。

## 当前限制

- 这是 PoC，`app-server` 后端会按微信用户复用一个临时 thread；重启脚本后上下文会丢失。
- 图片、文件、视频暂时只会转成文字占位说明，尚未下载并传给 Codex。
- 微信长回复会按 `max_reply_chars` 分段发送。
- 后续如果要产品化，建议把 thread 映射持久化，并接进 Codex++ 管理工具。

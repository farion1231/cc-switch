# cc-switch 敏感信息走环境变量（zh）

> 适用版本：v3.20.4+（feat: kaixuan bundle + env-var expansion）
> 适用场景：API Key、auth token 等敏感字段不希望落 `~/.cc-switch/cc-switch.db` 或 `~/.codex/auth.json`。

## TL;DR

API Key 字段直接写 `$MY_ENV_VAR` 或 `$MY_ENV_VAR` 占位符，cc-switch 在切档前会从进程环境里读出来并展开；落盘字段里只剩空字符串（如果你想完全没值）或加密值（沿用既有落盘逻辑）。

```text
# ~/.zshenv
export KAIXUAN_KXPMS_KEY="sk-live-xxxxxxxx"
export KAIXUAN_LOCAL_KEY="sk-local-yyyyyyyy"
```

UI 输入框填：

```text
$KAIXUAN_KXPMS_KEY
```

cc-switch 启动 → 用户点「安装/重装」→ 后端把 `$KAIXUAN_KXPMS_KEY` 展开成 `sk-live-...` 写进 `~/.codex/auth.json` 与 settings.db → 不再有明文 `$KAIXUAN_KXPMS_KEY` 字面量留在数据库里。

## 支持的语法

| 语法 | 示例 | 说明 |
|---|---|---|
| `$NAME` | `$MY_KEY` | POSIX 风格；`NAME` 首字符必须是字母或下划线（`$5` 当字面量） |
| `${NAME}` | `${MY_KEY}` | 括号风格；适合「紧贴字母数字」时显式分界（`${KEY}_v2`） |
| 多个 | `prefix-$A-${B}-suffix` | 同一字段多个占位符都展开 |

**未闭合 `${`** 按字面处理（避免把 `${ABC` 误吃成 `$ABC` 变量）。

## 行为契约

1. **空字符串** → 当「未填」，落盘字段为空。前端会在 bundle install 完成后用红色 banner 标出「缺失 env」。
2. **占位符 + env 已定义且非空** → 展开成真实值后再写库。原始 `$NAME` 不留。
3. **占位符 + env 缺失或为空** → 落盘为空字符串；后端返回 `missingEnvVars: [...]`，UI 提示用户。
4. **混合**（如 `prefix-${A}-suffix`）→ 各自独立展开，缺失的子项变空。

## 推荐部署

### macOS（launchd 持久环境变量）

```bash
# 1. 写入 ~/.zshenv（仅当前用户）
cat >> ~/.zshenv <<'EOF'
export KAIXUAN_KXPMS_KEY="sk-live-xxxxxxxx"
export KAIXUAN_LOCAL_KEY="sk-local-yyyyyyyy"
EOF
source ~/.zshenv

# 2. 让 GUI 应用（cc-switch.app）也能拿到
# macOS GUI 不读 ~/.zshenv；用 launchctl setenv 写到用户域
launchctl setenv KAIXUAN_KXPMS_KEY "$KAIXUAN_KXPMS_KEY"
launchctl setenv KAIXUAN_LOCAL_KEY "$KAIXUAN_LOCAL_KEY"

# 3. 重启 cc-switch.app 让它继承新 env
```

### macOS（Keychain 单文件）

```bash
# 把密钥存进 Keychain，应用启动时用 security 命令读
security add-generic-password -a "$USER" -s "kaixuan-kxpms-key" -w
# 在 ~/.zshenv 里改成：
# export KAIXUAN_KXPMS_KEY="$(security find-generic-password -a "$USER" -s kaixuan-kxpms-key -w)"
```

### Linux（systemd 用户实例）

```text
# ~/.config/systemd/user/cc-switch.service.d/env.conf
[Service]
Environment="KAIXUAN_KXPMS_KEY=sk-live-xxxxxxxx"
Environment="KAIXUAN_LOCAL_KEY=sk-local-yyyyyyyy"
```

## 安全约束

1. **不要把 `$NAME` 写进 Git**。`.env*` 已经在 `.gitignore`，但 bundle install 流程里 `$NAME` 本身会进 settings.db 的 settings_config JSON——如果开了「导出全配置到 dotfiles 仓库」等同步功能，记得把 settings.db 加入 ignore。
2. **不要 echo 展开后的真值**。`echo $KAIXUAN_KXPMS_KEY` 会写到 shell history；改用 `echo ${KEY:0:6}...`。
3. **占位符是字符串匹配，不是表达式**。`$((1+1))`、`${VAR:-default}` 都不展开——保持简单可审计。
4. **多用户系统上慎用 `launchctl setenv`**。同一 GUI 域内所有应用都能读到；非个人机器请用 Keychain。

## 调试：bundle install 后看不到真值

```bash
# 1. 看后端是否真展开了
sqlite3 ~/.cc-switch/cc-switch.db \
  "SELECT settings_config FROM providers WHERE id='kaixuan-kxpms';" \
  | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d['auth'])"
# 期望输出：{"OPENAI_API_KEY": "sk-live-xxxx"} （非空）

# 2. 看 cc-switch 进程是否继承到 env
lsof -p $(pgrep -f "cc-switch") 2>/dev/null | grep -i "kaixuan" || echo "env 没传进进程"

# 3. 看 ~/.codex/auth.json 是否真写了
cat ~/.codex/auth.json | python3 -c "import sys,json; print(json.load(sys.stdin))"
```

如果 (1) 输出 `{"OPENAI_API_KEY": ""}` 而你 UI 填的是 `$KAIXUAN_KXPMS_KEY`：
- 检查 cc-switch 进程的环境：`ps eww -p $(pgrep -f cc-switch)` 看 `KAIXUAN_KXPMS_KEY=...` 是否在
- macOS GUI 没继承到 `~/.zshenv` → 用 `launchctl setenv`（见上文）

## 已知限制

- **不递归展开**：`${${A}_B}` 不支持；超界场景请直接写新 env var。
- **不缓存**：每次切档/启动都会重新读 env。重启 cc-switch 后 env 改了立即生效。
- **不加密**：env 本身是明文（依赖 OS 进程隔离）；想要静态加密请改用 OS Keychain + 上文 launchd 注入方案。
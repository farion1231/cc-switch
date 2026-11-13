# 插件 CLI 输出样例

插件页靠五家 CLI 的 `--json`（和少量纯文本）输出工作。这里是各家 CLI 的真实输出，
`../cli_fixtures.rs` 用它们跑解析：CLI 升级改了格式，测试先报出来。

| 目录 | CLI 版本 | 抓取日期 |
| --- | --- | --- |
| `claude-2.1.318/` | Claude Code 2.1.318 | 2026-11-06 |
| `codex-0.167.0/` | codex-cli 0.167.0 | 2026-11-06 |
| `grok-1.0.70/` | grok 1.0.70 (f50904eaee6c) | 2026-11-06 |
| `mcode-0.7.4/` | MiniMax Code 0.7.4 | 2026-11-06 |
| `openclaw-2026.11.1/` | OpenClaw 2026.11.1 (4ffdd20) | 2026-11-06 |

## 怎么抓的

每家都用临时目录隔离配置（`CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`GROK_HOME`、
`MINIMAX_DATA_DIR`、`OPENCLAW_STATE_DIR` + `OPENCLAW_CONFIG_PATH`），stdin 接 `/dev/null`
（和 CC Switch 调 CLI 时一样），不碰本机真实配置：

- Claude Code：本地测试市场 `ccs-test`，一个普通插件 `hello`（装好后停用），一个命令来源的
  插件 `cmdplug`（`install-needs-confirm.stdout`：不传 `-y`、stdin 不是终端时 CLI 只显示
  命令、立刻失败）。
- Grok Build：同样的测试市场复制一份，叫 `gmkt`；`hello` 装好后停用，改版本号再更新一次
  （`update-changed.txt`）。
- Codex：本地测试市场 `ccs-test`，`hello`（`AVAILABLE`，装好后在 `config.toml` 里停用）
  和 `locked`（`NOT_AVAILABLE`）。
- MiniMax Code：官方市场。安装要登录（`add-auth-required.stderr`），所以没有已安装的样例。
- OpenClaw：从 ClawHub 装了 `@openclaw/airtable`（`origin: global`），其余是随程序自带的。

## 整理过的地方

- 内容是 CLI 原样输出；JSON 文件重新按两格缩进排版过。
- 本机路径全部换成 `C:\fixture\…`（抓取用的临时目录、OpenClaw 的解包目录都在它下面），
  Grok `repo_key` 的路径前缀换成 `c--fixture`、末尾由真实路径算出的 hash 换成 `00000000`。
  样例里不含用户名、主机名或本机真实路径。
- Codex 的 `add-not-found.stderr` 去掉了临时目录触发的 PATH 别名 `WARNING:` 行（只在
  抓取环境里出现）。
- 大目录截短，留下的条目原样：Claude 的 `plugin-list-available.json`（官方目录 3605 项，
  留测试市场 + 3 项）、MiniMax Code 的 `plugin-list-available.json`（115 项留 4 项）、
  OpenClaw 的 `plugins-list.json`（70 项留 3 项）和 `marketplace-entries.json`（183 项留 4 项）。

## CLI 升级后

按上面的方法重抓一份放进新的 `<cli>-<版本>/` 目录，把 `cli_fixtures.rs` 里的路径指过去，
测试挂了就说明解析要跟着改。旧目录没有测试引用后删掉。

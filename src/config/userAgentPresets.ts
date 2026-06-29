/**
 * 自定义 User-Agent 预设。
 *
 * 取值来自 PR #3671 对 Kimi Coding Plan（api.kimi.com/coding）UA 白名单的 curl 实测：
 * `claude-cli/*`、`claude-code/*`、`Kilo-Code/*` 可通过；`codex-cli`、`kimi-cli` 会被 403。
 * 白名单只校验 UA 名称前缀、不看版本号，因此用静态值即可，版本不会因 Claude Code 升级而失效。
 *
 * 第一条是官方 Claude Code CLI 实际发送的完整格式（参见 `stream_check.rs` 里检测用的
 * `claude-cli/2.1.2 (external, cli)`），最贴近真实客户端、最稳过严格的 UA 校验；其余为简短变体。
 *
 * 这些预设主要用于"非白名单 Coding Agent（Codex/Gemini/Hermes/OpenClaw 等）想接入受 UA
 * 限制的上游"的场景——把转发请求伪装成已在白名单内的客户端。是否使用由用户显式选择。
 */
export const USER_AGENT_PRESETS: readonly string[] = [
  "claude-cli/2.1.161 (external, cli)",
  "claude-cli/2.1.161",
  "claude-code/1.0.0",
  "claude-code/0.1.0",
  "Kilo-Code/1.0",
];

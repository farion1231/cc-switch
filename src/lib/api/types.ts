// AppId 包含 UI 可切换和使用统计可筛选的全部应用。
export type AppId =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "openclaw"
  | "hermes"
  | "cursor";

// 旧配置领域使用 ManagedAppId，Cursor 不参与通用 Provider/Proxy/MCP/Skill 状态机。
export type ManagedAppId = Exclude<AppId, "cursor">;

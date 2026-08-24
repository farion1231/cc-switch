// 前端统一使用 AppId 作为应用标识（与后端命令参数 `app` 一致）
export type AppId =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "codex-desktop"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "openclaw"
  | "hermes"
  | "pi";

export type CodexAppId = Extract<AppId, "codex" | "codex-desktop">;

export function isCodexAppId(appId: string): appId is CodexAppId {
  return appId === "codex" || appId === "codex-desktop";
}

// 前端统一使用 AppId 作为应用标识（与后端命令参数 `app` 一致）
export type AppId =
  | "claude"
  | "claude-xcode"
  | "claude-desktop"
  | "codex"
  | "gemini"
  | "opencode"
  | "openclaw"
  | "hermes";

/**
 * 是否为 Claude Code 形态的应用（Claude 与 Claude (Xcode) 完全对等）。
 * 用于表单/校验等渲染分支，使 claude-xcode 复用 Claude 的全部 UI 行为。
 * 注意：不含 claude-desktop（其表单/逻辑独立）。
 */
export const isClaudeApp = (appId: AppId): boolean =>
  appId === "claude" || appId === "claude-xcode";

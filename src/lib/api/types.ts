// 前端统一使用 AppId 作为应用标识（与后端命令参数 `app` 一致）
export type AppId =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "copilot-byok"
  | "openclaw"
  | "hermes"
  | "pi"
  | "kimi"
  | "dsh"
  | "mcode"
  | "agy"
  | "omp";

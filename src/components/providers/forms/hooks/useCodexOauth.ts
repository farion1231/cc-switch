import { useManagedAuth } from "./useManagedAuth";
import type { CodexLoginMethod } from "@/lib/api/auth";

/**
 * Codex OAuth (ChatGPT Plus/Pro) 认证 hook
 *
 * 复用通用 useManagedAuth，仅指定 provider 为 "codex_oauth"
 */
export function useCodexOauth(loginMethod: CodexLoginMethod = "browser") {
  return useManagedAuth("codex_oauth", undefined, loginMethod);
}

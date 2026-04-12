import { useManagedAuth } from "./useManagedAuth";

/**
 * Codex OAuth (ChatGPT Plus/Pro) 认证 hook
 *
 * 复用通用 useManagedAuth，仅指定 provider 为 "codex_oauth"
 */
export function useCodexOauth() {
  return useManagedAuth("codex_oauth");
}

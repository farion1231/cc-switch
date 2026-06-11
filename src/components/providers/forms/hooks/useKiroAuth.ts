import { useManagedAuth } from "./useManagedAuth";

/**
 * Kiro (AWS CodeWhisperer/Q) 认证 hook
 *
 * 复用通用 useManagedAuth，指定 provider 为 "kiro"
 */
export function useKiroAuth() {
  return useManagedAuth("kiro");
}

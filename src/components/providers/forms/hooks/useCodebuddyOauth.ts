import { useManagedAuth } from "./useManagedAuth";

/**
 * CodeBuddy OAuth hook（国内站 copilot.tencent.com）。
 *
 * CodeBuddy 的登录流程是「申请 auth_state + 浏览器授权链接 + 轮询换 token」，
 * 没有 device code 的 user_code，`ManagedAuthDeviceCodeResponse.user_code` 为空字符串。
 */
export function useCodebuddyOauth() {
  return useManagedAuth("codebuddy_oauth");
}

import { invoke } from "@tauri-apps/api/core";

export type ManagedAuthProvider =
  | "github_copilot"
  | "codex_oauth"
  | "xai_oauth"
  | "agy_oauth"
  | "claude_official";

export type CodexLoginMethod = "browser" | "device_code";
export type AuthCenterTarget = ManagedAuthProvider | "kimi_oauth";

export const CODEX_OAUTH_DUPLICATE_ACCOUNT_ERROR =
  "codex_oauth_duplicate_account";

export interface ManagedAuthAccount {
  id: string;
  provider: ManagedAuthProvider;
  login: string;
  avatar_url: string | null;
  authenticated_at: number;
  is_default: boolean;
  github_domain: string;
  /** Codex-only: the account lacks identity or workspace metadata required for binding. */
  reauth_required?: boolean;
  /** xAI-only: the refresh credential is invalid and the account is unusable. */
  requires_reauth: boolean;
  /** Claude official only: when the sign-in of an account not in Claude Code's default slot expires (ms, as last recorded). */
  login_expires_at?: number | null;
  /** Claude official only: plan (`pro` / `max` / `team` / `enterprise`) from the account's sign-in. */
  plan?: string | null;
  /** Claude official only: `rateLimitTier` (e.g. `default_claude_max_20x`), tells Max 5x from 20x. */
  rate_limit_tier?: string | null;
}

export interface ManagedAuthStatus {
  provider: ManagedAuthProvider;
  authenticated: boolean;
  default_account_id: string | null;
  migration_error?: string | null;
  accounts: ManagedAuthAccount[];
}

export interface ManagedAuthDeviceCodeResponse {
  provider: ManagedAuthProvider;
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
  /** 后端已经把登录页交给系统浏览器了（Claude 官方账号在 Windows 上由 Claude Code 自己打开），前端不要再开一次 */
  browser_opened?: boolean;
}

export async function authStartLogin(
  authProvider: ManagedAuthProvider,
  githubDomain?: string,
  targetAccountId?: string,
  loginMethod?: CodexLoginMethod,
): Promise<ManagedAuthDeviceCodeResponse> {
  return invoke<ManagedAuthDeviceCodeResponse>("auth_start_login", {
    authProvider,
    githubDomain: githubDomain || null,
    targetAccountId: targetAccountId || null,
    ...(loginMethod ? { loginMethod } : {}),
  });
}

export async function authPollForAccount(
  authProvider: ManagedAuthProvider,
  deviceCode: string,
  githubDomain?: string,
): Promise<ManagedAuthAccount | null> {
  return invoke<ManagedAuthAccount | null>("auth_poll_for_account", {
    authProvider,
    deviceCode,
    githubDomain: githubDomain || null,
  });
}

export async function authCancelLogin(
  authProvider: ManagedAuthProvider,
  deviceCode: string,
): Promise<boolean> {
  return invoke<boolean>("auth_cancel_login", {
    authProvider,
    deviceCode,
  });
}

export async function authListAccounts(
  authProvider: ManagedAuthProvider,
): Promise<ManagedAuthAccount[]> {
  return invoke<ManagedAuthAccount[]>("auth_list_accounts", {
    authProvider,
  });
}

export async function authGetStatus(
  authProvider: ManagedAuthProvider,
): Promise<ManagedAuthStatus> {
  return invoke<ManagedAuthStatus>("auth_get_status", {
    authProvider,
  });
}

export async function authRemoveAccount(
  authProvider: ManagedAuthProvider,
  accountId: string,
): Promise<void> {
  return invoke("auth_remove_account", {
    authProvider,
    accountId,
  });
}

export async function authSetDefaultAccount(
  authProvider: ManagedAuthProvider,
  accountId: string,
): Promise<void> {
  return invoke("auth_set_default_account", {
    authProvider,
    accountId,
  });
}

export async function authLogout(
  authProvider: ManagedAuthProvider,
): Promise<void> {
  return invoke("auth_logout", {
    authProvider,
  });
}

/** 有没有正在运行的 agy：在跑的 agy 还用切换前的账号，续期时可能把旧登录写回去。 */
export async function agyAccountIsRunning(): Promise<boolean> {
  return invoke<boolean>("agy_account_is_running");
}

/** Claude 官方账号登录：浏览器回不到本机时，把落地页地址或页面上的代码交给正在等待的登录。 */
export async function claudeAccountSubmitPaste(
  flowId: string,
  pasted: string,
): Promise<void> {
  return invoke("claude_account_submit_paste", { flowId, pasted });
}

/** 切卡时切换 Claude Code 登录的状态。 */
export interface ClaudeLoginSwitchStatus {
  /** 切到选了账号的官方卡时会切换登录（没被挡住） */
  active: boolean;
  /** 不切换登录的原因：设了 Claude 配置目录覆盖 */
  blocked: "overrideDir" | null;
  /** Claude Code 现在登录的账号 key */
  live: string | null;
}

export async function getClaudeLoginSwitch(): Promise<ClaudeLoginSwitchStatus> {
  return invoke<ClaudeLoginSwitchStatus>("get_claude_login_switch");
}

export const authApi = {
  authStartLogin,
  authPollForAccount,
  authCancelLogin,
  authListAccounts,
  authGetStatus,
  authRemoveAccount,
  authSetDefaultAccount,
  authLogout,
  agyAccountIsRunning,
  claudeAccountSubmitPaste,
  getClaudeLoginSwitch,
};

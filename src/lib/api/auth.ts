import { invoke } from "@tauri-apps/api/core";
import type { CredentialStatus } from "@/types/subscription";

export type ManagedAuthProvider =
  | "github_copilot"
  | "codex_oauth"
  | "xai_oauth"
  | "agy_oauth"
  | "codebuddy_oauth";

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
}

export async function authStartLogin(
  authProvider: ManagedAuthProvider,
  githubDomain?: string,
  targetAccountId?: string,
): Promise<ManagedAuthDeviceCodeResponse> {
  return invoke<ManagedAuthDeviceCodeResponse>("auth_start_login", {
    authProvider,
    githubDomain: githubDomain || null,
    targetAccountId: targetAccountId || null,
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

/** CodeBuddy 凭据文件现在的状态（授权中心顶部的「收录当前账号」提示用）。 */
export interface CodebuddyLiveSummary {
  state: "none" | "logged_out" | "collected" | "uncollected" | "unrecognized";
  account_id: string | null;
  label: string | null;
  domain: string | null;
}

export async function codebuddyAccountLive(): Promise<CodebuddyLiveSummary> {
  return invoke<CodebuddyLiveSummary>("codebuddy_account_live");
}

/** 收录 CodeBuddy 当前登录的账号（已收录就更新副本）。 */
export async function codebuddyAccountImportCurrent(): Promise<ManagedAuthAccount> {
  return invoke<ManagedAuthAccount>("codebuddy_account_import_current");
}

/** 登录等待中再开一个跑 `codebuddy` 的终端（不重置这次登录）。 */
export async function codebuddyAccountOpenLoginTerminal(): Promise<void> {
  return invoke("codebuddy_account_open_login_terminal");
}

/** CodeBuddy 账号的套餐与积分（用该账号副本里的令牌只读查询，不切号、不续期）。 */
export interface CodebuddyQuota {
  credentialStatus: CredentialStatus;
  success: boolean;
  /** 套餐名（后端按语言给的）；企业账号为 null */
  planName: string | null;
  isPaid: boolean;
  enterprise: boolean;
  /** 企业额度不限量 */
  unlimited: boolean;
  /** 本周期积分：总量 / 剩余 / 已用 */
  total: number;
  remaining: number;
  used: number;
  error: string | null;
  queriedAt: number | null;
}

/** 5 分钟内重复查询走后端缓存；`force` 跳过缓存（手动刷新）。 */
export async function codebuddyAccountQuota(
  accountId: string,
  lang: string,
  force = false,
): Promise<CodebuddyQuota> {
  return invoke<CodebuddyQuota>("codebuddy_account_quota", {
    accountId,
    lang,
    force,
  });
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
  codebuddyAccountLive,
  codebuddyAccountImportCurrent,
  codebuddyAccountOpenLoginTerminal,
  codebuddyAccountQuota,
};

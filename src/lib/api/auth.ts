import { invoke } from "@tauri-apps/api/core";

export type ManagedAuthProvider =
  | "github_copilot"
  | "codex_oauth"
  | "xai_oauth"
  | "agy_oauth"
  | "workbuddy_account";

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
  /** WorkBuddy-only: the plaintext account type from the desktop sign-in. */
  account_kind?: string;
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

/** WorkBuddy 桌面端这边的登录情况：在不在运行、有没有登录、认不认得出、收录过没有。 */
export interface WorkBuddyLiveState {
  running: boolean;
  signedIn: boolean;
  recognized: boolean;
  imported: boolean;
}

export interface WorkBuddyImportResult {
  account: ManagedAuthAccount;
  /** 新收录的；false 表示这个号已在列表里，只更新了副本 */
  created: boolean;
}

export async function workbuddyAccountLiveState(): Promise<WorkBuddyLiveState> {
  return invoke<WorkBuddyLiveState>("workbuddy_account_live_state");
}

/** 收录 WorkBuddy 当前登录的账号（已收录的就更新副本）。 */
export async function workbuddyAccountImportCurrent(): Promise<WorkBuddyImportResult> {
  return invoke<WorkBuddyImportResult>("workbuddy_account_import_current");
}

/**
 * 先收录当前账号，再在本机写登出标记，让 WorkBuddy 下次打开停在登录页
 * （不调用服务端登出）。要求 WorkBuddy 已退出。
 */
export async function workbuddyAccountPrepareNewLogin(): Promise<ManagedAuthAccount | null> {
  return invoke<ManagedAuthAccount | null>(
    "workbuddy_account_prepare_new_login",
  );
}

export async function workbuddyAccountOpenApp(): Promise<void> {
  return invoke("workbuddy_account_open_app");
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
  workbuddyAccountLiveState,
  workbuddyAccountImportCurrent,
  workbuddyAccountPrepareNewLogin,
  workbuddyAccountOpenApp,
};

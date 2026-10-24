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
  /** 最近一次「重新登录」的结果（后端会留一会儿，按 id 只提示一次） */
  reauth?: WorkBuddyReauthOutcome | null;
}

/** 「重新登录」的结果：登回了原来的号（替换了它的快照），或登进了别的号（按普通收录处理）。 */
export interface WorkBuddyReauthOutcome {
  /** 这次结果的标识（完成时刻，毫秒） */
  id: number;
  targetId: string;
  targetLogin: string;
  accountId: string;
  login: string;
  /** 登回的就是发起重新登录的那个号 */
  sameAccount: boolean;
  /** 登进了别的号，而且是新收录的 */
  created: boolean;
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
 *
 * 传 `reauthAccountId` 是「重新登录」这个账号：之后登回它时替换它的旧快照，结果经
 * `workbuddyAccountLiveState` 的 `reauth` 告诉界面。WorkBuddy 运行中时照样报错，但目标已记下。
 */
export async function workbuddyAccountPrepareNewLogin(
  reauthAccountId?: string,
): Promise<ManagedAuthAccount | null> {
  return invoke<ManagedAuthAccount | null>(
    "workbuddy_account_prepare_new_login",
    { reauthAccountId: reauthAccountId ?? null },
  );
}

export async function workbuddyAccountOpenApp(): Promise<void> {
  return invoke("workbuddy_account_open_app");
}

/** 一次经 WorkBuddy 查到的套餐与积分（缓存在 CC Switch 的账号表里）。 */
export interface WorkBuddyQuotaSnapshot {
  /** 套餐名（后端给的，如「体验版」）；没有时缺省 */
  planName?: string | null;
  isPaid: boolean;
  /** 本周期积分：总量 / 剩余 / 已用 */
  total: number;
  remaining: number;
  used: number;
  /** 查询时刻（毫秒） */
  queriedAt: number;
}

/** 这次没刷新成的原因：WorkBuddy 没运行 / 不是它当前登着的号 / 它没登录 / 其他失败 */
export type WorkBuddyQuotaUnavailable =
  | "not_running"
  | "not_current"
  | "not_signed_in"
  | "failed";

export interface WorkBuddyQuota {
  /** 最近一次成功的结果（这次刚查的或上次缓存的） */
  snapshot: WorkBuddyQuotaSnapshot | null;
  /** 是 WorkBuddy 当前登着的号 */
  current: boolean;
  /** null 表示 snapshot 是新鲜的 */
  unavailable: WorkBuddyQuotaUnavailable | null;
  error: string | null;
}

/**
 * WorkBuddy 账号的套餐与积分。只有 WorkBuddy 正在运行、且就是它当前登着的号时才经它的本地接口
 * 只读查询（5 分钟内走缓存，`force` 跳过）；其余情况给上次缓存的结果和原因。
 */
export async function workbuddyAccountQuota(
  accountId: string,
  force = false,
): Promise<WorkBuddyQuota> {
  return invoke<WorkBuddyQuota>("workbuddy_account_quota", {
    accountId,
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
  workbuddyAccountLiveState,
  workbuddyAccountImportCurrent,
  workbuddyAccountPrepareNewLogin,
  workbuddyAccountOpenApp,
  workbuddyAccountQuota,
};

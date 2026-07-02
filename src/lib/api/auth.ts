import { invoke } from "@tauri-apps/api/core";

export type ManagedAuthProvider = "github_copilot" | "codex_oauth" | "kiro";

export interface ManagedAuthAccount {
  id: string;
  provider: ManagedAuthProvider;
  login: string;
  avatar_url: string | null;
  authenticated_at: number;
  is_default: boolean;
  github_domain: string;
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
  startUrl?: string,
  region?: string,
): Promise<ManagedAuthDeviceCodeResponse> {
  return invoke<ManagedAuthDeviceCodeResponse>("auth_start_login", {
    authProvider,
    githubDomain: githubDomain || null,
    startUrl: startUrl || null,
    region: region || null,
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

/**
 * Kiro 社交登录（Google / GitHub）。
 * 该调用会打开浏览器并阻塞直到用户完成登录或超时。
 */
export async function authKiroSocialLogin(
  provider?: "google" | "github",
): Promise<ManagedAuthAccount> {
  return invoke<ManagedAuthAccount>("auth_kiro_social_login", {
    provider: provider || null,
  });
}

/**
 * Kiro 主动导入本地 kiro-cli / kiro-ide 凭证（仅在点击时读取）。
 * 返回本次新导入的账号列表。
 */
export async function authKiroImportDynamic(): Promise<ManagedAuthAccount[]> {
  return invoke<ManagedAuthAccount[]>("auth_kiro_import_dynamic", {});
}

/**
 * 使用 KIRO_API_KEY（ksk_ 格式）登录 Kiro。
 */
export async function authKiroApiKeyLogin(
  apiKey: string,
): Promise<ManagedAuthAccount> {
  return invoke<ManagedAuthAccount>("auth_kiro_api_key_login", {
    apiKey,
  });
}

export const authApi = {
  authStartLogin,
  authPollForAccount,
  authListAccounts,
  authGetStatus,
  authRemoveAccount,
  authSetDefaultAccount,
  authLogout,
  authKiroSocialLogin,
  authKiroImportDynamic,
  authKiroApiKeyLogin,
};

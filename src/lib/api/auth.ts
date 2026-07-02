import { invoke } from "@tauri-apps/api/core";
import type { SubscriptionQuota } from "@/types/subscription";

export type ManagedAuthProvider =
  | "github_copilot"
  | "codex_oauth"
  | "claude_official";

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

export interface ClaudeOfficialAccount {
  id: string;
  label: string;
  email: string | null;
  quota: SubscriptionQuota | null;
  createdAt: number;
  updatedAt: number;
  storageKind: string;
}

export async function authStartLogin(
  authProvider: ManagedAuthProvider,
  githubDomain?: string,
): Promise<ManagedAuthDeviceCodeResponse> {
  if (authProvider === "claude_official") {
    throw new Error("Claude Official uses local credential snapshots.");
  }
  return invoke<ManagedAuthDeviceCodeResponse>("auth_start_login", {
    authProvider,
    githubDomain: githubDomain || null,
  });
}

export async function authPollForAccount(
  authProvider: ManagedAuthProvider,
  deviceCode: string,
  githubDomain?: string,
): Promise<ManagedAuthAccount | null> {
  if (authProvider === "claude_official") {
    throw new Error("Claude Official uses local credential snapshots.");
  }
  return invoke<ManagedAuthAccount | null>("auth_poll_for_account", {
    authProvider,
    deviceCode,
    githubDomain: githubDomain || null,
  });
}

export async function authListAccounts(
  authProvider: ManagedAuthProvider,
): Promise<ManagedAuthAccount[]> {
  if (authProvider === "claude_official") {
    throw new Error("Use claudeOfficialListAccounts instead.");
  }
  return invoke<ManagedAuthAccount[]>("auth_list_accounts", {
    authProvider,
  });
}

export async function authGetStatus(
  authProvider: ManagedAuthProvider,
): Promise<ManagedAuthStatus> {
  if (authProvider === "claude_official") {
    const accounts = await claudeOfficialListAccounts();
    return {
      provider: authProvider,
      authenticated: accounts.length > 0,
      default_account_id: accounts[0]?.id ?? null,
      accounts: accounts.map((account, index) => ({
        id: account.id,
        provider: authProvider,
        login: account.email || account.label,
        avatar_url: null,
        authenticated_at: account.updatedAt,
        is_default: index === 0,
        github_domain: "",
      })),
    };
  }
  return invoke<ManagedAuthStatus>("auth_get_status", {
    authProvider,
  });
}

export async function authRemoveAccount(
  authProvider: ManagedAuthProvider,
  accountId: string,
): Promise<void> {
  if (authProvider === "claude_official") {
    return claudeOfficialRemoveAccount(accountId);
  }
  return invoke("auth_remove_account", {
    authProvider,
    accountId,
  });
}

export async function authSetDefaultAccount(
  authProvider: ManagedAuthProvider,
  accountId: string,
): Promise<void> {
  if (authProvider === "claude_official") {
    await claudeOfficialActivateAccount(accountId);
    return;
  }
  return invoke("auth_set_default_account", {
    authProvider,
    accountId,
  });
}

export async function authLogout(
  authProvider: ManagedAuthProvider,
): Promise<void> {
  if (authProvider === "claude_official") {
    throw new Error("Claude Official snapshots can be removed individually.");
  }
  return invoke("auth_logout", {
    authProvider,
  });
}

export async function claudeOfficialListAccounts(): Promise<
  ClaudeOfficialAccount[]
> {
  return invoke<ClaudeOfficialAccount[]>("claude_official_list_accounts");
}

export async function claudeOfficialStartLogin(): Promise<void> {
  return invoke("claude_official_start_login");
}

export async function claudeOfficialCaptureCurrentAccount(
  label?: string,
): Promise<ClaudeOfficialAccount> {
  return invoke<ClaudeOfficialAccount>(
    "claude_official_capture_current_account",
    { label: label || null },
  );
}

export async function claudeOfficialActivateAccount(
  accountId: string,
): Promise<string[]> {
  return invoke<string[]>("claude_official_activate_account", { accountId });
}

export async function claudeOfficialRefreshAccountQuota(
  accountId: string,
): Promise<ClaudeOfficialAccount> {
  return invoke<ClaudeOfficialAccount>("claude_official_refresh_account_quota", {
    accountId,
  });
}

export async function claudeOfficialRemoveAccount(
  accountId: string,
): Promise<void> {
  return invoke("claude_official_remove_account", { accountId });
}

export const authApi = {
  authStartLogin,
  authPollForAccount,
  authListAccounts,
  authGetStatus,
  authRemoveAccount,
  authSetDefaultAccount,
  authLogout,
  claudeOfficialListAccounts,
  claudeOfficialStartLogin,
  claudeOfficialCaptureCurrentAccount,
  claudeOfficialRefreshAccountQuota,
  claudeOfficialActivateAccount,
  claudeOfficialRemoveAccount,
};

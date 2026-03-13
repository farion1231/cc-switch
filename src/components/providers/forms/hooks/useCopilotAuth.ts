import type { GitHubAccount } from "@/lib/api";
import { useManagedAuth } from "./useManagedAuth";

export function useCopilotAuth() {
  const managedAuth = useManagedAuth("github_copilot");
  const defaultAccount =
    managedAuth.accounts.find(
      (account) => account.id === managedAuth.defaultAccountId,
    ) ?? managedAuth.accounts[0];

  return {
    ...managedAuth,
    authStatus: managedAuth.authStatus
      ? {
          authenticated: managedAuth.authStatus.authenticated,
          username: defaultAccount?.login ?? null,
          expires_at: null,
          default_account_id: managedAuth.defaultAccountId,
          accounts: managedAuth.accounts as GitHubAccount[],
        }
      : undefined,
    username: defaultAccount?.login ?? null,
  };
}

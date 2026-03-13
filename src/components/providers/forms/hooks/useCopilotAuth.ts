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
          // Managed auth status does not expose a single provider-wide token expiry.
          expires_at: null,
          default_account_id: managedAuth.defaultAccountId,
          migration_error: managedAuth.migrationError,
          accounts: managedAuth.accounts as GitHubAccount[],
        }
      : undefined,
    // Managed auth status no longer exposes a single default token expiry.
    username: defaultAccount?.login ?? null,
  };
}

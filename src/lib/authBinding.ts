import type { ProviderMeta } from "@/types";

export function resolveManagedAccountId(
  meta: ProviderMeta | undefined,
  authProvider: string,
): string | null {
  const binding = meta?.authBinding;

  if (
    binding?.source === "managed_account" &&
    binding.authProvider === authProvider
  ) {
    const accountId = binding.accountId?.trim();
    return accountId ? accountId : null;
  }

  if (authProvider === "github_copilot") {
    const accountId = meta?.githubAccountId?.trim();
    return accountId ? accountId : null;
  }

  return null;
}

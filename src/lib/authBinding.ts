import type { Provider, ProviderMeta } from "@/types";

export function resolveManagedAccountId(
  meta: ProviderMeta | undefined,
  authProvider: string,
): string | null {
  const binding = meta?.authBinding;

  if (
    binding?.source === "managed_account" &&
    binding.authProvider === authProvider
  ) {
    return binding.accountId ?? null;
  }

  if (authProvider === "github_copilot") {
    return meta?.githubAccountId ?? null;
  }

  return null;
}

/** Native DSH Codex uses its local binding, not the CLI or proxy default account. */
export function resolveDshCodexAccountId(
  appId: string,
  provider: Provider,
): string | null {
  return appId === "dsh" && provider.id === "openai-codex"
    ? resolveManagedAccountId(provider.meta, "codex_oauth")?.trim() || null
    : null;
}

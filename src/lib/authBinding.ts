import { isLinux, isMac, isWindows } from "@/lib/platform";
import type { Provider, ProviderMeta } from "@/types";

/** 能不能切 agy 的全局登录：和后端 `agy_account::keychain::supported` 一致。 */
export function canSwitchAgyAccount(): boolean {
  return isMac() || isWindows() || isLinux();
}

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

export function isDshCodexAccountProvider(
  appId: string,
  providerId: string | undefined,
  meta: ProviderMeta | undefined,
): boolean {
  return (
    appId === "dsh" &&
    providerId === "openai-codex" &&
    meta?.authBinding?.source === "managed_account" &&
    meta.authBinding.authProvider === "codex_oauth"
  );
}

export function isDshCodexOAuthRoute(
  appId: string,
  providerId: string | undefined,
  settings: Record<string, unknown> | undefined,
): boolean {
  if (appId !== "dsh" || providerId !== "openai-codex") return false;
  const options = settings?.options;
  const apiKey =
    options && typeof options === "object" && "apiKey" in options
      ? options.apiKey
      : undefined;
  return ![settings?.apiKeyEnv, apiKey].some(
    (credential) => typeof credential === "string" && credential.trim() !== "",
  );
}

/** Native DSH Codex uses its local binding, not the CLI or proxy default account. */
export function resolveDshCodexAccountId(
  appId: string,
  provider: Provider,
): string | null {
  return isDshCodexAccountProvider(appId, provider.id, provider.meta)
    ? resolveManagedAccountId(provider.meta, "codex_oauth")?.trim() || null
    : null;
}

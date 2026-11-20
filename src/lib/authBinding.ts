import { isLinux, isMac, isWindows } from "@/lib/platform";
import type { ProviderMeta } from "@/types";

/** 能不能切 agy 的全局登录：和后端 `agy_account::keychain::supported` 一致。 */
export function canSwitchAgyAccount(): boolean {
  return isMac() || isWindows() || isLinux();
}

/**
 * 判断路径是否为 WSL UNC 路径（`\\wsl$\<distro>\...` 或 `\\wsl.localhost\<distro>\...`），
 * 与后端 `config::wsl_unc_to_linux` 及 `commands::misc::wsl_distro_from_path` 对齐。
 */
function isWslUncPath(path: string | null | undefined): boolean {
  const trimmed = path?.trim();
  if (!trimmed) return false;
  const normalized = trimmed.replace(/\//g, "\\");
  const stripped = normalized.replace(/^\\\\\?\\UNC\\/i, "\\\\");
  const match = /^\\\\(?:wsl\$|wsl\.localhost)\\([^\\]+)(?:\\|$)/i.exec(
    stripped,
  );
  if (!match) return false;
  const distro = match[1];
  return distro.length <= 64 && /^[A-Za-z0-9._-]+$/.test(distro);
}

/**
 * Antigravity 是否配置为在 WSL 中运行（仅 Windows 生效，与后端
 * `commands::tool_runs_in_wsl("agy")` 一致）。
 */
export function isAgyRunningInWsl(
  agyConfigDir: string | null | undefined,
): boolean {
  return isWindows() && isWslUncPath(agyConfigDir);
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

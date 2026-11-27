import { isLinux, isMac, isWindows } from "@/lib/platform";
import type { AppId, AuthCenterTarget } from "@/lib/api";
import type { ProviderMeta } from "@/types";

/** 官方账号登录在授权中心与供应商页之间的一一映射（单一事实源） */
export const OFFICIAL_AUTH_TARGET_BY_APP: Partial<
  Record<AppId, AuthCenterTarget>
> = {
  claude: "claude_official",
  codex: "codex_oauth",
  grokbuild: "xai_oauth",
  agy: "agy_oauth",
  copilot: "github_copilot",
  workbuddy: "workbuddy_account",
  codebuddy: "codebuddy_oauth",
  kimi: "kimi_oauth",
  dsh: "dsh_account",
};

const APP_ID_BY_AUTH_TARGET: Record<AuthCenterTarget, AppId> = {
  claude_official: "claude",
  codex_oauth: "codex",
  xai_oauth: "grokbuild",
  agy_oauth: "agy",
  github_copilot: "copilot",
  workbuddy_account: "workbuddy",
  codebuddy_oauth: "codebuddy",
  kimi_oauth: "kimi",
  dsh_account: "dsh",
};

export function appIdForAuthTarget(target: AuthCenterTarget): AppId {
  return APP_ID_BY_AUTH_TARGET[target];
}

/**
 * 若某应用选择“官方登录”预设应直接前往授权中心统一配置，返回对应的授权中心分区；否则返回 null。
 */
export function officialAuthTargetForPreset(
  appId: AppId | undefined,
  category: string | undefined,
  claudeBlockedByWsl = false,
): AuthCenterTarget | null {
  if (!appId || category !== "official") {
    return null;
  }
  const target = OFFICIAL_AUTH_TARGET_BY_APP[appId] ?? null;
  if (target === "agy_oauth" && !canSwitchAgyAccount()) {
    return null;
  }
  if (target === "claude_official" && claudeBlockedByWsl) {
    return null;
  }
  return target;
}

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

import type { AppId } from "@/lib/api";
import type { ManagedAuthProvider } from "@/lib/api/auth";
import type { Provider } from "@/types";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { resolveCodexOfficialIdentity } from "@/utils/providerCapabilities";

/** 能用托管账号（授权中心里的账号）的应用；其他应用的供应商不走代理注入的托管凭据。 */
export const MANAGED_ACCOUNT_APPS = [
  "claude",
  "claude-desktop",
  "codex",
] as const;

export type ManagedAccountApp = (typeof MANAGED_ACCOUNT_APPS)[number];

export type ProvidersByApp = Partial<
  Record<ManagedAccountApp, Record<string, Provider> | undefined>
>;

/** 在用某个托管账号的一个供应商 */
export interface ManagedAccountUser {
  appId: ManagedAccountApp;
  providerId: string;
  name: string;
  /** 没指定账号、跟着「默认」账号走的供应商（删掉这个账号后会改用新的默认账号） */
  viaDefault: boolean;
}

// 没写 providerType 的旧卡按 Claude 的请求地址认（对齐后端 Provider::is_github_copilot 等）
const BASE_URL_MARKERS: Partial<Record<ManagedAuthProvider, string>> = {
  github_copilot: "githubcopilot.com",
  codex_oauth: "chatgpt.com/backend-api/codex",
};

function claudeBaseUrl(provider: Provider): string {
  const env = (provider.settingsConfig as { env?: Record<string, unknown> })
    ?.env;
  const value = env?.ANTHROPIC_BASE_URL;
  return typeof value === "string" ? value : "";
}

/**
 * 供应商没指定账号时会不会用这个服务的「默认」账号（后端 forwarder 里
 * `managed_account_id_for(...)` 为空时取默认账号的那几条路径）。
 */
function followsDefaultAccount(
  appId: ManagedAccountApp,
  provider: Provider,
  authProvider: ManagedAuthProvider,
): boolean {
  // Codex 官方卡没绑托管账号时用的是 Codex 自己的登录，不碰授权中心的默认账号
  if (
    appId === "codex" &&
    resolveCodexOfficialIdentity(appId, provider) !== null
  ) {
    return false;
  }
  if (provider.meta?.providerType === authProvider) return true;
  const marker = BASE_URL_MARKERS[authProvider];
  return (
    appId !== "codex" && !!marker && claudeBaseUrl(provider).includes(marker)
  );
}

/**
 * 找出在用这些账号的供应商。`accountIds` 传一个就是「删除这个账号」，传全部就是「删除全部账号」。
 * 指定了账号的供应商按绑定的账号算；没指定的跟着默认账号算。
 */
export function findManagedAccountUsers(
  authProvider: ManagedAuthProvider,
  accountIds: readonly string[],
  defaultAccountId: string | null,
  providersByApp: ProvidersByApp,
): ManagedAccountUser[] {
  const ids = new Set(accountIds);
  const users: ManagedAccountUser[] = [];
  for (const appId of MANAGED_ACCOUNT_APPS) {
    const providers = providersByApp[appId];
    if (!providers) continue;
    for (const provider of Object.values(providers)) {
      const bound = resolveManagedAccountId(
        provider.meta,
        authProvider,
      )?.trim();
      if (bound) {
        if (ids.has(bound)) {
          users.push({
            appId,
            providerId: provider.id,
            name: provider.name,
            viaDefault: false,
          });
        }
        continue;
      }
      if (
        defaultAccountId &&
        ids.has(defaultAccountId) &&
        followsDefaultAccount(appId, provider, authProvider)
      ) {
        users.push({
          appId,
          providerId: provider.id,
          name: provider.name,
          viaDefault: true,
        });
      }
    }
  }
  return users;
}

/** 按应用分组（保持 MANAGED_ACCOUNT_APPS 的顺序），确认框里一行一个应用 */
export function groupUsersByApp(
  users: readonly ManagedAccountUser[],
): { appId: AppId; names: string[] }[] {
  return MANAGED_ACCOUNT_APPS.map((appId) => ({
    appId,
    names: users.filter((user) => user.appId === appId).map((u) => u.name),
  })).filter((group) => group.names.length > 0);
}

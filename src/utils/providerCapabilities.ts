import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { isOAuthProviderType } from "@/config/constants";
import { resolveManagedAccountId } from "@/lib/authBinding";
import {
  extractCodexBaseUrl,
  extractCodexExperimentalBearerToken,
  extractCodexWireApi,
  hasExplicitNonOpenAiCodexModelProvider,
  isCodexAnthropicWireApi,
  isCodexChatWireApi,
} from "@/utils/providerConfigUtils";

export const CODEX_OFFICIAL_PROVIDER_ID = "codex-official";
export const GROKBUILD_OFFICIAL_PROVIDER_ID = "grokbuild-official";

export type CodexOfficialIdentity =
  | "native_login"
  | "managed_account"
  | "api_key";

const nonEmptyString = (value: unknown): boolean =>
  typeof value === "string" && value.trim().length > 0;

function hasExplicitCodexThirdPartyUpstream(
  settings: Record<string, unknown>,
): boolean {
  const config = typeof settings.config === "string" ? settings.config : "";

  return (
    nonEmptyString(settings.baseUrl) ||
    nonEmptyString(settings.baseURL) ||
    nonEmptyString(settings.base_url) ||
    Boolean(extractCodexExperimentalBearerToken(config)) ||
    Boolean(extractCodexBaseUrl(config)) ||
    hasExplicitNonOpenAiCodexModelProvider(config)
  );
}

function hasStoredCodexApiKey(settings: Record<string, unknown>): boolean {
  const auth = settings.auth as Record<string, unknown> | undefined;
  return nonEmptyString(auth?.OPENAI_API_KEY);
}

export function resolveCodexOfficialIdentity(
  appId: AppId,
  provider: Pick<Provider, "id" | "category" | "meta" | "settingsConfig">,
): CodexOfficialIdentity | null {
  if (appId !== "codex") return null;

  const managedAccountId = resolveManagedAccountId(
    provider.meta,
    "codex_oauth",
  )?.trim();
  const hasFixedOfficialId = provider.id === CODEX_OFFICIAL_PROVIDER_ID;
  if (hasFixedOfficialId && provider.category === "official") {
    return managedAccountId ? "managed_account" : "native_login";
  }

  const settings = provider.settingsConfig as Record<string, unknown>;
  const auth = settings?.auth;
  const config = settings?.config;
  if (
    !auth ||
    typeof auth !== "object" ||
    Array.isArray(auth) ||
    (config != null && typeof config !== "string")
  ) {
    return null;
  }

  if (hasExplicitCodexThirdPartyUpstream(settings)) {
    return null;
  }

  if (managedAccountId) {
    return "managed_account";
  }
  if (hasStoredCodexApiKey(settings)) {
    return provider.category === "official" ? "api_key" : null;
  }
  return hasFixedOfficialId || provider.category === "official"
    ? "native_login"
    : null;
}

/** Keep the UI capability rule aligned with the Rust takeover policy. */
export function supportsOfficialProxyTakeover(
  appId: AppId,
  provider: Pick<Provider, "id" | "category" | "meta" | "settingsConfig">,
): boolean {
  const identity = resolveCodexOfficialIdentity(appId, provider);
  if (!identity || identity === "api_key") return false;
  if (
    provider.id === CODEX_OFFICIAL_PROVIDER_ID ||
    identity === "managed_account"
  ) {
    return true;
  }
  return true;
}

/**
 * Cursor 非 official 供应商在开启 Cursor 路由接管后，需公网路由隧道才能
 * 被 Cursor 云端访问（Cursor 云端 SSRF 拦私网）。未开启接管时可直连第三方
 * OpenAI 格式接口，无需公网路由。
 * 当前仅用于 ProviderCard badge（切换警告走 providerNeedsRouting）。
 */
export function providerNeedsPublicRoute(
  appId: AppId,
  provider: Pick<Provider, "category">,
): boolean {
  return appId === "cursor" && provider.category !== "official";
}

/**
 * 供应商在指定应用下是否必须开启路由接管才能正常工作（badge 与切换警告共用的权威谓词）。
 *
 * 权威信号是 `providerType`：托管 OAuth 供应商的凭据由本地代理按请求注入
 * （见 `forwarder.rs`，注入发生在转发路径上，请求必须经过代理 = 接管当前应用），
 * 且后端按 providerType 强制托管认证/格式而**无视 apiFormat**。因此 apiFormat
 * 只是可能被用户改动或旧数据缺省的次要信号，OAuth 供应商一律以 providerType 判定。
 *
 * - Claude Desktop 的普通供应商按 direct/proxy 模式判定；托管 OAuth 没有
 *   direct 逃生口（后端同样拒绝），始终需要本地路由。
 * - claude / codex / grokbuild 的托管 OAuth 同样恒需路由；非 OAuth 则按
 *   各自原生格式及完整 URL 模式判断是否需要本地处理。
 * - Cursor 非 official 供应商可直连第三方 OpenAI 格式接口，不强制路由；
 *   仅在用户开启 Cursor 路由接管时才需要配合公网路由隧道
 *   （见 providerNeedsPublicRoute，用于 badge 提示）。
 */
export function providerNeedsRouting(
  appId: AppId,
  provider: Provider,
): boolean {
  if (
    provider.category === "official" ||
    resolveCodexOfficialIdentity(appId, provider)
  )
    return false;

  const isManagedOAuth = isOAuthProviderType(provider.meta?.providerType);

  // Desktop 普通供应商由表单模式决定；托管 OAuth 的 token 只能由代理注入。
  if (appId === "claude-desktop") {
    return isManagedOAuth || provider.meta?.claudeDesktopMode === "proxy";
  }

  if (appId !== "claude" && appId !== "codex" && appId !== "grokbuild") {
    return false;
  }

  // 托管 OAuth：凭据由代理注入，与 apiFormat 无关，必须接管。
  if (isManagedOAuth) return true;

  if (appId === "claude") {
    const fmt = provider.meta?.apiFormat;
    // Claude 原生是 Anthropic 格式，任何非 anthropic 格式都需要代理转换。
    return provider.meta?.isFullUrl === true || (!!fmt && fmt !== "anthropic");
  }

  if (appId === "codex" || appId === "grokbuild") {
    const fmt = provider.meta?.apiFormat;
    // Codex 原生是 Responses，仅 Chat / Anthropic 需要转换（Responses 直连）。
    if (
      provider.meta?.isFullUrl === true ||
      fmt === "openai_chat" ||
      fmt === "anthropic"
    )
      return true;
    const config = (provider.settingsConfig as Record<string, unknown>)?.config;
    return (
      typeof config === "string" &&
      (isCodexChatWireApi(extractCodexWireApi(config)) ||
        isCodexAnthropicWireApi(extractCodexWireApi(config)))
    );
  }

  return false;
}

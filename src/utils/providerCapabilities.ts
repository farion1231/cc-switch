import { parse as parseToml } from "smol-toml";
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

const configured = (value: unknown): boolean => {
  if (value == null || value === false) return false;
  if (typeof value === "string") return value.trim().length > 0;
  if (Array.isArray(value)) return value.length > 0;
  if (typeof value === "object") return Object.values(value).some(configured);
  return true;
};

const object = (value: unknown): Record<string, unknown> | null =>
  value != null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const connectionFields = [
  "apiKey",
  "api_key",
  "baseUrl",
  "baseURL",
  "base_url",
  "apiBaseUrl",
  "apiKeyHelper",
  "awsAuthRefresh",
  "awsCredentialExport",
  "gcpAuthRefresh",
  "openai_base_url",
  "experimental_bearer_token",
  "env_key",
  "http_headers",
  "env_http_headers",
  "inferenceProvider",
  "inferenceGatewayBaseUrl",
  "inferenceGatewayApiKey",
  "inferenceGatewayAuthScheme",
  "inferenceModels",
];
const hasConnection = (value: Record<string, unknown>) =>
  connectionFields.some((key) => configured(value[key]));

/** Authentication/routing only; keep aligned with Provider::is_unconfigured_official. */
export function isNativeOfficialProvider(
  appId: AppId,
  provider: Provider,
): boolean {
  if (provider.category !== "official") return false;
  const meta = provider.meta;
  const nativeFormat =
    appId === "claude" || appId === "claude-desktop"
      ? "anthropic"
      : "openai_responses";
  if (
    meta &&
    (meta.authBinding?.source === "managed_account" ||
      configured(meta.providerType) ||
      configured(meta.githubAccountId) ||
      meta.isFullUrl === true ||
      (configured(meta.apiFormat) && meta.apiFormat !== nativeFormat) ||
      meta.claudeDesktopMode === "proxy" ||
      Object.keys(meta.claudeDesktopModelRoutes ?? {}).length > 0 ||
      Object.keys(meta.custom_endpoints ?? {}).length > 0)
  )
    return false;
  const settings = object(provider.settingsConfig);
  if (!settings || hasConnection(settings)) return false;
  if (settings.env !== undefined) {
    const env = object(settings.env);
    if (!env) return false;
    if (
      Object.entries(env).some(([key, value]) => {
        let connection = false;
        if (appId === "claude" || appId === "claude-desktop") {
          connection =
            /^(ANTHROPIC_|AWS_|VERTEX_REGION_)/.test(key) ||
            [
              "CLAUDE_CODE_USE_BEDROCK",
              "CLAUDE_CODE_USE_VERTEX",
              "CLAUDE_CODE_USE_FOUNDRY",
              "CLAUDE_CODE_USE_GATEWAY",
              "CLAUDE_CODE_USE_MANTLE",
              "CLAUDE_CODE_USE_ANTHROPIC_AWS",
              "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
              "CLOUD_ML_REGION",
              "GOOGLE_APPLICATION_CREDENTIALS",
              "CLAUDE_CODE_OAUTH_TOKEN",
              "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
              "CLAUDE_CODE_OAUTH_SCOPES",
              "CLAUDE_CODE_API_KEY_HELPER_TTL_MS",
            ].includes(key) ||
            (key.startsWith("CLAUDE_CODE_SKIP_") && key.endsWith("_AUTH"));
          if (
            key === "ANTHROPIC_MODEL" ||
            /^ANTHROPIC_DEFAULT_.*_MODEL$/.test(key)
          )
            connection = false;
        } else if (appId === "gemini") {
          connection =
            key.startsWith("GOOGLE_") ||
            [
              "GEMINI_API_KEY",
              "GEMINI_API_KEY_AUTH_MECHANISM",
              "GEMINI_CLI_CUSTOM_HEADERS",
              "GEMINI_DEFAULT_AUTH_TYPE",
              "GEMINI_CLI_USE_COMPUTE_ADC",
              "CODE_ASSIST_ENDPOINT",
              "CODE_ASSIST_API_VERSION",
            ].includes(key);
        } else {
          connection = [
            "OPENAI_API_KEY",
            "CODEX_API_KEY",
            "XAI_API_KEY",
          ].includes(key);
        }
        return connection && configured(value);
      })
    )
      return false;
  }
  switch (appId) {
    case "claude":
    case "claude-desktop":
      return true;
    case "gemini": {
      const selected = object(
        object(object(settings.config)?.security)?.auth,
      )?.selectedType;
      return !configured(selected) || selected === "oauth-personal";
    }
    case "codex":
    case "grokbuild": {
      if (settings.auth !== undefined) {
        const auth = object(settings.auth);
        if (
          !auth ||
          Object.entries(auth).some(
            ([key, value]) =>
              !["last_refresh", "auth_mode"].includes(key) && configured(value),
          )
        )
          return false;
      }
      if (settings.config != null && typeof settings.config !== "string")
        return false;
      let config: Record<string, unknown>;
      try {
        config = parseToml(
          typeof settings.config === "string" ? settings.config : "",
        );
      } catch {
        return false;
      }
      if (hasConnection(config)) return false;
      if (appId === "grokbuild") return !configured(config.model);
      const providers = object(config.model_providers);
      return (
        (!configured(config.model_provider) ||
          config.model_provider === "openai") &&
        (config.model_providers === undefined ||
          (providers !== null &&
            Object.values(providers).every((provider) => {
              const table = object(provider);
              return table !== null && !hasConnection(table);
            })))
      );
    }
    default:
      return false;
  }
}

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

/**
 * 官方账号卡（对应后端 `codex_direct::is_official`）：Codex 早期绑定托管账号的官方卡
 * 没有 category，按身份认。
 */
export function isOfficialAccount(
  appId: AppId,
  provider: Pick<Provider, "id" | "category" | "meta" | "settingsConfig">,
): boolean {
  return (
    provider.category === "official" ||
    resolveCodexOfficialIdentity(appId, provider) !== null
  );
}

/**
 * 能进故障转移队列（对应后端 `provider_router::provider_supports_failover`）：
 * Codex 官方账号卡靠客户端自己的登录走路由，不能和别家轮换。
 */
export function supportsFailover(
  appId: AppId,
  provider: Pick<Provider, "id" | "category" | "meta" | "settingsConfig">,
): boolean {
  return !supportsOfficialProxyTakeover(appId, provider);
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
 */
export function providerNeedsRouting(
  appId: AppId,
  provider: Provider,
): boolean {
  if (isOfficialAccount(appId, provider)) return false;

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

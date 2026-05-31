// Pure helpers for deciding whether a provider inherently requires the local
// proxy (a.k.a. "routing") to function correctly.
//
// This logic is extracted from `useProviderActions.switchProvider`'s
// `proxyRequiredReason` decision set, with two intentional differences:
//  1. The `!isProxyRunning` precondition is REMOVED — this function only
//     answers whether a provider *inherently* needs routing, independent of
//     whether the proxy happens to be running right now. Callers combine the
//     result with live takeover state to decide what to do.
//  2. The `provider.category !== "official"` guard is kept — official
//     providers never "need routing".
//
// `reason` is a STABLE i18n key string (e.g. "notifications.proxyReasonOpenAIChat")
// rather than a translated message. This keeps the function pure (no `t()`
// dependency) and lets each caller (badge / dialog) translate it.

import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  extractCodexExperimentalBearerToken,
  extractCodexWireApi,
  isCodexChatWireApi,
} from "@/utils/providerConfigUtils";

export interface ProxyRequirement {
  required: boolean;
  reason: string | null;
}

/**
 * Whether a provider is "official" (direct connection to the vendor's own API,
 * no custom base URL / API key). Official providers must never be routed through
 * the local proxy — doing so risks account bans.
 *
 * This is the single source of truth shared by the card badge, the switch guard,
 * and the action button's disable logic, so they can never disagree about which
 * providers count as official (which would let one path bypass confirmDisable).
 * Uses the broad detection (empty/absent base url counts as official).
 */
export function isOfficialProvider(provider: Provider, appId: AppId): boolean {
  if (provider.category === "official") {
    return true;
  }

  const config = provider.settingsConfig as Record<string, any>;
  if (appId === "claude") {
    const baseUrl = config?.env?.ANTHROPIC_BASE_URL;
    return !baseUrl || (typeof baseUrl === "string" && baseUrl.trim() === "");
  }
  if (appId === "codex") {
    // 无 OPENAI_API_KEY → 使用 Codex CLI 内置 OAuth（官方）
    const apiKey = config?.auth?.OPENAI_API_KEY;
    const bearerToken =
      typeof config?.config === "string"
        ? extractCodexExperimentalBearerToken(config.config)
        : undefined;
    return (
      !bearerToken &&
      (!apiKey || (typeof apiKey === "string" && apiKey.trim() === ""))
    );
  }
  if (appId === "gemini") {
    // 无 GEMINI_API_KEY 且无 GOOGLE_GEMINI_BASE_URL → Google OAuth 官方模式
    const apiKey = config?.env?.GEMINI_API_KEY;
    const baseUrl = config?.env?.GOOGLE_GEMINI_BASE_URL;
    return (
      (!apiKey || (typeof apiKey === "string" && apiKey.trim() === "")) &&
      (!baseUrl || (typeof baseUrl === "string" && baseUrl.trim() === ""))
    );
  }
  return false;
}

// Stable i18n keys used as the `reason` payload. Callers translate these.
// NOTE: `reason` is forward-looking API surface — current consumers (the
// ProviderCard badge, the ProviderList guard) read only `.required`; the
// confirm dialogs use fixed messages, so `reason` is not yet shown in the UI.
export const PROXY_REASON_KEYS = {
  copilot: "notifications.proxyReasonCopilot",
  openAIChat: "notifications.proxyReasonOpenAIChat",
  openAIResponses: "notifications.proxyReasonOpenAIResponses",
  geminiNative: "notifications.proxyReasonGeminiNative",
  claudeDesktop: "notifications.proxyReasonClaudeDesktop",
  fullUrl: "notifications.proxyReasonFullUrl",
} as const;

// Whether the Codex provider uses the Chat Completions wire protocol, either
// via the explicit `meta.apiFormat` flag or the `wire_api` field inside the
// TOML config string.
const isCodexChatFormat = (provider: Provider): boolean => {
  if (provider.meta?.apiFormat === "openai_chat") {
    return true;
  }
  const config = (provider.settingsConfig as Record<string, any> | undefined)
    ?.config;
  return (
    typeof config === "string" &&
    isCodexChatWireApi(extractCodexWireApi(config))
  );
};

/**
 * Decide whether a provider inherently requires the local proxy ("routing").
 *
 * @returns `{ required, reason }` where `reason` is a stable i18n key when
 *          `required` is true, or `null` when routing is not required.
 */
export const getProxyRequirement = (
  provider: Provider,
  appId: AppId,
): ProxyRequirement => {
  // Category-official providers never need routing. This is intentionally the
  // NARROW check (only the literal "official" category) — it answers "does this
  // provider's wire protocol need the proxy to transform it", which depends on
  // apiFormat, not on whether credentials happen to be empty. The BROAD
  // account-ban safety ("never route an official-looking provider") is enforced
  // separately by the switch guard via `isOfficialProvider` + `decideSwitchAction`
  // (where `isOfficial` dominates `needsRouting`), so the two never disagree in a
  // way that could route official traffic.
  if (provider.category === "official") {
    return { required: false, reason: null };
  }

  const meta = provider.meta;

  // Copilot-as-Claude. Mirror ProviderCard's broader detection (providerType OR
  // usage_script template) so this stays a superset once the badge unifies onto
  // this function — otherwise a templateType-only Copilot provider would lose
  // the badge and escape the routing guard.
  if (
    appId === "claude" &&
    (meta?.providerType === "github_copilot" ||
      meta?.usage_script?.templateType === "github_copilot")
  ) {
    return { required: true, reason: PROXY_REASON_KEYS.copilot };
  }

  // Claude using any non-anthropic API format requires the local proxy to
  // transform the wire protocol. The closed apiFormat enum is
  // {anthropic, openai_chat, openai_responses, gemini_native}; every
  // non-anthropic value maps to a backend transform (the Rust adapter's
  // claude_api_format_needs_transform enumerates the same three), so we treat
  // "non-anthropic" as the single source of truth instead of enumerating a
  // subset — which previously dropped gemini_native from the badge + guard.
  if (appId === "claude" && meta?.apiFormat && meta.apiFormat !== "anthropic") {
    const reason =
      meta.apiFormat === "openai_chat"
        ? PROXY_REASON_KEYS.openAIChat
        : meta.apiFormat === "openai_responses"
          ? PROXY_REASON_KEYS.openAIResponses
          : PROXY_REASON_KEYS.geminiNative;
    return { required: true, reason };
  }

  // Codex using Chat Completions wire protocol (meta flag or TOML wire_api)
  if (appId === "codex" && isCodexChatFormat(provider)) {
    return { required: true, reason: PROXY_REASON_KEYS.openAIChat };
  }

  // Claude Desktop in local-proxy mode
  if (appId === "claude-desktop" && meta?.claudeDesktopMode === "proxy") {
    return { required: true, reason: PROXY_REASON_KEYS.claudeDesktop };
  }

  // Full URL connection mode (claude / codex)
  if (meta?.isFullUrl && (appId === "claude" || appId === "codex")) {
    return { required: true, reason: PROXY_REASON_KEYS.fullUrl };
  }

  return { required: false, reason: null };
};

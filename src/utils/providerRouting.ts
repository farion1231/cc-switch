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
  extractCodexWireApi,
  isCodexChatWireApi,
} from "@/utils/providerConfigUtils";

export interface ProxyRequirement {
  required: boolean;
  reason: string | null;
}

// Stable i18n keys used as the `reason` payload. Callers translate these.
export const PROXY_REASON_KEYS = {
  copilot: "notifications.proxyReasonCopilot",
  openAIChat: "notifications.proxyReasonOpenAIChat",
  openAIResponses: "notifications.proxyReasonOpenAIResponses",
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
  // Official providers never need routing.
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

  // Claude using OpenAI Chat interface format
  if (appId === "claude" && meta?.apiFormat === "openai_chat") {
    return { required: true, reason: PROXY_REASON_KEYS.openAIChat };
  }

  // Claude using OpenAI Responses interface format
  if (appId === "claude" && meta?.apiFormat === "openai_responses") {
    return { required: true, reason: PROXY_REASON_KEYS.openAIResponses };
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

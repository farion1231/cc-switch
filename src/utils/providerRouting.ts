// Pure helpers for deciding whether a provider inherently requires the local
// proxy ("routing") to function — independent of whether the proxy is currently
// running. Callers combine the result with live takeover state.
//
// `reason` is a STABLE i18n key (not a translated message) so the function stays
// pure (no `t()` dependency); each caller (badge / dialog) translates it.

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
// NOTE: `reason` is forward-looking — current consumers read only `.required`
// (the confirm dialogs use fixed messages), so it is not yet shown in the UI.
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
  // Official detection is the literal `category === "official"` check ONLY — no
  // empty-credentials heuristic. That signal can't distinguish "official direct
  // connection" from "custom provider not filled in yet", so high-cost decisions
  // (ban protection, routing toggles) must not be built on it. The switch guard
  // uses the same narrow check, so badge and guard never disagree.
  if (provider.category === "official") {
    return { required: false, reason: null };
  }

  const meta = provider.meta;

  // Copilot-as-Claude. Mirror ProviderCard's detection (providerType OR
  // usage_script template) so a templateType-only Copilot provider can't escape
  // the routing guard.
  if (
    appId === "claude" &&
    (meta?.providerType === "github_copilot" ||
      meta?.usage_script?.templateType === "github_copilot")
  ) {
    return { required: true, reason: PROXY_REASON_KEYS.copilot };
  }

  // Any non-anthropic Claude apiFormat needs the proxy to transform the wire
  // protocol. Treat "non-anthropic" as the source of truth rather than
  // enumerating a subset — enumerating previously dropped gemini_native.
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

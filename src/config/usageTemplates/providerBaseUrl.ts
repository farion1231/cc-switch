import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

export const getProviderBaseUrl = (
  provider: Provider,
  appId: AppId,
): string | undefined => {
  const config = provider.settingsConfig as Record<string, any> | undefined;
  if (!config) return undefined;

  if (appId === "claude") {
    const env = config.env || {};
    return env.ANTHROPIC_BASE_URL;
  }

  if (appId === "codex") {
    return extractCodexBaseUrl(config.config || "");
  }

  if (appId === "gemini") {
    const env = config.env || {};
    return env.GOOGLE_GEMINI_BASE_URL;
  }

  if (appId === "opencode") {
    const options = config.options || {};
    return options.baseURL;
  }

  if (appId === "openclaw") {
    return config.baseUrl;
  }

  return undefined;
};

import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

/**
 * 从 provider 配置中提取 base URL（用于代理检查/预览）
 */
export function extractProviderBaseUrl(
  provider: Provider,
  appId: AppId,
): string | null {
  try {
    const config = provider.settingsConfig;
    if (!config) return null;

    if (appId === "claude") {
      const envUrl = config?.env?.ANTHROPIC_BASE_URL;
      return typeof envUrl === "string" ? envUrl.trim() : null;
    }

    if (appId === "codex" || appId === "opencode") {
      const tomlConfig = config?.config;
      if (typeof tomlConfig === "string") {
        return extractCodexBaseUrl(tomlConfig) ?? null;
      }

      const baseUrl = config?.base_url;
      return typeof baseUrl === "string" ? baseUrl.trim() : null;
    }

    if (appId === "gemini") {
      const envUrl = config?.env?.GOOGLE_GEMINI_BASE_URL;
      if (typeof envUrl === "string") return envUrl.trim();

      const baseUrl = config?.GEMINI_API_BASE || config?.base_url;
      return typeof baseUrl === "string" ? baseUrl.trim() : null;
    }

    return null;
  } catch {
    return null;
  }
}

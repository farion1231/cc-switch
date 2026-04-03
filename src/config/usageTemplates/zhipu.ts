import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { TEMPLATE_TYPES } from "@/config/constants";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

export interface ZhipuUsageConfig {
  template: typeof TEMPLATE_TYPES.ZHIPU;
  baseUrl: string;
}

export const ZHIPU_CN_USAGE_URL =
  "https://open.bigmodel.cn/api/monitor/usage/quota/limit";
export const ZHIPU_GLOBAL_USAGE_URL =
  "https://api.z.ai/api/monitor/usage/quota/limit";

export const buildZhipuUsageTemplate = (labels: {
  hours5Quota: string;
  weeklyQuota: string;
  mcpMonthly: string;
  mcpMonthlyUnit: string;
  queryFailed: string;
  resetSuffix: string;
}): string => `({
  request: {
    url: "{{baseUrl}}",
    method: "GET",
    headers: {
      "Authorization": "{{apiKey}}",
      "Content-Type": "application/json"
    }
  },
  extractor: (response) => {
    if (response.success && response.data) {
      const limits = Array.isArray(response.data.limits) ? response.data.limits : [];
      const level = response.data.level || "unknown";
      const formatResetTime = (limit) => {
        const resetDate = limit?.nextResetTime ? new Date(limit.nextResetTime) : null;
        if (!resetDate || Number.isNaN(resetDate.getTime())) {
          return undefined;
        }
        return (
          resetDate.getFullYear() +
          "-" +
          String(resetDate.getMonth() + 1).padStart(2, "0") +
          "-" +
          String(resetDate.getDate()).padStart(2, "0") +
          " " +
          String(resetDate.getHours()).padStart(2, "0") +
          ":" +
          String(resetDate.getMinutes()).padStart(2, "0") +
          " " +
          ${JSON.stringify(labels.resetSuffix)}
        );
      };

      const tokenLimits = limits.filter((l) => l.type === "TOKENS_LIMIT");
      const quota5Hours = tokenLimits[0];
      const weeklyQuota = tokenLimits[1];

      const mcp = limits.find((l) => l.type === "TIME_LIMIT");

      const result = [];

      if (quota5Hours) {
        result.push({
          planName: level.toUpperCase() + " · " + ${JSON.stringify(labels.hours5Quota)},
          remaining: 100 - (quota5Hours?.percentage || 0),
          used: quota5Hours?.percentage || 0,
          extra: formatResetTime(quota5Hours),
          unit: "%",
        });
      }

      if (weeklyQuota) {
        result.push({
          planName: level.toUpperCase() + " · " + ${JSON.stringify(labels.weeklyQuota)},
          remaining: 100 - (weeklyQuota?.percentage || 0),
          used: weeklyQuota?.percentage || 0,
          extra: formatResetTime(weeklyQuota),
          unit: "%",
        });
      }

      if (mcp) {
        result.push({
          planName: ${JSON.stringify(labels.mcpMonthly)},
          remaining: mcp.remaining || 0,
          used: mcp.currentValue || 0,
          total: mcp.usage || 1000,
          unit: ${JSON.stringify(labels.mcpMonthlyUnit)},
        });
      }

      return result;
    }
    return [
      {
        isValid: false,
        invalidMessage: response.msg || ${JSON.stringify(labels.queryFailed)}
      }
    ];
  }
})`;

const getProviderBaseUrl = (
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

  return undefined;
};

export const detectZhipuUsageConfig = (
  provider: Provider,
  appId: AppId,
): ZhipuUsageConfig | null => {
  const providerBaseUrl = getProviderBaseUrl(provider, appId);
  const candidates = [provider.websiteUrl, providerBaseUrl]
    .filter((value): value is string => typeof value === "string")
    .map((value) => value.toLowerCase());

  const isGlobal = candidates.some(
    (value) => value.includes("z.ai") || value.includes("api.z.ai"),
  );
  const isChina = candidates.some(
    (value) =>
      value.includes("bigmodel.cn") || value.includes("open.bigmodel.cn"),
  );

  if (!isGlobal && !isChina) {
    return null;
  }

  return {
    template: TEMPLATE_TYPES.ZHIPU,
    baseUrl: isGlobal ? ZHIPU_GLOBAL_USAGE_URL : ZHIPU_CN_USAGE_URL,
  };
};

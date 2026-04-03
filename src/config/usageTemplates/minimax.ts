import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { TEMPLATE_TYPES } from "@/config/constants";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

export interface MiniMaxUsageConfig {
  template: typeof TEMPLATE_TYPES.MINIMAX;
  baseUrl: string;
}

export const MINIMAX_CN_USAGE_URL =
  "https://www.minimaxi.com/v1/api/openplatform/coding_plan/remains";
export const MINIMAX_GLOBAL_USAGE_URL =
  "https://www.minimax.io/v1/api/openplatform/coding_plan/remains";

export const buildMiniMaxUsageTemplate = (labels: {
  hours5Quota: string;
  weeklyQuota: string;
  countUnit: string;
  countdownHourUnit: string;
  countdownMinuteUnit: string;
  resetInTemplate: string;
}): string => `({
  request: {
    url: "{{baseUrl}}",
    method: "GET",
    headers: {
      Authorization: "Bearer {{apiKey}}",
      "User-Agent": "cc-switch/1.0"
    },
  },
  extractor: function (response) {
    if (!response || !response.base_resp || response.base_resp.status_code !== 0) {
      return { isValid: false };
    }

    const models = response.model_remains || [];
    const targetModel = models.find(m => m.model_name && m.model_name.includes("MiniMax-M"));

    if (!targetModel) {
      return { isValid: false };
    }

    const total = Number(targetModel.current_interval_total_count || 0);
    const remaining = Number(targetModel.current_interval_usage_count || 0);
    const used = Math.max(0, total - remaining);
    const remainingMs = Number(targetModel.remains_time || 0);
    const totalMinutes =
      remainingMs > 0 ? Math.max(1, Math.round(remainingMs / 60000)) : 0;
    const resetHours = Math.floor(totalMinutes / 60);
    const resetMinutes = totalMinutes % 60;
    const resetTimeStr =
      totalMinutes > 0
        ? (() => {
            const durationParts = [];
            if (resetHours > 0) {
              durationParts.push(resetHours + " " + ${JSON.stringify(labels.countdownHourUnit)});
            }
            if (resetMinutes > 0) {
              durationParts.push(resetMinutes + " " + ${JSON.stringify(labels.countdownMinuteUnit)});
            }
            const timeLabel = durationParts.join(" ");
            return ${JSON.stringify(labels.resetInTemplate)}.replace("{{time}}", timeLabel);
          })()
        : undefined;

    const weeklyTotal = Number(targetModel.current_weekly_total_count || 0);
    const weeklyRemaining = Number(targetModel.current_weekly_usage_count || 0);
    const result = [
      {
        isValid: true,
        planName: targetModel.model_name || ${JSON.stringify(labels.hours5Quota)},
        remaining: remaining,
        used: used,
        total: total,
        unit: ${JSON.stringify(labels.countUnit)},
        extra: resetTimeStr
      }
    ];

    if (weeklyTotal > 0) {
      const weeklyUsed = Math.max(0, weeklyTotal - weeklyRemaining);
      result.push({
        isValid: true,
        planName: (targetModel.model_name || ${JSON.stringify(labels.hours5Quota)}) + " · " + ${JSON.stringify(labels.weeklyQuota)},
        remaining: weeklyRemaining,
        used: weeklyUsed,
        total: weeklyTotal,
        unit: ${JSON.stringify(labels.countUnit)}
      });
    }

    return result;
  },
});`;

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

export const detectMiniMaxUsageConfig = (
  provider: Provider,
  appId: AppId,
): MiniMaxUsageConfig | null => {
  const providerBaseUrl = getProviderBaseUrl(provider, appId);
  const candidates = [provider.websiteUrl, providerBaseUrl]
    .filter((value): value is string => typeof value === "string")
    .map((value) => value.toLowerCase());

  const isGlobal = candidates.some(
    (value) => value.includes("minimax.io") || value.includes("api.minimax.io"),
  );
  const isChina = candidates.some(
    (value) =>
      value.includes("minimaxi.com") || value.includes("api.minimaxi.com"),
  );

  if (!isGlobal && !isChina) {
    return null;
  }

  return {
    template: TEMPLATE_TYPES.MINIMAX,
    baseUrl: isGlobal ? MINIMAX_GLOBAL_USAGE_URL : MINIMAX_CN_USAGE_URL,
  };
};

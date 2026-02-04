import { useMemo } from "react";
import type { AppId } from "@/lib/api";
import type { ProviderCategory } from "@/types";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";

type PresetEntry = {
  id: string;
  preset: ProviderPreset | CodexProviderPreset | GeminiProviderPreset;
};

interface UseApiKeyLinkProps {
  appId: AppId;
  category?: ProviderCategory;
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  formWebsiteUrl: string;
}

/**
 * 管理 API Key 获取链接的显示和 URL
 */
export function useApiKeyLink({
  appId,
  category,
  selectedPresetId,
  presetEntries,
  formWebsiteUrl,
}: UseApiKeyLinkProps) {
  // 判断是否显示 API Key 获取链接
  const shouldShowApiKeyLink = useMemo(() => {
    return (
      category !== "official" &&
      (category === "cn_official" ||
        category === "aggregator" ||
        category === "third_party")
    );
  }, [category]);

  // 获取当前预设条目
  const currentPresetEntry = useMemo(() => {
    if (selectedPresetId && selectedPresetId !== "custom") {
      return presetEntries.find((item) => item.id === selectedPresetId);
    }
    return undefined;
  }, [selectedPresetId, presetEntries]);

  // 获取当前供应商的网址（用于 API Key 链接）
  const getWebsiteUrl = useMemo(() => {
    if (currentPresetEntry) {
      const preset = currentPresetEntry.preset;
      // 对于 cn_official、aggregator、third_party，优先使用 apiKeyUrl（可能包含推广参数）
      if (
        preset.category === "cn_official" ||
        preset.category === "aggregator" ||
        preset.category === "third_party"
      ) {
        return preset.apiKeyUrl || preset.websiteUrl || "";
      }
      return preset.websiteUrl || "";
    }
    return formWebsiteUrl || "";
  }, [currentPresetEntry, formWebsiteUrl]);

  // 提取合作伙伴信息
  const isPartner = useMemo(() => {
    return currentPresetEntry?.preset.isPartner ?? false;
  }, [currentPresetEntry]);

  const partnerPromotionKey = useMemo(() => {
    return currentPresetEntry?.preset.partnerPromotionKey;
  }, [currentPresetEntry]);

  return {
    shouldShowApiKeyLink:
      appId === "claude" || appId === "codex" || appId === "gemini"
        ? shouldShowApiKeyLink
        : false,
    websiteUrl: getWebsiteUrl,
    isPartner,
    partnerPromotionKey,
  };
}

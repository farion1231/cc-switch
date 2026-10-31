import { useMemo } from "react";
import type { AppId } from "@/lib/api";
import type { ProviderCategory } from "@/types";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";
import type { OpenCodeProviderPreset } from "@/config/opencodeProviderPresets";
import type { ClaudeDesktopProviderPreset } from "@/config/claudeDesktopProviderPresets";
import { extractBaseUrlForUsageDetection } from "@/config/codingPlanProviders";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

type PresetEntry = {
  id: string;
  preset:
    | ProviderPreset
    | CodexProviderPreset
    | GeminiProviderPreset
    | OpenCodeProviderPreset
    | ClaudeDesktopProviderPreset;
};

interface UseApiKeyLinkProps {
  appId: AppId;
  category?: ProviderCategory;
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  formWebsiteUrl: string;
  /** 表单当前的请求地址；没选预设（编辑已有供应商）时按它反查预设 */
  formBaseUrl?: string;
}

const LINK_CATEGORIES: ReadonlyArray<ProviderCategory | undefined> = [
  "cn_official",
  "aggregator",
  "third_party",
];

/** 链接指向：这三类优先用 apiKeyUrl（可能带推广参数），其余用官网 */
function presetLinkUrl(preset: PresetEntry["preset"]): string {
  if (LINK_CATEGORIES.includes(preset.category)) {
    return preset.apiKeyUrl || preset.websiteUrl || "";
  }
  return preset.websiteUrl || "";
}

function normalizeBaseUrl(url: string | null | undefined): string {
  return (url ?? "").trim().toLowerCase().replace(/\/+$/, "");
}

/** 各应用预设的请求地址放在不同位置 */
function presetBaseUrl(appId: AppId, preset: PresetEntry["preset"]): string {
  const p = preset as {
    baseUrl?: unknown;
    baseURL?: unknown;
    config?: unknown;
    settingsConfig?: Record<string, any>;
  };
  if (appId === "claude-desktop") {
    return typeof p.baseUrl === "string" ? p.baseUrl : "";
  }
  if (appId === "codex") {
    return typeof p.config === "string"
      ? (extractCodexBaseUrl(p.config) ?? "")
      : "";
  }
  if (appId === "gemini" && typeof p.baseURL === "string") {
    return p.baseURL;
  }
  return extractBaseUrlForUsageDetection(appId, p.settingsConfig) ?? "";
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
  formBaseUrl,
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

  // 没选预设（编辑已有供应商）：供应商不记来源预设，按请求地址反查。
  // 同一地址的几个预设链接或推广语不一致（如同一家的不同版本）时不猜，退回官网链接。
  const matchedPreset = useMemo(() => {
    if (currentPresetEntry) return undefined;
    const target = normalizeBaseUrl(formBaseUrl);
    if (!target) return undefined;
    const matches = presetEntries
      .map((entry) => entry.preset)
      .filter(
        (preset) => normalizeBaseUrl(presetBaseUrl(appId, preset)) === target,
      );
    const [first] = matches;
    if (!first || !presetLinkUrl(first)) return undefined;
    const consistent = matches.every(
      (preset) =>
        presetLinkUrl(preset) === presetLinkUrl(first) &&
        preset.partnerPromotionKey === first.partnerPromotionKey,
    );
    return consistent ? first : undefined;
  }, [appId, currentPresetEntry, formBaseUrl, presetEntries]);

  const linkPreset = currentPresetEntry?.preset ?? matchedPreset;

  // 获取当前供应商的网址（用于 API Key 链接）
  const getWebsiteUrl = useMemo(() => {
    if (linkPreset) return presetLinkUrl(linkPreset);
    return formWebsiteUrl || "";
  }, [linkPreset, formWebsiteUrl]);

  // 提取合作伙伴信息
  const isPartner = linkPreset?.isPartner ?? false;
  const partnerPromotionKey = linkPreset?.partnerPromotionKey;

  return {
    shouldShowApiKeyLink:
      appId === "claude" ||
      appId === "claude-desktop" ||
      appId === "codex" ||
      appId === "gemini" ||
      appId === "agy" ||
      appId === "opencode" ||
      appId === "openclaw" ||
      appId === "hermes"
        ? shouldShowApiKeyLink
        : false,
    websiteUrl: getWebsiteUrl,
    isPartner,
    partnerPromotionKey,
  };
}

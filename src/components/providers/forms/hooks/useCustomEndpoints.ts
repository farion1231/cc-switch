import { useMemo } from "react";
import type { AppId } from "@/lib/api";
import type { CustomEndpoint } from "@/types";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";

type PresetEntry = {
  id: string;
  preset: ProviderPreset | CodexProviderPreset;
};

interface UseCustomEndpointsProps {
  appId: AppId;
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  draftCustomEndpoints: string[];
  baseUrl: string;
  codexBaseUrl: string;
}

/**
 * 收集和管理自定义端点
 *
 * 收集来源：
 * 1. 用户在测速弹窗中新增的自定义端点
 * 2. 预设中的 endpointCandidates
 * 3. 当前选中的 Base URL
 */
export function useCustomEndpoints({
  appId,
  selectedPresetId,
  presetEntries,
  draftCustomEndpoints,
  baseUrl,
  codexBaseUrl,
}: UseCustomEndpointsProps) {
  const customEndpointsMap = useMemo(() => {
    const urlSet = new Set<string>();

    // 辅助函数：标准化并添加 URL
    const push = (raw?: string) => {
      const url = (raw || "").trim().replace(/\/+$/, "");
      if (url) urlSet.add(url);
    };

    // 1. 自定义端点（来自用户新增）
    for (const u of draftCustomEndpoints) push(u);

    // 2. 预设端点候选
    if (selectedPresetId && selectedPresetId !== "custom") {
      const entry = presetEntries.find((item) => item.id === selectedPresetId);
      if (entry) {
        const preset = entry.preset as any;
        if (Array.isArray(preset?.endpointCandidates)) {
          for (const u of preset.endpointCandidates as string[]) push(u);
        }
      }
    }

    // 3. 当前 Base URL
    if (appId === "codex") {
      push(codexBaseUrl);
    } else {
      push(baseUrl);
    }

    // 构建 CustomEndpoint map
    const urls = Array.from(urlSet.values());
    if (urls.length === 0) {
      return null;
    }

    const now = Date.now();
    const customMap: Record<string, CustomEndpoint> = {};
    for (const url of urls) {
      if (!customMap[url]) {
        customMap[url] = { url, addedAt: now, lastUsed: undefined };
      }
    }

    return customMap;
  }, [
    appId,
    selectedPresetId,
    presetEntries,
    draftCustomEndpoints,
    baseUrl,
    codexBaseUrl,
  ]);

  return customEndpointsMap;
}

import { useMemo } from "react";
import type { AppId } from "@/lib/api";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import type { ProviderMeta, EndpointCandidate } from "@/types";

type PresetEntry = {
  id: string;
  preset: ProviderPreset | CodexProviderPreset;
};

interface UseSpeedTestEndpointsProps {
  appId: AppId;
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  baseUrl: string;
  codexBaseUrl: string;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    meta?: ProviderMeta;
  };
}

/**
 * 收集端点测速弹窗的初始端点列表
 *
 * 收集来源：
 * 1. 当前选中的 Base URL
 * 2. 编辑模式下的初始数据 URL
 * 3. 预设中的 endpointCandidates
 *
 * 注意：已保存的自定义端点通过 getCustomEndpoints API 在 EndpointSpeedTest 组件中加载，
 * 不在此处读取，避免重复导入。
 */
export function useSpeedTestEndpoints({
  appId,
  selectedPresetId,
  presetEntries,
  baseUrl,
  codexBaseUrl,
  initialData,
}: UseSpeedTestEndpointsProps) {
  const claudeEndpoints = useMemo<EndpointCandidate[]>(() => {
    // Reuse this branch for Claude and Gemini (non-Codex)
    if (appId !== "claude" && appId !== "gemini") return [];

    const map = new Map<string, EndpointCandidate>();
    // 候选端点标记为 isCustom: false，表示来自预设或配置
    // 已保存的自定义端点会在 EndpointSpeedTest 组件中通过 API 加载
    const add = (url?: string, isCustom = false) => {
      if (!url) return;
      const sanitized = url.trim().replace(/\/+$/, "");
      if (!sanitized || map.has(sanitized)) return;
      map.set(sanitized, { url: sanitized, isCustom });
    };

    // 1. 当前 Base URL
    if (baseUrl) {
      add(baseUrl);
    }

    // 2. 编辑模式：初始数据中的 URL
    if (initialData && typeof initialData.settingsConfig === "object") {
      const configEnv = initialData.settingsConfig as {
        env?: { ANTHROPIC_BASE_URL?: string; GOOGLE_GEMINI_BASE_URL?: string };
      };
      const envUrls = [
        configEnv.env?.ANTHROPIC_BASE_URL,
        configEnv.env?.GOOGLE_GEMINI_BASE_URL,
      ];
      envUrls.forEach((u) => {
        if (typeof u === "string") add(u);
      });
    }

    // 3. 预设中的 endpointCandidates
    if (selectedPresetId && selectedPresetId !== "custom") {
      const entry = presetEntries.find((item) => item.id === selectedPresetId);
      if (entry) {
        const preset = entry.preset as ProviderPreset & {
          settingsConfig?: { env?: { GOOGLE_GEMINI_BASE_URL?: string } };
          endpointCandidates?: string[];
        };
        // 添加预设自己的 baseUrl（兼容 Claude/Gemini）
        const presetEnv = preset.settingsConfig as {
          env?: {
            ANTHROPIC_BASE_URL?: string;
            GOOGLE_GEMINI_BASE_URL?: string;
          };
        };
        const presetUrls = [
          presetEnv?.env?.ANTHROPIC_BASE_URL,
          presetEnv?.env?.GOOGLE_GEMINI_BASE_URL,
        ];
        presetUrls.forEach((u) => add(u));
        // 添加预设的候选端点
        if (preset.endpointCandidates) {
          preset.endpointCandidates.forEach((url) => add(url));
        }
      }
    }

    return Array.from(map.values());
  }, [appId, baseUrl, initialData, selectedPresetId, presetEntries]);

  const codexEndpoints = useMemo<EndpointCandidate[]>(() => {
    if (appId !== "codex") return [];

    const map = new Map<string, EndpointCandidate>();
    // 候选端点标记为 isCustom: false，表示来自预设或配置
    // 已保存的自定义端点会在 EndpointSpeedTest 组件中通过 API 加载
    const add = (url?: string, isCustom = false) => {
      if (!url) return;
      const sanitized = url.trim().replace(/\/+$/, "");
      if (!sanitized || map.has(sanitized)) return;
      map.set(sanitized, { url: sanitized, isCustom });
    };

    // 1. 当前 Codex Base URL
    if (codexBaseUrl) {
      add(codexBaseUrl);
    }

    // 2. 编辑模式：初始数据中的 URL
    const initialCodexConfig = initialData?.settingsConfig as
      | {
          config?: string;
        }
      | undefined;
    const configStr = initialCodexConfig?.config ?? "";
    // 从 TOML 中提取 base_url
    const match = /base_url\s*=\s*["']([^"']+)["']/i.exec(configStr);
    if (match?.[1]) {
      add(match[1]);
    }

    // 3. 预设中的 endpointCandidates
    if (selectedPresetId && selectedPresetId !== "custom") {
      const entry = presetEntries.find((item) => item.id === selectedPresetId);
      if (entry) {
        const preset = entry.preset as CodexProviderPreset;
        // 添加预设自己的 baseUrl
        const presetConfig = preset.config || "";
        const presetMatch = /base_url\s*=\s*["']([^"']+)["']/i.exec(
          presetConfig,
        );
        if (presetMatch?.[1]) {
          add(presetMatch[1]);
        }
        // 添加预设的候选端点
        if (preset.endpointCandidates) {
          preset.endpointCandidates.forEach((url) => add(url));
        }
      }
    }

    return Array.from(map.values());
  }, [appId, codexBaseUrl, initialData, selectedPresetId, presetEntries]);

  return appId === "codex" ? codexEndpoints : claudeEndpoints;
}

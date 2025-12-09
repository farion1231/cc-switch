import { useState, useCallback } from "react";
import type { ProviderCategory } from "@/types";
import {
  getApiKeyFromConfig,
  setApiKeyInConfig,
  hasApiKeyField,
} from "@/utils/providerConfigUtils";

interface UseApiKeyStateProps {
  initialConfig?: string;
  onConfigChange: (config: string) => void;
  selectedPresetId: string | null;
  category?: ProviderCategory;
  appType?: string;
}

/**
 * 管理 API Key 输入状态
 * 自动同步 API Key 和 JSON 配置
 */
export function useApiKeyState({
  initialConfig,
  onConfigChange,
  selectedPresetId,
  category,
  appType,
}: UseApiKeyStateProps) {
  const [apiKey, setApiKey] = useState(() => {
    if (initialConfig) {
      return getApiKeyFromConfig(initialConfig, appType);
    }
    return "";
  });

  const handleApiKeyChange = useCallback(
    (key: string) => {
      setApiKey(key);

      const configString = setApiKeyInConfig(
        initialConfig || "{}",
        key.trim(),
        {
          // 最佳实践：仅在“新增模式”且“非官方类别”时补齐缺失字段
          // - 新增模式：selectedPresetId !== null
          // - 非官方类别：category !== undefined && category !== "official"
          // - 官方类别：不创建字段（UI 也会禁用输入框）
          // - 未传入 category：不创建字段（避免意外行为）
          createIfMissing:
            selectedPresetId !== null &&
            category !== undefined &&
            category !== "official",
          appType,
        },
      );

      onConfigChange(configString);
    },
    [initialConfig, selectedPresetId, category, appType, onConfigChange],
  );

  const showApiKey = useCallback(
    (config: string, isEditMode: boolean) => {
      return (
        selectedPresetId !== null ||
        (isEditMode && hasApiKeyField(config, appType))
      );
    },
    [selectedPresetId, appType],
  );

  return {
    apiKey,
    setApiKey,
    handleApiKeyChange,
    showApiKey,
  };
}

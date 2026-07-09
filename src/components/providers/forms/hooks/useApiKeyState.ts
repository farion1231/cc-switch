import { useEffect, useState, useCallback } from "react";
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
  apiKeyField?: string;
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
  apiKeyField,
}: UseApiKeyStateProps) {
  const [apiKey, setApiKey] = useState(() => {
    if (initialConfig) {
      return getApiKeyFromConfig(initialConfig, appType);
    }
    return "";
  });

  // 当外部通过 form.reset / 读取 live 等方式更新配置时，同步回 API Key 状态
  // - 仅在 JSON 可解析时同步，避免用户编辑 JSON 过程中因临时无效导致输入框闪烁
  useEffect(() => {
    if (!initialConfig) return;

    try {
      JSON.parse(initialConfig);
    } catch {
      return;
    }

    // 从配置中提取 API Key（如果不存在则返回空字符串）
    const extracted = getApiKeyFromConfig(initialConfig, appType);
    if (extracted !== apiKey) {
      setApiKey(extracted);
    }
  }, [initialConfig, appType, apiKey]);

  const handleApiKeyChange = useCallback(
    (key: string) => {
      setApiKey(key);

      const configString = setApiKeyInConfig(
        initialConfig || "{}",
        key.trim(),
        {
          // 仅在"非官方/非云服务商类别"时补齐缺失字段
          // - official：走 OAuth/订阅登录，不创建字段（UI 也会禁用输入框）
          // - cloud_provider：走顶层 apiKey 或 IAM，不创建 env 字段
          // - undefined（导入/旧版本数据未分类）：视为自定义，允许创建，
          //   否则编辑模式下输入的 API Key 不会写入配置（#5041）
          createIfMissing:
            category !== "official" && category !== "cloud_provider",
          appType,
          apiKeyField,
        },
      );

      onConfigChange(configString);
    },
    [
      initialConfig,
      selectedPresetId,
      category,
      appType,
      apiKeyField,
      onConfigChange,
    ],
  );

  const showApiKey = useCallback(
    (config: string, isEditMode: boolean) => {
      if (selectedPresetId !== null) return true;
      if (!isEditMode) return false;
      // 编辑模式：非官方/非云服务商类别（含 undefined 的导入/旧版本数据）
      // 始终显示输入框，便于补填缺失的 API Key（#5041）；
      // official / cloud_provider 仅在配置已有对应字段时显示
      if (category !== "official" && category !== "cloud_provider") {
        return true;
      }
      return hasApiKeyField(config, appType);
    },
    [selectedPresetId, category, appType],
  );

  return {
    apiKey,
    setApiKey,
    handleApiKeyChange,
    showApiKey,
  };
}

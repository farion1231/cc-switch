import { useState, useCallback, useEffect } from "react";

interface UseHermesConfigStateProps {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  onSettingsConfigChange?: (config: string) => void;
  getSettingsConfig?: () => string;
}

/**
 * 管理 Hermes 配置状态
 * Hermes 配置结构: { name, base_url, api_key, model, transport }
 * 使用蛇形命名以匹配后端 Hermes config.yaml 格式
 */
export function useHermesConfigState({
  initialData,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseHermesConfigStateProps) {
  const [hermesApiKey, setHermesApiKey] = useState("");
  const [hermesBaseUrl, setHermesBaseUrl] = useState("");
  const [hermesModel, setHermesModel] = useState("");
  const [hermesProviderName, setHermesProviderName] = useState("");

  // 构建 settingsConfig JSON（使用蛇形命名）
  const buildSettingsConfig = useCallback(() => {
    const config: Record<string, unknown> = {
      transport: "openai_chat",
    };

    if (hermesProviderName) {
      config.name = hermesProviderName;
    }
    if (hermesBaseUrl) {
      config.base_url = hermesBaseUrl;
    }
    if (hermesApiKey) {
      config.api_key = hermesApiKey;
    }
    if (hermesModel) {
      config.model = hermesModel;
    }

    return config;
  }, [hermesProviderName, hermesBaseUrl, hermesApiKey, hermesModel]);

  // 更新 settingsConfig（用于实时同步）
  const updateHermesConfig = useCallback(
    (updater: (config: Record<string, unknown>) => void) => {
      if (!onSettingsConfigChange || !getSettingsConfig) return;

      try {
        const configStr = getSettingsConfig();
        const config = configStr ? JSON.parse(configStr) : {};
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // 如果解析失败，从当前状态重新构建
        const config = buildSettingsConfig();
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      }
    },
    [onSettingsConfigChange, getSettingsConfig, buildSettingsConfig],
  );

  // 初始化 Hermes 配置（编辑模式）
  useEffect(() => {
    if (!initialData) return;

    const config = initialData.settingsConfig;
    if (typeof config === "object" && config !== null) {
      const cfg = config as Record<string, unknown>;

      // 使用蛇形命名读取
      if (typeof cfg.api_key === "string") {
        setHermesApiKey(cfg.api_key);
      }
      if (typeof cfg.base_url === "string") {
        setHermesBaseUrl(cfg.base_url);
      }
      if (typeof cfg.model === "string") {
        setHermesModel(cfg.model);
      }
      if (typeof cfg.name === "string") {
        setHermesProviderName(cfg.name);
      }
      // 兼容驼峰命名（如果之前使用了驼峰）
      if (typeof cfg.apiKey === "string" && !cfg.api_key) {
        setHermesApiKey(cfg.apiKey as string);
      }
      if (typeof cfg.baseUrl === "string" && !cfg.base_url) {
        setHermesBaseUrl(cfg.baseUrl as string);
      }
      if (typeof cfg.provider_name === "string" && !cfg.name) {
        setHermesProviderName(cfg.provider_name as string);
      }
    }
  }, [initialData]);

  // 处理 Hermes API Key 输入
  const handleHermesApiKeyChange = useCallback(
    (key: string) => {
      const trimmed = key.trim();
      setHermesApiKey(trimmed);
      updateHermesConfig((config) => {
        config.api_key = trimmed;
      });
    },
    [updateHermesConfig],
  );

  // 处理 Hermes Base URL 变化
  const handleHermesBaseUrlChange = useCallback(
    (url: string) => {
      const sanitized = url.trim().replace(/\/+$/, "");
      setHermesBaseUrl(sanitized);
      updateHermesConfig((config) => {
        config.base_url = sanitized;
      });
    },
    [updateHermesConfig],
  );

  // 处理 Hermes Model 变化
  const handleHermesModelChange = useCallback(
    (model: string) => {
      const trimmed = model.trim();
      setHermesModel(trimmed);
      updateHermesConfig((config) => {
        config.model = trimmed;
      });
    },
    [updateHermesConfig],
  );

  // 处理 Hermes Provider Name 变化
  const handleHermesProviderNameChange = useCallback(
    (name: string) => {
      const trimmed = name.trim();
      setHermesProviderName(trimmed);
      updateHermesConfig((config) => {
        config.name = trimmed;
      });
    },
    [updateHermesConfig],
  );

  // 重置配置（用于预设切换）
  const resetHermesConfig = useCallback(
    (settingsConfig: Record<string, unknown>) => {
      const cfg = settingsConfig as Record<string, unknown>;

      // 使用蛇形命名
      if (typeof cfg.api_key === "string") {
        setHermesApiKey(cfg.api_key);
      } else if (typeof cfg.apiKey === "string") {
        setHermesApiKey(cfg.apiKey as string);
      } else {
        setHermesApiKey("");
      }

      if (typeof cfg.base_url === "string") {
        setHermesBaseUrl(cfg.base_url);
      } else if (typeof cfg.baseUrl === "string") {
        setHermesBaseUrl(cfg.baseUrl as string);
      } else {
        setHermesBaseUrl("");
      }

      if (typeof cfg.model === "string") {
        setHermesModel(cfg.model);
      } else {
        setHermesModel("");
      }

      if (typeof cfg.name === "string") {
        setHermesProviderName(cfg.name);
      } else if (typeof cfg.provider_name === "string") {
        setHermesProviderName(cfg.provider_name as string);
      } else {
        setHermesProviderName("");
      }
    },
    [],
  );

  return {
    hermesApiKey,
    hermesBaseUrl,
    hermesModel,
    hermesProviderName,
    setHermesApiKey,
    setHermesBaseUrl,
    setHermesModel,
    setHermesProviderName,
    handleHermesApiKeyChange,
    handleHermesBaseUrlChange,
    handleHermesModelChange,
    handleHermesProviderNameChange,
    resetHermesConfig,
    buildHermesSettingsConfig: buildSettingsConfig,
  };
}

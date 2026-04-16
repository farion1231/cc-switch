import { useState, useCallback, useEffect } from "react";

interface UseHermesConfigStateProps {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
}

/**
 * 管理 Hermes 配置状态
 * Hermes 配置结构: { provider, apiKey, baseUrl, model } 或 { env: {...}, config: {...} }
 */
export function useHermesConfigState({
  initialData,
}: UseHermesConfigStateProps) {
  const [hermesApiKey, setHermesApiKey] = useState("");
  const [hermesBaseUrl, setHermesBaseUrl] = useState("");
  const [hermesModel, setHermesModel] = useState("");
  const [hermesProvider, setHermesProvider] = useState("");

  // 初始化 Hermes 配置（编辑模式）
  useEffect(() => {
    if (!initialData) return;

    const config = initialData.settingsConfig;
    if (typeof config === "object" && config !== null) {
      // 尝试从顶层字段读取
      const cfg = config as Record<string, unknown>;

      // 检查是否是 env/config 结构（类似 Gemini）
      if (cfg.env && typeof cfg.env === "object") {
        const env = cfg.env as Record<string, unknown>;
        if (typeof env.HERMES_API_KEY === "string") {
          setHermesApiKey(env.HERMES_API_KEY);
        } else if (typeof env.ANTHROPIC_API_KEY === "string") {
          setHermesApiKey(env.ANTHROPIC_API_KEY);
        } else if (typeof env.OPENROUTER_API_KEY === "string") {
          setHermesApiKey(env.OPENROUTER_API_KEY);
        }
        if (typeof env.HERMES_BASE_URL === "string") {
          setHermesBaseUrl(env.HERMES_BASE_URL);
        }

        // 从 config 部分读取
        if (cfg.config && typeof cfg.config === "object") {
          const configPart = cfg.config as Record<string, unknown>;
          if (typeof configPart.model === "string") {
            setHermesModel(configPart.model);
          }
          if (typeof configPart.baseUrl === "string") {
            setHermesBaseUrl(configPart.baseUrl);
          }
        }
      } else {
        // 直接从顶层读取
        if (typeof cfg.apiKey === "string") {
          setHermesApiKey(cfg.apiKey);
        }
        if (typeof cfg.baseUrl === "string") {
          setHermesBaseUrl(cfg.baseUrl);
        }
        if (typeof cfg.model === "string") {
          setHermesModel(cfg.model);
        }
        if (typeof cfg.provider === "string") {
          setHermesProvider(cfg.provider);
        }
      }
    }
  }, [initialData]);

  // 处理 Hermes API Key 输入
  const handleHermesApiKeyChange = useCallback((key: string) => {
    setHermesApiKey(key.trim());
  }, []);

  // 处理 Hermes Base URL 变化
  const handleHermesBaseUrlChange = useCallback((url: string) => {
    setHermesBaseUrl(url.trim().replace(/\/+$/, ""));
  }, []);

  // 处理 Hermes Model 变化
  const handleHermesModelChange = useCallback((model: string) => {
    setHermesModel(model.trim());
  }, []);

  // 处理 Hermes Provider 变化
  const handleHermesProviderChange = useCallback((provider: string) => {
    setHermesProvider(provider.trim());
  }, []);

  // 重置配置（用于预设切换）
  const resetHermesConfig = useCallback(
    (settingsConfig: Record<string, unknown>) => {
      const cfg = settingsConfig as Record<string, unknown>;

      if (typeof cfg.apiKey === "string") {
        setHermesApiKey(cfg.apiKey);
      } else {
        setHermesApiKey("");
      }

      if (typeof cfg.baseUrl === "string") {
        setHermesBaseUrl(cfg.baseUrl);
      } else {
        setHermesBaseUrl("");
      }

      if (typeof cfg.model === "string") {
        setHermesModel(cfg.model);
      } else {
        setHermesModel("");
      }

      if (typeof cfg.provider === "string") {
        setHermesProvider(cfg.provider);
      } else {
        setHermesProvider("");
      }
    },
    [],
  );

  // 构建 settingsConfig JSON
  const buildSettingsConfig = useCallback(() => {
    const config: Record<string, unknown> = {};

    if (hermesProvider) {
      config.provider = hermesProvider;
    }
    if (hermesApiKey) {
      config.apiKey = hermesApiKey;
    }
    if (hermesBaseUrl) {
      config.baseUrl = hermesBaseUrl;
    }
    if (hermesModel) {
      config.model = hermesModel;
    }

    return config;
  }, [hermesProvider, hermesApiKey, hermesBaseUrl, hermesModel]);

  return {
    hermesApiKey,
    hermesBaseUrl,
    hermesModel,
    hermesProvider,
    setHermesApiKey,
    setHermesBaseUrl,
    setHermesModel,
    setHermesProvider,
    handleHermesApiKeyChange,
    handleHermesBaseUrlChange,
    handleHermesModelChange,
    handleHermesProviderChange,
    resetHermesConfig,
    buildHermesSettingsConfig: buildSettingsConfig,
  };
}

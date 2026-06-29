import { useState, useCallback } from "react";
import type { OpenCodeModel, OpenCodeProviderConfig } from "@/types";
import {
  OPENCODE_DEFAULT_NPM,
  OPENCODE_DEFAULT_CONFIG,
  isKnownOpencodeOptionKey,
  parseOpencodeConfig,
  toOpencodeExtraOptions,
} from "../helpers/opencodeFormUtils";

interface UseOpencodeFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: string;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export interface OpencodeFormState {
  opencodeProviderKey: string;
  setOpencodeProviderKey: (key: string) => void;
  opencodeNpm: string;
  opencodeApiKey: string;
  opencodeBaseUrl: string;
  opencodeModels: Record<string, OpenCodeModel>;
  opencodeExtraOptions: Record<string, string>;
  handleOpencodeNpmChange: (npm: string) => void;
  handleOpencodeApiKeyChange: (apiKey: string) => void;
  handleOpencodeBaseUrlChange: (baseUrl: string) => void;
  handleOpencodeModelsChange: (models: Record<string, OpenCodeModel>) => void;
  handleOpencodeExtraOptionsChange: (options: Record<string, string>) => void;
  resetOpencodeState: (config?: OpenCodeProviderConfig) => void;
}

export function useOpencodeFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseOpencodeFormStateParams): OpencodeFormState {
  const initialOpencodeConfig =
    appId === "opencode"
      ? parseOpencodeConfig(initialData?.settingsConfig)
      : null;
  const initialOpencodeOptions = initialOpencodeConfig?.options || {};

  const [opencodeProviderKey, setOpencodeProviderKey] = useState<string>(() => {
    if (appId !== "opencode") return "";
    return providerId || "";
  });

  const [opencodeNpm, setOpencodeNpm] = useState<string>(() => {
    if (appId !== "opencode") return OPENCODE_DEFAULT_NPM;
    return initialOpencodeConfig?.npm || OPENCODE_DEFAULT_NPM;
  });

  const [opencodeApiKey, setOpencodeApiKey] = useState<string>(() => {
    if (appId !== "opencode") return "";
    const value = initialOpencodeOptions.apiKey;
    return typeof value === "string" ? value : "";
  });

  const [opencodeBaseUrl, setOpencodeBaseUrl] = useState<string>(() => {
    if (appId !== "opencode") return "";
    const value = initialOpencodeOptions.baseURL;
    return typeof value === "string" ? value : "";
  });

  const [opencodeModels, setOpencodeModels] = useState<
    Record<string, OpenCodeModel>
  >(() => {
    if (appId !== "opencode") return {};
    return initialOpencodeConfig?.models || {};
  });

  const [opencodeExtraOptions, setOpencodeExtraOptions] = useState<
    Record<string, string>
  >(() => {
    if (appId !== "opencode") return {};
    return toOpencodeExtraOptions(initialOpencodeOptions);
  });

  const updateOpencodeSettings = useCallback(
    (updater: (config: Record<string, any>) => void) => {
      try {
        const config = JSON.parse(
          getSettingsConfig() || OPENCODE_DEFAULT_CONFIG,
        ) as Record<string, any>;
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {}
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleOpencodeNpmChange = useCallback(
    (npm: string) => {
      setOpencodeNpm(npm);
      updateOpencodeSettings((config) => {
        config.npm = npm;
      });
    },
    [updateOpencodeSettings],
  );

  const handleOpencodeApiKeyChange = useCallback(
    (apiKey: string) => {
      setOpencodeApiKey(apiKey);
      updateOpencodeSettings((config) => {
        if (!config.options) config.options = {};
        config.options.apiKey = apiKey;
      });
    },
    [updateOpencodeSettings],
  );

  const handleOpencodeBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setOpencodeBaseUrl(baseUrl);
      updateOpencodeSettings((config) => {
        if (!config.options) config.options = {};
        config.options.baseURL = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateOpencodeSettings],
  );

  const handleOpencodeModelsChange = useCallback(
    (models: Record<string, OpenCodeModel>) => {
      setOpencodeModels(models);
      updateOpencodeSettings((config) => {
        config.models = models;
      });
    },
    [updateOpencodeSettings],
  );

  const handleOpencodeExtraOptionsChange = useCallback(
    (options: Record<string, string>) => {
      setOpencodeExtraOptions(options);
      updateOpencodeSettings((config) => {
        if (!config.options) config.options = {};

        for (const k of Object.keys(config.options)) {
          if (!isKnownOpencodeOptionKey(k)) {
            delete config.options[k];
          }
        }

        for (const [k, v] of Object.entries(options)) {
          const trimmedKey = k.trim();
          if (trimmedKey && !trimmedKey.startsWith("option-")) {
            try {
              config.options[trimmedKey] = JSON.parse(v);
            } catch {
              config.options[trimmedKey] = v;
            }
          }
        }
      });
    },
    [updateOpencodeSettings],
  );

  const resetOpencodeState = useCallback((config?: OpenCodeProviderConfig) => {
    setOpencodeProviderKey("");
    setOpencodeNpm(config?.npm || OPENCODE_DEFAULT_NPM);
    setOpencodeBaseUrl(config?.options?.baseURL || "");
    setOpencodeApiKey(config?.options?.apiKey || "");
    setOpencodeModels(config?.models || {});
    setOpencodeExtraOptions(toOpencodeExtraOptions(config?.options || {}));
  }, []);

  return {
    opencodeProviderKey,
    setOpencodeProviderKey,
    opencodeNpm,
    opencodeApiKey,
    opencodeBaseUrl,
    opencodeModels,
    opencodeExtraOptions,
    handleOpencodeNpmChange,
    handleOpencodeApiKeyChange,
    handleOpencodeBaseUrlChange,
    handleOpencodeModelsChange,
    handleOpencodeExtraOptionsChange,
    resetOpencodeState,
  };
}

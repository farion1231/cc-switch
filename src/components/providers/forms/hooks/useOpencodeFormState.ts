import { useState, useCallback } from "react";
import type { OpenCodeModel, OpenCodeProviderConfig } from "@/types";
import {
  OPENCODE_DEFAULT_NPM,
  OPENCODE_DEFAULT_CONFIG,
  OPENCODE_EXTRA_OPTION_DRAFT_PREFIX,
  OPENCODE_HEADER_DRAFT_PREFIX,
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
  opencodeHeaders: Record<string, string>;
  opencodeModels: Record<string, OpenCodeModel>;
  opencodeExtraOptions: Record<string, string>;
  handleOpencodeNpmChange: (npm: string) => void;
  handleOpencodeApiKeyChange: (apiKey: string) => void;
  handleOpencodeBaseUrlChange: (baseUrl: string) => void;
  handleOpencodeHeadersChange: (headers: Record<string, string>) => void;
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

  const [opencodeHeaders, setOpencodeHeaders] = useState<
    Record<string, string>
  >(() => {
    if (appId !== "opencode") return {};
    const headers = initialOpencodeOptions.headers;
    return headers && typeof headers === "object"
      ? (headers as Record<string, string>)
      : {};
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

  const handleOpencodeHeadersChange = useCallback(
    (headers: Record<string, string>) => {
      setOpencodeHeaders(headers);
      updateOpencodeSettings((config) => {
        if (!config.options) config.options = {};

        const nextHeaders: Record<string, string> = {};
        for (const [key, value] of Object.entries(headers)) {
          const trimmedKey = key.trim();
          if (trimmedKey && !key.startsWith(OPENCODE_HEADER_DRAFT_PREFIX)) {
            nextHeaders[trimmedKey] = value;
          }
        }

        if (Object.keys(nextHeaders).length > 0) {
          config.options.headers = nextHeaders;
        } else {
          delete config.options.headers;
        }
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
          if (trimmedKey && !k.startsWith(OPENCODE_EXTRA_OPTION_DRAFT_PREFIX)) {
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
    setOpencodeHeaders(config?.options?.headers || {});
    setOpencodeModels(config?.models || {});
    setOpencodeExtraOptions(toOpencodeExtraOptions(config?.options || {}));
  }, []);

  return {
    opencodeProviderKey,
    setOpencodeProviderKey,
    opencodeNpm,
    opencodeApiKey,
    opencodeBaseUrl,
    opencodeHeaders,
    opencodeModels,
    opencodeExtraOptions,
    handleOpencodeNpmChange,
    handleOpencodeApiKeyChange,
    handleOpencodeBaseUrlChange,
    handleOpencodeHeadersChange,
    handleOpencodeModelsChange,
    handleOpencodeExtraOptionsChange,
    resetOpencodeState,
  };
}

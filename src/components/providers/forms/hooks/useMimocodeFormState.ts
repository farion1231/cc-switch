import { useState, useCallback, useMemo } from "react";
import type { OpenCodeModel, OpenCodeProviderConfig } from "@/types";
import {
  OPENCODE_DEFAULT_NPM,
  OPENCODE_DEFAULT_CONFIG,
  isKnownOpencodeOptionKey,
  parseOpencodeConfig,
  toOpencodeExtraOptions,
} from "../helpers/opencodeFormUtils";
import { useProvidersQuery } from "@/lib/query/queries";

interface UseMimocodeFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: string;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export interface MimocodeFormState {
  mimocodeProviderKey: string;
  setMimocodeProviderKey: (key: string) => void;
  mimocodeNpm: string;
  mimocodeApiKey: string;
  mimocodeBaseUrl: string;
  mimocodeModels: Record<string, OpenCodeModel>;
  mimocodeExtraOptions: Record<string, string>;
  existingMimocodeKeys: string[];
  handleMimocodeNpmChange: (npm: string) => void;
  handleMimocodeApiKeyChange: (apiKey: string) => void;
  handleMimocodeBaseUrlChange: (baseUrl: string) => void;
  handleMimocodeModelsChange: (models: Record<string, OpenCodeModel>) => void;
  handleMimocodeExtraOptionsChange: (options: Record<string, string>) => void;
  resetMimocodeState: (config?: OpenCodeProviderConfig) => void;
}

export const MIMOCODE_DEFAULT_CONFIG = OPENCODE_DEFAULT_CONFIG;

export function useMimocodeFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseMimocodeFormStateParams): MimocodeFormState {
  const { data: mimocodeProvidersData } = useProvidersQuery("mimo");
  const existingMimocodeKeys = useMemo(() => {
    if (!mimocodeProvidersData?.providers) return [];
    return Object.keys(mimocodeProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [mimocodeProvidersData?.providers, providerId]);

  const initialMimocodeConfig =
    appId === "mimo" ? parseOpencodeConfig(initialData?.settingsConfig) : null;
  const initialMimocodeOptions = initialMimocodeConfig?.options || {};

  const [mimocodeProviderKey, setMimocodeProviderKey] = useState<string>(() => {
    if (appId !== "mimo") return "";
    return providerId || "";
  });

  const [mimocodeNpm, setMimocodeNpm] = useState<string>(() => {
    if (appId !== "mimo") return OPENCODE_DEFAULT_NPM;
    return initialMimocodeConfig?.npm || OPENCODE_DEFAULT_NPM;
  });

  const [mimocodeApiKey, setMimocodeApiKey] = useState<string>(() => {
    if (appId !== "mimo") return "";
    const value = initialMimocodeOptions.apiKey;
    return typeof value === "string" ? value : "";
  });

  const [mimocodeBaseUrl, setMimocodeBaseUrl] = useState<string>(() => {
    if (appId !== "mimo") return "";
    const value = initialMimocodeOptions.baseURL;
    return typeof value === "string" ? value : "";
  });

  const [mimocodeModels, setMimocodeModels] = useState<
    Record<string, OpenCodeModel>
  >(() => {
    if (appId !== "mimo") return {};
    return initialMimocodeConfig?.models || {};
  });

  const [mimocodeExtraOptions, setMimocodeExtraOptions] = useState<
    Record<string, string>
  >(() => {
    if (appId !== "mimo") return {};
    return toOpencodeExtraOptions(initialMimocodeOptions);
  });

  const updateMimocodeSettings = useCallback(
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

  const handleMimocodeNpmChange = useCallback(
    (npm: string) => {
      setMimocodeNpm(npm);
      updateMimocodeSettings((config) => {
        config.npm = npm;
      });
    },
    [updateMimocodeSettings],
  );

  const handleMimocodeApiKeyChange = useCallback(
    (apiKey: string) => {
      setMimocodeApiKey(apiKey);
      updateMimocodeSettings((config) => {
        if (!config.options) config.options = {};
        config.options.apiKey = apiKey;
      });
    },
    [updateMimocodeSettings],
  );

  const handleMimocodeBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setMimocodeBaseUrl(baseUrl);
      updateMimocodeSettings((config) => {
        if (!config.options) config.options = {};
        config.options.baseURL = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateMimocodeSettings],
  );

  const handleMimocodeModelsChange = useCallback(
    (models: Record<string, OpenCodeModel>) => {
      setMimocodeModels(models);
      updateMimocodeSettings((config) => {
        config.models = models;
      });
    },
    [updateMimocodeSettings],
  );

  const handleMimocodeExtraOptionsChange = useCallback(
    (options: Record<string, string>) => {
      setMimocodeExtraOptions(options);
      updateMimocodeSettings((config) => {
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
    [updateMimocodeSettings],
  );

  const resetMimocodeState = useCallback((config?: OpenCodeProviderConfig) => {
    setMimocodeProviderKey("");
    setMimocodeNpm(config?.npm || OPENCODE_DEFAULT_NPM);
    setMimocodeBaseUrl(config?.options?.baseURL || "");
    setMimocodeApiKey(config?.options?.apiKey || "");
    setMimocodeModels(config?.models || {});
    setMimocodeExtraOptions(toOpencodeExtraOptions(config?.options || {}));
  }, []);

  return {
    mimocodeProviderKey,
    setMimocodeProviderKey,
    mimocodeNpm,
    mimocodeApiKey,
    mimocodeBaseUrl,
    mimocodeModels,
    mimocodeExtraOptions,
    existingMimocodeKeys,
    handleMimocodeNpmChange,
    handleMimocodeApiKeyChange,
    handleMimocodeBaseUrlChange,
    handleMimocodeModelsChange,
    handleMimocodeExtraOptionsChange,
    resetMimocodeState,
  };
}

import { useState, useCallback, useMemo } from "react";
import type { PiModelEntry, PiProviderConfig } from "@/types";
import type { AppId } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import { PI_DEFAULT_CONFIG } from "@/config/piProviderPresets";

interface UsePiFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export interface PiFormState {
  piProviderKey: string;
  setPiProviderKey: (key: string) => void;
  piBaseUrl: string;
  piApiKey: string;
  piApi: string;
  piModels: PiModelEntry[];
  existingPiKeys: string[];
  handlePiBaseUrlChange: (baseUrl: string) => void;
  handlePiApiKeyChange: (apiKey: string) => void;
  handlePiApiChange: (api: string) => void;
  handlePiModelsChange: (models: PiModelEntry[]) => void;
  resetPiState: (config?: PiProviderConfig, providerKey?: string) => void;
}

function parsePiField<T>(
  initialData: UsePiFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    const config = JSON.parse(
      initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig)
        : PI_DEFAULT_CONFIG,
    );
    return (config[field] as T) || fallback;
  } catch {
    return fallback;
  }
}

export function usePiFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UsePiFormStateParams): PiFormState {
  const { data: piProvidersData } = useProvidersQuery("pi");
  const existingPiKeys = useMemo(() => {
    if (!piProvidersData?.providers) return [];
    return Object.keys(piProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [piProvidersData?.providers, providerId]);

  const [piProviderKey, setPiProviderKey] = useState<string>(() => {
    if (appId !== "pi") return "";
    return providerId || "";
  });

  const [piBaseUrl, setPiBaseUrl] = useState<string>(() => {
    if (appId !== "pi") return "";
    return parsePiField(initialData, "baseUrl", "");
  });

  const [piApiKey, setPiApiKey] = useState<string>(() => {
    if (appId !== "pi") return "";
    return parsePiField(initialData, "apiKey", "");
  });

  const [piApi, setPiApi] = useState<string>(() => {
    if (appId !== "pi") return "openai-completions";
    return parsePiField(initialData, "api", "openai-completions");
  });

  const [piModels, setPiModels] = useState<PiModelEntry[]>(() => {
    if (appId !== "pi") return [];
    return parsePiField<PiModelEntry[]>(initialData, "models", []);
  });

  const updatePiConfig = useCallback(
    (updater: (config: Record<string, any>) => void) => {
      try {
        const config = JSON.parse(getSettingsConfig() || PI_DEFAULT_CONFIG);
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handlePiBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setPiBaseUrl(baseUrl);
      updatePiConfig((config) => {
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updatePiConfig],
  );

  const handlePiApiKeyChange = useCallback(
    (apiKey: string) => {
      setPiApiKey(apiKey);
      updatePiConfig((config) => {
        config.apiKey = apiKey;
      });
    },
    [updatePiConfig],
  );

  const handlePiApiChange = useCallback(
    (api: string) => {
      setPiApi(api);
      updatePiConfig((config) => {
        config.api = api;
      });
    },
    [updatePiConfig],
  );

  const handlePiModelsChange = useCallback(
    (models: PiModelEntry[]) => {
      setPiModels(models);
      updatePiConfig((config) => {
        config.models = models;
      });
    },
    [updatePiConfig],
  );

  const resetPiState = useCallback(
    (config?: PiProviderConfig, providerKey?: string) => {
      setPiProviderKey(providerKey || "");
      setPiBaseUrl(config?.baseUrl || "");
      setPiApiKey(config?.apiKey || "");
      setPiApi(config?.api || "openai-completions");
      setPiModels(config?.models || []);
    },
    [],
  );

  return {
    piProviderKey,
    setPiProviderKey,
    piBaseUrl,
    piApiKey,
    piApi,
    piModels,
    existingPiKeys,
    handlePiBaseUrlChange,
    handlePiApiKeyChange,
    handlePiApiChange,
    handlePiModelsChange,
    resetPiState,
  };
}

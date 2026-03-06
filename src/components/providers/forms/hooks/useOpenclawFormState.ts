import { useState, useCallback, useMemo } from "react";
import type { OpenClawModel } from "@/types";
import type { AppId } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import { OPENCLAW_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";

interface UseOpenclawFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export interface OpenclawFormState {
  openclawProviderKey: string;
  setOpenclawProviderKey: (key: string) => void;
  openclawBaseUrl: string;
  openclawApiKey: string;
  openclawApi: string;
  openclawModels: OpenClawModel[];
  existingOpenclawKeys: string[];
  handleOpenclawBaseUrlChange: (baseUrl: string) => void;
  handleOpenclawApiKeyChange: (apiKey: string) => void;
  handleOpenclawApiChange: (api: string) => void;
  handleOpenclawModelsChange: (models: OpenClawModel[]) => void;
  resetOpenclawState: (config?: {
    baseUrl?: string;
    apiKey?: string;
    api?: string;
    models?: OpenClawModel[];
  }) => void;
}

function parseOpenclawField<T>(
  initialData: UseOpenclawFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    const config = JSON.parse(
      initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig)
        : OPENCLAW_DEFAULT_CONFIG,
    );
    return (config[field] as T) || fallback;
  } catch {
    return fallback;
  }
}

export function useOpenclawFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseOpenclawFormStateParams): OpenclawFormState {
  // Query existing providers for duplicate key checking
  const { data: openclawProvidersData } = useProvidersQuery("openclaw");
  const existingOpenclawKeys = useMemo(() => {
    if (!openclawProvidersData?.providers) return [];
    return Object.keys(openclawProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [openclawProvidersData?.providers, providerId]);

  const [openclawProviderKey, setOpenclawProviderKey] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    return providerId || "";
  });

  const [openclawBaseUrl, setOpenclawBaseUrl] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    return parseOpenclawField(initialData, "baseUrl", "");
  });

  const [openclawApiKey, setOpenclawApiKey] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    return parseOpenclawField(initialData, "apiKey", "");
  });

  const [openclawApi, setOpenclawApi] = useState<string>(() => {
    if (appId !== "openclaw") return "openai-completions";
    return parseOpenclawField(initialData, "api", "openai-completions");
  });

  const [openclawModels, setOpenclawModels] = useState<OpenClawModel[]>(() => {
    if (appId !== "openclaw") return [];
    return parseOpenclawField<OpenClawModel[]>(initialData, "models", []);
  });

  const updateOpenclawConfig = useCallback(
    (updater: (config: Record<string, any>) => void) => {
      try {
        const config = JSON.parse(
          getSettingsConfig() || OPENCLAW_DEFAULT_CONFIG,
        );
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleOpenclawBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setOpenclawBaseUrl(baseUrl);
      updateOpenclawConfig((config) => {
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawApiKeyChange = useCallback(
    (apiKey: string) => {
      setOpenclawApiKey(apiKey);
      updateOpenclawConfig((config) => {
        config.apiKey = apiKey;
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawApiChange = useCallback(
    (api: string) => {
      setOpenclawApi(api);
      updateOpenclawConfig((config) => {
        config.api = api;
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawModelsChange = useCallback(
    (models: OpenClawModel[]) => {
      setOpenclawModels(models);
      updateOpenclawConfig((config) => {
        config.models = models;
      });
    },
    [updateOpenclawConfig],
  );

  const resetOpenclawState = useCallback(
    (config?: {
      baseUrl?: string;
      apiKey?: string;
      api?: string;
      models?: OpenClawModel[];
    }) => {
      setOpenclawProviderKey("");
      setOpenclawBaseUrl(config?.baseUrl || "");
      setOpenclawApiKey(config?.apiKey || "");
      setOpenclawApi(config?.api || "openai-completions");
      setOpenclawModels(config?.models || []);
    },
    [],
  );

  return {
    openclawProviderKey,
    setOpenclawProviderKey,
    openclawBaseUrl,
    openclawApiKey,
    openclawApi,
    openclawModels,
    existingOpenclawKeys,
    handleOpenclawBaseUrlChange,
    handleOpenclawApiKeyChange,
    handleOpenclawApiChange,
    handleOpenclawModelsChange,
    resetOpenclawState,
  };
}

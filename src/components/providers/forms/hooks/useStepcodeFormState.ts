import { useState, useCallback, useMemo } from "react";
import type { StepCodeModel, StepCodeProviderConfig } from "@/types";
import type { AppId } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import { STEPCODE_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";

interface UseStepcodeFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export interface StepcodeFormState {
  stepcodeProviderKey: string;
  setStepcodeProviderKey: (key: string) => void;
  stepcodeBaseUrl: string;
  stepcodeApiKey: string;
  stepcodeApi: string;
  stepcodeModels: StepCodeModel[];
  existingStepcodeKeys: string[];
  handleStepcodeBaseUrlChange: (baseUrl: string) => void;
  handleStepcodeApiKeyChange: (apiKey: string) => void;
  handleStepcodeApiChange: (api: string) => void;
  handleStepcodeModelsChange: (models: StepCodeModel[]) => void;
  resetStepcodeState: (config?: StepCodeProviderConfig) => void;
}

function parseStepcodeField<T>(
  initialData: UseStepcodeFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    const config = JSON.parse(
      initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig)
        : STEPCODE_DEFAULT_CONFIG,
    );
    return (config[field] as T) || fallback;
  } catch {
    return fallback;
  }
}

export function useStepcodeFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseStepcodeFormStateParams): StepcodeFormState {
  // Query existing providers for duplicate key checking
  const { data: stepcodeProvidersData } = useProvidersQuery("stepcode");
  const existingStepcodeKeys = useMemo(() => {
    if (!stepcodeProvidersData?.providers) return [];
    return Object.keys(stepcodeProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [stepcodeProvidersData?.providers, providerId]);

  const [stepcodeProviderKey, setStepcodeProviderKey] = useState<string>(() => {
    if (appId !== "stepcode") return "";
    return providerId || "";
  });

  const [stepcodeBaseUrl, setStepcodeBaseUrl] = useState<string>(() => {
    if (appId !== "stepcode") return "";
    return parseStepcodeField(initialData, "baseUrl", "");
  });

  const [stepcodeApiKey, setStepcodeApiKey] = useState<string>(() => {
    if (appId !== "stepcode") return "";
    return parseStepcodeField(initialData, "apiKey", "");
  });

  const [stepcodeApi, setStepcodeApi] = useState<string>(() => {
    if (appId !== "stepcode") return "openai-responses";
    return parseStepcodeField(initialData, "api", "openai-responses");
  });

  const [stepcodeModels, setStepcodeModels] = useState<StepCodeModel[]>(() => {
    if (appId !== "stepcode") return [];
    return parseStepcodeField<StepCodeModel[]>(initialData, "models", []);
  });

  const updateStepcodeConfig = useCallback(
    (updater: (config: Record<string, any>) => void) => {
      try {
        const config = JSON.parse(
          getSettingsConfig() || STEPCODE_DEFAULT_CONFIG,
        );
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleStepcodeBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setStepcodeBaseUrl(baseUrl);
      updateStepcodeConfig((config) => {
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateStepcodeConfig],
  );

  const handleStepcodeApiKeyChange = useCallback(
    (apiKey: string) => {
      setStepcodeApiKey(apiKey);
      updateStepcodeConfig((config) => {
        config.apiKey = apiKey;
      });
    },
    [updateStepcodeConfig],
  );

  const handleStepcodeApiChange = useCallback(
    (api: string) => {
      setStepcodeApi(api);
      updateStepcodeConfig((config) => {
        config.api = api;
      });
    },
    [updateStepcodeConfig],
  );

  const handleStepcodeModelsChange = useCallback(
    (models: StepCodeModel[]) => {
      setStepcodeModels(models);
      updateStepcodeConfig((config) => {
        config.models = models;
      });
    },
    [updateStepcodeConfig],
  );

  const resetStepcodeState = useCallback((config?: StepCodeProviderConfig) => {
    setStepcodeProviderKey("");
    setStepcodeBaseUrl(config?.baseUrl || "");
    setStepcodeApiKey(config?.apiKey || "");
    setStepcodeApi(config?.api || "openai-responses");
    setStepcodeModels(config?.models || []);
  }, []);

  return {
    stepcodeProviderKey,
    setStepcodeProviderKey,
    stepcodeBaseUrl,
    stepcodeApiKey,
    stepcodeApi,
    stepcodeModels,
    existingStepcodeKeys,
    handleStepcodeBaseUrlChange,
    handleStepcodeApiKeyChange,
    handleStepcodeApiChange,
    handleStepcodeModelsChange,
    resetStepcodeState,
  };
}

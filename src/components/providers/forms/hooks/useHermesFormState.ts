import { useState, useCallback, useMemo } from "react";
import type { AppId } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import {
  HERMES_DEFAULT_API_MODE,
  type HermesApiMode,
  type HermesModel,
  type HermesProviderSettingsConfig,
} from "@/config/hermesProviderPresets";

interface UseHermesFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

const HERMES_DEFAULT_CONFIG_OBJ = {
  name: "",
  base_url: "",
  api_key: "",
} as const;

export const HERMES_DEFAULT_CONFIG = JSON.stringify(
  HERMES_DEFAULT_CONFIG_OBJ,
  null,
  2,
);

export interface HermesFormState {
  hermesProviderKey: string;
  setHermesProviderKey: (key: string) => void;
  hermesBaseUrl: string;
  hermesApiKey: string;
  hermesApiMode: HermesApiMode;
  hermesModels: HermesModel[];
  hermesRateLimitDelay: number | undefined;
  existingHermesKeys: string[];
  handleHermesBaseUrlChange: (baseUrl: string) => void;
  handleHermesApiKeyChange: (apiKey: string) => void;
  handleHermesApiModeChange: (mode: HermesApiMode) => void;
  handleHermesModelsChange: (models: HermesModel[]) => void;
  handleHermesRateLimitDelayChange: (delay: number | undefined) => void;
  resetHermesState: (config?: Partial<HermesProviderSettingsConfig>) => void;
}

function parseHermesField<T>(
  initialData: UseHermesFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    if (initialData?.settingsConfig) {
      return (initialData.settingsConfig[field] as T) || fallback;
    }
    return (
      ((HERMES_DEFAULT_CONFIG_OBJ as Record<string, unknown>)[field] as T) ||
      fallback
    );
  } catch {
    return fallback;
  }
}

function parseRateLimitDelay(raw: unknown): number | undefined {
  return typeof raw === "number" && Number.isFinite(raw) && raw >= 0
    ? raw
    : undefined;
}

export function useHermesFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseHermesFormStateParams): HermesFormState {
  const { data: hermesProvidersData } = useProvidersQuery("hermes");
  const existingHermesKeys = useMemo(() => {
    if (!hermesProvidersData?.providers) return [];
    return Object.keys(hermesProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [hermesProvidersData?.providers, providerId]);

  const [hermesProviderKey, setHermesProviderKey] = useState<string>(() => {
    if (appId !== "hermes") return "";
    return providerId || "";
  });

  const [hermesBaseUrl, setHermesBaseUrl] = useState<string>(() => {
    if (appId !== "hermes") return "";
    return parseHermesField(initialData, "base_url", "");
  });

  const [hermesApiKey, setHermesApiKey] = useState<string>(() => {
    if (appId !== "hermes") return "";
    return parseHermesField(initialData, "api_key", "");
  });

  const [hermesApiMode, setHermesApiMode] = useState<HermesApiMode>(() => {
    if (appId !== "hermes") return HERMES_DEFAULT_API_MODE;
    const stored = parseHermesField<HermesApiMode | "">(
      initialData,
      "api_mode",
      "",
    );
    return stored || HERMES_DEFAULT_API_MODE;
  });

  const [hermesModels, setHermesModels] = useState<HermesModel[]>(() => {
    if (appId !== "hermes") return [];
    return parseHermesField<HermesModel[]>(initialData, "models", []);
  });

  const [hermesRateLimitDelay, setHermesRateLimitDelay] = useState<
    number | undefined
  >(() => {
    if (appId !== "hermes") return undefined;
    return parseRateLimitDelay(initialData?.settingsConfig?.rate_limit_delay);
  });

  const updateHermesConfig = useCallback(
    (updater: (config: Record<string, unknown>) => void) => {
      try {
        const config = JSON.parse(getSettingsConfig() || HERMES_DEFAULT_CONFIG);
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleHermesBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setHermesBaseUrl(baseUrl);
      updateHermesConfig((config) => {
        config.base_url = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateHermesConfig],
  );

  const handleHermesApiKeyChange = useCallback(
    (apiKey: string) => {
      setHermesApiKey(apiKey);
      updateHermesConfig((config) => {
        config.api_key = apiKey;
      });
    },
    [updateHermesConfig],
  );

  const handleHermesApiModeChange = useCallback(
    (mode: HermesApiMode) => {
      setHermesApiMode(mode);
      updateHermesConfig((config) => {
        config.api_mode = mode;
      });
    },
    [updateHermesConfig],
  );

  const handleHermesModelsChange = useCallback(
    (models: HermesModel[]) => {
      setHermesModels(models);
      updateHermesConfig((config) => {
        if (models.length === 0) {
          delete config.models;
        } else {
          config.models = models;
        }
      });
    },
    [updateHermesConfig],
  );

  const handleHermesRateLimitDelayChange = useCallback(
    (delay: number | undefined) => {
      setHermesRateLimitDelay(delay);
      updateHermesConfig((config) => {
        if (delay === undefined) {
          delete config.rate_limit_delay;
        } else {
          config.rate_limit_delay = delay;
        }
      });
    },
    [updateHermesConfig],
  );

  const resetHermesState = useCallback(
    (config?: Partial<HermesProviderSettingsConfig>) => {
      setHermesProviderKey("");
      setHermesBaseUrl(config?.base_url || "");
      setHermesApiKey(config?.api_key || "");
      setHermesApiMode(config?.api_mode ?? HERMES_DEFAULT_API_MODE);
      setHermesModels(config?.models ?? []);
      setHermesRateLimitDelay(parseRateLimitDelay(config?.rate_limit_delay));
    },
    [],
  );

  return {
    hermesProviderKey,
    setHermesProviderKey,
    hermesBaseUrl,
    hermesApiKey,
    hermesApiMode,
    hermesModels,
    hermesRateLimitDelay,
    existingHermesKeys,
    handleHermesBaseUrlChange,
    handleHermesApiKeyChange,
    handleHermesApiModeChange,
    handleHermesModelsChange,
    handleHermesRateLimitDelayChange,
    resetHermesState,
  };
}

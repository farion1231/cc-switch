import { useState, useCallback } from "react";
import type { AppId } from "@/lib/api";
import type {
  WorkBuddyModel,
  WorkBuddyProviderSettingsConfig,
} from "@/config/workbuddyProviderPresets";

interface UseWorkbuddyFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

const WORKBUDDY_DEFAULT_CONFIG_OBJ = {
  baseUrl: "",
  apiKey: "",
} as const;

export const WORKBUDDY_DEFAULT_CONFIG = JSON.stringify(
  WORKBUDDY_DEFAULT_CONFIG_OBJ,
  null,
  2,
);

export interface WorkbuddyFormState {
  workbuddyBaseUrl: string;
  workbuddyApiKey: string;
  workbuddyModels: WorkBuddyModel[];
  handleWorkbuddyBaseUrlChange: (baseUrl: string) => void;
  handleWorkbuddyApiKeyChange: (apiKey: string) => void;
  handleWorkbuddyModelsChange: (models: WorkBuddyModel[]) => void;
  resetWorkbuddyState: (
    config?: Partial<WorkBuddyProviderSettingsConfig>,
  ) => void;
}

function parseWorkbuddyField<T>(
  initialData: UseWorkbuddyFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    if (initialData?.settingsConfig) {
      return (initialData.settingsConfig[field] as T) || fallback;
    }
    return (
      ((WORKBUDDY_DEFAULT_CONFIG_OBJ as Record<string, unknown>)[field] as T) ||
      fallback
    );
  } catch {
    return fallback;
  }
}

export function useWorkbuddyFormState({
  initialData,
  appId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseWorkbuddyFormStateParams): WorkbuddyFormState {
  const [workbuddyBaseUrl, setWorkbuddyBaseUrl] = useState<string>(() => {
    if (appId !== "workbuddy") return "";
    return parseWorkbuddyField(initialData, "baseUrl", "");
  });

  const [workbuddyApiKey, setWorkbuddyApiKey] = useState<string>(() => {
    if (appId !== "workbuddy") return "";
    return parseWorkbuddyField(initialData, "apiKey", "");
  });

  const [workbuddyModels, setWorkbuddyModels] = useState<WorkBuddyModel[]>(
    () => {
      if (appId !== "workbuddy") return [];
      return parseWorkbuddyField<WorkBuddyModel[]>(initialData, "models", []);
    },
  );

  const updateWorkbuddyConfig = useCallback(
    (updater: (config: Record<string, unknown>) => void) => {
      try {
        const config = JSON.parse(
          getSettingsConfig() || WORKBUDDY_DEFAULT_CONFIG,
        );
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // 忽略解析失败，保持表单本地状态可用
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleWorkbuddyBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setWorkbuddyBaseUrl(baseUrl);
      updateWorkbuddyConfig((config) => {
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateWorkbuddyConfig],
  );

  const handleWorkbuddyApiKeyChange = useCallback(
    (apiKey: string) => {
      setWorkbuddyApiKey(apiKey);
      updateWorkbuddyConfig((config) => {
        config.apiKey = apiKey;
      });
    },
    [updateWorkbuddyConfig],
  );

  const handleWorkbuddyModelsChange = useCallback(
    (models: WorkBuddyModel[]) => {
      setWorkbuddyModels(models);
      updateWorkbuddyConfig((config) => {
        if (models.length === 0) {
          delete config.models;
        } else {
          config.models = models;
        }
      });
    },
    [updateWorkbuddyConfig],
  );

  const resetWorkbuddyState = useCallback(
    (config?: Partial<WorkBuddyProviderSettingsConfig>) => {
      setWorkbuddyBaseUrl(config?.baseUrl || "");
      setWorkbuddyApiKey(config?.apiKey || "");
      setWorkbuddyModels(config?.models ?? []);
    },
    [],
  );

  return {
    workbuddyBaseUrl,
    workbuddyApiKey,
    workbuddyModels,
    handleWorkbuddyBaseUrlChange,
    handleWorkbuddyApiKeyChange,
    handleWorkbuddyModelsChange,
    resetWorkbuddyState,
  };
}

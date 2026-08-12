import { useState, useCallback, useMemo } from "react";
import type { AppId } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import {
  KIMI_DEFAULT_API_TYPE,
  type KimiApiType,
  type KimiModel,
  type KimiProviderSettingsConfig,
} from "@/config/kimiProviderPresets";

interface UseKimiFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

const KIMI_DEFAULT_CONFIG_OBJ = {
  name: "",
  type: "openai",
  base_url: "",
  api_key: "",
} as const;

export const KIMI_DEFAULT_CONFIG = JSON.stringify(
  KIMI_DEFAULT_CONFIG_OBJ,
  null,
  2,
);

export interface KimiFormState {
  kimiProviderKey: string;
  setKimiProviderKey: (key: string) => void;
  kimiType: KimiApiType;
  kimiBaseUrl: string;
  kimiApiKey: string;
  kimiModels: KimiModel[];
  kimiDefaultModel: string;
  existingKimiKeys: string[];
  handleKimiTypeChange: (type: KimiApiType) => void;
  handleKimiBaseUrlChange: (baseUrl: string) => void;
  handleKimiApiKeyChange: (apiKey: string) => void;
  handleKimiModelsChange: (models: KimiModel[]) => void;
  handleKimiDefaultModelChange: (alias: string) => void;
  resetKimiState: (config?: Partial<KimiProviderSettingsConfig>) => void;
}

function parseKimiField<T>(
  initialData: UseKimiFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    if (initialData?.settingsConfig) {
      return (initialData.settingsConfig[field] as T) || fallback;
    }
    return (
      ((KIMI_DEFAULT_CONFIG_OBJ as Record<string, unknown>)[field] as T) ||
      fallback
    );
  } catch {
    return fallback;
  }
}

export function useKimiFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseKimiFormStateParams): KimiFormState {
  const { data: kimiProvidersData } = useProvidersQuery("kimi");
  const existingKimiKeys = useMemo(() => {
    if (!kimiProvidersData?.providers) return [];
    return Object.keys(kimiProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [kimiProvidersData?.providers, providerId]);

  const [kimiProviderKey, setKimiProviderKey] = useState<string>(() => {
    if (appId !== "kimi") return "";
    return providerId || "";
  });

  const [kimiType, setKimiType] = useState<KimiApiType>(() => {
    if (appId !== "kimi") return KIMI_DEFAULT_API_TYPE;
    const stored = parseKimiField<KimiApiType | "">(
      initialData,
      "type",
      "",
    );
    return stored || KIMI_DEFAULT_API_TYPE;
  });

  const [kimiBaseUrl, setKimiBaseUrl] = useState<string>(() => {
    if (appId !== "kimi") return "";
    return parseKimiField(initialData, "base_url", "");
  });

  const [kimiApiKey, setKimiApiKey] = useState<string>(() => {
    if (appId !== "kimi") return "";
    return parseKimiField(initialData, "api_key", "");
  });

  const [kimiModels, setKimiModels] = useState<KimiModel[]>(() => {
    if (appId !== "kimi") return [];
    return parseKimiField<KimiModel[]>(initialData, "models", []);
  });

  const [kimiDefaultModel, setKimiDefaultModel] = useState<string>(() => {
    if (appId !== "kimi") return "";
    return parseKimiField(initialData, "default_model", "");
  });

  const updateKimiConfig = useCallback(
    (
      updater: (config: Record<string, unknown>, key: string) => void,
      key = kimiProviderKey,
    ) => {
      try {
        const config = JSON.parse(getSettingsConfig() || KIMI_DEFAULT_CONFIG);
        config.name = key;
        updater(config, key);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [getSettingsConfig, onSettingsConfigChange, kimiProviderKey],
  );

  // 更新 provider key 时同步写入 settingsConfig.name（TOML 的 provider 名）。
  const updateProviderKey = useCallback(
    (key: string) => {
      setKimiProviderKey(key);
      updateKimiConfig((config) => {
        config.name = key;
      }, key);
    },
    [updateKimiConfig],
  );

  const handleKimiTypeChange = useCallback(
    (type: KimiApiType) => {
      setKimiType(type);
      updateKimiConfig((config) => {
        config.type = type;
      });
    },
    [updateKimiConfig],
  );

  const handleKimiBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setKimiBaseUrl(baseUrl);
      updateKimiConfig((config) => {
        config.base_url = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateKimiConfig],
  );

  const handleKimiApiKeyChange = useCallback(
    (apiKey: string) => {
      setKimiApiKey(apiKey);
      updateKimiConfig((config) => {
        config.api_key = apiKey;
      });
    },
    [updateKimiConfig],
  );

  const handleKimiModelsChange = useCallback(
    (models: KimiModel[]) => {
      setKimiModels(models);
      updateKimiConfig((config) => {
        if (models.length === 0) {
          delete config.models;
          return;
        }
        config.models = models;
        // 默认模型不存在时回填为第一个有效（非空）模型 id
        const currentDefault =
          (config.default_model as string | undefined) || "";
        const firstValidId = models.find((m) => m.id.trim() !== "")?.id;
        if (
          !currentDefault ||
          !models.some((m) => m.id === currentDefault)
        ) {
          config.default_model = firstValidId;
          setKimiDefaultModel(firstValidId ?? "");
        }
      });
    },
    [updateKimiConfig],
  );

  const handleKimiDefaultModelChange = useCallback(
    (alias: string) => {
      setKimiDefaultModel(alias);
      updateKimiConfig((config) => {
        if (alias) {
          config.default_model = alias;
        } else {
          delete config.default_model;
        }
      });
    },
    [updateKimiConfig],
  );

  const resetKimiState = useCallback(
    (config?: Partial<KimiProviderSettingsConfig>) => {
      setKimiProviderKey(config?.name || "");
      setKimiType(config?.type ?? KIMI_DEFAULT_API_TYPE);
      setKimiBaseUrl(config?.base_url || "");
      setKimiApiKey(config?.api_key || "");
      setKimiModels(config?.models ?? []);
      setKimiDefaultModel(config?.default_model || "");
    },
    [],
  );

  return {
    kimiProviderKey,
    setKimiProviderKey: updateProviderKey,
    kimiType,
    kimiBaseUrl,
    kimiApiKey,
    kimiModels,
    kimiDefaultModel,
    existingKimiKeys,
    handleKimiTypeChange,
    handleKimiBaseUrlChange,
    handleKimiApiKeyChange,
    handleKimiModelsChange,
    handleKimiDefaultModelChange,
    resetKimiState,
  };
}

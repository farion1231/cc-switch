import { useState, useCallback } from "react";
import type { AppId } from "@/lib/api";
import type { CodexCatalogModel } from "@/types";

interface UseCursorFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

const CURSOR_DEFAULT_CONFIG_OBJ = {
  baseUrl: "",
  apiKey: "",
  model: "",
} as const;

export const CURSOR_DEFAULT_CONFIG = JSON.stringify(
  CURSOR_DEFAULT_CONFIG_OBJ,
  null,
  2,
);

export interface CursorFormState {
  cursorBaseUrl: string;
  cursorApiKey: string;
  cursorModel: string;
  cursorCatalogModels: CodexCatalogModel[];
  handleCursorBaseUrlChange: (baseUrl: string) => void;
  handleCursorApiKeyChange: (apiKey: string) => void;
  handleCursorModelChange: (model: string) => void;
  handleCursorCatalogModelsChange: (models: CodexCatalogModel[]) => void;
  resetCursorState: (config?: Record<string, unknown>) => void;
}

function parseCursorField<T>(
  initialData: UseCursorFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    if (initialData?.settingsConfig) {
      return (initialData.settingsConfig[field] as T) || fallback;
    }
    return (
      ((CURSOR_DEFAULT_CONFIG_OBJ as Record<string, unknown>)[field] as T) ||
      fallback
    );
  } catch {
    return fallback;
  }
}

export function useCursorFormState({
  initialData,
  appId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseCursorFormStateParams): CursorFormState {
  const [cursorBaseUrl, setCursorBaseUrl] = useState<string>(() => {
    if (appId !== "cursor") return "";
    return parseCursorField(initialData, "baseUrl", "");
  });

  const [cursorApiKey, setCursorApiKey] = useState<string>(() => {
    if (appId !== "cursor") return "";
    return parseCursorField(initialData, "apiKey", "");
  });

  const [cursorModel, setCursorModel] = useState<string>(() => {
    if (appId !== "cursor") return "";
    return parseCursorField(initialData, "model", "");
  });

  const [cursorCatalogModels, setCursorCatalogModels] = useState<
    CodexCatalogModel[]
  >(() => {
    if (appId !== "cursor") return [];
    try {
      const config = initialData?.settingsConfig as Record<string, unknown>;
      const modelCatalog = config?.modelCatalog as
        | { models?: CodexCatalogModel[] }
        | undefined;
      return Array.isArray(modelCatalog?.models) ? modelCatalog.models : [];
    } catch {
      return [];
    }
  });

  const updateConfig = useCallback(
    (updater: (config: Record<string, unknown>) => void) => {
      try {
        const config = JSON.parse(getSettingsConfig() || CURSOR_DEFAULT_CONFIG);
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleCursorBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setCursorBaseUrl(baseUrl);
      updateConfig((config) => {
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateConfig],
  );

  const handleCursorApiKeyChange = useCallback(
    (apiKey: string) => {
      setCursorApiKey(apiKey);
      updateConfig((config) => {
        config.apiKey = apiKey;
      });
    },
    [updateConfig],
  );

  const handleCursorModelChange = useCallback(
    (model: string) => {
      setCursorModel(model);
      updateConfig((config) => {
        if (model.trim()) {
          config.model = model.trim();
        } else {
          delete config.model;
        }
      });
    },
    [updateConfig],
  );

  const handleCursorCatalogModelsChange = useCallback(
    (models: CodexCatalogModel[]) => {
      setCursorCatalogModels(models);
      updateConfig((config) => {
        if (models.length > 0) {
          config.modelCatalog = { models };
        } else {
          delete config.modelCatalog;
        }
      });
    },
    [updateConfig],
  );

  const resetCursorState = useCallback((config?: Record<string, unknown>) => {
    setCursorBaseUrl(config?.baseUrl ? String(config.baseUrl) : "");
    setCursorApiKey(config?.apiKey ? String(config.apiKey) : "");
    setCursorModel(config?.model ? String(config.model) : "");
    const modelCatalog = config?.modelCatalog as
      | { models?: CodexCatalogModel[] }
      | undefined;
    setCursorCatalogModels(
      Array.isArray(modelCatalog?.models) ? modelCatalog.models : [],
    );
  }, []);

  return {
    cursorBaseUrl,
    cursorApiKey,
    cursorModel,
    cursorCatalogModels,
    handleCursorBaseUrlChange,
    handleCursorApiKeyChange,
    handleCursorModelChange,
    handleCursorCatalogModelsChange,
    resetCursorState,
  };
}

import { useCallback, useState } from "react";
import type { AppId } from "@/lib/api";
import type { OpenClawModel } from "@/types";
import { PI_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";

interface UsePiFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

interface PiProviderConfig {
  baseUrl?: string;
  baseURL?: string;
  apiKey?: string;
  api?: string;
  models?: OpenClawModel[];
  defaultModel?: string;
}

export interface PiFormState {
  piBaseUrl: string;
  piApiKey: string;
  piApi: string;
  piModels: OpenClawModel[];
  piDefaultModel: string;
  handlePiBaseUrlChange: (baseUrl: string) => void;
  handlePiApiKeyChange: (apiKey: string) => void;
  handlePiApiChange: (api: string) => void;
  handlePiModelsChange: (models: OpenClawModel[]) => void;
  handlePiDefaultModelChange: (model: string) => void;
}

function parsePiConfig(
  initialData: UsePiFormStateParams["initialData"],
): PiProviderConfig {
  try {
    return JSON.parse(
      initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig)
        : PI_DEFAULT_CONFIG,
    ) as PiProviderConfig;
  } catch {
    return JSON.parse(PI_DEFAULT_CONFIG) as PiProviderConfig;
  }
}

export function usePiFormState({
  initialData,
  appId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UsePiFormStateParams): PiFormState {
  const initial = parsePiConfig(initialData);

  const [piBaseUrl, setPiBaseUrl] = useState(() => {
    if (appId !== "pi") return "";
    return initial.baseUrl || initial.baseURL || "";
  });
  const [piApiKey, setPiApiKey] = useState(() => {
    if (appId !== "pi") return "";
    return initial.apiKey || "";
  });
  const [piApi, setPiApi] = useState(() => {
    if (appId !== "pi") return "openai-chat";
    return initial.api || "openai-chat";
  });
  const [piModels, setPiModels] = useState<OpenClawModel[]>(() => {
    if (appId !== "pi") return [];
    return initial.models || [];
  });
  const [piDefaultModel, setPiDefaultModel] = useState(() => {
    if (appId !== "pi") return "";
    return initial.defaultModel || initial.models?.[0]?.id || "";
  });

  const updatePiConfig = useCallback(
    (updater: (config: Record<string, unknown>) => void) => {
      try {
        const parsed = JSON.parse(getSettingsConfig() || PI_DEFAULT_CONFIG);
        if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
          return;
        }
        const config = parsed as Record<string, unknown>;
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // Leave invalid JSON editor content untouched while the user is editing.
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handlePiBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setPiBaseUrl(baseUrl);
      updatePiConfig((config) => {
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
        delete config.baseURL;
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
    (models: OpenClawModel[]) => {
      setPiModels(models);
      const selectableModelIds = models
        .map((model) => model.id.trim())
        .filter(Boolean);
      const nextDefaultModel = selectableModelIds.includes(piDefaultModel)
        ? piDefaultModel
        : selectableModelIds[0] || "";
      setPiDefaultModel(nextDefaultModel);
      updatePiConfig((config) => {
        config.models = models;
        config.defaultModel = nextDefaultModel;
      });
    },
    [piDefaultModel, updatePiConfig],
  );

  const handlePiDefaultModelChange = useCallback(
    (model: string) => {
      setPiDefaultModel(model);
      updatePiConfig((config) => {
        config.defaultModel = model;
      });
    },
    [updatePiConfig],
  );

  return {
    piBaseUrl,
    piApiKey,
    piApi,
    piModels,
    piDefaultModel,
    handlePiBaseUrlChange,
    handlePiApiKeyChange,
    handlePiApiChange,
    handlePiModelsChange,
    handlePiDefaultModelChange,
  };
}

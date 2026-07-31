import { useCallback, useEffect, useState } from "react";
import type { AppId } from "@/lib/api";
import type { OpenClawModel } from "@/types";
import { PI_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";

interface UsePiFormStateParams {
  appId: AppId;
  settingsConfig: string;
  onSettingsConfigChange: (config: string) => void;
}

interface PiProviderConfig {
  baseUrl: string;
  apiKey: string;
  api: string;
  models: OpenClawModel[];
  defaultModel: string;
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

function parsePiConfig(rawConfig: string): PiProviderConfig | null {
  try {
    const parsed = JSON.parse(rawConfig || PI_DEFAULT_CONFIG);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return null;
    }
    const config = parsed as Record<string, unknown>;
    for (const field of [
      "baseUrl",
      "baseURL",
      "apiKey",
      "api",
      "defaultModel",
    ]) {
      if (config[field] !== undefined && typeof config[field] !== "string") {
        return null;
      }
    }

    const models: OpenClawModel[] = [];
    if (config.models !== undefined) {
      if (!Array.isArray(config.models)) return null;
      for (const entry of config.models) {
        if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
          return null;
        }
        const model = entry as Record<string, unknown>;
        if (
          typeof model.id !== "string" ||
          (model.name !== undefined && typeof model.name !== "string")
        ) {
          return null;
        }
        models.push({
          ...model,
          id: model.id,
          name: typeof model.name === "string" ? model.name : model.id,
        } as OpenClawModel);
      }
    }

    const baseUrl =
      typeof config.baseUrl === "string"
        ? config.baseUrl
        : typeof config.baseURL === "string"
          ? config.baseURL
          : "";
    const defaultModel =
      typeof config.defaultModel === "string"
        ? config.defaultModel
        : models[0]?.id || "";

    return {
      baseUrl,
      apiKey: typeof config.apiKey === "string" ? config.apiKey : "",
      api: typeof config.api === "string" ? config.api : "openai-completions",
      models,
      defaultModel,
    };
  } catch {
    return null;
  }
}

export function usePiFormState({
  appId,
  settingsConfig,
  onSettingsConfigChange,
}: UsePiFormStateParams): PiFormState {
  const fallback = parsePiConfig(PI_DEFAULT_CONFIG) as PiProviderConfig;
  const initial =
    appId === "pi" ? parsePiConfig(settingsConfig) || fallback : fallback;

  const [piBaseUrl, setPiBaseUrl] = useState(() => {
    if (appId !== "pi") return "";
    return initial.baseUrl;
  });
  const [piApiKey, setPiApiKey] = useState(() => {
    if (appId !== "pi") return "";
    return initial.apiKey;
  });
  const [piApi, setPiApi] = useState(() => {
    if (appId !== "pi") return "openai-completions";
    return initial.api;
  });
  const [piModels, setPiModels] = useState<OpenClawModel[]>(() => {
    if (appId !== "pi") return [];
    return initial.models;
  });
  const [piDefaultModel, setPiDefaultModel] = useState(() => {
    if (appId !== "pi") return "";
    return initial.defaultModel || initial.models[0]?.id || "";
  });

  useEffect(() => {
    if (appId !== "pi") return;
    const config = parsePiConfig(settingsConfig);
    if (!config) return;

    setPiBaseUrl(config.baseUrl);
    setPiApiKey(config.apiKey);
    setPiApi(config.api);
    setPiModels(config.models);
    setPiDefaultModel(config.defaultModel || config.models[0]?.id || "");
  }, [appId, settingsConfig]);

  const updatePiConfig = useCallback(
    (updater: (config: Record<string, unknown>) => void) => {
      try {
        const parsed = JSON.parse(settingsConfig || PI_DEFAULT_CONFIG);
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
    [settingsConfig, onSettingsConfigChange],
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

import { useState, useCallback } from "react";
import {
  DSH_OFFICIAL_DEFAULT_CONFIG,
  DSH_CUSTOM_DEFAULT_CONFIG,
  isDshOfficial,
  normalizeDshModels,
  withDshRowKeys,
  type DshModel,
} from "../helpers/deepseekHarnessFormUtils";

interface UseDeepSeekHarnessFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    category?: string;
    meta?: { dshCurrentModel?: string; providerType?: string };
  };
  appId: string;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export interface DeepSeekHarnessFormState {
  dshProviderKey: string;
  setDshProviderKey: (key: string) => void;
  dshIsOfficial: boolean;
  dshApiKey: string;
  dshApiKeyEnv: string;
  dshBaseUrl: string;
  dshApi: string;
  dshModels: DshModel[];
  dshDefaultModel: string;
  handleDshApiKeyChange: (apiKey: string) => void;
  handleDshApiKeyEnvChange: (apiKeyEnv: string) => void;
  handleDshBaseUrlChange: (baseUrl: string) => void;
  handleDshApiChange: (api: string) => void;
  handleDshModelsChange: (models: DshModel[]) => void;
  handleDshDefaultModelChange: (model: string) => void;
  resetDshState: (
    config?: Record<string, unknown>,
    isOfficial?: boolean,
  ) => void;
}

export function useDeepSeekHarnessFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseDeepSeekHarnessFormStateParams): DeepSeekHarnessFormState {
  const initial =
    appId === "deepseek-harness" ? (initialData?.settingsConfig ?? {}) : {};
  const initialModels =
    appId === "deepseek-harness"
      ? withDshRowKeys(normalizeDshModels(initial.models))
      : [];

  const [dshProviderKey, setDshProviderKey] = useState<string>(() => {
    if (appId !== "deepseek-harness") return "";
    return (
      providerId ??
      (isDshOfficial(
        providerId,
        initialData?.category,
        initial,
        initialData?.meta?.providerType,
      )
        ? "deepseek-official"
        : "")
    );
  });

  const [dshIsOfficial, setDshIsOfficial] = useState<boolean>(() => {
    if (appId !== "deepseek-harness") return false;
    return isDshOfficial(
      providerId,
      initialData?.category,
      initial,
      initialData?.meta?.providerType,
    );
  });

  const [dshApiKey, setDshApiKey] = useState<string>(() => {
    if (appId !== "deepseek-harness") return "";
    return typeof initial.apiKey === "string" ? initial.apiKey : "";
  });

  const [dshApiKeyEnv, setDshApiKeyEnv] = useState<string>(() => {
    if (appId !== "deepseek-harness") return "";
    const official = isDshOfficial(
      providerId,
      initialData?.category,
      initial,
      initialData?.meta?.providerType,
    );
    if (typeof initial.apiKeyEnv === "string" && initial.apiKeyEnv) {
      return initial.apiKeyEnv;
    }
    return official ? "DEEPSEEK_API_KEY" : "CUSTOM_DSH_API_KEY";
  });

  const [dshBaseUrl, setDshBaseUrl] = useState<string>(() => {
    if (appId !== "deepseek-harness") return "";
    return typeof initial.baseURL === "string" ? initial.baseURL : "";
  });

  const [dshApi, setDshApi] = useState<string>(() => {
    if (appId !== "deepseek-harness") return "openai-completions";
    return typeof initial.api === "string" && initial.api
      ? initial.api
      : "openai-completions";
  });

  const [dshModels, setDshModels] = useState<DshModel[]>(() => {
    if (appId !== "deepseek-harness") return [];
    return initialModels;
  });

  const [dshDefaultModel, setDshDefaultModel] = useState<string>(() => {
    if (appId !== "deepseek-harness") return "";
    return initialData?.meta?.dshCurrentModel ?? initialModels[0]?.id ?? "";
  });

  const updateDshSettings = useCallback(
    (updater: (config: Record<string, unknown>) => void) => {
      try {
        const fallback = dshIsOfficial
          ? DSH_OFFICIAL_DEFAULT_CONFIG
          : DSH_CUSTOM_DEFAULT_CONFIG;
        const config = JSON.parse(
          getSettingsConfig() || JSON.stringify(fallback),
        ) as Record<string, unknown>;
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch (error) {
        // Field edits cannot be merged into a malformed JSON document; warn so
        // the drop is at least visible in the console instead of fully silent.
        console.warn(
          "[DeepSeekHarness] malformed settingsConfig, edit skipped",
          error,
        );
      }
    },
    [getSettingsConfig, onSettingsConfigChange, dshIsOfficial],
  );

  const handleDshApiKeyChange = useCallback(
    (apiKey: string) => {
      setDshApiKey(apiKey);
      updateDshSettings((config) => {
        config.apiKey = apiKey;
      });
    },
    [updateDshSettings],
  );

  const handleDshApiKeyEnvChange = useCallback(
    (apiKeyEnv: string) => {
      const nextValue = apiKeyEnv.toUpperCase();
      setDshApiKeyEnv(nextValue);
      updateDshSettings((config) => {
        config.apiKeyEnv = nextValue;
      });
    },
    [updateDshSettings],
  );

  const handleDshBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setDshBaseUrl(baseUrl);
      updateDshSettings((config) => {
        config.baseURL = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateDshSettings],
  );

  const handleDshApiChange = useCallback(
    (api: string) => {
      setDshApi(api);
      updateDshSettings((config) => {
        config.api = api;
      });
    },
    [updateDshSettings],
  );

  const handleDshModelsChange = useCallback(
    (models: DshModel[]) => {
      setDshModels(models);
      updateDshSettings((config) => {
        // Row keys are client-only React state and never reach the config.
        config.models = models.map(({ rowKey: _rowKey, ...model }) => model);
      });
    },
    [updateDshSettings],
  );

  const handleDshDefaultModelChange = useCallback((model: string) => {
    // The current model lives in provider meta (`dshCurrentModel`), not in the
    // native settings config, so only local state changes here.
    setDshDefaultModel(model);
  }, []);

  const resetDshState = useCallback(
    (config?: Record<string, unknown>, isOfficial?: boolean) => {
      const source = config ?? {};
      const official =
        isOfficial ?? isDshOfficial(undefined, undefined, source);
      const models = normalizeDshModels(source.models);

      setDshIsOfficial(official);
      setDshProviderKey(official ? "deepseek-official" : "");
      setDshApiKey(typeof source.apiKey === "string" ? source.apiKey : "");
      const apiKeyEnv =
        typeof source.apiKeyEnv === "string" ? source.apiKeyEnv : "";
      setDshApiKeyEnv(
        apiKeyEnv || (official ? "DEEPSEEK_API_KEY" : "CUSTOM_DSH_API_KEY"),
      );
      setDshBaseUrl(typeof source.baseURL === "string" ? source.baseURL : "");
      setDshApi(
        typeof source.api === "string" && source.api
          ? source.api
          : "openai-completions",
      );
      setDshModels(withDshRowKeys(models));
      setDshDefaultModel(models[0]?.id ?? "");
    },
    [],
  );

  return {
    dshProviderKey,
    setDshProviderKey,
    dshIsOfficial,
    dshApiKey,
    dshApiKeyEnv,
    dshBaseUrl,
    dshApi,
    dshModels,
    dshDefaultModel,
    handleDshApiKeyChange,
    handleDshApiKeyEnvChange,
    handleDshBaseUrlChange,
    handleDshApiChange,
    handleDshModelsChange,
    handleDshDefaultModelChange,
    resetDshState,
  };
}

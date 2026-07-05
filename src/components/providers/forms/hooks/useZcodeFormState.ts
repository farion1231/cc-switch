import { useCallback, useState } from "react";
import type { ZCodeModel, ZCodeProviderConfig } from "@/types";

interface UseZcodeFormStateParams {
  initialData?: {
    name?: string;
    settingsConfig?: Record<string, unknown>;
  };
  appId: string;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export interface ZcodeFormState {
  zcodeProviderKey: string;
  setZcodeProviderKey: (key: string) => void;
  zcodeKind: string;
  zcodeApiKey: string;
  zcodeBaseUrl: string;
  zcodeModels: Record<string, ZCodeModel>;
  zcodeExtraOptions: Record<string, string>;
  handleZcodeKindChange: (kind: string) => void;
  handleZcodeApiKeyChange: (apiKey: string) => void;
  handleZcodeBaseUrlChange: (baseUrl: string) => void;
  handleZcodeModelsChange: (models: Record<string, ZCodeModel>) => void;
  handleZcodeExtraOptionsChange: (options: Record<string, string>) => void;
  resetZcodeState: (config?: ZCodeProviderConfig) => void;
}

const DEFAULT_CONFIG: ZCodeProviderConfig = {
  name: "Custom Provider",
  kind: "openai-compatible",
  options: {
    baseURL: "",
    apiKey: "",
    apiKeyRequired: true,
  },
  enabled: true,
  source: "custom",
  models: {},
};

function parseZcodeConfig(
  config?: Record<string, unknown>,
): ZCodeProviderConfig {
  if (!config || typeof config !== "object") return DEFAULT_CONFIG;
  const maybe = config as Partial<ZCodeProviderConfig>;
  return {
    ...DEFAULT_CONFIG,
    ...maybe,
    kind: typeof maybe.kind === "string" ? maybe.kind : DEFAULT_CONFIG.kind,
    options: {
      ...DEFAULT_CONFIG.options,
      ...(maybe.options && typeof maybe.options === "object"
        ? maybe.options
        : {}),
    },
    models:
      maybe.models && typeof maybe.models === "object" ? maybe.models : {},
  };
}

function stringifyConfig(config: ZCodeProviderConfig): string {
  return JSON.stringify(config, null, 2);
}

function extraOptionsFromConfig(
  config: ZCodeProviderConfig,
): Record<string, string> {
  const known = new Set(["apiKey", "baseURL", "apiKeyRequired"]);
  const options: Record<string, string> = {};
  for (const [key, value] of Object.entries(config.options || {})) {
    if (known.has(key)) continue;
    options[key] = typeof value === "string" ? value : JSON.stringify(value);
  }
  return options;
}

function buildConfig(
  initialName: string | undefined,
  kind: string,
  apiKey: string,
  baseUrl: string,
  models: Record<string, ZCodeModel>,
  extraOptions: Record<string, string>,
): ZCodeProviderConfig {
  const options: Record<string, unknown> = {
    baseURL: baseUrl,
    apiKey,
    apiKeyRequired: true,
  };
  for (const [key, raw] of Object.entries(extraOptions)) {
    if (!key.trim()) continue;
    try {
      options[key] = JSON.parse(raw);
    } catch {
      options[key] = raw;
    }
  }
  return {
    name: initialName || "Custom Provider",
    kind,
    options,
    enabled: true,
    source: "custom",
    models,
  };
}

export function useZcodeFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
}: UseZcodeFormStateParams): ZcodeFormState {
  const initialConfig =
    appId === "zcode"
      ? parseZcodeConfig(initialData?.settingsConfig)
      : DEFAULT_CONFIG;

  const [zcodeProviderKey, setZcodeProviderKey] = useState<string>(
    () => providerId || "",
  );
  const [zcodeKind, setZcodeKind] = useState<string>(() => initialConfig.kind);
  const [zcodeApiKey, setZcodeApiKey] = useState<string>(() =>
    String(initialConfig.options?.apiKey || ""),
  );
  const [zcodeBaseUrl, setZcodeBaseUrl] = useState<string>(() =>
    String(initialConfig.options?.baseURL || ""),
  );
  const [zcodeModels, setZcodeModels] = useState<Record<string, ZCodeModel>>(
    () => initialConfig.models || {},
  );
  const [zcodeExtraOptions, setZcodeExtraOptions] = useState<
    Record<string, string>
  >(() => extraOptionsFromConfig(initialConfig));

  const emit = useCallback(
    (
      kind = zcodeKind,
      apiKey = zcodeApiKey,
      baseUrl = zcodeBaseUrl,
      models = zcodeModels,
      extraOptions = zcodeExtraOptions,
    ) => {
      onSettingsConfigChange(
        stringifyConfig(
          buildConfig(
            initialData?.name,
            kind,
            apiKey,
            baseUrl,
            models,
            extraOptions,
          ),
        ),
      );
    },
    [
      initialData?.name,
      onSettingsConfigChange,
      zcodeApiKey,
      zcodeBaseUrl,
      zcodeExtraOptions,
      zcodeKind,
      zcodeModels,
    ],
  );

  const handleZcodeKindChange = useCallback(
    (kind: string) => {
      setZcodeKind(kind);
      emit(kind);
    },
    [emit],
  );

  const handleZcodeApiKeyChange = useCallback(
    (apiKey: string) => {
      setZcodeApiKey(apiKey);
      emit(undefined, apiKey);
    },
    [emit],
  );

  const handleZcodeBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setZcodeBaseUrl(baseUrl);
      emit(undefined, undefined, baseUrl);
    },
    [emit],
  );

  const handleZcodeModelsChange = useCallback(
    (models: Record<string, ZCodeModel>) => {
      setZcodeModels(models);
      emit(undefined, undefined, undefined, models);
    },
    [emit],
  );

  const handleZcodeExtraOptionsChange = useCallback(
    (options: Record<string, string>) => {
      setZcodeExtraOptions(options);
      emit(undefined, undefined, undefined, undefined, options);
    },
    [emit],
  );

  const resetZcodeState = useCallback(
    (config?: ZCodeProviderConfig) => {
      const next = config || DEFAULT_CONFIG;
      setZcodeKind(next.kind);
      setZcodeApiKey(String(next.options?.apiKey || ""));
      setZcodeBaseUrl(String(next.options?.baseURL || ""));
      setZcodeModels(next.models || {});
      setZcodeExtraOptions(extraOptionsFromConfig(next));
      onSettingsConfigChange(stringifyConfig(next));
    },
    [onSettingsConfigChange],
  );

  return {
    zcodeProviderKey,
    setZcodeProviderKey,
    zcodeKind,
    zcodeApiKey,
    zcodeBaseUrl,
    zcodeModels,
    zcodeExtraOptions,
    handleZcodeKindChange,
    handleZcodeApiKeyChange,
    handleZcodeBaseUrlChange,
    handleZcodeModelsChange,
    handleZcodeExtraOptionsChange,
    resetZcodeState,
  };
}

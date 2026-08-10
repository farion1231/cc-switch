import { useState, useCallback, useMemo, useEffect } from "react";
import type { AppId } from "@/lib/api";
import type { PiApiType, PiModel } from "@/config/piProviderPresets";
import { useProvidersQuery } from "@/lib/query/queries";

interface UsePiFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
}

export interface PiFormState {
  piProviderKey: string;
  piBaseUrl: string;
  piApiKey: string;
  piApiType: PiApiType;
  piModels: PiModel[];
  baseUrlEnvKey: string;
  apiKeyEnvKey: string;
  existingPiKeys: string[];
  setPiProviderKey: (key: string) => void;
  setPiBaseUrl: (url: string) => void;
  setPiApiKey: (key: string) => void;
  setPiApiType: (type: PiApiType) => void;
  setPiModels: (models: PiModel[]) => void;
  resetPiState: (config?: Record<string, unknown>) => void;
}

/** Explicit `api` values recognised from settings_config (kept in sync with PI_API_TYPES). */
const PI_KNOWN_API_TYPES: string[] = [
  "anthropic-messages",
  "openai-completions",
  "openai-responses",
  "google-generative-ai",
];

/**
 * Detect the API type from env keys in the config.
 * If OPENAI_* keys are present, assume openai-completions.
 * Otherwise default to anthropic-messages.
 */
function detectApiType(config?: Record<string, unknown>): PiApiType {
  const env = (config?.env as Record<string, unknown>) || {};
  if ("OPENAI_BASE_URL" in env || "OPENAI_API_KEY" in env) {
    return "openai-completions";
  }
  return "anthropic-messages";
}

export const PI_DEFAULT_CONFIG = JSON.stringify(
  { api: "anthropic-messages", env: {}, models: [] },
  null,
  2,
);

/**
 * Shared contract for Pi env key names per API protocol.
 * Keep in sync with PI_API_TYPES and the Rust `resolve_usage_credentials`/live.rs
 * fallback chains (ANTHROPIC_* first, then OPENAI_BASE_URL, then OPENAI_API_BASE).
 */
export function getPiEnvKeys(apiType: PiApiType): {
  baseUrlKey: string;
  apiKeyKey: string;
} {
  switch (apiType) {
    case "openai-completions":
    case "openai-responses":
      return { baseUrlKey: "OPENAI_BASE_URL", apiKeyKey: "OPENAI_API_KEY" };
    case "anthropic-messages":
    case "google-generative-ai":
    default:
      return {
        baseUrlKey: "ANTHROPIC_BASE_URL",
        apiKeyKey: "ANTHROPIC_API_KEY",
      };
  }
}

export function usePiFormState({
  appId,
  providerId,
  initialData,
}: UsePiFormStateParams): PiFormState {
  const { data: piProvidersData } = useProvidersQuery("pi");
  const existingPiKeys = useMemo(() => {
    if (appId !== "pi" || !piProvidersData?.providers) return [];
    return Object.keys(piProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [appId, piProvidersData?.providers, providerId]);

  const [piProviderKey, setPiProviderKeyRaw] = useState<string>(() => {
    if (appId !== "pi") return "";
    return providerId || "";
  });

  const initialApiType = useMemo(() => {
    if (appId !== "pi" || !initialData?.settingsConfig)
      return "anthropic-messages" as PiApiType;
    // Check for explicit api field first
    const explicitApi = initialData.settingsConfig.api as string | undefined;
    if (explicitApi && PI_KNOWN_API_TYPES.includes(explicitApi)) {
      return explicitApi as PiApiType;
    }
    return detectApiType(initialData.settingsConfig);
  }, [appId, initialData]);

  const initialEnvKeys = useMemo(
    () => getPiEnvKeys(initialApiType),
    [initialApiType],
  );

  const [piApiType, setPiApiTypeState] = useState<PiApiType>(initialApiType);
  const [baseUrlEnvKey, setBaseUrlEnvKey] = useState(initialEnvKeys.baseUrlKey);
  const [apiKeyEnvKey, setApiKeyEnvKey] = useState(initialEnvKeys.apiKeyKey);

  const [piBaseUrl, setPiBaseUrl] = useState<string>(() => {
    if (appId !== "pi" || !initialData?.settingsConfig) return "";
    const env =
      (initialData.settingsConfig.env as Record<string, unknown>) || {};
    return (env[initialEnvKeys.baseUrlKey] as string) || "";
  });

  const [piApiKey, setPiApiKey] = useState<string>(() => {
    if (appId !== "pi" || !initialData?.settingsConfig) return "";
    const env =
      (initialData.settingsConfig.env as Record<string, unknown>) || {};
    return (env[initialEnvKeys.apiKeyKey] as string) || "";
  });

  const [piModels, setPiModels] = useState<PiModel[]>(() => {
    if (appId !== "pi" || !initialData?.settingsConfig) return [];
    const models = initialData.settingsConfig.models;
    if (Array.isArray(models)) {
      return models as PiModel[];
    }
    return [];
  });

  // Reset state when the editing target (appId + providerId) changes to avoid
  // stale initialData from a previous edit target.
  useEffect(() => {
    if (appId === "pi" && initialData?.settingsConfig) {
      resetPiState(initialData.settingsConfig);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appId, providerId]);

  const setPiApiType = useCallback((type: PiApiType) => {
    setPiApiTypeState(type);
    const keys = getPiEnvKeys(type);
    setBaseUrlEnvKey(keys.baseUrlKey);
    setApiKeyEnvKey(keys.apiKeyKey);
  }, []);

  const setPiProviderKey = useCallback((key: string) => {
    setPiProviderKeyRaw(key.toLowerCase().replace(/[^a-z0-9-]/g, ""));
  }, []);

  const resetPiState = useCallback((config?: Record<string, unknown>) => {
    setPiProviderKeyRaw("");
    if (config) {
      const explicitApi = config.api as string | undefined;
      const newApiType =
        explicitApi && PI_KNOWN_API_TYPES.includes(explicitApi)
          ? (explicitApi as PiApiType)
          : detectApiType(config);
      setPiApiTypeState(newApiType);
      const keys = getPiEnvKeys(newApiType);
      setBaseUrlEnvKey(keys.baseUrlKey);
      setApiKeyEnvKey(keys.apiKeyKey);
      const env = (config.env as Record<string, unknown>) || {};
      setPiBaseUrl((env[keys.baseUrlKey] as string) || "");
      setPiApiKey((env[keys.apiKeyKey] as string) || "");
      const models = config.models;
      setPiModels(Array.isArray(models) ? (models as PiModel[]) : []);
    } else {
      setPiApiTypeState("anthropic-messages");
      setBaseUrlEnvKey("ANTHROPIC_BASE_URL");
      setApiKeyEnvKey("ANTHROPIC_API_KEY");
      setPiBaseUrl("");
      setPiApiKey("");
      setPiModels([]);
    }
  }, []);

  return {
    piProviderKey,
    piBaseUrl,
    piApiKey,
    piApiType,
    piModels,
    baseUrlEnvKey,
    apiKeyEnvKey,
    existingPiKeys,
    setPiProviderKey,
    setPiBaseUrl,
    setPiApiKey,
    setPiApiType,
    setPiModels,
    resetPiState,
  };
}

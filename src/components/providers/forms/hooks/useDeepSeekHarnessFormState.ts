import { useState, useCallback, useMemo, useRef, useEffect } from "react";
import {
  DSH_OFFICIAL_DEFAULT_CONFIG,
  DSH_CUSTOM_DEFAULT_CONFIG,
  isDshOfficial,
  normalizeDshModels,
  withDshRowKeys,
  type DshModel,
} from "../helpers/deepseekHarnessFormUtils";
import { reserveApiKeyEnv } from "@/config/deepseekHarnessProviderPresets";

interface UseDeepSeekHarnessFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    category?: string;
    meta?: { dshCurrentModel?: string; providerType?: string };
  };
  appId: string;
  providerId?: string;
  /** Credential references already used by native routes; a new custom
   * route's default reference must avoid them (a shared reference would let
   * the second route's key silently overwrite the first's). */
  usedCredentialRefs?: string[];
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
  usedCredentialRefs,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseDeepSeekHarnessFormStateParams): DeepSeekHarnessFormState {
  const initial =
    appId === "deepseek-harness" ? (initialData?.settingsConfig ?? {}) : {};
  const initialModels =
    appId === "deepseek-harness"
      ? withDshRowKeys(normalizeDshModels(initial.models))
      : [];
  const usedRefs = useMemo(
    () => new Set(usedCredentialRefs ?? []),
    [usedCredentialRefs],
  );
  const suggestApiKeyEnv = useCallback(
    (providerKey: string) =>
      reserveApiKeyEnv(providerKey.trim() || "Custom DSH", new Set(usedRefs)),
    [usedRefs],
  );
  // The reference auto-derived from the provider key. While the user has not
  // edited the credential ref by hand, key edits keep regenerating it.
  const autoApiKeyEnvRef = useRef<string | null>(null);

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
    if (
      (official || providerId) &&
      typeof initial.apiKeyEnv === "string" &&
      initial.apiKeyEnv
    ) {
      return initial.apiKeyEnv;
    }
    if (official) return "DEEPSEEK_API_KEY";
    const suggested = suggestApiKeyEnv(providerId ?? "");
    autoApiKeyEnvRef.current = suggested;
    return suggested;
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

  // Native credential refs are loaded asynchronously. Re-evaluate only automatic
  // suggestions, never persisted or manually entered refs, against that snapshot.
  useEffect(() => {
    if (
      appId !== "deepseek-harness" ||
      dshIsOfficial ||
      autoApiKeyEnvRef.current === null
    )
      return;
    const nextValue = suggestApiKeyEnv(dshProviderKey);
    if (nextValue === dshApiKeyEnv) return;
    autoApiKeyEnvRef.current = nextValue;
    setDshApiKeyEnv(nextValue);
    updateDshSettings((config) => {
      config.apiKeyEnv = nextValue;
    });
  }, [
    appId,
    dshIsOfficial,
    dshProviderKey,
    dshApiKeyEnv,
    suggestApiKeyEnv,
    updateDshSettings,
  ]);

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
      autoApiKeyEnvRef.current = null;
      setDshApiKeyEnv(nextValue);
      updateDshSettings((config) => {
        config.apiKeyEnv = nextValue;
      });
    },
    [updateDshSettings],
  );

  const setDshProviderKeyWithRefSync = useCallback(
    (key: string) => {
      setDshProviderKey(key);
      if (dshIsOfficial) return;
      // Persisted and hand-edited references are authoritative, including the
      // legacy default. Only this form's own suggestions follow key edits.
      if (autoApiKeyEnvRef.current === null) return;
      const nextValue = suggestApiKeyEnv(key);
      autoApiKeyEnvRef.current = nextValue;
      setDshApiKeyEnv(nextValue);
      updateDshSettings((config) => {
        config.apiKeyEnv = nextValue;
      });
    },
    [dshApiKeyEnv, dshIsOfficial, suggestApiKeyEnv, updateDshSettings],
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
      if (official) {
        autoApiKeyEnvRef.current = null;
        setDshApiKeyEnv(apiKeyEnv || "DEEPSEEK_API_KEY");
      } else {
        // Reset selects a new preset/custom route, not an existing route.
        // Catalog references are templates, not permission to share credentials.
        const suggested = suggestApiKeyEnv("");
        autoApiKeyEnvRef.current = suggested;
        setDshApiKeyEnv(suggested);
      }
      setDshBaseUrl(typeof source.baseURL === "string" ? source.baseURL : "");
      setDshApi(
        typeof source.api === "string" && source.api
          ? source.api
          : "openai-completions",
      );
      setDshModels(withDshRowKeys(models));
      setDshDefaultModel(models[0]?.id ?? "");
    },
    [suggestApiKeyEnv],
  );

  return {
    dshProviderKey,
    setDshProviderKey: setDshProviderKeyWithRefSync,
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

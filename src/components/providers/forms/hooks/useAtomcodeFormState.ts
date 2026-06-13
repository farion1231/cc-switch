import { useState, useCallback, useMemo } from "react";
import type { AppId } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import type { AtomcodeProviderSettingsConfig } from "@/config/atomcodeProviderPresets";

type AtomcodeType = "openai" | "claude" | "ollama";
const VALID_ATOMCODE_TYPES: readonly AtomcodeType[] = ["openai", "claude", "ollama"];

interface UseAtomcodeFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

const ATOMCODE_DEFAULT_CONFIG_OBJ: AtomcodeProviderSettingsConfig = {
  providerKey: "",
  type: "openai",
  model: "",
};

export const ATOMCODE_DEFAULT_CONFIG = JSON.stringify(
  ATOMCODE_DEFAULT_CONFIG_OBJ,
  null,
  2,
);

export interface AtomcodeFormState {
  atomcodeProviderKey: string;
  setAtomcodeProviderKey: (key: string) => void;
  atomcodeType: AtomcodeType;
  atomcodeModel: string;
  atomcodeApiKey: string;
  atomcodeBaseUrl: string;
  atomcodeContextWindow: number | undefined;
  atomcodeThinkingEnabled: boolean;
  atomcodeThinkingBudget: number | undefined;
  existingAtomcodeKeys: string[];
  handleAtomcodeTypeChange: (type: AtomcodeType) => void;
  handleAtomcodeModelChange: (model: string) => void;
  handleAtomcodeApiKeyChange: (apiKey: string) => void;
  handleAtomcodeBaseUrlChange: (baseUrl: string) => void;
  handleAtomcodeContextWindowChange: (val: number | undefined) => void;
  handleAtomcodeThinkingEnabledChange: (enabled: boolean) => void;
  handleAtomcodeThinkingBudgetChange: (budget: number | undefined) => void;
  resetAtomcodeState: (
    config?: Partial<AtomcodeProviderSettingsConfig & { thinking_enabled?: boolean; thinking_budget?: number }>,
  ) => void;
}

function parseAtomcodeField<T>(
  initialData: UseAtomcodeFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    if (initialData?.settingsConfig) {
      const v = initialData.settingsConfig[field];
      return v !== undefined && v !== null ? (v as T) : fallback;
    }
    return fallback;
  } catch {
    return fallback;
  }
}

export function useAtomcodeFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseAtomcodeFormStateParams): AtomcodeFormState {
  const { data: atomcodeProvidersData } = useProvidersQuery("atomcode");
  const existingAtomcodeKeys = useMemo(() => {
    if (!atomcodeProvidersData?.providers) return [];
    return Object.keys(atomcodeProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [atomcodeProvidersData?.providers, providerId]);

  const [atomcodeProviderKey, setAtomcodeProviderKey] = useState<string>(() => {
    if (appId !== "atomcode") return "";
    return providerId || parseAtomcodeField(initialData, "providerKey", "");
  });

  const [atomcodeType, setAtomcodeType] = useState<AtomcodeType>(() => {
    if (appId !== "atomcode") return "openai";
    const rawType = parseAtomcodeField(initialData, "type", "openai");
    return (VALID_ATOMCODE_TYPES as readonly string[]).includes(rawType)
      ? (rawType as AtomcodeType)
      : "openai";
  });

  const [atomcodeModel, setAtomcodeModel] = useState<string>(() => {
    if (appId !== "atomcode") return "";
    return parseAtomcodeField(initialData, "model", "");
  });

  const [atomcodeApiKey, setAtomcodeApiKey] = useState<string>(() => {
    if (appId !== "atomcode") return "";
    return parseAtomcodeField(initialData, "api_key", "");
  });

  const [atomcodeBaseUrl, setAtomcodeBaseUrl] = useState<string>(() => {
    if (appId !== "atomcode") return "";
    return parseAtomcodeField(initialData, "base_url", "");
  });

  const [atomcodeContextWindow, setAtomcodeContextWindow] = useState<
    number | undefined
  >(() => {
    if (appId !== "atomcode") return undefined;
    const raw = parseAtomcodeField<number | undefined>(
      initialData,
      "context_window",
      undefined,
    );
    return typeof raw === "number" && raw > 0 ? raw : undefined;
  });

  const [atomcodeThinkingEnabled, setAtomcodeThinkingEnabled] = useState<boolean>(
    () => {
      if (appId !== "atomcode") return false;
      return parseAtomcodeField(initialData, "thinking_enabled", false);
    },
  );

  const [atomcodeThinkingBudget, setAtomcodeThinkingBudget] = useState<
    number | undefined
  >(() => {
    if (appId !== "atomcode") return undefined;
    const raw = parseAtomcodeField<number | undefined>(
      initialData,
      "thinking_budget",
      undefined,
    );
    return typeof raw === "number" && raw > 0 ? raw : undefined;
  });

  const buildAndEmitConfig = useCallback(
    (overrides: Record<string, unknown>) => {
      try {
        // Build flat config from current state + overrides
        const current = JSON.parse(
          getSettingsConfig() || ATOMCODE_DEFAULT_CONFIG,
        ) as Record<string, unknown>;
        const merged = { ...current, ...overrides };

        // Remove blank optional fields
        const clean: Record<string, unknown> = {};
        for (const [k, v] of Object.entries(merged)) {
          if (
            v === "" ||
            v === null ||
            v === undefined ||
            (typeof v === "number" && !Number.isFinite(v))
          ) {
            continue;
          }
          clean[k] = v;
        }
        onSettingsConfigChange(JSON.stringify(clean, null, 2));
      } catch {
        // ignore parse errors
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleAtomcodeTypeChange = useCallback(
    (type: AtomcodeType) => {
      setAtomcodeType(type);
      buildAndEmitConfig({ type });
    },
    [buildAndEmitConfig],
  );

  const handleAtomcodeModelChange = useCallback(
    (model: string) => {
      setAtomcodeModel(model);
      buildAndEmitConfig({ model });
    },
    [buildAndEmitConfig],
  );

  const handleAtomcodeApiKeyChange = useCallback(
    (apiKey: string) => {
      setAtomcodeApiKey(apiKey);
      buildAndEmitConfig({ api_key: apiKey || undefined });
    },
    [buildAndEmitConfig],
  );

  const handleAtomcodeBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setAtomcodeBaseUrl(baseUrl);
      buildAndEmitConfig({ base_url: baseUrl.trim() || undefined });
    },
    [buildAndEmitConfig],
  );

  const handleAtomcodeContextWindowChange = useCallback(
    (val: number | undefined) => {
      setAtomcodeContextWindow(val);
      buildAndEmitConfig({ context_window: val });
    },
    [buildAndEmitConfig],
  );

  const handleAtomcodeThinkingEnabledChange = useCallback(
    (enabled: boolean) => {
      setAtomcodeThinkingEnabled(enabled);
      buildAndEmitConfig({ thinking_enabled: enabled || undefined });
    },
    [buildAndEmitConfig],
  );

  const handleAtomcodeThinkingBudgetChange = useCallback(
    (budget: number | undefined) => {
      setAtomcodeThinkingBudget(budget);
      buildAndEmitConfig({ thinking_budget: budget });
    },
    [buildAndEmitConfig],
  );

  const resetAtomcodeState = useCallback(
    (
      config?: Partial<
        AtomcodeProviderSettingsConfig & {
          thinking_enabled?: boolean;
          thinking_budget?: number;
        }
      >,
    ) => {
      setAtomcodeProviderKey(config?.providerKey || "");
      const rawResetType = config?.type ?? "openai";
      setAtomcodeType(
        (VALID_ATOMCODE_TYPES as readonly string[]).includes(rawResetType)
          ? (rawResetType as AtomcodeType)
          : "openai",
      );
      setAtomcodeModel(config?.model || "");
      setAtomcodeApiKey(config?.api_key || "");
      setAtomcodeBaseUrl(config?.base_url || "");
      setAtomcodeContextWindow(
        typeof config?.context_window === "number" && config.context_window > 0
          ? config.context_window
          : undefined,
      );
      setAtomcodeThinkingEnabled(config?.thinking_enabled ?? false);
      setAtomcodeThinkingBudget(
        typeof config?.thinking_budget === "number" && config.thinking_budget > 0
          ? config.thinking_budget
          : undefined,
      );
    },
    [],
  );

  return {
    atomcodeProviderKey,
    setAtomcodeProviderKey,
    atomcodeType,
    atomcodeModel,
    atomcodeApiKey,
    atomcodeBaseUrl,
    atomcodeContextWindow,
    atomcodeThinkingEnabled,
    atomcodeThinkingBudget,
    existingAtomcodeKeys,
    handleAtomcodeTypeChange,
    handleAtomcodeModelChange,
    handleAtomcodeApiKeyChange,
    handleAtomcodeBaseUrlChange,
    handleAtomcodeContextWindowChange,
    handleAtomcodeThinkingEnabledChange,
    handleAtomcodeThinkingBudgetChange,
    resetAtomcodeState,
  };
}

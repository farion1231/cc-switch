import { useCallback, useMemo, useState } from "react";
import type { AppId } from "@/lib/api";
import {
  kimiDefaultSettingsConfig,
  kimiOfficialProviderKey,
  type KimiEditorConfig,
  type KimiModelEntry,
  type KimiProviderConfig,
  type KimiProviderSettingsConfig,
  type KimiProviderType,
} from "@/config/kimiProviderPresets";
import { KIMI_DEFAULT_EDITOR_CONFIG } from "@/utils/kimiConfigUtils";

interface UseKimiFormStateParams {
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export interface KimiFormState {
  kimiProviderKey: string;
  kimiProviderType: KimiProviderType;
  kimiBaseUrl: string;
  kimiApiKey: string;
  kimiOauth: boolean;
  kimiEnv: Record<string, string>;
  kimiCustomHeaders: Record<string, string>;
  kimiModels: KimiModelEntry[];
  kimiDefaultModel: string;
  kimiDefaultThinking: boolean | undefined;
  kimiDefaultPermissionMode: string;
  kimiDefaultPlanMode: boolean | undefined;
  kimiMergeAllAvailableSkills: boolean | undefined;
  kimiTelemetry: boolean | undefined;
  kimiThinkingMode: string;
  kimiThinkingEffort: string;
  kimiMaxRetriesPerStep: number | undefined;
  kimiReservedContextSize: number | undefined;
  kimiMaxRunningTasks: number | undefined;
  kimiKeepAliveOnExit: boolean | undefined;
  kimiMicroCompaction: boolean | undefined;
  handleKimiProviderKeyChange: (key: string) => void;
  handleKimiProviderTypeChange: (type: KimiProviderType) => void;
  handleKimiBaseUrlChange: (baseUrl: string) => void;
  handleKimiApiKeyChange: (apiKey: string) => void;
  handleKimiOauthChange: (enabled: boolean) => void;
  handleKimiEnvChange: (env: Record<string, string>) => void;
  handleKimiCustomHeadersChange: (headers: Record<string, string>) => void;
  handleKimiModelsChange: (models: KimiModelEntry[]) => void;
  handleKimiDefaultModelChange: (model: string) => void;
  handleKimiDefaultThinkingChange: (value: boolean | undefined) => void;
  handleKimiDefaultPermissionModeChange: (value: string) => void;
  handleKimiDefaultPlanModeChange: (value: boolean | undefined) => void;
  handleKimiMergeAllAvailableSkillsChange: (value: boolean | undefined) => void;
  handleKimiTelemetryChange: (value: boolean | undefined) => void;
  handleKimiThinkingModeChange: (value: string) => void;
  handleKimiThinkingEffortChange: (value: string) => void;
  handleKimiMaxRetriesPerStepChange: (value: number | undefined) => void;
  handleKimiReservedContextSizeChange: (value: number | undefined) => void;
  handleKimiMaxRunningTasksChange: (value: number | undefined) => void;
  handleKimiKeepAliveOnExitChange: (value: boolean | undefined) => void;
  handleKimiMicroCompactionChange: (value: boolean | undefined) => void;
  resetKimiState: (
    config?: KimiProviderSettingsConfig,
    providerKey?: string,
  ) => void;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === "object" && !Array.isArray(value);

const asStringRecord = (value: unknown): Record<string, string> => {
  if (!isRecord(value)) return {};
  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, string] => {
      return typeof entry[1] === "string";
    }),
  );
};

const asNumber = (value: unknown): number | undefined =>
  typeof value === "number" && Number.isFinite(value) ? value : undefined;

const asBoolean = (value: unknown): boolean | undefined =>
  typeof value === "boolean" ? value : undefined;

const normalizeProviderKey = (value: string) => value.trim();

function parseSettingsConfig(raw: string): KimiProviderSettingsConfig {
  try {
    const parsed = JSON.parse(raw || KIMI_DEFAULT_EDITOR_CONFIG);
    if (isRecord(parsed) && isRecord(parsed.config)) {
      return parsed as unknown as KimiProviderSettingsConfig;
    }
  } catch {
    // fall through
  }
  return kimiDefaultSettingsConfig;
}

function firstProviderKey(config: KimiEditorConfig): string {
  return Object.keys(config.providers ?? {})[0] ?? kimiOfficialProviderKey;
}

function resolveProviderConfig(
  config: KimiEditorConfig,
  providerKey: string,
): KimiProviderConfig {
  return (
    config.providers?.[providerKey] ??
    config.providers?.[firstProviderKey(config)] ??
    kimiDefaultSettingsConfig.config.providers?.[kimiOfficialProviderKey] ?? {
      type: "kimi",
    }
  );
}

function modelsToEntries(config: KimiEditorConfig): KimiModelEntry[] {
  return Object.entries(config.models ?? {}).map(([id, model]) => ({
    id,
    ...model,
  }));
}

function deriveState(raw: string, providerId?: string) {
  const settings = parseSettingsConfig(raw);
  const config = settings.config;
  const providerKey = normalizeProviderKey(
    providerId ?? firstProviderKey(config),
  );
  const provider = resolveProviderConfig(config, providerKey);

  return {
    providerKey,
    provider,
    models: modelsToEntries(config),
    defaultModel: config.default_model ?? "",
    defaultThinking: asBoolean(config.default_thinking),
    defaultPermissionMode: config.default_permission_mode ?? "",
    defaultPlanMode: asBoolean(config.default_plan_mode),
    mergeAllAvailableSkills: asBoolean(config.merge_all_available_skills),
    telemetry: asBoolean(config.telemetry),
    thinkingMode: config.thinking?.mode ?? "",
    thinkingEffort: config.thinking?.effort ?? "",
    maxRetriesPerStep: asNumber(config.loop_control?.max_retries_per_step),
    reservedContextSize: asNumber(config.loop_control?.reserved_context_size),
    maxRunningTasks: asNumber(config.background?.max_running_tasks),
    keepAliveOnExit: asBoolean(config.background?.keep_alive_on_exit),
    microCompaction: asBoolean(config.experimental?.micro_compaction),
  };
}

export function useKimiFormState({
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseKimiFormStateParams): KimiFormState {
  const initial = useMemo(
    () =>
      appId === "kimi"
        ? deriveState(getSettingsConfig(), providerId)
        : deriveState(KIMI_DEFAULT_EDITOR_CONFIG),
    [appId, getSettingsConfig, providerId],
  );

  const [kimiProviderKey, setKimiProviderKey] = useState(initial.providerKey);
  const [kimiProviderType, setKimiProviderType] = useState<KimiProviderType>(
    initial.provider.type,
  );
  const [kimiBaseUrl, setKimiBaseUrl] = useState(
    initial.provider.base_url ?? "",
  );
  const [kimiApiKey, setKimiApiKey] = useState(initial.provider.api_key ?? "");
  const [kimiOauth, setKimiOauth] = useState(initial.provider.oauth === true);
  const [kimiEnv, setKimiEnv] = useState<Record<string, string>>(
    asStringRecord(initial.provider.env),
  );
  const [kimiCustomHeaders, setKimiCustomHeaders] = useState<
    Record<string, string>
  >(asStringRecord(initial.provider.custom_headers));
  const [kimiModels, setKimiModels] = useState<KimiModelEntry[]>(
    initial.models,
  );
  const [kimiDefaultModel, setKimiDefaultModel] = useState(
    initial.defaultModel,
  );
  const [kimiDefaultThinking, setKimiDefaultThinking] = useState(
    initial.defaultThinking,
  );
  const [kimiDefaultPermissionMode, setKimiDefaultPermissionMode] = useState(
    initial.defaultPermissionMode,
  );
  const [kimiDefaultPlanMode, setKimiDefaultPlanMode] = useState(
    initial.defaultPlanMode,
  );
  const [kimiMergeAllAvailableSkills, setKimiMergeAllAvailableSkills] =
    useState(initial.mergeAllAvailableSkills);
  const [kimiTelemetry, setKimiTelemetry] = useState(initial.telemetry);
  const [kimiThinkingMode, setKimiThinkingMode] = useState(
    initial.thinkingMode,
  );
  const [kimiThinkingEffort, setKimiThinkingEffort] = useState(
    initial.thinkingEffort,
  );
  const [kimiMaxRetriesPerStep, setKimiMaxRetriesPerStep] = useState(
    initial.maxRetriesPerStep,
  );
  const [kimiReservedContextSize, setKimiReservedContextSize] = useState(
    initial.reservedContextSize,
  );
  const [kimiMaxRunningTasks, setKimiMaxRunningTasks] = useState(
    initial.maxRunningTasks,
  );
  const [kimiKeepAliveOnExit, setKimiKeepAliveOnExit] = useState(
    initial.keepAliveOnExit,
  );
  const [kimiMicroCompaction, setKimiMicroCompaction] = useState(
    initial.microCompaction,
  );

  const updateKimiConfig = useCallback(
    (updater: (settings: KimiProviderSettingsConfig) => void) => {
      const settings = parseSettingsConfig(getSettingsConfig());
      updater(settings);
      onSettingsConfigChange(JSON.stringify(settings, null, 2));
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const ensureProvider = useCallback(
    (settings: KimiProviderSettingsConfig, providerKey = kimiProviderKey) => {
      const key = normalizeProviderKey(providerKey) || kimiOfficialProviderKey;
      settings.config.providers ??= {};
      settings.config.providers[key] ??= {
        type: kimiProviderType,
        api_key: kimiApiKey,
        base_url: kimiBaseUrl,
      };
      return settings.config.providers[key];
    },
    [kimiApiKey, kimiBaseUrl, kimiProviderKey, kimiProviderType],
  );

  const handleKimiProviderKeyChange = useCallback(
    (key: string) => {
      const nextKey = normalizeProviderKey(key);
      const previousKey = kimiProviderKey;
      setKimiProviderKey(key);
      updateKimiConfig((settings) => {
        settings.config.providers ??= {};
        if (nextKey && previousKey && nextKey !== previousKey) {
          settings.config.providers[nextKey] = settings.config.providers[
            previousKey
          ] ?? {
            type: kimiProviderType,
            api_key: kimiApiKey,
            base_url: kimiBaseUrl,
          };
          delete settings.config.providers[previousKey];
          Object.values(settings.config.models ?? {}).forEach((model) => {
            if (model.provider === previousKey) {
              model.provider = nextKey;
            }
          });
        }
      });
    },
    [
      kimiApiKey,
      kimiBaseUrl,
      kimiProviderKey,
      kimiProviderType,
      updateKimiConfig,
    ],
  );

  const handleKimiProviderTypeChange = useCallback(
    (type: KimiProviderType) => {
      setKimiProviderType(type);
      updateKimiConfig((settings) => {
        ensureProvider(settings).type = type;
      });
    },
    [ensureProvider, updateKimiConfig],
  );

  const handleKimiBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setKimiBaseUrl(baseUrl);
      updateKimiConfig((settings) => {
        ensureProvider(settings).base_url = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [ensureProvider, updateKimiConfig],
  );

  const handleKimiApiKeyChange = useCallback(
    (apiKey: string) => {
      setKimiApiKey(apiKey);
      updateKimiConfig((settings) => {
        ensureProvider(settings).api_key = apiKey;
      });
    },
    [ensureProvider, updateKimiConfig],
  );

  const handleKimiOauthChange = useCallback(
    (enabled: boolean) => {
      setKimiOauth(enabled);
      updateKimiConfig((settings) => {
        const provider = ensureProvider(settings);
        if (enabled) {
          provider.oauth = true;
        } else {
          delete provider.oauth;
        }
      });
    },
    [ensureProvider, updateKimiConfig],
  );

  const handleKimiEnvChange = useCallback(
    (env: Record<string, string>) => {
      setKimiEnv(env);
      updateKimiConfig((settings) => {
        ensureProvider(settings).env = env;
      });
    },
    [ensureProvider, updateKimiConfig],
  );

  const handleKimiCustomHeadersChange = useCallback(
    (headers: Record<string, string>) => {
      setKimiCustomHeaders(headers);
      updateKimiConfig((settings) => {
        ensureProvider(settings).custom_headers = headers;
      });
    },
    [ensureProvider, updateKimiConfig],
  );

  const handleKimiModelsChange = useCallback(
    (models: KimiModelEntry[]) => {
      setKimiModels(models);
      updateKimiConfig((settings) => {
        settings.config.models = Object.fromEntries(
          models
            .filter((model) => model.id.trim())
            .map(({ id, ...model }) => [id.trim(), model]),
        );
        if (
          settings.config.default_model &&
          !settings.config.models[settings.config.default_model]
        ) {
          settings.config.default_model = models[0]?.id;
          setKimiDefaultModel(models[0]?.id ?? "");
        }
      });
    },
    [updateKimiConfig],
  );

  const handleKimiDefaultModelChange = useCallback(
    (model: string) => {
      setKimiDefaultModel(model);
      updateKimiConfig((settings) => {
        settings.config.default_model = model;
      });
    },
    [updateKimiConfig],
  );

  const setTopLevel = useCallback(
    <T>(
      setter: (value: T) => void,
      field: keyof KimiEditorConfig,
      value: T,
    ) => {
      setter(value);
      updateKimiConfig((settings) => {
        if (value === undefined || value === "") {
          delete settings.config[field];
        } else {
          (settings.config as Record<string, unknown>)[field] = value;
        }
      });
    },
    [updateKimiConfig],
  );

  const resetKimiState = useCallback(
    (settingsConfig?: KimiProviderSettingsConfig, providerKey?: string) => {
      const raw = JSON.stringify(settingsConfig ?? kimiDefaultSettingsConfig);
      const next = deriveState(raw, providerKey);
      setKimiProviderKey(next.providerKey);
      setKimiProviderType(next.provider.type);
      setKimiBaseUrl(next.provider.base_url ?? "");
      setKimiApiKey(next.provider.api_key ?? "");
      setKimiOauth(next.provider.oauth === true);
      setKimiEnv(asStringRecord(next.provider.env));
      setKimiCustomHeaders(asStringRecord(next.provider.custom_headers));
      setKimiModels(next.models);
      setKimiDefaultModel(next.defaultModel);
      setKimiDefaultThinking(next.defaultThinking);
      setKimiDefaultPermissionMode(next.defaultPermissionMode);
      setKimiDefaultPlanMode(next.defaultPlanMode);
      setKimiMergeAllAvailableSkills(next.mergeAllAvailableSkills);
      setKimiTelemetry(next.telemetry);
      setKimiThinkingMode(next.thinkingMode);
      setKimiThinkingEffort(next.thinkingEffort);
      setKimiMaxRetriesPerStep(next.maxRetriesPerStep);
      setKimiReservedContextSize(next.reservedContextSize);
      setKimiMaxRunningTasks(next.maxRunningTasks);
      setKimiKeepAliveOnExit(next.keepAliveOnExit);
      setKimiMicroCompaction(next.microCompaction);
    },
    [],
  );

  return {
    kimiProviderKey,
    kimiProviderType,
    kimiBaseUrl,
    kimiApiKey,
    kimiOauth,
    kimiEnv,
    kimiCustomHeaders,
    kimiModels,
    kimiDefaultModel,
    kimiDefaultThinking,
    kimiDefaultPermissionMode,
    kimiDefaultPlanMode,
    kimiMergeAllAvailableSkills,
    kimiTelemetry,
    kimiThinkingMode,
    kimiThinkingEffort,
    kimiMaxRetriesPerStep,
    kimiReservedContextSize,
    kimiMaxRunningTasks,
    kimiKeepAliveOnExit,
    kimiMicroCompaction,
    handleKimiProviderKeyChange,
    handleKimiProviderTypeChange,
    handleKimiBaseUrlChange,
    handleKimiApiKeyChange,
    handleKimiOauthChange,
    handleKimiEnvChange,
    handleKimiCustomHeadersChange,
    handleKimiModelsChange,
    handleKimiDefaultModelChange,
    handleKimiDefaultThinkingChange: (value) =>
      setTopLevel(setKimiDefaultThinking, "default_thinking", value),
    handleKimiDefaultPermissionModeChange: (value) =>
      setTopLevel(
        setKimiDefaultPermissionMode,
        "default_permission_mode",
        value,
      ),
    handleKimiDefaultPlanModeChange: (value) =>
      setTopLevel(setKimiDefaultPlanMode, "default_plan_mode", value),
    handleKimiMergeAllAvailableSkillsChange: (value) =>
      setTopLevel(
        setKimiMergeAllAvailableSkills,
        "merge_all_available_skills",
        value,
      ),
    handleKimiTelemetryChange: (value) =>
      setTopLevel(setKimiTelemetry, "telemetry", value),
    handleKimiThinkingModeChange: (value) => {
      setKimiThinkingMode(value);
      updateKimiConfig((settings) => {
        settings.config.thinking ??= {};
        if (value) settings.config.thinking.mode = value;
        else delete settings.config.thinking.mode;
      });
    },
    handleKimiThinkingEffortChange: (value) => {
      setKimiThinkingEffort(value);
      updateKimiConfig((settings) => {
        settings.config.thinking ??= {};
        if (value) settings.config.thinking.effort = value;
        else delete settings.config.thinking.effort;
      });
    },
    handleKimiMaxRetriesPerStepChange: (value) => {
      setKimiMaxRetriesPerStep(value);
      updateKimiConfig((settings) => {
        settings.config.loop_control ??= {};
        if (value === undefined) {
          delete settings.config.loop_control.max_retries_per_step;
        } else {
          settings.config.loop_control.max_retries_per_step = value;
        }
      });
    },
    handleKimiReservedContextSizeChange: (value) => {
      setKimiReservedContextSize(value);
      updateKimiConfig((settings) => {
        settings.config.loop_control ??= {};
        if (value === undefined) {
          delete settings.config.loop_control.reserved_context_size;
        } else {
          settings.config.loop_control.reserved_context_size = value;
        }
      });
    },
    handleKimiMaxRunningTasksChange: (value) => {
      setKimiMaxRunningTasks(value);
      updateKimiConfig((settings) => {
        settings.config.background ??= {};
        if (value === undefined) {
          delete settings.config.background.max_running_tasks;
        } else {
          settings.config.background.max_running_tasks = value;
        }
      });
    },
    handleKimiKeepAliveOnExitChange: (value) => {
      setKimiKeepAliveOnExit(value);
      updateKimiConfig((settings) => {
        settings.config.background ??= {};
        if (value === undefined) {
          delete settings.config.background.keep_alive_on_exit;
        } else {
          settings.config.background.keep_alive_on_exit = value;
        }
      });
    },
    handleKimiMicroCompactionChange: (value) => {
      setKimiMicroCompaction(value);
      updateKimiConfig((settings) => {
        settings.config.experimental ??= {};
        if (value === undefined) {
          delete settings.config.experimental.micro_compaction;
        } else {
          settings.config.experimental.micro_compaction = value;
        }
      });
    },
    resetKimiState,
  };
}

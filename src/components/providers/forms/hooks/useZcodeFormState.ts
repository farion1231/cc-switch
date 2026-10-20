import { useState, useCallback, useRef } from "react";
import type { ZCodeModel, ZCodeProviderConfig } from "@/types";
import {
  ZCODE_DEFAULT_KIND,
  ZCODE_DEFAULT_CONFIG,
  ZCODE_EXTRA_OPTION_DRAFT_PREFIX,
  extractZcodeConfigExtras,
  parseZcodeConfig,
  toZcodeExtraOptions,
} from "../helpers/zcodeFormUtils";

interface UseZcodeFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: string;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
}

export interface ZcodeFormState {
  zcodeProviderKey: string;
  setZcodeProviderKey: (key: string) => void;
  zcodeKind: string;
  zcodeName: string;
  zcodeApiKey: string;
  zcodeBaseUrl: string;
  zcodeModels: Record<string, ZCodeModel>;
  zcodeExtraOptions: Record<string, string>;
  /** Whether an API key was present in the initial config (used for validation). */
  zcodeApiKeyRequired: boolean;
  handleZcodeKindChange: (kind: string) => void;
  handleZcodeNameChange: (name: string) => void;
  handleZcodeApiKeyChange: (apiKey: string) => void;
  handleZcodeBaseUrlChange: (baseUrl: string) => void;
  handleZcodeModelsChange: (models: Record<string, ZCodeModel>) => void;
  handleZcodeExtraOptionsChange: (options: Record<string, string>) => void;
  resetZcodeState: (config?: ZCodeProviderConfig) => void;
  /** 外部改写 settingsConfig（JSON 编辑器/粘贴）后，把结构化字段重新对齐 */
  hydrateFromJson: (json: string) => void;
}

interface ZcodeFieldState {
  kind: string;
  name: string;
  apiKey: string;
  baseUrl: string;
  models: Record<string, ZCodeModel>;
  extraOptions: Record<string, string>;
  /** Preserved from the initial config so edits don't silently flip it. */
  enabled: boolean | null;
  /** Preserved from the initial config so edits don't overwrite the source. */
  source: string | undefined;
}

/**
 * Manages ZCode provider form state.
 *
 * Unlike the OpenCode hook, this rebuilds settingsConfig directly from local
 * state rather than reading it back via a getSettingsConfig() closure. This
 * avoids the stale-read footgun and keeps the source of truth in React state.
 * A ref mirrors the latest field state so emitConfig always sees current values.
 *
 * apiKeyRequired is derived from the initial config (whether a key field was
 * present), never hard-coded — fixes the PR#4975 regression where validation
 * forced a key even for custom/optional providers.
 */
export function useZcodeFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
}: UseZcodeFormStateParams): ZcodeFormState {
  const initialZcodeConfig =
    appId === "zcode" ? parseZcodeConfig(initialData?.settingsConfig) : null;
  const initialZcodeOptions = initialZcodeConfig?.options || {};

  // Top-level keys outside the structured whitelist (priority, tags, …) must
  // survive every structured edit, so capture them once and merge on emit.
  const initialExtras =
    appId === "zcode"
      ? extractZcodeConfigExtras(initialData?.settingsConfig)
      : {};

  const initialFields: ZcodeFieldState = {
    kind: initialZcodeConfig?.kind || ZCODE_DEFAULT_KIND,
    name: initialZcodeConfig?.name || "",
    apiKey:
      typeof initialZcodeOptions.apiKey === "string"
        ? initialZcodeOptions.apiKey
        : "",
    baseUrl:
      typeof initialZcodeOptions.baseURL === "string"
        ? initialZcodeOptions.baseURL
        : "",
    models: initialZcodeConfig?.models || {},
    extraOptions: toZcodeExtraOptions(initialZcodeOptions),
    // Preserve the original enabled/source so editing an unrelated field
    // doesn't silently re-enable a disabled provider or rewrite its source.
    enabled:
      initialZcodeConfig?.enabled === undefined
        ? null
        : initialZcodeConfig.enabled,
    source: initialZcodeConfig?.source,
  };

  const [zcodeProviderKey, setZcodeProviderKey] = useState<string>(
    appId === "zcode" ? providerId || "" : "",
  );
  const [fields, setFields] = useState<ZcodeFieldState>(
    appId === "zcode"
      ? initialFields
      : {
          kind: ZCODE_DEFAULT_KIND,
          name: "",
          apiKey: "",
          baseUrl: "",
          models: {},
          extraOptions: {},
          enabled: null,
          source: undefined,
        },
  );

  // apiKeyRequired: derived from initial config presence, NOT hard-coded.
  const [zcodeApiKeyRequired] = useState<boolean>(() => {
    if (appId !== "zcode") return false;
    // parseZcodeConfig(undefined) returns the default template whose options
    // carry apiKey: "", so the hasOwnProperty check would always pass;
    // a brand-new provider (no initial settingsConfig) requires no key.
    if (!initialData?.settingsConfig) return false;
    return Object.prototype.hasOwnProperty.call(initialZcodeOptions, "apiKey");
  });

  // Ref mirror so emitConfig always reads the latest values synchronously.
  const fieldsRef = useRef(fields);
  fieldsRef.current = fields;

  // 最近一次 emit 出去的配置串：用于识别 settingsConfig 的变化是「外部改动」
  // （用户在 JSON 编辑器里粘贴/修改）还是自己 emit 的回声，避免循环互相覆盖。
  const lastEmittedRef = useRef<string | null>(null);
  const extrasRef = useRef(initialExtras);

  /**
   * Rebuild the full ZCodeProviderConfig from the given (next) field state and
   * emit it. Caller passes the post-update fields so we never read stale state.
   */
  const emitConfig = useCallback(
    (next: ZcodeFieldState) => {
      const options: Record<string, unknown> = {
        baseURL: next.baseUrl.trim().replace(/\/+$/, ""),
        apiKey: next.apiKey,
      };
      for (const [k, v] of Object.entries(next.extraOptions)) {
        const trimmedKey = k.trim();
        if (trimmedKey && !k.startsWith(ZCODE_EXTRA_OPTION_DRAFT_PREFIX)) {
          try {
            options[trimmedKey] = JSON.parse(v);
          } catch {
            options[trimmedKey] = v;
          }
        }
      }

      // Extras (priority, tags, …) go first so the structured fields the form
      // owns always win; anything the user cannot edit here passes through.
      const config: ZCodeProviderConfig = {
        ...extrasRef.current,
        kind: next.kind,
        options: options as ZCodeProviderConfig["options"],
        models: next.models,
      };
      if (next.name) config.name = next.name;
      // Preserve the original enabled/source rather than forcing defaults on
      // every edit — otherwise saving an unrelated change could re-enable a
      // disabled provider or rewrite a non-custom source.
      // For newly created providers (enabled === null) we default to enabled.
      config.enabled = next.enabled === null ? true : next.enabled;
      config.source = next.source ?? "custom";

      const emitted = JSON.stringify(config, null, 2);
      lastEmittedRef.current = emitted;
      onSettingsConfigChange(emitted);
    },
    [onSettingsConfigChange],
  );

  /** 外部（JSON 编辑器/预设）改写了 settingsConfig：把结构化字段重新对齐，
   *  不触发 emit，避免和 JSON 视图互相覆盖。extras 保留原样。 */
  const hydrateFromJson = useCallback((json: string) => {
    if (lastEmittedRef.current === json) return; // 自己 emit 的回声
    lastEmittedRef.current = json;
    try {
      const parsed = JSON.parse(json) as ZCodeProviderConfig;
      const options = parsed.options || {};
      setFields((prev) => ({
        ...prev,
        kind: parsed.kind || ZCODE_DEFAULT_KIND,
        name: parsed.name || "",
        baseUrl:
          typeof options.baseURL === "string" ? options.baseURL : prev.baseUrl,
        apiKey: typeof options.apiKey === "string" ? options.apiKey : "",
        models: parsed.models || {},
        extraOptions: toZcodeExtraOptions(options),
        enabled: parsed.enabled === undefined ? null : parsed.enabled,
        source: parsed.source,
      }));
      extrasRef.current = extractZcodeConfigExtras(parsed);
    } catch {
      // JSON 还没写完整（用户输入中），保持现有字段不动
    }
  }, []);

  /** Apply a partial field update, emit, and return the merged state. */
  const updateFields = useCallback(
    (patch: Partial<ZcodeFieldState>) => {
      const next: ZcodeFieldState = { ...fieldsRef.current, ...patch };
      fieldsRef.current = next;
      setFields(next);
      emitConfig(next);
    },
    [emitConfig],
  );

  const handleZcodeKindChange = useCallback(
    (kind: string) => updateFields({ kind }),
    [updateFields],
  );
  const handleZcodeNameChange = useCallback(
    (name: string) => updateFields({ name }),
    [updateFields],
  );
  const handleZcodeApiKeyChange = useCallback(
    (apiKey: string) => updateFields({ apiKey }),
    [updateFields],
  );
  const handleZcodeBaseUrlChange = useCallback(
    (baseUrl: string) => updateFields({ baseUrl }),
    [updateFields],
  );
  const handleZcodeModelsChange = useCallback(
    (models: Record<string, ZCodeModel>) => updateFields({ models }),
    [updateFields],
  );
  const handleZcodeExtraOptionsChange = useCallback(
    (extraOptions: Record<string, string>) => updateFields({ extraOptions }),
    [updateFields],
  );

  const resetZcodeState = useCallback((config?: ZCodeProviderConfig) => {
    const next: ZcodeFieldState = {
      kind: config?.kind || ZCODE_DEFAULT_KIND,
      name: config?.name || "",
      baseUrl: config?.options?.baseURL || "",
      apiKey: config?.options?.apiKey || "",
      models: config?.models || {},
      extraOptions: toZcodeExtraOptions(config?.options || {}),
      enabled: config?.enabled === undefined ? null : config.enabled,
      source: config?.source,
    };
    // A reset installs a whole new config (e.g. a preset), so its extras —
    // if any — replace whatever the previous provider carried.
    extrasRef.current = extractZcodeConfigExtras(config);
    fieldsRef.current = next;
    setZcodeProviderKey("");
    setFields(next);
  }, []);

  return {
    zcodeProviderKey,
    setZcodeProviderKey,
    zcodeKind: fields.kind,
    zcodeName: fields.name,
    zcodeApiKey: fields.apiKey,
    zcodeBaseUrl: fields.baseUrl,
    zcodeModels: fields.models,
    zcodeExtraOptions: fields.extraOptions,
    zcodeApiKeyRequired,
    handleZcodeKindChange,
    handleZcodeNameChange,
    handleZcodeApiKeyChange,
    handleZcodeBaseUrlChange,
    handleZcodeModelsChange,
    handleZcodeExtraOptionsChange,
    resetZcodeState,
    hydrateFromJson,
  };
}

export { ZCODE_DEFAULT_CONFIG };

import { useState, useCallback, useRef } from "react";
import type { ZCodeModel, ZCodeProviderConfig } from "@/types";
import {
  ZCODE_DEFAULT_KIND,
  ZCODE_DEFAULT_CONFIG,
  ZCODE_EXTRA_OPTION_DRAFT_PREFIX,
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
}

interface ZcodeFieldState {
  kind: string;
  name: string;
  apiKey: string;
  baseUrl: string;
  models: Record<string, ZCodeModel>;
  extraOptions: Record<string, string>;
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
        },
  );

  // apiKeyRequired: derived from initial config presence, NOT hard-coded.
  const [zcodeApiKeyRequired] = useState<boolean>(() => {
    if (appId !== "zcode" || !initialZcodeConfig) return false;
    return Object.prototype.hasOwnProperty.call(initialZcodeOptions, "apiKey");
  });

  // Ref mirror so emitConfig always reads the latest values synchronously.
  const fieldsRef = useRef(fields);
  fieldsRef.current = fields;

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

      const config: ZCodeProviderConfig = {
        kind: next.kind,
        options: options as ZCodeProviderConfig["options"],
        models: next.models,
      };
      if (next.name) config.name = next.name;
      // Preserve sensible defaults for round-tripping with ZCode.
      config.enabled = true;
      config.source = "custom";

      onSettingsConfigChange(JSON.stringify(config, null, 2));
    },
    [onSettingsConfigChange],
  );

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
    };
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
  };
}

export { ZCODE_DEFAULT_CONFIG };

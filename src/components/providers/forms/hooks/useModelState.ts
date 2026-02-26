import { useState, useCallback, useEffect, useRef } from "react";

interface UseModelStateProps {
  settingsConfig: string;
  onConfigChange: (config: string) => void;
}

/**
 * Detect if unified mapping is enabled by checking if all model fields have the same value
 */
function detectUnifiedMapping(settingsConfig: string): {
  enabled: boolean;
  targetModel: string;
} {
  try {
    const cfg = settingsConfig ? JSON.parse(settingsConfig) : {};
    const env = cfg?.env || {};

    const model =
      typeof env.ANTHROPIC_MODEL === "string" ? env.ANTHROPIC_MODEL : "";
    const haiku =
      typeof env.ANTHROPIC_DEFAULT_HAIKU_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_HAIKU_MODEL
        : "";
    const sonnet =
      typeof env.ANTHROPIC_DEFAULT_SONNET_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_SONNET_MODEL
        : "";
    const opus =
      typeof env.ANTHROPIC_DEFAULT_OPUS_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_OPUS_MODEL
        : "";

    // If all four model fields have the same non-empty value, unified mapping is enabled
    if (model && model === haiku && model === sonnet && model === opus) {
      console.log(
        `[UnifiedMapping] Detected unified mapping enabled - target model: "${model}"`,
      );
      return { enabled: true, targetModel: model };
    }

    if (model || haiku || sonnet || opus) {
      console.log(
        `[UnifiedMapping] Detected mixed model config (not unified) - model: "${model}", haiku: "${haiku}", sonnet: "${sonnet}", opus: "${opus}"`,
      );
    }

    return { enabled: false, targetModel: model };
  } catch {
    return { enabled: false, targetModel: "" };
  }
}

/**
 * Parse model values from settings config JSON
 */
function parseModelsFromConfig(settingsConfig: string) {
  try {
    const cfg = settingsConfig ? JSON.parse(settingsConfig) : {};
    const env = cfg?.env || {};
    const model =
      typeof env.ANTHROPIC_MODEL === "string" ? env.ANTHROPIC_MODEL : "";
    const reasoning =
      typeof env.ANTHROPIC_REASONING_MODEL === "string"
        ? env.ANTHROPIC_REASONING_MODEL
        : "";
    const small =
      typeof env.ANTHROPIC_SMALL_FAST_MODEL === "string"
        ? env.ANTHROPIC_SMALL_FAST_MODEL
        : "";
    const haiku =
      typeof env.ANTHROPIC_DEFAULT_HAIKU_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_HAIKU_MODEL
        : small || model;
    const sonnet =
      typeof env.ANTHROPIC_DEFAULT_SONNET_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_SONNET_MODEL
        : model || small;
    const opus =
      typeof env.ANTHROPIC_DEFAULT_OPUS_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_OPUS_MODEL
        : model || small;

    return { model, reasoning, haiku, sonnet, opus };
  } catch {
    return { model: "", reasoning: "", haiku: "", sonnet: "", opus: "" };
  }
}

/**
 * 管理模型选择状态
 * 支持 ANTHROPIC_MODEL, ANTHROPIC_REASONING_MODEL 和各类型默认模型
 */
export function useModelState({
  settingsConfig,
  onConfigChange,
}: UseModelStateProps) {
  // Initialize state by parsing config directly (fixes edit mode backfill)
  const [claudeModel, setClaudeModel] = useState(
    () => parseModelsFromConfig(settingsConfig).model,
  );
  const [reasoningModel, setReasoningModel] = useState(
    () => parseModelsFromConfig(settingsConfig).reasoning,
  );
  const [defaultHaikuModel, setDefaultHaikuModel] = useState(
    () => parseModelsFromConfig(settingsConfig).haiku,
  );
  const [defaultSonnetModel, setDefaultSonnetModel] = useState(
    () => parseModelsFromConfig(settingsConfig).sonnet,
  );
  const [defaultOpusModel, setDefaultOpusModel] = useState(
    () => parseModelsFromConfig(settingsConfig).opus,
  );

  // Unified mapping state
  const [unifiedMappingEnabled, setUnifiedMappingEnabled] = useState(() =>
    detectUnifiedMapping(settingsConfig).enabled,
  );
  const [unifiedTargetModel, setUnifiedTargetModel] = useState(() =>
    detectUnifiedMapping(settingsConfig).targetModel,
  );

  const isUserEditingRef = useRef(false);
  const lastConfigRef = useRef(settingsConfig);

  // 初始化读取：读新键；若缺失，按兼容优先级回退
  // Haiku: DEFAULT_HAIKU || SMALL_FAST || MODEL
  // Sonnet: DEFAULT_SONNET || MODEL || SMALL_FAST
  // Opus: DEFAULT_OPUS || MODEL || SMALL_FAST
  // 仅在 settingsConfig 变化时同步一次（表单加载/切换预设时）
  useEffect(() => {
    if (lastConfigRef.current === settingsConfig) {
      return;
    }

    if (isUserEditingRef.current) {
      isUserEditingRef.current = false;
      lastConfigRef.current = settingsConfig;
      return;
    }

    lastConfigRef.current = settingsConfig;

    try {
      const cfg = settingsConfig ? JSON.parse(settingsConfig) : {};
      const env = cfg?.env || {};
      const model =
        typeof env.ANTHROPIC_MODEL === "string" ? env.ANTHROPIC_MODEL : "";
      const reasoning =
        typeof env.ANTHROPIC_REASONING_MODEL === "string"
          ? env.ANTHROPIC_REASONING_MODEL
          : "";
      const small =
        typeof env.ANTHROPIC_SMALL_FAST_MODEL === "string"
          ? env.ANTHROPIC_SMALL_FAST_MODEL
          : "";
      const haiku =
        typeof env.ANTHROPIC_DEFAULT_HAIKU_MODEL === "string"
          ? env.ANTHROPIC_DEFAULT_HAIKU_MODEL
          : small || model;
      const sonnet =
        typeof env.ANTHROPIC_DEFAULT_SONNET_MODEL === "string"
          ? env.ANTHROPIC_DEFAULT_SONNET_MODEL
          : model || small;
      const opus =
        typeof env.ANTHROPIC_DEFAULT_OPUS_MODEL === "string"
          ? env.ANTHROPIC_DEFAULT_OPUS_MODEL
          : model || small;

      setClaudeModel(model || "");
      setReasoningModel(reasoning || "");
      setDefaultHaikuModel(haiku || "");
      setDefaultSonnetModel(sonnet || "");
      setDefaultOpusModel(opus || "");

      // Sync unified mapping state
      const unified = detectUnifiedMapping(settingsConfig);
      setUnifiedMappingEnabled(unified.enabled);
      setUnifiedTargetModel(unified.targetModel);
    } catch {
      // ignore
    }
  }, [settingsConfig]);

  const handleModelChange = useCallback(
    (
      field:
        | "ANTHROPIC_MODEL"
        | "ANTHROPIC_REASONING_MODEL"
        | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
        | "ANTHROPIC_DEFAULT_SONNET_MODEL"
        | "ANTHROPIC_DEFAULT_OPUS_MODEL",
      value: string,
    ) => {
      isUserEditingRef.current = true;

      if (field === "ANTHROPIC_MODEL") setClaudeModel(value);
      if (field === "ANTHROPIC_REASONING_MODEL") setReasoningModel(value);
      if (field === "ANTHROPIC_DEFAULT_HAIKU_MODEL")
        setDefaultHaikuModel(value);
      if (field === "ANTHROPIC_DEFAULT_SONNET_MODEL")
        setDefaultSonnetModel(value);
      if (field === "ANTHROPIC_DEFAULT_OPUS_MODEL") setDefaultOpusModel(value);

      try {
        const currentConfig = settingsConfig
          ? JSON.parse(settingsConfig)
          : { env: {} };
        if (!currentConfig.env) currentConfig.env = {};

        // 新键仅写入；旧键不再写入
        const trimmed = value.trim();
        if (trimmed) {
          currentConfig.env[field] = trimmed;
        } else {
          delete currentConfig.env[field];
        }
        // 删除旧键
        delete currentConfig.env["ANTHROPIC_SMALL_FAST_MODEL"];

        onConfigChange(JSON.stringify(currentConfig, null, 2));
      } catch (err) {
        console.error("Failed to update model config:", err);
      }
    },
    [settingsConfig, onConfigChange],
  );

  // Handle unified mapping toggle
  const handleUnifiedMappingToggle = useCallback(
    (enabled: boolean) => {
      isUserEditingRef.current = true;
      setUnifiedMappingEnabled(enabled);

      const targetModel = unifiedTargetModel.trim();

      console.log(
        `[UnifiedMapping] User ${enabled ? "enabled" : "disabled"} unified mapping, target model: "${targetModel}"`,
      );

      try {
        const currentConfig = settingsConfig
          ? JSON.parse(settingsConfig)
          : { env: {} };
        if (!currentConfig.env) currentConfig.env = {};

        if (enabled && targetModel) {
          // Enable unified mapping: set all model fields to the target model
          currentConfig.env.ANTHROPIC_MODEL = targetModel;
          currentConfig.env.ANTHROPIC_DEFAULT_HAIKU_MODEL = targetModel;
          currentConfig.env.ANTHROPIC_DEFAULT_SONNET_MODEL = targetModel;
          currentConfig.env.ANTHROPIC_DEFAULT_OPUS_MODEL = targetModel;
          currentConfig.env.ANTHROPIC_REASONING_MODEL = targetModel;

          console.log(
            `[UnifiedMapping] Applied unified mapping - setting all model fields to: "${targetModel}"`,
          );

          // Update local states
          setClaudeModel(targetModel);
          setReasoningModel(targetModel);
          setDefaultHaikuModel(targetModel);
          setDefaultSonnetModel(targetModel);
          setDefaultOpusModel(targetModel);
        } else {
          console.log(
            `[UnifiedMapping] Disabled - preserving existing config (not clearing env keys)`,
          );
        }
        // When disabling, we don't delete existing env keys to preserve user config

        onConfigChange(JSON.stringify(currentConfig, null, 2));
      } catch (err) {
        console.error("Failed to update unified mapping config:", err);
      }
    },
    [settingsConfig, onConfigChange, unifiedTargetModel],
  );

  // Handle unified target model change
  const handleUnifiedTargetModelChange = useCallback(
    (model: string) => {
      isUserEditingRef.current = true;
      setUnifiedTargetModel(model);

      const targetModel = model.trim();

      console.log(
        `[UnifiedMapping] User changed target model to: "${targetModel}", unified mapping enabled: ${unifiedMappingEnabled}`,
      );

      // If unified mapping is enabled, sync all model fields
      if (unifiedMappingEnabled && targetModel) {
        console.log(
          `[UnifiedMapping] Syncing all model fields to: "${targetModel}"`,
        );

        try {
          const currentConfig = settingsConfig
            ? JSON.parse(settingsConfig)
            : { env: {} };
          if (!currentConfig.env) currentConfig.env = {};

          currentConfig.env.ANTHROPIC_MODEL = targetModel;
          currentConfig.env.ANTHROPIC_DEFAULT_HAIKU_MODEL = targetModel;
          currentConfig.env.ANTHROPIC_DEFAULT_SONNET_MODEL = targetModel;
          currentConfig.env.ANTHROPIC_DEFAULT_OPUS_MODEL = targetModel;
          currentConfig.env.ANTHROPIC_REASONING_MODEL = targetModel;

          setClaudeModel(targetModel);
          setReasoningModel(targetModel);
          setDefaultHaikuModel(targetModel);
          setDefaultSonnetModel(targetModel);
          setDefaultOpusModel(targetModel);

          onConfigChange(JSON.stringify(currentConfig, null, 2));

          console.log(
            `[UnifiedMapping] Successfully applied mapping - ANTHROPIC_MODEL: "${targetModel}"`,
          );
        } catch (err) {
          console.error("Failed to update unified target model:", err);
        }
      }
    },
    [settingsConfig, onConfigChange, unifiedMappingEnabled],
  );

  return {
    claudeModel,
    setClaudeModel,
    reasoningModel,
    setReasoningModel,
    defaultHaikuModel,
    setDefaultHaikuModel,
    defaultSonnetModel,
    setDefaultSonnetModel,
    defaultOpusModel,
    setDefaultOpusModel,
    handleModelChange,
    unifiedMappingEnabled,
    unifiedTargetModel,
    handleUnifiedMappingToggle,
    handleUnifiedTargetModelChange,
  };
}

import { useState, useCallback, useEffect } from "react";

interface UseModelStateProps {
  settingsConfig: string;
  onConfigChange: (config: string) => void;
}

/**
 * 管理模型选择状态
 * 支持 ANTHROPIC_MODEL, ANTHROPIC_REASONING_MODEL 和各类型默认模型
 */
export function useModelState({
  settingsConfig,
  onConfigChange,
}: UseModelStateProps) {
  const [claudeModel, setClaudeModel] = useState("");
  const [reasoningModel, setReasoningModel] = useState("");
  const [defaultHaikuModel, setDefaultHaikuModel] = useState("");
  const [defaultSonnetModel, setDefaultSonnetModel] = useState("");
  const [defaultOpusModel, setDefaultOpusModel] = useState("");

  // 初始化读取：读新键；若缺失，按兼容优先级回退
  // Haiku: DEFAULT_HAIKU || SMALL_FAST || MODEL
  // Sonnet: DEFAULT_SONNET || MODEL || SMALL_FAST
  // Opus: DEFAULT_OPUS || MODEL || SMALL_FAST
  // 仅在 settingsConfig 变化时同步一次（表单加载/切换预设时）
  useEffect(() => {
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
  };
}

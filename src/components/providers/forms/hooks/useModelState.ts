import { useState, useCallback, useEffect, useRef } from "react";

interface UseModelStateProps {
  settingsConfig: string;
  onConfigChange: (config: string) => void;
  /**
   * 现读当前配置。latestConfigRef 只在渲染期从 settingsConfig 快照赋值，可能
   * 早于别处（如 JSON 编辑器）的写入。改模型时若基于它重建整份 JSON 会把那些
   * 改动覆盖回去。
   */
  getSettingsConfig?: () => string;
}

export type ClaudeModelEnvField =
  | "ANTHROPIC_MODEL"
  | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
  | "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME"
  | "ANTHROPIC_DEFAULT_SONNET_MODEL"
  | "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"
  | "ANTHROPIC_DEFAULT_OPUS_MODEL"
  | "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"
  | "ANTHROPIC_DEFAULT_FABLE_MODEL"
  | "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME"
  | "CLAUDE_CODE_SUBAGENT_MODEL";

export const CLAUDE_ONE_M_MARKER = "[1M]";

export function hasClaudeOneMMarker(model: string): boolean {
  return model.trimEnd().toLowerCase().endsWith("[1m]");
}

export function stripClaudeOneMMarker(model: string): string {
  const trimmedEnd = model.trimEnd();
  if (!trimmedEnd.toLowerCase().endsWith("[1m]")) return model;
  return trimmedEnd.slice(0, -CLAUDE_ONE_M_MARKER.length).trimEnd();
}

export function setClaudeOneMMarker(model: string, enabled: boolean): string {
  const base = stripClaudeOneMMarker(model).trim();
  if (!base) return "";
  return enabled ? `${base}${CLAUDE_ONE_M_MARKER}` : base;
}

/**
 * Parse model values from settings config JSON
 *
 * 各档只读取 env 里真实存在的键：键缺失时返回空串，由输入框 placeholder 提示
 * 运行时回落目标（fable → opus，其余档 → 默认兜底模型，见 proxy/model_mapper）。
 * 不再把回落值填进受控 input——那会造成「已保存」的假象（settings_config 里
 * 并没有该键），保存后看起来又像被改写（issue #7456）。
 */
function parseModelsFromConfig(settingsConfig: string) {
  const empty = {
    model: "",
    haiku: "",
    haikuName: "",
    sonnet: "",
    sonnetName: "",
    opus: "",
    opusName: "",
    fable: "",
    fableName: "",
    subagent: "",
  };
  try {
    const cfg = settingsConfig ? JSON.parse(settingsConfig) : {};
    const env = cfg?.env || {};
    const read = (key: string) =>
      typeof env[key] === "string" ? (env[key] as string) : "";

    return {
      model: read("ANTHROPIC_MODEL"),
      haiku: read("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
      haikuName: read("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME"),
      sonnet: read("ANTHROPIC_DEFAULT_SONNET_MODEL"),
      sonnetName: read("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"),
      opus: read("ANTHROPIC_DEFAULT_OPUS_MODEL"),
      opusName: read("ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"),
      fable: read("ANTHROPIC_DEFAULT_FABLE_MODEL"),
      fableName: read("ANTHROPIC_DEFAULT_FABLE_MODEL_NAME"),
      subagent: read("CLAUDE_CODE_SUBAGENT_MODEL"),
    };
  } catch {
    return empty;
  }
}

/**
 * 管理模型选择状态
 * 支持 ANTHROPIC_MODEL 和各类型默认模型
 */
export function useModelState({
  settingsConfig,
  onConfigChange,
  getSettingsConfig,
}: UseModelStateProps) {
  const initial = useState(() => parseModelsFromConfig(settingsConfig))[0];
  const [claudeModel, setClaudeModel] = useState(initial.model);
  const [defaultHaikuModel, setDefaultHaikuModel] = useState(initial.haiku);
  const [defaultHaikuModelName, setDefaultHaikuModelName] = useState(
    initial.haikuName,
  );
  const [defaultSonnetModel, setDefaultSonnetModel] = useState(initial.sonnet);
  const [defaultSonnetModelName, setDefaultSonnetModelName] = useState(
    initial.sonnetName,
  );
  const [defaultOpusModel, setDefaultOpusModel] = useState(initial.opus);
  const [defaultOpusModelName, setDefaultOpusModelName] = useState(
    initial.opusName,
  );
  const [defaultFableModel, setDefaultFableModel] = useState(initial.fable);
  const [defaultFableModelName, setDefaultFableModelName] = useState(
    initial.fableName,
  );
  const [subagentModel, setSubagentModel] = useState(initial.subagent);

  const isUserEditingRef = useRef(false);
  const lastConfigRef = useRef(settingsConfig);
  const latestConfigRef = useRef(settingsConfig);

  latestConfigRef.current = settingsConfig;

  // 仅在 settingsConfig 外部变化时同步（表单加载 / 切换预设）；
  // 用户正在编辑时 (isUserEditingRef) 跳过一次以避免回填覆盖。
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

    const parsed = parseModelsFromConfig(settingsConfig);
    setClaudeModel(parsed.model);
    setDefaultHaikuModel(parsed.haiku);
    setDefaultHaikuModelName(parsed.haikuName);
    setDefaultSonnetModel(parsed.sonnet);
    setDefaultSonnetModelName(parsed.sonnetName);
    setDefaultOpusModel(parsed.opus);
    setDefaultOpusModelName(parsed.opusName);
    setDefaultFableModel(parsed.fable);
    setDefaultFableModelName(parsed.fableName);
    setSubagentModel(parsed.subagent);
  }, [settingsConfig]);

  const handleModelChange = useCallback(
    (field: ClaudeModelEnvField, value: string) => {
      isUserEditingRef.current = true;

      if (field === "ANTHROPIC_MODEL") setClaudeModel(value);
      if (field === "ANTHROPIC_DEFAULT_HAIKU_MODEL")
        setDefaultHaikuModel(value);
      if (field === "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME")
        setDefaultHaikuModelName(value);
      if (field === "ANTHROPIC_DEFAULT_SONNET_MODEL")
        setDefaultSonnetModel(value);
      if (field === "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")
        setDefaultSonnetModelName(value);
      if (field === "ANTHROPIC_DEFAULT_OPUS_MODEL") setDefaultOpusModel(value);
      if (field === "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME")
        setDefaultOpusModelName(value);
      if (field === "ANTHROPIC_DEFAULT_FABLE_MODEL")
        setDefaultFableModel(value);
      if (field === "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME")
        setDefaultFableModelName(value);
      if (field === "CLAUDE_CODE_SUBAGENT_MODEL") setSubagentModel(value);

      try {
        const currentRaw = getSettingsConfig?.() ?? latestConfigRef.current;
        const currentConfig = currentRaw ? JSON.parse(currentRaw) : { env: {} };
        if (!currentConfig.env) currentConfig.env = {};
        const env = currentConfig.env as Record<string, unknown>;

        // 新键仅写入；旧键不再写入
        const trimmed = value.trim();
        if (trimmed) {
          env[field] = trimmed;
        } else {
          delete env[field];
        }
        // 删除旧键
        delete env["ANTHROPIC_SMALL_FAST_MODEL"];

        const updatedConfig = JSON.stringify(currentConfig, null, 2);
        latestConfigRef.current = updatedConfig;
        onConfigChange(updatedConfig);
      } catch (err) {
        console.error("Failed to update model config:", err);
      }
    },
    [getSettingsConfig, onConfigChange],
  );

  return {
    claudeModel,
    setClaudeModel,
    defaultHaikuModel,
    setDefaultHaikuModel,
    defaultHaikuModelName,
    setDefaultHaikuModelName,
    defaultSonnetModel,
    setDefaultSonnetModel,
    defaultSonnetModelName,
    setDefaultSonnetModelName,
    defaultOpusModel,
    setDefaultOpusModel,
    defaultOpusModelName,
    setDefaultOpusModelName,
    defaultFableModel,
    setDefaultFableModel,
    defaultFableModelName,
    setDefaultFableModelName,
    subagentModel,
    setSubagentModel,
    handleModelChange,
  };
}

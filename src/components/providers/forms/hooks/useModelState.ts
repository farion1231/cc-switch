import { useState, useCallback, useEffect, useRef } from "react";

interface UseModelStateProps {
  settingsConfig: string;
  onConfigChange: (config: string) => void;
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

// ---- 通用后缀解析器（泛化 [1M] 布尔标记为任意粒度窗口后缀）----

export interface ModelSuffixResult {
  slug: string;
  window?: number;
}

function parseWindowToken(token: string): number | undefined {
  const trimmed = token.trim();
  if (!trimmed) return undefined;
  // 清洗括号、逗号、下划线、空格等装饰字符
  const cleaned = trimmed.replace(/[[\]()_,\s]/g, "");
  if (!cleaned) return undefined;

  // 提取末尾单位（K/k/M/m），去掉后得到数字部分
  const last = cleaned[cleaned.length - 1];
  let numPart: string;
  let multiplier: number;
  if (last === "K" || last === "k") {
    numPart = cleaned.slice(0, -1);
    multiplier = 1000;
  } else if (last === "M" || last === "m") {
    numPart = cleaned.slice(0, -1);
    multiplier = 1000000;
  } else if (last >= "0" && last <= "9") {
    // 纯数字
    numPart = cleaned;
    multiplier = 1;
  } else {
    // 未知单位（如 G）→ 不合法
    return undefined;
  }

  // 支持小数（如 1.5M → 1.5 × 1000000 = 1500000）
  const num = numPart.includes(".")
    ? Number.parseFloat(numPart)
    : Number.parseInt(numPart, 10);
  if (!Number.isFinite(num) || num <= 0) return undefined;
  return Math.round(num * multiplier);
}

export function parseModelSuffix(model: string): ModelSuffixResult {
  const trimmed = model.trim();
  const close = trimmed.lastIndexOf("]");
  if (close !== trimmed.length - 1) {
    return { slug: model, window: undefined };
  }
  const open = trimmed.lastIndexOf("[", close);
  if (open <= 0) return { slug: model, window: undefined };
  const slug = trimmed.slice(0, open).trim();
  if (!slug) return { slug: model, window: undefined };
  const window = parseWindowToken(trimmed.slice(open + 1, close));
  if (window === undefined) return { slug: model, window: undefined };
  return { slug, window };
}

export function stripModelSuffix(model: string): string {
  return parseModelSuffix(model).slug;
}

export function setModelSuffix(model: string, windowStr: string): string {
  const base = stripModelSuffix(model).trim();
  if (!base) return "";
  const trimmed = windowStr.trim();
  if (!trimmed) return base;
  const cleaned = trimmed.replace(/[[\]()_,\s]/g, "");
  if (!cleaned) return base;
  const window = parseWindowToken(cleaned);
  if (window === undefined) return base;
  // 小数输入时输出计算结果（如 1.5M → [1500000]），否则保留清洗后的原始格式
  const suffix = cleaned.includes(".") ? String(window) : cleaned.toLowerCase();
  return `${base}[${suffix}]`;
}

/**
 * 改模型名时保留原 model 的 context window 后缀。
 * 例如原 model 是 "deepseek[200k]"，用户改成 "glm-5.2"，
 * 返回 "glm-5.2[200k]"，避免改模型名丢窗口配置。
 * 若新输入本身带后缀，以原 model 的后缀为准（用户改的是名字不是窗口）。
 */
export function reapplySuffix(oldModel: string, newInput: string): string {
  const suffixResult = parseModelSuffix(oldModel);
  const oldSuffix = suffixResult.window
    ? oldModel.slice(oldModel.lastIndexOf("["))
    : "";
  const newBase = stripModelSuffix(newInput).trim();
  if (!newBase) return "";
  return oldSuffix ? `${newBase}${oldSuffix}` : newBase;
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
    const small =
      typeof env.ANTHROPIC_SMALL_FAST_MODEL === "string"
        ? env.ANTHROPIC_SMALL_FAST_MODEL
        : "";
    const haiku =
      typeof env.ANTHROPIC_DEFAULT_HAIKU_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_HAIKU_MODEL
        : small || model;
    const haikuName =
      typeof env.ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME === "string"
        ? env.ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME
        : stripClaudeOneMMarker(haiku);
    const sonnet =
      typeof env.ANTHROPIC_DEFAULT_SONNET_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_SONNET_MODEL
        : model || small;
    const sonnetName =
      typeof env.ANTHROPIC_DEFAULT_SONNET_MODEL_NAME === "string"
        ? env.ANTHROPIC_DEFAULT_SONNET_MODEL_NAME
        : stripClaudeOneMMarker(sonnet);
    const opus =
      typeof env.ANTHROPIC_DEFAULT_OPUS_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_OPUS_MODEL
        : model || small;
    const opusName =
      typeof env.ANTHROPIC_DEFAULT_OPUS_MODEL_NAME === "string"
        ? env.ANTHROPIC_DEFAULT_OPUS_MODEL_NAME
        : stripClaudeOneMMarker(opus);
    // 回填链镜像运行时映射链（fable → opus → default），保证 UI 展示
    // 与代理实际转发的模型一致。
    const fable =
      typeof env.ANTHROPIC_DEFAULT_FABLE_MODEL === "string"
        ? env.ANTHROPIC_DEFAULT_FABLE_MODEL
        : opus;
    const fableName =
      typeof env.ANTHROPIC_DEFAULT_FABLE_MODEL_NAME === "string"
        ? env.ANTHROPIC_DEFAULT_FABLE_MODEL_NAME
        : stripClaudeOneMMarker(fable);
    const subagent =
      typeof env.CLAUDE_CODE_SUBAGENT_MODEL === "string"
        ? env.CLAUDE_CODE_SUBAGENT_MODEL
        : "";

    return {
      model,
      haiku,
      haikuName,
      sonnet,
      sonnetName,
      opus,
      opusName,
      fable,
      fableName,
      subagent,
    };
  } catch {
    return {
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
  }
}

/**
 * 管理模型选择状态
 * 支持 ANTHROPIC_MODEL 和各类型默认模型
 */
export function useModelState({
  settingsConfig,
  onConfigChange,
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
        const currentConfig = latestConfigRef.current
          ? JSON.parse(latestConfigRef.current)
          : { env: {} };
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
    [onConfigChange],
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

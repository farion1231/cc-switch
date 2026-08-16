import { useState, useCallback, useEffect, useRef } from "react";
import { isPlainObject } from "@/utils/providerConfigUtils";

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

const MODEL_ENV_FIELDS: ClaudeModelEnvField[] = [
  "ANTHROPIC_MODEL",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  "ANTHROPIC_DEFAULT_SONNET_MODEL",
  "ANTHROPIC_DEFAULT_OPUS_MODEL",
  "ANTHROPIC_DEFAULT_FABLE_MODEL",
  "CLAUDE_CODE_SUBAGENT_MODEL",
];

export const CLAUDE_ONE_M_MARKER = "[1M]";

export function stripClaudeOneMMarker(model: string): string {
  const trimmedEnd = model.trimEnd();
  if (!trimmedEnd.toLowerCase().endsWith("[1m]")) return model;
  return trimmedEnd.slice(0, -CLAUDE_ONE_M_MARKER.length).trimEnd();
}

// ---- 通用后缀解析器（泛化 [1M] 布尔标记为任意粒度窗口后缀）----

export interface ModelSuffixResult {
  slug: string;
  window?: number;
}

export function parseWindowToken(token: string): number | undefined {
  const trimmed = token.trim();
  if (!trimmed) return undefined;
  // 与 Rust 端一致：全串校验，不接受小数、分隔符或未知单位
  const match = /^(\d+)([KkMm])?$/.exec(trimmed);
  if (!match) return undefined;
  const num = Number(match[1]);
  if (!Number.isFinite(num) || num <= 0) return undefined;
  const multiplier = match[2]
    ? match[2].toLowerCase() === "k"
      ? 1000
      : 1000000
    : 1;
  return num * multiplier;
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
  const inner = trimmed.slice(open + 1, close);
  if (/\s/.test(inner)) return { slug: model, window: undefined };
  const window = parseWindowToken(inner);
  if (window === undefined) return { slug: model, window: undefined };
  return { slug, window };
}

export function stripModelSuffix(model: string): string {
  return parseModelSuffix(model).slug;
}

export function readContextWindows(
  settingsConfig: string,
): Record<string, number> {
  try {
    const cfg = JSON.parse(settingsConfig || "{}") as unknown;
    if (!isPlainObject(cfg)) return {};
    const contextWindows = cfg.contextWindows;
    if (!isPlainObject(contextWindows)) return {};
    const result: Record<string, number> = {};
    for (const [key, value] of Object.entries(contextWindows)) {
      if (typeof value === "number" && Number.isFinite(value) && value > 0) {
        result[key] = value;
      }
    }
    return result;
  } catch {
    return {};
  }
}

export function writeContextWindow(
  config: string,
  roleEnvKey: string,
  value: number | null,
  token?: string | null,
): string {
  let parsed: Record<string, any>;
  try {
    const candidate = JSON.parse(config || "{}") as unknown;
    if (!isPlainObject(candidate)) return config;
    parsed = candidate;
  } catch {
    return config;
  }

  const env = isPlainObject(parsed.env) ? parsed.env : undefined;
  if (env) {
    const current = env[roleEnvKey];
    if (typeof current === "string") {
      env[roleEnvKey] = stripModelSuffix(current);
    } else {
      delete env[roleEnvKey];
    }
  } else if (parsed.env !== undefined) {
    parsed.env = {};
  }

  const contextWindows = isPlainObject(parsed.contextWindows)
    ? parsed.contextWindows
    : {};
  parsed.contextWindows = contextWindows;
  // 原样输入串（200k / 2m）作为 display-only 内部字段随配置持久化，
  // 重开编辑页时优先回显原样串；数字解析值仍由 contextWindows 承载。
  const contextWindowTokens = isPlainObject(parsed.contextWindowTokens)
    ? parsed.contextWindowTokens
    : {};
  if (value === null || value <= 0) {
    delete contextWindows[roleEnvKey];
    delete contextWindowTokens[roleEnvKey];
  } else {
    contextWindows[roleEnvKey] = value;
    if (token && token.trim()) {
      contextWindowTokens[roleEnvKey] = token.trim();
    } else {
      delete contextWindowTokens[roleEnvKey];
    }
  }
  if (Object.keys(contextWindowTokens).length > 0) {
    parsed.contextWindowTokens = contextWindowTokens;
  } else {
    delete parsed.contextWindowTokens;
  }
  return JSON.stringify(parsed, null, 2);
}

/**
 * 读取 display-only 的原样输入串（如 "200k"）；仅作输入框回显，
 * 不参与任何窗口计算（解析值以 contextWindows 数字为准）。
 */
export function readContextWindowTokens(
  settingsConfig: string,
): Record<string, string> {
  try {
    const cfg = JSON.parse(settingsConfig || "{}") as unknown;
    if (!isPlainObject(cfg)) return {};
    const tokens = cfg.contextWindowTokens;
    if (!isPlainObject(tokens)) return {};
    const result: Record<string, string> = {};
    for (const [key, value] of Object.entries(tokens)) {
      if (typeof value === "string" && value.trim().length > 0) {
        result[key] = value;
      }
    }
    return result;
  } catch {
    return {};
  }
}

/**
 * 保存路径统一迁移：把 env 模型名中的合法旧后缀搬进 contextWindows，
 * 模型名恢复干净。spec 1.1 要求迁移持久化写回 DB，这里在表单保存时执行。
 * 返回原字符串当且仅当没有任何迁移发生（便于调用方判断）。
 */
export function migrateLegacyModelSuffixes(config: string): string {
  let parsed: Record<string, any>;
  try {
    const candidate = JSON.parse(config || "{}") as unknown;
    if (!isPlainObject(candidate)) return config;
    parsed = candidate;
  } catch {
    return config;
  }

  const env = isPlainObject(parsed.env) ? parsed.env : undefined;
  if (!env) return config;
  if (
    parsed.contextWindows !== undefined &&
    !isPlainObject(parsed.contextWindows)
  ) {
    // 与 Rust migrate_legacy_suffix_to_context_windows 一致：形状非法时不迁移
    return config;
  }

  const contextWindows = isPlainObject(parsed.contextWindows)
    ? parsed.contextWindows
    : {};
  let changed = false;
  for (const field of MODEL_ENV_FIELDS) {
    const current = env[field];
    if (typeof current !== "string") continue;
    const parsedSuffix = parseModelSuffix(current);
    if (parsedSuffix.window === undefined) continue;
    // 与 Rust migrate_legacy_suffix_to_context_windows 对齐：始终剥离 env 后缀，
    // 但 contextWindows 只在键缺失时填充，已存在的用户配置不被后缀覆盖。
    env[field] = parsedSuffix.slug;
    if (!(field in contextWindows)) {
      contextWindows[field] = parsedSuffix.window;
    }
    changed = true;
  }
  if (!changed) return config;
  parsed.contextWindows = contextWindows;
  return JSON.stringify(parsed, null, 2);
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

      // 前端不再硬剥模型后缀（允许 deepseek[200k] 原样输入），
      // 剥离与迁移由保存路径 migrateLegacyModelSuffixes 完成。
      const nextValue = value.trim();

      if (field === "ANTHROPIC_MODEL") setClaudeModel(nextValue);
      if (field === "ANTHROPIC_DEFAULT_HAIKU_MODEL")
        setDefaultHaikuModel(nextValue);
      if (field === "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME")
        setDefaultHaikuModelName(nextValue);
      if (field === "ANTHROPIC_DEFAULT_SONNET_MODEL")
        setDefaultSonnetModel(nextValue);
      if (field === "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")
        setDefaultSonnetModelName(nextValue);
      if (field === "ANTHROPIC_DEFAULT_OPUS_MODEL")
        setDefaultOpusModel(nextValue);
      if (field === "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME")
        setDefaultOpusModelName(nextValue);
      if (field === "ANTHROPIC_DEFAULT_FABLE_MODEL")
        setDefaultFableModel(nextValue);
      if (field === "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME")
        setDefaultFableModelName(nextValue);
      if (field === "CLAUDE_CODE_SUBAGENT_MODEL") setSubagentModel(nextValue);

      try {
        let currentConfig = latestConfigRef.current
          ? JSON.parse(latestConfigRef.current)
          : { env: {} };
        if (!currentConfig.env) currentConfig.env = {};
        if (MODEL_ENV_FIELDS.includes(field) && nextValue) {
          const oldModel =
            typeof currentConfig.env[field] === "string"
              ? currentConfig.env[field]
              : "";
          const window =
            parseModelSuffix(oldModel).window ?? parseModelSuffix(value).window;
          if (window !== undefined) {
            // 对象直读直写：省去 writeContextWindow 的 parse/stringify 往返。
            const contextWindows = isPlainObject(currentConfig.contextWindows)
              ? currentConfig.contextWindows
              : {};
            const existing = contextWindows[field];
            const hasValidWindow =
              typeof existing === "number" &&
              Number.isFinite(existing) &&
              existing > 0;
            // 只迁移缺失键，避免旧模型后缀覆盖用户已显式配置的 contextWindows
            if (!hasValidWindow) {
              currentConfig.contextWindows = contextWindows;
              if (typeof currentConfig.env[field] === "string") {
                currentConfig.env[field] = stripModelSuffix(
                  currentConfig.env[field] as string,
                );
              }
              contextWindows[field] = window;
            }
          }
        }
        const env = currentConfig.env as Record<string, unknown>;

        // 新键仅写入；旧键不再写入
        const trimmed = nextValue;
        if (trimmed) {
          env[field] = trimmed;
        } else {
          delete env[field];
          if (isPlainObject(currentConfig.contextWindows)) {
            delete currentConfig.contextWindows[field];
          }
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

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

/** 字段 → 对应的 setState 映射，供批量更新使用 */
interface FieldSetter {
  set: (value: string) => void;
  value: string;
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
    // 仅在值真正变化时 setState，避免无变化时的冗余重渲染
    setClaudeModel((prev: string) => (prev !== parsed.model ? parsed.model : prev));
    setDefaultHaikuModel((prev: string) =>
      prev !== parsed.haiku ? parsed.haiku : prev,
    );
    setDefaultHaikuModelName((prev: string) =>
      prev !== parsed.haikuName ? parsed.haikuName : prev,
    );
    setDefaultSonnetModel((prev: string) =>
      prev !== parsed.sonnet ? parsed.sonnet : prev,
    );
    setDefaultSonnetModelName((prev: string) =>
      prev !== parsed.sonnetName ? parsed.sonnetName : prev,
    );
    setDefaultOpusModel((prev: string) =>
      prev !== parsed.opus ? parsed.opus : prev,
    );
    setDefaultOpusModelName((prev: string) =>
      prev !== parsed.opusName ? parsed.opusName : prev,
    );
    setDefaultFableModel((prev: string) =>
      prev !== parsed.fable ? parsed.fable : prev,
    );
    setDefaultFableModelName((prev: string) =>
      prev !== parsed.fableName ? parsed.fableName : prev,
    );
    setSubagentModel((prev: string) =>
      prev !== parsed.subagent ? parsed.subagent : prev,
    );
  }, [settingsConfig]);

  /** 将字段映射到对应的 setState，用于 handleModelChange 和 handleBatchModelChange */
  const getFieldSetterMap = useCallback(
    (): Record<string, FieldSetter> => ({
      ANTHROPIC_MODEL: { set: setClaudeModel, value: claudeModel },
      ANTHROPIC_DEFAULT_HAIKU_MODEL: {
        set: setDefaultHaikuModel,
        value: defaultHaikuModel,
      },
      ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME: {
        set: setDefaultHaikuModelName,
        value: defaultHaikuModelName,
      },
      ANTHROPIC_DEFAULT_SONNET_MODEL: {
        set: setDefaultSonnetModel,
        value: defaultSonnetModel,
      },
      ANTHROPIC_DEFAULT_SONNET_MODEL_NAME: {
        set: setDefaultSonnetModelName,
        value: defaultSonnetModelName,
      },
      ANTHROPIC_DEFAULT_OPUS_MODEL: {
        set: setDefaultOpusModel,
        value: defaultOpusModel,
      },
      ANTHROPIC_DEFAULT_OPUS_MODEL_NAME: {
        set: setDefaultOpusModelName,
        value: defaultOpusModelName,
      },
      ANTHROPIC_DEFAULT_FABLE_MODEL: {
        set: setDefaultFableModel,
        value: defaultFableModel,
      },
      ANTHROPIC_DEFAULT_FABLE_MODEL_NAME: {
        set: setDefaultFableModelName,
        value: defaultFableModelName,
      },
      CLAUDE_CODE_SUBAGENT_MODEL: {
        set: setSubagentModel,
        value: subagentModel,
      },
    }),
    [
      claudeModel,
      defaultHaikuModel,
      defaultHaikuModelName,
      defaultSonnetModel,
      defaultSonnetModelName,
      defaultOpusModel,
      defaultOpusModelName,
      defaultFableModel,
      defaultFableModelName,
      subagentModel,
    ],
  );

  const handleModelChange = useCallback(
    (field: ClaudeModelEnvField, value: string) => {
      isUserEditingRef.current = true;

      const setterMap = getFieldSetterMap();
      const entry = setterMap[field];
      if (entry && value !== entry.value) {
        entry.set(value);
      }

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
    [onConfigChange, getFieldSetterMap],
  );

  /**
   * 批量更新模型字段：一次性修改多个字段，只触发一次 onConfigChange。
   * 解决"一键设置"等场景下逐字段调用导致的连续重渲染问题。
   */
  const handleBatchModelChange = useCallback(
    (changes: Array<[ClaudeModelEnvField, string]>) => {
      if (changes.length === 0) return;
      isUserEditingRef.current = true;

      const setterMap = getFieldSetterMap();

      // 批量更新本地状态
      for (const [field, value] of changes) {
        const entry = setterMap[field];
        if (entry && value !== entry.value) {
          entry.set(value);
        }
      }

      // 一次性修改 config JSON
      try {
        const currentConfig = latestConfigRef.current
          ? JSON.parse(latestConfigRef.current)
          : { env: {} };
        if (!currentConfig.env) currentConfig.env = {};
        const env = currentConfig.env as Record<string, unknown>;

        for (const [field, value] of changes) {
          const trimmed = value.trim();
          if (trimmed) {
            env[field] = trimmed;
          } else {
            delete env[field];
          }
        }
        // 删除旧键
        delete env["ANTHROPIC_SMALL_FAST_MODEL"];

        const updatedConfig = JSON.stringify(currentConfig, null, 2);
        latestConfigRef.current = updatedConfig;
        onConfigChange(updatedConfig);
      } catch (err) {
        console.error("Failed to batch update model config:", err);
      }
    },
    [onConfigChange, getFieldSetterMap],
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
    handleBatchModelChange,
  };
}

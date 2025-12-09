import { useState, useCallback, useEffect } from "react";

interface UseGeminiConfigStateProps {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
}

/**
 * 管理 Gemini 配置状态
 * Gemini 配置包含两部分：env (环境变量) 和 config (扩展配置 JSON)
 */
export function useGeminiConfigState({
  initialData,
}: UseGeminiConfigStateProps) {
  const [geminiEnv, setGeminiEnvState] = useState("");
  const [geminiConfig, setGeminiConfigState] = useState("");
  const [geminiApiKey, setGeminiApiKey] = useState("");
  const [geminiBaseUrl, setGeminiBaseUrl] = useState("");
  const [geminiModel, setGeminiModel] = useState("");
  const [envError, setEnvError] = useState("");
  const [configError, setConfigError] = useState("");

  // 将 JSON env 对象转换为 .env 格式字符串
  // 保留所有环境变量，已知 key 优先显示
  const envObjToString = useCallback(
    (envObj: Record<string, unknown>): string => {
      const priorityKeys = [
        "GOOGLE_GEMINI_BASE_URL",
        "GEMINI_API_KEY",
        "GEMINI_MODEL",
      ];
      const lines: string[] = [];
      const addedKeys = new Set<string>();

      // 先添加已知 key（按顺序）
      for (const key of priorityKeys) {
        if (typeof envObj[key] === "string" && envObj[key]) {
          lines.push(`${key}=${envObj[key]}`);
          addedKeys.add(key);
        }
      }

      // 再添加其他自定义 key（保留用户添加的环境变量）
      for (const [key, value] of Object.entries(envObj)) {
        if (!addedKeys.has(key) && typeof value === "string") {
          lines.push(`${key}=${value}`);
        }
      }

      return lines.join("\n");
    },
    [],
  );

  // 将 .env 格式字符串转换为 JSON env 对象
  const envStringToObj = useCallback(
    (envString: string): Record<string, string> => {
      const env: Record<string, string> = {};
      const lines = envString.split("\n");
      lines.forEach((line) => {
        const trimmed = line.trim();
        if (!trimmed || trimmed.startsWith("#")) return;
        const equalIndex = trimmed.indexOf("=");
        if (equalIndex > 0) {
          const key = trimmed.substring(0, equalIndex).trim();
          const value = trimmed.substring(equalIndex + 1).trim();
          env[key] = value;
        }
      });
      return env;
    },
    [],
  );

  // 初始化 Gemini 配置（编辑模式）
  useEffect(() => {
    if (!initialData) return;

    const config = initialData.settingsConfig;
    if (typeof config === "object" && config !== null) {
      // 设置 env
      const env = (config as any).env || {};
      setGeminiEnvState(envObjToString(env));

      // 设置 config
      const configObj = (config as any).config || {};
      setGeminiConfigState(JSON.stringify(configObj, null, 2));

      // 提取 API Key、Base URL 和 Model
      if (typeof env.GEMINI_API_KEY === "string") {
        setGeminiApiKey(env.GEMINI_API_KEY);
      }
      if (typeof env.GOOGLE_GEMINI_BASE_URL === "string") {
        setGeminiBaseUrl(env.GOOGLE_GEMINI_BASE_URL);
      }
      if (typeof env.GEMINI_MODEL === "string") {
        setGeminiModel(env.GEMINI_MODEL);
      }
    }
  }, [initialData, envObjToString]);

  // 从 geminiEnv 中提取并同步 API Key、Base URL 和 Model
  useEffect(() => {
    const envObj = envStringToObj(geminiEnv);
    const extractedKey = envObj.GEMINI_API_KEY || "";
    const extractedBaseUrl = envObj.GOOGLE_GEMINI_BASE_URL || "";
    const extractedModel = envObj.GEMINI_MODEL || "";

    if (extractedKey !== geminiApiKey) {
      setGeminiApiKey(extractedKey);
    }
    if (extractedBaseUrl !== geminiBaseUrl) {
      setGeminiBaseUrl(extractedBaseUrl);
    }
    if (extractedModel !== geminiModel) {
      setGeminiModel(extractedModel);
    }
  }, [geminiEnv, envStringToObj, geminiApiKey, geminiBaseUrl, geminiModel]);

  // 验证 Gemini Config JSON
  const validateGeminiConfig = useCallback((value: string): string => {
    if (!value.trim()) return ""; // 空值允许
    try {
      const parsed = JSON.parse(value);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return "";
      }
      return "Config must be a JSON object";
    } catch {
      return "Invalid JSON format";
    }
  }, []);

  // 设置 env
  const setGeminiEnv = useCallback((value: string) => {
    setGeminiEnvState(value);
    // .env 格式较宽松，不做严格校验
    setEnvError("");
  }, []);

  // 设置 config (支持函数更新)
  const setGeminiConfig = useCallback(
    (value: string | ((prev: string) => string)) => {
      const newValue =
        typeof value === "function" ? value(geminiConfig) : value;
      setGeminiConfigState(newValue);
      setConfigError(validateGeminiConfig(newValue));
    },
    [geminiConfig, validateGeminiConfig],
  );

  // 处理 Gemini API Key 输入并写回 env
  const handleGeminiApiKeyChange = useCallback(
    (key: string) => {
      const trimmed = key.trim();
      setGeminiApiKey(trimmed);

      const envObj = envStringToObj(geminiEnv);
      envObj.GEMINI_API_KEY = trimmed;
      const newEnv = envObjToString(envObj);
      setGeminiEnv(newEnv);
    },
    [geminiEnv, envStringToObj, envObjToString, setGeminiEnv],
  );

  // 处理 Gemini Base URL 变化
  const handleGeminiBaseUrlChange = useCallback(
    (url: string) => {
      const sanitized = url.trim().replace(/\/+$/, "");
      setGeminiBaseUrl(sanitized);

      const envObj = envStringToObj(geminiEnv);
      envObj.GOOGLE_GEMINI_BASE_URL = sanitized;
      const newEnv = envObjToString(envObj);
      setGeminiEnv(newEnv);
    },
    [geminiEnv, envStringToObj, envObjToString, setGeminiEnv],
  );

  // 处理 Gemini Model 变化
  const handleGeminiModelChange = useCallback(
    (model: string) => {
      const trimmed = model.trim();
      setGeminiModel(trimmed);

      const envObj = envStringToObj(geminiEnv);
      envObj.GEMINI_MODEL = trimmed;
      const newEnv = envObjToString(envObj);
      setGeminiEnv(newEnv);
    },
    [geminiEnv, envStringToObj, envObjToString, setGeminiEnv],
  );

  // 处理 env 变化
  const handleGeminiEnvChange = useCallback(
    (value: string) => {
      setGeminiEnv(value);
    },
    [setGeminiEnv],
  );

  // 处理 config 变化
  const handleGeminiConfigChange = useCallback(
    (value: string) => {
      setGeminiConfig(value);
    },
    [setGeminiConfig],
  );

  // 重置配置（用于预设切换）
  const resetGeminiConfig = useCallback(
    (env: Record<string, unknown>, config: Record<string, unknown>) => {
      const envString = envObjToString(env);
      const configString = JSON.stringify(config, null, 2);

      setGeminiEnv(envString);
      setGeminiConfig(configString);

      // 提取 API Key、Base URL 和 Model
      if (typeof env.GEMINI_API_KEY === "string") {
        setGeminiApiKey(env.GEMINI_API_KEY);
      } else {
        setGeminiApiKey("");
      }

      if (typeof env.GOOGLE_GEMINI_BASE_URL === "string") {
        setGeminiBaseUrl(env.GOOGLE_GEMINI_BASE_URL);
      } else {
        setGeminiBaseUrl("");
      }

      if (typeof env.GEMINI_MODEL === "string") {
        setGeminiModel(env.GEMINI_MODEL);
      } else {
        setGeminiModel("");
      }
    },
    [envObjToString, setGeminiEnv, setGeminiConfig],
  );

  return {
    geminiEnv,
    geminiConfig,
    geminiApiKey,
    geminiBaseUrl,
    geminiModel,
    envError,
    configError,
    setGeminiEnv,
    setGeminiConfig,
    handleGeminiApiKeyChange,
    handleGeminiBaseUrlChange,
    handleGeminiModelChange,
    handleGeminiEnvChange,
    handleGeminiConfigChange,
    resetGeminiConfig,
    envStringToObj,
    envObjToString,
  };
}

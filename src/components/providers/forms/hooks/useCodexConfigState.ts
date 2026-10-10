import { useState, useCallback, useEffect, useRef } from "react";
import {
  extractCodexBaseUrl,
  extractCodexExperimentalBearerToken,
  extractCodexModelName,
  extractCodexProviderId,
  setCodexBaseUrl as setCodexBaseUrlInConfig,
  setCodexModelName as setCodexModelNameInConfig,
  setCodexProviderId as setCodexProviderIdInConfig,
  updateCodexExperimentalBearerToken,
} from "@/utils/providerConfigUtils";
import { normalizeTomlText } from "@/utils/textNormalization";
import type { CodexCatalogModel } from "@/types";

interface UseCodexConfigStateProps {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
}

// auth.json 缺 OPENAI_API_KEY 时回退到 config.toml 的 experimental_bearer_token
// (Mobile 兼容形态：保留 ChatGPT 登录态但用第三方 token)
function pickCodexApiKey(
  authObj: { OPENAI_API_KEY?: unknown } | null | undefined,
  configText: string,
): string {
  if (authObj && typeof authObj.OPENAI_API_KEY === "string") {
    const key = authObj.OPENAI_API_KEY;
    if (key) return key;
  }
  return extractCodexExperimentalBearerToken(configText) || "";
}

// 目录行 load 映射（隐藏字段白名单重建）。抽成可导出的纯函数，与
// normalizeCodexCatalogModelsForSave 成对做 load→save 回环测试——
// 任一侧丢字段都会静默清空供应商的逐模型声明（reasoningLevels、
// baseInstructions 等），且 UI 无可察觉。
export const mapCodexCatalogModelForForm = (item: any): CodexCatalogModel => {
  // 隐藏字段（原生 Responses profile 用）不在行 UI 暴露，但必须 load→save
  // 原样保留，否则编辑保存 MiMo/MiniMax 等会丢官方 base_instructions、
  // 并行工具、图像模态。DB SSOT 为 camelCase、live 反解兜底可能为 snake_case，
  // 双格式兼容（与 displayName/contextWindow 一致）。
  const supportsParallelToolCalls =
    typeof item?.supportsParallelToolCalls === "boolean"
      ? item.supportsParallelToolCalls
      : typeof item?.supports_parallel_tool_calls === "boolean"
        ? item.supports_parallel_tool_calls
        : undefined;
  const inputModalities = Array.isArray(item?.inputModalities)
    ? item.inputModalities
    : Array.isArray(item?.input_modalities)
      ? item.input_modalities
      : undefined;
  const baseInstructions =
    typeof item?.baseInstructions === "string"
      ? item.baseInstructions
      : typeof item?.base_instructions === "string"
        ? item.base_instructions
        : undefined;
  const reasoningLevels = Array.isArray(item?.reasoningLevels)
    ? item.reasoningLevels
    : Array.isArray(item?.reasoning_levels)
      ? item.reasoning_levels
      : undefined;
  const defaultReasoningLevel =
    typeof item?.defaultReasoningLevel === "string"
      ? item.defaultReasoningLevel
      : typeof item?.default_reasoning_level === "string"
        ? item.default_reasoning_level
        : undefined;
  return {
    model: typeof item?.model === "string" ? item.model : "",
    displayName:
      typeof item?.displayName === "string"
        ? item.displayName
        : typeof item?.display_name === "string"
          ? item.display_name
          : "",
    contextWindow:
      typeof item?.contextWindow === "string" ||
      typeof item?.contextWindow === "number"
        ? item.contextWindow
        : typeof item?.context_window === "string" ||
            typeof item?.context_window === "number"
          ? item.context_window
          : "",
    ...(supportsParallelToolCalls !== undefined
      ? { supportsParallelToolCalls }
      : {}),
    ...(inputModalities ? { inputModalities } : {}),
    ...(baseInstructions ? { baseInstructions } : {}),
    ...(reasoningLevels && reasoningLevels.length > 0
      ? { reasoningLevels }
      : {}),
    ...(defaultReasoningLevel ? { defaultReasoningLevel } : {}),
  };
};

/**
 * 管理 Codex 配置状态
 * Codex 配置包含两部分：auth.json (JSON) 和 config.toml (TOML 字符串)
 */
export function useCodexConfigState({ initialData }: UseCodexConfigStateProps) {
  const [codexAuth, setCodexAuthState] = useState("");
  const [codexConfig, setCodexConfigState] = useState("");
  const [codexApiKey, setCodexApiKey] = useState("");
  const [codexBaseUrl, setCodexBaseUrl] = useState("");
  const [codexModel, setCodexModel] = useState("");
  const [codexProviderId, setCodexProviderId] = useState("");
  // Provider ID 输入框是否被用户改过：区分「用户输入的自定义 id」和「从 TOML 回显的
  // 既有选路」。保留名（amazon-bedrock 等）作为已有 BuiltIn 路由是合法的，只有用户
  // 新建/修改自定义 id 时才拦（issue #7856 review）。
  const [codexProviderIdEdited, setCodexProviderIdEdited] = useState(false);
  const [codexCatalogModels, setCodexCatalogModels] = useState<
    CodexCatalogModel[]
  >([]);
  const [codexAuthError, setCodexAuthError] = useState("");

  const isUpdatingCodexBaseUrlRef = useRef(false);
  const isUpdatingCodexModelRef = useRef(false);
  const isUpdatingCodexProviderIdRef = useRef(false);

  // 初始化 Codex 配置（编辑模式）
  useEffect(() => {
    if (!initialData) return;

    const config = initialData.settingsConfig;
    if (typeof config === "object" && config !== null) {
      // 设置 config.toml
      const configStr =
        typeof (config as any).config === "string"
          ? (config as any).config
          : "";
      setCodexConfigState(configStr);

      // 设置 auth.json
      const auth = (config as any).auth || {};
      setCodexAuthState(JSON.stringify(auth, null, 2));

      const modelCatalog = (config as any).modelCatalog;
      const rawCatalogModels = Array.isArray(modelCatalog?.models)
        ? modelCatalog.models
        : [];
      setCodexCatalogModels(
        rawCatalogModels
          .map(mapCodexCatalogModelForForm)
          .filter((item: CodexCatalogModel) => item.model.trim()),
      );

      // 提取 Base URL
      const initialBaseUrl = extractCodexBaseUrl(configStr);
      if (initialBaseUrl) {
        setCodexBaseUrl(initialBaseUrl);
      }

      setCodexApiKey(pickCodexApiKey(auth, configStr));
    }
  }, [initialData]);

  // 与 TOML 配置保持基础 URL 同步
  useEffect(() => {
    if (isUpdatingCodexBaseUrlRef.current) {
      return;
    }
    const extracted = extractCodexBaseUrl(codexConfig) || "";
    setCodexBaseUrl((prev) => (prev === extracted ? prev : extracted));
  }, [codexConfig]);

  // 与 TOML 配置保持默认模型同步（顶层 model 键）
  useEffect(() => {
    if (isUpdatingCodexModelRef.current) {
      return;
    }
    const extracted = extractCodexModelName(codexConfig) || "";
    setCodexModel((prev) => (prev === extracted ? prev : extracted));
  }, [codexConfig]);

  // 与 TOML 配置保持 Provider ID 同步（model_provider 选路；无选路行显示为空 = 默认 custom）
  useEffect(() => {
    if (isUpdatingCodexProviderIdRef.current) {
      return;
    }
    const extracted = extractCodexProviderId(codexConfig) || "";
    setCodexProviderId((prev) => (prev === extracted ? prev : extracted));
    // 回显的值（含已有内置路由如 amazon-bedrock 的选路）不是用户新建的自定义 id：
    // 保留名校验只对用户输入过的值生效，配置一回显就复位。
    setCodexProviderIdEdited(false);
  }, [codexConfig]);

  // 获取 API Key（从 auth JSON）
  const getCodexAuthApiKey = useCallback((authString: string): string => {
    try {
      const auth = JSON.parse(authString || "{}");
      return typeof auth.OPENAI_API_KEY === "string" ? auth.OPENAI_API_KEY : "";
    } catch {
      return "";
    }
  }, []);

  // 从 codexAuth 中提取并同步 API Key
  useEffect(() => {
    let parsed: { OPENAI_API_KEY?: unknown } | null = null;
    try {
      parsed = JSON.parse(codexAuth || "{}");
    } catch {
      parsed = null;
    }
    const extractedKey = pickCodexApiKey(parsed, codexConfig);
    setCodexApiKey((prev) => (prev === extractedKey ? prev : extractedKey));
  }, [codexAuth, codexConfig]);

  // 验证 Codex Auth JSON
  const validateCodexAuth = useCallback((value: string): string => {
    if (!value.trim()) return "";
    try {
      const parsed = JSON.parse(value);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        return "Auth JSON must be an object";
      }
      return "";
    } catch {
      return "Invalid JSON format";
    }
  }, []);

  // 设置 auth 并验证
  const setCodexAuth = useCallback(
    (value: string) => {
      setCodexAuthState(value);
      setCodexAuthError(validateCodexAuth(value));
    },
    [validateCodexAuth],
  );

  // 设置 config (支持函数更新)
  const setCodexConfig = useCallback(
    (value: string | ((prev: string) => string)) => {
      setCodexConfigState((prev) =>
        typeof value === "function"
          ? (value as (input: string) => string)(prev)
          : value,
      );
    },
    [],
  );

  // 处理 Codex API Key 输入并写回 auth.json
  // 同步: 若 config.toml 当前含 experimental_bearer_token (Mobile 兼容形态),
  // 也一并更新/清除——否则用户清空输入框会被 pickCodexApiKey 的 fallback 又填回去
  const handleCodexApiKeyChange = useCallback(
    (key: string) => {
      const trimmed = key.trim();
      setCodexApiKey(trimmed);
      try {
        const auth = JSON.parse(codexAuth || "{}");
        auth.OPENAI_API_KEY = trimmed;
        setCodexAuth(JSON.stringify(auth, null, 2));
      } catch {
        // ignore
      }
      setCodexConfig((prev) =>
        updateCodexExperimentalBearerToken(prev, trimmed),
      );
    },
    [codexAuth, setCodexAuth, setCodexConfig],
  );

  // 处理 Codex Base URL 变化
  const handleCodexBaseUrlChange = useCallback(
    (url: string) => {
      const sanitized = url.trim();
      setCodexBaseUrl(sanitized);

      isUpdatingCodexBaseUrlRef.current = true;
      setCodexConfig((prev) => setCodexBaseUrlInConfig(prev, sanitized));
      setTimeout(() => {
        isUpdatingCodexBaseUrlRef.current = false;
      }, 0);
    },
    [setCodexConfig],
  );

  // 处理默认模型变化（写回 TOML 顶层 model；清空则删掉该行，交回 Codex 内置默认）
  // 剥控制字符：值可能来自 /models 下拉（远端数据），换行等会破坏单行 TOML 语义
  const handleCodexModelChange = useCallback(
    (model: string) => {
      const sanitized = model.replace(/[\u0000-\u001f\u007f]/g, "").trim();
      setCodexModel(sanitized);

      isUpdatingCodexModelRef.current = true;
      setCodexConfig((prev) => setCodexModelNameInConfig(prev, sanitized));
      setTimeout(() => {
        isUpdatingCodexModelRef.current = false;
      }, 0);
    },
    [setCodexConfig],
  );

  // 处理 Provider ID 变化（改写 model_provider 选路与 [model_providers.<id>] 表键；
  // 清空回到默认 custom）。剥控制字符：粘贴进去的换行等会破坏单行 TOML 语义。
  const handleCodexProviderIdChange = useCallback(
    (providerId: string) => {
      const sanitized = providerId.replace(/[\u0000-\u001f\u007f]/g, "");
      setCodexProviderId(sanitized);
      setCodexProviderIdEdited(true);

      isUpdatingCodexProviderIdRef.current = true;
      setCodexConfig((prev) => setCodexProviderIdInConfig(prev, sanitized));
      setTimeout(() => {
        isUpdatingCodexProviderIdRef.current = false;
      }, 0);
    },
    [setCodexConfig],
  );

  // 处理 config 变化（同步 Base URL）
  const handleCodexConfigChange = useCallback(
    (value: string) => {
      // 归一化中文/全角/弯引号，避免 TOML 解析报错
      const normalized = normalizeTomlText(value);
      setCodexConfig(normalized);

      if (!isUpdatingCodexBaseUrlRef.current) {
        const extracted = extractCodexBaseUrl(normalized) || "";
        if (extracted !== codexBaseUrl) {
          setCodexBaseUrl(extracted);
        }
      }
    },
    [setCodexConfig, codexBaseUrl],
  );

  // 重置配置（用于预设切换）
  const resetCodexConfig = useCallback(
    (
      auth: Record<string, unknown>,
      config: string,
      modelCatalogModels: CodexCatalogModel[] = [],
    ) => {
      const authString = JSON.stringify(auth, null, 2);
      setCodexAuth(authString);
      setCodexConfig(config);
      setCodexCatalogModels(modelCatalogModels);

      const baseUrl = extractCodexBaseUrl(config);
      setCodexBaseUrl(baseUrl || "");

      setCodexApiKey(pickCodexApiKey(auth, config));
    },
    [setCodexAuth, setCodexConfig, setCodexCatalogModels],
  );

  return {
    codexAuth,
    codexConfig,
    codexApiKey,
    codexBaseUrl,
    codexModel,
    codexProviderId,
    codexProviderIdEdited,
    codexCatalogModels,
    codexAuthError,
    setCodexAuth,
    setCodexConfig,
    setCodexCatalogModels,
    handleCodexApiKeyChange,
    handleCodexBaseUrlChange,
    handleCodexModelChange,
    handleCodexProviderIdChange,
    handleCodexConfigChange,
    resetCodexConfig,
    getCodexAuthApiKey,
    validateCodexAuth,
  };
}

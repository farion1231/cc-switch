import { useState, useCallback, useEffect, useRef } from "react";
import {
  extractCodexBaseUrl,
  extractCodexExperimentalBearerToken,
  setCodexBaseUrl as setCodexBaseUrlInConfig,
  updateCodexExperimentalBearerToken,
} from "@/utils/providerConfigUtils";
import { normalizeTomlText } from "@/utils/textNormalization";
import type {
  CodexApiFormat,
  CodexCatalogModel,
  CodexRoutingConfig,
  CodexRoutingRoute,
} from "@/types";

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

// 将旧版手写 route 数组迁移成新的 codexRouting 结构，供表单展示和保存。
function normalizeLegacyCodexRoute(route: any, index: number): CodexRoutingRoute {
  const models = Array.isArray(route?.models)
    ? route.models.filter((item: unknown): item is string => typeof item === "string")
    : [];
  const prefixes = Array.isArray(route?.modelPrefixes)
    ? route.modelPrefixes
    : Array.isArray(route?.model_prefixes)
      ? route.model_prefixes
      : [];
  const apiFormat = String(route?.wire_api ?? route?.wireApi ?? route?.apiFormat ?? "openai_chat");
  const normalizedApiFormat: CodexApiFormat =
    apiFormat === "responses"
      ? "openai_responses"
      : apiFormat === "messages"
        ? "openai_messages"
        : apiFormat === "chat"
          ? "openai_chat"
          : (apiFormat as CodexApiFormat);

  return {
    id: String(route?.id || `route-${index + 1}`),
    label: typeof route?.label === "string" ? route.label : route?.name,
    enabled: route?.enabled !== false,
    match: {
      models,
      prefixes: prefixes.filter((item: unknown): item is string => typeof item === "string"),
    },
    upstream: {
      baseUrl: route?.baseUrl ?? route?.baseURL ?? route?.base_url ?? "",
      apiFormat: normalizedApiFormat,
      auth: route?.auth?.source
        ? route.auth
        : {
            source: route?.providerType === "codex_oauth" ? "managed_codex_oauth" : "provider_config",
            authProvider: route?.providerType === "codex_oauth" ? "codex_oauth" : undefined,
          },
      apiKey: route?.apiKey ?? route?.api_key ?? route?.auth?.OPENAI_API_KEY ?? "",
      modelMap: route?.modelMap ?? undefined,
    },
    capabilities: route?.capabilities ?? undefined,
  };
}

// 读取新 schema；没有新 schema 时，把旧字段转换成新结构以便 UI 保存时写回 codexRouting。
function extractCodexRoutingConfig(config: Record<string, any>): CodexRoutingConfig {
  const routing = config.codexRouting;
  if (routing && typeof routing === "object") {
    return {
      enabled: routing.enabled !== false,
      defaultRouteId: typeof routing.defaultRouteId === "string" ? routing.defaultRouteId : "",
      routes: Array.isArray(routing.routes) ? routing.routes : [],
    };
  }

  const legacyRoutes = Array.isArray(config.codexModelRoutes)
    ? config.codexModelRoutes
    : Array.isArray(config.modelRoutes)
      ? config.modelRoutes
      : [];
  return legacyRoutes.length > 0
    ? {
        enabled: true,
        defaultRouteId: "",
        routes: legacyRoutes.map(normalizeLegacyCodexRoute),
      }
    : { enabled: false, defaultRouteId: "", routes: [] };
}

/**
 * 管理 Codex 配置状态
 * Codex 配置包含两部分：auth.json (JSON) 和 config.toml (TOML 字符串)
 */
export function useCodexConfigState({ initialData }: UseCodexConfigStateProps) {
  const [codexAuth, setCodexAuthState] = useState("");
  const [codexConfig, setCodexConfigState] = useState("");
  const [codexApiKey, setCodexApiKey] = useState("");
  const [codexBaseUrl, setCodexBaseUrl] = useState("");
  const [codexCatalogModels, setCodexCatalogModels] = useState<
    CodexCatalogModel[]
  >([]);
  const [codexRouting, setCodexRouting] = useState<CodexRoutingConfig>({
    enabled: false,
    defaultRouteId: "",
    routes: [],
  });
  const [codexAuthError, setCodexAuthError] = useState("");

  const isUpdatingCodexBaseUrlRef = useRef(false);

  // 初始化 Codex 配置（编辑模式）
  useEffect(() => {
    if (!initialData) return;

    const config = initialData.settingsConfig;
    if (typeof config === "object" && config !== null) {
      // 设置 auth.json
      const auth = (config as any).auth || {};
      setCodexAuthState(JSON.stringify(auth, null, 2));

      // 设置 config.toml
      const configStr =
        typeof (config as any).config === "string"
          ? (config as any).config
          : "";
      setCodexConfigState(configStr);

      const modelCatalog = (config as any).modelCatalog;
      const rawCatalogModels = Array.isArray(modelCatalog?.models)
        ? modelCatalog.models
        : [];
      setCodexCatalogModels(
        rawCatalogModels
          .map((item: any) => ({
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
          }))
          .filter((item: CodexCatalogModel) => item.model.trim()),
      );
      setCodexRouting(extractCodexRoutingConfig(config as Record<string, any>));

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
      routingConfig: CodexRoutingConfig = {
        enabled: false,
        defaultRouteId: "",
        routes: [],
      },
    ) => {
      const authString = JSON.stringify(auth, null, 2);
      setCodexAuth(authString);
      setCodexConfig(config);
      setCodexCatalogModels(modelCatalogModels);
      setCodexRouting(routingConfig);

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
    codexCatalogModels,
    codexRouting,
    codexAuthError,
    setCodexAuth,
    setCodexConfig,
    setCodexCatalogModels,
    setCodexRouting,
    handleCodexApiKeyChange,
    handleCodexBaseUrlChange,
    handleCodexConfigChange,
    resetCodexConfig,
    getCodexAuthApiKey,
    validateCodexAuth,
  };
}

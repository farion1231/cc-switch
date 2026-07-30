import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { configApi } from "@/lib/api";

const LEGACY_STORAGE_KEY = "cc-switch:gemini-common-config-snippet";
const DEFAULT_GEMINI_COMMON_CONFIG_SNIPPET = "{}";

/** 供应商专属、不可共享的键（端点与主凭据），按名单剥离。 */
const GEMINI_COMMON_ENV_FORBIDDEN_KEYS = [
  "GOOGLE_GEMINI_BASE_URL",
  "GEMINI_API_KEY",
] as const;

/**
 * 凭据键的模式匹配，对应后端 `ProviderService::is_sensitive_config_key`。
 *
 * 共享片段会被深合并进**其它** Gemini 供应商的 env，所以任何凭据都不能进来。
 * 固定名单挡不住下一个 `*_API_KEY`（`GOOGLE_API_KEY` 就是这么漏过去的），
 * 必须用模式覆盖整类。两端判定要一致，否则后端清理完还能从这里手工写回去。
 */
const SENSITIVE_EXACT = [
  "APIKEY",
  "API_KEY",
  "TOKEN",
  "SECRET",
  "PASSWORD",
  "CREDENTIALS",
];
// 单数 `_TOKEN` 命中 AWS_SESSION_TOKEN 等，但不误伤复数 `_TOKENS`（那是正常配置）
const SENSITIVE_SUFFIXES = [
  // 裸 `_KEY` 是最常见的凭据写法（OPENAI_KEY / GROQ_KEY / XAI_KEY…），必须单列：
  // 只枚举 `_API_KEY` / `_ACCESS_KEY` 这些子类，等于把最普通的那一种漏在外面。
  "_KEY",
  "_API_KEY",
  "_ACCESS_KEY",
  "_ACCESS_KEY_ID",
  "_KEY_ID",
  "_PRIVATE_KEY",
  // 不带分隔符的复合写法各走各的后缀：`_KEY` 够不着 `..._APIKEY`。
  "_APIKEY",
  "_ACCESSKEY",
  "_SECRETKEY",
  "_APITOKEN",
  "_AUTH_TOKEN",
  "_TOKEN",
  // GITHUB_PAT / GITLAB_PAT 等 personal access token 的惯用写法，
  // 既不含 TOKEN 也不含 KEY，前面每一条规则都够不着。
  "_PAT",
  // 口令类的常见缩写。`_PASS` 不会误伤 `*_BYPASS`，`_PWD` 不会误伤 PWD / OLDPWD。
  "_PWD",
  "_PASS",
  "_PASSPHRASE",
  "_CREDS",
];
const SENSITIVE_CONTAINS = [
  "SECRET",
  "PASSWORD",
  "PASSWD",
  "CREDENTIAL",
  "PRIVATE_KEY",
  "BEARER_TOKEN",
];

function isForbiddenCommonEnvKey(name: string): boolean {
  if (
    GEMINI_COMMON_ENV_FORBIDDEN_KEYS.includes(
      name as (typeof GEMINI_COMMON_ENV_FORBIDDEN_KEYS)[number],
    )
  ) {
    return true;
  }
  const upper = name.toUpperCase();
  return (
    SENSITIVE_EXACT.includes(upper) ||
    SENSITIVE_SUFFIXES.some((suffix) => upper.endsWith(suffix)) ||
    SENSITIVE_CONTAINS.some((part) => upper.includes(part))
  );
}

interface UseGeminiCommonConfigProps {
  envValue: string;
  onEnvChange: (env: string) => void;
  envStringToObj: (envString: string) => Record<string, string>;
  envObjToString: (envObj: Record<string, unknown>) => string;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  initialEnabled?: boolean;
  selectedPresetId?: string;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.prototype.toString.call(value) === "[object Object]"
  );
}

/**
 * 管理 Gemini 通用配置片段 (JSON 格式)
 * 写入 Gemini 的 .env，但会排除以下敏感字段：
 * - GOOGLE_GEMINI_BASE_URL
 * - GEMINI_API_KEY
 */
export function useGeminiCommonConfig({
  envValue,
  onEnvChange,
  envStringToObj,
  envObjToString,
  initialData,
  initialEnabled,
  selectedPresetId,
}: UseGeminiCommonConfigProps) {
  const { t } = useTranslation();
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_GEMINI_COMMON_CONFIG_SNIPPET,
  );
  const [commonConfigError, setCommonConfigError] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isExtracting, setIsExtracting] = useState(false);

  // 用于跟踪是否正在通过通用配置更新
  const isUpdatingFromCommonConfig = useRef(false);
  // 用于跟踪新建模式是否已初始化默认勾选
  const hasInitializedNewMode = useRef(false);
  // 用于跟踪编辑模式是否已初始化显式开关/预览
  const hasInitializedEditMode = useRef(false);

  // 当预设变化时，重置初始化标记，使新预设能够重新触发初始化逻辑
  useEffect(() => {
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
  }, [selectedPresetId, initialEnabled]);

  const parseSnippetEnv = useCallback(
    (
      snippetString: string,
    ): { env: Record<string, string>; error?: string } => {
      const trimmed = snippetString.trim();
      if (!trimmed) {
        return { env: {} };
      }

      let parsed: unknown;
      try {
        parsed = JSON.parse(trimmed);
      } catch {
        return { env: {}, error: t("geminiConfig.invalidJsonFormat") };
      }

      if (!isPlainObject(parsed)) {
        return { env: {}, error: t("geminiConfig.invalidJsonFormat") };
      }

      const keys = Object.keys(parsed);
      const forbiddenKeys = keys.filter(isForbiddenCommonEnvKey);
      if (forbiddenKeys.length > 0) {
        return {
          env: {},
          error: t("geminiConfig.commonConfigInvalidKeys", {
            keys: forbiddenKeys.join(", "),
          }),
        };
      }

      const env: Record<string, string> = {};
      for (const [key, value] of Object.entries(parsed)) {
        if (typeof value !== "string") {
          return {
            env: {},
            error: t("geminiConfig.commonConfigInvalidValues"),
          };
        }
        const normalized = value.trim();
        if (!normalized) continue;
        env[key] = normalized;
      }

      return { env };
    },
    [t],
  );

  const hasEnvCommonConfigSnippet = useCallback(
    (envObj: Record<string, string>, snippetEnv: Record<string, string>) => {
      const entries = Object.entries(snippetEnv);
      if (entries.length === 0) return false;
      return entries.every(([key, value]) => envObj[key] === value);
    },
    [],
  );

  const applySnippetToEnv = useCallback(
    (envObj: Record<string, string>, snippetEnv: Record<string, string>) => {
      const updated = { ...envObj };
      for (const [key, value] of Object.entries(snippetEnv)) {
        if (typeof value === "string") {
          updated[key] = value;
        }
      }
      return updated;
    },
    [],
  );

  const removeSnippetFromEnv = useCallback(
    (envObj: Record<string, string>, snippetEnv: Record<string, string>) => {
      const updated = { ...envObj };
      for (const [key, value] of Object.entries(snippetEnv)) {
        if (typeof value === "string" && updated[key] === value) {
          delete updated[key];
        }
      }
      return updated;
    },
    [],
  );

  // 初始化：从 config.json 加载，支持从 localStorage 迁移
  useEffect(() => {
    let mounted = true;

    const loadSnippet = async () => {
      try {
        // 使用统一 API 加载
        const snippet = await configApi.getCommonConfigSnippet("gemini");

        if (snippet && snippet.trim()) {
          if (mounted) {
            setCommonConfigSnippetState(snippet);
          }
        } else {
          // 如果 config.json 中没有，尝试从 localStorage 迁移
          if (typeof window !== "undefined") {
            try {
              const legacySnippet =
                window.localStorage.getItem(LEGACY_STORAGE_KEY);
              if (legacySnippet && legacySnippet.trim()) {
                const parsed = parseSnippetEnv(legacySnippet);
                if (parsed.error) {
                  console.warn(
                    "[迁移] legacy Gemini 通用配置片段格式不符合当前规则，跳过迁移",
                  );
                  return;
                }
                // 迁移到 config.json
                await configApi.setCommonConfigSnippet("gemini", legacySnippet);
                if (mounted) {
                  setCommonConfigSnippetState(legacySnippet);
                }
                // 清理 localStorage
                window.localStorage.removeItem(LEGACY_STORAGE_KEY);
                console.log(
                  "[迁移] Gemini 通用配置已从 localStorage 迁移到 config.json",
                );
              }
            } catch (e) {
              console.warn("[迁移] 从 localStorage 迁移失败:", e);
            }
          }
        }
      } catch (error) {
        console.error("加载 Gemini 通用配置失败:", error);
      } finally {
        if (mounted) {
          setIsLoading(false);
        }
      }
    };

    loadSnippet();

    return () => {
      mounted = false;
    };
  }, [parseSnippetEnv]);

  // 初始化时检查通用配置片段（编辑模式）
  useEffect(() => {
    if (
      !initialData?.settingsConfig ||
      isLoading ||
      hasInitializedEditMode.current
    ) {
      return;
    }

    hasInitializedEditMode.current = true;

    try {
      const env =
        isPlainObject(initialData.settingsConfig.env) &&
        Object.keys(initialData.settingsConfig.env).length > 0
          ? (initialData.settingsConfig.env as Record<string, string>)
          : {};
      const parsed = parseSnippetEnv(commonConfigSnippet);
      if (parsed.error) {
        if (commonConfigSnippet.trim()) {
          setCommonConfigError(parsed.error);
        }
        setUseCommonConfig(false);
        return;
      }
      const inferredHasCommon = hasEnvCommonConfigSnippet(
        env,
        parsed.env as Record<string, string>,
      );

      // 优先级：显式设置的 initialEnabled > 从配置推断的值
      // 如果 initialEnabled 为 undefined，使用推断值
      const hasCommon =
        initialEnabled !== undefined ? initialEnabled : inferredHasCommon;

      // 如果应该启用通用配置但配置中还没有，则自动添加
      if (
        hasCommon &&
        !inferredHasCommon &&
        Object.keys(parsed.env).length > 0
      ) {
        const currentEnv = envStringToObj(envValue);
        const merged = applySnippetToEnv(currentEnv, parsed.env);
        const nextEnvString = envObjToString(merged);

        setCommonConfigError("");
        setUseCommonConfig(true);
        isUpdatingFromCommonConfig.current = true;
        onEnvChange(nextEnvString);
        setTimeout(() => {
          isUpdatingFromCommonConfig.current = false;
        }, 0);
        return;
      }

      setCommonConfigError("");
      setUseCommonConfig(hasCommon);
    } catch {
      // ignore parse error
    }
  }, [
    applySnippetToEnv,
    commonConfigSnippet,
    envObjToString,
    envStringToObj,
    envValue,
    hasEnvCommonConfigSnippet,
    initialData,
    initialEnabled,
    isLoading,
    onEnvChange,
    parseSnippetEnv,
  ]);

  // 新建模式：如果通用配置片段存在且有效，默认启用
  useEffect(() => {
    if (initialData || isLoading || hasInitializedNewMode.current) {
      return;
    }

    hasInitializedNewMode.current = true;

    const parsed = parseSnippetEnv(commonConfigSnippet);
    if (parsed.error) {
      if (commonConfigSnippet.trim()) {
        setCommonConfigError(parsed.error);
      }
      setUseCommonConfig(false);
      return;
    }
    const hasContent = Object.keys(parsed.env).length > 0;
    if (!hasContent) return;

    setCommonConfigError("");
    setUseCommonConfig(true);
    const currentEnv = envStringToObj(envValue);
    const merged = applySnippetToEnv(currentEnv, parsed.env);
    const nextEnvString = envObjToString(merged);

    isUpdatingFromCommonConfig.current = true;
    onEnvChange(nextEnvString);
    setTimeout(() => {
      isUpdatingFromCommonConfig.current = false;
    }, 0);
  }, [
    initialData,
    isLoading,
    commonConfigSnippet,
    envValue,
    envStringToObj,
    envObjToString,
    applySnippetToEnv,
    onEnvChange,
    parseSnippetEnv,
  ]);

  // 处理通用配置开关
  const handleCommonConfigToggle = useCallback(
    (checked: boolean) => {
      const parsed = parseSnippetEnv(commonConfigSnippet);
      if (parsed.error) {
        setCommonConfigError(parsed.error);
        setUseCommonConfig(false);
        return;
      }
      if (Object.keys(parsed.env).length === 0) {
        setCommonConfigError(t("geminiConfig.noCommonConfigToApply"));
        setUseCommonConfig(false);
        return;
      }

      const currentEnv = envStringToObj(envValue);
      const updatedEnvObj = checked
        ? applySnippetToEnv(currentEnv, parsed.env)
        : removeSnippetFromEnv(currentEnv, parsed.env);

      setCommonConfigError("");
      setUseCommonConfig(checked);

      isUpdatingFromCommonConfig.current = true;
      onEnvChange(envObjToString(updatedEnvObj));
      setTimeout(() => {
        isUpdatingFromCommonConfig.current = false;
      }, 0);
    },
    [
      applySnippetToEnv,
      commonConfigSnippet,
      envObjToString,
      envStringToObj,
      envValue,
      onEnvChange,
      parseSnippetEnv,
      removeSnippetFromEnv,
      t,
    ],
  );

  // 处理通用配置片段变化
  const handleCommonConfigSnippetChange = useCallback(
    (value: string): boolean => {
      const previousSnippet = commonConfigSnippet;

      if (!value.trim()) {
        setCommonConfigError("");

        if (useCommonConfig) {
          const parsedPrevious = parseSnippetEnv(previousSnippet);
          if (
            !parsedPrevious.error &&
            Object.keys(parsedPrevious.env).length > 0
          ) {
            const currentEnv = envStringToObj(envValue);
            const updatedEnv = removeSnippetFromEnv(
              currentEnv,
              parsedPrevious.env,
            );
            onEnvChange(envObjToString(updatedEnv));
          }
          setUseCommonConfig(false);
        }

        setCommonConfigSnippetState("");
        configApi
          .setCommonConfigSnippet("gemini", "")
          .catch((error: unknown) => {
            console.error("保存 Gemini 通用配置失败:", error);
            setCommonConfigError(
              t("geminiConfig.saveFailed", { error: String(error) }),
            );
          });
        return true;
      }

      // 校验 JSON 格式
      const parsed = parseSnippetEnv(value);
      if (parsed.error) {
        setCommonConfigError(parsed.error);
        return false;
      }

      // 若当前启用通用配置，需要替换为最新片段
      if (useCommonConfig) {
        const prevParsed = parseSnippetEnv(previousSnippet);
        const prevEnv = prevParsed.error ? {} : prevParsed.env;
        const nextEnv = parsed.env;
        const currentEnv = envStringToObj(envValue);

        const withoutOld =
          Object.keys(prevEnv).length > 0
            ? removeSnippetFromEnv(currentEnv, prevEnv)
            : currentEnv;
        const withNew =
          Object.keys(nextEnv).length > 0
            ? applySnippetToEnv(withoutOld, nextEnv)
            : withoutOld;

        isUpdatingFromCommonConfig.current = true;
        onEnvChange(envObjToString(withNew));
        setTimeout(() => {
          isUpdatingFromCommonConfig.current = false;
        }, 0);
      }

      setCommonConfigError("");
      setCommonConfigSnippetState(value);
      configApi
        .setCommonConfigSnippet("gemini", value)
        .catch((error: unknown) => {
          console.error("保存 Gemini 通用配置失败:", error);
          setCommonConfigError(
            t("geminiConfig.saveFailed", { error: String(error) }),
          );
        });

      return true;
    },
    [
      applySnippetToEnv,
      commonConfigSnippet,
      envObjToString,
      envStringToObj,
      envValue,
      onEnvChange,
      parseSnippetEnv,
      removeSnippetFromEnv,
      t,
      useCommonConfig,
    ],
  );

  // 当 env 变化时检查是否包含通用配置（但避免在通过通用配置更新时检查）
  useEffect(() => {
    if (isUpdatingFromCommonConfig.current || isLoading) {
      return;
    }
    const parsed = parseSnippetEnv(commonConfigSnippet);
    if (parsed.error) return;
    const envObj = envStringToObj(envValue);
    setUseCommonConfig(
      hasEnvCommonConfigSnippet(envObj, parsed.env as Record<string, string>),
    );
  }, [
    envValue,
    commonConfigSnippet,
    envStringToObj,
    hasEnvCommonConfigSnippet,
    isLoading,
    parseSnippetEnv,
  ]);

  // 从编辑器当前内容提取通用配置片段
  const handleExtract = useCallback(async () => {
    setIsExtracting(true);
    setCommonConfigError("");

    try {
      const extracted = await configApi.extractCommonConfigSnippet("gemini", {
        settingsConfig: JSON.stringify({
          env: envStringToObj(envValue),
        }),
      });

      if (!extracted || extracted === "{}") {
        setCommonConfigError(t("geminiConfig.extractNoCommonConfig"));
        return;
      }

      // 验证 JSON 格式
      const parsed = parseSnippetEnv(extracted);
      if (parsed.error) {
        setCommonConfigError(t("geminiConfig.extractedConfigInvalid"));
        return;
      }

      // 更新片段状态
      setCommonConfigSnippetState(extracted);

      // 保存到后端
      await configApi.setCommonConfigSnippet("gemini", extracted);
    } catch (error) {
      console.error("提取 Gemini 通用配置失败:", error);
      setCommonConfigError(
        t("geminiConfig.extractFailed", { error: String(error) }),
      );
    } finally {
      setIsExtracting(false);
    }
  }, [envStringToObj, envValue, parseSnippetEnv, t]);

  const clearCommonConfigError = useCallback(() => {
    setCommonConfigError("");
  }, []);

  return {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    isLoading,
    isExtracting,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
    handleExtract,
    clearCommonConfigError,
  };
}

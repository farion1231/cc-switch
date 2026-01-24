import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { configApi } from "@/lib/api";
import {
  replaceGeminiCommonConfigSnippet,
  GEMINI_COMMON_ENV_FORBIDDEN_KEYS,
  type GeminiForbiddenEnvKey,
} from "@/utils/providerConfigUtils";
import type { ProviderMeta } from "@/types";

const LEGACY_STORAGE_KEY = "cc-switch:gemini-common-config-snippet";
const DEFAULT_GEMINI_COMMON_CONFIG_SNIPPET = "{}";

interface UseGeminiCommonConfigProps {
  envValue: string;
  onEnvChange: (env: string) => void;
  envStringToObj: (envString: string) => Record<string, string>;
  envObjToString: (envObj: Record<string, unknown>) => string;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    meta?: ProviderMeta;
  };
  selectedPresetId?: string;
  /** 当前正在编辑的供应商 ID（用于同步时跳过） */
  currentProviderId?: string;
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
  selectedPresetId,
  currentProviderId,
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
  // 用于跟踪用户是否手动切换，避免自动检测覆盖用户意图
  const hasUserToggledCommonConfig = useRef(false);
  // 用于跟踪新建模式是否已初始化默认勾选
  const hasInitializedNewMode = useRef(false);
  // 用于跟踪编辑模式是否已初始化（避免反复覆盖用户切换）
  const hasInitializedEditMode = useRef(false);
  // 用于避免异步保存乱序导致的过期同步
  const saveSequenceRef = useRef(0);
  const saveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const enqueueSave = useCallback((saveFn: () => Promise<void>) => {
    const next = saveQueueRef.current.then(saveFn);
    saveQueueRef.current = next.catch(() => {});
    return next;
  }, []);

  // 当预设变化时，重置初始化标记，使新预设能够重新触发初始化逻辑
  useEffect(() => {
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
    hasUserToggledCommonConfig.current = false;
  }, [selectedPresetId]);

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
      const forbiddenKeys = keys.filter((key) =>
        GEMINI_COMMON_ENV_FORBIDDEN_KEYS.includes(key as GeminiForbiddenEnvKey),
      );
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

  // 初始化：从数据库加载，支持从 localStorage 迁移
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
          // 如果数据库中没有，尝试从 localStorage 迁移
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
                  "[迁移] Gemini 通用配置已从 localStorage 迁移到数据库",
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

  // 初始化时从 meta 读取启用状态（编辑模式）
  // 优先使用 meta，若 meta 未定义则回退到内容检测
  useEffect(() => {
    if (initialData && !isLoading && !hasInitializedEditMode.current) {
      hasInitializedEditMode.current = true;

      // 使用 meta 中记录的按 app 启用状态
      const metaByApp = initialData.meta?.commonConfigEnabledByApp;
      const resolvedMetaEnabled =
        metaByApp?.gemini ?? initialData.meta?.commonConfigEnabled;

      if (resolvedMetaEnabled !== undefined) {
        if (!resolvedMetaEnabled) {
          setUseCommonConfig(false);
          return;
        }

        const parsed = parseSnippetEnv(commonConfigSnippet);
        if (parsed.error || Object.keys(parsed.env).length === 0) {
          setCommonConfigError(
            parsed.error ?? t("geminiConfig.noCommonConfigToApply"),
          );
          setUseCommonConfig(false);
          return;
        }

        setCommonConfigError("");
        setUseCommonConfig(true);
        return;
      }

      // meta 未定义，回退到内容检测
      // Gemini 使用 JSON 格式的 env，检测通用配置片段是否已应用
      const parsed = parseSnippetEnv(commonConfigSnippet);
      if (parsed.error || Object.keys(parsed.env).length === 0) {
        setUseCommonConfig(false);
      } else {
        // 检查当前 env 是否包含通用配置中的所有键值对
        const currentEnv = envStringToObj(envValue);
        const allKeysMatch = Object.entries(parsed.env).every(
          ([key, value]) => currentEnv[key] === value,
        );
        setUseCommonConfig(allKeysMatch);
      }
    }
  }, [
    initialData,
    isLoading,
    commonConfigSnippet,
    parseSnippetEnv,
    envStringToObj,
    envValue,
    t,
  ]);

  // 新建模式：如果通用配置片段存在且有效，默认启用
  useEffect(() => {
    // 仅新建模式、加载完成、尚未初始化过
    if (!initialData && !isLoading && !hasInitializedNewMode.current) {
      hasInitializedNewMode.current = true;

      const parsed = parseSnippetEnv(commonConfigSnippet);
      if (parsed.error) return;
      const hasContent = Object.keys(parsed.env).length > 0;
      if (!hasContent) return;

      setUseCommonConfig(true);
      const currentEnv = envStringToObj(envValue);
      const merged = applySnippetToEnv(currentEnv, parsed.env);
      const nextEnvString = envObjToString(merged);

      isUpdatingFromCommonConfig.current = true;
      onEnvChange(nextEnvString);
      setTimeout(() => {
        isUpdatingFromCommonConfig.current = false;
      }, 0);
    }
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
      hasUserToggledCommonConfig.current = true;
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
    (value: string) => {
      const previousSnippet = commonConfigSnippet;
      setCommonConfigSnippetState(value);

      if (!value.trim()) {
        const saveId = ++saveSequenceRef.current;
        setCommonConfigError("");
        // 保存到 config.json（清空）
        enqueueSave(() => configApi.setCommonConfigSnippet("gemini", ""))
          .then(() => {
            if (saveSequenceRef.current !== saveId) return;
            // 清空时也需要同步：移除所有供应商的通用配置片段
            configApi.syncCommonConfigToProviders(
              "gemini",
              previousSnippet,
              "", // newSnippet 为空表示移除
              replaceGeminiCommonConfigSnippet,
              currentProviderId,
              (result) => {
                if (saveSequenceRef.current !== saveId) return;
                if (result.error) {
                  toast.error(t("providerForm.commonConfigSyncFailed"));
                }
              },
            );
          })
          .catch((error) => {
            if (saveSequenceRef.current !== saveId) return;
            console.error("保存 Gemini 通用配置失败:", error);
            setCommonConfigError(
              t("geminiConfig.saveFailed", { error: String(error) }),
            );
          });

        if (useCommonConfig) {
          const parsed = parseSnippetEnv(previousSnippet);
          if (!parsed.error && Object.keys(parsed.env).length > 0) {
            const currentEnv = envStringToObj(envValue);
            const updatedEnv = removeSnippetFromEnv(currentEnv, parsed.env);
            onEnvChange(envObjToString(updatedEnv));
          }
          setUseCommonConfig(false);
        }
        return;
      }

      // 校验 JSON 格式
      const parsed = parseSnippetEnv(value);
      if (parsed.error) {
        setCommonConfigError(parsed.error);
        return;
      }

      const saveId = ++saveSequenceRef.current;
      setCommonConfigError("");
      enqueueSave(() => configApi.setCommonConfigSnippet("gemini", value))
        .then(() => {
          if (saveSequenceRef.current !== saveId) return;
          // 保存成功后，同步更新所有启用了通用配置的供应商
          configApi.syncCommonConfigToProviders(
            "gemini",
            previousSnippet,
            value,
            replaceGeminiCommonConfigSnippet,
            currentProviderId,
            (result) => {
              if (saveSequenceRef.current !== saveId) return;
              if (result.error) {
                toast.error(t("providerForm.commonConfigSyncFailed"));
              }
            },
          );
        })
        .catch((error) => {
          if (saveSequenceRef.current !== saveId) return;
          console.error("保存 Gemini 通用配置失败:", error);
          setCommonConfigError(
            t("geminiConfig.saveFailed", { error: String(error) }),
          );
        });

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
    },
    [
      applySnippetToEnv,
      commonConfigSnippet,
      currentProviderId,
      envObjToString,
      envStringToObj,
      envValue,
      onEnvChange,
      enqueueSave,
      parseSnippetEnv,
      removeSnippetFromEnv,
      t,
      useCommonConfig,
    ],
  );

  // 当 env 变化时检查是否包含通用配置（避免通过通用配置更新时反复覆盖）
  useEffect(() => {
    if (isUpdatingFromCommonConfig.current || isLoading) {
      return;
    }
    const metaByApp = initialData?.meta?.commonConfigEnabledByApp;
    const hasExplicitMeta =
      metaByApp?.gemini !== undefined ||
      initialData?.meta?.commonConfigEnabled !== undefined;
    if (hasExplicitMeta || hasUserToggledCommonConfig.current) {
      return;
    }
    const parsed = parseSnippetEnv(commonConfigSnippet);
    if (parsed.error || Object.keys(parsed.env).length === 0) {
      setUseCommonConfig(false);
      return;
    }
    const envObj = envStringToObj(envValue);
    const hasCommon = Object.entries(parsed.env).every(
      ([key, value]) => envObj[key] === value,
    );
    setUseCommonConfig(hasCommon);
  }, [
    envValue,
    commonConfigSnippet,
    envStringToObj,
    isLoading,
    parseSnippetEnv,
    initialData,
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

  return {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    isLoading,
    isExtracting,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
    handleExtract,
  };
}

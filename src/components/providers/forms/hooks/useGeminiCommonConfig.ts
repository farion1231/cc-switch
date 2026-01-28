import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { configApi } from "@/lib/api";
import {
  parseGeminiCommonConfigSnippet,
  GEMINI_CONFIG_ERROR_CODES,
} from "@/utils/providerConfigUtils";
import { computeFinalConfig, extractDifference } from "@/utils/configMerge";
import type { ProviderMeta } from "@/types";

const LEGACY_STORAGE_KEY = "cc-switch:gemini-common-config-snippet";
const DEFAULT_GEMINI_COMMON_CONFIG_SNIPPET = "{}";

interface UseGeminiCommonConfigProps {
  /**
   * 当前 env 值（字符串格式，如 "KEY=VALUE\nKEY2=VALUE2"）
   */
  envValue: string;
  /**
   * env 变化回调
   */
  onEnvChange: (env: string) => void;
  /**
   * 字符串转对象
   */
  envStringToObj: (envString: string) => Record<string, string>;
  /**
   * 对象转字符串
   */
  envObjToString: (envObj: Record<string, unknown>) => string;
  /**
   * 初始数据（编辑模式）
   */
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    meta?: ProviderMeta;
  };
  /**
   * 当前选中的预设 ID
   */
  selectedPresetId?: string;
  /**
   * 当前正在编辑的供应商 ID
   */
  currentProviderId?: string;
}

export interface UseGeminiCommonConfigReturn {
  /** 是否启用通用配置 */
  useCommonConfig: boolean;
  /** 通用配置片段 (JSON 格式) */
  commonConfigSnippet: string;
  /** 通用配置错误信息 */
  commonConfigError: string;
  /** 是否正在加载 */
  isLoading: boolean;
  /** 是否正在提取 */
  isExtracting: boolean;
  /** 通用配置开关处理函数 */
  handleCommonConfigToggle: (checked: boolean) => void;
  /** 通用配置片段变化处理函数 */
  handleCommonConfigSnippetChange: (snippet: string) => void;
  /** 从当前配置提取通用配置 */
  handleExtract: () => Promise<void>;
  /** 最终 env 对象（运行时合并结果，只读） */
  finalEnv: Record<string, string>;
  /** 是否有待保存的通用配置变更 */
  hasUnsavedCommonConfig: boolean;
  /** 获取待保存的通用配置片段（用于 handleSubmit） */
  getPendingCommonConfigSnippet: () => string | null;
  /** 标记通用配置已保存 */
  markCommonConfigSaved: () => void;
}

/**
 * 管理 Gemini 通用配置片段（重构版）
 *
 * 新架构：
 * - envValue：传入当前 env 字符串
 * - commonConfigSnippet：存储在数据库中的通用配置片段
 * - finalEnv：运行时计算 = merge(commonConfig, customEnv)
 * - 开启/关闭通用配置只改变 enabled 状态，不修改 envValue
 */
export function useGeminiCommonConfig({
  envValue,
  onEnvChange,
  envStringToObj,
  envObjToString,
  initialData,
  selectedPresetId,
  // currentProviderId is reserved for future use
}: UseGeminiCommonConfigProps): UseGeminiCommonConfigReturn {
  const { t } = useTranslation();

  // 内部管理的通用配置启用状态
  const [useCommonConfig, setUseCommonConfig] = useState(false);

  // 通用配置片段（从数据库加载）
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_GEMINI_COMMON_CONFIG_SNIPPET,
  );
  const [commonConfigError, setCommonConfigError] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isExtracting, setIsExtracting] = useState(false);
  // 是否有待保存的通用配置变更
  const [hasUnsavedCommonConfig, setHasUnsavedCommonConfig] = useState(false);

  // 用于跟踪编辑模式是否已初始化
  const hasInitializedEditMode = useRef(false);
  // 用于跟踪新建模式是否已初始化
  const hasInitializedNewMode = useRef(false);

  // 将 envValue 字符串转换为对象
  const customEnv = useMemo(
    () => envStringToObj(envValue),
    [envValue, envStringToObj],
  );

  // 当预设变化时，重置初始化标记
  useEffect(() => {
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
  }, [selectedPresetId]);

  // 解析通用配置片段 - 使用共享解析器
  // 支持三种格式: ENV (KEY=VALUE), 扁平 JSON, 包裹 JSON {"env":{...}}
  const parseSnippetEnv = useCallback(
    (
      snippetString: string,
    ): { env: Record<string, string>; error?: string } => {
      const result = parseGeminiCommonConfigSnippet(snippetString, {
        strictForbiddenKeys: true,
      });

      if (result.error) {
        // Map error codes to i18n keys
        if (result.error.startsWith(GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS)) {
          const keys = result.error.split(": ")[1] ?? result.error;
          return {
            env: {},
            error: t("geminiConfig.commonConfigInvalidKeys", { keys }),
          };
        }
        if (
          result.error.startsWith(GEMINI_CONFIG_ERROR_CODES.VALUE_NOT_STRING)
        ) {
          return {
            env: {},
            error: t("geminiConfig.commonConfigInvalidValues"),
          };
        }
        // Generic format error (NOT_OBJECT, ENV_NOT_OBJECT, or parse failure)
        return { env: {}, error: t("geminiConfig.invalidJsonFormat") };
      }

      return { env: result.env };
    },
    [t],
  );

  // 获取片段应用错误
  const getSnippetApplyError = useCallback(
    (snippet: string) => {
      const parsed = parseSnippetEnv(snippet);
      if (parsed.error) {
        return parsed.error;
      }
      if (Object.keys(parsed.env).length === 0) {
        return t("geminiConfig.noCommonConfigToApply");
      }
      return "";
    },
    [parseSnippetEnv, t],
  );

  // ============================================================================
  // 加载通用配置片段（从数据库，支持 localStorage 迁移）
  // ============================================================================
  useEffect(() => {
    let mounted = true;

    const loadSnippet = async () => {
      try {
        const snippet = await configApi.getCommonConfigSnippet("gemini");

        if (snippet && snippet.trim()) {
          if (mounted) {
            setCommonConfigSnippetState(snippet);
          }
        } else {
          // 尝试从 localStorage 迁移
          if (typeof window !== "undefined") {
            try {
              const legacySnippet =
                window.localStorage.getItem(LEGACY_STORAGE_KEY);
              if (legacySnippet && legacySnippet.trim()) {
                const parsed = parseSnippetEnv(legacySnippet);
                if (!parsed.error) {
                  await configApi.setCommonConfigSnippet(
                    "gemini",
                    legacySnippet,
                  );
                  if (mounted) {
                    setCommonConfigSnippetState(legacySnippet);
                  }
                  window.localStorage.removeItem(LEGACY_STORAGE_KEY);
                  console.log(
                    "[迁移] Gemini 通用配置已从 localStorage 迁移到数据库",
                  );
                }
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

  // ============================================================================
  // 编辑模式初始化：从 meta 读取启用状态
  // ============================================================================
  useEffect(() => {
    if (initialData && !isLoading && !hasInitializedEditMode.current) {
      hasInitializedEditMode.current = true;

      const metaByApp = initialData.meta?.commonConfigEnabledByApp;
      const resolvedMetaEnabled =
        metaByApp?.gemini ?? initialData.meta?.commonConfigEnabled;

      if (resolvedMetaEnabled !== undefined) {
        if (!resolvedMetaEnabled) {
          setUseCommonConfig(false);
          return;
        }
        const snippetError = getSnippetApplyError(commonConfigSnippet);
        if (snippetError) {
          setCommonConfigError(snippetError);
          setUseCommonConfig(false);
          return;
        }
        setCommonConfigError("");
        setUseCommonConfig(true);
      } else {
        setUseCommonConfig(false);
      }
    }
  }, [initialData, isLoading, commonConfigSnippet, getSnippetApplyError]);

  // ============================================================================
  // 新建模式初始化：如果通用配置有效，默认启用
  // ============================================================================
  useEffect(() => {
    if (!initialData && !isLoading && !hasInitializedNewMode.current) {
      hasInitializedNewMode.current = true;

      const parsed = parseSnippetEnv(commonConfigSnippet);
      if (!parsed.error && Object.keys(parsed.env).length > 0) {
        setUseCommonConfig(true);
      }
    }
  }, [initialData, commonConfigSnippet, isLoading, parseSnippetEnv]);

  // ============================================================================
  // 计算最终 env（运行时合并）
  // ============================================================================
  const finalEnv = useMemo((): Record<string, string> => {
    if (!useCommonConfig) {
      return customEnv;
    }

    const parsed = parseSnippetEnv(commonConfigSnippet);
    if (parsed.error || Object.keys(parsed.env).length === 0) {
      return customEnv;
    }

    // 通用配置作为 base，自定义 env 覆盖
    const merged = computeFinalConfig(
      customEnv as Record<string, unknown>,
      parsed.env as Record<string, unknown>,
      true,
    );

    // 转换回 Record<string, string>
    const result: Record<string, string> = {};
    for (const [key, value] of Object.entries(merged)) {
      if (typeof value === "string") {
        result[key] = value;
      }
    }
    return result;
  }, [customEnv, commonConfigSnippet, useCommonConfig, parseSnippetEnv]);

  // ============================================================================
  // 处理通用配置开关
  // ============================================================================
  const handleCommonConfigToggle = useCallback(
    (checked: boolean) => {
      if (checked) {
        const snippetError = getSnippetApplyError(commonConfigSnippet);
        if (snippetError) {
          setCommonConfigError(snippetError);
          setUseCommonConfig(false);
          return;
        }
      }
      setCommonConfigError("");
      setUseCommonConfig(checked);
      // 新架构：不修改 envValue，只改变 enabled 状态
    },
    [commonConfigSnippet, getSnippetApplyError],
  );

  // ============================================================================
  // 处理通用配置片段变化（延迟保存模式：只更新本地状态，实际保存在表单提交时）
  // ============================================================================
  const handleCommonConfigSnippetChange = useCallback(
    (value: string) => {
      setCommonConfigSnippetState(value);

      if (!value.trim()) {
        setCommonConfigError("");
        setHasUnsavedCommonConfig(true);
        return;
      }

      // JSON 格式校验
      const parsed = parseSnippetEnv(value);
      if (parsed.error) {
        setCommonConfigError(parsed.error);
        return;
      }

      setCommonConfigError("");
      setHasUnsavedCommonConfig(true);

      // 注意：新架构下不再需要同步到其他供应商的 settingsConfig
      // 因为 finalEnv 是运行时计算的
    },
    [parseSnippetEnv],
  );

  // ============================================================================
  // 从当前最终 env 提取通用配置片段（延迟保存模式：只更新本地状态，实际保存在表单提交时）
  // ============================================================================
  const handleExtract = useCallback(async () => {
    setIsExtracting(true);
    setCommonConfigError("");

    try {
      const extracted = await configApi.extractCommonConfigSnippet("gemini", {
        settingsConfig: JSON.stringify({
          env: finalEnv,
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

      // 更新片段状态（延迟保存：不立即调用后端 API）
      setCommonConfigSnippetState(extracted);
      setHasUnsavedCommonConfig(true);

      // 提取成功后，从 customEnv 中移除与 extracted 相同的部分
      const diffResult = extractDifference(
        customEnv as Record<string, unknown>,
        parsed.env as Record<string, unknown>,
      );
      const newCustomEnv: Record<string, string> = {};
      for (const [key, value] of Object.entries(diffResult.customConfig)) {
        if (typeof value === "string") {
          newCustomEnv[key] = value;
        }
      }
      onEnvChange(envObjToString(newCustomEnv));
      // Notify user that config was modified (提示用户需要保存)
      toast.success(
        t("geminiConfig.extractSuccessNeedSave", {
          defaultValue: "已提取通用配置，点击保存按钮完成保存",
        }),
      );
    } catch (error) {
      console.error("提取 Gemini 通用配置失败:", error);
      setCommonConfigError(
        t("geminiConfig.extractFailed", { error: String(error) }),
      );
    } finally {
      setIsExtracting(false);
    }
  }, [finalEnv, customEnv, onEnvChange, envObjToString, parseSnippetEnv, t]);

  // ============================================================================
  // 获取待保存的通用配置片段（用于 handleSubmit 中统一保存）
  // ============================================================================
  const getPendingCommonConfigSnippet = useCallback(() => {
    return hasUnsavedCommonConfig ? commonConfigSnippet : null;
  }, [hasUnsavedCommonConfig, commonConfigSnippet]);

  // ============================================================================
  // 标记通用配置已保存
  // ============================================================================
  const markCommonConfigSaved = useCallback(() => {
    setHasUnsavedCommonConfig(false);
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
    finalEnv,
    hasUnsavedCommonConfig,
    getPendingCommonConfigSnippet,
    markCommonConfigSaved,
  };
}

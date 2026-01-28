import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { validateJsonConfig } from "@/utils/providerConfigUtils";
import { configApi } from "@/lib/api";
import {
  computeFinalConfig,
  extractDifference,
  isPlainObject,
} from "@/utils/configMerge";
import type { ProviderMeta } from "@/types";

const LEGACY_STORAGE_KEY = "cc-switch:common-config-snippet";
const DEFAULT_COMMON_CONFIG_SNIPPET = `{
  "includeCoAuthoredBy": false
}`;

interface UseCommonConfigSnippetProps {
  /**
   * 当前配置（用于显示和运行时合并）
   * 新架构：传入自定义配置，返回最终配置
   */
  settingsConfig: string;
  /**
   * 配置变化回调
   */
  onConfigChange: (config: string) => void;
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
   * 当 false 时跳过所有逻辑，返回禁用状态。默认：true
   */
  enabled?: boolean;
  /**
   * 当前正在编辑的供应商 ID
   */
  currentProviderId?: string;
}

export interface UseCommonConfigSnippetReturn {
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
  /** 最终配置（运行时合并结果，只读） */
  finalConfig: string;
  /** 是否有待保存的通用配置变更 */
  hasUnsavedCommonConfig: boolean;
  /** 获取待保存的通用配置片段（用于 handleSubmit） */
  getPendingCommonConfigSnippet: () => string | null;
  /** 标记通用配置已保存 */
  markCommonConfigSaved: () => void;
}

/**
 * 管理 Claude 通用配置片段（重构版）
 *
 * 新架构：
 * - settingsConfig：传入自定义配置（供应商独有部分）
 * - commonConfigSnippet：存储在数据库中的通用配置片段
 * - finalConfig：运行时计算 = merge(commonConfig, settingsConfig)
 * - 开启/关闭通用配置只改变 enabled 状态，不修改 settingsConfig
 */
export function useCommonConfigSnippet({
  settingsConfig,
  onConfigChange,
  initialData,
  selectedPresetId,
  enabled = true,
  // currentProviderId is reserved for future use
}: UseCommonConfigSnippetProps): UseCommonConfigSnippetReturn {
  const { t } = useTranslation();

  // 内部管理的通用配置启用状态
  const [useCommonConfig, setUseCommonConfig] = useState(false);

  // 通用配置片段（从数据库加载）
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_COMMON_CONFIG_SNIPPET,
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

  // 当预设变化时，重置初始化标记
  useEffect(() => {
    if (!enabled) return;
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
  }, [selectedPresetId, enabled]);

  // 解析 JSON 配置片段
  const parseSnippet = useCallback(
    (
      snippetString: string,
    ): { config: Record<string, unknown>; error?: string } => {
      const trimmed = snippetString.trim();
      if (!trimmed) {
        return { config: {} };
      }

      try {
        const parsed = JSON.parse(trimmed);
        if (!isPlainObject(parsed)) {
          return { config: {}, error: t("claudeConfig.invalidJsonFormat") };
        }
        return { config: parsed };
      } catch {
        return { config: {}, error: t("claudeConfig.invalidJsonFormat") };
      }
    },
    [t],
  );

  // 获取片段应用错误
  const getSnippetApplyError = useCallback(
    (snippet: string) => {
      if (!snippet.trim()) {
        return t("claudeConfig.noCommonConfigToApply");
      }
      const validationError = validateJsonConfig(snippet, "通用配置片段");
      if (validationError) {
        return validationError;
      }
      try {
        const parsed = JSON.parse(snippet) as Record<string, unknown>;
        if (Object.keys(parsed).length === 0) {
          return t("claudeConfig.noCommonConfigToApply");
        }
      } catch {
        return t("claudeConfig.noCommonConfigToApply");
      }
      return "";
    },
    [t],
  );

  // ============================================================================
  // 加载通用配置片段（从数据库，支持 localStorage 迁移）
  // ============================================================================
  useEffect(() => {
    if (!enabled) {
      setIsLoading(false);
      return;
    }
    let mounted = true;

    const loadSnippet = async () => {
      try {
        const snippet = await configApi.getCommonConfigSnippet("claude");

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
                const parsed = parseSnippet(legacySnippet);
                if (!parsed.error) {
                  await configApi.setCommonConfigSnippet(
                    "claude",
                    legacySnippet,
                  );
                  if (mounted) {
                    setCommonConfigSnippetState(legacySnippet);
                  }
                  window.localStorage.removeItem(LEGACY_STORAGE_KEY);
                  console.log(
                    "[迁移] Claude 通用配置已从 localStorage 迁移到数据库",
                  );
                }
              }
            } catch (e) {
              console.warn("[迁移] 从 localStorage 迁移失败:", e);
            }
          }
        }
      } catch (error) {
        console.error("加载通用配置失败:", error);
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
  }, [enabled, parseSnippet]);

  // ============================================================================
  // 编辑模式初始化：从 meta 读取启用状态
  // ============================================================================
  useEffect(() => {
    if (!enabled) return;
    if (initialData && !isLoading && !hasInitializedEditMode.current) {
      hasInitializedEditMode.current = true;

      const metaByApp = initialData.meta?.commonConfigEnabledByApp;
      const resolvedMetaEnabled =
        metaByApp?.claude ?? initialData.meta?.commonConfigEnabled;

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
  }, [
    enabled,
    initialData,
    isLoading,
    commonConfigSnippet,
    getSnippetApplyError,
  ]);

  // ============================================================================
  // 新建模式初始化：如果通用配置有效，默认启用
  // ============================================================================
  useEffect(() => {
    if (!enabled) return;
    if (!initialData && !isLoading && !hasInitializedNewMode.current) {
      hasInitializedNewMode.current = true;

      const parsed = parseSnippet(commonConfigSnippet);
      if (!parsed.error && Object.keys(parsed.config).length > 0) {
        setUseCommonConfig(true);
      }
    }
  }, [enabled, initialData, commonConfigSnippet, isLoading, parseSnippet]);

  // ============================================================================
  // 计算最终配置（运行时合并）
  // ============================================================================
  const finalConfig = useMemo((): string => {
    if (!enabled) return settingsConfig;

    try {
      const customParsed = settingsConfig ? JSON.parse(settingsConfig) : {};
      if (!isPlainObject(customParsed)) {
        return settingsConfig;
      }

      if (!useCommonConfig) {
        return settingsConfig;
      }

      const snippetParsed = parseSnippet(commonConfigSnippet);
      if (
        snippetParsed.error ||
        Object.keys(snippetParsed.config).length === 0
      ) {
        return settingsConfig;
      }

      // 通用配置作为 base，自定义配置覆盖
      const merged = computeFinalConfig(
        customParsed,
        snippetParsed.config,
        true,
      );

      return JSON.stringify(merged, null, 2);
    } catch {
      return settingsConfig;
    }
  }, [
    enabled,
    settingsConfig,
    commonConfigSnippet,
    useCommonConfig,
    parseSnippet,
  ]);

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
      // 新架构：不修改 settingsConfig，只改变 enabled 状态
    },
    [commonConfigSnippet, getSnippetApplyError],
  );

  // ============================================================================
  // 处理通用配置片段变化（延迟保存模式：只更新本地状态，实际保存在表单提交时）
  // ============================================================================
  const handleCommonConfigSnippetChange = useCallback((value: string) => {
    setCommonConfigSnippetState(value);

    if (!value.trim()) {
      setCommonConfigError("");
      setHasUnsavedCommonConfig(true);
      return;
    }

    // JSON 格式校验
    const validationError = validateJsonConfig(value, "通用配置片段");
    if (validationError) {
      setCommonConfigError(validationError);
      return;
    }

    setCommonConfigError("");
    setHasUnsavedCommonConfig(true);

    // 注意：新架构下不再需要同步到其他供应商的 settingsConfig
    // 因为 finalConfig 是运行时计算的
  }, []);

  // ============================================================================
  // 从当前最终配置提取通用配置片段（延迟保存模式：只更新本地状态，实际保存在表单提交时）
  // ============================================================================
  const handleExtract = useCallback(async () => {
    setIsExtracting(true);
    setCommonConfigError("");

    try {
      const extracted = await configApi.extractCommonConfigSnippet("claude", {
        settingsConfig: finalConfig,
      });

      if (!extracted || extracted === "{}") {
        setCommonConfigError(t("claudeConfig.extractNoCommonConfig"));
        return;
      }

      // 验证 JSON 格式
      const validationError = validateJsonConfig(extracted, "提取的配置");
      if (validationError) {
        setCommonConfigError(t("claudeConfig.extractedConfigInvalid"));
        return;
      }

      // 更新片段状态（延迟保存：不立即调用后端 API）
      setCommonConfigSnippetState(extracted);
      setHasUnsavedCommonConfig(true);

      // 提取成功后，从 settingsConfig 中移除与 extracted 相同的部分
      try {
        const customParsed = settingsConfig ? JSON.parse(settingsConfig) : {};
        const extractedParsed = JSON.parse(extracted);

        if (isPlainObject(customParsed) && isPlainObject(extractedParsed)) {
          const diffResult = extractDifference(customParsed, extractedParsed);
          onConfigChange(JSON.stringify(diffResult.customConfig, null, 2));
          // Notify user that config was modified (提示用户需要保存)
          toast.success(
            t("claudeConfig.extractSuccessNeedSave", {
              defaultValue: "已提取通用配置，点击保存按钮完成保存",
            }),
          );
        }
      } catch (parseError) {
        console.warn(
          "[Extract] Failed to update settingsConfig after extract:",
          parseError,
        );
      }
    } catch (error) {
      console.error("提取通用配置失败:", error);
      setCommonConfigError(
        t("claudeConfig.extractFailed", { error: String(error) }),
      );
    } finally {
      setIsExtracting(false);
    }
  }, [finalConfig, settingsConfig, onConfigChange, t]);

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
    finalConfig,
    hasUnsavedCommonConfig,
    getPendingCommonConfigSnippet,
    markCommonConfigSaved,
  };
}

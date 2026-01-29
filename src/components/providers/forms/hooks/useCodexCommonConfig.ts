import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import TOML from "smol-toml";
import { configApi } from "@/lib/api";
import {
  computeFinalTomlConfig,
  extractTomlDifference,
} from "@/utils/tomlConfigMerge";
import type { ProviderMeta } from "@/types";

const LEGACY_STORAGE_KEY = "cc-switch:codex-common-config-snippet";
const DEFAULT_CODEX_COMMON_CONFIG_SNIPPET = `# Common Codex config
# Add your common TOML configuration here`;

/** TOML 校验错误码 */
export type TomlValidationErrorCode =
  | "TOML_SYNTAX_ERROR"
  | "TOML_PARSE_FAILED"
  | "";

/**
 * 校验 TOML 格式
 * @param tomlText - 待校验的 TOML 文本
 * @returns 错误码，如果校验通过则返回空字符串
 */
function validateTomlFormat(tomlText: string): TomlValidationErrorCode {
  // 空字符串或仅包含注释/空行视为合法
  const lines = tomlText.split("\n");
  const hasContent = lines.some((line) => {
    const trimmed = line.trim();
    return trimmed && !trimmed.startsWith("#");
  });
  if (!hasContent) {
    return "";
  }

  try {
    TOML.parse(tomlText);
    return "";
  } catch {
    return "TOML_SYNTAX_ERROR";
  }
}

interface UseCodexCommonConfigProps {
  /**
   * 当前 Codex 配置（JSON 格式，包含 auth 和 config）
   */
  codexConfig: string;
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
   * 当前正在编辑的供应商 ID
   */
  currentProviderId?: string;
}

export interface UseCodexCommonConfigReturn {
  /** 是否启用通用配置 */
  useCommonConfig: boolean;
  /** 通用配置片段 (TOML 格式) */
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
 * 管理 Codex 通用配置片段（重构版）
 *
 * 新架构：
 * - codexConfig：传入当前配置（JSON 格式，包含 auth 和 config）
 * - commonConfigSnippet：存储在数据库中的通用 TOML 配置片段
 * - finalConfig：运行时计算 = merge(commonConfig, config)
 * - 开启/关闭通用配置只改变 enabled 状态，不修改 codexConfig
 */
export function useCodexCommonConfig({
  codexConfig,
  onConfigChange,
  initialData,
  selectedPresetId,
  // currentProviderId is reserved for future use
}: UseCodexCommonConfigProps): UseCodexCommonConfigReturn {
  const { t } = useTranslation();

  // 内部管理的通用配置启用状态
  const [useCommonConfig, setUseCommonConfig] = useState(false);

  // 通用配置片段（从数据库加载）
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_CODEX_COMMON_CONFIG_SNIPPET,
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

  // 检查 TOML 片段是否有实质内容
  const hasSnippetContent = useCallback((snippet: string) => {
    const lines = snippet.split("\n");
    return lines.some((line) => {
      const trimmed = line.trim();
      return trimmed && !trimmed.startsWith("#");
    });
  }, []);

  // 从 codexConfig 提取 config 字段（TOML 格式）
  // 注意：codexConfig 可能是以下两种格式之一：
  // 1. 直接的 TOML 字符串（来自 useCodexConfigState）
  // 2. JSON 字符串 { auth: {...}, config: "..." }（旧的 settingsConfig 格式）
  const extractConfigToml = useCallback((configInput: string): string => {
    if (!configInput || !configInput.trim()) {
      return "";
    }

    // 尝试解析为 JSON（旧格式）
    try {
      const parsed = JSON.parse(configInput);
      if (typeof parsed?.config === "string") {
        return parsed.config;
      }
      // 如果是 JSON 对象但没有 config 字段，返回空
      if (typeof parsed === "object" && parsed !== null) {
        return "";
      }
    } catch {
      // JSON 解析失败，说明是直接的 TOML 字符串
    }

    // 直接返回作为 TOML 字符串
    return configInput;
  }, []);

  // 当预设变化时，重置初始化标记
  useEffect(() => {
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
  }, [selectedPresetId]);

  // ============================================================================
  // 加载通用配置片段（从数据库，支持 localStorage 迁移）
  // ============================================================================
  useEffect(() => {
    let mounted = true;

    const loadSnippet = async () => {
      try {
        const snippet = await configApi.getCommonConfigSnippet("codex");

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
                const tomlError = validateTomlFormat(legacySnippet);
                if (!tomlError) {
                  await configApi.setCommonConfigSnippet(
                    "codex",
                    legacySnippet,
                  );
                  if (mounted) {
                    setCommonConfigSnippetState(legacySnippet);
                  }
                  window.localStorage.removeItem(LEGACY_STORAGE_KEY);
                  console.log(
                    "[迁移] Codex 通用配置已从 localStorage 迁移到数据库",
                  );
                }
              }
            } catch (e) {
              console.warn("[迁移] 从 localStorage 迁移失败:", e);
            }
          }
        }
      } catch (error) {
        console.error("加载 Codex 通用配置失败:", error);
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
  }, []);

  // ============================================================================
  // 编辑模式初始化：从 meta 读取启用状态
  // ============================================================================
  useEffect(() => {
    if (initialData && !isLoading && !hasInitializedEditMode.current) {
      hasInitializedEditMode.current = true;

      const metaByApp = initialData.meta?.commonConfigEnabledByApp;
      const resolvedMetaEnabled =
        metaByApp?.codex ?? initialData.meta?.commonConfigEnabled;

      if (resolvedMetaEnabled !== undefined) {
        if (!resolvedMetaEnabled) {
          setUseCommonConfig(false);
          return;
        }
        if (!hasSnippetContent(commonConfigSnippet)) {
          setCommonConfigError(t("codexConfig.noCommonConfigToApply"));
          setUseCommonConfig(false);
          return;
        }
        const tomlError = validateTomlFormat(commonConfigSnippet);
        if (tomlError) {
          setCommonConfigError(
            t("codexConfig.tomlFormatError", { defaultValue: "TOML 格式错误" }),
          );
          setUseCommonConfig(false);
          return;
        }
        setCommonConfigError("");
        setUseCommonConfig(true);
      } else {
        setUseCommonConfig(false);
      }
    }
  }, [initialData, isLoading, commonConfigSnippet, hasSnippetContent, t]);

  // ============================================================================
  // 新建模式初始化：如果通用配置有效，默认启用
  // ============================================================================
  useEffect(() => {
    if (!initialData && !isLoading && !hasInitializedNewMode.current) {
      hasInitializedNewMode.current = true;

      if (hasSnippetContent(commonConfigSnippet)) {
        const tomlError = validateTomlFormat(commonConfigSnippet);
        if (!tomlError) {
          setUseCommonConfig(true);
        }
      }
    }
  }, [initialData, commonConfigSnippet, isLoading, hasSnippetContent]);

  // ============================================================================
  // 计算最终配置（运行时合并）
  // ============================================================================
  const finalConfig = useMemo((): string => {
    const customToml = extractConfigToml(codexConfig);

    if (!useCommonConfig) {
      return customToml;
    }

    const result = computeFinalTomlConfig(
      customToml,
      commonConfigSnippet,
      true,
    );

    if (result.error) {
      // 合并失败时返回原配置
      return customToml;
    }

    return result.finalConfig;
  }, [codexConfig, commonConfigSnippet, useCommonConfig, extractConfigToml]);

  // ============================================================================
  // 处理通用配置开关
  // ============================================================================
  const handleCommonConfigToggle = useCallback(
    (checked: boolean) => {
      if (checked) {
        if (!hasSnippetContent(commonConfigSnippet)) {
          setCommonConfigError(t("codexConfig.noCommonConfigToApply"));
          setUseCommonConfig(false);
          return;
        }
        const tomlError = validateTomlFormat(commonConfigSnippet);
        if (tomlError) {
          setCommonConfigError(
            t("codexConfig.tomlFormatError", { defaultValue: "TOML 格式错误" }),
          );
          setUseCommonConfig(false);
          return;
        }
      }
      setCommonConfigError("");
      setUseCommonConfig(checked);
      // 新架构：不修改 codexConfig，只改变 enabled 状态
    },
    [commonConfigSnippet, hasSnippetContent, t],
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

      // TOML 格式校验
      const tomlError = validateTomlFormat(value);
      if (tomlError) {
        setCommonConfigError(
          t("codexConfig.tomlSyntaxError", {
            defaultValue: "TOML 格式错误，请检查语法",
          }),
        );
        return;
      }

      setCommonConfigError("");
      setHasUnsavedCommonConfig(true);

      // 注意：新架构下不再需要同步到其他供应商的 settingsConfig
      // 因为 finalConfig 是运行时计算的
    },
    [t],
  );

  // ============================================================================
  // 从当前最终配置提取通用配置片段（延迟保存模式：只更新本地状态，实际保存在表单提交时）
  // ============================================================================
  const handleExtract = useCallback(async () => {
    setIsExtracting(true);
    setCommonConfigError("");

    try {
      const extracted = await configApi.extractCommonConfigSnippet("codex", {
        settingsConfig: JSON.stringify({
          config: finalConfig ?? "",
        }),
      });

      if (!extracted || !extracted.trim()) {
        setCommonConfigError(t("codexConfig.extractNoCommonConfig"));
        return;
      }

      // 验证 TOML 格式
      const tomlError = validateTomlFormat(extracted);
      if (tomlError) {
        setCommonConfigError(
          t("codexConfig.extractedTomlInvalid", {
            defaultValue: "提取的配置 TOML 格式错误",
          }),
        );
        return;
      }

      // 更新片段状态（延迟保存：不立即调用后端 API）
      setCommonConfigSnippetState(extracted);
      setHasUnsavedCommonConfig(true);

      // 提取成功后，从 config 中移除与 extracted 相同的部分
      const customToml = extractConfigToml(codexConfig);
      const diffResult = extractTomlDifference(customToml, extracted);
      if (!diffResult.error) {
        // 更新 codexConfig
        // codexConfig 可能是两种格式：
        // 1. 纯 TOML 字符串（来自 useCodexConfigState）
        // 2. JSON 字符串 { auth: {...}, config: "..." }（旧格式）
        let updated = false;
        try {
          const parsed = JSON.parse(codexConfig);
          if (typeof parsed?.config === "string") {
            // JSON 格式，更新 config 字段
            parsed.config = diffResult.customToml;
            onConfigChange(JSON.stringify(parsed, null, 2));
            updated = true;
          }
        } catch {
          // JSON 解析失败，说明是纯 TOML 字符串
        }

        // 如果是纯 TOML 字符串，直接更新
        if (!updated) {
          onConfigChange(diffResult.customToml);
        }

        // Notify user that config was modified (提示用户需要保存)
        toast.success(
          t("codexConfig.extractSuccessNeedSave", {
            defaultValue: "已提取通用配置，点击保存按钮完成保存",
          }),
        );
      }
    } catch (error) {
      console.error("提取 Codex 通用配置失败:", error);
      setCommonConfigError(
        t("codexConfig.extractFailed", { error: String(error) }),
      );
    } finally {
      setIsExtracting(false);
    }
  }, [finalConfig, codexConfig, onConfigChange, extractConfigToml, t]);

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

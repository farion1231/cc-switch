/**
 * 通用配置管理基础 Hook
 *
 * 提供加载、切换、保存、提取等通用逻辑，通过 Adapter 注入格式特定处理。
 * 支持 Claude (JSON), Codex (TOML), Gemini (ENV/JSON) 三种格式。
 */

import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { configApi } from "@/lib/api";
import type { ProviderMeta } from "@/types";

// ============================================================================
// 类型定义
// ============================================================================

/** 应用类型 */
export type CommonConfigAppKey = "claude" | "codex" | "gemini";

/** 解析结果 */
export interface ParseResult<T> {
  config: T | null;
  error: string | null;
}

/** 合并结果 */
export interface MergeResult<T> {
  merged: T;
  error?: string;
}

/** 差异提取结果 */
export interface ExtractResult<T> {
  custom: T;
  hasCommonKeys: boolean;
  error?: string;
}

/**
 * 格式适配器接口
 *
 * 每种格式（JSON/TOML/ENV）需要实现此接口
 */
export interface CommonConfigAdapter<TConfig, TFinal> {
  /** 应用标识 */
  appKey: CommonConfigAppKey;

  /** 默认片段内容 */
  defaultSnippet: string;

  /** localStorage 迁移 key (可选) */
  legacyStorageKey?: string;

  /**
   * 解析片段字符串为配置对象
   * @param snippet - 原始片段字符串
   * @returns 解析结果
   */
  parseSnippet: (snippet: string) => ParseResult<TConfig>;

  /**
   * 验证片段是否有有效内容（非空、非纯注释）
   * @param snippet - 片段字符串
   * @returns 是否有有效内容
   */
  hasValidContent: (snippet: string) => boolean;

  /**
   * 检查配置是否包含指定的片段内容
   * 用于检测供应商配置是否已应用通用配置
   * @param configStr - 供应商的配置字符串 (settingsConfig)
   * @param snippetStr - 通用配置片段字符串
   * @returns 是否包含片段内容
   */
  hasContent: (configStr: string, snippetStr: string) => boolean;

  /**
   * 获取片段应用错误（用于 toggle 时验证）
   * @param snippet - 片段字符串
   * @param t - i18n 翻译函数
   * @returns 错误信息，无错误返回空字符串
   */
  getApplyError: (
    snippet: string,
    t: (key: string, options?: Record<string, unknown>) => string,
  ) => string;

  /**
   * 从表单输入解析当前配置
   * @param input - 表单输入值
   * @returns 解析后的配置对象
   */
  parseInput: (input: string) => TConfig;

  /**
   * 计算最终合并配置
   * @param custom - 自定义配置
   * @param common - 通用配置
   * @param enabled - 是否启用
   * @returns 合并后的最终配置
   */
  computeFinal: (custom: TConfig, common: TConfig, enabled: boolean) => TFinal;

  /**
   * 提取差异（从自定义配置中移除与通用配置相同的部分）
   * @param custom - 自定义配置
   * @param common - 通用配置
   * @returns 差异结果
   */
  extractDiff: (custom: TConfig, common: TConfig) => ExtractResult<TConfig>;

  /**
   * 将配置对象序列化为表单输出
   * @param config - 配置对象
   * @returns 序列化后的字符串
   */
  serializeOutput: (config: TConfig) => string;

  /**
   * 构建提取 API 的请求参数
   * @param finalValue - 最终合并后的值
   * @returns API 请求参数
   */
  buildExtractRequest: (finalValue: TFinal) => { settingsConfig: string };
}

// ============================================================================
// Props 和 Return 类型
// ============================================================================

export interface UseCommonConfigBaseProps<TConfig, TFinal> {
  /** 格式适配器 */
  adapter: CommonConfigAdapter<TConfig, TFinal>;
  /** 当前表单输入值 */
  inputValue: string;
  /** 输入变化回调 */
  onInputChange: (value: string) => void;
  /** 初始数据（编辑模式） */
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    meta?: ProviderMeta;
  };
  /** 当前选中的预设 ID */
  selectedPresetId?: string;
  /** 是否启用此 hook（默认 true） */
  enabled?: boolean;
}

export interface UseCommonConfigBaseReturn<TFinal> {
  /** 是否启用通用配置 */
  useCommonConfig: boolean;
  /** 通用配置片段 */
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
  finalValue: TFinal;
  /** 是否有待保存的通用配置变更 */
  hasUnsavedCommonConfig: boolean;
  /** 获取待保存的通用配置片段（用于 handleSubmit） */
  getPendingCommonConfigSnippet: () => string | null;
  /** 标记通用配置已保存 */
  markCommonConfigSaved: () => void;
}

// ============================================================================
// 基础 Hook 实现
// ============================================================================

export function useCommonConfigBase<TConfig, TFinal>({
  adapter,
  inputValue,
  onInputChange,
  initialData,
  selectedPresetId,
  enabled = true,
}: UseCommonConfigBaseProps<
  TConfig,
  TFinal
>): UseCommonConfigBaseReturn<TFinal> {
  const { t } = useTranslation();

  // ============================================================================
  // 状态
  // ============================================================================
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    adapter.defaultSnippet,
  );
  const [commonConfigError, setCommonConfigError] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isExtracting, setIsExtracting] = useState(false);
  const [hasUnsavedCommonConfig, setHasUnsavedCommonConfig] = useState(false);

  // 初始化跟踪
  const hasInitializedEditMode = useRef(false);
  const hasInitializedNewMode = useRef(false);

  // ============================================================================
  // 预设变化时重置初始化标记
  // ============================================================================
  useEffect(() => {
    if (!enabled) return;
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
  }, [selectedPresetId, enabled]);

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
        const snippet = await configApi.getCommonConfigSnippet(adapter.appKey);

        if (snippet && snippet.trim()) {
          if (mounted) {
            setCommonConfigSnippetState(snippet);
          }
        } else if (adapter.legacyStorageKey && typeof window !== "undefined") {
          // 尝试从 localStorage 迁移
          try {
            const legacySnippet = window.localStorage.getItem(
              adapter.legacyStorageKey,
            );
            if (legacySnippet && legacySnippet.trim()) {
              const parsed = adapter.parseSnippet(legacySnippet);
              // 只有在解析成功且有有效内容时才迁移
              // 这避免了将 "{}" 这样的空 JSON 迁移为"有效配置"
              if (!parsed.error && adapter.hasValidContent(legacySnippet)) {
                await configApi.setCommonConfigSnippet(
                  adapter.appKey,
                  legacySnippet,
                );
                if (mounted) {
                  setCommonConfigSnippetState(legacySnippet);
                }
                window.localStorage.removeItem(adapter.legacyStorageKey);
                console.log(
                  `[迁移] ${adapter.appKey} 通用配置已从 localStorage 迁移到数据库`,
                );
              } else {
                // 解析失败或无有效内容，清理 localStorage 不迁移
                window.localStorage.removeItem(adapter.legacyStorageKey);
              }
            }
          } catch (e) {
            console.warn("[迁移] 从 localStorage 迁移失败:", e);
          }
        }
      } catch (error) {
        console.error(`加载 ${adapter.appKey} 通用配置失败:`, error);
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
  }, [enabled, adapter]);

  // ============================================================================
  // 编辑模式初始化：从 meta 读取启用状态
  // ============================================================================
  useEffect(() => {
    if (!enabled) return;
    if (initialData && !isLoading && !hasInitializedEditMode.current) {
      hasInitializedEditMode.current = true;

      const metaByApp = initialData.meta?.commonConfigEnabledByApp;
      const resolvedMetaEnabled =
        metaByApp?.[adapter.appKey] ?? initialData.meta?.commonConfigEnabled;

      if (resolvedMetaEnabled !== undefined) {
        if (!resolvedMetaEnabled) {
          setUseCommonConfig(false);
          return;
        }
        const applyError = adapter.getApplyError(commonConfigSnippet, t);
        if (applyError) {
          setCommonConfigError(applyError);
          setUseCommonConfig(false);
          return;
        }
        setCommonConfigError("");
        setUseCommonConfig(true);
      } else {
        setUseCommonConfig(false);
      }
    }
  }, [enabled, initialData, isLoading, commonConfigSnippet, adapter, t]);

  // ============================================================================
  // 新建模式初始化：如果通用配置有效，默认启用
  // ============================================================================
  useEffect(() => {
    if (!enabled) return;
    if (!initialData && !isLoading && !hasInitializedNewMode.current) {
      hasInitializedNewMode.current = true;

      if (adapter.hasValidContent(commonConfigSnippet)) {
        const parsed = adapter.parseSnippet(commonConfigSnippet);
        if (!parsed.error && parsed.config !== null) {
          setUseCommonConfig(true);
        }
      }
    }
  }, [enabled, initialData, commonConfigSnippet, isLoading, adapter]);

  // ============================================================================
  // 计算最终配置（运行时合并）
  // ============================================================================
  const finalValue = useMemo((): TFinal => {
    const customConfig = adapter.parseInput(inputValue);

    if (!enabled || !useCommonConfig) {
      return adapter.computeFinal(customConfig, customConfig, false);
    }

    const snippetParsed = adapter.parseSnippet(commonConfigSnippet);
    if (snippetParsed.error || snippetParsed.config === null) {
      return adapter.computeFinal(customConfig, customConfig, false);
    }

    return adapter.computeFinal(customConfig, snippetParsed.config, true);
  }, [enabled, inputValue, commonConfigSnippet, useCommonConfig, adapter]);

  // ============================================================================
  // 处理通用配置开关
  // ============================================================================
  const handleCommonConfigToggle = useCallback(
    (checked: boolean) => {
      if (checked) {
        const applyError = adapter.getApplyError(commonConfigSnippet, t);
        if (applyError) {
          setCommonConfigError(applyError);
          setUseCommonConfig(false);
          return;
        }
      }
      setCommonConfigError("");
      setUseCommonConfig(checked);
    },
    [commonConfigSnippet, adapter, t],
  );

  // ============================================================================
  // 处理通用配置片段变化（延迟保存模式）
  // ============================================================================
  const handleCommonConfigSnippetChange = useCallback(
    (value: string) => {
      setCommonConfigSnippetState(value);

      if (!value.trim()) {
        setCommonConfigError("");
        setHasUnsavedCommonConfig(true);
        return;
      }

      // 格式校验
      const parsed = adapter.parseSnippet(value);
      if (parsed.error) {
        setCommonConfigError(parsed.error);
        return;
      }

      setCommonConfigError("");
      setHasUnsavedCommonConfig(true);
    },
    [adapter],
  );

  // ============================================================================
  // 从当前最终配置提取通用配置片段
  // ============================================================================
  const handleExtract = useCallback(async () => {
    setIsExtracting(true);
    setCommonConfigError("");

    try {
      const request = adapter.buildExtractRequest(finalValue);
      const extracted = await configApi.extractCommonConfigSnippet(
        adapter.appKey,
        request,
      );

      if (
        !extracted ||
        !extracted.trim() ||
        !adapter.hasValidContent(extracted)
      ) {
        setCommonConfigError(
          t(`${adapter.appKey}Config.extractNoCommonConfig`, {
            defaultValue: "无法提取通用配置",
          }),
        );
        return;
      }

      // 验证提取结果格式
      const extractedParsed = adapter.parseSnippet(extracted);
      if (extractedParsed.error || extractedParsed.config === null) {
        setCommonConfigError(
          t(`${adapter.appKey}Config.extractedConfigInvalid`, {
            defaultValue: "提取的配置格式错误",
          }),
        );
        return;
      }

      // 更新片段状态
      setCommonConfigSnippetState(extracted);
      setHasUnsavedCommonConfig(true);

      // 从自定义配置中移除与提取内容相同的部分
      const customConfig = adapter.parseInput(inputValue);
      const diffResult = adapter.extractDiff(
        customConfig,
        extractedParsed.config,
      );

      if (!diffResult.error) {
        onInputChange(adapter.serializeOutput(diffResult.custom));
        toast.success(
          t(`${adapter.appKey}Config.extractSuccessNeedSave`, {
            defaultValue: "已提取通用配置，点击保存按钮完成保存",
          }),
        );
      }
    } catch (error) {
      console.error(`提取 ${adapter.appKey} 通用配置失败:`, error);
      setCommonConfigError(
        t(`${adapter.appKey}Config.extractFailed`, {
          error: String(error),
          defaultValue: "提取失败",
        }),
      );
    } finally {
      setIsExtracting(false);
    }
  }, [adapter, finalValue, inputValue, onInputChange, t]);

  // ============================================================================
  // 获取待保存的通用配置片段
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
    finalValue,
    hasUnsavedCommonConfig,
    getPendingCommonConfigSnippet,
    markCommonConfigSaved,
  };
}

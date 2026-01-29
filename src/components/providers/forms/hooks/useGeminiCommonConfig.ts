import { useMemo } from "react";
import {
  useCommonConfigBase,
  type UseCommonConfigBaseReturn,
} from "@/hooks/useCommonConfigBase";
import { createGeminiAdapter } from "@/hooks/commonConfigAdapters";
import type { ProviderMeta } from "@/types";

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
 * 管理 Gemini 通用配置片段
 *
 * 基于 useCommonConfigBase 泛型 Hook + Gemini ENV/JSON 适配器实现。
 *
 * 架构：
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
}: UseGeminiCommonConfigProps): UseGeminiCommonConfigReturn {
  // 创建适配器（需要转换函数）
  const adapter = useMemo(
    () => createGeminiAdapter({ envStringToObj, envObjToString }),
    [envStringToObj, envObjToString],
  );

  const base: UseCommonConfigBaseReturn<Record<string, string>> =
    useCommonConfigBase({
      adapter,
      inputValue: envValue,
      onInputChange: onEnvChange,
      initialData,
      selectedPresetId,
    });

  return {
    useCommonConfig: base.useCommonConfig,
    commonConfigSnippet: base.commonConfigSnippet,
    commonConfigError: base.commonConfigError,
    isLoading: base.isLoading,
    isExtracting: base.isExtracting,
    handleCommonConfigToggle: base.handleCommonConfigToggle,
    handleCommonConfigSnippetChange: base.handleCommonConfigSnippetChange,
    handleExtract: base.handleExtract,
    finalEnv: base.finalValue,
    hasUnsavedCommonConfig: base.hasUnsavedCommonConfig,
    getPendingCommonConfigSnippet: base.getPendingCommonConfigSnippet,
    markCommonConfigSaved: base.markCommonConfigSaved,
  };
}

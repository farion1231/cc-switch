import { useMemo } from "react";
import {
  useCommonConfigBase,
  type UseCommonConfigBaseReturn,
} from "@/hooks/useCommonConfigBase";
import { claudeAdapter } from "@/hooks/commonConfigAdapters";
import type { ProviderMeta } from "@/types";

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
 * 管理 Claude 通用配置片段
 *
 * 基于 useCommonConfigBase 泛型 Hook + Claude JSON 适配器实现。
 */
export function useCommonConfigSnippet({
  settingsConfig,
  onConfigChange,
  initialData,
  selectedPresetId,
  enabled = true,
}: UseCommonConfigSnippetProps): UseCommonConfigSnippetReturn {
  const adapter = useMemo(() => claudeAdapter, []);

  const base: UseCommonConfigBaseReturn<string> = useCommonConfigBase({
    adapter,
    inputValue: settingsConfig,
    onInputChange: onConfigChange,
    initialData,
    selectedPresetId,
    enabled,
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
    finalConfig: base.finalValue,
    hasUnsavedCommonConfig: base.hasUnsavedCommonConfig,
    getPendingCommonConfigSnippet: base.getPendingCommonConfigSnippet,
    markCommonConfigSaved: base.markCommonConfigSaved,
  };
}

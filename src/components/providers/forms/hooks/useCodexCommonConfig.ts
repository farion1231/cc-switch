import { useState, useMemo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { configApi } from "@/lib/api";
import {
  useCommonConfigBase,
  type UseCommonConfigBaseReturn,
} from "@/hooks/useCommonConfigBase";
import { codexAdapter } from "@/hooks/commonConfigAdapters";
import { extractTomlDifference } from "@/utils/tomlConfigMerge";
import type { ProviderMeta } from "@/types";

interface UseCodexCommonConfigProps {
  /**
   * 当前 Codex 配置（可能是纯 TOML 或 JSON wrapper 格式）
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
 * 检测 codexConfig 是否是 JSON wrapper 格式
 * @returns 如果是 JSON wrapper 返回解析后的对象，否则返回 null
 */
function detectJsonWrapperFormat(
  codexConfig: string,
): { auth?: unknown; config?: string } | null {
  try {
    const parsed = JSON.parse(codexConfig);
    if (typeof parsed?.config === "string") {
      return parsed;
    }
    if (typeof parsed === "object" && parsed !== null) {
      return parsed; // JSON 对象但没有 config 字段
    }
  } catch {
    // 不是 JSON
  }
  return null;
}

/**
 * 管理 Codex 通用配置片段
 *
 * 基于 useCommonConfigBase 泛型 Hook + Codex TOML 适配器实现。
 * 额外处理 Codex 双格式（纯 TOML / JSON wrapper）的写回逻辑。
 */
export function useCodexCommonConfig({
  codexConfig,
  onConfigChange,
  initialData,
  selectedPresetId,
}: UseCodexCommonConfigProps): UseCodexCommonConfigReturn {
  const { t } = useTranslation();
  const adapter = useMemo(() => codexAdapter, []);

  // 额外的 isExtracting 状态（base hook 的 handleExtract 不适用于 Codex）
  const [localIsExtracting, setLocalIsExtracting] = useState(false);
  const [localExtractError, setLocalExtractError] = useState("");

  const base: UseCommonConfigBaseReturn<string> = useCommonConfigBase({
    adapter,
    inputValue: codexConfig,
    onInputChange: onConfigChange,
    initialData,
    selectedPresetId,
  });

  // Codex 自定义 handleExtract：处理双格式写回 + 状态管理
  const handleExtract = useCallback(async () => {
    setLocalIsExtracting(true);
    setLocalExtractError("");

    try {
      const request = adapter.buildExtractRequest(base.finalValue);
      const extracted = await configApi.extractCommonConfigSnippet(
        "codex",
        request,
      );

      if (!extracted || !extracted.trim()) {
        setLocalExtractError(t("codexConfig.extractNoCommonConfig"));
        return;
      }

      // 验证 TOML 格式
      const parseResult = adapter.parseSnippet(extracted);
      if (parseResult.error || parseResult.config === null) {
        setLocalExtractError(
          t("codexConfig.extractedTomlInvalid", {
            defaultValue: "提取的配置 TOML 格式错误",
          }),
        );
        return;
      }

      // 更新 snippet 状态（通过 base 的 handler）
      base.handleCommonConfigSnippetChange(extracted);

      // 从 config 中移除与 extracted 相同的部分
      const customToml = adapter.parseInput(codexConfig);
      const diffResult = extractTomlDifference(customToml, extracted);

      if (!diffResult.error) {
        // Codex 双格式写回：检测原始格式并保持一致
        const jsonWrapper = detectJsonWrapperFormat(codexConfig);

        if (jsonWrapper && typeof jsonWrapper.config === "string") {
          // JSON wrapper 格式，更新 config 字段
          jsonWrapper.config = diffResult.customToml;
          onConfigChange(JSON.stringify(jsonWrapper, null, 2));
        } else {
          // 纯 TOML 格式
          onConfigChange(diffResult.customToml);
        }

        toast.success(
          t("codexConfig.extractSuccessNeedSave", {
            defaultValue: "已提取通用配置，点击保存按钮完成保存",
          }),
        );
      }
    } catch (error) {
      console.error("提取 Codex 通用配置失败:", error);
      setLocalExtractError(
        t("codexConfig.extractFailed", {
          error: String(error),
          defaultValue: "提取失败",
        }),
      );
    } finally {
      setLocalIsExtracting(false);
    }
  }, [adapter, base, codexConfig, onConfigChange, t]);

  // 合并 error：优先显示 extract 错误，其次是 base 的错误
  const combinedError = localExtractError || base.commonConfigError;

  return {
    useCommonConfig: base.useCommonConfig,
    commonConfigSnippet: base.commonConfigSnippet,
    commonConfigError: combinedError,
    isLoading: base.isLoading,
    isExtracting: localIsExtracting,
    handleCommonConfigToggle: base.handleCommonConfigToggle,
    handleCommonConfigSnippetChange: base.handleCommonConfigSnippetChange,
    handleExtract,
    finalConfig: base.finalValue,
    hasUnsavedCommonConfig: base.hasUnsavedCommonConfig,
    getPendingCommonConfigSnippet: base.getPendingCommonConfigSnippet,
    markCommonConfigSaved: base.markCommonConfigSaved,
  };
}

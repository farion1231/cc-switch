import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useCommonConfigSyncGuard } from "./useCommonConfigSyncGuard";
import {
  updateCommonConfigSnippet,
  hasCommonConfigSnippet,
  validateJsonConfig,
} from "@/utils/providerConfigUtils";
import { configApi } from "@/lib/api";

const LEGACY_STORAGE_KEY = "cc-switch:common-config-snippet";
const DEFAULT_COMMON_CONFIG_SNIPPET = `{
  "includeCoAuthoredBy": false
}`;

interface UseCommonConfigSnippetProps {
  settingsConfig: string;
  onConfigChange: (config: string) => void;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  initialEnabled?: boolean;
  selectedPresetId?: string;
  /** When false, the hook skips all logic and returns disabled state. Default: true */
  enabled?: boolean;
}

/**
 * 管理 Claude 通用配置片段
 * 从 config.json 读取和保存，支持从 localStorage 平滑迁移
 */
export function useCommonConfigSnippet({
  settingsConfig,
  onConfigChange,
  initialData,
  initialEnabled,
  selectedPresetId,
  enabled = true,
}: UseCommonConfigSnippetProps) {
  const { t } = useTranslation();
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_COMMON_CONFIG_SNIPPET,
  );
  const [commonConfigError, setCommonConfigError] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isExtracting, setIsExtracting] = useState(false);

  // 程序性配置写入的回显保护（替代 setTimeout 重置标记）
  const syncGuard = useCommonConfigSyncGuard();
  // 用于跟踪新建模式是否已初始化默认勾选
  const hasInitializedNewMode = useRef(false);
  // 用于识别 initialData 被 live 配置替换后的表单重置
  const lastInitialDataRef = useRef<typeof initialData | undefined>(undefined);
  const lastInitialEnabledRef = useRef<boolean | undefined>(undefined);

  // 当预设变化时，重置初始化标记，使新预设能够重新触发初始化逻辑
  useEffect(() => {
    if (!enabled) return;
    hasInitializedNewMode.current = false;
    lastInitialDataRef.current = undefined;
    lastInitialEnabledRef.current = undefined;
  }, [selectedPresetId, enabled]);

  // 初始化：从 config.json 加载，支持从 localStorage 迁移
  useEffect(() => {
    if (!enabled) {
      setIsLoading(false);
      return;
    }
    let mounted = true;

    const loadSnippet = async () => {
      try {
        // 使用统一 API 加载
        const snippet = await configApi.getCommonConfigSnippet("claude");

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
                // 迁移到 config.json
                await configApi.setCommonConfigSnippet("claude", legacySnippet);
                if (mounted) {
                  setCommonConfigSnippetState(legacySnippet);
                }
                // 清理 localStorage
                window.localStorage.removeItem(LEGACY_STORAGE_KEY);
                console.log(
                  "[迁移] Claude 通用配置已从 localStorage 迁移到 config.json",
                );
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
  }, [enabled]);

  // 编辑态初始化 / live 配置刷新：勾选状态以 meta.commonConfigEnabled 为准。
  //
  // 片段已由 useInitialDataCommonConfig 在数据层合并进 initialData，所以这里
  // 只需同步勾选状态。表单随后会被 reset 到 expectedConfig，把这次重置登记给
  // syncGuard，避免下面的同步 effect 在重置落地前用旧值做一次无谓的推断。
  useEffect(() => {
    if (!enabled) return;
    if (
      !initialData ||
      isLoading ||
      (lastInitialDataRef.current === initialData &&
        lastInitialEnabledRef.current === initialEnabled)
    ) {
      return;
    }

    lastInitialDataRef.current = initialData;
    lastInitialEnabledRef.current = initialEnabled;

    const expectedConfig = JSON.stringify(initialData.settingsConfig, null, 2);
    const inferredHasCommon = hasCommonConfigSnippet(
      expectedConfig,
      commonConfigSnippet,
    );
    // 优先级：显式设置的 initialEnabled > 从配置推断的值
    // 如果 initialEnabled 为 undefined，使用推断值
    const hasCommon =
      initialEnabled !== undefined ? initialEnabled : inferredHasCommon;

    setCommonConfigError("");
    setUseCommonConfig(hasCommon);
    // 无条件登记：即使表单当前已经是 expectedConfig，也要让同步 effect 跳过
    // 这一轮推断，否则片段为空/不匹配时会把刚设好的勾选状态又推断成 false。
    syncGuard.schedule(settingsConfig, expectedConfig);
  }, [
    enabled,
    initialData,
    initialEnabled,
    commonConfigSnippet,
    isLoading,
    settingsConfig,
  ]);

  // 新建模式：如果通用配置片段存在且有效，默认启用
  useEffect(() => {
    if (!enabled) return;
    // 仅新建模式、加载完成、尚未初始化过
    if (!initialData && !isLoading && !hasInitializedNewMode.current) {
      hasInitializedNewMode.current = true;

      // 检查片段是否有实质内容
      try {
        const snippetObj = JSON.parse(commonConfigSnippet);
        const hasContent = Object.keys(snippetObj).length > 0;
        if (hasContent) {
          setUseCommonConfig(true);
          // 合并通用配置到当前配置
          const { updatedConfig, error } = updateCommonConfigSnippet(
            settingsConfig,
            commonConfigSnippet,
            true,
          );
          if (!error) {
            syncGuard.schedule(settingsConfig, updatedConfig);
            onConfigChange(updatedConfig);
          }
        }
      } catch {
        // ignore parse error
      }
    }
  }, [
    enabled,
    initialData,
    commonConfigSnippet,
    isLoading,
    settingsConfig,
    onConfigChange,
  ]);

  // 处理通用配置开关
  const handleCommonConfigToggle = useCallback(
    (checked: boolean) => {
      const { updatedConfig, error: snippetError } = updateCommonConfigSnippet(
        settingsConfig,
        commonConfigSnippet,
        checked,
      );

      if (snippetError) {
        setCommonConfigError(snippetError);
        setUseCommonConfig(false);
        return;
      }

      setCommonConfigError("");
      setUseCommonConfig(checked);
      syncGuard.schedule(settingsConfig, updatedConfig);
      onConfigChange(updatedConfig);
    },
    [settingsConfig, commonConfigSnippet, onConfigChange],
  );

  // 处理通用配置片段变化
  const handleCommonConfigSnippetChange = useCallback(
    (value: string) => {
      const previousSnippet = commonConfigSnippet;
      setCommonConfigSnippetState(value);

      if (!value.trim()) {
        setCommonConfigError("");
        // 保存到 config.json（清空）
        configApi
          .setCommonConfigSnippet("claude", "")
          .catch((error: unknown) => {
            console.error("保存通用配置失败:", error);
            setCommonConfigError(
              t("claudeConfig.saveFailed", { error: String(error) }),
            );
          });

        if (useCommonConfig) {
          const { updatedConfig } = updateCommonConfigSnippet(
            settingsConfig,
            previousSnippet,
            false,
          );
          syncGuard.schedule(settingsConfig, updatedConfig);
          onConfigChange(updatedConfig);
          setUseCommonConfig(false);
        }
        return;
      }

      // 验证JSON格式
      const validationError = validateJsonConfig(value, "通用配置片段");
      if (validationError) {
        setCommonConfigError(validationError);
      } else {
        setCommonConfigError("");
        // 保存到 config.json
        configApi
          .setCommonConfigSnippet("claude", value)
          .catch((error: unknown) => {
            console.error("保存通用配置失败:", error);
            setCommonConfigError(
              t("claudeConfig.saveFailed", { error: String(error) }),
            );
          });
      }

      // 若当前启用通用配置且格式正确，需要替换为最新片段
      if (useCommonConfig && !validationError) {
        const removeResult = updateCommonConfigSnippet(
          settingsConfig,
          previousSnippet,
          false,
        );
        if (removeResult.error) {
          setCommonConfigError(removeResult.error);
          return;
        }
        const addResult = updateCommonConfigSnippet(
          removeResult.updatedConfig,
          value,
          true,
        );

        if (addResult.error) {
          setCommonConfigError(addResult.error);
          return;
        }

        syncGuard.schedule(settingsConfig, addResult.updatedConfig);
        onConfigChange(addResult.updatedConfig);
      }
    },
    [commonConfigSnippet, settingsConfig, useCommonConfig, onConfigChange],
  );

  // 配置变化同步 effect：跳过程序性写入的回显，其余按内容推断勾选状态，
  // 让用户手动增删片段时勾选框跟着变。
  useEffect(() => {
    if (!enabled) return;
    if (isLoading || syncGuard.skip(settingsConfig)) {
      return;
    }
    // 没有片段可比对时，推断不出任何信息——保持当前勾选状态，
    // 否则会把 meta.commonConfigEnabled 静默改写成 false。
    if (!commonConfigSnippet.trim()) return;

    const hasCommon = hasCommonConfigSnippet(
      settingsConfig,
      commonConfigSnippet,
    );
    setUseCommonConfig(hasCommon);
  }, [enabled, settingsConfig, commonConfigSnippet, isLoading]);

  // 从编辑器当前内容提取通用配置片段
  const handleExtract = useCallback(async () => {
    setIsExtracting(true);
    setCommonConfigError("");

    try {
      const extracted = await configApi.extractCommonConfigSnippet("claude", {
        settingsConfig,
      });

      if (!extracted || extracted === "{}") {
        setCommonConfigError(t("claudeConfig.extractNoCommonConfig"));
        return;
      }

      // 验证 JSON 格式
      const validationError = validateJsonConfig(extracted, "提取的配置");
      if (validationError) {
        setCommonConfigError(validationError);
        return;
      }

      // 更新片段状态
      setCommonConfigSnippetState(extracted);

      // 保存到后端
      await configApi.setCommonConfigSnippet("claude", extracted);
    } catch (error) {
      console.error("提取通用配置失败:", error);
      setCommonConfigError(
        t("claudeConfig.extractFailed", { error: String(error) }),
      );
    } finally {
      setIsExtracting(false);
    }
  }, [settingsConfig, t]);

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

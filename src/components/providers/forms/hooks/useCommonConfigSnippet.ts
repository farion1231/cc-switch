import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  updateCommonConfigSnippet,
  validateJsonConfig,
  hasCommonConfigSnippet,
  replaceCommonConfigSnippet,
} from "@/utils/providerConfigUtils";
import { configApi } from "@/lib/api";
import type { ProviderMeta } from "@/types";

const LEGACY_STORAGE_KEY = "cc-switch:common-config-snippet";
const DEFAULT_COMMON_CONFIG_SNIPPET = `{
  "includeCoAuthoredBy": false
}`;

interface UseCommonConfigSnippetProps {
  settingsConfig: string;
  onConfigChange: (config: string) => void;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    meta?: ProviderMeta;
  };
  selectedPresetId?: string;
  /** When false, the hook skips all logic and returns disabled state. Default: true */
  enabled?: boolean;
  /** 当前正在编辑的供应商 ID（用于同步时跳过） */
  currentProviderId?: string;
}

/**
 * 管理 Claude 通用配置片段
 * 从数据库读取和保存，支持从 localStorage 平滑迁移
 */
export function useCommonConfigSnippet({
  settingsConfig,
  onConfigChange,
  initialData,
  selectedPresetId,
  enabled = true,
  currentProviderId,
}: UseCommonConfigSnippetProps) {
  const { t } = useTranslation();
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_COMMON_CONFIG_SNIPPET,
  );
  const [commonConfigError, setCommonConfigError] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isExtracting, setIsExtracting] = useState(false);

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
    if (!enabled) return;
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
    hasUserToggledCommonConfig.current = false;
  }, [selectedPresetId, enabled]);

  // 初始化：从数据库加载，支持从 localStorage 迁移
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
          // 如果数据库中没有，尝试从 localStorage 迁移
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
                  "[迁移] Claude 通用配置已从 localStorage 迁移到数据库",
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

  // 初始化时从 meta 读取启用状态（编辑模式）
  // 优先使用 meta，若 meta 未定义则回退到内容检测
  useEffect(() => {
    if (!enabled) return;
    if (initialData && !isLoading && !hasInitializedEditMode.current) {
      hasInitializedEditMode.current = true;

      // 使用 meta 中记录的按 app 启用状态
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
        return;
      } else {
        // meta 未定义，回退到内容检测
        const settingsConfigStr =
          typeof initialData.settingsConfig === "string"
            ? initialData.settingsConfig
            : JSON.stringify(initialData.settingsConfig ?? {});
        const detected = hasCommonConfigSnippet(
          settingsConfigStr,
          commonConfigSnippet,
        );
        setUseCommonConfig(detected);
      }
    }
  }, [
    enabled,
    initialData,
    isLoading,
    commonConfigSnippet,
    getSnippetApplyError,
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
            isUpdatingFromCommonConfig.current = true;
            onConfigChange(updatedConfig);
            setTimeout(() => {
              isUpdatingFromCommonConfig.current = false;
            }, 0);
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
      hasUserToggledCommonConfig.current = true;
      if (checked) {
        const snippetError = getSnippetApplyError(commonConfigSnippet);
        if (snippetError) {
          setCommonConfigError(snippetError);
          setUseCommonConfig(false);
          return;
        }
      }
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
      // 标记正在通过通用配置更新
      isUpdatingFromCommonConfig.current = true;
      onConfigChange(updatedConfig);
      // 在下一个事件循环中重置标记
      setTimeout(() => {
        isUpdatingFromCommonConfig.current = false;
      }, 0);
    },
    [settingsConfig, commonConfigSnippet, onConfigChange, getSnippetApplyError],
  );

  // 处理通用配置片段变化
  const handleCommonConfigSnippetChange = useCallback(
    (value: string) => {
      const previousSnippet = commonConfigSnippet;
      setCommonConfigSnippetState(value);

      if (!value.trim()) {
        const saveId = ++saveSequenceRef.current;
        setCommonConfigError("");
        // 保存到数据库（清空）
        enqueueSave(() => configApi.setCommonConfigSnippet("claude", ""))
          .then(() => {
            if (saveSequenceRef.current !== saveId) return;
            // 清空时也需要同步：移除所有供应商的通用配置片段
            configApi.syncCommonConfigToProviders(
              "claude",
              previousSnippet,
              "", // newSnippet 为空表示移除
              replaceCommonConfigSnippet,
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
          onConfigChange(updatedConfig);
          setUseCommonConfig(false);
        }
        return;
      }

      // 验证JSON格式
      const validationError = validateJsonConfig(value, "通用配置片段");
      if (validationError) {
        setCommonConfigError(validationError);
        return;
      } else {
        const saveId = ++saveSequenceRef.current;
        setCommonConfigError("");
        // 保存到数据库
        enqueueSave(() => configApi.setCommonConfigSnippet("claude", value))
          .then(() => {
            if (saveSequenceRef.current !== saveId) return;
            // 保存成功后，同步更新所有启用了通用配置的供应商
            configApi.syncCommonConfigToProviders(
              "claude",
              previousSnippet,
              value,
              replaceCommonConfigSnippet,
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

        // 标记正在通过通用配置更新，避免触发状态检查
        isUpdatingFromCommonConfig.current = true;
        onConfigChange(addResult.updatedConfig);
        // 在下一个事件循环中重置标记
        setTimeout(() => {
          isUpdatingFromCommonConfig.current = false;
        }, 0);
      }
    },
    [
      commonConfigSnippet,
      settingsConfig,
      useCommonConfig,
      onConfigChange,
      currentProviderId,
      enqueueSave,
      t,
    ],
  );

  // 当配置变化时检查是否包含通用配置（避免通过通用配置更新时反复覆盖）
  useEffect(() => {
    if (!enabled) return;
    if (isUpdatingFromCommonConfig.current || isLoading) {
      return;
    }
    const metaByApp = initialData?.meta?.commonConfigEnabledByApp;
    const hasExplicitMeta =
      metaByApp?.claude !== undefined ||
      initialData?.meta?.commonConfigEnabled !== undefined;
    if (hasExplicitMeta || hasUserToggledCommonConfig.current) {
      return;
    }
    const hasCommon = hasCommonConfigSnippet(
      settingsConfig,
      commonConfigSnippet,
    );
    setUseCommonConfig(hasCommon);
  }, [enabled, settingsConfig, commonConfigSnippet, isLoading, initialData]);

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

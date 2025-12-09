import { useState, useEffect, useCallback, useRef } from "react";
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
}

/**
 * 管理 Claude 通用配置片段
 * 从 config.json 读取和保存，支持从 localStorage 平滑迁移
 */
export function useCommonConfigSnippet({
  settingsConfig,
  onConfigChange,
  initialData,
}: UseCommonConfigSnippetProps) {
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_COMMON_CONFIG_SNIPPET,
  );
  const [commonConfigError, setCommonConfigError] = useState("");
  const [isLoading, setIsLoading] = useState(true);

  // 用于跟踪是否正在通过通用配置更新
  const isUpdatingFromCommonConfig = useRef(false);

  // 初始化：从 config.json 加载，支持从 localStorage 迁移
  useEffect(() => {
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
  }, []);

  // 初始化时检查通用配置片段（编辑模式）
  useEffect(() => {
    if (initialData && !isLoading) {
      const configString = JSON.stringify(initialData.settingsConfig, null, 2);
      const hasCommon = hasCommonConfigSnippet(
        configString,
        commonConfigSnippet,
      );
      setUseCommonConfig(hasCommon);
    }
  }, [initialData, commonConfigSnippet, isLoading]);

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
      // 标记正在通过通用配置更新
      isUpdatingFromCommonConfig.current = true;
      onConfigChange(updatedConfig);
      // 在下一个事件循环中重置标记
      setTimeout(() => {
        isUpdatingFromCommonConfig.current = false;
      }, 0);
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
        configApi.setCommonConfigSnippet("claude", "").catch((error) => {
          console.error("保存通用配置失败:", error);
          setCommonConfigError(`保存失败: ${error}`);
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
      } else {
        setCommonConfigError("");
        // 保存到 config.json
        configApi.setCommonConfigSnippet("claude", value).catch((error) => {
          console.error("保存通用配置失败:", error);
          setCommonConfigError(`保存失败: ${error}`);
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
    [commonConfigSnippet, settingsConfig, useCommonConfig, onConfigChange],
  );

  // 当配置变化时检查是否包含通用配置（但避免在通过通用配置更新时检查）
  useEffect(() => {
    if (isUpdatingFromCommonConfig.current || isLoading) {
      return;
    }
    const hasCommon = hasCommonConfigSnippet(
      settingsConfig,
      commonConfigSnippet,
    );
    setUseCommonConfig(hasCommon);
  }, [settingsConfig, commonConfigSnippet, isLoading]);

  return {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    isLoading,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
  };
}

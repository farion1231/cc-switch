import { useState, useEffect, useCallback, useRef } from "react";
import { configApi } from "@/lib/api";

const LEGACY_STORAGE_KEY = "cc-switch:gemini-common-config-snippet";
const DEFAULT_GEMINI_COMMON_CONFIG_SNIPPET = `{
  "timeout": 30000,
  "maxRetries": 3
}`;

interface UseGeminiCommonConfigProps {
  configValue: string;
  onConfigChange: (config: string) => void;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
}

/**
 * 深度合并两个对象（用于合并通用配置）
 */
function deepMerge(target: any, source: any): any {
  if (typeof target !== "object" || target === null) {
    return source;
  }
  if (typeof source !== "object" || source === null) {
    return target;
  }
  if (Array.isArray(source)) {
    return source;
  }

  const result = { ...target };
  for (const key of Object.keys(source)) {
    if (typeof source[key] === "object" && !Array.isArray(source[key])) {
      result[key] = deepMerge(result[key], source[key]);
    } else {
      result[key] = source[key];
    }
  }
  return result;
}

/**
 * 从配置中移除通用配置片段（递归比较）
 */
function removeCommonConfig(config: any, commonConfig: any): any {
  if (typeof config !== "object" || config === null) {
    return config;
  }
  if (typeof commonConfig !== "object" || commonConfig === null) {
    return config;
  }

  const result = { ...config };
  for (const key of Object.keys(commonConfig)) {
    if (result[key] === undefined) continue;

    // 如果值完全相等，删除该键
    if (JSON.stringify(result[key]) === JSON.stringify(commonConfig[key])) {
      delete result[key];
    } else if (
      typeof result[key] === "object" &&
      !Array.isArray(result[key]) &&
      typeof commonConfig[key] === "object" &&
      !Array.isArray(commonConfig[key])
    ) {
      // 递归移除嵌套对象
      result[key] = removeCommonConfig(result[key], commonConfig[key]);
      // 如果移除后对象为空，删除该键
      if (Object.keys(result[key]).length === 0) {
        delete result[key];
      }
    }
  }
  return result;
}

/**
 * 检查配置中是否包含通用配置片段
 */
function hasCommonConfigSnippet(config: any, commonConfig: any): boolean {
  if (typeof config !== "object" || config === null) return false;
  if (typeof commonConfig !== "object" || commonConfig === null) return false;

  for (const key of Object.keys(commonConfig)) {
    if (config[key] === undefined) return false;
    if (JSON.stringify(config[key]) !== JSON.stringify(commonConfig[key])) {
      // 检查嵌套对象
      if (
        typeof config[key] === "object" &&
        !Array.isArray(config[key]) &&
        typeof commonConfig[key] === "object" &&
        !Array.isArray(commonConfig[key])
      ) {
        if (!hasCommonConfigSnippet(config[key], commonConfig[key])) {
          return false;
        }
      } else {
        return false;
      }
    }
  }
  return true;
}

/**
 * 管理 Gemini 通用配置片段 (JSON 格式)
 * 从 config.json 读取和保存，支持从 localStorage 平滑迁移
 */
export function useGeminiCommonConfig({
  configValue,
  onConfigChange,
  initialData,
}: UseGeminiCommonConfigProps) {
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_GEMINI_COMMON_CONFIG_SNIPPET,
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
        const snippet = await configApi.getCommonConfigSnippet("gemini");

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
                await configApi.setCommonConfigSnippet("gemini", legacySnippet);
                if (mounted) {
                  setCommonConfigSnippetState(legacySnippet);
                }
                // 清理 localStorage
                window.localStorage.removeItem(LEGACY_STORAGE_KEY);
                console.log(
                  "[迁移] Gemini 通用配置已从 localStorage 迁移到 config.json",
                );
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
  }, []);

  // 初始化时检查通用配置片段（编辑模式）
  useEffect(() => {
    if (initialData?.settingsConfig && !isLoading) {
      try {
        const config =
          typeof initialData.settingsConfig.config === "object"
            ? initialData.settingsConfig.config
            : {};
        const commonConfigObj = JSON.parse(commonConfigSnippet);
        const hasCommon = hasCommonConfigSnippet(config, commonConfigObj);
        setUseCommonConfig(hasCommon);
      } catch {
        // ignore parse error
      }
    }
  }, [initialData, commonConfigSnippet, isLoading]);

  // 处理通用配置开关
  const handleCommonConfigToggle = useCallback(
    (checked: boolean) => {
      try {
        const configObj = configValue.trim() ? JSON.parse(configValue) : {};
        const commonConfigObj = JSON.parse(commonConfigSnippet);

        let updatedConfig: any;
        if (checked) {
          // 合并通用配置
          updatedConfig = deepMerge(configObj, commonConfigObj);
        } else {
          // 移除通用配置
          updatedConfig = removeCommonConfig(configObj, commonConfigObj);
        }

        setCommonConfigError("");
        setUseCommonConfig(checked);

        // 标记正在通过通用配置更新
        isUpdatingFromCommonConfig.current = true;
        onConfigChange(JSON.stringify(updatedConfig, null, 2));

        // 在下一个事件循环中重置标记
        setTimeout(() => {
          isUpdatingFromCommonConfig.current = false;
        }, 0);
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : String(error);
        setCommonConfigError(`配置合并失败: ${errorMessage}`);
        setUseCommonConfig(false);
      }
    },
    [configValue, commonConfigSnippet, onConfigChange],
  );

  // 处理通用配置片段变化
  const handleCommonConfigSnippetChange = useCallback(
    (value: string) => {
      const previousSnippet = commonConfigSnippet;
      setCommonConfigSnippetState(value);

      if (!value.trim()) {
        setCommonConfigError("");
        // 保存到 config.json（清空）
        configApi.setCommonConfigSnippet("gemini", "").catch((error) => {
          console.error("保存 Gemini 通用配置失败:", error);
          setCommonConfigError(`保存失败: ${error}`);
        });

        if (useCommonConfig) {
          // 移除旧的通用配置
          try {
            const configObj = configValue.trim() ? JSON.parse(configValue) : {};
            const previousCommonConfigObj = JSON.parse(previousSnippet);
            const updatedConfig = removeCommonConfig(
              configObj,
              previousCommonConfigObj,
            );
            onConfigChange(JSON.stringify(updatedConfig, null, 2));
            setUseCommonConfig(false);
          } catch {
            // ignore
          }
        }
        return;
      }

      // 校验 JSON 格式
      try {
        JSON.parse(value);
        setCommonConfigError("");
        // 保存到 config.json
        configApi.setCommonConfigSnippet("gemini", value).catch((error) => {
          console.error("保存 Gemini 通用配置失败:", error);
          setCommonConfigError(`保存失败: ${error}`);
        });
      } catch {
        setCommonConfigError("通用配置片段格式错误（必须是有效的 JSON）");
        return;
      }

      // 若当前启用通用配置，需要替换为最新片段
      if (useCommonConfig) {
        try {
          const configObj = configValue.trim() ? JSON.parse(configValue) : {};
          const previousCommonConfigObj = JSON.parse(previousSnippet);
          const newCommonConfigObj = JSON.parse(value);

          // 先移除旧的通用配置
          const withoutOld = removeCommonConfig(
            configObj,
            previousCommonConfigObj,
          );
          // 再合并新的通用配置
          const withNew = deepMerge(withoutOld, newCommonConfigObj);

          // 标记正在通过通用配置更新，避免触发状态检查
          isUpdatingFromCommonConfig.current = true;
          onConfigChange(JSON.stringify(withNew, null, 2));

          // 在下一个事件循环中重置标记
          setTimeout(() => {
            isUpdatingFromCommonConfig.current = false;
          }, 0);
        } catch (error) {
          const errorMessage =
            error instanceof Error ? error.message : String(error);
          setCommonConfigError(`配置替换失败: ${errorMessage}`);
        }
      }
    },
    [commonConfigSnippet, configValue, useCommonConfig, onConfigChange],
  );

  // 当配置变化时检查是否包含通用配置（但避免在通过通用配置更新时检查）
  useEffect(() => {
    if (isUpdatingFromCommonConfig.current || isLoading) {
      return;
    }
    try {
      const configObj = configValue.trim() ? JSON.parse(configValue) : {};
      const commonConfigObj = JSON.parse(commonConfigSnippet);
      const hasCommon = hasCommonConfigSnippet(configObj, commonConfigObj);
      setUseCommonConfig(hasCommon);
    } catch {
      // ignore parse error
    }
  }, [configValue, commonConfigSnippet, isLoading]);

  return {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    isLoading,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
  };
}

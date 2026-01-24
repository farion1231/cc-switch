import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import TOML from "smol-toml";
import {
  updateTomlCommonConfigSnippet,
  hasTomlCommonConfigSnippet,
  replaceTomlCommonConfigSnippet,
} from "@/utils/providerConfigUtils";
import { configApi } from "@/lib/api";
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
  codexConfig: string;
  onConfigChange: (config: string) => void;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    meta?: ProviderMeta;
  };
  selectedPresetId?: string;
  /** 当前正在编辑的供应商 ID（用于同步时跳过） */
  currentProviderId?: string;
}

/**
 * 管理 Codex 通用配置片段 (TOML 格式)
 * 从数据库读取和保存，支持从 localStorage 平滑迁移
 */
export function useCodexCommonConfig({
  codexConfig,
  onConfigChange,
  initialData,
  selectedPresetId,
  currentProviderId,
}: UseCodexCommonConfigProps) {
  const { t } = useTranslation();
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_CODEX_COMMON_CONFIG_SNIPPET,
  );
  const [commonConfigError, setCommonConfigError] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isExtracting, setIsExtracting] = useState(false);

  const hasSnippetContent = useCallback((snippet: string) => {
    const lines = snippet.split("\n");
    return lines.some((line) => {
      const trimmed = line.trim();
      return trimmed && !trimmed.startsWith("#");
    });
  }, []);

  const getSnippetApplyError = useCallback(
    (snippet: string) => {
      if (!hasSnippetContent(snippet)) {
        return t("codexConfig.noCommonConfigToApply");
      }
      // 校验 TOML 语法
      const tomlError = validateTomlFormat(snippet);
      if (tomlError) {
        return t("mcp.error.tomlInvalid", {
          defaultValue: "TOML 格式错误，请检查语法",
        });
      }
      return "";
    },
    [hasSnippetContent, t],
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
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
    hasUserToggledCommonConfig.current = false;
  }, [selectedPresetId]);

  // 初始化：从数据库加载，支持从 localStorage 迁移
  useEffect(() => {
    let mounted = true;

    const loadSnippet = async () => {
      try {
        // 使用统一 API 加载
        const snippet = await configApi.getCommonConfigSnippet("codex");

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
                await configApi.setCommonConfigSnippet("codex", legacySnippet);
                if (mounted) {
                  setCommonConfigSnippetState(legacySnippet);
                }
                // 清理 localStorage
                window.localStorage.removeItem(LEGACY_STORAGE_KEY);
                console.log(
                  "[迁移] Codex 通用配置已从 localStorage 迁移到数据库",
                );
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

  // 初始化时从 meta 读取启用状态（编辑模式）
  // 优先使用 meta，若 meta 未定义则回退到内容检测
  useEffect(() => {
    if (initialData && !isLoading && !hasInitializedEditMode.current) {
      hasInitializedEditMode.current = true;

      // 使用 meta 中记录的按 app 启用状态
      const metaByApp = initialData.meta?.commonConfigEnabledByApp;
      const resolvedMetaEnabled =
        metaByApp?.codex ?? initialData.meta?.commonConfigEnabled;

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
        // Codex 使用 TOML 格式，从 settingsConfig.config 获取
        const codexConfigStr =
          typeof initialData.settingsConfig === "object" &&
          initialData.settingsConfig !== null
            ? String(
                (initialData.settingsConfig as Record<string, unknown>)
                  .config ?? "",
              )
            : "";
        const detected = hasTomlCommonConfigSnippet(
          codexConfigStr,
          commonConfigSnippet,
        );
        setUseCommonConfig(detected);
      }
    }
  }, [initialData, isLoading, commonConfigSnippet, getSnippetApplyError]);

  // 新建模式：如果通用配置片段存在且有效，默认启用
  useEffect(() => {
    // 仅新建模式、加载完成、尚未初始化过
    if (!initialData && !isLoading && !hasInitializedNewMode.current) {
      hasInitializedNewMode.current = true;

      // 检查 TOML 片段是否有实质内容（不只是注释和空行）
      const hasContent = hasSnippetContent(commonConfigSnippet);

      if (hasContent) {
        setUseCommonConfig(true);
        // 合并通用配置到当前配置
        const { updatedConfig, error } = updateTomlCommonConfigSnippet(
          codexConfig,
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
    }
  }, [
    initialData,
    commonConfigSnippet,
    isLoading,
    codexConfig,
    onConfigChange,
    hasSnippetContent,
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
      const { updatedConfig, error: snippetError } =
        updateTomlCommonConfigSnippet(
          codexConfig,
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
    [codexConfig, commonConfigSnippet, onConfigChange, getSnippetApplyError],
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
        enqueueSave(() => configApi.setCommonConfigSnippet("codex", ""))
          .then(() => {
            if (saveSequenceRef.current !== saveId) return;
            // 清空时也需要同步：移除所有供应商的通用配置片段
            configApi.syncCommonConfigToProviders(
              "codex",
              previousSnippet,
              "", // newSnippet 为空表示移除
              replaceTomlCommonConfigSnippet,
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
            console.error("保存 Codex 通用配置失败:", error);
            setCommonConfigError(
              t("codexConfig.saveFailed", { error: String(error) }),
            );
          });

        if (useCommonConfig) {
          const { updatedConfig } = updateTomlCommonConfigSnippet(
            codexConfig,
            previousSnippet,
            false,
          );
          onConfigChange(updatedConfig);
          setUseCommonConfig(false);
        }
        return;
      }

      // TOML 格式校验 - 在保存和同步前校验，避免传播无效配置
      const tomlError = validateTomlFormat(value);
      if (tomlError) {
        console.warn("Codex 通用配置 TOML 校验失败:", tomlError);
        setCommonConfigError(
          t("mcp.error.tomlInvalid", {
            defaultValue: "TOML 格式错误，请检查语法",
          }),
        );
        // 不保存、不同步，仅显示错误
        return;
      }

      const saveId = ++saveSequenceRef.current;
      setCommonConfigError("");
      // 保存到 config.json
      enqueueSave(() => configApi.setCommonConfigSnippet("codex", value))
        .then(() => {
          if (saveSequenceRef.current !== saveId) return;
          // 保存成功后，同步更新所有启用了通用配置的供应商
          configApi.syncCommonConfigToProviders(
            "codex",
            previousSnippet,
            value,
            replaceTomlCommonConfigSnippet,
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
          console.error("保存 Codex 通用配置失败:", error);
          setCommonConfigError(
            t("codexConfig.saveFailed", { error: String(error) }),
          );
        });

      // 若当前启用通用配置，需要替换为最新片段
      if (useCommonConfig) {
        const removeResult = updateTomlCommonConfigSnippet(
          codexConfig,
          previousSnippet,
          false,
        );
        if (removeResult.error) {
          setCommonConfigError(removeResult.error);
          return;
        }
        const addResult = updateTomlCommonConfigSnippet(
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
      codexConfig,
      useCommonConfig,
      onConfigChange,
      currentProviderId,
      enqueueSave,
      t,
    ],
  );

  // 当配置变化时检查是否包含通用配置（避免通过通用配置更新时反复覆盖）
  useEffect(() => {
    if (isUpdatingFromCommonConfig.current || isLoading) {
      return;
    }
    const metaByApp = initialData?.meta?.commonConfigEnabledByApp;
    const hasExplicitMeta =
      metaByApp?.codex !== undefined ||
      initialData?.meta?.commonConfigEnabled !== undefined;
    if (hasExplicitMeta || hasUserToggledCommonConfig.current) {
      return;
    }
    const hasCommon = hasTomlCommonConfigSnippet(
      codexConfig,
      commonConfigSnippet,
    );
    setUseCommonConfig(hasCommon);
  }, [codexConfig, commonConfigSnippet, isLoading, initialData]);

  // 从编辑器当前内容提取通用配置片段
  const handleExtract = useCallback(async () => {
    setIsExtracting(true);
    setCommonConfigError("");

    try {
      const extracted = await configApi.extractCommonConfigSnippet("codex", {
        settingsConfig: JSON.stringify({
          config: codexConfig ?? "",
        }),
      });

      if (!extracted || !extracted.trim()) {
        setCommonConfigError(t("codexConfig.extractNoCommonConfig"));
        return;
      }

      // 更新片段状态
      setCommonConfigSnippetState(extracted);

      // 保存到后端
      await configApi.setCommonConfigSnippet("codex", extracted);
    } catch (error) {
      console.error("提取 Codex 通用配置失败:", error);
      setCommonConfigError(
        t("codexConfig.extractFailed", { error: String(error) }),
      );
    } finally {
      setIsExtracting(false);
    }
  }, [codexConfig, t]);

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

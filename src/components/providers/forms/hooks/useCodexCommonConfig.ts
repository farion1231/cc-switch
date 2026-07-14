import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { parse as parseToml } from "smol-toml";
import { hasTomlCommonConfigSnippet } from "@/utils/providerConfigUtils";
import { configApi } from "@/lib/api";
import { normalizeTomlText } from "@/utils/textNormalization";

/**
 * 合并/剥离通用配置片段（走后端 toml_edit，保注释、保键序）。
 * 失败时返回原文本 + error，调用方据此决定是否回退 UI 状态。
 */
const applyTomlSnippet = async (
  configToml: string,
  snippetToml: string,
  enabled: boolean,
): Promise<{ updatedConfig: string; error?: string }> => {
  try {
    const updatedConfig = await configApi.updateTomlCommonConfigSnippet(
      configToml,
      snippetToml,
      enabled,
    );
    return { updatedConfig };
  } catch (e) {
    return { updatedConfig: configToml, error: String(e) };
  }
};

const LEGACY_STORAGE_KEY = "cc-switch:codex-common-config-snippet";
const DEFAULT_CODEX_COMMON_CONFIG_SNIPPET = `# Common Codex config
# Add your common TOML configuration here`;

interface UseCodexCommonConfigProps {
  enabled?: boolean;
  codexConfig: string;
  onConfigChange: (config: string) => void;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  initialEnabled?: boolean;
  selectedPresetId?: string;
}

/**
 * 管理 Codex 通用配置片段 (TOML 格式)
 * 从 config.json 读取和保存，支持从 localStorage 平滑迁移
 */
export function useCodexCommonConfig({
  enabled = true,
  codexConfig,
  onConfigChange,
  initialData,
  initialEnabled,
  selectedPresetId,
}: UseCodexCommonConfigProps) {
  const { t } = useTranslation();
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippetState] = useState<string>(
    DEFAULT_CODEX_COMMON_CONFIG_SNIPPET,
  );
  const [commonConfigError, setCommonConfigError] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isExtracting, setIsExtracting] = useState(false);

  // 用于跟踪是否正在通过通用配置更新
  const isUpdatingFromCommonConfig = useRef(false);
  // 用于跟踪新建模式是否已初始化默认勾选
  const hasInitializedNewMode = useRef(false);
  // 用于跟踪编辑模式是否已初始化显式开关/预览
  const hasInitializedEditMode = useRef(false);
  // 后端 TOML 合并是异步的：连续操作（快速点开关、连点保存）可能乱序
  // 返回。每个写 config 的异步操作在发起时领取递增序号，结果落地前
  // 校验自己仍是最新一次；过期结果直接丢弃，保证最后一次操作胜出。
  const tomlOpSeqRef = useRef(0);
  // 镜像最新的 codexConfig：用户在编辑器手动改动走父组件 onChange，
  // 不经过本 hook（序号不变）。异步结果落地前还要校验发起时的 config
  // 基线未被外部改写，否则会覆盖用户在请求在飞期间的手动编辑。
  const latestCodexConfigRef = useRef(codexConfig);
  useEffect(() => {
    latestCodexConfigRef.current = codexConfig;
  }, [codexConfig]);

  // 结果落地前的过期判定：hook 内有更新的操作（序号变了），或发起时的
  // config 基线已被外部改写（用户手动编辑为准）。任一成立即丢弃结果。
  const isTomlOpStale = useCallback(
    (seq: number, baseConfig: string) =>
      seq !== tomlOpSeqRef.current ||
      baseConfig !== latestCodexConfigRef.current,
    [],
  );

  // 当预设变化时，重置初始化标记，使新预设能够重新触发初始化逻辑
  useEffect(() => {
    hasInitializedNewMode.current = false;
    hasInitializedEditMode.current = false;
  }, [selectedPresetId, initialEnabled]);

  const parseCommonConfigSnippet = useCallback((snippetString: string) => {
    const trimmed = snippetString.trim();
    if (!trimmed) {
      return {
        hasContent: false,
      };
    }

    try {
      const parsed = parseToml(normalizeTomlText(snippetString)) as Record<
        string,
        unknown
      >;
      return {
        hasContent: Object.keys(parsed).length > 0,
      };
    } catch (error) {
      return {
        hasContent: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }, []);

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
        const snippet = await configApi.getCommonConfigSnippet("codex");

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
                await configApi.setCommonConfigSnippet("codex", legacySnippet);
                if (mounted) {
                  setCommonConfigSnippetState(legacySnippet);
                }
                // 清理 localStorage
                window.localStorage.removeItem(LEGACY_STORAGE_KEY);
                console.log(
                  "[迁移] Codex 通用配置已从 localStorage 迁移到 config.json",
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
  }, [enabled]);

  // 初始化时检查通用配置片段（编辑模式）
  useEffect(() => {
    if (
      !enabled ||
      !initialData?.settingsConfig ||
      isLoading ||
      hasInitializedEditMode.current
    ) {
      return;
    }

    hasInitializedEditMode.current = true;

    const parsedSnippet = parseCommonConfigSnippet(commonConfigSnippet);
    if (parsedSnippet.error) {
      if (commonConfigSnippet.trim()) {
        setCommonConfigError(parsedSnippet.error);
      }
      setUseCommonConfig(false);
      return;
    }

    const config =
      typeof initialData.settingsConfig.config === "string"
        ? initialData.settingsConfig.config
        : "";
    const inferredHasCommon = hasTomlCommonConfigSnippet(
      config,
      commonConfigSnippet,
    );

    // 优先级：显式设置的 initialEnabled > 从配置推断的值
    // 如果 initialEnabled 为 undefined，使用推断值
    const hasCommon =
      initialEnabled !== undefined ? initialEnabled : inferredHasCommon;

    // 如果应该启用通用配置但配置中还没有，则自动添加
    if (hasCommon && !inferredHasCommon && parsedSnippet.hasContent) {
      let cancelled = false;
      const seq = ++tomlOpSeqRef.current;
      (async () => {
        const { updatedConfig, error } = await applyTomlSnippet(
          codexConfig,
          commonConfigSnippet,
          true,
        );
        if (cancelled || isTomlOpStale(seq, codexConfig)) {
          return;
        }
        if (error) {
          setCommonConfigError(error);
          setUseCommonConfig(false);
          return;
        }

        setCommonConfigError("");
        setUseCommonConfig(true);
        isUpdatingFromCommonConfig.current = true;
        onConfigChange(updatedConfig);
        setTimeout(() => {
          isUpdatingFromCommonConfig.current = false;
        }, 0);
      })();
      return () => {
        cancelled = true;
      };
    }

    setCommonConfigError("");
    setUseCommonConfig(hasCommon);
  }, [
    codexConfig,
    commonConfigSnippet,
    enabled,
    initialData,
    initialEnabled,
    isLoading,
    isTomlOpStale,
    onConfigChange,
    parseCommonConfigSnippet,
  ]);

  // 新建模式：如果通用配置片段存在且有效，默认启用
  useEffect(() => {
    if (!enabled || initialData || isLoading || hasInitializedNewMode.current) {
      return;
    }

    hasInitializedNewMode.current = true;

    const parsedSnippet = parseCommonConfigSnippet(commonConfigSnippet);
    if (parsedSnippet.error) {
      if (commonConfigSnippet.trim()) {
        setCommonConfigError(parsedSnippet.error);
      }
      setUseCommonConfig(false);
      return;
    }
    if (!parsedSnippet.hasContent) {
      return;
    }

    let cancelled = false;
    const seq = ++tomlOpSeqRef.current;
    (async () => {
      const { updatedConfig, error } = await applyTomlSnippet(
        codexConfig,
        commonConfigSnippet,
        true,
      );
      if (cancelled || isTomlOpStale(seq, codexConfig)) {
        return;
      }
      if (error) {
        setCommonConfigError(error);
        setUseCommonConfig(false);
        return;
      }

      setCommonConfigError("");
      setUseCommonConfig(true);
      isUpdatingFromCommonConfig.current = true;
      onConfigChange(updatedConfig);
      setTimeout(() => {
        isUpdatingFromCommonConfig.current = false;
      }, 0);
    })();
    return () => {
      cancelled = true;
    };
  }, [
    initialData,
    commonConfigSnippet,
    isLoading,
    isTomlOpStale,
    codexConfig,
    enabled,
    onConfigChange,
    parseCommonConfigSnippet,
  ]);

  // 处理通用配置开关
  const handleCommonConfigToggle = useCallback(
    async (checked: boolean) => {
      if (!enabled) return;

      // 在同步校验之前领号：即使本次走同步早退分支，也要让更早发出、
      // 仍在飞的异步结果作废，避免它晚到后把开关翻回去。
      const seq = ++tomlOpSeqRef.current;
      const parsedSnippet = parseCommonConfigSnippet(commonConfigSnippet);
      if (parsedSnippet.error) {
        setCommonConfigError(parsedSnippet.error);
        setUseCommonConfig(false);
        return;
      }
      if (!parsedSnippet.hasContent) {
        setCommonConfigError(
          t("codexConfig.noCommonConfigToApply", {
            defaultValue: "通用配置片段为空或没有可写入的内容",
          }),
        );
        setUseCommonConfig(false);
        return;
      }

      const { updatedConfig, error: snippetError } = await applyTomlSnippet(
        codexConfig,
        commonConfigSnippet,
        checked,
      );
      if (isTomlOpStale(seq, codexConfig)) {
        return;
      }

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
    [
      codexConfig,
      commonConfigSnippet,
      isTomlOpStale,
      enabled,
      onConfigChange,
      parseCommonConfigSnippet,
      t,
    ],
  );

  // 处理通用配置片段变化
  const handleCommonConfigSnippetChange = useCallback(
    async (value: string): Promise<boolean> => {
      if (!enabled) return false;

      // 与 handleCommonConfigToggle 同一套序号：连续保存或保存与开关
      // 交错时，只允许最后一次操作的结果落地。
      const seq = ++tomlOpSeqRef.current;
      const previousSnippet = commonConfigSnippet;

      if (!value.trim()) {
        setCommonConfigError("");

        if (useCommonConfig) {
          const previousParsed = parseCommonConfigSnippet(previousSnippet);
          let updatedConfig = codexConfig;

          if (!previousParsed.error && previousParsed.hasContent) {
            const removeResult = await applyTomlSnippet(
              codexConfig,
              previousSnippet,
              false,
            );
            if (isTomlOpStale(seq, codexConfig)) {
              return false;
            }
            if (removeResult.error) {
              setCommonConfigError(removeResult.error);
              return false;
            }
            updatedConfig = removeResult.updatedConfig;
          }

          onConfigChange(updatedConfig);
          setUseCommonConfig(false);
        }

        setCommonConfigSnippetState("");
        configApi
          .setCommonConfigSnippet("codex", "")
          .catch((error: unknown) => {
            console.error("保存 Codex 通用配置失败:", error);
            setCommonConfigError(
              t("codexConfig.saveFailed", { error: String(error) }),
            );
          });
        return true;
      }

      const parsedNextSnippet = parseCommonConfigSnippet(value);
      if (parsedNextSnippet.error) {
        setCommonConfigError(parsedNextSnippet.error);
        return false;
      }

      // 若当前启用通用配置，需要替换为最新片段
      if (useCommonConfig) {
        let nextConfig = codexConfig;
        const previousParsed = parseCommonConfigSnippet(previousSnippet);

        if (!previousParsed.error && previousParsed.hasContent) {
          const removeResult = await applyTomlSnippet(
            codexConfig,
            previousSnippet,
            false,
          );
          if (isTomlOpStale(seq, codexConfig)) {
            return false;
          }
          if (removeResult.error) {
            setCommonConfigError(removeResult.error);
            return false;
          }
          nextConfig = removeResult.updatedConfig;
        }

        const addResult = await applyTomlSnippet(nextConfig, value, true);
        // nextConfig 派生自发起时的 codexConfig，基线校验仍对 codexConfig 做
        if (isTomlOpStale(seq, codexConfig)) {
          return false;
        }

        if (addResult.error) {
          setCommonConfigError(addResult.error);
          return false;
        }

        // 标记正在通过通用配置更新，避免触发状态检查
        isUpdatingFromCommonConfig.current = true;
        onConfigChange(addResult.updatedConfig);
        // 在下一个事件循环中重置标记
        setTimeout(() => {
          isUpdatingFromCommonConfig.current = false;
        }, 0);
      }

      setCommonConfigError("");
      setCommonConfigSnippetState(value);
      configApi
        .setCommonConfigSnippet("codex", value)
        .catch((error: unknown) => {
          console.error("保存 Codex 通用配置失败:", error);
          setCommonConfigError(
            t("codexConfig.saveFailed", { error: String(error) }),
          );
        });

      return true;
    },
    [
      commonConfigSnippet,
      codexConfig,
      isTomlOpStale,
      enabled,
      onConfigChange,
      parseCommonConfigSnippet,
      t,
      useCommonConfig,
    ],
  );

  // 当配置变化时检查是否包含通用配置（但避免在通过通用配置更新时检查）
  useEffect(() => {
    if (!enabled || isUpdatingFromCommonConfig.current || isLoading) {
      return;
    }
    const parsedSnippet = parseCommonConfigSnippet(commonConfigSnippet);
    if (parsedSnippet.error) {
      setUseCommonConfig(false);
      return;
    }
    const hasCommon = hasTomlCommonConfigSnippet(
      codexConfig,
      commonConfigSnippet,
    );
    setUseCommonConfig(hasCommon);
  }, [
    codexConfig,
    commonConfigSnippet,
    enabled,
    isLoading,
    parseCommonConfigSnippet,
  ]);

  // 从编辑器当前内容提取通用配置片段
  const handleExtract = useCallback(async () => {
    if (!enabled) return;

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
  }, [codexConfig, enabled, t]);

  const clearCommonConfigError = useCallback(() => {
    setCommonConfigError("");
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
    clearCommonConfigError,
  };
}

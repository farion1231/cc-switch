import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { parse as parseToml } from "smol-toml";
import { hasTomlCommonConfigSnippet } from "@/utils/providerConfigUtils";
import { configApi } from "@/lib/api";
import { normalizeTomlText } from "@/utils/textNormalization";
import { useCommonConfigSyncGuard } from "./useCommonConfigSyncGuard";

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

  // 程序性配置写入的回显保护（替代 setTimeout 重置标记）
  const syncGuard = useCommonConfigSyncGuard();
  // 用于跟踪新建模式是否已初始化默认勾选
  const hasInitializedNewMode = useRef(false);
  // 用于识别 initialData 被 live 配置替换后的 codexConfig 重置
  const lastInitialDataRef = useRef<typeof initialData | undefined>(undefined);
  const lastInitialEnabledRef = useRef<boolean | undefined>(undefined);
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
    lastInitialDataRef.current = undefined;
    lastInitialEnabledRef.current = undefined;
  }, [selectedPresetId]);

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
  }, []);

  // 编辑态初始化 / live 配置刷新：勾选状态以 meta.commonConfigEnabled 为准。
  //
  // 片段已由 useInitialDataCommonConfig 在数据层合并进 initialData，所以这里
  // 只需同步勾选状态。codexConfig 随后会被重置到 expectedConfig，把这次重置
  // 登记给 syncGuard，避免同步 effect 在重置落地前用旧值做一次无谓的推断。
  useEffect(() => {
    if (
      !initialData?.settingsConfig ||
      isLoading ||
      (lastInitialDataRef.current === initialData &&
        lastInitialEnabledRef.current === initialEnabled)
    ) {
      return;
    }

    lastInitialDataRef.current = initialData;
    lastInitialEnabledRef.current = initialEnabled;

    const expectedConfig =
      typeof initialData.settingsConfig.config === "string"
        ? initialData.settingsConfig.config
        : "";
    const inferredHasCommon = hasTomlCommonConfigSnippet(
      expectedConfig,
      commonConfigSnippet,
    );
    // 优先级：显式设置的 initialEnabled > 从配置推断的值
    // 如果 initialEnabled 为 undefined，使用推断值
    const hasCommon =
      initialEnabled !== undefined ? initialEnabled : inferredHasCommon;

    setCommonConfigError("");
    setUseCommonConfig(hasCommon);
    // 无条件登记：即使 codexConfig 当前已经是 expectedConfig，也要让同步 effect
    // 跳过这一轮推断，否则片段为空/不匹配时会把刚设好的勾选状态又推断成 false。
    syncGuard.schedule(codexConfig, expectedConfig);
  }, [
    codexConfig,
    commonConfigSnippet,
    initialData,
    initialEnabled,
    isLoading,
  ]);

  // 新建模式：如果通用配置片段存在且有效，默认启用
  useEffect(() => {
    if (initialData || isLoading || hasInitializedNewMode.current) {
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
      syncGuard.schedule(codexConfig, updatedConfig);
      onConfigChange(updatedConfig);
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
    onConfigChange,
    parseCommonConfigSnippet,
  ]);

  // 处理通用配置开关
  const handleCommonConfigToggle = useCallback(
    async (checked: boolean) => {
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
      syncGuard.schedule(codexConfig, updatedConfig);
      onConfigChange(updatedConfig);
    },
    [
      codexConfig,
      commonConfigSnippet,
      isTomlOpStale,
      onConfigChange,
      parseCommonConfigSnippet,
      t,
    ],
  );

  // 处理通用配置片段变化
  const handleCommonConfigSnippetChange = useCallback(
    async (value: string): Promise<boolean> => {
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

          syncGuard.schedule(codexConfig, updatedConfig);
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

        syncGuard.schedule(codexConfig, addResult.updatedConfig);
        onConfigChange(addResult.updatedConfig);
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
      onConfigChange,
      parseCommonConfigSnippet,
      t,
      useCommonConfig,
    ],
  );

  // 配置变化同步 effect：跳过程序性写入的回显，其余按内容推断勾选状态，
  // 让用户手动增删片段时勾选框跟着变。
  useEffect(() => {
    if (isLoading || syncGuard.skip(codexConfig)) {
      return;
    }
    const parsedSnippet = parseCommonConfigSnippet(commonConfigSnippet);
    if (parsedSnippet.error) {
      setUseCommonConfig(false);
      return;
    }
    // 没有片段可比对时，推断不出任何信息——保持当前勾选状态，
    // 否则会把 meta.commonConfigEnabled 静默改写成 false。
    if (!parsedSnippet.hasContent) return;

    const hasCommon = hasTomlCommonConfigSnippet(
      codexConfig,
      commonConfigSnippet,
    );
    setUseCommonConfig(hasCommon);
  }, [codexConfig, commonConfigSnippet, isLoading, parseCommonConfigSnippet]);

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

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
  // 用于识别 initialData 被 live 配置替换后的“codexConfig 重置窗口”
  const lastInitialDataRef = useRef<typeof initialData | undefined>(undefined);
  const lastInitialEnabledRef = useRef<boolean | undefined>(undefined);
  const preResetConfigRef = useRef<string | null>(null);
  const pendingResetConfigRef = useRef<string | null>(null);
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

  // 编辑态初始化 / live 配置刷新：勾选状态以 meta.commonConfigEnabled 为准，
  // 记录 codexConfig 即将重置到的快照值，由下面的同步 effect 等待重置落地后补回预览。
  useEffect(() => {
    if (!initialData?.settingsConfig || isLoading) return;
    if (
      lastInitialDataRef.current === initialData &&
      lastInitialEnabledRef.current === initialEnabled
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
    const hasCommon =
      initialEnabled !== undefined ? initialEnabled : inferredHasCommon;

    setCommonConfigError("");
    setUseCommonConfig(hasCommon);
    preResetConfigRef.current = codexConfig;
    pendingResetConfigRef.current = hasCommon ? expectedConfig : null;
  }, [
    initialData,
    initialEnabled,
    commonConfigSnippet,
    isLoading,
    codexConfig,
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
    const base = codexConfig;
    syncGuard.begin(base);
    (async () => {
      const { updatedConfig, error } = await applyTomlSnippet(
        base,
        commonConfigSnippet,
        true,
      );
      if (cancelled || isTomlOpStale(seq, base)) {
        syncGuard.abort();
        return;
      }
      if (error) {
        syncGuard.abort();
        setCommonConfigError(error);
        setUseCommonConfig(false);
        return;
      }

      setCommonConfigError("");
      setUseCommonConfig(true);
      syncGuard.schedule(base, updatedConfig);
      onConfigChange(updatedConfig);
    })();
    return () => {
      cancelled = true;
      syncGuard.abort();
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

      const base = codexConfig;
      syncGuard.begin(base);
      const { updatedConfig, error: snippetError } = await applyTomlSnippet(
        base,
        commonConfigSnippet,
        checked,
      );
      if (isTomlOpStale(seq, base)) {
        syncGuard.abort();
        return;
      }

      if (snippetError) {
        syncGuard.abort();
        setCommonConfigError(snippetError);
        setUseCommonConfig(false);
        return;
      }

      setCommonConfigError("");
      setUseCommonConfig(checked);
      syncGuard.schedule(base, updatedConfig);
      onConfigChange(updatedConfig);
    },
    [
      codexConfig,
      commonConfigSnippet,
      isTomlOpStale,
      onConfigChange,
      parseCommonConfigSnippet,
      t,
      syncGuard,
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
          const base = codexConfig;
          let updatedConfig = base;
          syncGuard.begin(base);

          if (!previousParsed.error && previousParsed.hasContent) {
            const removeResult = await applyTomlSnippet(
              base,
              previousSnippet,
              false,
            );
            if (isTomlOpStale(seq, base)) {
              syncGuard.abort();
              return false;
            }
            if (removeResult.error) {
              syncGuard.abort();
              setCommonConfigError(removeResult.error);
              return false;
            }
            updatedConfig = removeResult.updatedConfig;
          }

          syncGuard.schedule(base, updatedConfig);
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
        const base = codexConfig;
        let nextConfig = base;
        const previousParsed = parseCommonConfigSnippet(previousSnippet);
        syncGuard.begin(base);

        if (!previousParsed.error && previousParsed.hasContent) {
          const removeResult = await applyTomlSnippet(
            base,
            previousSnippet,
            false,
          );
          if (isTomlOpStale(seq, base)) {
            syncGuard.abort();
            return false;
          }
          if (removeResult.error) {
            syncGuard.abort();
            setCommonConfigError(removeResult.error);
            return false;
          }
          nextConfig = removeResult.updatedConfig;
        }

        const addResult = await applyTomlSnippet(nextConfig, value, true);
        // nextConfig 派生自发起时的 base，基线校验仍对 base 做
        if (isTomlOpStale(seq, base)) {
          syncGuard.abort();
          return false;
        }

        if (addResult.error) {
          syncGuard.abort();
          setCommonConfigError(addResult.error);
          return false;
        }

        syncGuard.schedule(base, addResult.updatedConfig);
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
      syncGuard,
    ],
  );

  // 配置变化同步 effect：先处理程序性写入回显，再处理 codexConfig 重置窗口，
  // 最后才进行用户编辑触发的内容推断（兼容没有 commonConfigEnabled 的旧供应商）。
  useEffect(() => {
    if (isLoading) return;
    if (syncGuard.skip(codexConfig)) return;

    // initialData 被 live 配置替换后，useCodexConfigState 会把 codexConfig 重置为
    // DB/live 快照（快照按设计不含片段）。等到重置值落地后把片段补回编辑预览。
    if (pendingResetConfigRef.current !== null) {
      const expected = pendingResetConfigRef.current;
      if (codexConfig === expected) {
        pendingResetConfigRef.current = null;
        preResetConfigRef.current = null;

        const parsedSnippet = parseCommonConfigSnippet(commonConfigSnippet);
        if (parsedSnippet.error) {
          if (commonConfigSnippet.trim()) {
            setCommonConfigError(parsedSnippet.error);
          }
          setUseCommonConfig(false);
          return;
        }
        const hasCommon =
          initialEnabled !== undefined
            ? initialEnabled
            : hasTomlCommonConfigSnippet(codexConfig, commonConfigSnippet);
        if (
          hasCommon &&
          !hasTomlCommonConfigSnippet(codexConfig, commonConfigSnippet) &&
          parsedSnippet.hasContent
        ) {
          const base = codexConfig;
          const seq = ++tomlOpSeqRef.current;
          syncGuard.begin(base);
          (async () => {
            const { updatedConfig, error } = await applyTomlSnippet(
              base,
              commonConfigSnippet,
              true,
            );
            if (isTomlOpStale(seq, base)) {
              syncGuard.abort();
              return;
            }
            if (error) {
              syncGuard.abort();
              setCommonConfigError(error);
              return;
            }
            syncGuard.schedule(base, updatedConfig);
            onConfigChange(updatedConfig);
          })();
          return;
        }
        return;
      }

      if (codexConfig === preResetConfigRef.current) {
        // 还没等到 codexConfig 重置
        return;
      }

      // 重置没有如期发生（或用户先编辑了），清除窗口并按当前值继续处理
      pendingResetConfigRef.current = null;
      preResetConfigRef.current = null;
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
    isLoading,
    parseCommonConfigSnippet,
    initialData,
    initialEnabled,
    isTomlOpStale,
    onConfigChange,
    syncGuard,
  ]);

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

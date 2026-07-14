// NOTE: Codex 1M 上下文 UI 已暂时隐藏（详见下方 CodexConfigSection 内 JSX 注释）。
// 如需恢复，请同时：
//   - 取消下面 `@/utils/providerConfigUtils` import 的注释
import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import {
  isCodexGoalModeEnabled,
  isCodexRemoteCompactionEnabled,
  setCodexGoalMode,
  setCodexRemoteCompaction,
} from "@/utils/providerConfigUtils";
/*
import {
  extractCodexTopLevelInt,
  setCodexTopLevelInt,
  removeCodexTopLevelField,
} from "@/utils/providerConfigUtils";
*/

interface CodexAuthSectionProps {
  appId?: "codex" | "grok";
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  error?: string;
  isProxyTakeover?: boolean;
}

/**
 * CodexAuthSection - Auth JSON editor section
 */
export const CodexAuthSection: React.FC<CodexAuthSectionProps> = ({
  appId = "codex",
  value,
  onChange,
  onBlur,
  error,
  isProxyTakeover = false,
}) => {
  const { t } = useTranslation();
  const [isDarkMode, setIsDarkMode] = useState(false);

  useEffect(() => {
    setIsDarkMode(document.documentElement.classList.contains("dark"));

    const observer = new MutationObserver(() => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  const handleChange = (newValue: string) => {
    onChange(newValue);
    if (onBlur) {
      onBlur();
    }
  };

  return (
    <div className="space-y-2">
      <label
        htmlFor="codexAuth"
        className="block text-sm font-medium text-foreground"
      >
        {t(appId === "grok" ? "grokConfig.authJson" : "codexConfig.authJson", {
          defaultValue:
            appId === "grok" ? "Grok API Key (JSON) *" : "auth.json (JSON) *",
        })}
      </label>

      <JsonEditor
        value={value}
        onChange={handleChange}
        placeholder={t(
          appId === "grok"
            ? "grokConfig.authJsonPlaceholder"
            : "codexConfig.authJsonPlaceholder",
        )}
        darkMode={isDarkMode}
        rows={6}
        showValidation={true}
        language="json"
      />

      {error && (
        <p className="text-xs text-red-500 dark:text-red-400">{error}</p>
      )}

      {!error && (
        <p className="text-xs text-muted-foreground">
          {t(
            appId === "grok"
              ? "grokConfig.authJsonHint"
              : isProxyTakeover
                ? "codexConfig.authJsonStorageHint"
                : "codexConfig.authJsonHint",
            {
              defaultValue:
                appId === "grok"
                  ? "供应商共享 API Key；每个 [model.*] 也可单独设置 api_key 或 env_key"
                  : undefined,
            },
          )}
        </p>
      )}
    </div>
  );
};

interface CodexConfigSectionProps {
  appId?: "codex" | "grok";
  showCodexFeatures?: boolean;
  onEditGrokGlobalConfig?: () => void;
  onAddGrokGlobalConfig?: () => void;
  isAddingGrokGlobalConfig?: boolean;
  value: string;
  onChange: (value: string) => void;
  providerName?: string;
  showRemoteCompaction?: boolean;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  onEditCommonConfig: () => void;
  commonConfigError?: string;
  configError?: string;
  isProxyTakeover?: boolean;
}

/**
 * CodexConfigSection - Config TOML editor section
 */
export const CodexConfigSection: React.FC<CodexConfigSectionProps> = ({
  appId = "codex",
  showCodexFeatures = true,
  onEditGrokGlobalConfig,
  onAddGrokGlobalConfig,
  isAddingGrokGlobalConfig = false,
  value,
  onChange,
  providerName,
  showRemoteCompaction = true,
  useCommonConfig,
  onCommonConfigToggle,
  onEditCommonConfig,
  commonConfigError,
  configError,
  isProxyTakeover = false,
}) => {
  const { t } = useTranslation();
  const [isDarkMode, setIsDarkMode] = useState(false);

  useEffect(() => {
    setIsDarkMode(document.documentElement.classList.contains("dark"));

    const observer = new MutationObserver(() => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  // Mirror value prop to local state (same pattern as CommonConfigEditor)
  const [localValue, setLocalValue] = useState(value);
  const localValueRef = useRef(value);
  useEffect(() => {
    setLocalValue(value);
    localValueRef.current = value;
  }, [value]);

  const handleLocalChange = useCallback(
    (newValue: string) => {
      if (newValue === localValueRef.current) return;
      localValueRef.current = newValue;
      setLocalValue(newValue);
      onChange(newValue);
    },
    [onChange],
  );

  const goalModeEnabled = useMemo(
    () => isCodexGoalModeEnabled(localValue),
    [localValue],
  );
  const remoteCompactionEnabled = useMemo(
    () => isCodexRemoteCompactionEnabled(localValue),
    [localValue],
  );

  const handleGoalModeToggle = useCallback(
    (checked: boolean) => {
      handleLocalChange(setCodexGoalMode(localValueRef.current || "", checked));
    },
    [handleLocalChange],
  );

  const handleRemoteCompactionToggle = useCallback(
    (checked: boolean) => {
      handleLocalChange(
        setCodexRemoteCompaction(
          localValueRef.current || "",
          checked,
          providerName,
        ),
      );
    },
    [handleLocalChange, providerName],
  );

  // Codex 1M 上下文相关状态/回调暂时禁用——见同文件下方 JSX 注释处的恢复说明。
  /*
  // Parse toggle states from TOML text
  const toggleStates = useMemo(() => {
    const contextWindow = extractCodexTopLevelInt(
      localValue,
      "model_context_window",
    );
    const compactLimit = extractCodexTopLevelInt(
      localValue,
      "model_auto_compact_token_limit",
    );
    return {
      contextWindow1M: contextWindow === 1000000,
      compactLimit: compactLimit ?? 900000,
    };
  }, [localValue]);

  // Debounce timer for compact limit input
  const compactTimerRef = useRef<ReturnType<typeof setTimeout>>();

  const handleContextWindowToggle = useCallback(
    (checked: boolean) => {
      let toml = localValueRef.current || "";
      if (checked) {
        toml = setCodexTopLevelInt(toml, "model_context_window", 1000000);
        // Auto-set compact limit if not already present
        if (
          extractCodexTopLevelInt(toml, "model_auto_compact_token_limit") ===
          undefined
        ) {
          toml = setCodexTopLevelInt(
            toml,
            "model_auto_compact_token_limit",
            900000,
          );
        }
      } else {
        toml = removeCodexTopLevelField(toml, "model_context_window");
        toml = removeCodexTopLevelField(toml, "model_auto_compact_token_limit");
      }
      handleLocalChange(toml);
    },
    [handleLocalChange],
  );

  const handleCompactLimitChange = useCallback(
    (inputValue: string) => {
      clearTimeout(compactTimerRef.current);
      compactTimerRef.current = setTimeout(() => {
        const num = parseInt(inputValue, 10);
        if (!Number.isNaN(num) && num > 0) {
          handleLocalChange(
            setCodexTopLevelInt(
              localValueRef.current || "",
              "model_auto_compact_token_limit",
              num,
            ),
          );
        }
      }, 500);
    },
    [handleLocalChange],
  );

  // Cleanup debounce timer
  useEffect(() => {
    return () => clearTimeout(compactTimerRef.current);
  }, []);
  */

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <label
          htmlFor="codexConfig"
          className="block text-sm font-medium text-foreground"
        >
          {t(
            appId === "grok"
              ? "grokConfig.profileToml"
              : "codexConfig.configToml",
            { defaultValue: "config.toml (TOML)" },
          )}
        </label>

        {showCodexFeatures && (
          <div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-1">
            <label className="inline-flex cursor-pointer items-center gap-2 text-sm text-muted-foreground">
              <input
                type="checkbox"
                checked={goalModeEnabled}
                onChange={(e) => handleGoalModeToggle(e.target.checked)}
                className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
              />
              {t("codexConfig.enableGoalMode")}
            </label>

            {showRemoteCompaction && (
              <label
                className="inline-flex cursor-pointer items-center gap-2 text-sm text-muted-foreground"
                title={t("codexConfig.remoteCompactionHint")}
              >
                <input
                  type="checkbox"
                  checked={remoteCompactionEnabled}
                  onChange={(e) =>
                    handleRemoteCompactionToggle(e.target.checked)
                  }
                  className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
                />
                {t("codexConfig.enableRemoteCompaction")}
              </label>
            )}

            <label className="inline-flex cursor-pointer items-center gap-2 text-sm text-muted-foreground">
              <input
                type="checkbox"
                checked={useCommonConfig}
                onChange={(e) => onCommonConfigToggle(e.target.checked)}
                className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
              />
              {t("codexConfig.writeCommonConfig")}
            </label>
          </div>
        )}
      </div>

      {showCodexFeatures && (
        <div className="flex items-center justify-end">
          <button
            type="button"
            onClick={onEditCommonConfig}
            className="text-xs text-blue-500 dark:text-blue-400 hover:underline"
          >
            {t("codexConfig.editCommonConfig")}
          </button>
        </div>
      )}
      {appId === "grok" && onEditGrokGlobalConfig && (
        <div className="flex items-center justify-end gap-3">
          {onAddGrokGlobalConfig && (
            <button
              type="button"
              onClick={onAddGrokGlobalConfig}
              disabled={isAddingGrokGlobalConfig}
              className="text-xs text-blue-500 hover:underline disabled:cursor-not-allowed disabled:opacity-50 dark:text-blue-400"
            >
              {isAddingGrokGlobalConfig
                ? t("common.saving")
                : t("grokConfig.addToGlobalConfig")}
            </button>
          )}
          <button
            type="button"
            onClick={onEditGrokGlobalConfig}
            className="text-xs text-blue-500 hover:underline dark:text-blue-400"
          >
            {t("grokConfig.editGlobalConfig")}
          </button>
        </div>
      )}

      {showCodexFeatures && commonConfigError && (
        <p className="text-xs text-red-500 dark:text-red-400 text-right">
          {commonConfigError}
        </p>
      )}

      {/* Codex 1M 上下文 UI 已隐藏：模型不再支持该字段。
          恢复方法：(1) 取消本段 JSX 注释；(2) 取消文件顶部 import 中 useMemo / extractCodexTopLevelInt / setCodexTopLevelInt / removeCodexTopLevelField 的注释；(3) 取消下方 toggleStates / compactTimerRef / handleContextWindowToggle / handleCompactLimitChange / cleanup useEffect 的注释。
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
          <input
            type="checkbox"
            checked={toggleStates.contextWindow1M}
            onChange={(e) => handleContextWindowToggle(e.target.checked)}
            className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
          />
          <span>{t("codexConfig.contextWindow1M")}</span>
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-muted-foreground">
          <span>{t("codexConfig.autoCompactLimit")}:</span>
          <input
            type="text"
            inputMode="numeric"
            pattern="[0-9]*"
            key={toggleStates.compactLimit}
            defaultValue={toggleStates.compactLimit}
            disabled={!toggleStates.contextWindow1M}
            onChange={(e) => handleCompactLimitChange(e.target.value)}
            className="w-28 h-7 px-2 text-sm rounded border border-border bg-background text-foreground disabled:opacity-50 disabled:cursor-not-allowed"
          />
        </label>
      </div>
      */}

      <JsonEditor
        value={localValue}
        onChange={handleLocalChange}
        placeholder=""
        darkMode={isDarkMode}
        rows={8}
        showValidation={false}
        language="javascript"
      />

      {configError && (
        <p className="text-xs text-red-500 dark:text-red-400">{configError}</p>
      )}

      {!configError && (
        <p className="text-xs text-muted-foreground">
          {t(
            appId === "grok"
              ? "grokConfig.profileTomlHint"
              : isProxyTakeover
                ? "codexConfig.configTomlStorageHint"
                : "codexConfig.configTomlHint",
            {
              defaultValue:
                appId === "grok"
                  ? "管理 endpoints、models、subagents 和 model.*；切换时保留其它 Grok 全局配置"
                  : undefined,
            },
          )}
        </p>
      )}
    </div>
  );
};

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
  extractCodexTopLevelInt,
  isCodexRemoteCompactionEnabled,
  removeCodexTopLevelField,
  setCodexRemoteCompaction,
  setCodexTopLevelInt,
} from "@/utils/providerConfigUtils";
import { parseWindowToken } from "./hooks/useModelState";

interface CodexAuthSectionProps {
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
        {t("codexConfig.authJson")}
      </label>

      <JsonEditor
        value={value}
        onChange={handleChange}
        placeholder={t("codexConfig.authJsonPlaceholder")}
        darkMode={isDarkMode}
        rows={3}
        showValidation={true}
        language="json"
      />

      {error && (
        <p className="text-xs text-red-500 dark:text-red-400">{error}</p>
      )}

      {!error && (
        <p className="text-xs text-muted-foreground">
          {t(
            isProxyTakeover
              ? "codexConfig.authJsonStorageHint"
              : "codexConfig.authJsonHint",
          )}
        </p>
      )}
    </div>
  );
};

interface CodexConfigSectionProps {
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
  commonConfigLoading?: boolean;
}

/**
 * CodexConfigSection - Config TOML editor section
 */
export const CodexConfigSection: React.FC<CodexConfigSectionProps> = ({
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
  commonConfigLoading = false,
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
  // 最近一次 commitTopLevelInt 产生的完整 TOML。value 回流等于它（正常
  // 回写）时原样串保留；不等（如用户直接编辑 JsonEditor）时清空原样串，
  // 避免过期 raw 在失焦时覆盖外部修改。
  const lastCommittedRef = useRef<string | null>(null);
  useEffect(() => {
    setLocalValue(value);
    localValueRef.current = value;
    if (
      lastCommittedRef.current !== null &&
      value !== lastCommittedRef.current
    ) {
      setRawTopLevelInt({});
    }
  }, [value]);

  const handleLocalChange = useCallback(
    (newValue: string) => {
      const previous = localValueRef.current;
      localValueRef.current = newValue;
      setLocalValue(newValue);
      if (newValue === previous) return;
      onChange(newValue);
    },
    [onChange],
  );

  const remoteCompactionEnabled = useMemo(
    () => isCodexRemoteCompactionEnabled(localValue),
    [localValue],
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

  // 从 config.toml 顶层提取两个独立数字输入值
  const topLevelIntValues = useMemo(
    () => ({
      contextWindow:
        extractCodexTopLevelInt(localValue, "model_context_window") ?? "",
      autoCompactLimit:
        extractCodexTopLevelInt(localValue, "model_auto_compact_token_limit") ??
        "",
    }),
    [localValue],
  );

  // 输入过程中保留用户原样字符串（支持 500K / 2m），失焦时才解析写入 TOML；
  // 未输入时回退到已存 TOML 顶层整数显示。回车只结束编辑状态，不触发表单保存。
  const [rawTopLevelInt, setRawTopLevelInt] = useState<{
    contextWindow?: string;
    autoCompactLimit?: string;
  }>({});

  const commitTopLevelInt = useCallback(
    (
      fieldName: "model_context_window" | "model_auto_compact_token_limit",
      rawKey: "contextWindow" | "autoCompactLimit",
      rawValue: string,
    ) => {
      const trimmed = rawValue.trim();
      const numericValue = trimmed ? parseWindowToken(trimmed) : undefined;
      if (trimmed && numericValue === undefined) {
        // 非法输入：不动 TOML（避免误删已存值），仅清除原样串，
        // 输入框回退到已存 TOML 顶层整数显示。
        setRawTopLevelInt((prev) => {
          if (prev[rawKey] === undefined) return prev;
          const next = { ...prev };
          delete next[rawKey];
          return next;
        });
        return;
      }
      const toml = numericValue
        ? setCodexTopLevelInt(
            localValueRef.current || "",
            fieldName,
            numericValue,
          )
        : removeCodexTopLevelField(localValueRef.current || "", fieldName);
      lastCommittedRef.current = toml;
      handleLocalChange(toml);
      setRawTopLevelInt((prev) => {
        const next = { ...prev };
        if (numericValue !== undefined) {
          // 合法 K/M 输入：解析值已写入 TOML，输入框继续原样显示。
          next[rawKey] = trimmed;
        } else {
          // 空输入：已从 TOML 移除该字段，回退到空显示。
          delete next[rawKey];
        }
        return next;
      });
    },
    [handleLocalChange],
  );

  const commitTopLevelIntFromKeyDown = useCallback(
    (
      event: React.KeyboardEvent<HTMLInputElement>,
      rawKey: "contextWindow" | "autoCompactLimit",
    ) => {
      if (event.key !== "Enter") return;
      event.preventDefault();
      event.stopPropagation();
      const fieldName =
        rawKey === "contextWindow"
          ? "model_context_window"
          : "model_auto_compact_token_limit";
      const raw = rawTopLevelInt[rawKey];
      if (raw !== undefined) {
        commitTopLevelInt(fieldName, rawKey, raw);
      }
      event.currentTarget.blur();
    },
    [commitTopLevelInt, rawTopLevelInt],
  );

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <label
          htmlFor="codexConfig"
          className="block text-sm font-medium text-foreground"
        >
          {t("codexConfig.configToml")}
        </label>

        <div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-1">
          {showRemoteCompaction && (
            <label
              className="inline-flex cursor-pointer items-center gap-2 text-sm text-muted-foreground"
              title={t("codexConfig.remoteCompactionHint")}
            >
              <input
                type="checkbox"
                checked={remoteCompactionEnabled}
                onChange={(e) => handleRemoteCompactionToggle(e.target.checked)}
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
              disabled={commonConfigLoading}
              className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
            />
            {t("codexConfig.writeCommonConfig")}
          </label>
        </div>
      </div>

      <div className="flex items-center justify-end">
        <button
          type="button"
          onClick={onEditCommonConfig}
          className="text-xs text-blue-500 dark:text-blue-400 hover:underline"
        >
          {t("codexConfig.editCommonConfig")}
        </button>
      </div>

      {commonConfigError && (
        <p className="text-xs text-red-500 dark:text-red-400 text-right">
          {commonConfigError}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <label className="inline-flex items-center gap-2 text-sm text-muted-foreground">
          <span>{t("codexConfig.contextWindow")}</span>
          <input
            type="text"
            inputMode="numeric"
            pattern="[0-9]*[KkMm]?"
            aria-label={t("codexConfig.contextWindow")}
            value={
              rawTopLevelInt.contextWindow ?? topLevelIntValues.contextWindow
            }
            placeholder={t("codexConfig.contextWindowPlaceholder")}
            onChange={(e) => {
              // e.currentTarget 在 handler 返回后被置空，updater 于渲染期才执行，
              // 必须先同步取值再进函数式 setState。
              const next = e.currentTarget.value;
              setRawTopLevelInt((prev) => ({ ...prev, contextWindow: next }));
            }}
            onBlur={() => {
              const raw = rawTopLevelInt.contextWindow;
              if (raw === undefined) return;
              commitTopLevelInt("model_context_window", "contextWindow", raw);
            }}
            onKeyDown={(event) =>
              commitTopLevelIntFromKeyDown(event, "contextWindow")
            }
            className="w-32 h-7 px-2 text-sm rounded border border-border bg-background text-foreground"
          />
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-muted-foreground">
          <span>{t("codexConfig.autoCompactLimit")}</span>
          <input
            type="text"
            inputMode="numeric"
            pattern="[0-9]*[KkMm]?"
            aria-label={t("codexConfig.autoCompactLimit")}
            value={
              rawTopLevelInt.autoCompactLimit ??
              topLevelIntValues.autoCompactLimit
            }
            placeholder={t("codexConfig.autoCompactLimitPlaceholder")}
            onChange={(e) => {
              const next = e.currentTarget.value;
              setRawTopLevelInt((prev) => ({
                ...prev,
                autoCompactLimit: next,
              }));
            }}
            onBlur={() => {
              const raw = rawTopLevelInt.autoCompactLimit;
              if (raw === undefined) return;
              commitTopLevelInt(
                "model_auto_compact_token_limit",
                "autoCompactLimit",
                raw,
              );
            }}
            onKeyDown={(event) =>
              commitTopLevelIntFromKeyDown(event, "autoCompactLimit")
            }
            className="w-32 h-7 px-2 text-sm rounded border border-border bg-background text-foreground"
          />
        </label>
      </div>

      <p className="text-xs text-muted-foreground">
        {t("codexConfig.contextWindowGlobalHint", {
          defaultValue:
            "全局上下文大小：填写后所有模型统一使用该长度；不填则以每个模型填写的上下文为准。压缩比例默认 0.95。",
        })}
      </p>

      <JsonEditor
        value={localValue}
        onChange={handleLocalChange}
        placeholder=""
        darkMode={isDarkMode}
        rows={3}
        showValidation={false}
        language="javascript"
      />

      {configError && (
        <p className="text-xs text-red-500 dark:text-red-400">{configError}</p>
      )}

      {!configError && (
        <p className="text-xs text-muted-foreground">
          {t(
            isProxyTakeover
              ? "codexConfig.configTomlStorageHint"
              : "codexConfig.configTomlHint",
          )}
        </p>
      )}
    </div>
  );
};

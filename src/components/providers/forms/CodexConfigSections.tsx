import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import type { ProviderEditorInactiveField } from "@/lib/api/providers";
import { InactiveFieldsPanel } from "./InactiveFieldsPanel";
import {
  extractCodexTopLevelInt,
  isCodexRemoteCompactionEnabled,
  removeCodexTopLevelField,
  setCodexRemoteCompaction,
  setCodexTopLevelInt,
} from "@/utils/providerConfigUtils";
import { fieldClass } from "@/components/ui/input";
import { cn } from "@/lib/utils";

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
        className="block text-sm font-medium text-fg-1"
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

      {error && <p className="text-xs text-danger-text">{error}</p>}

      {!error && (
        <p className="text-xs text-fg-2">
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
  configError?: string;
  isProxyTakeover?: boolean;
  /** 行里保存着、但不随切换生效的全局设置（值是可以照抄的 TOML）。 */
  inactiveFields?: ProviderEditorInactiveField[];
}

/**
 * CodexConfigSection - Config TOML editor section
 */
export const CodexConfigSection: React.FC<CodexConfigSectionProps> = ({
  value,
  onChange,
  providerName,
  showRemoteCompaction = true,
  configError,
  isProxyTakeover = false,
  inactiveFields = [],
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

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <label
          htmlFor="codexConfig"
          className="block text-sm font-medium text-fg-1"
        >
          {t("codexConfig.configToml")}
        </label>

        <div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-1">
          {showRemoteCompaction && (
            <label
              className="inline-flex cursor-pointer items-center gap-2 text-sm text-fg-2"
              title={t("codexConfig.remoteCompactionHint")}
            >
              <input
                type="checkbox"
                checked={remoteCompactionEnabled}
                onChange={(e) => handleRemoteCompactionToggle(e.target.checked)}
                className="ui-checkbox"
              />
              {t("codexConfig.enableRemoteCompaction")}
            </label>
          )}
        </div>
      </div>

      <p className="text-xs text-fg-2">
        {t("codexConfig.keyFieldsHint", {
          defaultValue:
            "地址、Key、模型、推理档位、上下文窗口和兼容开关随供应商切换；其余是 Codex 全局设置，保存后对所有供应商生效。",
        })}
      </p>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <label className="inline-flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input
            type="checkbox"
            checked={toggleStates.contextWindow1M}
            onChange={(e) => handleContextWindowToggle(e.target.checked)}
            className="ui-checkbox"
          />
          <span>{t("codexConfig.contextWindow1M")}</span>
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-fg-2">
          <span>{t("codexConfig.autoCompactLimit")}:</span>
          <input
            type="text"
            inputMode="numeric"
            pattern="[0-9]*"
            key={toggleStates.compactLimit}
            defaultValue={toggleStates.compactLimit}
            disabled={!toggleStates.contextWindow1M}
            onChange={(e) => handleCompactLimitChange(e.target.value)}
            className={cn(fieldClass, "h-7 w-28 px-2")}
          />
        </label>
      </div>

      <JsonEditor
        value={localValue}
        onChange={handleLocalChange}
        placeholder=""
        darkMode={isDarkMode}
        rows={3}
        showValidation={false}
        language="javascript"
      />

      <InactiveFieldsPanel
        fields={inactiveFields}
        hint={t("codexConfig.inactiveFieldsHint", {
          count: inactiveFields.length,
          defaultValue:
            "这个供应商还保存着 {{count}} 个不随切换生效的设置。点击复制它的 TOML，按需粘贴到上方；供应商里保存的原值不会删除。",
        })}
        action={{
          kind: "copy",
          copiedText: t("codexConfig.inactiveFieldCopied", {
            defaultValue: "已复制",
          }),
        }}
      />

      {configError && <p className="text-xs text-danger-text">{configError}</p>}

      {!configError && (
        <p className="text-xs text-fg-2">
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

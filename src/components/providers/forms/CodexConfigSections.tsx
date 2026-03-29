import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import {
  extractCodexTopLevelInt,
  setCodexTopLevelInt,
  removeCodexTopLevelField,
} from "@/utils/providerConfigUtils";

interface CodexAuthSectionProps {
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  error?: string;
}

/**
 * CodexAuthSection - Auth JSON editor section
 */
export const CodexAuthSection: React.FC<CodexAuthSectionProps> = ({
  value,
  onChange,
  onBlur,
  error,
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
        rows={6}
        showValidation={true}
        language="json"
      />

      {error && (
        <p className="text-xs text-red-500 dark:text-red-400">{error}</p>
      )}

      {!error && (
        <p className="text-xs text-muted-foreground">
          {t("codexConfig.authJsonHint")}
        </p>
      )}
    </div>
  );
};

interface CodexConfigSectionProps {
  value: string;
  onChange: (value: string) => void;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  onEditCommonConfig: () => void;
  commonConfigError?: string;
  configError?: string;
}

/**
 * CodexConfigSection - Config TOML editor section
 */
export const CodexConfigSection: React.FC<CodexConfigSectionProps> = ({
  value,
  onChange,
  useCommonConfig,
  onCommonConfigToggle,
  onEditCommonConfig,
  commonConfigError,
  configError,
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
        toml = removeCodexTopLevelField(
          toml,
          "model_auto_compact_token_limit",
        );
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
      <div className="flex items-center justify-between">
        <label
          htmlFor="codexConfig"
          className="block text-sm font-medium text-foreground"
        >
          {t("codexConfig.configToml")}
        </label>

        <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
          <input
            type="checkbox"
            checked={useCommonConfig}
            onChange={(e) => onCommonConfigToggle(e.target.checked)}
            className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default  rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
          />
          {t("codexConfig.writeCommonConfig")}
        </label>
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
          {t("codexConfig.configTomlHint")}
        </p>
      )}
    </div>
  );
};

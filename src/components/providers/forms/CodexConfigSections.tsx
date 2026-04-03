import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
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
        <p className="text-xs text-[hsl(var(--destructive))]">{error}</p>
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

function ToggleChip({
  checked,
  label,
  onCheckedChange,
}: {
  checked: boolean;
  label: string;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <label className="inline-flex cursor-pointer items-center gap-2 rounded-full border border-border/70 bg-background/55 px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:border-border-hover hover:text-foreground">
      <Checkbox
        checked={checked}
        onCheckedChange={(nextChecked) => onCheckedChange(nextChecked === true)}
      />
      <span>{label}</span>
    </label>
  );
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
    <div className="space-y-4 rounded-[calc(var(--radius)+0.25rem)] border border-border/70 bg-card/45 p-4 shadow-sm">
      <div className="flex items-center justify-between gap-3">
        <div className="space-y-1">
          <label
            htmlFor="codexConfig"
            className="block text-sm font-medium text-foreground"
          >
            {t("codexConfig.configToml")}
          </label>
          <p className="text-xs text-muted-foreground">
            {t("codexConfig.configTomlHint")}
          </p>
        </div>

        <ToggleChip
          checked={useCommonConfig}
          onCheckedChange={onCommonConfigToggle}
          label={t("codexConfig.writeCommonConfig")}
        />
      </div>

      <div className="flex items-center justify-end">
        <Button
          type="button"
          onClick={onEditCommonConfig}
          variant="link"
          size="sm"
          className="h-auto px-0 py-0 text-xs"
        >
          {t("codexConfig.editCommonConfig")}
        </Button>
      </div>

      {commonConfigError && (
        <p className="text-xs text-[hsl(var(--destructive))] text-right">
          {commonConfigError}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <ToggleChip
          checked={toggleStates.contextWindow1M}
          onCheckedChange={handleContextWindowToggle}
          label={t("codexConfig.contextWindow1M")}
        />
        <div className="inline-flex items-center gap-2 rounded-full border border-border/70 bg-background/55 px-3 py-1.5 text-sm text-muted-foreground">
          <span>{t("codexConfig.autoCompactLimit")}:</span>
          <Input
            type="text"
            inputMode="numeric"
            pattern="[0-9]*"
            key={toggleStates.compactLimit}
            defaultValue={toggleStates.compactLimit}
            disabled={!toggleStates.contextWindow1M}
            onChange={(e) => handleCompactLimitChange(e.target.value)}
            className="h-7 w-28 border-border/70 bg-background/70 px-2 text-sm"
          />
        </div>
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
        <p className="text-xs text-[hsl(var(--destructive))]">{configError}</p>
      )}
    </div>
  );
};

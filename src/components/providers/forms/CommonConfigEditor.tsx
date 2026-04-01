import { useTranslation } from "react-i18next";
import { useEffect, useState, useCallback, useMemo } from "react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Save, Download, Loader2, Package } from "lucide-react";
import JsonEditor from "@/components/JsonEditor";

interface CommonConfigEditorProps {
  value: string;
  onChange: (value: string) => void;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  commonConfigSnippet: string;
  onCommonConfigSnippetChange: (value: string) => void;
  commonConfigError: string;
  onEditClick: () => void;
  isModalOpen: boolean;
  onModalClose: () => void;
  onExtract?: () => void;
  isExtracting?: boolean;
}

interface ConfigToggleProps {
  checked: boolean;
  label: string;
  onCheckedChange: (checked: boolean) => void;
}

function ConfigToggle({ checked, label, onCheckedChange }: ConfigToggleProps) {
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

export function CommonConfigEditor({
  value,
  onChange,
  useCommonConfig,
  onCommonConfigToggle,
  commonConfigSnippet,
  onCommonConfigSnippetChange,
  commonConfigError,
  onEditClick,
  isModalOpen,
  onModalClose,
  onExtract,
  isExtracting,
}: CommonConfigEditorProps) {
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

  // Mirror value prop to local state so checkbox toggles and JsonEditor stay in sync
  // (parent uses form.getValues which doesn't trigger re-renders)
  const [localValue, setLocalValue] = useState(value);

  useEffect(() => {
    setLocalValue(value);
  }, [value]);

  const handleLocalChange = useCallback(
    (newValue: string) => {
      setLocalValue(newValue);
      onChange(newValue);
    },
    [onChange],
  );

  const toggleStates = useMemo(() => {
    try {
      const config = JSON.parse(localValue);
      return {
        hideAttribution:
          config?.attribution?.commit === "" && config?.attribution?.pr === "",
        teammates:
          config?.env?.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === "1" ||
          config?.env?.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === 1,
        enableToolSearch:
          config?.env?.ENABLE_TOOL_SEARCH === "true" ||
          config?.env?.ENABLE_TOOL_SEARCH === "1",
        effortHigh: config?.effortLevel === "high",
        disableAutoUpgrade:
          config?.env?.DISABLE_AUTOUPDATER === "1" ||
          config?.env?.DISABLE_AUTOUPDATER === 1,
      };
    } catch {
      return {
        hideAttribution: false,
        teammates: false,
        enableToolSearch: false,
        effortHigh: false,
        disableAutoUpgrade: false,
      };
    }
  }, [localValue]);

  const handleToggle = useCallback(
    (toggleKey: string, checked: boolean) => {
      try {
        const config = JSON.parse(localValue || "{}");

        switch (toggleKey) {
          case "hideAttribution":
            if (checked) {
              config.attribution = { commit: "", pr: "" };
            } else {
              delete config.attribution;
            }
            break;
          case "teammates":
            if (!config.env) config.env = {};
            if (checked) {
              config.env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = "1";
            } else {
              delete config.env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS;
              if (Object.keys(config.env).length === 0) delete config.env;
            }
            break;
          case "enableToolSearch":
            if (!config.env) config.env = {};
            if (checked) {
              config.env.ENABLE_TOOL_SEARCH = "true";
            } else {
              delete config.env.ENABLE_TOOL_SEARCH;
              if (Object.keys(config.env).length === 0) delete config.env;
            }
            break;
          case "effortHigh":
            if (checked) {
              config.effortLevel = "high";
            } else {
              delete config.effortLevel;
            }
            break;
          case "disableAutoUpgrade":
            if (!config.env) config.env = {};
            if (checked) {
              config.env.DISABLE_AUTOUPDATER = "1";
            } else {
              delete config.env.DISABLE_AUTOUPDATER;
              if (Object.keys(config.env).length === 0) delete config.env;
            }
            break;
        }

        handleLocalChange(JSON.stringify(config, null, 2));
      } catch {
        // Don't modify if JSON is invalid
      }
    },
    [localValue, handleLocalChange],
  );

  return (
    <>
      <div className="space-y-4 rounded-[calc(var(--radius)+0.25rem)] border border-border/70 bg-card/45 p-4 shadow-sm">
        <div className="flex items-center justify-between gap-3">
          <div className="space-y-1">
            <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("claudeConfig.commonConfigHint", {
                defaultValue: "通用配置片段将合并到所有启用它的供应商配置中",
              })}
            </p>
          </div>
          <label className="inline-flex cursor-pointer items-center gap-2 rounded-full border border-border/70 bg-background/55 px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:border-border-hover hover:text-foreground">
            <Checkbox
              id="useCommonConfig"
              checked={useCommonConfig}
              onCheckedChange={(checked) =>
                onCommonConfigToggle(checked === true)
              }
            />
            <span>
              {t("claudeConfig.writeCommonConfig", {
                defaultValue: "写入通用配置",
              })}
            </span>
          </label>
        </div>

        <div className="flex items-center justify-end">
          <Button
            type="button"
            onClick={onEditClick}
            variant="link"
            size="sm"
            className="h-auto px-0 py-0 text-xs"
          >
            {t("claudeConfig.editCommonConfig", {
              defaultValue: "编辑通用配置",
            })}
          </Button>
        </div>
        {commonConfigError && !isModalOpen && (
          <p className="text-right text-xs text-[hsl(var(--destructive))]">
            {commonConfigError}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <ConfigToggle
            checked={toggleStates.hideAttribution}
            onCheckedChange={(checked) =>
              handleToggle("hideAttribution", checked)
            }
            label={t("claudeConfig.hideAttribution")}
          />
          <ConfigToggle
            checked={toggleStates.teammates}
            onCheckedChange={(checked) => handleToggle("teammates", checked)}
            label={t("claudeConfig.enableTeammates")}
          />
          <ConfigToggle
            checked={toggleStates.enableToolSearch}
            onCheckedChange={(checked) =>
              handleToggle("enableToolSearch", checked)
            }
            label={t("claudeConfig.enableToolSearch")}
          />
          <ConfigToggle
            checked={toggleStates.effortHigh}
            onCheckedChange={(checked) => handleToggle("effortHigh", checked)}
            label={t("claudeConfig.effortHigh")}
          />
          <ConfigToggle
            checked={toggleStates.disableAutoUpgrade}
            onCheckedChange={(checked) =>
              handleToggle("disableAutoUpgrade", checked)
            }
            label={t("claudeConfig.disableAutoUpgrade")}
          />
        </div>
        <JsonEditor
          value={localValue}
          onChange={handleLocalChange}
          placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://your-api-endpoint.com",
    "ANTHROPIC_AUTH_TOKEN": "your-api-key-here"
  }
}`}
          darkMode={isDarkMode}
          rows={14}
          showValidation={true}
          language="json"
        />
      </div>

      <FullScreenPanel
        isOpen={isModalOpen}
        title={t("claudeConfig.editCommonConfigTitle", {
          defaultValue: "编辑通用配置片段",
        })}
        onClose={onModalClose}
        footer={
          <>
            {onExtract && (
              <Button
                type="button"
                variant="outline"
                onClick={onExtract}
                disabled={isExtracting}
                className="gap-2"
              >
                {isExtracting ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Download className="w-4 h-4" />
                )}
                {t("claudeConfig.extractFromCurrent", {
                  defaultValue: "从编辑内容提取",
                })}
              </Button>
            )}
            <Button type="button" variant="outline" onClick={onModalClose}>
              {t("common.cancel")}
            </Button>
            <Button type="button" onClick={onModalClose} className="gap-2">
              <Save className="w-4 h-4" />
              {t("common.save")}
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          <div className="rounded-lg border border-blue-200 dark:border-blue-800 bg-blue-50/50 dark:bg-blue-950/30 p-3 space-y-1.5">
            <p className="text-sm font-medium text-blue-800 dark:text-blue-300">
              {t("commonConfig.guideTitle")}
            </p>
            <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
              {t("commonConfig.guidePurpose")}
            </p>
            <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
              {t("commonConfig.guideUsage")}
            </p>
            <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
              {t("commonConfig.guideReExtract")}
            </p>
            <p className="text-xs text-muted-foreground">
              {t("commonConfig.guideReassurance")}
            </p>
          </div>
          {(!commonConfigSnippet ||
            commonConfigSnippet.trim() === "" ||
            commonConfigSnippet.trim() === "{}") && (
            <div className="flex flex-col items-center justify-center py-6 text-center text-muted-foreground">
              <Package className="h-8 w-8 mb-2 opacity-40" />
              <p className="text-sm font-medium">
                {t("commonConfig.emptyTitle")}
              </p>
              <p className="text-xs mt-1">{t("commonConfig.emptyHint")}</p>
            </div>
          )}
          <JsonEditor
            value={commonConfigSnippet}
            onChange={onCommonConfigSnippetChange}
            placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://your-api-endpoint.com"
  }
}`}
            darkMode={isDarkMode}
            rows={16}
            showValidation={true}
            language="json"
          />
          {commonConfigError && (
            <p className="text-sm text-[hsl(var(--destructive))]">
              {commonConfigError}
            </p>
          )}
        </div>
      </FullScreenPanel>
    </>
  );
}

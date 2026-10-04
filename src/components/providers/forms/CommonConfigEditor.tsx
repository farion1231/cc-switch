import { useTranslation } from "react-i18next";
import { useEffect, useState, useCallback, useMemo } from "react";
import { Label } from "@/components/ui/label";
import JsonEditor from "@/components/JsonEditor";
import type { ProviderEditorInactiveField } from "@/lib/api/providers";
import { InactiveFieldsPanel } from "./InactiveFieldsPanel";

interface CommonConfigEditorProps {
  value: string;
  onChange: (value: string) => void;
  /** 行里保存着、但不随切换生效的字段；可以一键加进上方 JSON（保存后成为全局设置）。 */
  inactiveFields?: ProviderEditorInactiveField[];
}

/** 把一个字段写进 JSON 文本；解析不了就原样返回。 */
const setFieldInConfig = (
  configText: string,
  field: ProviderEditorInactiveField,
): string | null => {
  try {
    const config = JSON.parse(configText || "{}") as Record<string, unknown>;
    let target: Record<string, unknown> = config;
    for (const segment of field.path.slice(0, -1)) {
      const next = target[segment];
      if (typeof next !== "object" || next === null || Array.isArray(next)) {
        target[segment] = {};
      }
      target = target[segment] as Record<string, unknown>;
    }
    target[field.path[field.path.length - 1]] = field.value;
    return JSON.stringify(config, null, 2);
  } catch {
    return null;
  }
};

export function CommonConfigEditor({
  value,
  onChange,
  inactiveFields = [],
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
          config?.attribution?.commit === "" &&
          config?.attribution?.pr === "" &&
          config?.attribution?.sessionUrl === false,
        teammates:
          config?.env?.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === "1" ||
          config?.env?.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === 1,
        enableToolSearch:
          config?.env?.ENABLE_TOOL_SEARCH === "true" ||
          config?.env?.ENABLE_TOOL_SEARCH === "1",
        effortMax: config?.env?.CLAUDE_CODE_EFFORT_LEVEL === "max",
        disableAutoUpgrade:
          config?.env?.DISABLE_AUTOUPDATER === "1" ||
          config?.env?.DISABLE_AUTOUPDATER === 1,
        disableArtifact:
          config?.env?.CLAUDE_CODE_DISABLE_ARTIFACT === "1" ||
          config?.env?.CLAUDE_CODE_DISABLE_ARTIFACT === 1,
      };
    } catch {
      return {
        hideAttribution: false,
        teammates: false,
        enableToolSearch: false,
        effortMax: false,
        disableAutoUpgrade: false,
        disableArtifact: false,
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
              config.attribution = { commit: "", pr: "", sessionUrl: false };
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
          case "effortMax":
            if (!config.env) config.env = {};
            if (checked) {
              config.env.CLAUDE_CODE_EFFORT_LEVEL = "max";
            } else {
              delete config.env.CLAUDE_CODE_EFFORT_LEVEL;
              if (Object.keys(config.env).length === 0) delete config.env;
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
          case "disableArtifact":
            // 第三方网关（如 DeepSeek）用严格 JSON Schema 校验工具定义，
            // Artifact 工具灰度中的 \p{..} 正则会让每个请求 400；
            // 该变量让 Claude Code 压根不把 Artifact 放进 tools 数组。
            if (!config.env) config.env = {};
            if (checked) {
              config.env.CLAUDE_CODE_DISABLE_ARTIFACT = "1";
            } else {
              delete config.env.CLAUDE_CODE_DISABLE_ARTIFACT;
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

  const handleAddInactiveField = useCallback(
    (field: ProviderEditorInactiveField) => {
      const next = setFieldInConfig(localValue, field);
      if (next !== null) {
        handleLocalChange(next);
      }
    },
    [localValue, handleLocalChange],
  );

  const pendingInactiveFields = useMemo(() => {
    try {
      const config = JSON.parse(localValue || "{}") as Record<string, unknown>;
      return inactiveFields.filter((field) => {
        let current: unknown = config;
        for (const segment of field.path) {
          if (typeof current !== "object" || current === null) return true;
          current = (current as Record<string, unknown>)[segment];
        }
        return JSON.stringify(current) !== JSON.stringify(field.value);
      });
    } catch {
      return inactiveFields;
    }
  }, [inactiveFields, localValue]);

  return (
    <div className="space-y-2">
      <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
      <p className="text-xs text-fg-2">
        {t("claudeConfig.keyFieldsHint", {
          defaultValue:
            "地址、Key、模型、上下文窗口和兼容开关随供应商切换；其余字段是 Claude Code 全局设置，保存后对所有供应商生效。",
        })}
      </p>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <label className="inline-flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input
            type="checkbox"
            checked={toggleStates.hideAttribution}
            onChange={(e) => handleToggle("hideAttribution", e.target.checked)}
            className="ui-checkbox"
          />
          <span>{t("claudeConfig.hideAttribution")}</span>
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input
            type="checkbox"
            checked={toggleStates.teammates}
            onChange={(e) => handleToggle("teammates", e.target.checked)}
            className="ui-checkbox"
          />
          <span>{t("claudeConfig.enableTeammates")}</span>
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input
            type="checkbox"
            checked={toggleStates.enableToolSearch}
            onChange={(e) => handleToggle("enableToolSearch", e.target.checked)}
            className="ui-checkbox"
          />
          <span>{t("claudeConfig.enableToolSearch")}</span>
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input
            type="checkbox"
            checked={toggleStates.effortMax}
            onChange={(e) => handleToggle("effortMax", e.target.checked)}
            className="ui-checkbox"
          />
          <span>{t("claudeConfig.effortMax")}</span>
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input
            type="checkbox"
            checked={toggleStates.disableAutoUpgrade}
            onChange={(e) =>
              handleToggle("disableAutoUpgrade", e.target.checked)
            }
            className="ui-checkbox"
          />
          <span>{t("claudeConfig.disableAutoUpgrade")}</span>
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-fg-2 cursor-pointer">
          <input
            type="checkbox"
            checked={toggleStates.disableArtifact}
            onChange={(e) => handleToggle("disableArtifact", e.target.checked)}
            className="ui-checkbox"
          />
          <span>{t("claudeConfig.disableArtifact")}</span>
        </label>
      </div>
      <JsonEditor
        value={localValue}
        onChange={handleLocalChange}
        ariaLabel={t("provider.configJson")}
        placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://your-api-endpoint.com",
    "ANTHROPIC_AUTH_TOKEN": "your-api-key-here"
  }
}`}
        darkMode={isDarkMode}
        rows={3}
        showValidation={true}
        language="json"
      />
      <InactiveFieldsPanel
        fields={pendingInactiveFields}
        hint={t("claudeConfig.inactiveFieldsHint", {
          count: pendingInactiveFields.length,
          defaultValue:
            "这个供应商还保存着 {{count}} 个不随切换生效的字段。点击可加入上方的全局设置，保存后写入配置文件；供应商里保存的原值不会删除。",
        })}
        action={{
          kind: "add",
          title: t("claudeConfig.addToGlobalSettings", {
            defaultValue: "加入全局设置",
          }),
          onAdd: handleAddInactiveField,
        }}
      />
    </div>
  );
}

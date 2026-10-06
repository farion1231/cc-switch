import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { GeminiEnvSection, GeminiConfigSection } from "./GeminiConfigSections";
import type { ProviderEditorInactiveField } from "@/lib/api/providers";
import { InactiveFieldsPanel } from "./InactiveFieldsPanel";

interface GeminiConfigEditorProps {
  envValue: string;
  configValue: string;
  onEnvChange: (value: string) => void;
  onConfigChange: (value: string) => void;
  onEnvBlur?: () => void;
  envError: string;
  configError: string;
  /** 行里保存着、但不随切换生效的设置。 */
  inactiveFields?: ProviderEditorInactiveField[];
}

/** `.env` 文本里这个变量现在的值（最后一次定义为准）。 */
const envValueOf = (envText: string, key: string): string | undefined => {
  let value: string | undefined;
  for (const line of envText.split("\n")) {
    const trimmed = line.trim().replace(/^export\s+/, "");
    const index = trimmed.indexOf("=");
    if (index > 0 && trimmed.slice(0, index).trim() === key) {
      value = trimmed.slice(index + 1).trim();
    }
  }
  return value;
};

/** 把变量写进 `.env` 文本：已有就改第一处、删掉重复的，没有就追加。 */
const setEnvLine = (envText: string, key: string, value: string): string => {
  const lines = envText.trim() ? envText.replace(/\n+$/, "").split("\n") : [];
  let written = false;
  const next = lines.flatMap((line) => {
    const trimmed = line.trim().replace(/^export\s+/, "");
    const index = trimmed.indexOf("=");
    if (index <= 0 || trimmed.slice(0, index).trim() !== key) return [line];
    if (written) return [];
    written = true;
    return [`${key}=${value}`];
  });
  if (!written) next.push(`${key}=${value}`);
  return next.join("\n");
};

const parseConfig = (configText: string): Record<string, unknown> | null => {
  try {
    const parsed = JSON.parse(configText.trim() || "{}");
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
};

const GeminiConfigEditor: React.FC<GeminiConfigEditorProps> = ({
  envValue,
  configValue,
  onEnvChange,
  onConfigChange,
  onEnvBlur,
  envError,
  configError,
  inactiveFields = [],
}) => {
  const { t } = useTranslation();

  // 已经加进上方编辑器（值相同）的不再列出。
  const pendingInactiveFields = useMemo(() => {
    const config = parseConfig(configValue);
    return inactiveFields.filter((field) => {
      const [scope, key] = field.path;
      if (scope === "env") {
        return envValueOf(envValue, key) !== String(field.value);
      }
      if (scope === "config" && config) {
        return JSON.stringify(config[key]) !== JSON.stringify(field.value);
      }
      return true;
    });
  }, [inactiveFields, envValue, configValue]);

  const handleAddInactiveField = (field: ProviderEditorInactiveField) => {
    const [scope, key] = field.path;
    if (scope === "env") {
      onEnvChange(setEnvLine(envValue, key, String(field.value)));
      return;
    }
    const config = parseConfig(configValue);
    if (scope === "config" && config) {
      onConfigChange(
        JSON.stringify({ ...config, [key]: field.value }, null, 2),
      );
    }
  };

  return (
    <div className="space-y-6">
      <p className="text-xs text-fg-2">
        {t("geminiConfig.keyFieldsHint", {
          defaultValue:
            "地址、Key、模型名和认证方式随供应商切换；其余环境变量和 settings.json 是 Gemini CLI 全局设置，保存后对所有供应商生效。",
        })}
      </p>

      <GeminiEnvSection
        value={envValue}
        onChange={onEnvChange}
        onBlur={onEnvBlur}
        error={envError}
      />

      <GeminiConfigSection
        value={configValue}
        onChange={onConfigChange}
        configError={configError}
      />

      <InactiveFieldsPanel
        fields={pendingInactiveFields}
        hint={t("geminiConfig.inactiveFieldsHint", {
          count: pendingInactiveFields.length,
          defaultValue:
            "这个供应商还保存着 {{count}} 个不随切换生效的设置。点击可加入上方的全局设置，保存后写入配置文件；供应商里保存的原值不会删除。",
        })}
        action={{
          kind: "add",
          title: t("geminiConfig.addToGlobalSettings", {
            defaultValue: "加入全局设置",
          }),
          onAdd: handleAddInactiveField,
        }}
      />
    </div>
  );
};

export default GeminiConfigEditor;

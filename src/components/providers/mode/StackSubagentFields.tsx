import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Settings } from "@/types";
import type { ProxyStackSubagentModel } from "@/types/proxy";
import { settingsApi } from "@/lib/api";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { SettingsSwitchRow } from "@/components/settings/SettingsLayout";

// Radix Select 不收空值：这两个哨兵代表「不写这个键」。
const FOLLOW_MAIN = "__follow_main__";
const MODEL_DEFAULT = "__model_default__";

const warningClass =
  "rounded-control bg-warning-soft px-3 py-2 text-caption text-warning-text";

export interface StackSubagentValue {
  model?: string;
  effort?: string;
  classic: boolean;
}

/** 子代理设置 → settings.json 字段（未设置的键写 undefined，保存时去掉）。 */
export function stackSubagentSettings(
  value: StackSubagentValue,
): Partial<Settings> {
  return {
    codexStackSubagentModel: value.model,
    codexStackSubagentReasoningEffort: value.effort,
    codexStackClassicTools: value.classic,
  };
}

/** settings.json → 子代理设置。 */
export function stackSubagentValue(settings?: Settings): StackSubagentValue {
  return {
    model: settings?.codexStackSubagentModel || undefined,
    effort: settings?.codexStackSubagentReasoningEffort || undefined,
    classic: settings?.codexStackClassicTools ?? true,
  };
}

export function sameStackSubagent(
  a: StackSubagentValue,
  b: StackSubagentValue,
): boolean {
  return (
    a.model === b.model && a.effort === b.effort && a.classic === b.classic
  );
}

interface StackSubagentFieldsProps {
  /** 能选的模型：已去掉默认那家（它不发布 `ccs-*` 模型） */
  models: ProxyStackSubagentModel[];
  value: StackSubagentValue;
  onChange: (next: StackSubagentValue) => void;
  disabled?: boolean;
}

/**
 * Codex 聚合的子代理模型、推理强度和经典子代理开关。聚合设置面板（改了就保存）和切换到
 * 聚合的确认框（确认时一起保存）共用。
 */
export function StackSubagentFields({
  models,
  value,
  onChange,
  disabled,
}: StackSubagentFieldsProps) {
  const { t } = useTranslation();
  const [forcesMultiAgentV2, setForcesMultiAgentV2] = useState(false);
  const { model, effort, classic } = value;
  const selected = models.find((candidate) => candidate.id === model);
  // 选过的模型不在名单里了（成员移除、模型不再发布、成了默认那家）：后端不写，跟随主模型
  const unavailable = model !== undefined && selected === undefined;

  // config.toml 用 [features] multi_agent_v2 强制了新版工具时，经典开关不生效
  useEffect(() => {
    if (!classic) {
      setForcesMultiAgentV2(false);
      return;
    }
    let cancelled = false;
    void settingsApi
      .codexForcesMultiAgentV2()
      .catch(() => false)
      .then((forced) => {
        if (!cancelled) setForcesMultiAgentV2(forced);
      });
    return () => {
      cancelled = true;
    };
  }, [classic]);

  const handleModelChange = (next: string) => {
    if (next === FOLLOW_MAIN) {
      onChange({ ...value, model: undefined, effort: undefined });
      return;
    }
    const candidate = models.find((entry) => entry.id === next);
    // 换模型后旧强度不在新模型的档位里就清掉（否则后端也不会写）
    const keepEffort =
      effort !== undefined && candidate?.efforts.includes(effort)
        ? effort
        : undefined;
    onChange({ ...value, model: next, effort: keepEffort });
  };

  const handleEffortChange = (next: string) => {
    onChange({ ...value, effort: next === MODEL_DEFAULT ? undefined : next });
  };

  const modelLabel = t("mode.stackSettings.subagentModel");
  const effortLabel = t("mode.stackSettings.subagentEffort");
  const effortValue =
    effort !== undefined && selected?.efforts.includes(effort)
      ? effort
      : MODEL_DEFAULT;

  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <label className="text-caption font-semibold text-fg-2">
          {modelLabel}
        </label>
        <Select
          value={model ?? FOLLOW_MAIN}
          onValueChange={handleModelChange}
          disabled={disabled}
        >
          <SelectTrigger className="h-9" aria-label={modelLabel}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="z-[70]">
            <SelectItem value={FOLLOW_MAIN}>
              {t("mode.stackSettings.followMain")}
            </SelectItem>
            {models.map((candidate) => (
              <SelectItem key={candidate.id} value={candidate.id}>
                {candidate.displayName}
              </SelectItem>
            ))}
            {unavailable && (
              <SelectItem value={model}>
                {t("mode.stackSettings.unavailableOption", { model })}
              </SelectItem>
            )}
          </SelectContent>
        </Select>
        {models.length === 0 && (
          <p className="text-caption text-fg-3">
            {t("mode.stackSettings.noModels")}
          </p>
        )}
      </div>

      <div className="space-y-1.5">
        <label className="text-caption font-semibold text-fg-2">
          {effortLabel}
        </label>
        <Select
          value={effortValue}
          onValueChange={handleEffortChange}
          disabled={disabled || selected === undefined}
        >
          <SelectTrigger className="h-9" aria-label={effortLabel}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="z-[70]">
            <SelectItem value={MODEL_DEFAULT}>
              {selected?.defaultEffort
                ? t("mode.stackSettings.effortModelDefaultNamed", {
                    effort: selected.defaultEffort,
                  })
                : t("mode.stackSettings.effortModelDefault")}
            </SelectItem>
            {(selected?.efforts ?? []).map((level) => (
              <SelectItem key={level} value={level}>
                {level}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {unavailable && (
        <p data-testid="stack-subagent-unavailable" className={warningClass}>
          {t("mode.stackSettings.unavailable")}
        </p>
      )}
      {selected && !classic && (
        <p data-testid="stack-subagent-classic-off" className={warningClass}>
          {t("mode.stackSettings.classicOff")}
        </p>
      )}

      <SettingsSwitchRow
        label={t("settings.codexStackClassicSubagents")}
        help={{
          title: t("settings.codexStackClassicSubagents"),
          body: t("settings.codexStackClassicSubagentsDescription"),
        }}
        description={
          forcesMultiAgentV2 ? (
            <span className="text-warning-text">
              {t("settings.codexStackClassicSubagentsForcedV2")}
            </span>
          ) : undefined
        }
        checked={classic}
        disabled={disabled}
        onCheckedChange={(next) => onChange({ ...value, classic: next })}
      />
    </div>
  );
}

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Feather } from "lucide-react";
import type { SettingsFormState } from "@/hooks/useSettings";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";

export const DEFAULT_AUTO_LIGHTWEIGHT_AFTER_MINUTES = 1;
export const MIN_AUTO_LIGHTWEIGHT_AFTER_MINUTES = 1;
export const MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES = 24 * 60;

interface AutoLightweightSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
  disabled?: boolean;
}

const parseMinutes = (value: string): number | null => {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) return null;

  const parsed = Number(trimmed);
  if (
    !Number.isSafeInteger(parsed) ||
    parsed < MIN_AUTO_LIGHTWEIGHT_AFTER_MINUTES ||
    parsed > MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES
  ) {
    return null;
  }
  return parsed;
};

export function AutoLightweightSettings({
  settings,
  onChange,
  disabled = false,
}: AutoLightweightSettingsProps) {
  const { t } = useTranslation();
  const storedMinutes = settings.autoLightweightAfterMinutes;
  const enabled = settings.autoLightweightEnabled;
  const [draftMinutes, setDraftMinutes] = useState(String(storedMinutes));
  const [hasError, setHasError] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setDraftMinutes(String(storedMinutes));
    setHasError(false);
  }, [storedMinutes]);

  const commit = (nextEnabled: boolean) => {
    if (disabled) return;

    const parsedMinutes = parseMinutes(draftMinutes);
    if (nextEnabled && parsedMinutes === null) {
      setHasError(true);
      return;
    }

    const minutes = parsedMinutes ?? storedMinutes;
    setDraftMinutes(String(minutes));
    setHasError(false);
    if (nextEnabled === enabled && minutes === storedMinutes) return;

    void onChange({
      autoLightweightEnabled: nextEnabled,
      autoLightweightAfterMinutes: minutes,
    });
  };

  return (
    <div className="rounded-xl border border-border bg-card/50 p-4 transition-colors hover:bg-muted/50">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-background ring-1 ring-border">
            <Feather className="h-4 w-4 text-emerald-500" />
          </div>
          <div className="min-w-0 space-y-1">
            <p className="text-sm font-medium leading-none">
              {t("settings.autoLightweightMode")}
            </p>
            <p className="text-xs text-muted-foreground">
              {t("settings.autoLightweightModeDescription")}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2 whitespace-nowrap text-sm">
          <label
            htmlFor="auto-lightweight-minutes"
            className="text-xs text-muted-foreground"
          >
            {t("settings.autoLightweightDelay")}
          </label>
          <Input
            ref={inputRef}
            id="auto-lightweight-minutes"
            className="h-8 w-20 px-2 text-center tabular-nums"
            type="number"
            inputMode="numeric"
            min={MIN_AUTO_LIGHTWEIGHT_AFTER_MINUTES}
            max={MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES}
            step={1}
            value={draftMinutes}
            disabled={!enabled || disabled}
            aria-invalid={hasError}
            aria-describedby={
              hasError ? "auto-lightweight-minutes-error" : undefined
            }
            onChange={(event) => {
              setDraftMinutes(event.target.value);
              if (hasError) setHasError(false);
            }}
            onBlur={() => commit(enabled)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.currentTarget.blur();
              } else if (event.key === "Escape") {
                setDraftMinutes(String(storedMinutes));
                setHasError(false);
              }
            }}
          />
          <span className="text-xs text-muted-foreground">
            {t("settings.minutesUnit")}
          </span>
          <Switch
            className="ml-1"
            checked={enabled}
            onCheckedChange={commit}
            onPointerDown={(event) => {
              if (document.activeElement === inputRef.current) {
                // 保持输入框焦点直到 click；由开关的一次合并保存同时提交草稿
                // 和启用状态，避免 blur 保存与 click 保存并发覆盖。
                event.preventDefault();
              }
            }}
            disabled={disabled}
            aria-label={t("settings.autoLightweightMode")}
          />
        </div>
      </div>
      {hasError ? (
        <p
          id="auto-lightweight-minutes-error"
          className="mt-2 text-right text-xs text-destructive"
          role="alert"
        >
          {t("settings.autoLightweightMinutesRange", {
            min: MIN_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
            max: MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
          })}
        </p>
      ) : null}
    </div>
  );
}

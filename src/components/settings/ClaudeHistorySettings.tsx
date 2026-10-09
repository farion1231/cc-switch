import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SettingsRow } from "@/components/settings/SettingsLayout";
import { settingsApi, type ClaudeHistoryRetention } from "@/lib/api/settings";

export function ClaudeHistorySettings() {
  const { t } = useTranslation();
  const inputId = useId();
  const errorId = useId();
  const [current, setCurrent] = useState<ClaudeHistoryRetention | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [reload, setReload] = useState(0);
  const busy = useRef(false);
  const mounted = useRef(false);

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    setLoading(true);
    setCurrent(null);
    setError(false);
    settingsApi.getClaudeHistoryRetention().then(
      (value) => {
        if (cancelled) return;
        setCurrent(value);
        setDraft(String(value.days ?? 30));
        setLoading(false);
      },
      () => {
        if (cancelled) return;
        setError(true);
        setLoading(false);
      },
    );
    return () => {
      cancelled = true;
      mounted.current = false;
    };
  }, [reload]);

  const days = Number(draft);
  const valid = /^\d+$/.test(draft) && Number.isSafeInteger(days) && days > 0;
  const disabled = loading || saving || !current;

  const save = async (next: number | null) => {
    if (!current || busy.current) return;
    busy.current = true;
    setSaving(true);
    setError(false);
    try {
      const value = await settingsApi.setClaudeHistoryRetention(current, next);
      if (!mounted.current) return;
      setCurrent(value);
      setDraft(String(value.days ?? 30));
    } catch {
      if (mounted.current) setError(true);
    } finally {
      busy.current = false;
      if (mounted.current) setSaving(false);
    }
  };

  return (
    <SettingsRow
      label={t("settings.claudeHistoryRetention.label")}
      description={current?.configPath}
      htmlFor={inputId}
      help={{
        title: t("settings.claudeHistoryRetention.label"),
        body: t("settings.claudeHistoryRetention.help"),
      }}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Input
          id={inputId}
          type="number"
          min={1}
          max={Number.MAX_SAFE_INTEGER}
          step={1}
          className="w-[140px]"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          disabled={disabled}
          aria-invalid={!loading && !!current && !valid}
          aria-describedby={
            error || (!disabled && !valid) ? errorId : undefined
          }
        />
        <span className="text-body text-fg-2">
          {t("settings.claudeHistoryRetention.days")}
        </span>
        <Button
          variant="solid"
          size="compact"
          disabled={disabled || !valid || days === (current?.days ?? 30)}
          onClick={() => void save(days)}
        >
          {t(saving ? "common.saving" : "common.save")}
        </Button>
        <Button
          variant="quiet"
          size="compact"
          disabled={disabled || current?.days === null}
          onClick={() => void save(null)}
        >
          {t("settings.claudeHistoryRetention.reset")}
        </Button>
        {error && (
          <Button
            variant="quiet"
            size="compact"
            disabled={saving || loading}
            onClick={() => setReload((value) => value + 1)}
          >
            {t("common.retry")}
          </Button>
        )}
      </div>
      {(error || (!disabled && !valid)) && (
        <p id={errorId} role="alert" className="mt-2 text-caption text-danger">
          {t(
            error
              ? "settings.claudeHistoryRetention.error"
              : "settings.claudeHistoryRetention.invalid",
          )}
        </p>
      )}
    </SettingsRow>
  );
}

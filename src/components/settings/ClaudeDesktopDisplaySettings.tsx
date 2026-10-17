import { useCallback, useEffect, useRef, useState } from "react";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { useTranslation } from "react-i18next";
import type { ClaudeDesktopDisplay } from "@/types";

interface ClaudeDesktopDisplaySettingsProps {
  value: ClaudeDesktopDisplay | null;
  onChange: (value: ClaudeDesktopDisplay | null) => void;
}

const EMPTY_DISPLAY: ClaudeDesktopDisplay = {
  name: "",
  subtitle: "",
  attribution: false,
};

/** Typing is pushed upstream at most this long after the last keystroke. */
const AUTOSAVE_DELAY_MS = 400;

const isSame = (a: ClaudeDesktopDisplay, b: ClaudeDesktopDisplay) =>
  a.name === b.name &&
  a.subtitle === b.subtitle &&
  a.attribution === b.attribution;

/**
 * Holds the editable draft of the display settings.
 *
 * The fields never write through to the prop on every keystroke: the settings
 * query refetches after each save and can resolve with the snapshot taken
 * before that save landed, so a controlled value would overwrite whatever the
 * user is still typing. Edits update the draft immediately, a debounced save
 * pushes the draft upstream, and an incoming prop only becomes the new draft
 * once it is at least as fresh as the draft we last put forward.
 */
function useDisplayDraft(
  value: ClaudeDesktopDisplay | null,
  onChange: (value: ClaudeDesktopDisplay) => void,
) {
  const [draft, setDraft] = useState<ClaudeDesktopDisplay>(
    value ?? EMPTY_DISPLAY,
  );
  // The last saved value, so toggling the feature off and back on restores what
  // was there instead of clearing it.
  const [lastValues, setLastValues] = useState<ClaudeDesktopDisplay>(
    value ?? EMPTY_DISPLAY,
  );
  // The newest draft that has been sent upstream or is queued to be: an
  // incoming prop that differs from it is a refetch of an older snapshot.
  const [inFlight, setInFlight] = useState<ClaudeDesktopDisplay | null>(null);
  const draftRef = useRef(draft);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearTimer = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = null;
  }, []);

  useEffect(() => clearTimer, [clearTimer]);

  useEffect(() => {
    if (!value) {
      // Turning the feature off drops the fields; a pending save would write
      // values that are no longer on screen.
      if (inFlight) {
        clearTimer();
        setInFlight(null);
      }
      return;
    }
    // Stale refetch, or an external update arriving before the draft has gone
    // out: the draft owns the fields until it is caught up.
    if (inFlight && !isSame(value, inFlight)) return;
    if (inFlight) setInFlight(null);
    draftRef.current = value;
    setDraft(value);
    setLastValues(value);
  }, [value, inFlight, clearTimer]);

  const send = useCallback(
    (next: ClaudeDesktopDisplay) => {
      clearTimer();
      draftRef.current = next;
      setDraft(next);
      setLastValues(next);
      setInFlight(next);
      onChange(next);
    },
    [clearTimer, onChange],
  );

  // Text edits: draft now, save once the user pauses.
  const edit = useCallback(
    (patch: Partial<ClaudeDesktopDisplay>) => {
      const next = { ...draftRef.current, ...patch };
      draftRef.current = next;
      setDraft(next);
      setInFlight(next);
      clearTimer();
      timerRef.current = setTimeout(() => send(next), AUTOSAVE_DELAY_MS);
    },
    [clearTimer, send],
  );

  // Switches have no intermediate states to lose, so they save right away.
  const apply = useCallback(
    (patch: Partial<ClaudeDesktopDisplay>) => {
      send({ ...draftRef.current, ...patch });
    },
    [send],
  );

  return { draft, lastValues, edit, apply };
}

export function ClaudeDesktopDisplaySettings({
  value,
  onChange,
}: ClaudeDesktopDisplaySettingsProps) {
  const { t } = useTranslation();
  const enabled = value !== null;
  const { draft, lastValues, edit, apply } = useDisplayDraft(value, onChange);

  const setEnabled = (on: boolean) => {
    onChange(on ? lastValues : null);
  };

  return (
    <section className="space-y-3">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.claudeDesktopDisplay")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.claudeDesktopDisplayHint")}
        </p>
      </header>
      <div className="flex items-center justify-between">
        <Label htmlFor="cdd-enable" className="text-sm">
          {t("settings.claudeDesktopDisplayEnable")}
        </Label>
        <Switch
          id="cdd-enable"
          checked={enabled}
          onCheckedChange={setEnabled}
        />
      </div>
      {enabled && (
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="cdd-name" className="text-sm">
              {t("settings.claudeDesktopDisplayName")}
            </Label>
            <Input
              id="cdd-name"
              value={draft.name}
              onChange={(e) => edit({ name: e.target.value })}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="cdd-subtitle" className="text-sm">
              {t("settings.claudeDesktopDisplaySubtitle")}
            </Label>
            <Input
              id="cdd-subtitle"
              value={draft.subtitle}
              onChange={(e) => edit({ subtitle: e.target.value })}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="cdd-attribution" className="text-sm">
              {t("settings.claudeDesktopDisplayAttribution")}
            </Label>
            <Switch
              id="cdd-attribution"
              checked={draft.attribution}
              onCheckedChange={(v) => apply({ attribution: v })}
            />
          </div>
        </div>
      )}
    </section>
  );
}

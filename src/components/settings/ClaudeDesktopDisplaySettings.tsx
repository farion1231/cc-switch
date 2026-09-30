import { useEffect, useState } from "react";
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

export function ClaudeDesktopDisplaySettings({
  value,
  onChange,
}: ClaudeDesktopDisplaySettingsProps) {
  const { t } = useTranslation();
  const enabled = value !== null;
  // Remember the last non-null value so toggling the feature off and back on
  // restores what was typed, instead of clearing it.
  const [lastValues, setLastValues] = useState<ClaudeDesktopDisplay>(
    () => value ?? EMPTY_DISPLAY,
  );

  useEffect(() => {
    if (value) setLastValues(value);
  }, [value]);

  const setEnabled = (on: boolean) => {
    onChange(on ? lastValues : null);
  };
  const patch = (p: Partial<ClaudeDesktopDisplay>) => {
    if (!value) return;
    onChange({ ...value, ...p });
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
        <Switch id="cdd-enable" checked={enabled} onCheckedChange={setEnabled} />
      </div>
      {enabled && (
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="cdd-name" className="text-sm">
              {t("settings.claudeDesktopDisplayName")}
            </Label>
            <Input
              id="cdd-name"
              value={value.name}
              onChange={(e) => patch({ name: e.target.value })}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="cdd-subtitle" className="text-sm">
              {t("settings.claudeDesktopDisplaySubtitle")}
            </Label>
            <Input
              id="cdd-subtitle"
              value={value.subtitle}
              onChange={(e) => patch({ subtitle: e.target.value })}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="cdd-attribution" className="text-sm">
              {t("settings.claudeDesktopDisplayAttribution")}
            </Label>
            <Switch
              id="cdd-attribution"
              checked={value.attribution}
              onCheckedChange={(v) => patch({ attribution: v })}
            />
          </div>
        </div>
      )}
    </section>
  );
}

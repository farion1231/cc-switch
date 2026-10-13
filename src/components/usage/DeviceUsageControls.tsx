import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown, Loader2 } from "lucide-react";
import type {
  DeviceUsageReport,
  DeviceUsageSettings,
} from "@/lib/api/deviceUsage";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { SourceCard } from "./UsageDataSourcesSheet";
import { getLocaleFromLanguage } from "./format";

/** Device selection occupies the same filter row as provider/model selection. */
export function DeviceUsageScope({
  scope,
  devices,
  onChange,
}: {
  scope: string;
  devices: DeviceUsageReport["devices"];
  onChange: (scope: string) => void;
}) {
  const { t } = useTranslation();
  const options = [
    { value: "local", label: t("usage.devices.local") },
    { value: "all", label: t("usage.devices.all") },
    ...devices
      .filter((d) => !d.local)
      .map((d) => ({ value: d.deviceId, label: d.deviceName })),
  ];
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="neutral"
          size="regular"
          className="min-w-[66px] max-w-[156px] shrink gap-1 pe-2 ps-3"
          aria-label={t("usage.devices.scope")}
        >
          <span className="min-w-0 truncate">
            {options.find((d) => d.value === scope)?.label ?? scope.slice(0, 8)}
          </span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-fg-2" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className="min-w-[200px] max-w-[320px] max-h-[360px] overflow-y-auto rounded-panel border-border bg-surface p-1 shadow-v7-md"
      >
        {options.map((option) => (
          <DropdownMenuItem
            key={option.value}
            className="h-[30px] gap-2 rounded-control px-2 text-body"
            onSelect={() => onChange(option.value)}
          >
            <Check
              aria-hidden="true"
              strokeWidth={1.75}
              className={cn(
                "h-3.5 w-3.5 shrink-0",
                scope !== option.value && "invisible",
              )}
            />
            <span className="min-w-0 truncate">{option.label}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** Mounted inside the existing data-sources sheet; no second sheet or page toolbar. */
export function DeviceUsageSource({
  settings,
  devices,
  saving,
  syncing,
  error,
  onConfigure,
  onSync,
}: {
  settings?: DeviceUsageSettings;
  devices: DeviceUsageReport["devices"];
  saving: boolean;
  syncing: boolean;
  error?: unknown;
  onConfigure: (enabled: boolean, name: string) => void;
  onSync: () => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(
    i18n.resolvedLanguage || i18n.language || "en",
  );
  // Keep drafts while background snapshots refresh; untouched fields follow server settings.
  const [draftName, setDraftName] = useState<string | null>(null);
  const [draftEnabled, setDraftEnabled] = useState<boolean | null>(null);
  const name = draftName ?? settings?.deviceName ?? "";
  const enabled = draftEnabled ?? settings?.enabled ?? false;
  return (
    <SourceCard
      title={t("usage.devices.settings")}
      help={{
        title: t("usage.devices.settings"),
        body: t("usage.devices.transport"),
      }}
      trailing={
        <Switch
          checked={enabled}
          onCheckedChange={setDraftEnabled}
          aria-label={t("usage.devices.enable")}
        />
      }
    >
      <label className="flex flex-col gap-1.5 text-caption text-fg-2">
        <span>{t("usage.devices.name")}</span>
        <Input
          maxLength={64}
          value={name}
          onChange={(e) => setDraftName(e.target.value)}
        />
      </label>
      <ul className="m-0 flex list-none flex-col gap-1 rounded-control bg-subtle px-3 py-2.5 text-caption text-fg-2">
        <li>{t("usage.devices.transport")}</li>
        {devices.map((d) => (
          <li key={d.deviceId}>
            {d.deviceName}
            {d.local ? ` · ${t("usage.devices.local")}` : ""}
            {d.capturedAt > 0 &&
              ` · ${new Date(d.capturedAt * 1000).toLocaleString(locale)}`}
          </li>
        ))}
      </ul>
      {error != null && (
        <p role="alert" className="m-0 text-caption text-danger-text">
          {String(error)}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={saving || !name.trim()}
          onClick={() => onConfigure(enabled, name)}
        >
          {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {t("common.save")}
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={!settings?.enabled || syncing}
          onClick={onSync}
        >
          {syncing && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {t("usage.devices.sync")}
        </Button>
      </div>
    </SourceCard>
  );
}

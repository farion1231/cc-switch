import { useTranslation } from "react-i18next";
import { ChevronRight, Loader2, RefreshCw } from "lucide-react";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { cn } from "@/lib/utils";
import { isDeviceStale } from "@/lib/query/usageSync";
import type { RemoteDeviceInfo, UsageSyncOverview } from "@/types/usageSync";
import { formatRelativeTime } from "./format";

interface UsageDevicesSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  overview?: UsageSyncOverview;
  syncing: boolean;
  onSyncNow: () => void;
  onToggleHidden: (deviceId: string, hidden: boolean) => void;
  /** 打开设置 → 云同步（用量同步开关在那里） */
  onOpenSyncSettings?: () => void;
}

type BadgeTone = "ok" | "warn" | "danger" | "muted";

const badgeClass: Record<BadgeTone, string> = {
  ok: "bg-success-soft text-success-text",
  warn: "bg-warning-soft text-warning-text",
  danger: "bg-danger-soft text-danger-text",
  muted: "bg-subtle text-fg-2",
};

function StatusBadge({ tone, label }: { tone: BadgeTone; label: string }) {
  return (
    <span
      className={cn(
        "shrink-0 rounded-[5px] px-1.5 text-badge leading-5",
        badgeClass[tone],
      )}
    >
      {label}
    </span>
  );
}

/** 设备状态 → 徽标。过期只在没有更严重的问题时显示。 */
export function deviceStatusBadge(
  device: RemoteDeviceInfo,
  staleAfterSecs: number,
  nowMs: number,
): { tone: BadgeTone; key: string } {
  switch (device.status) {
    case "hidden":
      return { tone: "muted", key: "usageSync.status.hidden" };
    case "incompatible":
      return { tone: "danger", key: "usageSync.status.incompatible" };
    case "error":
      return { tone: "danger", key: "usageSync.status.error" };
    case "missing":
      return { tone: "warn", key: "usageSync.status.missing" };
    default:
      return isDeviceStale(device, staleAfterSecs, nowMs)
        ? { tone: "warn", key: "usageSync.status.stale" }
        : { tone: "ok", key: "usageSync.status.ok" };
  }
}

function coverageText(
  device: RemoteDeviceInfo,
  t: (key: string, options?: Record<string, unknown>) => string,
): string | null {
  if (!device.firstDate || !device.lastDate) return null;
  const range = t("usageSync.devices.coverage", {
    from: device.firstDate,
    to: device.lastDate,
  });
  if (device.detailLevel === "daily") {
    return `${range} · ${t("usageSync.devices.dailyOnly")}`;
  }
  return device.detailFrom
    ? `${range} · ${t("usageSync.devices.detailFrom", { date: device.detailFrom })}`
    : range;
}

/** 「设备」抽屉：本机与已同步设备的状态、覆盖范围和错误。 */
export function UsageDevicesSheet({
  open,
  onOpenChange,
  overview,
  syncing,
  onSyncNow,
  onToggleHidden,
  onOpenSyncSettings,
}: UsageDevicesSheetProps) {
  const { t } = useTranslation();
  const now = Date.now();
  const settings = overview?.settings;
  const status = settings?.status;
  const enabled = settings?.enabled ?? false;
  const staleAfter = overview?.staleAfterSecs ?? 86_400;

  const relative = (seconds?: number | null) =>
    seconds == null
      ? t("usageSync.never")
      : formatRelativeTime(seconds * 1000, t, now);

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        width={420}
        closeLabel={t("common.close")}
        dismissOnOutsideClick
      >
        <SheetHeader className="pb-3">
          <SheetTitle>{t("usageSync.devices.title")}</SheetTitle>
          <SheetDescription>
            {t("usageSync.devices.description")}
          </SheetDescription>
        </SheetHeader>
        <SheetBody className="flex flex-col gap-3 border-t border-border">
          {!enabled && (
            <Notice
              tone="neutral"
              title={t("usageSync.devices.disabledTitle")}
              actions={
                onOpenSyncSettings ? (
                  <Button
                    variant="neutral"
                    size="compact"
                    onClick={() => {
                      onOpenChange(false);
                      onOpenSyncSettings();
                    }}
                  >
                    {t("usageSync.openSettings")}
                    <ChevronRight className="h-3.5 w-3.5" />
                  </Button>
                ) : undefined
              }
            >
              {t("usageSync.devices.disabledBody")}
            </Notice>
          )}
          {status?.identityConflict && (
            <Notice tone="danger" title={t("usageSync.conflict.title")}>
              {t("usageSync.conflict.body")}
            </Notice>
          )}
          {overview?.transportSecure === false && (
            <Notice tone="warning" title={t("usageSync.insecure.title")}>
              {t("usageSync.insecure.body")}
            </Notice>
          )}

          {overview && (
            <section className="flex flex-col gap-1.5 rounded-panel border border-border px-4 py-3">
              <div className="flex items-center gap-2">
                <h3 className="m-0 min-w-0 flex-1 truncate text-strong font-semibold text-fg-1">
                  {overview.thisDevice.deviceName}
                </h3>
                <StatusBadge tone="muted" label={t("usageSync.thisDevice")} />
              </div>
              <p className="m-0 text-caption text-fg-2">
                {enabled
                  ? t("usageSync.devices.lastUpload", {
                      time: relative(status?.lastUploadAt),
                    })
                  : t("usageSync.devices.notSharing")}
              </p>
              {status?.lastError && (
                <p className="m-0 break-words text-caption text-danger-text">
                  {status.lastError}
                </p>
              )}
            </section>
          )}

          {(overview?.devices ?? []).length === 0 ? (
            <p className="m-0 py-6 text-center text-body text-fg-3">
              {t("usageSync.devices.empty")}
            </p>
          ) : (
            <ul
              className="m-0 flex list-none flex-col gap-2 p-0"
              aria-label={t("usageSync.devices.listLabel")}
            >
              {overview?.devices.map((device) => {
                const badge = deviceStatusBadge(device, staleAfter, now);
                const coverage = coverageText(device, t);
                const hidden = device.status === "hidden";
                return (
                  <li
                    key={device.deviceId}
                    className="flex flex-col gap-1.5 rounded-panel border border-border px-4 py-3"
                  >
                    <div className="flex items-center gap-2">
                      <span
                        className="min-w-0 flex-1 truncate text-strong font-semibold text-fg-1"
                        title={device.deviceId}
                      >
                        {device.deviceName}
                      </span>
                      <StatusBadge tone={badge.tone} label={t(badge.key)} />
                    </div>
                    <p className="m-0 text-caption text-fg-2">
                      {t("usageSync.devices.lastUpload", {
                        time: relative(device.lastUploadAt),
                      })}
                      {device.platform ? ` · ${device.platform}` : ""}
                      {device.appVersion ? ` · v${device.appVersion}` : ""}
                    </p>
                    {coverage && (
                      <p className="m-0 text-caption text-fg-3">{coverage}</p>
                    )}
                    {device.lastError && device.status !== "ok" && (
                      <p className="m-0 break-words text-caption text-danger-text">
                        {device.lastError}
                      </p>
                    )}
                    <div className="flex justify-end">
                      <Button
                        type="button"
                        variant="quiet"
                        size="compact"
                        onClick={() => onToggleHidden(device.deviceId, !hidden)}
                      >
                        {hidden
                          ? t("usageSync.devices.show")
                          : t("usageSync.devices.hide")}
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}

          {enabled && (
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate text-caption text-fg-3">
                {overview?.remoteLocation ?? ""}
              </span>
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={syncing}
                onClick={onSyncNow}
              >
                {syncing ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <RefreshCw className="h-3.5 w-3.5" />
                )}
                {t("usageSync.syncNow")}
              </Button>
            </div>
          )}
          <p className="m-0 text-caption text-fg-3">
            {t("usageSync.devices.costNote")}
          </p>
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}

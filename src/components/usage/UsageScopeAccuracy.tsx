import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { isDeviceStale } from "@/lib/query/usageSync";
import type { RemoteDeviceInfo, UsageSyncOverview } from "@/types/usageSync";

/** One accuracy statement about the selected device scope. */
export type AccuracyItem =
  /** Request-level data and identity-ledger history: de-duplicated exactly. */
  | { kind: "exact" }
  /** Day-level history without request identities: counted per device. */
  | {
      kind: "unverified";
      requests: number;
      devices: {
        name: string;
        requests: number;
        from?: string | null;
        to?: string | null;
      }[];
    }
  /** Day-level only devices: partial days are not counted. */
  | { kind: "dailyOnly"; devices: string[] }
  /** Proxy/session pairs on different devices cannot be matched. */
  | { kind: "unknownOverlap" }
  /** Day-level rows of these devices use their own calendar. */
  | { kind: "calendar"; devices: { name: string; offset: string }[] }
  /** Data may be outdated / never synced / failed. */
  | { kind: "outdated"; devices: string[] }
  | { kind: "neverSynced"; devices: string[] }
  | { kind: "syncError"; error: string }
  /** Costs are estimates. */
  | { kind: "estimate" };

export function formatUtcOffset(minutes: number): string {
  const sign = minutes < 0 ? "−" : "+";
  const abs = Math.abs(minutes);
  const hours = String(Math.floor(abs / 60)).padStart(2, "0");
  const mins = String(abs % 60).padStart(2, "0");
  return `UTC${sign}${hours}:${mins}`;
}

/**
 * Accuracy statements for a scope, derived from the sync overview.
 * `device`: "local" | "all" | remote device id.
 */
export function buildScopeAccuracy(
  overview: UsageSyncOverview | undefined,
  device: string,
  thisDeviceLabel: string,
  nowMs: number = Date.now(),
): AccuracyItem[] {
  if (!overview || device === "local") return [];
  const visible = overview.devices.filter((d) => d.status !== "hidden");
  const scoped: RemoteDeviceInfo[] =
    device === "all" ? visible : visible.filter((d) => d.deviceId === device);
  if (scoped.length === 0 && device !== "all") return [];
  const items: AccuracyItem[] = [];

  if (device === "all") {
    items.push({ kind: "exact" });
    const holders = [
      {
        name: thisDeviceLabel,
        requests: overview.thisDevice.unverified.requests,
        from: overview.thisDevice.unverified.firstDate,
        to: overview.thisDevice.unverified.lastDate,
      },
      ...scoped.map((d) => ({
        name: d.deviceName,
        requests: d.unverified.requests,
        from: d.unverified.firstDate,
        to: d.unverified.lastDate,
      })),
    ].filter((h) => h.requests > 0);
    // Id-less history can overlap with any other device's copy of the same
    // requests (with or without ids), so it is flagged whenever another
    // device is in scope.
    if (holders.length > 0 && scoped.length > 0) {
      items.push({
        kind: "unverified",
        requests: holders.reduce((sum, h) => sum + h.requests, 0),
        devices: holders,
      });
    }
    items.push({ kind: "unknownOverlap" });
  }

  const dailyOnly = scoped.filter((d) => d.detailLevel === "daily");
  if (dailyOnly.length > 0) {
    items.push({
      kind: "dailyOnly",
      devices: dailyOnly.map((d) => d.deviceName),
    });
  }
  const localOffset = overview.thisDevice.utcOffsetMinutes;
  const otherCalendars = scoped.filter(
    (d) => d.utcOffsetMinutes != null && d.utcOffsetMinutes !== localOffset,
  );
  if (otherCalendars.length > 0) {
    items.push({
      kind: "calendar",
      devices: otherCalendars.map((d) => ({
        name: d.deviceName,
        offset: formatUtcOffset(d.utcOffsetMinutes ?? 0),
      })),
    });
  }
  const never = scoped.filter(
    (d) => d.lastUploadAt == null || d.lastFetchedAt == null,
  );
  if (never.length > 0) {
    items.push({
      kind: "neverSynced",
      devices: never.map((d) => d.deviceName),
    });
  }
  const outdated = scoped.filter(
    (d) =>
      !never.includes(d) &&
      (d.status !== "ok" || isDeviceStale(d, overview.staleAfterSecs, nowMs)),
  );
  if (outdated.length > 0) {
    items.push({
      kind: "outdated",
      devices: outdated.map((d) => d.deviceName),
    });
  }
  const error = overview.settings.status.lastError;
  if (error) items.push({ kind: "syncError", error });
  items.push({ kind: "estimate" });
  return items;
}

function itemText(
  item: AccuracyItem,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  switch (item.kind) {
    case "exact":
      return t("usageSync.accuracy.exact");
    case "unverified":
      return t("usageSync.accuracy.unverified", {
        count: item.requests,
        devices: item.devices
          .map((d) =>
            d.from && d.to
              ? `${d.name} ${d.requests} (${d.from} → ${d.to})`
              : `${d.name} ${d.requests}`,
          )
          .join(", "),
      });
    case "dailyOnly":
      return t("usageSync.notice.dailyOnly", {
        devices: item.devices.join(", "),
      });
    case "unknownOverlap":
      return t("usageSync.accuracy.unknownOverlap");
    case "calendar":
      return t("usageSync.accuracy.calendar", {
        devices: item.devices.map((d) => `${d.name} (${d.offset})`).join(", "),
      });
    case "outdated":
      return t("usageSync.notice.outdated", {
        devices: item.devices.join(", "),
      });
    case "neverSynced":
      return t("usageSync.accuracy.neverSynced", {
        devices: item.devices.join(", "),
      });
    case "syncError":
      return t("usageSync.accuracy.syncError", { error: item.error });
    case "estimate":
      return t("usageSync.accuracy.estimate");
  }
}

const toneClass: Partial<Record<AccuracyItem["kind"], string>> = {
  unverified: "text-warning-text",
  outdated: "text-warning-text",
  neverSynced: "text-warning-text",
  syncError: "text-danger-text",
};

/** Accuracy statement shown under the filter row for non-local scopes. */
export function UsageScopeAccuracy({ items }: { items: AccuracyItem[] }) {
  const { t } = useTranslation();
  if (items.length === 0) return null;
  const warnings = items.filter((i) => toneClass[i.kind] != null).length;
  return (
    <details className="group mx-6 mb-2 rounded-panel border border-border px-3 py-2">
      <summary className="cursor-pointer text-caption text-fg-2 marker:text-fg-3">
        {warnings > 0
          ? t("usageSync.accuracy.summaryWithIssues", { count: warnings })
          : t("usageSync.accuracy.summary")}
      </summary>
      <ul className="m-0 mt-1.5 flex list-disc flex-col gap-1 ps-5" role="note">
        {items.map((item, index) => (
          <li
            key={`${item.kind}-${index}`}
            className={cn("text-caption text-fg-3", toneClass[item.kind])}
          >
            {itemText(item, t)}
          </li>
        ))}
      </ul>
    </details>
  );
}

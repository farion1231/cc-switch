import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  UsageScopeAccuracy,
  buildScopeAccuracy,
  formatUtcOffset,
} from "@/components/usage/UsageScopeAccuracy";
import { deviceAccuracyLines } from "@/components/usage/UsageDevicesSheet";
import { overviewRefetchInterval } from "@/lib/query/usageSync";
import type { RemoteDeviceInfo, UsageSyncOverview } from "@/types/usageSync";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options && Object.keys(options).length
        ? `${key}:${Object.values(options).join("|")}`
        : key,
  }),
}));

const now = 1_760_000_000_000;
const secs = now / 1000;

const remote = (partial: Partial<RemoteDeviceInfo>): RemoteDeviceInfo => ({
  deviceId: "dev",
  deviceName: "Dev",
  detailLevel: "records",
  lastUploadAt: secs - 60,
  lastFetchedAt: secs - 30,
  status: "ok",
  logRows: 1,
  rollupRows: 0,
  utcOffsetMinutes: 480,
  unverified: { requests: 0 },
  ...partial,
});

const overview = (
  devices: RemoteDeviceInfo[],
  extra: Partial<UsageSyncOverview> = {},
): UsageSyncOverview => ({
  settings: {
    enabled: true,
    autoSync: true,
    backend: "webdav",
    detailLevel: "records",
    intervalMinutes: 15,
    status: { identityConflict: false },
  },
  availableBackends: { webdav: true, s3: false },
  thisDevice: {
    deviceId: "me",
    deviceName: "Mac",
    utcOffsetMinutes: 480,
    unverified: { requests: 0 },
  },
  devices,
  syncing: false,
  staleAfterSecs: 86_400,
  ...extra,
});

describe("buildScopeAccuracy", () => {
  it("says nothing for this device", () => {
    expect(
      buildScopeAccuracy(overview([remote({})]), "local", "Me", now),
    ).toEqual([]);
  });

  it("states exact, unknown overlap and estimate for a clean All scope", () => {
    const kinds = buildScopeAccuracy(
      overview([remote({})]),
      "all",
      "Me",
      now,
    ).map((i) => i.kind);
    expect(kinds).toEqual(["exact", "unknownOverlap", "estimate"]);
  });

  it("quantifies history without request ids per holder", () => {
    const items = buildScopeAccuracy(
      overview(
        [
          remote({
            deviceName: "NAS",
            unverified: {
              requests: 40,
              firstDate: "2025-01-01",
              lastDate: "2025-06-30",
            },
          }),
        ],
        {
          thisDevice: {
            deviceId: "me",
            deviceName: "Mac",
            utcOffsetMinutes: 480,
            unverified: { requests: 2 },
          },
        },
      ),
      "all",
      "This device",
      now,
    );
    const unverified = items.find((i) => i.kind === "unverified");
    expect(unverified).toEqual({
      kind: "unverified",
      requests: 42,
      devices: [
        { name: "This device", requests: 2, from: undefined, to: undefined },
        { name: "NAS", requests: 40, from: "2025-01-01", to: "2025-06-30" },
      ],
    });
  });

  it("flags daily-only, other calendars, never synced, outdated and errors", () => {
    const items = buildScopeAccuracy(
      overview(
        [
          remote({ deviceId: "a", deviceName: "Daily", detailLevel: "daily" }),
          remote({
            deviceId: "b",
            deviceName: "Berlin",
            utcOffsetMinutes: 120,
          }),
          remote({
            deviceId: "c",
            deviceName: "New",
            lastUploadAt: null,
            lastFetchedAt: null,
          }),
          remote({
            deviceId: "d",
            deviceName: "Old",
            lastUploadAt: secs - 3 * 86_400,
          }),
          remote({ deviceId: "e", deviceName: "Hidden", status: "hidden" }),
        ],
        {
          settings: {
            enabled: true,
            autoSync: true,
            detailLevel: "records",
            intervalMinutes: 15,
            status: { identityConflict: false, lastError: "401" },
          },
        },
      ),
      "all",
      "Me",
      now,
    );
    expect(items).toContainEqual({ kind: "dailyOnly", devices: ["Daily"] });
    expect(items).toContainEqual({
      kind: "calendar",
      devices: [{ name: "Berlin", offset: "UTC+02:00" }],
    });
    expect(items).toContainEqual({ kind: "neverSynced", devices: ["New"] });
    expect(items).toContainEqual({ kind: "outdated", devices: ["Old"] });
    expect(items).toContainEqual({ kind: "syncError", error: "401" });
    expect(JSON.stringify(items)).not.toContain("Hidden");
  });

  it("limits a single-device scope to that device", () => {
    const kinds = buildScopeAccuracy(
      overview([
        remote({ deviceId: "x", detailLevel: "daily" }),
        remote({ deviceId: "y" }),
      ]),
      "x",
      "Me",
      now,
    ).map((i) => i.kind);
    expect(kinds).toEqual(["dailyOnly", "estimate"]);
  });

  it("formats offsets including 45-minute and negative zones", () => {
    expect(formatUtcOffset(345)).toBe("UTC+05:45");
    expect(formatUtcOffset(-210)).toBe("UTC−03:30");
    expect(formatUtcOffset(0)).toBe("UTC+00:00");
  });
});

describe("UsageScopeAccuracy", () => {
  it("renders every statement and counts the limitations", () => {
    render(
      <UsageScopeAccuracy
        items={[
          { kind: "exact" },
          {
            kind: "unverified",
            requests: 3,
            devices: [{ name: "NAS", requests: 3 }],
          },
          { kind: "unknownOverlap" },
          { kind: "estimate" },
        ]}
      />,
    );
    expect(
      screen.getByText("usageSync.accuracy.summaryWithIssues:1"),
    ).toBeInTheDocument();
    expect(screen.getByText("usageSync.accuracy.exact")).toBeInTheDocument();
    expect(
      screen.getByText("usageSync.accuracy.unverified:3|NAS 3"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("usageSync.accuracy.unknownOverlap"),
    ).toBeInTheDocument();
    expect(screen.getByText("usageSync.accuracy.estimate")).toBeInTheDocument();
  });

  it("renders nothing without statements", () => {
    const { container } = render(<UsageScopeAccuracy items={[]} />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("deviceAccuracyLines", () => {
  const t = (key: string, options?: Record<string, unknown>) =>
    options ? `${key}:${Object.values(options).join("|")}` : key;
  it("describes id-less history, calendar and lineage", () => {
    expect(
      deviceAccuracyLines(
        {
          unverified: {
            requests: 5,
            firstDate: "2025-01-01",
            lastDate: "2025-02-01",
          },
          utcOffsetMinutes: -300,
          parentDeviceId: "parent-1234567890",
        },
        { "parent-1234567890": "Old Mac" },
        t,
      ),
    ).toEqual([
      "usageSync.devices.unverifiedRange:5|2025-01-01|2025-02-01",
      "usageSync.devices.calendar:UTC−05:00",
      "usageSync.devices.splitFrom:Old Mac",
    ]);
    expect(
      deviceAccuracyLines(
        {
          unverified: { requests: 0 },
          utcOffsetMinutes: null,
          parentDeviceId: null,
        },
        {},
        t,
      ),
    ).toEqual([]);
  });
});

describe("overviewRefetchInterval", () => {
  it("polls only while usage sync is on or remote data exists", () => {
    expect(overviewRefetchInterval(undefined)).toBe(false);
    const off = overview([], {
      settings: { ...overview([]).settings, enabled: false },
    });
    expect(overviewRefetchInterval(off)).toBe(false);
    expect(overviewRefetchInterval({ ...off, devices: [remote({})] })).toBe(
      60_000,
    );
    expect(overviewRefetchInterval(overview([]))).toBe(60_000);
  });
});

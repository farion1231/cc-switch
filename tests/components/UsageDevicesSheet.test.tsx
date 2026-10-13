import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  UsageDevicesSheet,
  deviceStatusBadge,
} from "@/components/usage/UsageDevicesSheet";
import type { RemoteDeviceInfo, UsageSyncOverview } from "@/types/usageSync";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options && Object.keys(options).length
        ? `${key}:${Object.values(options).join(",")}`
        : key,
  }),
}));

const now = Date.now();
const secs = Math.floor(now / 1000);

const device = (partial: Partial<RemoteDeviceInfo>): RemoteDeviceInfo => ({
  deviceId: "dev-1",
  deviceName: "Laptop",
  detailLevel: "records",
  lastUploadAt: secs - 60,
  status: "ok",
  logRows: 1,
  rollupRows: 0,
  ...partial,
});

describe("deviceStatusBadge", () => {
  it("prioritises hard errors over staleness", () => {
    expect(deviceStatusBadge(device({}), 86_400, now).key).toBe(
      "usageSync.status.ok",
    );
    expect(
      deviceStatusBadge(device({ lastUploadAt: secs - 90_000 }), 86_400, now)
        .key,
    ).toBe("usageSync.status.stale");
    expect(
      deviceStatusBadge(device({ lastUploadAt: null }), 86_400, now).key,
    ).toBe("usageSync.status.stale");
    expect(
      deviceStatusBadge(
        device({ status: "incompatible", lastUploadAt: secs - 90_000 }),
        86_400,
        now,
      ).key,
    ).toBe("usageSync.status.incompatible");
    expect(
      deviceStatusBadge(device({ status: "error" }), 86_400, now).tone,
    ).toBe("danger");
    expect(
      deviceStatusBadge(device({ status: "hidden" }), 86_400, now).key,
    ).toBe("usageSync.status.hidden");
  });
});

describe("UsageDevicesSheet", () => {
  const overview: UsageSyncOverview = {
    settings: {
      enabled: true,
      autoSync: true,
      backend: "s3",
      detailLevel: "records",
      intervalMinutes: 15,
      status: { identityConflict: true, lastError: "boom" },
    },
    availableBackends: { webdav: false, s3: true },
    remoteLocation: "S3: bucket/cc-switch-sync/usage-v1/default",
    transportSecure: false,
    thisDevice: { deviceId: "me", deviceName: "My Mac" },
    devices: [
      device({
        deviceId: "dev-err",
        deviceName: "Broken PC",
        status: "error",
        lastError: "checksum mismatch",
        firstDate: "2026-01-01",
        lastDate: "2026-10-13",
        detailFrom: "2026-09-13",
      }),
      device({
        deviceId: "dev-daily",
        deviceName: "Private NAS",
        detailLevel: "daily",
        firstDate: "2026-10-01",
        lastDate: "2026-10-12",
      }),
      device({
        deviceId: "dev-hidden",
        deviceName: "Old PC",
        status: "hidden",
      }),
    ],
    syncing: false,
    staleAfterSecs: 86_400,
  };

  it("lists devices with status, coverage, errors and warnings", async () => {
    const onToggleHidden = vi.fn();
    const onSyncNow = vi.fn();
    const user = userEvent.setup();
    render(
      <UsageDevicesSheet
        open
        onOpenChange={() => {}}
        overview={overview}
        syncing={false}
        onSyncNow={onSyncNow}
        onToggleHidden={onToggleHidden}
      />,
    );

    expect(screen.getByText("My Mac")).toBeInTheDocument();
    expect(screen.getByText("usageSync.conflict.title")).toBeInTheDocument();
    expect(screen.getByText("usageSync.insecure.title")).toBeInTheDocument();
    expect(screen.getByText("checksum mismatch")).toBeInTheDocument();
    expect(
      screen.getByText(
        "usageSync.devices.coverage:2026-01-01,2026-10-13 · usageSync.devices.detailFrom:2026-09-13",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "usageSync.devices.coverage:2026-10-01,2026-10-12 · usageSync.devices.dailyOnly",
      ),
    ).toBeInTheDocument();

    const hideButtons = screen.getAllByRole("button", {
      name: "usageSync.devices.hide",
    });
    await user.click(hideButtons[0]);
    expect(onToggleHidden).toHaveBeenCalledWith("dev-err", true);
    await user.click(
      screen.getByRole("button", { name: "usageSync.devices.show" }),
    );
    expect(onToggleHidden).toHaveBeenCalledWith("dev-hidden", false);

    await user.click(screen.getByRole("button", { name: "usageSync.syncNow" }));
    expect(onSyncNow).toHaveBeenCalled();
  });

  it("explains how to enable sync when it is off", () => {
    render(
      <UsageDevicesSheet
        open
        onOpenChange={() => {}}
        overview={{
          ...overview,
          settings: { ...overview.settings, enabled: false },
          devices: [],
        }}
        syncing={false}
        onSyncNow={() => {}}
        onToggleHidden={() => {}}
        onOpenSyncSettings={() => {}}
      />,
    );
    expect(
      screen.getByText("usageSync.devices.disabledTitle"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "usageSync.syncNow" }),
    ).not.toBeInTheDocument();
  });
});

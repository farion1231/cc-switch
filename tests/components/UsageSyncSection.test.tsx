import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageSyncSection } from "@/components/settings/UsageSyncSection";
import type { UsageSyncOverview } from "@/types/usageSync";

const apiMock = vi.hoisted(() => ({
  getOverview: vi.fn(),
  saveSettings: vi.fn(),
  runNow: vi.fn(),
  setDeviceName: vi.fn(),
  setDeviceHidden: vi.fn(),
  resolveConflict: vi.fn(),
}));
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@/lib/api/usageSync", () => ({ usageSyncApi: apiMock }));
vi.mock("@/lib/toast", () => ({ toast: toastMock }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const base: UsageSyncOverview = {
  settings: {
    enabled: false,
    autoSync: true,
    detailLevel: "records",
    intervalMinutes: 15,
    status: { identityConflict: false },
  },
  availableBackends: { webdav: true, s3: false },
  remoteLocation: null,
  transportSecure: null,
  thisDevice: { deviceId: "id", deviceName: "My Mac", customName: null },
  devices: [],
  syncing: false,
  staleAfterSecs: 86400,
};

const renderSection = () =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <UsageSyncSection />
    </QueryClientProvider>,
  );

describe("UsageSyncSection", () => {
  beforeEach(() => {
    Object.values(apiMock).forEach((fn) => fn.mockReset());
    Object.values(toastMock).forEach((fn) => fn.mockReset());
    apiMock.saveSettings.mockResolvedValue(base.settings);
  });

  it("is off by default and enables with the only configured backend", async () => {
    apiMock.getOverview.mockResolvedValue(base);
    const user = userEvent.setup();
    renderSection();
    const toggle = await screen.findByRole("switch", {
      name: "usageSync.settings.enable",
    });
    expect(toggle).not.toBeChecked();
    expect(
      screen.getByText("usageSync.settings.privacyNotShared"),
    ).toBeInTheDocument();

    await user.click(toggle);
    await waitFor(() =>
      expect(apiMock.saveSettings).toHaveBeenCalledWith({
        enabled: true,
        autoSync: true,
        backend: "webdav",
        detailLevel: "records",
        intervalMinutes: 15,
      }),
    );
  });

  it("requires an explicit backend when both are configured", async () => {
    apiMock.getOverview.mockResolvedValue({
      ...base,
      availableBackends: { webdav: true, s3: true },
    });
    const user = userEvent.setup();
    renderSection();
    await user.click(
      await screen.findByRole("switch", { name: "usageSync.settings.enable" }),
    );
    expect(toastMock.warning).toHaveBeenCalledWith(
      "usageSync.settings.chooseBackendFirst",
    );
    expect(apiMock.saveSettings).not.toHaveBeenCalled();
  });

  it("cannot be enabled without cloud storage", async () => {
    apiMock.getOverview.mockResolvedValue({
      ...base,
      availableBackends: { webdav: false, s3: false },
    });
    renderSection();
    expect(
      await screen.findByText("usageSync.settings.noBackendTitle"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "usageSync.settings.enable" }),
    ).toBeDisabled();
  });

  it("offers conflict resolution and sync now when enabled", async () => {
    apiMock.getOverview.mockResolvedValue({
      ...base,
      settings: {
        ...base.settings,
        enabled: true,
        backend: "webdav",
        status: { identityConflict: true },
      },
      remoteLocation: "WebDAV: /cc-switch-sync/usage-v1/default",
      transportSecure: false,
    });
    apiMock.resolveConflict.mockResolvedValue(undefined);
    apiMock.runNow.mockResolvedValue({
      uploadedPartitions: 1,
      manifestWritten: true,
      identityConflict: false,
      devicesSeen: 2,
      devicesUpdated: 1,
      deviceErrors: [],
    });
    const user = userEvent.setup();
    renderSection();
    expect(
      await screen.findByText("usageSync.insecure.title"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "usageSync.conflict.newIdentity" }),
    );
    await waitFor(() =>
      expect(apiMock.resolveConflict).toHaveBeenCalledWith("newIdentity"),
    );
    await user.click(screen.getByRole("button", { name: "usageSync.syncNow" }));
    await waitFor(() => expect(apiMock.runNow).toHaveBeenCalled());
    await waitFor(() =>
      expect(toastMock.success).toHaveBeenCalledWith("usageSync.syncCompleted"),
    );
  });
});

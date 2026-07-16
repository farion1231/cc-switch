import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { WebdavSyncSection } from "@/components/settings/WebdavSyncSection";
import type { S3SyncSettings, WebDavSyncSettings } from "@/types";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastWarningMock = vi.fn();
const toastInfoMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, ...props }: any) => <button {...props}>{children}</button>,
}));

vi.mock("@/components/ui/input", () => ({
  Input: (props: any) => <input {...props} />,
}));

vi.mock("@/components/ui/switch", () => ({
  Switch: ({ checked, onCheckedChange, ...props }: any) => (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onCheckedChange?.(!checked)}
      {...props}
    />
  ),
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ value, onValueChange, children }: any) => (
    <select
      data-testid="webdav-preset-select"
      value={value}
      onChange={(event) => onValueChange?.(event.target.value)}
    >
      {children}
    </select>
  ),
  SelectTrigger: ({ children }: any) => <>{children}</>,
  SelectValue: () => null,
  SelectContent: ({ children }: any) => <>{children}</>,
  SelectItem: ({ value, children }: any) => (
    <option value={value}>{children}</option>
  ),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) => (open ? <div>{children}</div> : null),
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogDescription: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
}));

const { settingsApiMock } = vi.hoisted(() => ({
  settingsApiMock: {
    webdavTestConnection: vi.fn(),
    webdavSyncSaveSettings: vi.fn(),
    webdavSyncFetchRemoteInfo: vi.fn(),
    webdavSyncUpload: vi.fn(),
    webdavSyncPrepareDownload: vi.fn(),
    webdavSyncApplyDownload: vi.fn(),
    s3SyncFetchRemoteInfo: vi.fn(),
    s3SyncPrepareDownload: vi.fn(),
    s3SyncApplyDownload: vi.fn(),
  },
}));

vi.mock("@/lib/api", () => ({
  settingsApi: settingsApiMock,
}));

const baseConfig: WebDavSyncSettings = {
  enabled: true,
  baseUrl: "https://dav.example.com/dav/",
  username: "alice",
  password: "secret",
  remoteRoot: "cc-switch-sync",
  profile: "default",
  autoSync: false,
  status: {},
};

const baseS3Config: S3SyncSettings = {
  enabled: true,
  region: "us-east-1",
  bucket: "cc-switch-sync",
  accessKeyId: "AKIAEXAMPLE",
  secretAccessKey: "secret",
  endpoint: "https://s3.example.com",
  remoteRoot: "cc-switch-sync",
  profile: "default",
  autoSync: false,
  status: {},
};

function renderSection(
  config?: WebDavSyncSettings,
  s3Config?: S3SyncSettings,
) {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <WebdavSyncSection config={config} s3Config={s3Config} />
    </QueryClientProvider>,
  );
  return { ...view, client };
}

describe("WebdavSyncSection", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastWarningMock.mockReset();
    toastInfoMock.mockReset();
    settingsApiMock.webdavTestConnection.mockReset();
    settingsApiMock.webdavSyncSaveSettings.mockReset();
    settingsApiMock.webdavSyncFetchRemoteInfo.mockReset();
    settingsApiMock.webdavSyncUpload.mockReset();
    settingsApiMock.webdavSyncPrepareDownload.mockReset();
    settingsApiMock.webdavSyncApplyDownload.mockReset();
    settingsApiMock.s3SyncFetchRemoteInfo.mockReset();
    settingsApiMock.s3SyncPrepareDownload.mockReset();
    settingsApiMock.s3SyncApplyDownload.mockReset();

    settingsApiMock.webdavSyncSaveSettings.mockResolvedValue({ success: true });
    settingsApiMock.webdavTestConnection.mockResolvedValue({
      success: true,
      message: "ok",
    });
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValue({
      deviceName: "My MacBook",
      createdAt: "2026-02-01T10:00:00Z",
      snapshotId: "snapshot-1",
      version: 2,
      compatible: true,
      artifacts: ["db.sql", "skills.zip"],
    });
    settingsApiMock.webdavSyncUpload.mockResolvedValue({ status: "uploaded" });
    settingsApiMock.webdavSyncPrepareDownload.mockResolvedValue({
      previewId: "preview-1",
      newProviderCount: 0,
      existingProviderCount: 1,
      credentialConflicts: [],
      exactRestoreCredentialFieldCount: 0,
    });
    settingsApiMock.webdavSyncApplyDownload.mockResolvedValue({ status: "downloaded" });
    settingsApiMock.s3SyncFetchRemoteInfo.mockResolvedValue({
      deviceName: "S3 Device",
      createdAt: "2026-02-01T10:00:00Z",
      snapshotId: "s3-snapshot-1",
      version: 2,
      compatible: true,
      artifacts: ["db.sql", "skills.zip"],
    });
    settingsApiMock.s3SyncPrepareDownload.mockResolvedValue({
      previewId: "s3-preview-1",
      newProviderCount: 0,
      existingProviderCount: 1,
      credentialConflicts: [],
      exactRestoreCredentialFieldCount: 0,
    });
    settingsApiMock.s3SyncApplyDownload.mockResolvedValue({ status: "downloaded" });
  });

  it("shows auto sync error callout when last auto sync failed", () => {
    renderSection({
      ...baseConfig,
      status: {
        lastError: "network timeout",
        lastErrorSource: "auto",
      },
    });

    expect(
      screen.getByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).toBeInTheDocument();
    expect(screen.getByText("network timeout")).toBeInTheDocument();
  });

  it("does not show auto sync error callout for manual sync errors", () => {
    renderSection({
      ...baseConfig,
      status: {
        lastError: "manual upload failed",
        lastErrorSource: "manual",
      },
    });

    expect(
      screen.queryByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).not.toBeInTheDocument();
  });

  it("does not show auto sync error callout when source is missing", () => {
    renderSection({
      ...baseConfig,
      autoSync: true,
      status: {
        lastError: "legacy error without source",
      },
    });

    expect(
      screen.queryByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).not.toBeInTheDocument();
  });

  it("shows validation error when saving without base url", async () => {
    renderSection({ ...baseConfig, baseUrl: "" });

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    expect(toastErrorMock).toHaveBeenCalledWith("settings.webdavSync.missingUrl");
    expect(settingsApiMock.webdavSyncSaveSettings).not.toHaveBeenCalled();
  });

  it("saves settings and auto tests connection", async () => {
    renderSection(baseConfig);

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });
    expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        baseUrl: "https://dav.example.com/dav/",
        username: "alice",
        password: "",
        autoSync: false,
      }),
      false,
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavTestConnection).toHaveBeenCalledWith(
        expect.objectContaining({
          baseUrl: "https://dav.example.com/dav/",
        }),
        true,
      );
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.saveAndTestSuccess",
    );
  });

  it("preserves password only for the single post-save refresh", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("secret");

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("");
  });

  it("does not preserve password after a later external config refresh", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("secret");

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection
          config={{ ...baseConfig, username: "bob", password: "" }}
        />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("");
  });

  it("does not submit a preserved password again when testing without touching it", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.test" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavTestConnection).toHaveBeenLastCalledWith(
        expect.objectContaining({
          password: "",
        }),
        true,
      );
    });
  });

  it("saves auto sync as true after toggle", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("switch", { name: "settings.webdavSync.autoSync" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "confirm.autoSync.confirm" }),
    );
    await waitFor(() => {
      expect(
        screen.getByRole("switch", { name: "settings.webdavSync.autoSync" }),
      ).toHaveAttribute("aria-checked", "true");
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          autoSync: true,
        }),
        false,
      );
    });
  });

  it("blocks upload when there are unsaved changes", async () => {
    renderSection(baseConfig);

    fireEvent.change(
      screen.getByPlaceholderText("settings.webdavSync.usernamePlaceholder"),
      { target: { value: "bob" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );

    expect(toastErrorMock).toHaveBeenCalledWith(
      "settings.webdavSync.unsavedChanges",
    );
    expect(settingsApiMock.webdavSyncFetchRemoteInfo).not.toHaveBeenCalled();
  });

  it("disables sync buttons until config is saved", () => {
    renderSection(undefined);

    const uploadButton = screen.getByRole("button", {
      name: "settings.webdavSync.upload",
    });
    const downloadButton = screen.getByRole("button", {
      name: "settings.webdavSync.download",
    });

    expect(uploadButton).toBeDisabled();
    expect(downloadButton).toBeDisabled();
    expect(settingsApiMock.webdavSyncFetchRemoteInfo).not.toHaveBeenCalled();
  });

  it("fetches remote info and uploads after confirmation", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmUpload.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncUpload).toHaveBeenCalledTimes(1);
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.uploadSuccess",
    );
  });

  it("blocks upload confirmation if form changes after dialog opens", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.change(screen.getByPlaceholderText("cc-switch-sync"), {
      target: { value: "new-root" },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmUpload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.unsavedChanges",
      );
    });
    expect(settingsApiMock.webdavSyncUpload).not.toHaveBeenCalled();
  });

  it("fetches remote info and downloads after confirmation", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncPrepareDownload).toHaveBeenCalledTimes(1);
      expect(settingsApiMock.webdavSyncApplyDownload).toHaveBeenCalledWith(
        "preview-1",
        [],
      );
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.downloadSuccess",
    );
  });

  it("blocks download confirmation if form changes after dialog opens", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.change(screen.getByPlaceholderText("default"), {
      target: { value: "new-profile" },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.unsavedChanges",
      );
    });
    expect(settingsApiMock.webdavSyncPrepareDownload).not.toHaveBeenCalled();
    expect(settingsApiMock.webdavSyncApplyDownload).not.toHaveBeenCalled();
  });

  it("shows info when no remote snapshot is found for download", async () => {
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValueOnce({ empty: true });
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(toastInfoMock).toHaveBeenCalledWith("settings.webdavSync.noRemoteData");
    });
    expect(settingsApiMock.webdavSyncPrepareDownload).not.toHaveBeenCalled();
    expect(settingsApiMock.webdavSyncApplyDownload).not.toHaveBeenCalled();
  });

  it("blocks download when remote snapshot is incompatible", async () => {
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValueOnce({
      deviceName: "Legacy Machine",
      createdAt: "2025-01-01T00:00:00Z",
      snapshotId: "legacy-snapshot",
      version: 1,
      compatible: false,
      artifacts: ["db.sql"],
    });
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.incompatibleVersion",
      );
    });
    expect(settingsApiMock.webdavSyncPrepareDownload).not.toHaveBeenCalled();
    expect(settingsApiMock.webdavSyncApplyDownload).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    ).not.toBeInTheDocument();
  });

  it("shows error when download fails after confirmation", async () => {
    settingsApiMock.webdavSyncPrepareDownload.mockRejectedValueOnce(new Error("boom"));
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.downloadFailed",
      );
    });
  });

  it("shows credential confirm when restore preview has credential impact", async () => {
    settingsApiMock.webdavSyncPrepareDownload.mockResolvedValueOnce({
      previewId: "preview-impact",
      newProviderCount: 0,
      existingProviderCount: 2,
      credentialConflicts: [
        {
          field: "apiKey",
          storedMasked: "sk-local***",
          liveMasked: "sk-remote***",
        },
      ],
      exactRestoreCredentialFieldCount: 1,
    });
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncPrepareDownload).toHaveBeenCalledTimes(1);
      expect(
        screen.getByRole("button", {
          name: "settings.webdavSync.confirmCredentialImpact.confirm",
        }),
      ).toBeInTheDocument();
    });
    expect(settingsApiMock.webdavSyncApplyDownload).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmCredentialImpact.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncApplyDownload).toHaveBeenCalledWith(
        "preview-impact",
        [],
      );
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.downloadSuccess",
    );
  });


  it("S3 prepare/apply download path keeps local credentials by default", async () => {
    renderSection(baseConfig, baseS3Config);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.s3Sync.download" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.s3SyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.s3Sync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.s3SyncPrepareDownload).toHaveBeenCalledTimes(1);
      expect(settingsApiMock.s3SyncApplyDownload).toHaveBeenCalledWith(
        "s3-preview-1",
        [],
      );
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.s3Sync.downloadSuccess",
    );
  });

  it("S3 credential impact requires explicit confirm before apply", async () => {
    settingsApiMock.s3SyncPrepareDownload.mockResolvedValueOnce({
      previewId: "s3-preview-impact",
      newProviderCount: 1,
      existingProviderCount: 2,
      credentialConflicts: [
        {
          appType: "claude",
          providerId: "p1",
          providerName: "Claude",
          apiKeyDiffers: true,
          baseUrlDiffers: false,
        },
      ],
      exactRestoreCredentialFieldCount: 1,
    });

    renderSection(baseConfig, baseS3Config);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.s3Sync.download" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.s3SyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.s3Sync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(
        screen.getByRole("button", {
          name: "settings.s3Sync.confirmCredentialImpact.confirm",
        }),
      ).toBeInTheDocument();
    });
    expect(settingsApiMock.s3SyncApplyDownload).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.s3Sync.confirmCredentialImpact.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.s3SyncApplyDownload).toHaveBeenCalledWith(
        "s3-preview-impact",
        [],
      );
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.s3Sync.downloadSuccess",
    );
  });

});

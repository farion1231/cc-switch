import React, { Suspense } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { http, HttpResponse } from "msw";
import { SettingsPage } from "@/components/settings/SettingsPage";
import {
  resetProviderState,
  getSettings,
  getAppConfigDirOverride,
} from "../msw/state";
import { server } from "../msw/server";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) =>
    open ? <div data-testid="dialog-root">{children}</div> : null,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
  DialogDescription: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "system", setTheme: vi.fn() }),
}));

// 「数据」一节的其他卡片有自己的接口，这里只测目录和导入导出
vi.mock("@/components/settings/BackupListSection", () => ({
  BackupListSection: () => <div>backup-list-section</div>,
}));
vi.mock("@/components/settings/WebdavSyncSection", () => ({
  WebdavSyncSection: () => <div>webdav-sync-section</div>,
}));
vi.mock("@/components/settings/LogConfigPanel", () => ({
  LogConfigPanel: () => <div>log-config-panel</div>,
}));

vi.mock("@/components/settings/DirectorySettings", async () => {
  const actual = await vi.importActual<
    typeof import("@/components/settings/DirectorySettings")
  >("@/components/settings/DirectorySettings");
  return actual;
});

vi.mock("@/components/settings/ImportExportSection", () => ({
  ImportExportSection: ({
    status,
    selectedFile,
    errorMessage,
    isImporting,
    onSelectFile,
    onImport,
    onExport,
    onClear,
  }: any) => (
    <div>
      <div data-testid="import-status">{status}</div>
      <div data-testid="selected-file">{selectedFile || "none"}</div>
      <button onClick={onSelectFile}>settings.selectConfigFile</button>
      <button onClick={onImport} disabled={!selectedFile || isImporting}>
        {isImporting ? "settings.importing" : "settings.import"}
      </button>
      <button onClick={onExport}>settings.exportConfig</button>
      <button onClick={onClear}>common.clear</button>
      {errorMessage ? <span>{errorMessage}</span> : null}
    </div>
  ),
}));

vi.mock("@/components/settings/AboutSection", () => ({
  AboutSection: ({ isPortable }: any) => <div>about:{String(isPortable)}</div>,
}));

const renderDialog = (
  props?: Partial<React.ComponentProps<typeof SettingsPage>>,
) => {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <SettingsPage
          section="data"
          onOpenApps={() => {}}
          onOpenApp={() => {}}
          {...props}
        />
      </Suspense>
    </QueryClientProvider>,
  );
};

beforeEach(() => {
  resetProviderState();
  toastSuccessMock.mockReset();
  toastErrorMock.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("SettingsPage integration", () => {
  it("loads default settings from MSW", async () => {
    renderDialog();

    const appInput = await screen.findByPlaceholderText(
      "settings.browsePlaceholderApp",
    );
    expect((appInput as HTMLInputElement).value).toBe("/home/mock/.cc-switch");
    expect(
      screen.queryByText(/chatLanguageModels\.json/),
    ).not.toBeInTheDocument();
  });

  it("imports configuration and triggers success callback", async () => {
    const onImportSuccess = vi.fn();
    renderDialog({ onImportSuccess });

    fireEvent.click(await screen.findByText("settings.selectConfigFile"));
    await waitFor(() =>
      expect(screen.getByTestId("selected-file").textContent).toContain(
        "/mock/import-settings.json",
      ),
    );

    fireEvent.click(screen.getByText("settings.import"));
    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalled());
    await waitFor(() => expect(onImportSuccess).toHaveBeenCalled(), {
      timeout: 4000,
    });
    expect(getSettings().language).toBe("en");
  });

  it("saves settings and handles restart prompt", async () => {
    renderDialog();

    const appInput = await screen.findByPlaceholderText(
      "settings.browsePlaceholderApp",
    );
    fireEvent.change(appInput, { target: { value: "/custom/app" } });
    fireEvent.click(screen.getByText("common.save"));

    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalled());
    await screen.findByText("settings.restartRequired");
    fireEvent.click(screen.getByText("settings.restartLater"));
    await waitFor(() =>
      expect(
        screen.queryByText("settings.restartRequired"),
      ).not.toBeInTheDocument(),
    );

    expect(getAppConfigDirOverride()).toBe("/custom/app");
  });

  it("allows browsing and resetting the data directory", async () => {
    renderDialog();

    const appInput = (await screen.findByPlaceholderText(
      "settings.browsePlaceholderApp",
    )) as HTMLInputElement;
    expect(appInput.value).toBe("/home/mock/.cc-switch");

    fireEvent.click(
      screen.getByRole("button", { name: "settings.browseDirectory" }),
    );
    await waitFor(() =>
      expect(appInput.value).toBe("/home/mock/.cc-switch/picked"),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "settings.resetDefault" }),
    );
    await waitFor(() => expect(appInput.value).toBe("/home/mock/.cc-switch"));
  });

  it("allows browsing and resetting an app's config directory", async () => {
    renderDialog({ section: "appConfig" });

    const claudeInput = (await screen.findByPlaceholderText(
      "settings.browsePlaceholderClaude",
    )) as HTMLInputElement;
    fireEvent.change(claudeInput, { target: { value: "/custom/claude" } });
    await waitFor(() => expect(claudeInput.value).toBe("/custom/claude"));

    const browseButtons = screen.getAllByRole("button", {
      name: "settings.browseDirectory",
    });
    const resetButtons = screen.getAllByRole("button", {
      name: "settings.resetDefault",
    });
    fireEvent.click(browseButtons[0]);
    await waitFor(() =>
      expect(claudeInput.value).toBe("/custom/claude/picked"),
    );

    fireEvent.click(resetButtons[0]);
    await waitFor(() => expect(claudeInput.value).toBe("/home/mock/.claude"));
  });

  it("notifies when export fails", async () => {
    renderDialog();

    await screen.findByText("settings.exportConfig");

    server.use(
      http.post("http://tauri.local/save_file_dialog", () =>
        HttpResponse.json(null),
      ),
    );
    fireEvent.click(screen.getByText("settings.exportConfig"));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    const cancelMessage = toastErrorMock.mock.calls.at(-1)?.[0] as string;
    expect(cancelMessage).toMatch(
      /settings\.selectFileFailed|请选择.*保存路径/,
    );

    toastErrorMock.mockClear();

    server.use(
      http.post("http://tauri.local/save_file_dialog", () =>
        HttpResponse.json("/mock/export-settings.json"),
      ),
      http.post("http://tauri.local/export_config_to_file", () =>
        HttpResponse.json({ success: false, message: "disk-full" }),
      ),
    );

    fireEvent.click(screen.getByText("settings.exportConfig"));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    const exportMessage = toastErrorMock.mock.calls.at(-1)?.[0] as string;
    expect(exportMessage).toContain("disk-full");
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });
});

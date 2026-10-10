import React, { Suspense } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { http, HttpResponse } from "msw";
import type { Settings } from "@/types";
import { SettingsPage } from "@/components/settings/SettingsPage";
import {
  resetProviderState,
  getSettings,
  setSettings,
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

  it.each([undefined, "/saved/claude"])(
    "discards directory drafts after unrelated autosave with saved directory %s",
    async (savedDirectory) => {
      setSettings({ claudeConfigDir: savedDirectory });
      const resolvedDirectory = savedDirectory ?? "/home/mock/.claude";
      const saves: Settings[] = [];
      const syncLive = vi.fn();
      server.use(
        http.post("http://tauri.local/get_config_dir", async ({ request }) => {
          const { app } = (await request.json()) as { app: string };
          return HttpResponse.json(
            app === "claude" ? resolvedDirectory : `/default/${app}`,
          );
        }),
        http.post("http://tauri.local/save_settings", async ({ request }) => {
          const { settings } = (await request.json()) as { settings: Settings };
          saves.push(settings);
          setSettings(settings);
          return HttpResponse.json(true);
        }),
        http.post("http://tauri.local/sync_current_providers_live", () => {
          syncLive();
          return HttpResponse.json(true);
        }),
      );
      renderDialog({ section: "appConfig" });

      const directory = await screen.findByPlaceholderText(
        "settings.browsePlaceholderClaude",
      );
      expect(directory).toHaveValue(resolvedDirectory);
      fireEvent.change(directory, { target: { value: "/unsaved/claude" } });
      expect(directory).toHaveValue("/unsaved/claude");
      expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
      expect(saves).toHaveLength(0);

      const toggle = screen.getByRole("switch", {
        name: "settings.skipClaudeOnboarding",
      });
      fireEvent.click(toggle);
      await waitFor(() =>
        expect(getSettings().skipClaudeOnboarding).toBe(true),
      );
      expect(saves[0].claudeConfigDir).toBe(savedDirectory);
      await waitFor(() =>
        expect(
          screen.queryByRole("button", { name: "common.save" }),
        ).toBeNull(),
      );
      await waitFor(() => expect(directory).toHaveValue(resolvedDirectory));

      fireEvent.click(toggle);
      await waitFor(() =>
        expect(getSettings().skipClaudeOnboarding).toBe(false),
      );
      expect(saves).toHaveLength(2);
      expect(saves[1].claudeConfigDir).toBe(savedDirectory);
      expect(directory).toHaveValue(resolvedDirectory);
      expect(syncLive).not.toHaveBeenCalled();
    },
  );

  it.each([undefined, "/saved/claude"])(
    "saves directory edits and resets after picker cancellation from %s",
    async (savedDirectory) => {
      setSettings({ claudeConfigDir: savedDirectory });
      const selectDirectory = vi.fn();
      server.use(
        http.post("http://tauri.local/get_config_dir", async ({ request }) => {
          const { app } = (await request.json()) as { app: string };
          return HttpResponse.json(
            app === "claude"
              ? (getSettings().claudeConfigDir ?? "/home/mock/.claude")
              : `/default/${app}`,
          );
        }),
        http.post("http://tauri.local/save_settings", async ({ request }) => {
          const { settings } = (await request.json()) as { settings: Settings };
          // The backend replaces settings, so an omitted override is cleared.
          setSettings({
            ...settings,
            claudeConfigDir: settings.claudeConfigDir,
          });
          return HttpResponse.json(true);
        }),
        http.post("http://tauri.local/pick_directory", () => {
          selectDirectory();
          return HttpResponse.json(null);
        }),
      );
      renderDialog({ section: "appConfig" });
      const directory = await screen.findByPlaceholderText(
        "settings.browsePlaceholderClaude",
      );
      fireEvent.change(directory, { target: { value: "/edited/claude" } });
      fireEvent.click(
        screen.getAllByRole("button", { name: "settings.browseDirectory" })[0],
      );
      await waitFor(() => expect(selectDirectory).toHaveBeenCalledOnce());
      expect(directory).toHaveValue("/edited/claude");
      expect(getSettings().claudeConfigDir).toBe(savedDirectory);

      fireEvent.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() =>
        expect(getSettings().claudeConfigDir).toBe("/edited/claude"),
      );
      await waitFor(() => expect(toastSuccessMock).toHaveBeenCalledOnce());
      expect(directory).toHaveValue("/edited/claude");
      expect(screen.queryByRole("button", { name: "common.save" })).toBeNull();

      fireEvent.click(
        screen.getAllByRole("button", { name: "settings.resetDefault" })[0],
      );
      expect(directory).toHaveValue("/home/mock/.claude");
      expect(getSettings().claudeConfigDir).toBe("/edited/claude");
      fireEvent.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() =>
        expect(getSettings().claudeConfigDir).toBeUndefined(),
      );
      await waitFor(() => expect(toastSuccessMock).toHaveBeenCalledTimes(2));
      expect(directory).toHaveValue("/home/mock/.claude");
      expect(screen.queryByRole("button", { name: "common.save" })).toBeNull();
    },
  );

  it.each([
    [
      "hermes",
      "hermesConfigDir",
      "settings.browsePlaceholderHermes",
      "/env/hermes-home",
    ],
    ["pi", "piConfigDir", "settings.browsePlaceholderPi", "/env/pi-agent"],
  ] as const)(
    "restores backend-resolved %s default after draft discard",
    async (targetApp, field, placeholder, backendDefault) => {
      setSettings({ [field]: undefined });
      const saves: Settings[] = [];
      server.use(
        http.post("http://tauri.local/get_config_dir", async ({ request }) => {
          const { app } = (await request.json()) as { app: string };
          return HttpResponse.json(
            app === targetApp ? backendDefault : `/default/${app}`,
          );
        }),
        http.post("http://tauri.local/save_settings", async ({ request }) => {
          const { settings } = (await request.json()) as { settings: Settings };
          saves.push(settings);
          setSettings({ ...settings, [field]: settings[field] });
          return HttpResponse.json(true);
        }),
      );
      renderDialog({ section: "appConfig" });
      const directory = await screen.findByPlaceholderText(placeholder);
      await waitFor(() => expect(directory).toHaveValue(backendDefault));
      fireEvent.change(directory, { target: { value: `/draft/${targetApp}` } });
      expect(directory).toHaveValue(`/draft/${targetApp}`);
      fireEvent.click(
        screen.getByRole("switch", { name: "settings.skipClaudeOnboarding" }),
      );
      await waitFor(() =>
        expect(getSettings().skipClaudeOnboarding).toBe(true),
      );
      await waitFor(() =>
        expect(
          screen.queryByRole("button", { name: "common.save" }),
        ).toBeNull(),
      );
      expect(saves[0][field]).toBeUndefined();
      await waitFor(() => expect(directory).toHaveValue(backendDefault));
    },
  );

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

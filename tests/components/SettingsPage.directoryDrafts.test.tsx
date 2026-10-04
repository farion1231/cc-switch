import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsPage } from "@/components/settings/SettingsPage";
import type { Settings } from "@/types";
import type { SettingsSection } from "@/lib/navigation";

let persisted: Settings;
const saveMock = vi.fn();
const getMock = vi.fn();
const syncLiveMock = vi.fn();
const selectDirectoryMock = vi.fn();

// Exercise the real settings hooks, query/refetch cycle, and rendered controls.
// Only the native boundary and unrelated settings panels are replaced.
vi.mock("@/lib/api", () => ({
  settingsApi: {
    get: () => getMock(),
    save: (settings: Settings) => saveMock(settings),
    getAppConfigDirOverride: async () => null,
    getConfigDir: async (app: string) => `/fixture/default/${app}`,
    isPortable: async () => false,
    setAppConfigDirOverride: async () => true,
    syncCurrentProvidersLive: () => syncLiveMock(),
    selectConfigDirectory: () => selectDirectoryMock(),
    applyClaudeOnboardingSkip: async () => true,
    clearClaudeOnboardingSkip: async () => true,
  },
  providersApi: { updateTrayMenu: async () => true },
}));
vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "system", setTheme: vi.fn() }),
}));
vi.mock("@/components/settings/TerminalSettings", () => ({
  TerminalSelect: () => null,
}));
vi.mock("@/components/settings/AboutSection", () => ({
  AboutSection: () => null,
}));
vi.mock("@/components/settings/WebdavSyncSection", () => ({
  WebdavSyncSection: () => null,
}));
vi.mock("@/components/settings/BackupListSection", () => ({
  BackupListSection: () => null,
}));
vi.mock("@/components/settings/LogConfigPanel", () => ({
  LogConfigPanel: () => null,
}));
vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => null,
}));
vi.mock("@/components/settings/sections/RoutingSection", () => ({
  RoutingSection: () => null,
}));
vi.mock("@/components/settings/CodexAuthSettings", () => ({
  CodexAuthSettings: () => null,
}));

const directories = [
  ["claudeConfigDir", "settings.browsePlaceholderClaude"],
  ["codexConfigDir", "settings.browsePlaceholderCodex"],
  ["geminiConfigDir", "settings.browsePlaceholderGemini"],
  ["grokConfigDir", "settings.browsePlaceholderGrok"],
  ["opencodeConfigDir", "settings.browsePlaceholderOpencode"],
  ["openclawConfigDir", "settings.browsePlaceholderOpenclaw"],
  ["hermesConfigDir", "settings.browsePlaceholderHermes"],
  ["piConfigDir", "settings.browsePlaceholderPi"],
] as const;

async function renderSettings() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const page = (section: SettingsSection) => (
    <QueryClientProvider client={client}>
      <SettingsPage
        section={section}
        onOpenApps={vi.fn()}
        onOpenApp={vi.fn()}
      />
    </QueryClientProvider>
  );
  const view = render(page("appConfig"));
  await screen.findByPlaceholderText("settings.browsePlaceholderOpencode");
  return {
    ...view,
    client,
    navigate: (section: SettingsSection) => view.rerender(page(section)),
  };
}

beforeEach(() => {
  persisted = {
    showInTray: true,
    minimizeToTrayOnClose: true,
    showProfileSwitcher: true,
    language: "zh",
    claudeConfigDir: "/fixture/saved/claude",
    codexConfigDir: "/fixture/saved/codex",
    geminiConfigDir: "/fixture/saved/gemini",
    grokConfigDir: "/fixture/saved/grok",
    opencodeConfigDir: "/fixture/saved/opencode",
    openclawConfigDir: "/fixture/saved/openclaw",
    hermesConfigDir: "/fixture/saved/hermes",
    piConfigDir: "/fixture/saved/pi",
  };
  saveMock.mockReset();
  getMock.mockReset();
  getMock.mockImplementation(async () => ({ ...persisted }));
  saveMock.mockImplementation(async (settings: Settings) => {
    persisted = { ...settings };
    return true;
  });
  syncLiveMock.mockReset();
  syncLiveMock.mockResolvedValue(true);
  selectDirectoryMock.mockReset();
  selectDirectoryMock.mockResolvedValue(null);
});

describe("SettingsPage directory drafts", () => {
  it.each(directories)(
    "keeps %s pending through unrelated autosaves until Save is clicked",
    async (field, placeholder) => {
      const savedPath = persisted[field];
      const draftPath = "/fixture/pending/directory";
      const view = await renderSettings();
      fireEvent.change(screen.getByPlaceholderText(placeholder), {
        target: { value: draftPath },
      });
      expect(screen.getByRole("button", { name: "common.save" })).toBeVisible();
      expect(saveMock).not.toHaveBeenCalled();

      view.navigate("general");
      fireEvent.click(
        screen.getByRole("switch", {
          name: "settings.appVisibility.showProfileSwitcher",
        }),
      );
      await waitFor(() => expect(persisted.showProfileSwitcher).toBe(false));
      await waitFor(() => expect(view.client.isMutating()).toBe(0));
      await waitFor(() => expect(view.client.isFetching()).toBe(0));
      expect(saveMock.mock.calls[0][0][field]).toBe(savedPath);
      expect(persisted[field]).toBe(savedPath);
      expect(syncLiveMock).not.toHaveBeenCalled();

      // A second independent autosave must not consume the pending path either.
      fireEvent.click(
        screen.getByRole("switch", { name: "settings.minimizeToTray" }),
      );
      await waitFor(() => expect(persisted.minimizeToTrayOnClose).toBe(false));
      await waitFor(() => expect(view.client.isMutating()).toBe(0));
      await waitFor(() => expect(view.client.isFetching()).toBe(0));
      view.navigate("appConfig");
      expect(screen.getByPlaceholderText(placeholder)).toHaveValue(draftPath);
      fireEvent.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() => expect(persisted[field]).toBe(draftPath));
      await waitFor(() =>
        expect(
          screen.queryByRole("button", { name: "common.save" }),
        ).toBeNull(),
      );
      expect(syncLiveMock).toHaveBeenCalledTimes(
        field === "hermesConfigDir" || field === "piConfigDir" ? 0 : 1,
      );
    },
  );

  it("keeps resetting a saved directory to its default pending until Save", async () => {
    const view = await renderSettings();
    // The reset buttons follow the app rows: Claude, Codex, Gemini, Grok, OpenCode.
    fireEvent.click(
      screen.getAllByRole("button", { name: "settings.resetDefault" })[4],
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeVisible();
    view.navigate("general");
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    await waitFor(() => expect(persisted.minimizeToTrayOnClose).toBe(false));
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    await waitFor(() => expect(view.client.isFetching()).toBe(0));
    expect(persisted.opencodeConfigDir).toBe("/fixture/saved/opencode");
    view.navigate("appConfig");
    expect(screen.getByRole("button", { name: "common.save" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(persisted.opencodeConfigDir).toBeUndefined());
  });

  it("does not save a reverted draft or a cancelled directory picker", async () => {
    const view = await renderSettings();
    const input = screen.getByPlaceholderText(
      "settings.browsePlaceholderOpencode",
    );
    fireEvent.change(input, { target: { value: "/fixture/pending" } });
    fireEvent.change(input, { target: { value: "/fixture/saved/opencode" } });
    expect(screen.queryByRole("button", { name: "common.save" })).toBeNull();
    fireEvent.click(
      screen.getAllByRole("button", { name: "settings.browseDirectory" })[4],
    );
    await waitFor(() => expect(selectDirectoryMock).toHaveBeenCalledOnce());
    expect(input).toHaveValue("/fixture/saved/opencode");
    expect(saveMock).not.toHaveBeenCalled();
    view.navigate("general");
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    await waitFor(() => expect(persisted.minimizeToTrayOnClose).toBe(false));
    expect(persisted.opencodeConfigDir).toBe("/fixture/saved/opencode");
  });

  it("keeps a first directory override pending when the saved path is unset", async () => {
    persisted.opencodeConfigDir = undefined;
    const view = await renderSettings();
    fireEvent.change(
      screen.getByPlaceholderText("settings.browsePlaceholderOpencode"),
      {
        target: { value: "/fixture/first-override" },
      },
    );
    view.navigate("general");
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    await waitFor(() => expect(persisted.minimizeToTrayOnClose).toBe(false));
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(persisted.opencodeConfigDir).toBeUndefined();
    view.navigate("appConfig");
    expect(
      screen.getByPlaceholderText("settings.browsePlaceholderOpencode"),
    ).toHaveValue("/fixture/first-override");
    expect(screen.getByRole("button", { name: "common.save" })).toBeVisible();
  });

  it("keeps a newer edit pending when an earlier explicit Save finishes", async () => {
    const view = await renderSettings();
    const input = screen.getByPlaceholderText(
      "settings.browsePlaceholderOpenclaw",
    );
    fireEvent.change(input, { target: { value: "/fixture/first-edit" } });
    let finishSave!: () => void;
    saveMock.mockImplementationOnce(
      (settings: Settings) =>
        new Promise<boolean>((resolve) => {
          finishSave = () => {
            persisted = { ...settings };
            resolve(true);
          };
        }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    fireEvent.change(input, { target: { value: "/fixture/newer-edit" } });
    finishSave();
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(persisted.openclawConfigDir).toBe("/fixture/first-edit");
    expect(input).toHaveValue("/fixture/newer-edit");
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(persisted.openclawConfigDir).toBe("/fixture/newer-edit"),
    );
  });

  it("does not restore an old path when a toggle overlaps an explicit Save", async () => {
    const view = await renderSettings();
    fireEvent.change(
      screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
      {
        target: { value: "/fixture/explicitly-saved" },
      },
    );
    let finishSave!: () => void;
    saveMock.mockImplementationOnce(
      (settings: Settings) =>
        new Promise<boolean>((resolve) => {
          finishSave = () => {
            persisted = { ...settings };
            resolve(true);
          };
        }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    view.navigate("general");
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    finishSave();
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(persisted.minimizeToTrayOnClose).toBe(false);
    expect(persisted.openclawConfigDir).toBe("/fixture/explicitly-saved");
    expect(saveMock.mock.calls[1][0].openclawConfigDir).toBe(
      "/fixture/explicitly-saved",
    );
    view.navigate("appConfig");
    expect(
      screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
    ).toHaveValue("/fixture/explicitly-saved");
    expect(screen.queryByRole("button", { name: "common.save" })).toBeNull();
  });

  it("uses a successful directory Save even if its settings refetch fails", async () => {
    const view = await renderSettings();
    fireEvent.change(
      screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
      {
        target: { value: "/fixture/explicitly-saved" },
      },
    );
    getMock.mockRejectedValueOnce(new Error("synthetic refetch failure"));
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(view.client.getQueryState(["settings"])?.status).toBe("error");
    view.navigate("general");
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(persisted.openclawConfigDir).toBe("/fixture/explicitly-saved");
    expect(persisted.minimizeToTrayOnClose).toBe(false);
  });

  it("continues a queued toggle after a failed directory Save without committing the draft", async () => {
    const view = await renderSettings();
    fireEvent.change(
      screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
      {
        target: { value: "/fixture/pending/openclaw" },
      },
    );
    let failSave!: () => void;
    saveMock.mockImplementationOnce(
      () =>
        new Promise<boolean>((_resolve, reject) => {
          failSave = () => reject(new Error("synthetic save failure"));
        }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    view.navigate("general");
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    failSave();
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(persisted.openclawConfigDir).toBe("/fixture/saved/openclaw");
    expect(persisted.minimizeToTrayOnClose).toBe(false);
    view.navigate("appConfig");
    expect(
      screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
    ).toHaveValue("/fixture/pending/openclaw");
    expect(screen.getByRole("button", { name: "common.save" })).toBeVisible();
  });

  it("retains both preferences when two toggles overlap", async () => {
    const view = await renderSettings();
    view.navigate("general");
    let finishSave!: () => void;
    saveMock.mockImplementationOnce(
      (settings: Settings) =>
        new Promise<boolean>((resolve) => {
          finishSave = () => {
            persisted = { ...settings };
            resolve(true);
          };
        }),
    );
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.appVisibility.showProfileSwitcher",
      }),
    );
    finishSave();
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(persisted.minimizeToTrayOnClose).toBe(false);
    expect(persisted.showProfileSwitcher).toBe(false);
  });

  it("finishes already-requested queued saves safely after unmount", async () => {
    const view = await renderSettings();
    fireEvent.change(
      screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
      {
        target: { value: "/fixture/explicitly-saved" },
      },
    );
    let finishSave!: () => void;
    saveMock.mockImplementationOnce(
      (settings: Settings) =>
        new Promise<boolean>((resolve) => {
          finishSave = () => {
            persisted = { ...settings };
            resolve(true);
          };
        }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    view.navigate("general");
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    view.unmount();
    finishSave();
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(persisted.openclawConfigDir).toBe("/fixture/explicitly-saved");
    expect(persisted.minimizeToTrayOnClose).toBe(false);
    expect(view.container).toBeEmptyDOMElement();
  });

  it("refreshes untouched directory fields while preserving a dirty one", async () => {
    const view = await renderSettings();
    fireEvent.change(
      screen.getByPlaceholderText("settings.browsePlaceholderOpencode"),
      {
        target: { value: "/fixture/pending/opencode" },
      },
    );
    persisted.openclawConfigDir = "/fixture/refreshed/openclaw";
    await view.client.invalidateQueries({ queryKey: ["settings"] });
    await waitFor(() =>
      expect(
        screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
      ).toHaveValue("/fixture/refreshed/openclaw"),
    );
    expect(
      screen.getByPlaceholderText("settings.browsePlaceholderOpencode"),
    ).toHaveValue("/fixture/pending/opencode");
    expect(screen.getAllByRole("button", { name: "common.save" })).toHaveLength(
      1,
    );
  });

  it("discards a directory draft when the settings page is unmounted without Save", async () => {
    const view = await renderSettings();
    fireEvent.change(
      screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
      {
        target: { value: "/fixture/pending/openclaw" },
      },
    );
    view.unmount();
    await renderSettings();
    expect(
      screen.getByPlaceholderText("settings.browsePlaceholderOpenclaw"),
    ).toHaveValue("/fixture/saved/openclaw");
    expect(screen.queryByRole("button", { name: "common.save" })).toBeNull();
    expect(saveMock).not.toHaveBeenCalled();
  });
});

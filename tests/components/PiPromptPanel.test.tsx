import {
  QueryClient,
  QueryClientProvider,
  useMutation,
} from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import PromptPanel from "@/components/prompts/PromptPanel";
import { promptsApi, type Prompt } from "@/lib/api";

const mocks = vi.hoisted(() => ({
  state: {
    prompts: {} as Record<string, Prompt>,
    currentFileContent: null as string | null,
  },
  reload: vi.fn(),
  toggleEnabled: vi.fn(),
  importFromFile: vi.fn(),
  toastSuccess: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.name ? `${key}:${String(options.name)}` : key,
    i18n: { language: "en" },
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: vi.fn(),
    dismiss: vi.fn(),
  },
}));

vi.mock("@/hooks/usePromptActions", () => ({
  usePromptActions: () => ({
    prompts: mocks.state.prompts,
    loading: false,
    currentFileContent: mocks.state.currentFileContent,
    togglingId: null,
    reload: mocks.reload,
    savePrompt: vi.fn(),
    deletePrompt: vi.fn(),
    toggleEnabled: mocks.toggleEnabled,
    importFromFile: mocks.importFromFile,
    getLatestPrompts: () => null,
  }),
}));

vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: vi.fn(),
}));

function renderWithClient(ui: React.ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>{ui}</QueryClientProvider>,
  );
}

describe("Pi prompts page", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    mocks.state.prompts = {
      team: { id: "team", name: "Team", content: "# Team", enabled: false },
    };
    mocks.state.currentFileContent = "# Pi rules";
    mocks.reload.mockReset();
    mocks.reload.mockResolvedValue(true);
    mocks.toggleEnabled.mockReset();
    mocks.toggleEnabled.mockResolvedValue(true);
    mocks.importFromFile.mockReset();
    mocks.importFromFile.mockResolvedValue("imported");
    mocks.toastSuccess.mockReset();
    vi.spyOn(promptsApi, "getFileLocation").mockResolvedValue({
      path: "/Users/me/.pi/agent/AGENTS.md",
      displayPath: "~/.pi/agent/AGENTS.md",
    });
    vi.spyOn(promptsApi, "getPrompts").mockResolvedValue({});
    vi.spyOn(promptsApi, "listPiPromptTemplates").mockResolvedValue([]);
    vi.spyOn(promptsApi, "getPiPromptFile").mockResolvedValue({
      exists: false,
      revision: "missing",
      content: "",
    });
  });

  it.each(["pi", "rpi"] as const)(
    "switches %s between library, system prompt and templates",
    async (app) => {
      vi.mocked(promptsApi.getFileLocation).mockResolvedValue({
        path: `/Users/me/.${app}/agent/AGENTS.md`,
        displayPath: `~/.${app}/agent/AGENTS.md`,
      });
      renderWithClient(
        <PromptPanel
          appId={app}
          apps={["claude", "pi", "rpi"]}
          onAppChange={vi.fn()}
        />,
      );

      expect(
        await screen.findByRole("button", { name: "prompts.add" }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "pi.prompts.globalTab" }),
      ).toHaveAttribute("aria-pressed", "true");

      fireEvent.click(
        screen.getByRole("button", { name: "pi.prompts.systemTab" }),
      );
      expect(
        screen.queryByRole("button", { name: "prompts.add" }),
      ).not.toBeInTheDocument();
      expect(
        await screen.findByRole("list", {
          name: "pi.prompts.systemFilesLabel",
        }),
      ).toBeInTheDocument();
      expect(screen.getByText(`~/.${app}/agent/`)).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "pi.prompts.templatesTab" }),
      );
      expect(
        screen.getAllByRole("button", { name: "pi.prompts.newTemplate" })
          .length,
      ).toBeGreaterThan(0);
      expect(screen.getByText(`~/.${app}/agent/prompts/`)).toBeInTheDocument();
    },
  );

  it.each([
    ["pi", "rpi"],
    ["rpi", "pi"],
  ] as const)(
    "discards %s native drafts before switching to %s",
    async (from, to) => {
      vi.mocked(promptsApi.getPiPromptFile).mockImplementation(async (app) => ({
        exists: true,
        revision: `${app}-revision`,
        content: `${app} content`,
      }));
      const replace = vi.spyOn(promptsApi, "replacePiPromptFile");
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      // A previously visited app has warm native-file caches, so no loading
      // frame can accidentally hide/unmount the stale editor for us.
      client.setQueryData([to, "promptFile", "system_append"], {
        exists: true,
        revision: `${to}-revision`,
        content: `${to} content`,
      });
      const ui = (appId: "pi" | "rpi") => (
        <QueryClientProvider client={client}>
          <PromptPanel
            appId={appId}
            apps={["pi", "rpi"]}
            onAppChange={vi.fn()}
          />
        </QueryClientProvider>
      );
      const view = render(ui(from));
      fireEvent.click(
        screen.getByRole("button", { name: "pi.prompts.systemTab" }),
      );
      fireEvent.click(
        await screen.findByRole("button", {
          name: "common.edit APPEND_SYSTEM.md",
        }),
      );
      const editor = screen.getByPlaceholderText(
        "pi.prompts.instructionPlaceholder",
      );
      fireEvent.change(editor, { target: { value: `${from} unsaved secret` } });
      view.rerender(ui(to));
      expect(
        screen.queryByPlaceholderText("pi.prompts.instructionPlaceholder"),
      ).not.toBeInTheDocument();
      expect(replace).not.toHaveBeenCalled();
      fireEvent.click(
        screen.getByRole("button", { name: "pi.prompts.systemTab" }),
      );
      fireEvent.click(
        await screen.findByRole("button", {
          name: "common.edit APPEND_SYSTEM.md",
        }),
      );
      expect(
        screen.getByPlaceholderText("pi.prompts.instructionPlaceholder"),
      ).toHaveValue(`${to} content`);
    },
  );

  it.each(["pi", "rpi"] as const)(
    "locks %s navigation while its native editor is saving",
    async (app) => {
      function PendingNativeSave() {
        const save = useMutation({
          mutationKey: [app, "promptSave"],
          mutationFn: () => new Promise<void>(() => {}),
        });
        return (
          <button type="button" onClick={() => save.mutate()}>
            start-native-save
          </button>
        );
      }
      const onNavigationBlockedChange = vi.fn();
      renderWithClient(
        <>
          <PromptPanel
            appId={app}
            apps={["claude", "pi", "rpi"]}
            onAppChange={vi.fn()}
            onNavigationBlockedChange={onNavigationBlockedChange}
          />
          <PendingNativeSave />
        </>,
      );
      await screen.findByRole("button", { name: "prompts.add" });
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(false);
      fireEvent.click(
        screen.getByRole("button", { name: "start-native-save" }),
      );
      await waitFor(() =>
        expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(true),
      );
    },
  );

  it("does not let a Pi save control rpi navigation", async () => {
    function PendingPiSave() {
      const save = useMutation({
        mutationKey: ["pi", "promptSave"],
        mutationFn: () => new Promise<void>(() => {}),
      });
      return (
        <>
          <button onClick={() => save.mutate()}>start-pi-save</button>
          {save.isPending && <span>pi-save-pending</span>}
        </>
      );
    }
    const onNavigationBlockedChange = vi.fn();
    renderWithClient(
      <>
        <PromptPanel
          appId="rpi"
          apps={["pi", "rpi"]}
          onAppChange={vi.fn()}
          onNavigationBlockedChange={onNavigationBlockedChange}
        />
        <PendingPiSave />
      </>,
    );
    await screen.findByRole("button", { name: "prompts.add" });
    fireEvent.click(screen.getByRole("button", { name: "start-pi-save" }));
    await screen.findByText("pi-save-pending");
    await waitFor(() =>
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(false),
    );
    expect(onNavigationBlockedChange).not.toHaveBeenCalledWith(true);
  });

  it("warns about an external AGENTS.md and stores it in the library", async () => {
    renderWithClient(
      <PromptPanel appId="pi" apps={["pi"]} onAppChange={vi.fn()} />,
    );

    expect(
      await screen.findByText("pi.prompts.externalTitle"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "pi.prompts.saveToLibrary" }),
    );

    await waitFor(() => expect(mocks.importFromFile).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "prompts.toast.importedPi",
        expect.anything(),
      ),
    );
    // 导入的那条按内容相等就是启用中的，删不掉：不给一个一定失败的「撤销」
    const options = mocks.toastSuccess.mock.calls.find(
      (call) => call[0] === "prompts.toast.importedPi",
    )?.[1];
    expect(options?.action).toBeUndefined();
  });

  it("enables a library prompt and says AGENTS.md was written", async () => {
    mocks.state.currentFileContent = null;
    renderWithClient(
      <PromptPanel appId="pi" apps={["pi"]} onAppChange={vi.fn()} />,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "prompts.enableAria:Team" }),
    );

    await waitFor(() =>
      expect(mocks.toggleEnabled).toHaveBeenCalledWith("team", true),
    );
    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "prompts.toast.enabledPi:Team",
        expect.objectContaining({
          action: expect.objectContaining({ label: "prompts.undo" }),
        }),
      ),
    );
  });
});

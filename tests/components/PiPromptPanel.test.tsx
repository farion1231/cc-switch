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

  it("switches between library, system prompt and templates", async () => {
    renderWithClient(
      <PromptPanel appId="pi" apps={["claude", "pi"]} onAppChange={vi.fn()} />,
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
      await screen.findByRole("list", { name: "pi.prompts.systemFilesLabel" }),
    ).toBeInTheDocument();
    expect(screen.getByText("~/.pi/agent/")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "pi.prompts.templatesTab" }),
    );
    expect(
      screen.getAllByRole("button", { name: "pi.prompts.newTemplate" }).length,
    ).toBeGreaterThan(0);
    expect(screen.getByText("~/.pi/agent/prompts/")).toBeInTheDocument();
  });

  it("locks navigation while a system prompt or template editor is saving", async () => {
    function PendingNativeSave() {
      const save = useMutation({
        mutationKey: ["pi", "promptSave"],
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
          appId="pi"
          apps={["claude", "pi"]}
          onAppChange={vi.fn()}
          onNavigationBlockedChange={onNavigationBlockedChange}
        />
        <PendingNativeSave />
      </>,
    );
    await screen.findByRole("button", { name: "prompts.add" });
    expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(false);
    fireEvent.click(screen.getByRole("button", { name: "start-native-save" }));
    await waitFor(() =>
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(true),
    );
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

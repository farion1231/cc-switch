import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import PromptPanel, {
  type PromptPanelProps,
} from "@/components/prompts/PromptPanel";
import { promptsApi, type AppId, type Prompt } from "@/lib/api";

const mocks = vi.hoisted(() => ({
  state: {
    prompts: {} as Record<string, Prompt>,
    loading: false,
    currentFileContent: null as string | null,
    latest: null as Record<string, Prompt> | null,
  },
  reload: vi.fn(),
  getReload: vi.fn(),
  savePrompt: vi.fn(),
  deletePrompt: vi.fn(),
  toggleEnabled: vi.fn(),
  importFromFile: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      const value = options?.name ?? options?.path ?? options?.app;
      return value === undefined ? key : `${key}:${String(value)}`;
    },
    i18n: { language: "en" },
  }),
}));

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), {
    success: mocks.toastSuccess,
    error: mocks.toastError,
    dismiss: vi.fn(),
  }),
}));

vi.mock("@/hooks/usePromptActions", () => ({
  usePromptActions: (appId: AppId) => ({
    prompts: mocks.state.prompts,
    loading: mocks.state.loading,
    currentFileContent: mocks.state.currentFileContent,
    togglingId: null,
    reload: mocks.getReload(appId),
    savePrompt: mocks.savePrompt,
    deletePrompt: mocks.deletePrompt,
    toggleEnabled: mocks.toggleEnabled,
    importFromFile: mocks.importFromFile,
    getLatestPrompts: () => mocks.state.latest,
  }),
}));

vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: vi.fn(),
}));

vi.mock("@/components/prompts/PromptFormPanel", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("@/components/prompts/PromptFormPanel")
    >();
  return {
    ...actual,
    default: ({
      editingId,
      initialData,
      onSave,
      onClose,
      onDelete,
    }: {
      editingId?: string;
      initialData?: Prompt;
      onSave: (id: string, prompt: Prompt) => Promise<void | boolean>;
      onClose: () => void;
      onDelete?: (id: string) => void;
    }) => (
      <div data-testid="prompt-form">
        {editingId}:{initialData?.name}
        <button
          type="button"
          onClick={async () => {
            const saved = await onSave(
              editingId ?? "new-prompt",
              initialData ?? {
                id: "new-prompt",
                name: "New Prompt",
                content: "New content",
                enabled: false,
              },
            );
            if (saved !== false) onClose();
          }}
        >
          form-save
        </button>
        <button type="button" onClick={onClose}>
          form-close
        </button>
        {editingId && onDelete ? (
          <button type="button" onClick={() => onDelete(editingId)}>
            form-delete
          </button>
        ) : null}
      </div>
    ),
  };
});

const createPrompts = (): Record<string, Prompt> => ({
  "record-index-47": {
    id: "payload-identifier-92",
    name: "Aurora Prompt",
    description: "Contains the nebula phrase",
    content: "Follow the quasar instruction exactly.",
    enabled: true,
  },
  "second-record": {
    id: "second-payload",
    name: "Harbor Prompt",
    description: "Deployment checklist",
    content: "Prepare the release notes.",
    enabled: false,
  },
});

const APPS: AppId[] = ["claude", "codex", "gemini", "hermes"];

function Harness(props: Partial<PromptPanelProps> & { client: QueryClient }) {
  const { client, ...rest } = props;
  return (
    <QueryClientProvider client={client}>
      <PromptPanel
        appId="claude"
        apps={APPS}
        onAppChange={() => undefined}
        {...rest}
      />
    </QueryClientProvider>
  );
}

function renderPanel(props: Partial<PromptPanelProps> = {}) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = render(<Harness client={client} {...props} />);
  return {
    ...view,
    rerenderWith: (next: Partial<PromptPanelProps>) =>
      view.rerender(<Harness client={client} {...next} />),
  };
}

function searchFor(value: string) {
  fireEvent.change(
    screen.getByRole("textbox", { name: "prompts.searchAriaLabel" }),
    { target: { value } },
  );
}

const enableButton = (name: string) =>
  screen.getByRole("button", { name: `prompts.enableAria:${name}` });
const disableButton = (name: string) =>
  screen.getByRole("button", { name: `prompts.disableAria:${name}` });
const editButton = (name: string) =>
  screen.getByRole("button", { name: `prompts.editAria:${name}` });

async function waitForPanelReady() {
  await waitFor(() => {
    expect(editButton("Harbor Prompt")).toBeEnabled();
  });
}

async function openRowMenu(name: string) {
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("button", { name: `prompts.rowMoreActions:${name}` }),
  );
  return { user, menu: await screen.findByRole("menu") };
}

/** 最后一次 toast.success 的「撤销」 */
function lastUndo(): (() => void) | undefined {
  const call = mocks.toastSuccess.mock.calls.at(-1);
  return call?.[1]?.action?.onClick;
}

describe("PromptPanel", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    mocks.state.prompts = createPrompts();
    mocks.state.loading = false;
    mocks.state.currentFileContent = "Follow the quasar instruction exactly.";
    mocks.state.latest = null;
    mocks.reload.mockReset();
    mocks.reload.mockResolvedValue(true);
    mocks.getReload.mockReset();
    mocks.getReload.mockImplementation(() => mocks.reload);
    mocks.savePrompt.mockReset();
    mocks.savePrompt.mockResolvedValue(true);
    mocks.deletePrompt.mockReset();
    mocks.deletePrompt.mockResolvedValue(true);
    mocks.toggleEnabled.mockReset();
    mocks.toggleEnabled.mockResolvedValue(true);
    mocks.importFromFile.mockReset();
    mocks.importFromFile.mockResolvedValue("imported-1");
    mocks.toastSuccess.mockReset();
    mocks.toastError.mockReset();
    vi.spyOn(promptsApi, "getFileLocation").mockImplementation(async (app) => ({
      path: `/Users/me/.${app}/CLAUDE.md`,
      displayPath: `~/.${app}/CLAUDE.md`,
    }));
    vi.spyOn(promptsApi, "getPrompts").mockResolvedValue({});
    vi.spyOn(promptsApi, "upsertPrompt").mockResolvedValue(undefined);
    vi.spyOn(promptsApi, "enablePrompt").mockResolvedValue(undefined);
    vi.spyOn(promptsApi, "deletePrompt").mockResolvedValue(undefined);
  });

  it.each([
    ["record ID", "RECORD-INDEX-47"],
    ["prompt ID", "PAYLOAD-IDENTIFIER-92"],
    ["name", "  aUrOrA  "],
    ["description", "NEBULA PHRASE"],
    ["content", "QUASAR INSTRUCTION"],
  ])("filters by %s", async (_field, query) => {
    renderPanel();
    await waitForPanelReady();

    searchFor(query);

    expect(screen.getByText("Aurora Prompt")).toBeInTheDocument();
    expect(screen.queryByText("Harbor Prompt")).not.toBeInTheDocument();
  });

  it("distinguishes an empty prompt collection from no search matches", async () => {
    const view = renderPanel();
    await waitForPanelReady();

    searchFor("does-not-exist");
    expect(screen.getByText("prompts.noSearchResults")).toBeInTheDocument();

    mocks.state.prompts = {};
    view.rerenderWith({});

    expect(
      screen.getByText("prompts.emptyTitle:Claude Code"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("prompts.noSearchResults"),
    ).not.toBeInTheDocument();
    // 列表空了就不显示搜索框
    expect(
      screen.queryByRole("textbox", { name: "prompts.searchAriaLabel" }),
    ).not.toBeInTheDocument();
  });

  it("clears the query from the no-match state and restores all prompts", async () => {
    renderPanel();
    await waitForPanelReady();

    searchFor("zzz");
    const noMatch = screen.getByText("prompts.noSearchResults").parentElement!;
    fireEvent.click(
      within(noMatch).getByRole("button", { name: "prompts.clearSearch" }),
    );

    expect(
      screen.getByRole("textbox", { name: "prompts.searchAriaLabel" }),
    ).toHaveValue("");
    expect(screen.getByText("Aurora Prompt")).toBeInTheDocument();
    expect(screen.getByText("Harbor Prompt")).toBeInTheDocument();
  });

  it("clears the query when the app changes", async () => {
    const view = renderPanel();
    await waitForPanelReady();
    searchFor("aurora");

    view.rerenderWith({ appId: "codex" });

    await waitFor(() => {
      expect(
        screen.getByRole("textbox", { name: "prompts.searchAriaLabel" }),
      ).toHaveValue("");
    });
    expect(screen.getByText("Harbor Prompt")).toBeInTheDocument();
  });

  it("shows the target file, its size and the enabled badge", async () => {
    renderPanel();
    await waitForPanelReady();

    expect(await screen.findByText("~/.claude/CLAUDE.md")).toBeInTheDocument();
    expect(
      screen.getByTitle("prompts.targetFile ~/.claude/CLAUDE.md · 38 B"),
    ).toBeInTheDocument();
    const activeRow = screen.getByTestId("prompt-row-record-index-47");
    expect(within(activeRow).getByText("prompts.enabled")).toBeInTheDocument();
    expect(
      within(screen.getByTestId("prompt-row-second-record")).queryByText(
        "prompts.enabled",
      ),
    ).not.toBeInTheDocument();
  });

  it("preserves record IDs for filtered toggle, edit, and delete actions", async () => {
    renderPanel();
    await waitForPanelReady();
    searchFor("deployment");

    fireEvent.click(enableButton("Harbor Prompt"));
    expect(mocks.toggleEnabled).toHaveBeenCalledWith("second-record", true);
    await waitForPanelReady();

    fireEvent.click(editButton("Harbor Prompt"));
    expect(screen.getByTestId("prompt-form")).toHaveTextContent(
      "second-record:Harbor Prompt",
    );
    fireEvent.click(screen.getByRole("button", { name: "form-close" }));

    const { user, menu } = await openRowMenu("Harbor Prompt");
    await user.click(
      within(menu).getByRole("menuitem", { name: "common.delete" }),
    );

    await waitFor(() => {
      expect(mocks.deletePrompt).toHaveBeenCalledWith("second-record");
    });
  });

  it("deletes without a confirmation and restores the same record on undo", async () => {
    renderPanel();
    await waitForPanelReady();
    const original = mocks.state.prompts["second-record"];

    const { user, menu } = await openRowMenu("Harbor Prompt");
    await user.click(
      within(menu).getByRole("menuitem", { name: "common.delete" }),
    );

    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "prompts.toast.deleted:Harbor Prompt",
        expect.objectContaining({
          description: "prompts.toast.deletedSub:~/.claude/CLAUDE.md",
        }),
      ),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    act(() => lastUndo()?.());
    await waitFor(() =>
      expect(promptsApi.upsertPrompt).toHaveBeenCalledWith(
        "claude",
        "second-record",
        original,
      ),
    );
  });

  it("keeps the enabled prompt from being deleted and says why", async () => {
    renderPanel();
    await waitForPanelReady();

    const { menu } = await openRowMenu("Aurora Prompt");
    const item = within(menu).getByRole("menuitem", {
      name: /common.delete/,
    });
    expect(item).toHaveAttribute("data-disabled");
    expect(within(menu).getByText("prompts.deleteBlocked")).toBeInTheDocument();
  });

  it("deletes from the drawer after closing it", async () => {
    renderPanel();
    await waitForPanelReady();

    fireEvent.click(editButton("Harbor Prompt"));
    fireEvent.click(screen.getByRole("button", { name: "form-delete" }));

    await waitFor(() =>
      expect(mocks.deletePrompt).toHaveBeenCalledWith("second-record"),
    );
    expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();
  });

  it("disabling says the file was emptied and undo re-enables it", async () => {
    renderPanel({ appId: "hermes" });
    await waitForPanelReady();

    fireEvent.click(disableButton("Aurora Prompt"));

    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "prompts.toast.disabled:Aurora Prompt",
        expect.objectContaining({
          description: "prompts.toast.hermesOff",
        }),
      ),
    );
    act(() => lastUndo()?.());
    await waitFor(() =>
      expect(promptsApi.enablePrompt).toHaveBeenCalledWith(
        "hermes",
        "record-index-47",
      ),
    );
  });

  it("enabling reports a backup of the file and undo switches back", async () => {
    mocks.state.latest = {
      ...createPrompts(),
      "backup-1": {
        id: "backup-1",
        name: "Original 1",
        content: "x",
        enabled: false,
      },
    };
    renderPanel();
    await waitForPanelReady();

    fireEvent.click(enableButton("Harbor Prompt"));

    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "prompts.toast.enabled:Harbor Prompt",
        expect.objectContaining({
          description: "prompts.toast.backup:Original 1",
        }),
      ),
    );
    act(() => lastUndo()?.());
    await waitFor(() =>
      expect(promptsApi.enablePrompt).toHaveBeenCalledWith(
        "claude",
        "record-index-47",
      ),
    );
  });

  it("offers no undo when enabling would lose a hand-written file", async () => {
    mocks.state.prompts = {
      draft: { id: "draft", name: "Draft", content: "d", enabled: false },
    };
    mocks.state.currentFileContent = "hand-written";
    mocks.state.latest = {
      ...mocks.state.prompts,
      "backup-9": {
        id: "backup-9",
        name: "Orig",
        content: "x",
        enabled: false,
      },
    };
    renderPanel();
    await waitFor(() => expect(enableButton("Draft")).toBeEnabled());

    fireEvent.click(enableButton("Draft"));

    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
    expect(lastUndo()).toBeUndefined();
  });

  it("imports the existing file from the empty state with an undo", async () => {
    mocks.state.prompts = {};
    mocks.state.currentFileContent = "# Rules\n\nBe brief.";
    renderPanel({ appId: "grokbuild" });

    expect(
      await screen.findByText("prompts.emptyTitle:Grok Build"),
    ).toBeInTheDocument();
    expect(screen.getByText(/# Rules\s+Be brief\./)).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "prompts.importExisting" }),
    );

    await waitFor(() => expect(mocks.importFromFile).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "prompts.toast.imported:~/.grokbuild/CLAUDE.md",
        expect.anything(),
      ),
    );
    act(() => lastUndo()?.());
    await waitFor(() =>
      expect(promptsApi.deletePrompt).toHaveBeenCalledWith(
        "grokbuild",
        "imported-1",
      ),
    );
  });

  it("hides the import entry on an empty file", async () => {
    mocks.state.prompts = {};
    mocks.state.currentFileContent = "  \n";
    renderPanel();

    await screen.findByText("prompts.emptyTitle:Claude Code");
    expect(
      screen.queryByRole("button", { name: "prompts.importExisting" }),
    ).not.toBeInTheDocument();
  });

  it("copies a prompt to other apps, leaving Hermes unchecked by default", async () => {
    vi.mocked(promptsApi.getPrompts).mockImplementation(
      async (app): Promise<Record<string, Prompt>> =>
        app === "codex"
          ? {
              same: {
                id: "same",
                name: "Harbor Prompt",
                content: "",
                enabled: false,
              },
            }
          : {},
    );
    renderPanel();
    await waitForPanelReady();

    const { user, menu } = await openRowMenu("Harbor Prompt");
    await user.click(
      within(menu).getByRole("menuitem", { name: "prompts.copyToApps" }),
    );

    const dialog = await screen.findByRole("dialog");
    await waitFor(() =>
      expect(within(dialog).getByLabelText(/Gemini CLI/)).toBeChecked(),
    );
    expect(within(dialog).getByLabelText(/Codex/)).not.toBeChecked();
    expect(within(dialog).getByLabelText(/Hermes/)).not.toBeChecked();
    expect(
      within(dialog).getByText("prompts.copyDialog.sameName"),
    ).toBeInTheDocument();

    await user.click(
      within(dialog).getByRole("button", { name: "prompts.copyDialog.go" }),
    );

    await waitFor(() =>
      expect(promptsApi.upsertPrompt).toHaveBeenCalledWith(
        "gemini",
        expect.any(String),
        expect.objectContaining({
          name: "Harbor Prompt",
          content: "Prepare the release notes.",
          enabled: false,
        }),
      ),
    );
    expect(promptsApi.upsertPrompt).toHaveBeenCalledTimes(1);
  });

  it("serializes toggle writes and reports the interaction as blocked", async () => {
    let resolveToggle!: () => void;
    mocks.toggleEnabled.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        resolveToggle = resolve;
      }),
    );
    const onInteractionBlockedChange = vi.fn();
    renderPanel({ onInteractionBlockedChange });
    await waitForPanelReady();

    const toggle = enableButton("Harbor Prompt");
    fireEvent.click(toggle);
    fireEvent.click(toggle);

    expect(mocks.toggleEnabled).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
    });
    // 外观上的禁用晚 300ms 出现（useDelayedFlag），拦截本身是立即的
    await waitFor(() => expect(toggle).toBeDisabled());
    expect(editButton("Harbor Prompt")).toBeDisabled();
    expect(screen.getByRole("button", { name: "prompts.add" })).toBeDisabled();

    await act(async () => {
      resolveToggle();
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(false);
    });
  });

  it("blocks all prompt actions while the collection is loading", async () => {
    mocks.state.loading = true;
    mocks.state.prompts = {};
    const onInteractionBlockedChange = vi.fn();
    renderPanel({ onInteractionBlockedChange });

    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
    });
    expect(screen.getByText("prompts.loading")).toBeInTheDocument();
    const add = screen.getByRole("button", { name: "prompts.add" });
    fireEvent.click(add);
    expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();
    await waitFor(() => expect(add).toBeDisabled());
    fireEvent.click(add);
    expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();
    expect(mocks.toggleEnabled).not.toHaveBeenCalled();
  });

  it("queues external reloads until the active write finishes", async () => {
    renderPanel();
    await waitForPanelReady();
    expect(mocks.reload).toHaveBeenCalledTimes(1);
    mocks.reload.mockClear();

    let resolveToggle!: () => void;
    mocks.toggleEnabled.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        resolveToggle = resolve;
      }),
    );
    fireEvent.click(enableButton("Harbor Prompt"));

    act(() => {
      window.dispatchEvent(
        new CustomEvent("prompt-imported", { detail: { app: "claude" } }),
      );
      window.dispatchEvent(
        new CustomEvent("prompt-imported", { detail: { app: "claude" } }),
      );
    });
    expect(mocks.reload).not.toHaveBeenCalled();

    await act(async () => {
      resolveToggle();
      await Promise.resolve();
    });
    await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
  });

  it("runs one compensating reload when a toggle write cannot refresh", async () => {
    renderPanel();
    await waitForPanelReady();
    mocks.reload.mockClear();
    mocks.toggleEnabled.mockResolvedValueOnce(false);

    fireEvent.click(enableButton("Harbor Prompt"));

    await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
  });

  it("runs one compensating reload when a save write cannot refresh", async () => {
    renderPanel();
    await waitForPanelReady();
    mocks.reload.mockClear();
    mocks.savePrompt.mockResolvedValueOnce(false);

    fireEvent.click(editButton("Harbor Prompt"));
    fireEvent.click(screen.getByRole("button", { name: "form-save" }));

    await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
  });

  it("allows navigation but blocks interactions during a pure reload", async () => {
    let resolveReload!: (value: boolean) => void;
    mocks.reload.mockReturnValueOnce(
      new Promise<boolean>((resolve) => {
        resolveReload = resolve;
      }),
    );
    const onInteractionBlockedChange = vi.fn();
    const onNavigationBlockedChange = vi.fn();
    renderPanel({ onInteractionBlockedChange, onNavigationBlockedChange });

    await waitFor(() => {
      expect(mocks.reload).toHaveBeenCalledTimes(1);
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(false);
    });

    await act(async () => {
      resolveReload(true);
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(false);
    });
    expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(false);
  });

  it("refreshes on window focus and removes the listener on unmount", async () => {
    const { unmount } = renderPanel();
    await waitForPanelReady();
    mocks.reload.mockClear();

    fireEvent(window, new Event("focus"));
    await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
    await waitForPanelReady();
    unmount();
    mocks.reload.mockClear();
    fireEvent(window, new Event("focus"));
    expect(mocks.reload).not.toHaveBeenCalled();
  });

  it("keeps controls enabled through a quick focus reload but still blocks clicks", async () => {
    renderPanel();
    await waitForPanelReady();

    let resolveReload!: () => void;
    mocks.reload.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        resolveReload = resolve;
      }),
    );
    fireEvent(window, new Event("focus"));
    const add = screen.getByRole("button", { name: "prompts.add" });
    expect(add).not.toBeDisabled();
    fireEvent.click(add);
    expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();

    await act(async () => {
      resolveReload();
      await Promise.resolve();
    });
    expect(add).not.toBeDisabled();
  });

  it.each(["focus", "prompt-imported"])(
    "queues %s reloads while the drawer is open",
    async (trigger) => {
      renderPanel();
      await waitForPanelReady();
      mocks.reload.mockClear();

      fireEvent.click(editButton("Harbor Prompt"));
      fireEvent(
        window,
        trigger === "focus"
          ? new Event("focus")
          : new CustomEvent("prompt-imported", { detail: { app: "claude" } }),
      );
      expect(mocks.reload).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole("button", { name: "form-close" }));
      await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
    },
  );

  it("starts the latest app reload without waiting for an older app", async () => {
    let resolveClaudeReload!: () => void;
    const claudeReload = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveClaudeReload = resolve;
        }),
    );
    const codexReload = vi.fn().mockResolvedValue(undefined);
    mocks.getReload.mockImplementation((appId: AppId) =>
      appId === "codex" ? codexReload : claudeReload,
    );

    const view = renderPanel();
    await waitFor(() => expect(claudeReload).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "prompts.add" }),
      ).toBeDisabled(),
    );

    view.rerenderWith({ appId: "codex" });
    await waitFor(() => expect(codexReload).toHaveBeenCalledTimes(1));
    expect(claudeReload).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveClaudeReload();
      await Promise.resolve();
    });
    expect(codexReload).toHaveBeenCalledTimes(1);
  });

  it("locks form saves and cannot close the form while a save is pending", async () => {
    let resolveSave!: () => void;
    mocks.savePrompt.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        resolveSave = resolve;
      }),
    );
    const onInteractionBlockedChange = vi.fn();
    renderPanel({ onInteractionBlockedChange });
    await waitForPanelReady();

    fireEvent.click(editButton("Harbor Prompt"));
    const save = screen.getByRole("button", { name: "form-save" });
    fireEvent.click(save);
    fireEvent.click(save);
    fireEvent.click(screen.getByRole("button", { name: "form-close" }));

    expect(mocks.savePrompt).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("prompt-form")).toBeInTheDocument();
    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
    });

    await act(async () => {
      resolveSave();
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(false);
    });
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "prompts.toast.saved:Harbor Prompt",
      expect.anything(),
    );
  });

  it("closes stale drawers when the app changes", async () => {
    const view = renderPanel();
    await waitForPanelReady();

    fireEvent.click(editButton("Harbor Prompt"));
    expect(screen.getByTestId("prompt-form")).toBeInTheDocument();

    view.rerenderWith({ appId: "codex" });
    await waitFor(() => {
      expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();
    });
  });

  it("uses the Hermes rule in the page help", async () => {
    renderPanel({ appId: "hermes" });
    await waitForPanelReady();

    expect(
      screen.getByRole("button", { name: "prompts.helpHermesTitle" }),
    ).toBeInTheDocument();
  });

  it("switches apps from the picker", async () => {
    const onAppChange = vi.fn();
    renderPanel({ onAppChange });
    await waitForPanelReady();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: /Claude Code/ }));
    await user.click(await screen.findByRole("button", { name: /Hermes/ }));

    expect(onAppChange).toHaveBeenCalledWith("hermes");
  });
});

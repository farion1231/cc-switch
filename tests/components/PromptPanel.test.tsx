import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import PromptPanel, {
  type PromptPanelHandle,
} from "@/components/prompts/PromptPanel";
import type { AppId, Prompt } from "@/lib/api";

const mocks = vi.hoisted(() => ({
  state: {
    prompts: {} as Record<
      string,
      {
        id: string;
        name: string;
        content: string;
        description?: string;
        enabled: boolean;
      }
    >,
    loading: false,
  },
  reload: vi.fn(),
  getReload: vi.fn(),
  savePrompt: vi.fn(),
  deletePrompt: vi.fn(),
  toggleEnabled: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === "prompts.count") return `${key}:${options?.count}`;
      if (key === "prompts.enabledName") return `${key}:${options?.name}`;
      if (key === "prompts.confirm.deleteMessage") {
        return `${key}:${options?.name}`;
      }
      return key;
    },
  }),
}));

vi.mock("@/hooks/usePromptActions", () => ({
  usePromptActions: (appId: AppId) => ({
    prompts: mocks.state.prompts,
    loading: mocks.state.loading,
    reload: mocks.getReload(appId),
    savePrompt: mocks.savePrompt,
    deletePrompt: mocks.deletePrompt,
    toggleEnabled: mocks.toggleEnabled,
  }),
}));

vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: vi.fn(),
}));

vi.mock("@/components/prompts/PromptFormPanel", () => ({
  default: ({
    editingId,
    initialData,
    onSave,
    onClose,
  }: {
    editingId?: string;
    initialData?: Prompt;
    onSave: (id: string, prompt: Prompt) => Promise<void | boolean>;
    onClose: () => void;
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
    </div>
  ),
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({
    message,
    onConfirm,
    onCancel,
    pending,
  }: {
    message: string;
    onConfirm: (checked: boolean) => void;
    onCancel: () => void;
    pending?: boolean;
  }) => (
    <div role="dialog">
      <span>{message}</span>
      <button type="button" disabled={pending} onClick={() => onConfirm(false)}>
        confirm-dialog
      </button>
      <button type="button" disabled={pending} onClick={onCancel}>
        cancel-dialog
      </button>
    </div>
  ),
}));

const createPrompts = () => ({
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

function renderPanel(appId: AppId = "claude") {
  return render(
    <PromptPanel open appId={appId} onOpenChange={() => undefined} />,
  );
}

function searchFor(value: string) {
  fireEvent.change(
    screen.getByRole("textbox", { name: "prompts.searchAriaLabel" }),
    { target: { value } },
  );
}

async function waitForPanelReady() {
  await waitFor(() => {
    expect(screen.getAllByRole("switch")[0]).toBeEnabled();
  });
}

describe("PromptPanel", () => {
  beforeEach(() => {
    mocks.state.prompts = createPrompts();
    mocks.state.loading = false;
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
    expect(screen.queryByText("prompts.empty")).not.toBeInTheDocument();

    mocks.state.prompts = {};
    view.rerender(
      <PromptPanel open appId="claude" onOpenChange={() => undefined} />,
    );

    expect(screen.getByText("prompts.empty")).toBeInTheDocument();
    expect(
      screen.queryByText("prompts.noSearchResults"),
    ).not.toBeInTheDocument();
  });

  it("clears the query and restores all prompts", async () => {
    renderPanel();
    await waitForPanelReady();
    const input = screen.getByRole("textbox", {
      name: "prompts.searchAriaLabel",
    });

    searchFor("aurora");
    expect(screen.queryByText("Harbor Prompt")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.clear" }));

    expect(input).toHaveValue("");
    expect(screen.getByText("Aurora Prompt")).toBeInTheDocument();
    expect(screen.getByText("Harbor Prompt")).toBeInTheDocument();
  });

  it("clears the query when the app changes", async () => {
    const view = renderPanel("claude");
    await waitForPanelReady();
    searchFor("aurora");

    view.rerender(
      <PromptPanel open appId="codex" onOpenChange={() => undefined} />,
    );

    await waitFor(() => {
      expect(
        screen.getByRole("textbox", { name: "prompts.searchAriaLabel" }),
      ).toHaveValue("");
    });
    await waitForPanelReady();
    expect(screen.getByText("Aurora Prompt")).toBeInTheDocument();
    expect(screen.getByText("Harbor Prompt")).toBeInTheDocument();
  });

  it("keeps totals and the enabled prompt based on the full collection", async () => {
    const { container } = renderPanel();
    await waitForPanelReady();

    searchFor("harbor");

    const summary = container.querySelector(".glass .text-sm");
    expect(summary).toHaveTextContent("prompts.count:2");
    expect(summary).toHaveTextContent("prompts.enabledName:Aurora Prompt");
    expect(screen.queryByText("Aurora Prompt")).not.toBeInTheDocument();
  });

  it("preserves record IDs for filtered toggle, edit, and delete actions", async () => {
    renderPanel();
    await waitForPanelReady();
    searchFor("quasar instruction");

    fireEvent.click(screen.getByRole("switch"));
    expect(mocks.toggleEnabled).toHaveBeenCalledWith("record-index-47", false);
    await waitFor(() => {
      expect(screen.getByTitle("common.edit")).toBeEnabled();
    });

    fireEvent.click(screen.getByTitle("common.edit"));
    expect(screen.getByTestId("prompt-form")).toHaveTextContent(
      "record-index-47:Aurora Prompt",
    );
    fireEvent.click(screen.getByRole("button", { name: "form-close" }));

    fireEvent.click(screen.getByTitle("common.delete"));
    expect(
      screen.getByText("prompts.confirm.deleteMessage:Aurora Prompt"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "confirm-dialog" }));

    await waitFor(() => {
      expect(mocks.deletePrompt).toHaveBeenCalledWith("record-index-47");
    });
  });

  it("keeps the search field outside the scrollable viewport", async () => {
    const { container } = renderPanel();
    await waitForPanelReady();
    const input = screen.getByRole("textbox", {
      name: "prompts.searchAriaLabel",
    });
    const viewport = container.querySelector(
      "[data-radix-scroll-area-viewport]",
    );

    expect(viewport).not.toBeNull();
    expect(viewport).not.toContainElement(input);
  });

  it("serializes toggle writes and reports the interaction as blocked", async () => {
    let resolveToggle!: () => void;
    mocks.toggleEnabled.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        resolveToggle = resolve;
      }),
    );
    const onInteractionBlockedChange = vi.fn();
    const ref = createRef<PromptPanelHandle>();
    render(
      <PromptPanel
        ref={ref}
        open
        appId="claude"
        onOpenChange={() => undefined}
        onInteractionBlockedChange={onInteractionBlockedChange}
      />,
    );
    await waitForPanelReady();

    const toggle = screen.getAllByRole("switch")[0];
    fireEvent.click(toggle);
    fireEvent.click(toggle);

    expect(mocks.toggleEnabled).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
    });
    expect(toggle).toBeDisabled();
    expect(screen.getAllByTitle("common.edit")[0]).toBeDisabled();
    expect(screen.getAllByTitle("common.delete")[0]).toBeDisabled();

    act(() => ref.current?.openAdd());
    expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();

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
    const ref = createRef<PromptPanelHandle>();
    const onInteractionBlockedChange = vi.fn();
    render(
      <PromptPanel
        ref={ref}
        open
        appId="claude"
        onOpenChange={() => undefined}
        onInteractionBlockedChange={onInteractionBlockedChange}
      />,
    );

    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
    });
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByTitle("common.edit")).not.toBeInTheDocument();
    expect(screen.queryByTitle("common.delete")).not.toBeInTheDocument();

    act(() => ref.current?.openAdd());
    expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();
    expect(mocks.toggleEnabled).not.toHaveBeenCalled();
    expect(mocks.savePrompt).not.toHaveBeenCalled();
    expect(mocks.deletePrompt).not.toHaveBeenCalled();
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
    fireEvent.click(screen.getAllByRole("switch")[0]);

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

    fireEvent.click(screen.getAllByRole("switch")[0]);

    await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
  });

  it("runs one compensating reload when a delete write cannot refresh", async () => {
    renderPanel();
    await waitForPanelReady();
    mocks.reload.mockClear();
    mocks.deletePrompt.mockResolvedValueOnce(false);

    fireEvent.click(screen.getAllByTitle("common.delete")[0]);
    fireEvent.click(screen.getByRole("button", { name: "confirm-dialog" }));

    await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
  });

  it("runs one compensating reload when a save write cannot refresh", async () => {
    renderPanel();
    await waitForPanelReady();
    mocks.reload.mockClear();
    mocks.savePrompt.mockResolvedValueOnce(false);

    fireEvent.click(screen.getAllByTitle("common.edit")[0]);
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

    render(
      <PromptPanel
        open
        appId="claude"
        onOpenChange={() => undefined}
        onInteractionBlockedChange={onInteractionBlockedChange}
        onNavigationBlockedChange={onNavigationBlockedChange}
      />,
    );

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

  it("queues external reloads while an edit or confirmation is open", async () => {
    renderPanel();
    await waitForPanelReady();
    mocks.reload.mockClear();

    fireEvent.click(screen.getAllByTitle("common.edit")[0]);
    act(() => {
      window.dispatchEvent(
        new CustomEvent("prompt-imported", { detail: { app: "claude" } }),
      );
    });
    expect(mocks.reload).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "form-close" }));
    await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
    mocks.reload.mockClear();
    await waitForPanelReady();

    fireEvent.click(screen.getAllByTitle("common.delete")[0]);
    act(() => {
      window.dispatchEvent(
        new CustomEvent("prompt-imported", { detail: { app: "claude" } }),
      );
    });
    expect(mocks.reload).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "cancel-dialog" }));
    await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
  });

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

    const ref = createRef<PromptPanelHandle>();
    const view = render(
      <PromptPanel
        ref={ref}
        open
        appId="claude"
        onOpenChange={() => undefined}
      />,
    );
    await waitFor(() => expect(claudeReload).toHaveBeenCalledTimes(1));
    act(() => ref.current?.openAdd());
    expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();

    view.rerender(
      <PromptPanel
        ref={ref}
        open
        appId="codex"
        onOpenChange={() => undefined}
      />,
    );
    await waitFor(() => expect(codexReload).toHaveBeenCalledTimes(1));
    expect(claudeReload).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveClaudeReload();
      await Promise.resolve();
    });
    expect(codexReload).toHaveBeenCalledTimes(1);
  });

  it("keeps delete confirmation pending and ignores duplicate confirms", async () => {
    let resolveDelete!: () => void;
    mocks.deletePrompt.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        resolveDelete = resolve;
      }),
    );
    renderPanel();
    await waitForPanelReady();

    fireEvent.click(screen.getAllByTitle("common.delete")[0]);
    const confirm = screen.getByRole("button", { name: "confirm-dialog" });
    const cancel = screen.getByRole("button", { name: "cancel-dialog" });
    fireEvent.click(confirm);
    fireEvent.click(confirm);

    expect(mocks.deletePrompt).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(confirm).toBeDisabled();
      expect(cancel).toBeDisabled();
    });

    await act(async () => {
      resolveDelete();
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  });

  it("locks form saves and cannot close the form while a save is pending", async () => {
    let resolveSave!: () => void;
    mocks.savePrompt.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        resolveSave = resolve;
      }),
    );
    const onInteractionBlockedChange = vi.fn();
    render(
      <PromptPanel
        open
        appId="claude"
        onOpenChange={() => undefined}
        onInteractionBlockedChange={onInteractionBlockedChange}
      />,
    );
    await waitForPanelReady();

    fireEvent.click(screen.getAllByTitle("common.edit")[0]);
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
  });

  it("closes stale forms and confirmations when the app changes", async () => {
    const view = renderPanel("claude");
    await waitForPanelReady();

    fireEvent.click(screen.getAllByTitle("common.edit")[0]);
    expect(screen.getByTestId("prompt-form")).toBeInTheDocument();

    view.rerender(
      <PromptPanel open appId="codex" onOpenChange={() => undefined} />,
    );
    await waitFor(() => {
      expect(screen.queryByTestId("prompt-form")).not.toBeInTheDocument();
    });
    await waitForPanelReady();

    fireEvent.click(screen.getAllByTitle("common.delete")[0]);
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    view.rerender(
      <PromptPanel open appId="gemini" onOpenChange={() => undefined} />,
    );
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  });
});

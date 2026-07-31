import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import PromptPanel from "@/components/prompts/PromptPanel";
import type { AppId } from "@/lib/api";

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
  usePromptActions: () => ({
    prompts: mocks.state.prompts,
    loading: mocks.state.loading,
    reload: mocks.reload,
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
  }: {
    editingId?: string;
    initialData?: { name: string };
  }) => (
    <div data-testid="prompt-form">
      {editingId}:{initialData?.name}
    </div>
  ),
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({
    message,
    onConfirm,
  }: {
    message: string;
    onConfirm: (checked: boolean) => void;
  }) => (
    <div role="dialog">
      <span>{message}</span>
      <button type="button" onClick={() => onConfirm(false)}>
        confirm-dialog
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

describe("PromptPanel", () => {
  beforeEach(() => {
    mocks.state.prompts = createPrompts();
    mocks.state.loading = false;
    mocks.reload.mockReset();
    mocks.reload.mockResolvedValue(undefined);
    mocks.savePrompt.mockReset();
    mocks.savePrompt.mockResolvedValue(undefined);
    mocks.deletePrompt.mockReset();
    mocks.deletePrompt.mockResolvedValue(undefined);
    mocks.toggleEnabled.mockReset();
    mocks.toggleEnabled.mockResolvedValue(undefined);
  });

  it.each([
    ["record ID", "RECORD-INDEX-47"],
    ["prompt ID", "PAYLOAD-IDENTIFIER-92"],
    ["name", "  aUrOrA  "],
    ["description", "NEBULA PHRASE"],
    ["content", "QUASAR INSTRUCTION"],
  ])("filters by %s", (_field, query) => {
    renderPanel();

    searchFor(query);

    expect(screen.getByText("Aurora Prompt")).toBeInTheDocument();
    expect(screen.queryByText("Harbor Prompt")).not.toBeInTheDocument();
  });

  it("distinguishes an empty prompt collection from no search matches", () => {
    const view = renderPanel();

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

  it("clears the query and restores all prompts", () => {
    renderPanel();
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
    searchFor("aurora");

    view.rerender(
      <PromptPanel open appId="codex" onOpenChange={() => undefined} />,
    );

    await waitFor(() => {
      expect(
        screen.getByRole("textbox", { name: "prompts.searchAriaLabel" }),
      ).toHaveValue("");
    });
    expect(screen.getByText("Aurora Prompt")).toBeInTheDocument();
    expect(screen.getByText("Harbor Prompt")).toBeInTheDocument();
  });

  it("keeps totals and the enabled prompt based on the full collection", () => {
    const { container } = renderPanel();

    searchFor("harbor");

    const summary = container.querySelector(".glass .text-sm");
    expect(summary).toHaveTextContent("prompts.count:2");
    expect(summary).toHaveTextContent("prompts.enabledName:Aurora Prompt");
    expect(screen.queryByText("Aurora Prompt")).not.toBeInTheDocument();
  });

  it("preserves record IDs for filtered toggle, edit, and delete actions", async () => {
    renderPanel();
    searchFor("quasar instruction");

    fireEvent.click(screen.getByRole("switch"));
    expect(mocks.toggleEnabled).toHaveBeenCalledWith("record-index-47", false);

    fireEvent.click(screen.getByTitle("common.edit"));
    expect(screen.getByTestId("prompt-form")).toHaveTextContent(
      "record-index-47:Aurora Prompt",
    );

    fireEvent.click(screen.getByTitle("common.delete"));
    expect(
      screen.getByText("prompts.confirm.deleteMessage:Aurora Prompt"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "confirm-dialog" }));

    await waitFor(() => {
      expect(mocks.deletePrompt).toHaveBeenCalledWith("record-index-47");
    });
  });

  it("keeps the search field outside the scrollable viewport", () => {
    const { container } = renderPanel();
    const input = screen.getByRole("textbox", {
      name: "prompts.searchAriaLabel",
    });
    const viewport = container.querySelector(
      "[data-radix-scroll-area-viewport]",
    );

    expect(viewport).not.toBeNull();
    expect(viewport).not.toContainElement(input);
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import PromptPanel from "@/components/prompts/PromptPanel";
import { promptsApi, type AppId, type GlobalPromptEntry } from "@/lib/api";

const mocks = vi.hoisted(() => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === "prompts.listSeparator") return ", ";
      const value = options?.name ?? options?.apps ?? options?.app;
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

vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: vi.fn(),
}));

const APPS: AppId[] = ["claude", "codex", "mcode"];

const entry = (
  overrides: Partial<GlobalPromptEntry> = {},
): GlobalPromptEntry => ({
  id: "g1",
  name: "Team Rules",
  content: "# Rules\nBe concise.",
  description: "Shared coding rules",
  createdAt: 1,
  updatedAt: 1,
  apps: { claude: true },
  modifiedApps: [],
  ...overrides,
});

function renderGlobal() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const onAppChange = vi.fn();
  render(
    <QueryClientProvider client={client}>
      <PromptPanel appId="global" apps={APPS} onAppChange={onAppChange} />
    </QueryClientProvider>,
  );
  return { onAppChange };
}

/** 某一行里某个应用列的勾选框（列顺序 = APPS） */
function cellFor(rowName: string, app: AppId) {
  const row = screen.getByText(rowName).closest("li") as HTMLElement;
  const cells = within(row).getAllByRole("button", {
    name: /^appMatrix\.cell\./,
  });
  return cells[APPS.indexOf(app)] as HTMLButtonElement;
}

describe("GlobalPromptPanel", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    mocks.toastSuccess.mockReset();
    mocks.toastError.mockReset();
    vi.spyOn(promptsApi, "getPrompts").mockResolvedValue({});
  });

  it("shows which apps use each global prompt", async () => {
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([entry()]);
    renderGlobal();

    await screen.findByText("Team Rules");
    expect(cellFor("Team Rules", "claude")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(cellFor("Team Rules", "codex")).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    // 列头每个应用一列
    for (const name of ["Claude Code", "Codex", "MiniMax Code"]) {
      expect(screen.getByRole("img", { name })).toBeInTheDocument();
    }
  });

  it("ticking an app enables it there and reports the prompt it replaced", async () => {
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([entry()]);
    vi.spyOn(promptsApi, "getPrompts").mockResolvedValue({
      local: {
        id: "local",
        name: "Old Codex Prompt",
        content: "old",
        enabled: true,
      },
    });
    const setApp = vi
      .spyOn(promptsApi, "setGlobalPromptApp")
      .mockResolvedValue(undefined);
    const enable = vi
      .spyOn(promptsApi, "enablePrompt")
      .mockResolvedValue(undefined);
    renderGlobal();
    await screen.findByText("Team Rules");

    fireEvent.click(cellFor("Team Rules", "codex"));

    await waitFor(() =>
      expect(setApp).toHaveBeenCalledWith("g1", "codex", true),
    );
    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
    const [title, options] = mocks.toastSuccess.mock.calls.at(-1)!;
    expect(title).toBe("prompts.global.toast.applied:Team Rules");
    expect(options.description).toContain(
      "prompts.global.toast.replaced:Old Codex Prompt",
    );

    // 撤销：换回之前启用的那条
    options.action.onClick();
    await waitFor(() => expect(enable).toHaveBeenCalledWith("codex", "local"));
  });

  it("unticking an app disables it there with an undo", async () => {
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([entry()]);
    const setApp = vi
      .spyOn(promptsApi, "setGlobalPromptApp")
      .mockResolvedValue(undefined);
    renderGlobal();
    await screen.findByText("Team Rules");

    fireEvent.click(cellFor("Team Rules", "claude"));

    await waitFor(() =>
      expect(setApp).toHaveBeenCalledWith("g1", "claude", false),
    );
    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
    const [title, options] = mocks.toastSuccess.mock.calls.at(-1)!;
    expect(title).toBe("prompts.global.toast.removed:Team Rules");
    options.action.onClick();
    await waitFor(() =>
      expect(setApp).toHaveBeenLastCalledWith("g1", "claude", true),
    );
  });

  it("marks a failed write on the cell and in the row", async () => {
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([entry()]);
    vi.spyOn(promptsApi, "setGlobalPromptApp").mockRejectedValue(
      new Error("too large"),
    );
    renderGlobal();
    await screen.findByText("Team Rules");

    fireEvent.click(cellFor("Team Rules", "mcode"));

    await waitFor(() =>
      expect(mocks.toastError).toHaveBeenCalledWith(
        "prompts.global.toast.applyFailed:MiniMax Code",
        expect.anything(),
      ),
    );
    expect(cellFor("Team Rules", "mcode")).toHaveAttribute(
      "data-state",
      "fail",
    );
    expect(
      screen.getByText("prompts.global.rowFail:MiniMax Code"),
    ).toBeInTheDocument();
  });

  it("offers to rewrite files that were edited elsewhere", async () => {
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([
      entry({ apps: { claude: true, codex: true }, modifiedApps: ["codex"] }),
    ]);
    const setApp = vi
      .spyOn(promptsApi, "setGlobalPromptApp")
      .mockResolvedValue(undefined);
    renderGlobal();

    expect(
      await screen.findByText(/prompts\.global\.modified:Codex/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "prompts.global.reapply" }),
    );

    await waitFor(() =>
      expect(setApp).toHaveBeenCalledWith("g1", "codex", true),
    );
    expect(setApp).toHaveBeenCalledTimes(1);
  });

  it("blocks deleting a prompt that is still in use and says where", async () => {
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([
      entry({ apps: { claude: true, codex: true } }),
    ]);
    const remove = vi.spyOn(promptsApi, "deleteGlobalPrompt");
    renderGlobal();
    await screen.findByText("Team Rules");
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", {
        name: "prompts.rowMoreActions:Team Rules",
      }),
    );
    const menu = await screen.findByRole("menu");
    const item = within(menu).getByRole("menuitem", { name: /common\.delete/ });
    expect(item).toHaveAttribute("data-disabled");
    expect(
      within(item).getByText("prompts.global.deleteBlocked:Claude Code, Codex"),
    ).toBeInTheDocument();
    expect(remove).not.toHaveBeenCalled();
  });

  it("deletes an unused prompt and restores it on undo", async () => {
    const unused = entry({ apps: {} });
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([unused]);
    const remove = vi
      .spyOn(promptsApi, "deleteGlobalPrompt")
      .mockResolvedValue(undefined);
    const upsert = vi
      .spyOn(promptsApi, "upsertGlobalPrompt")
      .mockResolvedValue({ id: "g1", failures: [] });
    renderGlobal();
    await screen.findByText("Team Rules");
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", {
        name: "prompts.rowMoreActions:Team Rules",
      }),
    );
    await user.click(
      within(await screen.findByRole("menu")).getByRole("menuitem", {
        name: "common.delete",
      }),
    );

    await waitFor(() => expect(remove).toHaveBeenCalledWith("g1"));
    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
    mocks.toastSuccess.mock.calls.at(-1)![1].action.onClick();
    await waitFor(() =>
      expect(upsert).toHaveBeenCalledWith(
        expect.objectContaining({ id: "g1", name: "Team Rules" }),
      ),
    );
  });

  it("creates a global prompt from the empty state", async () => {
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([]);
    const upsert = vi
      .spyOn(promptsApi, "upsertGlobalPrompt")
      .mockResolvedValue({ id: "new-id", failures: [] });
    renderGlobal();

    expect(
      await screen.findByText("prompts.global.emptyTitle"),
    ).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(
      screen.getAllByRole("button", { name: "prompts.global.add" })[0],
    );
    await user.type(
      await screen.findByLabelText(/prompts\.name/),
      "Review Checklist",
    );
    await user.type(screen.getByLabelText("prompts.content"), "Check tests");
    await user.click(screen.getByRole("button", { name: "prompts.addSubmit" }));

    await waitFor(() =>
      expect(upsert).toHaveBeenCalledWith({
        id: "",
        name: "Review Checklist",
        content: "Check tests",
        description: undefined,
        createdAt: undefined,
      }),
    );
  });

  it("lists the apps an edit will sync to in the form", async () => {
    vi.spyOn(promptsApi, "getGlobalPrompts").mockResolvedValue([
      entry({ apps: { claude: true, mcode: true } }),
    ]);
    const upsert = vi
      .spyOn(promptsApi, "upsertGlobalPrompt")
      .mockResolvedValue({
        id: "g1",
        failures: [{ app: "mcode", error: "disk full" }],
      });
    renderGlobal();
    const user = userEvent.setup();
    await user.click(await screen.findByText("Team Rules"));

    expect(
      await screen.findByText(
        "prompts.global.formNote:Claude Code, MiniMax Code",
      ),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "prompts.global.saveAndSync" }),
    );

    await waitFor(() =>
      expect(upsert).toHaveBeenCalledWith(
        expect.objectContaining({ id: "g1", createdAt: 1 }),
      ),
    );
    // 内容已保存：成功的应用照常报告，失败的单独报错
    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "prompts.global.toast.savedSynced:Team Rules",
        expect.anything(),
      ),
    );
    expect(mocks.toastError).toHaveBeenCalledWith(
      "prompts.global.toast.syncFailed:MiniMax Code",
      expect.objectContaining({ description: "disk full" }),
    );
  });
});

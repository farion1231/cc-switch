import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import SubagentsPanel from "@/components/subagents/SubagentsPanel";
import type { Subagent, UnmanagedSubagent } from "@/lib/api/subagents";

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  getDir: vi.fn(),
  scanUnmanaged: vi.fn(),
  listPlugin: vi.fn(),
  toggle: vi.fn(),
  upsert: vi.fn(),
  delete: vi.fn(),
  import: vi.fn(),
  resolveDrift: vi.fn(),
  getDiskContent: vi.fn(),
  getModelOptions: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      const value = options?.name ?? options?.path;
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

vi.mock("@/lib/api/subagents", () => ({
  subagentsApi: {
    list: mocks.list,
    getDir: mocks.getDir,
    scanUnmanaged: mocks.scanUnmanaged,
    listPlugin: mocks.listPlugin,
    toggle: mocks.toggle,
    upsert: mocks.upsert,
    delete: mocks.delete,
    import: mocks.import,
    resolveDrift: mocks.resolveDrift,
    getDiskContent: mocks.getDiskContent,
    getModelOptions: mocks.getModelOptions,
  },
}));

const reviewer: Subagent = {
  id: "a1",
  name: "code-reviewer",
  fileName: "code-reviewer.md",
  description: "Reviews code",
  content:
    "---\nname: code-reviewer\ndescription: Reviews code\ntools: Read, Grep, Glob, Bash\nmodel: opus\ncolor: blue\n---\n\nReview.\n",
  enabled: true,
  source: "local",
  status: "drifted",
  filePath: "/home/u/.claude/agents/code-reviewer.md",
};

const tester: Subagent = {
  id: "a2",
  name: "tester",
  fileName: "tester.md",
  content: "---\nname: tester\ndescription: Runs tests\n---\n\nTest.\n",
  enabled: false,
  source: "imported",
  status: "disabled",
  filePath: "/home/u/.claude/agents/tester.md",
};

const unmanaged: UnmanagedSubagent[] = [
  {
    fileName: "helper.md",
    path: "/home/u/.claude/agents/helper.md",
    name: "helper",
    description: "Helps",
    content: "---\nname: helper\ndescription: Helps\n---\n",
  },
  {
    fileName: "broken.md",
    path: "/home/u/.claude/agents/broken.md",
    content: "---\nname: [oops\n---\n",
    parseError: "bad yaml",
  },
];

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <SubagentsPanel />
    </QueryClientProvider>,
  );
}

describe("SubagentsPanel", () => {
  beforeEach(() => {
    mocks.list.mockResolvedValue([reviewer, tester]);
    mocks.getDir.mockResolvedValue({
      path: "/home/u/.claude/agents",
      displayPath: "~/.claude/agents",
      exists: true,
    });
    mocks.scanUnmanaged.mockResolvedValue(unmanaged);
    mocks.listPlugin.mockResolvedValue([]);
  });

  it("shows model, tools summary and drift status for managed rows", async () => {
    renderPanel();
    const row = await screen.findByTestId("subagent-row-a1");
    expect(within(row).getByText("opus")).toBeInTheDocument();
    expect(
      within(row).getByText("subagents.tools.summaryMore"),
    ).toBeInTheDocument();
    expect(
      within(row).getByText("subagents.status.drifted"),
    ).toBeInTheDocument();
    expect(
      within(row).getByRole("button", { name: "subagents.actions.resolve" }),
    ).toBeInTheDocument();

    const other = screen.getByTestId("subagent-row-a2");
    expect(
      within(other).getByText("subagents.model.inheritShort"),
    ).toBeInTheDocument();
    expect(within(other).getByText("subagents.tools.all")).toBeInTheDocument();
  });

  it("asks before retrying a drifted write with force", async () => {
    const user = userEvent.setup();
    mocks.toggle
      .mockRejectedValueOnce("SUBAGENT_DRIFTED: code-reviewer.md changed")
      .mockResolvedValueOnce({ ...reviewer, enabled: false });
    renderPanel();

    const row = await screen.findByTestId("subagent-row-a1");
    await user.click(
      within(row).getByRole("switch", {
        name: "subagents.actions.disableAria:code-reviewer",
      }),
    );

    expect(mocks.toggle).toHaveBeenCalledWith("a1", false, false);
    const confirm = await screen.findByRole("button", {
      name: "subagents.conflict.driftedConfirm",
    });
    expect(mocks.toastError).not.toHaveBeenCalled();
    await user.click(confirm);

    await waitFor(() =>
      expect(mocks.toggle).toHaveBeenLastCalledWith("a1", false, true),
    );
    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
  });

  it("toasts other errors without asking", async () => {
    const user = userEvent.setup();
    mocks.toggle.mockRejectedValueOnce("SUBAGENT_INVALID: missing name");
    renderPanel();

    const row = await screen.findByTestId("subagent-row-a2");
    await user.click(
      within(row).getByRole("switch", {
        name: "subagents.actions.enableAria:tester",
      }),
    );

    await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
    expect(
      screen.queryByRole("button", {
        name: "subagents.conflict.driftedConfirm",
      }),
    ).not.toBeInTheDocument();
  });

  it("imports only parsable unmanaged files", async () => {
    const user = userEvent.setup();
    mocks.import.mockResolvedValue([{ ...tester, id: "a3", name: "helper" }]);
    renderPanel();

    await screen.findByTestId("subagent-row-a1");
    await user.click(
      screen.getByRole("tab", { name: /subagents.tabs.unmanagedCount/ }),
    );

    expect(
      await screen.findByRole("checkbox", {
        name: "subagents.unmanaged.selectAria:helper",
      }),
    ).toBeChecked();
    const broken = screen.getByRole("checkbox", {
      name: "subagents.unmanaged.selectAria:broken.md",
    });
    expect(broken).toBeDisabled();
    expect(broken).not.toBeChecked();

    await user.click(
      screen.getByRole("button", {
        name: /subagents.unmanaged.importSelected/,
      }),
    );
    await waitFor(() =>
      expect(mocks.import).toHaveBeenCalledWith(["helper.md"]),
    );
  });
});

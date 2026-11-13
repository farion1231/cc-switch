import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import SubagentsSection from "@/components/subagents/SubagentsSection";
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
  fileName: "code-reviewer",
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
  fileName: "tester",
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

const onProviderSubmit = vi.fn((event: { preventDefault: () => void }) =>
  event.preventDefault(),
);

/**
 * 区块实际挂在供应商编辑页的 <form> 里，测试也包一层 form：区块里任何按钮都不该提交它。
 * 默认折叠，`expand` 为 true 时先点开标题。
 */
async function renderSection(
  props: { subagentModelOverride?: string } = {},
  { expand = true }: { expand?: boolean } = {},
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const utils = render(
    <QueryClientProvider client={client}>
      <form onSubmit={onProviderSubmit}>
        <SubagentsSection {...props} />
      </form>
    </QueryClientProvider>,
  );
  if (expand) {
    await userEvent.click(
      screen.getByRole("button", { name: /subagents\.section\.title/ }),
    );
  }
  return utils;
}

describe("SubagentsSection", () => {
  beforeEach(() => {
    mocks.list.mockResolvedValue([reviewer, tester]);
    mocks.getDir.mockResolvedValue({
      path: "/home/u/.claude/agents",
      displayPath: "~/.claude/agents",
      exists: true,
    });
    mocks.scanUnmanaged.mockResolvedValue(unmanaged);
    mocks.listPlugin.mockResolvedValue([]);
    onProviderSubmit.mockClear();
  });

  it("shows model, tools summary and drift status for managed rows", async () => {
    await renderSection();
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
    await renderSection();

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
    await renderSection();

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
    await renderSection();

    await screen.findByTestId("subagent-row-a1");
    await user.click(
      screen.getByRole("button", { name: /^subagents\.import/ }),
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
  it("reads model and tools from /agents files that strict YAML rejects", async () => {
    // Claude Code /agents 生成的 description 没加引号、带 `Context:`
    const content =
      "---\nname: api-designer\ndescription: Use this agent when designing APIs. Examples: <example>Context: adding a resource</example>\ntools: Read, Grep\nmodel: sonnet\n---\n\nYou design APIs.\n";
    mocks.list.mockResolvedValue([
      { ...tester, id: "a4", name: "api-designer", enabled: true, content },
    ]);
    mocks.scanUnmanaged.mockResolvedValue([
      {
        fileName: "api-designer-2.md",
        path: "/home/u/.claude/agents/api-designer-2.md",
        name: "api-designer-2",
        content,
      },
    ]);
    const user = userEvent.setup();
    await renderSection();

    const row = await screen.findByTestId("subagent-row-a4");
    expect(within(row).getByText("sonnet")).toBeInTheDocument();
    expect(within(row).getByTitle("Read, Grep")).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: /^subagents\.import/ }),
    );
    const item = (
      await screen.findByRole("checkbox", {
        name: "subagents.unmanaged.selectAria:api-designer-2",
      })
    ).closest("li");
    expect(item).not.toBeNull();
    expect(within(item!).getByText("sonnet")).toBeInTheDocument();
  });

  it("shows the count and pending issues while collapsed", async () => {
    await renderSection({}, { expand: false });
    const trigger = screen.getByRole("button", {
      name: /subagents\.section\.title/,
    });
    await waitFor(() =>
      expect(trigger).toHaveTextContent("subagents.section.needsAttention"),
    );
    expect(trigger).toHaveTextContent("subagents.section.summary");
    expect(screen.queryByTestId("subagent-row-a1")).not.toBeInTheDocument();
  });

  it("warns when this provider's subagent model overrides agent files", async () => {
    await renderSection({ subagentModelOverride: "glm-4.7-air" });
    await screen.findByTestId("subagent-row-a1");
    expect(
      screen.getByText("subagents.section.overrideWarning"),
    ).toBeInTheDocument();
  });

  it("never submits or dirties the surrounding provider form", async () => {
    const user = userEvent.setup();
    mocks.toggle.mockResolvedValue({ ...tester, enabled: true });
    await renderSection();

    const row = await screen.findByTestId("subagent-row-a2");
    const toggle = within(row).getByRole("switch", {
      name: "subagents.actions.enableAria:tester",
    });
    // 编辑页的未保存跟踪按 DOM closest 豁免：区块里的即时写盘操作不算供应商表单的改动
    expect(toggle.closest("[data-unsaved-ignore]")).not.toBeNull();
    await user.click(toggle);
    await waitFor(() => expect(mocks.toggle).toHaveBeenCalled());

    await user.click(
      screen.getByRole("button", { name: /^subagents\.import/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "subagents.section.backToList" }),
    );
    await user.click(screen.getByRole("button", { name: /subagents\.new/ }));

    expect(onProviderSubmit).not.toHaveBeenCalled();
  });
});

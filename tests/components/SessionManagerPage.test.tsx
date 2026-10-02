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
import { SessionManagerPage } from "@/components/sessions/SessionManagerPage";
import { piApi } from "@/lib/api/pi";
import { sessionsApi } from "@/lib/api/sessions";
import type { SessionMessage, SessionMeta } from "@/types";
import { setSessionFixtures, setSettings } from "../msw/state";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const platform = vi.hoisted(() => ({ mac: false }));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/lib/platform", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/platform")>();
  return { ...actual, isMac: () => platform.mac };
});

// jsdom 没有布局，虚拟列表一条都不渲染；这里让它把全部条目都画出来
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 100,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        index,
        key: index,
        start: index * 100,
      })),
    measureElement: () => undefined,
    scrollToIndex: () => undefined,
  }),
}));

const HOUR = 3600 * 1000;

const renderPage = (
  appId = "codex",
  props: Partial<Parameters<typeof SessionManagerPage>[0]> = {},
) => {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return {
    client,
    ...render(
      <QueryClientProvider client={client}>
        <SessionManagerPage appId={appId} {...props} />
      </QueryClientProvider>,
    ),
  };
};

const sessionList = () => screen.getByRole("region", { name: "会话列表" });

const openRow = (title: string) =>
  fireEvent.click(within(sessionList()).getByRole("button", { name: title }));

const openAppMenu = async () =>
  userEvent.click(screen.getByRole("button", { name: /^应用：/ }));

describe("SessionManagerPage", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    platform.mac = false;
    window.localStorage.clear();
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    });

    const now = Date.now();
    const sessions: SessionMeta[] = [
      {
        providerId: "codex",
        sessionId: "codex-session-1",
        title: "Alpha Session",
        summary: "Alpha summary",
        projectDir: "/mock/codex",
        createdAt: now - 3 * HOUR,
        lastActiveAt: now - HOUR,
        sourcePath: "/mock/codex/session-1.jsonl",
        resumeCommand: "codex resume codex-session-1",
      },
      {
        providerId: "codex",
        sessionId: "codex-session-2",
        title: "Beta Session",
        summary: "Beta summary",
        projectDir: "/mock/codex",
        createdAt: now - 5 * HOUR,
        lastActiveAt: now - 2 * HOUR,
        sourcePath: "/mock/codex/session-2.jsonl",
        resumeCommand: "codex resume codex-session-2",
      },
      {
        providerId: "claude",
        sessionId: "claude-session-1",
        title: "Claude Session",
        summary: "Claude summary",
        projectDir: "/mock/claude",
        createdAt: now - 2 * HOUR,
        lastActiveAt: now - 30 * 60 * 1000,
        sourcePath: "/mock/claude/session-1.jsonl",
        resumeCommand: "claude --resume claude-session-1",
      },
      {
        providerId: "codex",
        sessionId: "codex-session-3",
        title: "Gamma Session",
        summary: "Gamma summary",
        projectDir: null,
        createdAt: now - 30 * 24 * HOUR,
        lastActiveAt: now - 30 * 24 * HOUR,
        sourcePath: "/mock/codex/archived_sessions/session-3.jsonl",
        resumeCommand: "codex resume codex-session-3",
      },
      {
        providerId: "hermes",
        sessionId: "hermes-1",
        title: "Hermes Session",
        projectDir: null,
        createdAt: now - 4 * HOUR,
        lastActiveAt: now - 4 * HOUR,
        sourcePath: "/mock/hermes/state.db",
      },
    ];
    const messages: Record<string, SessionMessage[]> = {
      "codex:/mock/codex/session-1.jsonl": [
        { role: "user", content: "alpha question", ts: now - 3 * HOUR },
        { role: "assistant", content: "[Tool: shell]" },
        { role: "tool", content: "output line" },
        {
          role: "assistant",
          content: "Here:\n```ts\nconst a = 1;\n```",
          ts: now - HOUR,
        },
      ],
      "codex:/mock/codex/session-2.jsonl": [
        { role: "user", content: "beta", ts: now - 2 * HOUR },
      ],
      "codex:/mock/codex/archived_sessions/session-3.jsonl": [
        { role: "user", content: "gamma", ts: now - 30 * 24 * HOUR },
      ],
      "claude:/mock/claude/session-1.jsonl": [
        { role: "user", content: "claude", ts: now },
      ],
      "hermes:/mock/hermes/state.db": [],
    };

    setSessionFixtures(sessions, messages);
  });

  it("surfaces a relative Pi sessionDir instead of presenting an empty scan as authoritative", async () => {
    const discovery = vi.spyOn(piApi, "getSessionDiscovery").mockResolvedValue({
      status: "requires_project_context",
      configuredPath: ".pi/sessions",
    });

    renderPage("pi");

    expect(await screen.findByText(".pi/sessions")).toBeInTheDocument();
    expect(discovery).toHaveBeenCalledTimes(1);
    discovery.mockRestore();
  });

  it("lists the current app's sessions grouped by project with unknown last", async () => {
    renderPage("codex");

    await screen.findByRole("button", { name: "Alpha Session" });
    const headings = within(sessionList())
      .getAllByRole("heading", { level: 3 })
      .map((heading) => heading.textContent);
    expect(headings[0]).toContain("codex");
    expect(headings[1]).toContain("未知目录");
    expect(screen.queryByText("Claude Session")).not.toBeInTheDocument();
    // 归档的 Codex 会话带「已归档」
    expect(screen.getByText("已归档")).toBeInTheDocument();
    // 应用按钮写全称和数量
    expect(screen.getByRole("button", { name: /^应用：/ })).toHaveTextContent(
      "Codex3",
    );
  });

  it("collapses a project group and remembers it", async () => {
    renderPage("codex");

    const toggle = await screen.findByRole("button", { name: /^codex/ });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Alpha Session")).not.toBeInTheDocument();
    expect(
      window.localStorage.getItem("cc-switch.sessionManager.collapsedProjects"),
    ).toContain("/mock/codex");
  });

  it("switches to all apps and to time buckets", async () => {
    renderPage("codex");
    await screen.findByText("Alpha Session");

    await openAppMenu();
    const menu = await screen.findByRole("menu");
    const items = within(menu).getAllByRole("menuitemradio");
    // 全部应用 + 9 个来源，按会话数从多到少
    expect(items).toHaveLength(10);
    expect(items[0]).toHaveTextContent("全部应用5");
    expect(items[1]).toHaveTextContent("Codex3");
    expect(items[items.length - 1]).toHaveTextContent("无会话");
    await userEvent.click(items[0]);

    expect(await screen.findByText("Claude Session")).toBeInTheDocument();
    expect(screen.getByText("Codex 2")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "按时间" }));
    expect(
      within(sessionList()).getByRole("heading", { name: "今天" }),
    ).toBeInTheDocument();
    expect(
      within(sessionList()).getByRole("heading", { name: "更早" }),
    ).toBeInTheDocument();
  });

  it("only offers apps that are shown in the sidebar", async () => {
    setSettings({
      visibleApps: {
        claude: true,
        "claude-desktop": true,
        codex: true,
        gemini: false,
        grokbuild: false,
        opencode: false,
        openclaw: false,
        hermes: false,
        pi: false,
        mcode: false,
      },
    });
    renderPage("codex");
    await screen.findByText("Alpha Session");
    await waitFor(async () => {
      await openAppMenu();
      const menu = await screen.findByRole("menu");
      expect(within(menu).getAllByRole("menuitemradio")).toHaveLength(3);
    });
    const menu = screen.getByRole("menu");
    expect(within(menu).queryByText("Hermes")).not.toBeInTheDocument();
    // 隐藏应用的会话也不计入「全部应用」
    expect(within(menu).getAllByRole("menuitemradio")[0]).toHaveTextContent(
      "全部应用4",
    );
  });

  it("shows the Claude Desktop note when coming from Claude Desktop", async () => {
    renderPage("claude", { fromApp: "claude-desktop" });
    expect(
      await screen.findByText(
        "Claude Desktop 没有单独的会话记录，这里显示 Claude Code 的会话。",
      ),
    ).toBeInTheDocument();
  });

  it("filters by search and offers to clear an empty search", async () => {
    renderPage("codex");
    await screen.findByText("Alpha Session");

    const search = screen.getByRole("textbox", { name: "搜索会话" });
    fireEvent.change(search, { target: { value: "Beta" } });
    await waitFor(() =>
      expect(screen.queryByText("Alpha Session")).not.toBeInTheDocument(),
    );
    expect(screen.getByText("Beta")).toBeInTheDocument();

    fireEvent.change(search, { target: { value: "zzz" } });
    expect(
      await screen.findByText("没有标题、目录或首末消息匹配“zzz”的会话"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "清除搜索" }));
    expect(await screen.findByText("Alpha Session")).toBeInTheDocument();
  });

  it("drills into a session, merges tool calls and steps between sessions", async () => {
    renderPage("codex");
    await screen.findByText("Alpha Session");

    openRow("Alpha Session");

    expect(
      await screen.findByRole("heading", { level: 2, name: "Alpha Session" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("alpha question")).toBeInTheDocument();
    expect(screen.getByText("shell")).toBeInTheDocument();
    expect(screen.getByText("const a = 1;")).toBeInTheDocument();
    // 列表藏起来了
    expect(screen.queryByRole("region", { name: "会话列表" })).toBeNull();

    // 只看对话：工具调用隐藏
    fireEvent.click(screen.getByRole("switch"));
    expect(screen.queryByText("shell")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "下一个会话" }));
    expect(
      await screen.findByRole("heading", { level: 2, name: "Beta Session" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "上一个会话" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "返回会话列表" }));
    expect(await screen.findByText("Alpha Session")).toBeInTheDocument();
  });

  it("explains why a Hermes session cannot be resumed", async () => {
    renderPage("hermes");
    await screen.findByText("Hermes Session");
    openRow("Hermes Session");

    expect(
      await screen.findByText("暂不支持从命令行恢复 Hermes 会话"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "复制恢复命令" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByText("Hermes 只显示最近 500 个会话"),
    ).toBeInTheDocument();
  });

  it("shows the read error instead of an empty session", async () => {
    const spy = vi
      .spyOn(sessionsApi, "getMessages")
      .mockRejectedValue(new Error("no such column: created_at"));
    renderPage("codex");
    await screen.findByText("Alpha Session");
    openRow("Alpha Session");

    expect(await screen.findByText("无法读取这个会话")).toBeInTheDocument();
    expect(screen.getByText("no such column: created_at")).toBeInTheDocument();
    spy.mockRestore();
  });

  it("launches the resume command in the preferred terminal on macOS", async () => {
    platform.mac = true;
    setSettings({ preferredTerminal: "iterm2" });
    const launch = vi
      .spyOn(sessionsApi, "launchTerminal")
      .mockResolvedValue(true);
    renderPage("codex");
    await screen.findByText("Alpha Session");
    openRow("Alpha Session");

    const resume = await screen.findByRole("button", {
      name: /中恢复$/,
    });
    fireEvent.click(resume);

    await waitFor(() =>
      expect(launch).toHaveBeenCalledWith({
        command: "codex resume codex-session-1",
        cwd: "/mock/codex",
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalled();
    launch.mockRestore();
  });

  it("copies the resume command where one-click resume is unavailable", async () => {
    renderPage("codex");
    await screen.findByText("Alpha Session");
    openRow("Alpha Session");

    fireEvent.click(
      await screen.findByRole("button", { name: "复制恢复命令" }),
    );
    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
        "codex resume codex-session-1",
      ),
    );
  });

  it("deletes the open session after confirming and returns to the list", async () => {
    renderPage("codex");
    await screen.findByText("Alpha Session");
    openRow("Alpha Session");
    await screen.findByRole("heading", { level: 2, name: "Alpha Session" });

    await userEvent.click(
      screen.getByRole("button", { name: "Alpha Session 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "删除…" }),
    );

    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("删除会话「Alpha Session」？");
    expect(dialog).toHaveTextContent("只删除这一个会话文件");
    fireEvent.click(within(dialog).getByRole("button", { name: "删除会话" }));

    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { level: 2, name: "Alpha Session" }),
      ).not.toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(screen.queryByText("Alpha Session")).not.toBeInTheDocument(),
    );
    expect(screen.getByText("Beta Session")).toBeInTheDocument();
  });

  it("selects several sessions and deletes them in one batch", async () => {
    const deleteMany = vi.spyOn(sessionsApi, "deleteMany");
    renderPage("codex");
    await screen.findByText("Alpha Session");

    await userEvent.click(
      screen.getByRole("button", { name: "会话的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "选择多个…" }),
    );

    fireEvent.click(
      screen.getByRole("checkbox", { name: "选择 Alpha Session" }),
    );
    fireEvent.click(
      screen.getByRole("checkbox", { name: "选择 Beta Session" }),
    );
    expect(screen.getByText("已选 2 个会话")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "删除 2 个会话…" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("删除 2 个会话？");
    expect(dialog).toHaveTextContent("Codex 2 个");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "删除 2 个会话" }),
    );

    await waitFor(() => expect(deleteMany).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.queryByText("Alpha Session")).not.toBeInTheDocument(),
    );
    expect(screen.queryByText("Beta Session")).not.toBeInTheDocument();
    expect(screen.getByText("Gamma Session")).toBeInTheDocument();
    // 删完退出选择
    expect(screen.queryByText(/已选/)).not.toBeInTheDocument();
    deleteMany.mockRestore();
  });

  it("keeps the selection when the batch delete request fails", async () => {
    const deleteMany = vi
      .spyOn(sessionsApi, "deleteMany")
      .mockRejectedValue(new Error("boom"));
    renderPage("codex");
    await screen.findByText("Alpha Session");

    await userEvent.click(
      screen.getByRole("button", { name: "会话的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "选择多个…" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "全选匹配结果" }));
    expect(screen.getByText("已选 3 个会话")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "删除 3 个会话…" }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "删除 3 个会话",
      }),
    );

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(screen.getByText("已选 3 个会话")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "删除 3 个会话…" }),
    ).toBeEnabled();
    deleteMany.mockRestore();
  });

  it("drops hidden selections from the count and keeps search usable while selecting", async () => {
    renderPage("codex");
    await screen.findByText("Alpha Session");

    // 行首的复选框直接进入选择态
    fireEvent.click(
      screen.getAllByRole("checkbox", { name: "选择 Alpha Session" })[0],
    );
    expect(screen.getByText("已选 1 个会话")).toBeInTheDocument();

    fireEvent.change(screen.getByRole("textbox", { name: "搜索会话" }), {
      target: { value: "Beta" },
    });
    expect(await screen.findByText("匹配 1 个会话")).toBeInTheDocument();
    expect(screen.getByText(/其中 1 个不在当前结果里/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "退出选择" }));
    expect(screen.queryByText(/已选/)).not.toBeInTheDocument();
  });
});

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";
import type { McpApps, McpServer, McpServerSpec } from "@/types";

const mocks = vi.hoisted(() => ({
  serversMap: {} as Record<string, McpServer>,
  isLoading: false,
  isError: false,
  toggle: vi.fn(),
  bulkToggle: vi.fn(),
  deleteServer: vi.fn(),
  importServers: vi.fn(),
  resync: vi.fn(),
  refetch: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
  toastWarning: vi.fn(),
  visibleApps: ["claude", "codex", "gemini"] as string[],
}));

vi.mock("@/hooks/useMcp", () => ({
  useAllMcpServers: () => ({
    data: mocks.serversMap,
    isLoading: mocks.isLoading,
    isError: mocks.isError,
    error: mocks.isError ? new Error("database is locked") : null,
    refetch: mocks.refetch,
  }),
  useToggleMcpApp: () => ({ mutateAsync: mocks.toggle, isPending: false }),
  useBulkToggleMcpApp: () => ({
    mutateAsync: mocks.bulkToggle,
    isPending: false,
  }),
  useDeleteMcpServer: () => ({
    mutateAsync: mocks.deleteServer,
    isPending: false,
  }),
  useImportMcpFromApps: () => ({
    mutateAsync: mocks.importServers,
    isPending: false,
  }),
  useResyncMcpToApps: () => ({
    mutateAsync: mocks.resync,
    isPending: false,
  }),
}));

vi.mock("@/components/mcp/useVisibleAppIds", () => ({
  useVisibleAppIds: (ids: string[]) =>
    ids.filter((id) => mocks.visibleApps.includes(id)),
}));

vi.mock("@/components/mcp/McpFormModal", () => ({
  default: ({ editingId }: { editingId?: string }) => (
    <div data-testid="mcp-drawer">{editingId ?? "add"}</div>
  ),
}));

vi.mock("sonner", () => ({
  toast: {
    error: mocks.toastError,
    success: mocks.toastSuccess,
    warning: mocks.toastWarning,
    info: vi.fn(),
  },
}));

type ServerOverrides = Partial<Omit<McpServer, "apps" | "server">> & {
  apps?: Partial<McpApps>;
  server?: Partial<McpServerSpec>;
};

function makeServer(id: string, overrides: ServerOverrides = {}): McpServer {
  const { apps, server, ...metadata } = overrides;
  return {
    id,
    name: id,
    ...metadata,
    server: { type: "stdio", command: "default-command", ...server },
    apps: {
      claude: false,
      codex: false,
      gemini: false,
      grokbuild: false,
      opencode: false,
      openclaw: false,
      hermes: false,
      ...apps,
    },
  } as McpServer;
}

const renderPanel = (onBlocked?: (blocked: boolean) => void) =>
  render(<UnifiedMcpPanel onInteractionBlockedChange={onBlocked} />);

const rowNames = () =>
  within(screen.getByRole("list", { name: "mcpPage.listLabel" }))
    .getAllByRole("listitem")
    .map((item) => item.querySelector("span")?.textContent);

describe("UnifiedMcpPanel", () => {
  beforeEach(() => {
    mocks.serversMap = {};
    mocks.isLoading = false;
    mocks.isError = false;
    mocks.visibleApps = ["claude", "codex", "gemini"];
    mocks.toggle.mockReset().mockResolvedValue(true);
    mocks.bulkToggle.mockReset().mockImplementation(async ({ serverIds }) => ({
      succeeded: serverIds,
      failed: [],
    }));
    mocks.deleteServer.mockReset().mockResolvedValue(true);
    mocks.importServers.mockReset().mockResolvedValue(0);
    mocks.resync.mockReset().mockImplementation(async (apps?: string[]) =>
      (apps ?? ["claude", "codex", "gemini"]).map((app) => ({
        app,
        ok: true,
      })),
    );
    mocks.toastWarning.mockReset();
    mocks.refetch.mockReset().mockResolvedValue({ data: mocks.serversMap });
    mocks.toastError.mockReset();
    mocks.toastSuccess.mockReset();
  });

  it("searches the allow-listed fields but never env or header values", async () => {
    mocks.serversMap = {
      alpha: makeServer("alpha", {
        server: { command: "uvx", args: ["fetch-tool"] },
      }),
      beta: makeServer("beta", {
        server: {
          type: "http",
          url: "https://beta.example.com/mcp",
          headers: { Authorization: "secret-token" },
          env: { API_KEY: "hidden-value" },
        },
      }),
    };
    renderPanel();
    expect(rowNames()).toEqual(["alpha", "beta"]);
    // 第二行显示命令 / 去掉协议的 URL，不显示请求头
    expect(screen.getByText("uvx fetch-tool")).toBeInTheDocument();
    expect(screen.getByText("beta.example.com/mcp")).toBeInTheDocument();
    expect(screen.queryByText(/secret-token/)).not.toBeInTheDocument();

    const search = screen.getByRole("textbox", {
      name: "mcp.unifiedPanel.searchAriaLabel",
    });
    await userEvent.type(search, "fetch-tool");
    expect(rowNames()).toEqual(["alpha"]);
    await userEvent.clear(search);
    await userEvent.type(search, "secret-token");
    expect(screen.getByText("mcpPage.noMatch")).toBeInTheDocument();
    await userEvent.clear(search);
    await userEvent.type(search, "hidden-value");
    expect(screen.getByText("mcpPage.noMatch")).toBeInTheDocument();
  });

  it("shows the empty state, not a search miss, when there are no servers", () => {
    renderPanel();
    expect(screen.getByText("mcpPage.emptyTitle")).toBeInTheDocument();
    expect(screen.queryByText("mcpPage.noMatch")).not.toBeInTheDocument();
  });

  it("shows the load error instead of the empty state", () => {
    mocks.isError = true;
    renderPanel();
    expect(screen.getByText("mcpPage.loadFailed")).toBeInTheDocument();
    expect(screen.getByText("database is locked")).toBeInTheDocument();
    expect(screen.queryByText("mcpPage.emptyTitle")).not.toBeInTheDocument();
  });

  it("only renders columns for apps shown on the Apps page", () => {
    mocks.serversMap = { alpha: makeServer("alpha") };
    renderPanel();
    const header = screen.getByTestId("mcp-matrix");
    expect(
      within(header).getAllByRole("button", { name: /appMatrix.columnAria/ }),
    ).toHaveLength(3);
  });

  it("highlights the column header when a cell gets keyboard focus", async () => {
    mocks.serversMap = { alpha: makeServer("alpha") };
    renderPanel();
    const columns = screen.getAllByRole("button", {
      name: /appMatrix.columnAria/,
    });
    const cell = screen.getAllByRole("button", { name: /appMatrix.cell/ })[1];
    act(() => cell.focus());
    expect(columns[1]).toHaveAttribute("data-highlighted");
    expect(screen.getByTestId("matrix-column-name")).toHaveTextContent("Codex");
    act(() => cell.blur());
    expect(columns[1]).not.toHaveAttribute("data-highlighted");
  });

  it("bulk-enables only the rows left by the search and offers undo", async () => {
    mocks.serversMap = {
      "alpha-one": makeServer("alpha-one"),
      "alpha-two": makeServer("alpha-two", { apps: { codex: true } }),
      beta: makeServer("beta"),
    };
    renderPanel();
    await userEvent.type(
      screen.getByRole("textbox", { name: "mcp.unifiedPanel.searchAriaLabel" }),
      "alpha",
    );
    const codexColumn = screen.getAllByRole("button", {
      name: /appMatrix.columnAria/,
    })[1];
    await userEvent.click(codexColumn);
    expect(screen.getByText("appMatrix.pop.scopeSearch")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "appMatrix.pop.enableRest" }),
    );

    await waitFor(() => expect(mocks.bulkToggle).toHaveBeenCalledTimes(1));
    expect(mocks.bulkToggle).toHaveBeenCalledWith({
      serverIds: ["alpha-one"],
      app: "codex",
      enabled: true,
    });
    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
    const [, options] = mocks.toastSuccess.mock.calls[0];
    expect(options.action.label).toBe("appMatrix.undo");

    options.action.onClick();
    await waitFor(() => expect(mocks.bulkToggle).toHaveBeenCalledTimes(2));
    expect(mocks.bulkToggle).toHaveBeenLastCalledWith({
      serverIds: ["alpha-one"],
      app: "codex",
      enabled: false,
    });
  });

  it("marks a failed write with a warning and retries the wanted state", async () => {
    mocks.serversMap = { serena: makeServer("serena") };
    mocks.toggle.mockRejectedValueOnce(new Error("config.toml line 12"));
    renderPanel();

    await userEvent.click(
      screen.getAllByRole("button", { name: "appMatrix.cell.off" })[1],
    );
    await waitFor(() =>
      expect(screen.getByText("mcpPage.failNoticeTitle")).toBeInTheDocument(),
    );
    expect(mocks.toggle).toHaveBeenCalledWith({
      serverId: "serena",
      app: "codex",
      enabled: true,
    });
    const failCell = screen.getByRole("button", {
      name: "appMatrix.cell.fail",
    });

    await userEvent.click(failCell);
    await waitFor(() => expect(mocks.toggle).toHaveBeenCalledTimes(2));
    expect(mocks.toggle).toHaveBeenLastCalledWith({
      serverId: "serena",
      app: "codex",
      enabled: true,
    });
    await waitFor(() =>
      expect(
        screen.queryByText("mcpPage.failNoticeTitle"),
      ).not.toBeInTheDocument(),
    );
  });

  it("retries a failed app from the notice by resyncing that app", async () => {
    mocks.serversMap = { serena: makeServer("serena") };
    mocks.toggle.mockRejectedValueOnce(new Error("config.toml line 12"));
    renderPanel();

    await userEvent.click(
      screen.getAllByRole("button", { name: "appMatrix.cell.off" })[1],
    );
    await waitFor(() =>
      expect(screen.getByText("mcpPage.failNoticeTitle")).toBeInTheDocument(),
    );

    await userEvent.click(screen.getByRole("button", { name: "common.retry" }));

    await waitFor(() => expect(mocks.resync).toHaveBeenCalledWith(["codex"]));
    expect(mocks.toggle).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(
        screen.queryByText("mcpPage.failNoticeTitle"),
      ).not.toBeInTheDocument(),
    );
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "mcpPage.toast.written",
      expect.anything(),
    );
  });

  it("keeps the notice when the resync retry fails again", async () => {
    mocks.serversMap = { serena: makeServer("serena") };
    mocks.toggle.mockRejectedValueOnce(new Error("config.toml line 12"));
    mocks.resync.mockResolvedValueOnce([
      { app: "codex", ok: false, error: "still broken" },
    ]);
    renderPanel();

    await userEvent.click(
      screen.getAllByRole("button", { name: "appMatrix.cell.off" })[1],
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "common.retry" }),
    );

    await waitFor(() => expect(mocks.resync).toHaveBeenCalledTimes(1));
    expect(screen.getByText("mcpPage.failNoticeTitle")).toBeInTheDocument();
    expect(mocks.toastSuccess).not.toHaveBeenCalled();
  });

  it("resyncs every app from the more menu and reports the ones that failed", async () => {
    mocks.serversMap = {
      serena: makeServer("serena", { apps: { codex: true } }),
    };
    mocks.resync.mockResolvedValueOnce([
      { app: "claude", ok: true },
      { app: "codex", ok: false, error: "config.toml line 12" },
    ]);
    renderPanel();

    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "mcpPage.resync" }),
    );

    await waitFor(() => expect(mocks.resync).toHaveBeenCalledWith(undefined));
    await waitFor(() =>
      expect(screen.getByText("mcpPage.failNoticeTitle")).toBeInTheDocument(),
    );
    expect(mocks.toastWarning).toHaveBeenCalledWith(
      "mcpPage.toast.resyncPartial",
      expect.anything(),
    );

    // 再来一次全部成功：通知条消失
    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "mcpPage.resync" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByText("mcpPage.failNoticeTitle"),
      ).not.toBeInTheDocument(),
    );
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "mcpPage.toast.resynced",
      expect.anything(),
    );
  });

  it("opens the edit drawer from the pencil button", async () => {
    mocks.serversMap = {
      serena: makeServer("serena", { apps: { claude: true, hermes: true } }),
    };
    renderPanel();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.editAria" }),
    );
    expect(screen.getByTestId("mcp-drawer")).toHaveTextContent("serena");
  });

  it("deletes after confirming in the dialog", async () => {
    mocks.serversMap = {
      serena: makeServer("serena", { apps: { claude: true } }),
    };
    renderPanel();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.rowMoreAria" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "mcpPage.deleteEllipsis" }),
    );
    expect(screen.getByText("mcpPage.deleteBodyApps")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "common.delete" }),
    );
    await waitFor(() =>
      expect(mocks.deleteServer).toHaveBeenCalledWith("serena"),
    );
  });

  it("reports which servers an import added", async () => {
    mocks.serversMap = {};
    mocks.refetch.mockResolvedValue({
      data: {
        context7: makeServer("context7"),
        fetch: makeServer("fetch"),
      },
    });
    renderPanel();
    await userEvent.click(
      screen.getAllByRole("button", { name: "mcpPage.importFromApps" })[0],
    );
    await waitFor(() =>
      expect(screen.getByText("mcpPage.import.title")).toBeInTheDocument(),
    );
    expect(mocks.importServers).toHaveBeenCalled();
    expect(screen.getByText("mcpPage.import.added")).toBeInTheDocument();
  });

  it("reports the blocked state while the drawer is open", async () => {
    const onBlocked = vi.fn();
    mocks.serversMap = { alpha: makeServer("alpha") };
    renderPanel(onBlocked);
    expect(onBlocked).toHaveBeenLastCalledWith(false);
    await userEvent.click(screen.getByRole("button", { name: "mcpPage.add" }));
    expect(onBlocked).toHaveBeenLastCalledWith(true);
  });
});

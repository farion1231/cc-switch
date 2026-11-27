import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { PluginEntry } from "@/lib/api/plugins";

const m = vi.hoisted(() => ({
  listInstalled: vi.fn(),
  listCatalog: vi.fn(),
  listMarketplaces: vi.fn(),
  install: vi.fn(),
  uninstall: vi.fn(),
  setEnabled: vi.fn(),
  update: vi.fn(),
  addMarketplace: vi.fn(),
  removeMarketplace: vi.fn(),
  refreshMarketplaces: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: m.toastSuccess,
    error: m.toastError,
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/lib/api/plugins", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/plugins")>()),
  pluginsApi: {
    listInstalled: m.listInstalled,
    listCatalog: m.listCatalog,
    listMarketplaces: m.listMarketplaces,
    install: m.install,
    uninstall: m.uninstall,
    setEnabled: m.setEnabled,
    update: m.update,
    addMarketplace: m.addMarketplace,
    removeMarketplace: m.removeMarketplace,
    refreshMarketplaces: m.refreshMarketplaces,
  },
}));

import {
  DEFAULT_PLUGIN_PAGE_SIZE,
  PluginsPage,
  availableFromCatalog,
  filterPlugins,
  pluginRowKeys,
  withCatalogDetails,
} from "@/components/plugins/PluginsPage";
import { resetPluginWriteState, samePluginEntry } from "@/hooks/usePlugins";

const plugin = (over: Partial<PluginEntry> = {}): PluginEntry => ({
  id: "frontend-design@official",
  name: "frontend-design",
  marketplace: "official",
  version: "1.1.1",
  description: "Frontend design skill",
  installed: true,
  enabled: true,
  scope: "user",
  manageable: true,
  installable: true,
  ...over,
});

const catalogItem = (over: Partial<PluginEntry> = {}) =>
  plugin({ installed: false, enabled: false, scope: null, ...over });

const superpowers = catalogItem({
  id: "superpowers@official",
  name: "superpowers",
  description: "Core skills library",
});

/** 手动 resolve 的 Promise：看请求还没回来时界面是什么样 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderPage(props: React.ComponentProps<typeof PluginsPage> = {}) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <PluginsPage {...props} />
    </QueryClientProvider>,
  );
}

/** 只数 Claude Code 的已安装查询（另一个应用会被预取） */
const claudeListCalls = () =>
  m.listInstalled.mock.calls.filter(([app]) => app === "claude").length;

const enableSwitch = () =>
  screen.findByRole("switch", { name: "pluginsPage.enableAria" });

describe("plugin list helpers", () => {
  it("filters by id, name, display name, description and marketplace", () => {
    const entries = [
      plugin(),
      plugin({
        id: "x@other",
        name: "x",
        marketplace: "other",
        description: "",
      }),
      plugin({
        id: "y@m",
        name: "y",
        displayName: "Game Studio",
        description: null,
      }),
      plugin({
        id: "git:github.com/org/repo",
        name: "@scope/handy-tools",
        marketplace: "",
        description: null,
      }),
    ];
    expect(filterPlugins(entries, "  ")).toHaveLength(4);
    expect(filterPlugins(entries, "FRONTEND").map((e) => e.id)).toEqual([
      "frontend-design@official",
    ]);
    expect(filterPlugins(entries, "other").map((e) => e.id)).toEqual([
      "x@other",
    ]);
    expect(filterPlugins(entries, "studio").map((e) => e.id)).toEqual(["y@m"]);
    expect(filterPlugins(entries, "handy-tools").map((e) => e.id)).toEqual([
      "git:github.com/org/repo",
    ]);
  });

  it("derives Discover from the catalog and fills missing details", () => {
    const installed = [plugin({ description: null })];
    const catalog = [
      catalogItem({ description: "From catalog", displayName: "FD" }),
      superpowers,
    ];
    expect(availableFromCatalog(catalog, installed).map((e) => e.id)).toEqual([
      "superpowers@official",
    ]);
    const [filled] = withCatalogDetails(installed, catalog);
    expect(filled.description).toBe("From catalog");
    expect(filled.displayName).toBe("FD");
    expect(filled.enabled).toBe(true);
  });

  it("matches catalog packages to the id they get once installed", () => {
    // OpenClaw：目录给 ClawHub 包名，装好后的插件 id 是最后一段
    const installed = [
      plugin({
        id: "memory-lancedb",
        name: "memory-lancedb",
        marketplace: "",
        description: null,
      }),
    ];
    const catalog = [
      catalogItem({
        id: "@openclaw/memory-lancedb",
        name: "@openclaw/memory-lancedb",
        installedId: "memory-lancedb",
        description: "LanceDB memory",
        marketplace: "clawhub",
      }),
      catalogItem({
        id: "@x/other",
        name: "@x/other",
        installedId: "other",
        marketplace: "clawhub",
      }),
    ];
    expect(availableFromCatalog(catalog, installed).map((e) => e.id)).toEqual([
      "@x/other",
    ]);
    expect(withCatalogDetails(installed, catalog)[0].description).toBe(
      "LanceDB memory",
    );
  });

  it("matches pinned Pi packages via catalogId", () => {
    const installed = [
      plugin({
        id: "npm:pi-pinned@1.0.0",
        name: "pi-pinned",
        catalogId: "npm:pi-pinned",
        marketplace: "",
        description: null,
      }),
    ];
    const catalog = [
      catalogItem({
        id: "npm:pi-pinned",
        name: "pi-pinned",
        marketplace: "",
        description: "Pinned package",
      }),
      catalogItem({
        id: "npm:pi-other",
        name: "pi-other",
        marketplace: "",
      }),
    ];
    expect(availableFromCatalog(catalog, installed).map((e) => e.id)).toEqual([
      "npm:pi-other",
    ]);
    expect(withCatalogDetails(installed, catalog)[0].description).toBe(
      "Pinned package",
    );
  });

  it("filters non-user entries by projectPath and distinguishes rows by (id, scope, projectPath)", () => {
    const localA = plugin({
      id: "context7@official",
      name: "context7",
      scope: "local",
      projectPath: "/work/proj-a",
      manageable: false,
    });
    const localB = plugin({
      id: "context7@official",
      name: "context7",
      scope: "local",
      projectPath: "/work/proj-b",
      manageable: false,
    });
    const projectA = plugin({
      id: "context7@official",
      name: "context7",
      scope: "project",
      projectPath: "/work/proj-a",
      manageable: false,
    });
    const userRow = plugin({
      id: "context7@official",
      name: "context7",
      scope: "user",
      projectPath: "/ignored-user-path",
      manageable: true,
    });

    expect(
      filterPlugins([userRow, localA, localB], "PROJ-B").map(
        (e) => e.projectPath,
      ),
    ).toEqual(["/work/proj-b"]);
    expect(filterPlugins([userRow], "ignored-user-path")).toEqual([]);

    expect(samePluginEntry(localA, { ...localA })).toBe(true);
    expect(samePluginEntry(localA, localB)).toBe(false);
    expect(samePluginEntry(localA, projectA)).toBe(false);
    expect(samePluginEntry(userRow, localA)).toBe(false);
    expect(samePluginEntry(userRow, { ...userRow, projectPath: null })).toBe(
      true,
    );
    expect(
      samePluginEntry(
        plugin({ scope: null, projectPath: null }),
        plugin({ scope: undefined, projectPath: "   " }),
      ),
    ).toBe(true);
  });

  it("generates unique, stable row keys across multi-project installs and stale caches", () => {
    const localA = plugin({
      id: "context7@official",
      scope: "local",
      projectPath: "/work/proj-a",
      manageable: false,
    });
    const localB = plugin({
      id: "context7@official",
      scope: "local",
      projectPath: "/work/proj-b",
      manageable: false,
    });
    const projectA = plugin({
      id: "context7@official",
      scope: "project",
      projectPath: "/work/proj-a",
      manageable: false,
    });
    const keys = pluginRowKeys([localA, localB, projectA]);
    expect(new Set(keys).size).toBe(3);

    // 更新版本号或补齐 installPath 后，同一条目的 key 不变；user 范围忽略意外带入的 projectPath
    const beforeReconcile = plugin({
      id: "superpowers@official",
      scope: "user",
      version: undefined,
      installPath: null,
      projectPath: "/ignored-user-path",
    });
    const afterReconcile = plugin({
      id: "superpowers@official",
      scope: "user",
      version: "2.0.0",
      installPath: "/cache/superpowers/2.0.0",
      projectPath: null,
    });
    expect(pluginRowKeys([beforeReconcile])).toEqual(
      pluginRowKeys([afterReconcile]),
    );
    expect(
      pluginRowKeys([plugin({ scope: "local", projectPath: "   " })]),
    ).toEqual(pluginRowKeys([plugin({ scope: "local", projectPath: null })]));

    // 旧缓存里两条相同 (id, scope) 且缺少 projectPath 的条目也必须生成唯一 key
    const staleDup = plugin({
      id: "context7@official",
      scope: "local",
      projectPath: undefined,
      manageable: false,
    });
    const staleKeys = pluginRowKeys([staleDup, { ...staleDup }]);
    expect(new Set(staleKeys).size).toBe(2);

    // 冒号（如 Windows 盘符或 Pi 的 npm: 前缀）不会造成字段边界冲突
    const colonKeys = pluginRowKeys([
      plugin({ id: "a:b", scope: null, projectPath: null }),
      plugin({ id: "a", scope: "b", projectPath: null }),
      plugin({ id: "p@m", scope: "local", projectPath: "C:\\work\\a" }),
      plugin({ id: "p@m", scope: "local", projectPath: "C:\\work\\b" }),
    ]);
    expect(new Set(colonKeys).size).toBe(4);
  });
});

describe("PluginsPage", () => {
  beforeEach(() => {
    resetPluginWriteState();
    m.listInstalled.mockReset().mockResolvedValue([plugin()]);
    m.listCatalog.mockReset().mockResolvedValue([catalogItem(), superpowers]);
    m.listMarketplaces.mockReset().mockResolvedValue([]);
    m.install.mockReset().mockResolvedValue({ changed: true });
    m.uninstall.mockReset().mockResolvedValue({ changed: true });
    m.setEnabled.mockReset().mockResolvedValue({ changed: true });
    m.update.mockReset().mockResolvedValue({ changed: false });
    m.addMarketplace.mockReset().mockResolvedValue({ changed: true });
    m.removeMarketplace.mockReset().mockResolvedValue({ changed: true });
    m.refreshMarketplaces.mockReset().mockResolvedValue({ changed: true });
    m.toastSuccess.mockReset();
    m.toastError.mockReset();
  });

  it("shows the cached installed list before the CLI answers", async () => {
    localStorage.setItem(
      "cc-switch-cache:plugins:claude:installed",
      JSON.stringify([plugin({ name: "cached-plugin", id: "cached@m" })]),
    );
    const fresh = deferred<PluginEntry[]>();
    m.listInstalled.mockReturnValue(fresh.promise);
    renderPage();
    expect(await screen.findByText("cached-plugin")).toBeInTheDocument();
    expect(
      screen.getByRole("status", { name: "pluginsPage.refreshing" }),
    ).toBeInTheDocument();

    await act(async () => fresh.resolve([plugin()]));
    expect(await screen.findByText("frontend-design")).toBeInTheDocument();
    expect(screen.queryByText("cached-plugin")).not.toBeInTheDocument();
    expect(
      JSON.parse(
        localStorage.getItem("cc-switch-cache:plugins:claude:installed")!,
      )[0].id,
    ).toBe("frontend-design@official");
  });

  it("flips the switch immediately and reconciles once afterwards", async () => {
    renderPage();
    const toggle = await enableSwitch();
    await waitFor(() => expect(claudeListCalls()).toBe(1));

    const pending = deferred<{ changed: boolean }>();
    m.setEnabled.mockReturnValue(pending.promise);
    await userEvent.click(toggle);
    // 请求还没回来，开关已经是关的
    expect(toggle).toHaveAttribute("data-state", "unchecked");
    expect(m.setEnabled).toHaveBeenCalledWith(
      "claude",
      "frontend-design@official",
      false,
    );
    expect(claudeListCalls()).toBe(1);

    m.listInstalled.mockResolvedValue([plugin({ enabled: false })]);
    await act(async () => pending.resolve({ changed: true }));
    await waitFor(() => expect(claudeListCalls()).toBe(2));
    expect(toggle).toHaveAttribute("data-state", "unchecked");
    expect(m.toastSuccess).toHaveBeenCalledWith(
      "pluginsPage.toast.disabled",
      expect.anything(),
    );
    // 启停不动目录
    expect(m.listCatalog).toHaveBeenCalledTimes(1);
  });

  it("rolls the switch back when the CLI fails", async () => {
    m.setEnabled.mockRejectedValue("Plugin not found");
    renderPage();
    const toggle = await enableSwitch();
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(m.toastError).toHaveBeenCalledWith(
        "pluginsPage.toast.toggleFailed",
        expect.objectContaining({ description: "Plugin not found" }),
      ),
    );
    expect(toggle).toHaveAttribute("data-state", "checked");
  });

  it("waits for every pending action before reconciling", async () => {
    m.listInstalled.mockResolvedValue([
      plugin(),
      plugin({ id: "b@official", name: "b" }),
    ]);
    renderPage();
    const switches = await screen.findAllByRole("switch", {
      name: "pluginsPage.enableAria",
    });
    await waitFor(() => expect(claudeListCalls()).toBe(1));
    const first = deferred<{ changed: boolean }>();
    const second = deferred<{ changed: boolean }>();
    m.setEnabled
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    await userEvent.click(switches[0]);
    await userEvent.click(switches[1]);
    await act(async () => first.resolve({ changed: true }));
    // 第二个还在跑：不对账，也不把它的乐观状态盖掉
    expect(claudeListCalls()).toBe(1);
    expect(switches[1]).toHaveAttribute("data-state", "unchecked");

    await act(async () => second.resolve({ changed: true }));
    await waitFor(() => expect(claudeListCalls()).toBe(2));
  });

  it("keeps project-scope plugins read-only", async () => {
    m.listInstalled.mockResolvedValue([
      plugin({ scope: "project", manageable: false }),
    ]);
    renderPage();
    expect(await enableSwitch()).toBeDisabled();
    expect(screen.getByText("pluginsPage.scope")).toBeInTheDocument();
  });

  it("shows Installed while the catalog is still loading", async () => {
    const catalog = deferred<PluginEntry[]>();
    m.listCatalog.mockReturnValue(catalog.promise);
    renderPage();
    expect(await screen.findByText("frontend-design")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewDiscover" }),
    );
    expect(screen.getByText("pluginsPage.loadingCatalog")).toBeInTheDocument();
    await act(async () => catalog.resolve([catalogItem(), superpowers]));
    expect(await screen.findByText("superpowers")).toBeInTheDocument();
  });

  it("moves an installed plugin over without re-reading the catalog", async () => {
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    expect(await screen.findByText("superpowers")).toBeInTheDocument();

    // 对账慢：装好后不等它，条目马上换到已安装
    const reconcile = deferred<PluginEntry[]>();
    m.listInstalled.mockReturnValue(reconcile.promise);
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.install" }),
    );
    await waitFor(() =>
      expect(m.install).toHaveBeenCalledWith("claude", "superpowers@official"),
    );
    await waitFor(() =>
      expect(screen.queryByText("superpowers")).not.toBeInTheDocument(),
    );
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewInstalledCount" }),
    );
    expect(screen.getByText("superpowers")).toBeInTheDocument();
    expect(m.listCatalog).toHaveBeenCalledTimes(1);
    await act(async () =>
      reconcile.resolve([
        plugin(),
        plugin({ ...superpowers, installed: true, enabled: true }),
      ]),
    );
  });

  it("points to the terminal when Claude Code wants the user to confirm a command", async () => {
    m.install.mockRejectedValue("PLUGIN_NEEDS_TERMINAL_CONFIRM");
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "pluginsPage.install" }),
    );
    await waitFor(() =>
      expect(m.toastError).toHaveBeenCalledWith(
        "pluginsPage.toast.installFailed",
        expect.objectContaining({
          description: "pluginsPage.toast.installNeedsTerminal",
        }),
      ),
    );
    // 没装上，条目留在「发现」里
    expect(screen.getByText("superpowers")).toBeInTheDocument();
  });

  it("loads the whole catalog and pages through Discover", async () => {
    const total = DEFAULT_PLUGIN_PAGE_SIZE * 2 + 5;
    m.listCatalog.mockResolvedValue(
      Array.from({ length: total }, (_, i) =>
        catalogItem({ id: `p${i}@m`, name: `p${i}` }),
      ),
    );
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    const list = await screen.findByTestId("plugins-discover");
    const installButtons = () =>
      within(list).getAllByRole("button", { name: "pluginsPage.install" });
    // 只挂当前页的行
    expect(installButtons()).toHaveLength(DEFAULT_PLUGIN_PAGE_SIZE);
    expect(within(list).getByText("p0")).toBeInTheDocument();
    expect(within(list).queryByText(`p${total - 1}`)).not.toBeInTheDocument();

    const next = () => screen.getByRole("button", { name: "usage.nextPage" });
    await userEvent.click(next());
    await userEvent.click(next());
    expect(installButtons()).toHaveLength(5);
    expect(within(list).getByText(`p${total - 1}`)).toBeInTheDocument();
    expect(next()).toBeDisabled();

    // 换关键词回到第一页；p1、p10–p19、p100–p104 一共 16 个，一页放得下就不画翻页条
    await userEvent.type(
      screen.getByRole("textbox", { name: "pluginsPage.searchAria" }),
      "p1",
    );
    await waitFor(() => expect(installButtons()).toHaveLength(16));
    expect(
      screen.queryByRole("button", { name: "usage.nextPage" }),
    ).not.toBeInTheDocument();

    await userEvent.type(
      screen.getByRole("textbox", { name: "pluginsPage.searchAria" }),
      "nothing-matches",
    );
    expect(
      await screen.findByText("pluginsPage.noMatches"),
    ).toBeInTheDocument();
  });

  it("stays on the current page when an install shrinks Discover", async () => {
    const total = DEFAULT_PLUGIN_PAGE_SIZE + 1;
    m.listCatalog.mockResolvedValue(
      Array.from({ length: total }, (_, i) =>
        catalogItem({ id: `p${i}@m`, name: `p${i}` }),
      ),
    );
    m.install.mockResolvedValue({ changed: true });
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    const list = await screen.findByTestId("plugins-discover");
    await userEvent.click(
      screen.getByRole("button", { name: "usage.nextPage" }),
    );
    expect(within(list).getByText(`p${total - 1}`)).toBeInTheDocument();
    // 装掉第二页唯一的一项：页数变成 1，收回到第一页，不留空页
    m.listInstalled.mockResolvedValue([
      plugin(),
      plugin({
        id: `p${total - 1}@m`,
        name: `p${total - 1}`,
        marketplace: "m",
      }),
    ]);
    await userEvent.click(
      within(list).getByRole("button", { name: "pluginsPage.install" }),
    );
    await waitFor(() =>
      expect(
        within(list).getAllByRole("button", { name: "pluginsPage.install" }),
      ).toHaveLength(DEFAULT_PLUGIN_PAGE_SIZE),
    );
    expect(within(list).getByText("p0")).toBeInTheDocument();
  });

  it("removes the row while uninstalling and restores it on failure", async () => {
    const pending = deferred<{ changed: boolean }>();
    m.uninstall.mockReturnValue(pending.promise);
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "pluginsPage.uninstall" }),
    );
    expect(m.uninstall).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "pluginsPage.uninstall" }),
    );
    await waitFor(() =>
      expect(screen.queryByText("frontend-design")).not.toBeInTheDocument(),
    );
    expect(m.uninstall).toHaveBeenCalledWith(
      "claude",
      "frontend-design@official",
    );

    await act(async () => pending.reject("busy"));
    expect(await screen.findByText("frontend-design")).toBeInTheDocument();
    expect(m.toastError).toHaveBeenCalledWith(
      "pluginsPage.toast.uninstallFailed",
      expect.objectContaining({ description: "busy" }),
    );
  });

  it("explains when the CLI is not installed", async () => {
    m.listInstalled.mockRejectedValue("PLUGIN_CLI_NOT_INSTALLED");
    renderPage();
    expect(
      await screen.findByText("pluginsPage.cliMissingTitle"),
    ).toBeInTheDocument();
    expect(screen.queryByText("common.retry")).not.toBeInTheDocument();
  });

  it("shows loading again while retrying a failed read", async () => {
    // 有上次的结果时，重试中查询仍是 error 状态（没有结果才回到 pending）
    localStorage.setItem(
      "cc-switch-cache:plugins:claude:installed",
      JSON.stringify([plugin({ name: "cached-plugin", id: "cached@m" })]),
    );
    m.listInstalled.mockRejectedValueOnce("Command timed out after 90s");
    renderPage();
    expect(
      await screen.findByText("Command timed out after 90s"),
    ).toBeInTheDocument();

    const retry = deferred<PluginEntry[]>();
    m.listInstalled.mockReturnValue(retry.promise);
    await userEvent.click(screen.getByRole("button", { name: "common.retry" }));
    expect(await screen.findByText("pluginsPage.loading")).toBeInTheDocument();
    expect(
      screen.queryByText("Command timed out after 90s"),
    ).not.toBeInTheDocument();

    await act(async () => retry.resolve([plugin()]));
    expect(await screen.findByText("frontend-design")).toBeInTheDocument();
  });

  it("prefetches the other app and switches to it", async () => {
    renderPage({ apps: ["claude", "codex"] });
    await screen.findByText("frontend-design");
    await waitFor(() => expect(m.listInstalled).toHaveBeenCalledWith("codex"));
    const codexCalls = () =>
      m.listInstalled.mock.calls.filter(([app]) => app === "codex").length;
    const before = codexCalls();
    await userEvent.click(screen.getByRole("button", { name: /Codex/ }));
    expect(await screen.findByText("frontend-design")).toBeInTheDocument();
    // 预取的结果还新鲜：切过去不再起一次 CLI
    expect(codexCalls()).toBe(before);
  });

  it("prefetches each other app only once per run", async () => {
    const first = renderPage({ apps: ["claude", "codex"] });
    await screen.findByText("frontend-design");
    await waitFor(() => expect(m.listInstalled).toHaveBeenCalledWith("codex"));
    first.unmount();
    const codexCalls = () =>
      m.listInstalled.mock.calls.filter(([app]) => app === "codex").length;
    const before = codexCalls();
    renderPage({ apps: ["claude", "codex"] });
    await screen.findByText("frontend-design");
    await waitFor(() => expect(claudeListCalls()).toBe(2));
    expect(codexCalls()).toBe(before);
  });

  it("hides the app switch when only one app is visible", async () => {
    renderPage({ apps: ["codex"] });
    await screen.findByText("frontend-design");
    expect(m.listInstalled).toHaveBeenCalledWith("codex");
    expect(
      screen.queryByRole("group", { name: "pluginsPage.appAria" }),
    ).not.toBeInTheDocument();
  });

  it("adds a marketplace and re-reads the catalog", async () => {
    renderPage();
    await screen.findByText("frontend-design");
    await waitFor(() => expect(m.listCatalog).toHaveBeenCalledTimes(1));
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.addMarketplace" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(
      within(dialog).getByRole("textbox"),
      "  anthropics/claude-plugins-official ",
    );
    await userEvent.click(
      within(dialog).getByRole("button", { name: "common.add" }),
    );
    await waitFor(() =>
      expect(m.addMarketplace).toHaveBeenCalledWith(
        "claude",
        "anthropics/claude-plugins-official",
      ),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(m.listCatalog).toHaveBeenCalledTimes(2));
  });

  it("keeps the add dialog open when adding fails", async () => {
    m.addMarketplace.mockRejectedValue("not a marketplace");
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.addMarketplace" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(within(dialog).getByRole("textbox"), "o/r");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "common.add" }),
    );
    await waitFor(() => expect(m.toastError).toHaveBeenCalled());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(within(dialog).getByRole("textbox")).toHaveValue("o/r");
  });

  it("lets a slow marketplace add run on after the dialog is closed", async () => {
    let finish!: (value: { changed: boolean }) => void;
    m.addMarketplace.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.addMarketplace" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(within(dialog).getByRole("textbox"), "big/repo");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "common.add" }),
    );
    expect(
      await within(dialog).findByText("pluginsPage.addMarketplaceWaiting"),
    ).toBeInTheDocument();
    // 进行中按钮叫「关闭」：只关窗口，不中止添加
    await userEvent.click(
      within(dialog).getByRole("button", { name: "common.close" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );

    finish({ changed: true });
    await waitFor(() => expect(m.toastSuccess).toHaveBeenCalled());
    expect(m.addMarketplace).toHaveBeenCalledTimes(1);
  });

  it("lists marketplaces and only offers removal for user-added ones", async () => {
    m.listMarketplaces.mockResolvedValue([
      { name: "builtin", removable: false, location: "/x" },
      { name: "mine", source: "me/plugins", removable: true },
    ]);
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewMarketplaces" }),
    );
    const list = await screen.findByTestId("plugins-marketplaces");
    expect(within(list).getByText("pluginsPage.builtIn")).toBeInTheDocument();
    const removeButtons = within(list).getAllByRole("button", {
      name: "pluginsPage.removeMarketplace",
    });
    expect(removeButtons).toHaveLength(1);
    await userEvent.click(removeButtons[0]);
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(dialog).getByRole("button", {
        name: "pluginsPage.removeMarketplace",
      }),
    );
    await waitFor(() =>
      expect(m.removeMarketplace).toHaveBeenCalledWith("claude", "mine"),
    );
  });

  it("pages through a long marketplace list", async () => {
    const total = DEFAULT_PLUGIN_PAGE_SIZE + 3;
    m.listMarketplaces.mockResolvedValue(
      Array.from({ length: total }, (_, i) => ({
        name: `market-${i}`,
        source: `me/market-${i}`,
        removable: true,
      })),
    );
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewMarketplaces" }),
    );
    const list = await screen.findByTestId("plugins-marketplaces");
    const removeButtons = () =>
      within(list).getAllByRole("button", {
        name: "pluginsPage.removeMarketplace",
      });
    expect(removeButtons()).toHaveLength(DEFAULT_PLUGIN_PAGE_SIZE);
    await userEvent.click(
      screen.getByRole("button", { name: "usage.nextPage" }),
    );
    expect(removeButtons()).toHaveLength(3);
    expect(within(list).getByText(`market-${total - 1}`)).toBeInTheDocument();
  });

  it("asks before installing for apps whose CLI needs explicit trust", async () => {
    m.listInstalled.mockResolvedValue([]);
    m.listCatalog.mockResolvedValue([
      catalogItem({ id: "hello", name: "hello", marketplace: "mkt" }),
    ]);
    renderPage({ apps: ["grokbuild"] });
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "pluginsPage.install" }),
    );
    // 先确认，不直接装
    expect(m.install).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("hello")).toBeInTheDocument();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "pluginsPage.install" }),
    );
    await waitFor(() =>
      expect(m.install).toHaveBeenCalledWith("grokbuild", "hello"),
    );
  });

  it("keeps bundled OpenClaw plugins toggle-only and hides marketplace management", async () => {
    m.listInstalled.mockResolvedValue([
      plugin({
        id: "a2a",
        name: "a2a",
        displayName: "A2A",
        marketplace: "",
        scope: null,
        removable: false,
      }),
    ]);
    renderPage({ apps: ["openclaw"] });
    const toggle = await enableSwitch();
    expect(toggle).toBeEnabled();
    expect(screen.getByText("pluginsPage.builtIn")).toBeInTheDocument();
    expect(screen.queryByText("@")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "pluginsPage.moreActions" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("tab", { name: "pluginsPage.viewMarketplaces" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "pluginsPage.addMarketplace" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "pluginsPage.refreshCatalog" }),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText("pluginsPage.restartNoteGateway").length,
    ).toBeGreaterThan(0);

    await userEvent.click(toggle);
    await waitFor(() =>
      expect(m.setEnabled).toHaveBeenCalledWith("openclaw", "a2a", false),
    );
  });

  it("installs an OpenClaw package under its plugin id after confirming", async () => {
    m.listInstalled.mockResolvedValue([]);
    m.listCatalog.mockResolvedValue([
      catalogItem({
        id: "@openclaw/memory-lancedb",
        name: "@openclaw/memory-lancedb",
        displayName: "Memory LanceDB",
        marketplace: "clawhub",
        installedId: "memory-lancedb",
      }),
    ]);
    renderPage({ apps: ["openclaw"] });
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    const reconcile = deferred<PluginEntry[]>();
    m.listInstalled.mockReturnValue(reconcile.promise);
    await userEvent.click(
      await screen.findByRole("button", { name: "pluginsPage.install" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "pluginsPage.install" }),
    );
    await waitFor(() =>
      expect(m.install).toHaveBeenCalledWith(
        "openclaw",
        "@openclaw/memory-lancedb",
      ),
    );
    // 装好立即挪到已安装：用插件 id，可以卸载
    await waitFor(() =>
      expect(screen.queryByText("Memory LanceDB")).not.toBeInTheDocument(),
    );
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewInstalledCount" }),
    );
    expect(screen.getByText("Memory LanceDB")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "pluginsPage.moreActions" }),
    ).toBeInTheDocument();
    await act(async () =>
      reconcile.resolve([
        plugin({
          id: "memory-lancedb",
          name: "memory-lancedb",
          marketplace: "",
        }),
      ]),
    );
  });

  it("manages Pi packages without marketplaces and confirms installs", async () => {
    m.listInstalled.mockResolvedValue([
      plugin({
        id: "npm:@scope/pi-tools",
        name: "@scope/pi-tools",
        marketplace: "",
        scope: null,
      }),
    ]);
    m.listCatalog.mockResolvedValue([
      catalogItem({
        id: "npm:@scope/pi-tools",
        name: "@scope/pi-tools",
        marketplace: "",
      }),
      catalogItem({
        id: "npm:pi-review",
        name: "pi-review",
        marketplace: "",
        description: "Review prompts",
      }),
    ]);
    m.install.mockResolvedValue({ changed: true });
    renderPage({ apps: ["pi"] });

    const toggle = await enableSwitch();
    expect(screen.queryByText("pluginsPage.builtIn")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("tab", { name: "pluginsPage.viewMarketplaces" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "pluginsPage.refreshCatalog" }),
    ).toBeInTheDocument();
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(m.setEnabled).toHaveBeenCalledWith(
        "pi",
        "npm:@scope/pi-tools",
        false,
      ),
    );

    // 已装的不在「发现」里；安装前先确认
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    expect(await screen.findByText("pi-review")).toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "pluginsPage.install" }),
    ).toHaveLength(1);
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.install" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("npm:pi-review")).toBeInTheDocument();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "pluginsPage.install" }),
    );
    await waitFor(() =>
      expect(m.install).toHaveBeenCalledWith("pi", "npm:pi-review"),
    );
  });

  it("removes a previously pinned Pi package from Discover after uninstall and reinstall", async () => {
    const pinnedInstalled = plugin({
      id: "npm:pi-pinned@1.0.0",
      name: "pi-pinned",
      catalogId: "npm:pi-pinned",
      marketplace: "",
      scope: null,
    });
    const catalogEntry = catalogItem({
      id: "npm:pi-pinned",
      name: "pi-pinned",
      marketplace: "",
      description: "Pinned package",
    });
    m.listInstalled.mockResolvedValue([pinnedInstalled]);
    m.listCatalog.mockResolvedValue([catalogEntry]);
    m.uninstall.mockResolvedValue({ changed: true });
    m.install.mockResolvedValue({ changed: true });

    renderPage({ apps: ["pi"] });
    await screen.findByText("pi-pinned");

    // 卸载固定版本的包，对账返回空列表
    m.listInstalled.mockResolvedValue([]);
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "pluginsPage.uninstall" }),
    );
    const uninstallDialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(uninstallDialog).getByRole("button", {
        name: "pluginsPage.uninstall",
      }),
    );
    await waitFor(() =>
      expect(m.uninstall).toHaveBeenCalledWith("pi", "npm:pi-pinned@1.0.0"),
    );

    // 切到「发现」能看到它；重新安装后立刻消失，且对账返回不带版本的 npm:pi-pinned 后仍留在已安装
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    expect(await screen.findByText("pi-pinned")).toBeInTheDocument();

    m.listInstalled.mockResolvedValue([
      plugin({
        id: "npm:pi-pinned",
        name: "pi-pinned",
        catalogId: null,
        marketplace: "",
        scope: null,
      }),
    ]);
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.install" }),
    );
    const installDialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(installDialog).getByRole("button", {
        name: "pluginsPage.install",
      }),
    );
    await waitFor(() =>
      expect(m.install).toHaveBeenCalledWith("pi", "npm:pi-pinned"),
    );
    await waitFor(() =>
      expect(screen.queryByText("pi-pinned")).not.toBeInTheDocument(),
    );
    // 目录没有因为装卸被重读
    expect(m.listCatalog).toHaveBeenCalledTimes(1);
  });

  it("keeps the in-flight spinner and disabled controls when switching apps and back", async () => {
    const updating = deferred<{ changed: boolean }>();
    m.update.mockReturnValue(updating.promise);
    m.listInstalled.mockImplementation(async (app) =>
      app === "codex"
        ? [plugin({ id: "studio@mkt", name: "studio", enabled: false })]
        : [plugin()],
    );
    renderPage({ apps: ["codex", "claude"] });
    await screen.findByText("studio");

    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "pluginsPage.update" }),
    );
    expect(screen.getByLabelText("pluginsPage.working")).toBeInTheDocument();
    expect(await enableSwitch()).toBeDisabled();

    // 切到 Claude Code：不会把 Codex 的转圈带过去
    await userEvent.click(screen.getByRole("button", { name: /Claude Code/ }));
    await screen.findByText("frontend-design");
    expect(
      screen.queryByLabelText("pluginsPage.working"),
    ).not.toBeInTheDocument();

    // 切回 Codex：转圈仍在，开关仍禁用，不能在中途重新启用
    await userEvent.click(screen.getByRole("button", { name: /Codex/ }));
    await screen.findByText("studio");
    expect(screen.getByLabelText("pluginsPage.working")).toBeInTheDocument();
    expect(await enableSwitch()).toBeDisabled();

    await act(async () => updating.resolve({ changed: true }));
    await waitFor(() =>
      expect(
        screen.queryByLabelText("pluginsPage.working"),
      ).not.toBeInTheDocument(),
    );
    expect(await enableSwitch()).toBeEnabled();
  });

  it("offers no update for apps without an update command", async () => {
    m.listInstalled.mockResolvedValue([
      plugin({
        id: "computer-use@official",
        name: "computer-use",
        scope: null,
      }),
    ]);
    renderPage({ apps: ["mcode"] });
    await screen.findByText("computer-use");
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.moreActions" }),
    );
    expect(
      await screen.findByRole("menuitem", { name: "pluginsPage.uninstall" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: "pluginsPage.update" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "pluginsPage.refreshMarketplaces" }),
    ).not.toBeInTheDocument();
  });

  it("tells the user to sign in when MiniMax Code requires login", async () => {
    m.listInstalled.mockResolvedValue([]);
    m.listCatalog.mockResolvedValue([
      catalogItem({
        id: "frontend-design@official",
        name: "frontend-design",
        marketplace: "official",
      }),
    ]);
    m.install.mockRejectedValue("PLUGIN_AUTH_REQUIRED");
    renderPage({ apps: ["mcode"] });
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "pluginsPage.install" }),
    );
    await waitFor(() =>
      expect(m.toastError).toHaveBeenCalledWith(
        "pluginsPage.toast.installFailed",
        expect.objectContaining({
          description: "pluginsPage.toast.authRequired",
        }),
      ),
    );
    // 没装上，条目留在「发现」里
    expect(screen.getByText("frontend-design")).toBeInTheDocument();
  });

  it("switches apps from a list when many apps are visible", async () => {
    renderPage({
      apps: ["claude", "codex", "grokbuild", "mcode", "openclaw"],
    });
    await screen.findByText("frontend-design");
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.appAria" }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: /OpenClaw/ }),
    );
    await waitFor(() =>
      expect(m.listInstalled).toHaveBeenCalledWith("openclaw"),
    );
    // OpenClaw 没有市场页
    expect(
      screen.queryByRole("tab", { name: "pluginsPage.viewMarketplaces" }),
    ).not.toBeInTheDocument();
  });

  it("returns to Installed when switching to an app without marketplaces", async () => {
    renderPage({ apps: ["claude", "mcode"] });
    await screen.findByText("frontend-design");
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewMarketplaces" }),
    );
    await screen.findByText("pluginsPage.emptyMarketsTitle");
    await userEvent.click(screen.getByRole("button", { name: /MiniMax Code/ }));
    expect(
      await screen.findByRole("tab", {
        name: "pluginsPage.viewInstalledCount",
        selected: true,
      }),
    ).toBeInTheDocument();
    expect(m.listMarketplaces).not.toHaveBeenCalledWith("mcode");
  });

  it("manages Antigravity plugins via catalog ids on install and dir names afterwards", async () => {
    m.listInstalled.mockResolvedValue([
      plugin({
        id: "arize-skills",
        name: "arize-skills",
        displayName: "Arize AX",
        marketplace: "antigravity-plugins-partners",
        enabled: false,
        scope: null,
      }),
    ]);
    m.listCatalog.mockResolvedValue([
      catalogItem({
        id: "arize-skills@antigravity-plugins-partners",
        name: "arize-skills",
        displayName: "Arize AX",
        marketplace: "antigravity-plugins-partners",
        installedId: "arize-skills",
      }),
      catalogItem({
        id: "gcalendar@antigravity-plugins-official",
        name: "gcalendar",
        displayName: "Google Calendar",
        marketplace: "antigravity-plugins-official",
        installedId: "gcalendar",
        description: "Check availability, schedule events, and RSVP.",
      }),
    ]);
    renderPage({ apps: ["agy"] });

    expect(await screen.findByText("Arize AX")).toBeInTheDocument();
    expect(
      screen.queryByRole("tab", { name: "pluginsPage.viewMarketplaces" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "pluginsPage.addMarketplace" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "pluginsPage.refreshCatalog" }),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText("pluginsPage.restartNote").length,
    ).toBeGreaterThan(0);

    // 需要配置 MCP 变量的插件启用失败时，开关弹回并展示 CLI 报错
    m.setEnabled.mockRejectedValueOnce(
      'plugin "arize-skills" cannot be enabled here: its MCP servers need configuring first',
    );
    const arizeToggle = await enableSwitch();
    await userEvent.click(arizeToggle);
    await waitFor(() =>
      expect(m.setEnabled).toHaveBeenCalledWith("agy", "arize-skills", true),
    );
    expect(arizeToggle).toHaveAttribute("data-state", "unchecked");
    expect(m.toastError).toHaveBeenCalledWith(
      "pluginsPage.toast.toggleFailed",
      expect.objectContaining({
        description:
          'plugin "arize-skills" cannot be enabled here: its MCP servers need configuring first',
      }),
    );

    // 发现页过滤掉已装的 arize-skills；安装 gcalendar 不弹确认框，按 name@marketplace 传给 CLI
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    expect(await screen.findByText("Google Calendar")).toBeInTheDocument();
    expect(screen.queryByText("Arize AX")).not.toBeInTheDocument();

    const reconcile = deferred<PluginEntry[]>();
    m.listInstalled.mockReturnValue(reconcile.promise);
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.install" }),
    );
    await waitFor(() =>
      expect(m.install).toHaveBeenCalledWith(
        "agy",
        "gcalendar@antigravity-plugins-official",
      ),
    );
    await waitFor(() =>
      expect(screen.queryByText("Google Calendar")).not.toBeInTheDocument(),
    );

    // 装好立即进已安装并保留市场名，启停、更新、卸载都传目录名 gcalendar
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewInstalledCount" }),
    );
    expect(screen.getByText("Google Calendar")).toBeInTheDocument();
    expect(
      screen.getByText("@antigravity-plugins-official"),
    ).toBeInTheDocument();
    await act(async () =>
      reconcile.resolve([
        plugin({
          id: "arize-skills",
          name: "arize-skills",
          displayName: "Arize AX",
          marketplace: "antigravity-plugins-partners",
          enabled: false,
          scope: null,
        }),
        plugin({
          id: "gcalendar",
          name: "gcalendar",
          displayName: "Google Calendar",
          marketplace: "antigravity-plugins-official",
          enabled: true,
          scope: null,
        }),
      ]),
    );
    const menus = screen.getAllByRole("button", {
      name: "pluginsPage.moreActions",
    });
    await userEvent.click(menus[1]);
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "pluginsPage.update" }),
    );
    await waitFor(() =>
      expect(m.update).toHaveBeenCalledWith("agy", "gcalendar"),
    );
  });

  it("asks to reinstall when an Antigravity plugin sits in a legacy numeric folder", async () => {
    m.listInstalled.mockResolvedValue([
      plugin({
        id: "-1870732469773246571",
        name: "-1870732469773246571",
        displayName: "Firebase",
        marketplace: "antigravity-plugins-official",
        scope: null,
      }),
    ]);
    m.update.mockRejectedValue("PLUGIN_UPDATE_NEEDS_REINSTALL");
    renderPage({ apps: ["agy"] });

    await screen.findByText("Firebase");
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "pluginsPage.update" }),
    );
    await waitFor(() =>
      expect(m.toastError).toHaveBeenCalledWith(
        "pluginsPage.toast.updateFailed",
        expect.objectContaining({
          description: "pluginsPage.toast.updateNeedsReinstall",
        }),
      ),
    );
  });

  it("keeps newly installed Antigravity plugins disabled and prompts /plugin when MCP configuration is required", async () => {
    m.listInstalled.mockResolvedValue([]);
    m.listCatalog.mockResolvedValue([
      catalogItem({
        id: "arize-skills@antigravity-plugins-partners",
        name: "arize-skills",
        displayName: "Arize AX",
        marketplace: "antigravity-plugins-partners",
        installedId: "arize-skills",
      }),
    ]);
    m.install.mockResolvedValue({
      changed: true,
      needsConfiguration: true,
    });
    renderPage({ apps: ["agy"] });

    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    const reconcile = deferred<PluginEntry[]>();
    m.listInstalled.mockReturnValue(reconcile.promise);
    await userEvent.click(
      await screen.findByRole("button", { name: "pluginsPage.install" }),
    );

    await waitFor(() =>
      expect(m.toastSuccess).toHaveBeenCalledWith(
        "pluginsPage.toast.installed",
        expect.objectContaining({
          description: "pluginsPage.toast.installedNeedsConfig",
        }),
      ),
    );
    // 对账尚未完成时，已安装列表里也直接是停用状态，不会先亮后灭
    await userEvent.click(
      screen.getByRole("tab", { name: "pluginsPage.viewInstalledCount" }),
    );
    expect(await screen.findByText("Arize AX")).toBeInTheDocument();
    expect(await enableSwitch()).toHaveAttribute("data-state", "unchecked");

    await act(async () =>
      reconcile.resolve([
        plugin({
          id: "arize-skills",
          name: "arize-skills",
          displayName: "Arize AX",
          marketplace: "antigravity-plugins-partners",
          enabled: false,
          scope: null,
        }),
      ]),
    );
    expect(await enableSwitch()).toHaveAttribute("data-state", "unchecked");
  });

  it("guides the user to close running Antigravity sessions when an update fails because the plugin is in use", async () => {
    m.listInstalled.mockResolvedValue([
      plugin({
        id: "gmail",
        name: "gmail",
        displayName: "Gmail",
        marketplace: "antigravity-plugins-official",
        enabled: true,
        scope: null,
      }),
    ]);
    m.listCatalog.mockResolvedValue([]);
    m.update.mockRejectedValue("PLUGIN_IN_USE");
    renderPage({ apps: ["agy"] });

    expect(await screen.findByText("Gmail")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "pluginsPage.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "pluginsPage.update" }),
    );
    await waitFor(() =>
      expect(m.toastError).toHaveBeenCalledWith(
        "pluginsPage.toast.updateFailed",
        expect.objectContaining({
          description: "pluginsPage.toast.pluginInUse",
        }),
      ),
    );
  });

  it("cleans up all stale cached duplicate local-scope rows after refresh without leaving ghost DOM nodes", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      // 模拟旧版 v4.0.40 缓存：同一个插件 id 在两个项目里装了 local 范围，但缓存里没有 projectPath
      const staleLocal = plugin({
        id: "context7@claude-plugins-official",
        name: "context7",
        marketplace: "claude-plugins-official",
        version: "41f619d61b99",
        scope: "local",
        manageable: false,
        removable: false,
      });
      const userPlugin = plugin({
        id: "frontend-design@claude-plugins-official",
        name: "frontend-design",
        marketplace: "claude-plugins-official",
        scope: "user",
      });
      const removableUserPlugin = plugin({
        id: "playwright@claude-plugins-official",
        name: "playwright",
        marketplace: "claude-plugins-official",
        scope: "user",
      });
      localStorage.setItem(
        "cc-switch-cache:plugins:claude:installed",
        JSON.stringify([
          removableUserPlugin,
          userPlugin,
          staleLocal,
          { ...staleLocal },
        ]),
      );

      const fresh = deferred<PluginEntry[]>();
      m.listInstalled.mockReturnValueOnce(fresh.promise);
      renderPage();

      const list = await screen.findByTestId("plugins-installed");
      expect(within(list).getAllByText("context7")).toHaveLength(2);
      expect(list.children).toHaveLength(4);

      // 卸载排在前面的 user 插件时，后面重复的旧缓存 local 行不会触发幽灵节点残留
      const uninstallPending = deferred<{ changed: boolean }>();
      m.uninstall.mockReturnValue(uninstallPending.promise);
      await userEvent.click(
        within(list).getAllByRole("button", {
          name: "pluginsPage.moreActions",
        })[0],
      );
      await userEvent.click(
        await screen.findByRole("menuitem", { name: "pluginsPage.uninstall" }),
      );
      const dialog = await screen.findByRole("dialog");
      await userEvent.click(
        within(dialog).getByRole("button", { name: "pluginsPage.uninstall" }),
      );
      await waitFor(() => expect(list.children).toHaveLength(3));
      expect(within(list).queryByText("playwright")).not.toBeInTheDocument();
      expect(within(list).getAllByText("context7")).toHaveLength(2);

      // 在终端里删掉全部 local 插件后，对账刷新只返回剩余 user 插件：不能残留任何幽灵行
      m.listInstalled.mockResolvedValue([userPlugin]);
      await act(async () => uninstallPending.resolve({ changed: true }));
      await waitFor(() =>
        expect(within(list).queryByText("context7")).not.toBeInTheDocument(),
      );
      expect(list.children).toHaveLength(1);
      expect(within(list).getByText("frontend-design")).toBeInTheDocument();
      expect(errorSpy).not.toHaveBeenCalled();
    } finally {
      errorSpy.mockRestore();
    }
  });

  it("renders, searches, and cleanly transitions multi-project local-scope installs", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const localA = plugin({
        id: "context7@claude-plugins-official",
        name: "context7",
        marketplace: "claude-plugins-official",
        scope: "local",
        projectPath: "/work/alpha-app",
        manageable: false,
        removable: false,
      });
      const localB = plugin({
        id: "context7@claude-plugins-official",
        name: "context7",
        marketplace: "claude-plugins-official",
        scope: "local",
        projectPath: "/work/beta-service",
        manageable: false,
        removable: false,
      });
      const userPlugin = plugin({
        id: "frontend-design@official",
        name: "frontend-design",
        scope: "user",
        projectPath: "/ignored-user-path",
      });
      const blankPathProject = plugin({
        id: "lint@official",
        name: "lint",
        scope: "project",
        projectPath: "   ",
        manageable: false,
        removable: false,
      });

      m.listInstalled.mockImplementation(async (app) =>
        app === "codex"
          ? [plugin({ id: "codex-only@m", name: "codex-only", scope: null })]
          : [userPlugin, localA, localB, blankPathProject],
      );
      m.listCatalog.mockResolvedValue([superpowers]);

      renderPage({ apps: ["claude", "codex"] });
      const list = await screen.findByTestId("plugins-installed");
      expect(list.children).toHaveLength(4);
      expect(within(list).getByText("/work/alpha-app")).toHaveAttribute(
        "title",
        "/work/alpha-app",
      );
      expect(within(list).getByText("/work/beta-service")).toHaveAttribute(
        "title",
        "/work/beta-service",
      );
      expect(
        within(list).queryByText("/ignored-user-path"),
      ).not.toBeInTheDocument();
      // 仅含空白的 projectPath 不应渲染多余的空路径段落
      expect(list.children[3].querySelectorAll("p")).toHaveLength(1);

      // 按项目路径过滤与清空不残留幽灵行
      const searchBox = screen.getByRole("textbox", {
        name: "pluginsPage.searchAria",
      });
      await userEvent.type(searchBox, "beta-service");
      await waitFor(() => expect(list.children).toHaveLength(1));
      expect(within(list).getByText("/work/beta-service")).toBeInTheDocument();
      expect(
        within(list).queryByText("/work/alpha-app"),
      ).not.toBeInTheDocument();

      await userEvent.clear(searchBox);
      await waitFor(() => expect(list.children).toHaveLength(4));

      // 切到发现页再切到 Codex：不会残留任何 Claude 的 local 行
      await userEvent.click(
        await screen.findByRole("tab", {
          name: "pluginsPage.viewDiscoverCount",
        }),
      );
      const discover = await screen.findByTestId("plugins-discover");
      expect(discover.children).toHaveLength(1);
      expect(within(discover).queryByText("context7")).not.toBeInTheDocument();

      await userEvent.click(
        screen.getByRole("tab", { name: "pluginsPage.viewInstalledCount" }),
      );
      await userEvent.click(screen.getByRole("button", { name: /Codex/ }));
      await waitFor(() =>
        expect(screen.getByText("codex-only")).toBeInTheDocument(),
      );
      expect(screen.queryByText("context7")).not.toBeInTheDocument();
      expect(errorSpy).not.toHaveBeenCalled();
    } finally {
      errorSpy.mockRestore();
    }
  });

  it("isolates busy spinners and optimistic mutations from read-only local rows while keeping Discover install spinners working", async () => {
    const userRow = plugin({
      id: "context7@official",
      name: "context7",
      scope: "user",
      enabled: true,
      manageable: true,
      removable: true,
    });
    const localRow = plugin({
      id: "context7@official",
      name: "context7",
      scope: "local",
      projectPath: "/work/alpha-app",
      enabled: true,
      manageable: false,
      removable: false,
    });
    const updatableUserRow = plugin({
      id: "playwright@official",
      name: "playwright",
      scope: "user",
      enabled: true,
      manageable: true,
      removable: true,
    });
    const updatableLocalRow = plugin({
      id: "playwright@official",
      name: "playwright",
      scope: "local",
      projectPath: "/work/beta-app",
      enabled: true,
      manageable: false,
      removable: false,
    });
    // Claude 目录条目在后端 Default 下 manageable 为 false，安装时仍需显示转圈
    const claudeDiscoverItem = catalogItem({
      id: "superpowers@official",
      name: "superpowers",
      manageable: false,
    });
    m.listInstalled.mockResolvedValue([
      userRow,
      localRow,
      updatableUserRow,
      updatableLocalRow,
    ]);
    m.listCatalog.mockResolvedValue([claudeDiscoverItem]);

    renderPage({ apps: ["claude"] });
    const list = await screen.findByTestId("plugins-installed");
    expect(list.children).toHaveLength(4);

    const togglePending = deferred<{ changed: boolean }>();
    m.setEnabled.mockReturnValue(togglePending.promise);
    const switches = within(list).getAllByRole("switch", {
      name: "pluginsPage.enableAria",
    });
    expect(switches[0]).toBeEnabled();
    expect(switches[1]).toBeDisabled();

    await userEvent.click(switches[0]);
    // 只有 user 行显示转圈并乐观切关；同名 local 只读行不转圈也不被改状态
    expect(within(list).getAllByLabelText("pluginsPage.working")).toHaveLength(
      1,
    );
    expect(switches[0]).toHaveAttribute("data-state", "unchecked");
    expect(switches[1]).toHaveAttribute("data-state", "checked");

    m.listInstalled.mockResolvedValue([
      { ...userRow, enabled: false },
      localRow,
      updatableUserRow,
      updatableLocalRow,
    ]);
    await act(async () => togglePending.resolve({ changed: true }));
    await waitFor(() =>
      expect(
        within(list).queryByLabelText("pluginsPage.working"),
      ).not.toBeInTheDocument(),
    );

    // 更新 user 行时：只有 user 行显示转圈，同名 local 只读行不显示转圈
    const updatePending = deferred<{ changed: boolean }>();
    m.update.mockReturnValue(updatePending.promise);
    await userEvent.click(
      within(list).getAllByRole("button", {
        name: "pluginsPage.moreActions",
      })[2],
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "pluginsPage.update" }),
    );
    expect(within(list).getAllByLabelText("pluginsPage.working")).toHaveLength(
      1,
    );
    await act(async () => updatePending.resolve({ changed: false }));
    await waitFor(() =>
      expect(
        within(list).queryByLabelText("pluginsPage.working"),
      ).not.toBeInTheDocument(),
    );

    // 卸载 user 行并在失败时回滚：local 行不显示转圈且始终保留
    const uninstallPending = deferred<{ changed: boolean }>();
    m.uninstall.mockReturnValue(uninstallPending.promise);
    await userEvent.click(
      within(list).getAllByRole("button", {
        name: "pluginsPage.moreActions",
      })[0],
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "pluginsPage.uninstall" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "pluginsPage.uninstall" }),
    );
    await waitFor(() => expect(list.children).toHaveLength(3));
    expect(within(list).getByText("/work/alpha-app")).toBeInTheDocument();
    expect(
      within(list).queryByLabelText("pluginsPage.working"),
    ).not.toBeInTheDocument();

    await act(async () => uninstallPending.reject("boom"));
    await waitFor(() => expect(list.children).toHaveLength(4));

    // 切到发现页安装 manageable: false 的 Claude 目录项：按钮和转圈正常工作
    const installPending = deferred<{ changed: boolean }>();
    m.install.mockReturnValue(installPending.promise);
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    const discover = await screen.findByTestId("plugins-discover");
    const installBtn = within(discover).getByRole("button", {
      name: "pluginsPage.install",
    });
    await userEvent.click(installBtn);
    expect(
      within(discover).getByLabelText("pluginsPage.working"),
    ).toBeInTheDocument();
    expect(
      within(discover).getByRole("button", { name: "pluginsPage.installing" }),
    ).toBeDisabled();

    await act(async () => installPending.resolve({ changed: true }));
  });
});

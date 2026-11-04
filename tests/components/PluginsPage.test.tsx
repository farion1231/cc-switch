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
  DISCOVER_RENDER_LIMIT,
  PluginsPage,
  availableFromCatalog,
  filterPlugins,
  withCatalogDetails,
} from "@/components/plugins/PluginsPage";
import { resetPluginWriteState } from "@/hooks/usePlugins";

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
  it("filters by id, display name, description and marketplace", () => {
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
    ];
    expect(filterPlugins(entries, "  ")).toHaveLength(3);
    expect(filterPlugins(entries, "FRONTEND").map((e) => e.id)).toEqual([
      "frontend-design@official",
    ]);
    expect(filterPlugins(entries, "other").map((e) => e.id)).toEqual([
      "x@other",
    ]);
    expect(filterPlugins(entries, "studio").map((e) => e.id)).toEqual(["y@m"]);
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

  it("filters Discover with search and caps long lists", async () => {
    const many = Array.from({ length: DISCOVER_RENDER_LIMIT + 5 }, (_, i) =>
      catalogItem({ id: `p${i}@m`, name: `p${i}` }),
    );
    m.listCatalog.mockResolvedValue(many);
    renderPage();
    await screen.findByText("frontend-design");
    await userEvent.click(
      await screen.findByRole("tab", { name: "pluginsPage.viewDiscoverCount" }),
    );
    const list = await screen.findByTestId("plugins-discover");
    expect(
      within(list).getAllByRole("button", { name: "pluginsPage.install" }),
    ).toHaveLength(DISCOVER_RENDER_LIMIT);
    expect(screen.getByText("pluginsPage.truncated")).toBeInTheDocument();

    await userEvent.type(
      screen.getByRole("textbox", { name: "pluginsPage.searchAria" }),
      "nothing-matches",
    );
    expect(
      await screen.findByText("pluginsPage.noMatches"),
    ).toBeInTheDocument();
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
});

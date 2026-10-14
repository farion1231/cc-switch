import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { ProxyAppId } from "@/config/appConfig";
import type { AppMode } from "@/types/proxy";
import { SwitchModePanel } from "@/components/providers/mode/SwitchModePanel";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

// 只关心模式层：卡片画成按钮，按 presentation 的 key 找；「更多」菜单里的项加 menu- 前缀
vi.mock("@/components/providers/ProviderCard", () => ({
  ProviderCard: ({ provider, presentation }: any) => (
    <div data-testid={`card-${provider.id}`}>
      {presentation.buttons.map((button: any) => (
        <button
          key={button.key}
          data-testid={`${button.key}-${provider.id}`}
          disabled={Boolean(button.disabledReason)}
          onClick={button.onClick}
        >
          {button.label}
        </button>
      ))}
      {(presentation.menuItems ?? []).map((item: any) => (
        <button
          key={item.key}
          data-testid={`menu-${item.key}-${provider.id}`}
          disabled={Boolean(item.disabledReason)}
          onClick={item.onSelect}
        >
          {item.label}
        </button>
      ))}
    </div>
  ),
}));

vi.mock("@/hooks/useStreamCheck", () => ({
  useStreamCheck: () => ({ checkProvider: vi.fn(), isChecking: () => false }),
}));

const TAURI_ENDPOINT = "http://tauri.local";

const provider = (id: string): Provider => ({
  id,
  name: id,
  settingsConfig: {},
});

function mockMode(
  mode: AppMode,
  routeProviderId: string | null,
  directProviderId: string | null = routeProviderId,
) {
  server.use(
    http.post(`${TAURI_ENDPOINT}/get_app_mode`, () =>
      HttpResponse.json({
        mode,
        attached: mode !== "direct",
        routeProviderId,
        directProviderId,
      }),
    ),
    http.post(`${TAURI_ENDPOINT}/get_proxy_status`, () =>
      HttpResponse.json({ running: true }),
    ),
  );
}

function renderPanel(
  app: ProxyAppId,
  providers: Record<string, Provider>,
  onSwitch = vi.fn(),
) {
  const queryClient = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SwitchModePanel
        app={app}
        providers={providers}
        currentProviderId={Object.keys(providers)[0] ?? ""}
        isLoading={false}
        onSwitch={onSwitch}
        onOpenRoutingSettings={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />
    </QueryClientProvider>,
  );
  return { ...view, queryClient };
}

beforeEach(() => {
  vi.mocked(toast.info).mockClear();
  vi.mocked(toast.success).mockClear();
  // jsdom 没有 scrollIntoView，确认框里的下拉打开时会调
  Element.prototype.scrollIntoView = vi.fn();
});

describe("SwitchModePanel — Stack mode", () => {
  it("adds and removes Stack members through the backend", async () => {
    mockMode("stack", "route");
    const setCalls: unknown[] = [];
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: true,
          members: [
            { providerId: "route", modelIds: ["m-route"], route: true },
            { providerId: "kimi", modelIds: ["m-kimi"], route: false },
          ],
        }),
      ),
      http.post(
        `${TAURI_ENDPOINT}/set_proxy_stack_member`,
        async ({ request }) => {
          setCalls.push(await request.json());
          return HttpResponse.json(null);
        },
      ),
    );

    renderPanel("claude", {
      route: provider("route"),
      kimi: provider("kimi"),
      other: provider("other"),
    });

    fireEvent.click(await screen.findByTestId("remove-kimi"));
    fireEvent.click(await screen.findByTestId("add-other"));
    await waitFor(() => expect(setCalls).toHaveLength(2));
    expect(setCalls).toEqual([
      { appType: "claude", providerId: "kimi", enabled: false },
      { appType: "claude", providerId: "other", enabled: true },
    ]);
  });

  it("says why the Codex model list could not be read", async () => {
    mockMode("stack", "route");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: true,
          members: [{ providerId: "route", modelIds: [], route: true }],
          notice: "officialModelsUnavailable",
          noticeReasons: ["loginStale", "noCli"],
        }),
      ),
    );
    renderPanel("codex", { route: provider("route") });

    expect(
      await screen.findByText("provider.officialModelsUnavailable"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("provider.officialModelsReason.loginStale"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("provider.officialModelsReason.noCli"),
    ).toBeInTheDocument();
  });

  it("warns about Codex clients on an old model list in every mode", async () => {
    const stack = (active: boolean) =>
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active,
          members: [{ providerId: "route", modelIds: [], route: true }],
          staleClients: { daemon: true, others: false },
        }),
      );

    mockMode("stack", "route");
    server.use(stack(true));
    const view = renderPanel("codex", { route: provider("route") });
    expect(
      await screen.findByText("proxy.stackMode.codexStale.title"),
    ).toBeInTheDocument();
    view.unmount();

    mockMode("route", "route");
    server.use(stack(false));
    renderPanel("codex", { route: provider("route") });
    expect(
      await screen.findByText("proxy.stackMode.codexStale.title"),
    ).toBeInTheDocument();
  });

  it("shows a dismissed Codex stale notice again once the model list changes again", async () => {
    let revision = "r1";
    mockMode("direct", "route");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: false,
          members: [],
          staleClients: { daemon: false, others: true },
          staleRevision: revision,
        }),
      ),
    );
    const { queryClient } = renderPanel("codex", { route: provider("route") });
    await screen.findByText("proxy.stackMode.codexStale.title");
    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    expect(
      screen.queryByText("proxy.stackMode.codexStale.title"),
    ).not.toBeInTheDocument();

    // 同一份：再拉一次还是关着。
    await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    expect(
      screen.queryByText("proxy.stackMode.codexStale.title"),
    ).not.toBeInTheDocument();

    // 同一个供应商又改了一次目录：同样的客户端，但已经是新的一份。
    revision = "r2";
    await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    expect(
      await screen.findByText("proxy.stackMode.codexStale.title"),
    ).toBeInTheDocument();
  });

  it.each(["direct", "route", "stack"] as const)(
    "shows a cached account notice in %s mode",
    async (mode) => {
      mockMode(mode, "route");
      server.use(
        http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
          HttpResponse.json({
            active: mode === "stack",
            members: [],
            staleClients: { daemon: true, others: false, auth: true },
          }),
        ),
      );
      renderPanel("codex", { route: provider("route") });
      expect(
        await screen.findByText("proxy.stackMode.codexStale.authTitle"),
      ).toBeInTheDocument();
      expect(
        screen.queryByText("proxy.stackMode.codexStale.title"),
      ).not.toBeInTheDocument();
    },
  );

  it("does not read the Stack list for apps without Stack mode", async () => {
    mockMode("route", "a");
    let stackReads = 0;
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () => {
        stackReads += 1;
        return HttpResponse.json({ active: false, members: [] });
      }),
    );

    renderPanel("gemini", { a: provider("a") });
    await screen.findByTestId("card-a");
    expect(stackReads).toBe(0);
    expect(
      screen.queryByRole("tab", { name: /mode\.stack/ }),
    ).not.toBeInTheDocument();
  });
});

describe("SwitchModePanel — mode layer", () => {
  function captureTakeover() {
    const calls: unknown[] = [];
    server.use(
      http.post(
        `${TAURI_ENDPOINT}/set_proxy_takeover_for_app`,
        async ({ request }) => {
          calls.push(await request.json());
          return HttpResponse.json(null);
        },
      ),
    );
    return calls;
  }

  it("enters Stack from the notice and picks the default in the dialog", async () => {
    const user = userEvent.setup();
    mockMode("direct", null, "a");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: false, members: [] }),
      ),
    );
    const calls = captureTakeover();
    renderPanel("claude", { a: provider("a"), kimi: provider("kimi") });

    await user.click(
      await screen.findByRole("button", { name: /mode\.names\.stack/ }),
    );
    // 行上只有名单的添加，「设为默认」在「更多」菜单里；没有会切模式的按钮
    const row = await screen.findByTestId("card-kimi");
    expect(
      Array.from(row.querySelectorAll("button")).map((b) => b.dataset.testid),
    ).toEqual(["add-kimi", "menu-setDefault-kimi"]);
    expect(screen.getByTestId("add-kimi")).not.toBeDisabled();

    await user.click(
      screen.getByRole("button", { name: "mode.activate.stack" }),
    );
    expect(await screen.findByText("mode.dialog.stackTitle")).toBeVisible();
    // 预选直连那家，可以在框里换
    const select = screen.getByRole("combobox", {
      name: "mode.dialog.stackDefault",
    });
    expect(select).toHaveTextContent("a");
    await user.click(select);
    await user.click(await screen.findByRole("option", { name: "kimi" }));
    expect(select).toHaveTextContent("kimi");
    // Claude 官方订阅进不了聚合，没有「官方只能做默认」的提示
    expect(screen.queryByTestId("stack-official-note")).not.toBeInTheDocument();

    await user.click(screen.getByText("mode.dialog.confirmStack"));
    await waitFor(() =>
      expect(calls).toEqual([
        { appType: "claude", enabled: true, stack: true, route: "kimi" },
      ]),
    );
  });

  it("records the Stack default from the card menu while direct, without entering the mode", async () => {
    const user = userEvent.setup();
    let route: string | null = null;
    const routeCalls: unknown[] = [];
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_app_mode`, () =>
        HttpResponse.json({
          mode: "direct",
          attached: false,
          routeProviderId: route,
          directProviderId: "a",
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: false, members: [] }),
      ),
      http.post(`${TAURI_ENDPOINT}/set_proxy_route`, async ({ request }) => {
        const body = (await request.json()) as { providerId: string };
        routeCalls.push(body);
        route = body.providerId;
        return HttpResponse.json(null);
      }),
    );
    const takeover = captureTakeover();
    const onSwitch = vi.fn();
    renderPanel("claude", { a: provider("a"), kimi: provider("kimi") }, onSwitch);

    await user.click(
      await screen.findByRole("button", { name: /mode\.names\.stack/ }),
    );
    await user.click(await screen.findByTestId("menu-setDefault-kimi"));

    // 只记下选择：不进聚合、不走切换供应商
    await waitFor(() =>
      expect(routeCalls).toEqual([{ appType: "claude", providerId: "kimi" }]),
    );
    expect(takeover).toEqual([]);
    expect(onSwitch).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith(
        "mode.toast.defaultRemembered",
        expect.anything(),
      ),
    );
    // 记下的那家成了「切换后的默认」：自己不再有「设为默认」，原来那家有了
    await screen.findByTestId("menu-setDefault-a");
    expect(screen.queryByTestId("menu-setDefault-kimi")).not.toBeInTheDocument();

    // 确认框预选它
    await user.click(
      screen.getByRole("button", { name: "mode.activate.stack" }),
    );
    expect(
      await screen.findByRole("combobox", { name: "mode.dialog.stackDefault" }),
    ).toHaveTextContent("kimi");
  });

  it("changes the Stack default right away once Stack mode is on", async () => {
    mockMode("stack", "route");
    const routeCalls: unknown[] = [];
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: true,
          members: [
            { providerId: "route", modelIds: ["m-route"], route: true },
            { providerId: "kimi", modelIds: ["m-kimi"], route: false },
          ],
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/set_proxy_route`, async ({ request }) => {
        routeCalls.push(await request.json());
        return HttpResponse.json(null);
      }),
    );
    const onSwitch = vi.fn();
    const kimi = provider("kimi");
    const other = provider("other");
    renderPanel("claude", { route: provider("route"), kimi, other }, onSwitch);

    // 已添加的、还没添加的都能直接设为默认，走的是切换供应商（当场生效）
    fireEvent.click(await screen.findByTestId("menu-setDefault-kimi"));
    fireEvent.click(screen.getByTestId("menu-setDefault-other"));
    expect(onSwitch.mock.calls.map(([p]) => p)).toEqual([kimi, other]);
    expect(screen.queryByTestId("menu-setDefault-route")).not.toBeInTheDocument();
    expect(routeCalls).toEqual([]);
  });

  it("cannot change the Stack default from the preview while routing", async () => {
    mockMode("route", "a");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: false, members: [] }),
      ),
    );
    renderPanel("claude", { a: provider("a"), kimi: provider("kimi") });

    // 等模式读到了再换页签：生效的模式一变，查看的那格会跳回生效的那格
    await screen.findByText("→ a");
    fireEvent.click(screen.getByRole("button", { name: /mode\.names\.stack/ }));
    // 默认和路由目标是同一个指针：路由中改它就是当场换路由，只能在切换时的确认框里选
    expect(await screen.findByTestId("menu-setDefault-kimi")).toBeDisabled();
  });

  it("warns that a Codex official account drops out of the Stack unless it is the default", async () => {
    const user = userEvent.setup();
    mockMode("direct", null, "kimi");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: false, members: [] }),
      ),
    );
    const account: Provider = {
      ...provider("account"),
      category: "official",
      settingsConfig: { auth: {}, config: "" },
    };
    renderPanel("codex", { account, kimi: provider("kimi") });

    await user.click(
      await screen.findByRole("button", { name: /mode\.names\.stack/ }),
    );
    await user.click(
      await screen.findByRole("button", { name: "mode.activate.stack" }),
    );
    const select = await screen.findByRole("combobox", {
      name: "mode.dialog.stackDefault",
    });
    expect(select).toHaveTextContent("kimi");
    expect(screen.getByTestId("stack-official-note")).toHaveTextContent(
      "mode.dialog.stackOfficialNote",
    );

    await user.click(select);
    await user.click(
      await screen.findByRole("option", { name: "mode.dialog.officialOption" }),
    );
    expect(screen.queryByTestId("stack-official-note")).not.toBeInTheDocument();
  });

  it("enters routing from the notice with the direct provider preselected, without an acknowledgement checkbox", async () => {
    mockMode("direct", null, "b");
    const calls = captureTakeover();
    renderPanel("claude", { a: provider("a"), b: provider("b") });

    fireEvent.click(
      await screen.findByRole("button", { name: /mode\.names\.route/ }),
    );
    // 行上没有进入路由的按钮，入口只有通知条
    await screen.findByTestId("card-b");
    expect(screen.getByTestId("card-b").querySelector("button")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "mode.activate.route" }),
    );
    expect(await screen.findByText("mode.dialog.routeTitle")).toBeVisible();
    // 没有上次的路由目标时，预选直连那家
    expect(
      screen.getByRole("combobox", { name: "mode.dialog.routeTo" }),
    ).toHaveTextContent("b");
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    const confirm = screen.getByText("mode.dialog.confirmRoute");
    expect(confirm.closest("button")).not.toBeDisabled();
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(calls).toEqual([
        { appType: "claude", enabled: true, stack: false, route: "b" },
      ]),
    );
  });

  it("shows only the route target in the status line, not the local address", async () => {
    mockMode("route", "route");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_status`, () =>
        HttpResponse.json({ running: true, address: "127.0.0.1", port: 15721 }),
      ),
    );
    renderPanel("gemini", { route: provider("route") });

    expect(await screen.findByText("→ route")).toBeInTheDocument();
    expect(screen.queryByText(/15721/)).not.toBeInTheDocument();
  });
});

describe("SwitchModePanel — tray needs-route request", () => {
  it("opens the same needs-routing dialog once the provider is there", async () => {
    mockMode("direct", null);
    const onHandled = vi.fn();
    const copilot: Provider = {
      ...provider("copilot"),
      meta: { providerType: "github_copilot" },
    };
    const queryClient = createTestQueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <SwitchModePanel
          app="claude"
          providers={{ kimi: provider("kimi"), copilot }}
          currentProviderId="kimi"
          isLoading={false}
          onSwitch={vi.fn()}
          onOpenRoutingSettings={vi.fn()}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
          needsRouteRequest={{ providerId: "copilot", nonce: 1 }}
          onNeedsRouteHandled={onHandled}
        />
      </QueryClientProvider>,
    );

    expect(
      await screen.findByText("mode.dialog.needsRouteTitle"),
    ).toBeInTheDocument();
    expect(onHandled).toHaveBeenCalledTimes(1);
  });
});

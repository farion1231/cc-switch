import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { AppMode } from "@/types/proxy";
import { SwitchModePanel } from "@/components/providers/mode/SwitchModePanel";
import { server } from "../msw/server";
import { getSettings, setSettings } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";
import type { AppIdWith } from "@/config/appSpecs";

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
  app: AppIdWith<"routeMode">,
  providers: Record<string, Provider>,
  onSwitch = vi.fn(),
  onOpenAggregation = vi.fn(),
) {
  const queryClient = createTestQueryClient();
  const statusSlot = document.createElement("div");
  const view = render(
    <QueryClientProvider client={queryClient}>
      <div
        data-testid="connection-header"
        ref={(node) => {
          node?.appendChild(statusSlot);
        }}
      />
      <SwitchModePanel
        app={app}
        statusSlot={statusSlot}
        providers={providers}
        currentProviderId={Object.keys(providers)[0] ?? ""}
        isLoading={false}
        onSwitch={onSwitch}
        onOpenRoutingSettings={vi.fn()}
        onOpenAggregation={onOpenAggregation}
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
  it("hides per-app Aggregation tab and exposes only Direct and Failover tabs", async () => {
    mockMode("direct", null, "route");
    renderPanel("codex", { route: provider("route") });

    expect(
      await screen.findByRole("button", { name: /mode\.names\.direct/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /mode\.names\.route/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /mode\.names\.stack/ }),
    ).not.toBeInTheDocument();
  });

  it("shows a global Aggregation notice with navigation when active mode is stack", async () => {
    const user = userEvent.setup();
    const onOpenAggregation = vi.fn();
    mockMode("stack", "route");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: true,
          members: [{ providerId: "route", modelIds: ["m"], route: true }],
        }),
      ),
    );
    renderPanel("claude", { route: provider("route") }, vi.fn(), onOpenAggregation);

    const openBtn = await screen.findByRole("button", {
      name: /aggregation\.goToPage|前往聚合页/,
    });
    expect(openBtn).toBeInTheDocument();
    await user.click(openBtn);
    expect(onOpenAggregation).toHaveBeenCalledTimes(1);
  });

  it("warns about Codex clients on an old model list in every mode", async () => {
    const stack = (active: boolean) =>
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active,
          members: [
            {
              providerId: "route",
              modelIds: [],
              route: true,
              memberModels: 0,
            },
          ],
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

describe("SwitchModePanel — Claude Desktop", () => {
  it("offers Direct and Failover tabs, without failover queue or route settings", async () => {
    mockMode("route", "map", "direct");
    let failoverReads = 0;
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_auto_failover_enabled`, () => {
        failoverReads += 1;
        return HttpResponse.json(false);
      }),
      http.post(`${TAURI_ENDPOINT}/get_failover_queue`, () => {
        failoverReads += 1;
        return HttpResponse.json([]);
      }),
    );

    renderPanel("claude-desktop", {
      direct: provider("direct"),
      map: provider("map"),
    });

    expect(
      await screen.findByLabelText("mode.status.routeLead map"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /mode\.names\.direct/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /mode\.names\.stack/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(failoverReads).toBe(0);
  });

  it("shows the Desktop config check in the notice slot", async () => {
    mockMode("route", "map", "direct");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_claude_desktop_status`, () =>
        HttpResponse.json({
          supported: true,
          configured: true,
          mode: "proxy",
          missingRouteMappings: true,
          staleRawModels: false,
          gatewayTokenConfigured: true,
          proxyRunning: true,
        }),
      ),
    );

    renderPanel("claude-desktop", { map: provider("map") });

    expect(
      await screen.findByText("claudeDesktop.statusMissingRouteMappings"),
    ).toBeInTheDocument();
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

  it.each(["route", "stack"] as const)(
    "keeps %s status gray when the service runs but the app is detached",
    async (mode) => {
      mockMode(mode, "a");
      server.use(
        http.post(`${TAURI_ENDPOINT}/get_app_mode`, () =>
          HttpResponse.json({
            mode,
            attached: false,
            routeProviderId: "a",
            directProviderId: "a",
          }),
        ),
      );
      renderPanel("claude", { a: provider("a") });
      await screen.findByLabelText(new RegExp(`mode.status.${mode}Lead`));
      expect(
        screen.getByTestId("connection-header").querySelector(".bg-fg-3"),
      ).not.toBeNull();
      expect(
        screen.getByTestId("connection-header").querySelector(`.bg-${mode}`),
      ).toBeNull();
    },
  );

  it("shows only the route target in the status line, not the local address", async () => {
    mockMode("route", "route");
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_status`, () =>
        HttpResponse.json({ running: true, address: "127.0.0.1", port: 15721 }),
      ),
    );
    renderPanel("gemini", { route: provider("route") });

    expect(
      await screen.findByLabelText("mode.status.routeLead route"),
    ).toHaveTextContent("route");
    expect(screen.getByTestId("connection-header")).toHaveTextContent("route");
    fireEvent.click(screen.getByRole("button", { name: "mode.names.direct" }));
    expect(
      screen.getByLabelText("mode.status.routeLead route"),
    ).toBeInTheDocument();
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

describe("SwitchModePanel — pause the Codex official login", () => {
  const PAUSED_TITLE = "mode.notice.officialLoginPaused";
  const TOGGLE = "settings.codexPauseOfficialLogin.title";

  it("keeps a notice up while paused in route mode and resumes from it", async () => {
    const user = userEvent.setup();
    setSettings({ codexProxyWithoutOfficialLogin: true });
    mockMode("route", "route");
    renderPanel("codex", { route: provider("route") });

    expect(await screen.findByText(PAUSED_TITLE)).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "mode.notice.resumeOfficialLogin" }),
    );

    await waitFor(() =>
      expect(screen.queryByText(PAUSED_TITLE)).not.toBeInTheDocument(),
    );
    expect(getSettings().codexProxyWithoutOfficialLogin).toBe(false);
  });

  it("shows no notice in direct mode, where the pause has no effect", async () => {
    setSettings({ codexProxyWithoutOfficialLogin: true });
    mockMode("direct", "route");
    renderPanel("codex", { route: provider("route") });

    await screen.findByTestId("card-route");
    expect(screen.queryByText(PAUSED_TITLE)).not.toBeInTheDocument();
  });

  it("offers the switch in the route settings for Codex only", async () => {
    const user = userEvent.setup();
    mockMode("route", "route");
    const codex = renderPanel("codex", { route: provider("route") });
    await user.click(
      await screen.findByRole("button", { name: "mode.routeSettings" }),
    );
    expect(
      await screen.findByRole("switch", { name: TOGGLE }),
    ).toBeInTheDocument();
    codex.unmount();

    renderPanel("claude", { route: provider("route") });
    await user.click(
      await screen.findByRole("button", { name: "mode.routeSettings" }),
    );
    await screen.findByText("mode.routeSettingsTitle");
    expect(
      screen.queryByRole("switch", { name: TOGGLE }),
    ).not.toBeInTheDocument();
  });
});

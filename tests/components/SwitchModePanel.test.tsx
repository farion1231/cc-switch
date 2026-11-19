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

const provider = (id: string, official = false): Provider => ({
  id,
  name: id,
  category: official ? "official" : undefined,
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

function mockAuthAccounts(
  authProvider: "claude_official" | "codex_oauth",
  accounts: Array<{ id: string; login: string }>,
  defaultAccountId: string | null = accounts[0]?.id ?? null,
) {
  server.use(
    http.post(`${TAURI_ENDPOINT}/auth_get_status`, async ({ request }) => {
      const { authProvider: requested } = (await request.json()) as {
        authProvider: string;
      };
      if (requested !== authProvider) {
        return HttpResponse.json({
          provider: requested,
          authenticated: false,
          default_account_id: null,
          accounts: [],
        });
      }
      return HttpResponse.json({
        provider: authProvider,
        authenticated: accounts.length > 0,
        default_account_id: defaultAccountId,
        accounts: accounts.map((a) => ({ ...a, authenticated_at: 100 })),
      });
    }),
  );
}

function renderPanel(
  app: AppIdWith<"routeMode">,
  providers: Record<string, Provider>,
  onSwitch = vi.fn(),
  onOpenAggregation = vi.fn(),
  onOpenAuthCenter = vi.fn(),
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
        onOpenAuthCenter={onOpenAuthCenter}
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

const boundOfficial = (
  id: string,
  authProvider: "claude_official" | "codex_oauth",
  accountId: string,
): Provider => ({
  ...provider(id, true),
  meta: {
    authBinding: {
      source: "managed_account",
      authProvider,
      accountId,
    },
  },
});

describe("SwitchModePanel — Direct official login only", () => {
  it("hides Direct/Route/Stack mode tabs and titlebar mode status inside app pages", async () => {
    mockMode("direct", null, "official-1");
    mockAuthAccounts("codex_oauth", [{ id: "acc-1", login: "alice@openai.com" }]);
    renderPanel("codex", {
      "official-1": boundOfficial("official-1", "codex_oauth", "acc-1"),
      external: provider("external", false),
    });

    // Only logged-in official accounts are rendered inside the app page; external providers live in Aggregation
    expect(await screen.findByTestId("card-official-1")).toBeInTheDocument();
    expect(screen.queryByTestId("card-external")).not.toBeInTheDocument();

    expect(
      screen.queryByRole("button", { name: /mode\.names\.direct/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /mode\.names\.route/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /mode\.names\.stack/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText(/mode\.status\.(direct|route|stack)Lead/),
    ).not.toBeInTheDocument();
  });

  it("hides unbound seeded official providers when no account is logged in and routes empty state to Auth Center", async () => {
    const user = userEvent.setup();
    const onOpenAuthCenter = vi.fn();
    mockMode("direct", null, "codex-official");
    mockAuthAccounts("codex_oauth", []);
    renderPanel(
      "codex",
      {
        "codex-official": provider("codex-official", true),
      },
      vi.fn(),
      vi.fn(),
      onOpenAuthCenter,
    );

    expect(
      await screen.findByText("尚未登录官方账号"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("card-codex-official")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "前往授权中心" }));
    expect(onOpenAuthCenter).toHaveBeenCalledTimes(1);
  });

  it("synthesizes and materializes Auth Center accounts on manual direct switch", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    const added: unknown[] = [];
    mockMode("direct", null, "other-direct");
    mockAuthAccounts("claude_official", [
      { id: "acc-alice", login: "alice@anthropic.com" },
    ]);
    server.use(
      http.post(`${TAURI_ENDPOINT}/add_provider`, async ({ request }) => {
        added.push(await request.json());
        return HttpResponse.json(true);
      }),
    );

    renderPanel("claude", {}, onSwitch);

    const switchBtn = await screen.findByTestId(
      "switch-__auth_claude_official_acc-alice",
    );
    await user.click(switchBtn);

    await waitFor(() => {
      expect(added).toHaveLength(1);
      expect(onSwitch).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "claude_official-acc-alice",
          name: "alice@anthropic.com",
          category: "official",
        }),
        undefined,
      );
    });
  });

  it("shows a global Aggregation notice with navigation when active mode is stack or route", async () => {
    const user = userEvent.setup();
    const onOpenAggregation = vi.fn();
    mockMode("stack", "route");
    mockAuthAccounts("claude_official", [
      { id: "acc-1", login: "alice@anthropic.com" },
    ]);
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({
          active: true,
          members: [{ providerId: "route", modelIds: ["m"], route: true }],
        }),
      ),
    );
    renderPanel(
      "claude",
      {
        official: boundOfficial("official", "claude_official", "acc-1"),
        route: provider("route", false),
      },
      vi.fn(),
      onOpenAggregation,
    );

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

    renderPanel("gemini", { a: provider("a", true) });
    await screen.findByTestId("card-a");
    expect(stackReads).toBe(0);
    expect(
      screen.queryByRole("tab", { name: /mode\.stack/ }),
    ).not.toBeInTheDocument();
  });
});

describe("SwitchModePanel — Claude Desktop", () => {
  it("hides mode tabs, titlebar status, failover queue, and route settings", async () => {
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
      direct: provider("direct", true),
      map: provider("map"),
    });

    expect(await screen.findByTestId("card-direct")).toBeInTheDocument();
    expect(
      screen.queryByLabelText(/mode\.status\.(direct|route|stack)Lead/),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /mode\.names\.direct/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /mode\.names\.route/ }),
    ).not.toBeInTheDocument();
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

  it("allows exiting back to Direct official login from Aggregation takeover notice", async () => {
    mockMode("route", "external", "official");
    const calls = captureTakeover();
    renderPanel("claude", {
      official: provider("official", true),
      external: provider("external", false),
    });

    fireEvent.click(
      await screen.findByRole("button", { name: "mode.notice.backToDirect" }),
    );
    expect(await screen.findByText("mode.dialog.exitTitle")).toBeVisible();
    fireEvent.click(screen.getByText("mode.dialog.confirmDirect"));
    await waitFor(() =>
      expect(calls).toEqual([
        { appType: "claude", enabled: false, stack: false, route: null },
      ]),
    );
  });

  it.each(["direct", "route", "stack"] as const)(
    "never renders titlebar mode status in %s mode",
    async (mode) => {
      mockMode(mode, "a");
      renderPanel("gemini", { a: provider("a", true) });
      await screen.findByTestId("card-a");
      expect(
        screen.queryByLabelText(/mode\.status\.(direct|route|stack)Lead/),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: /mode\.names\.(direct|route|stack)/ }),
      ).not.toBeInTheDocument();
    },
  );
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
    mockMode("direct", "official");
    mockAuthAccounts("codex_oauth", [{ id: "acc-1", login: "alice@openai.com" }]);
    renderPanel("codex", {
      official: boundOfficial("official", "codex_oauth", "acc-1"),
    });

    await screen.findByTestId("card-official");
    expect(screen.queryByText(PAUSED_TITLE)).not.toBeInTheDocument();
  });

  it("does not expose per-app route settings button inside SwitchModePanel", async () => {
    mockMode("route", "route");
    mockAuthAccounts("codex_oauth", [{ id: "acc-1", login: "alice@openai.com" }]);
    renderPanel("codex", {
      official: boundOfficial("official", "codex_oauth", "acc-1"),
      route: provider("route"),
    });
    await screen.findByTestId("card-official");
    expect(
      screen.queryByRole("button", { name: "mode.routeSettings" }),
    ).not.toBeInTheDocument();
  });
});

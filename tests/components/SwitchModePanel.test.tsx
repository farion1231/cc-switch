import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider, focusManager } from "@tanstack/react-query";
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

// 只关心模式层：卡片画成按钮，按 presentation 的 key 找
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

function renderPanel(app: ProxyAppId, providers: Record<string, Provider>) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <SwitchModePanel
        app={app}
        providers={providers}
        currentProviderId={Object.keys(providers)[0] ?? ""}
        isLoading={false}
        onSwitch={vi.fn()}
        onOpenRoutingSettings={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.mocked(toast.info).mockClear();
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

  it("reminds to restart Claude Code when Stack models change outside add / remove", async () => {
    mockMode("stack", "route");
    let members = [
      { providerId: "route", modelIds: ["m-route-1"], route: true },
      { providerId: "kimi", modelIds: ["m-kimi"], route: false },
    ];
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: true, members }),
      ),
      http.post(`${TAURI_ENDPOINT}/set_proxy_stack_member`, () =>
        HttpResponse.json(null),
      ),
    );

    renderPanel("claude", { route: provider("route"), kimi: provider("kimi") });

    // 移出名单：保存成功的提示已经说了要重启，不再提示
    const remove = await screen.findByTestId("remove-kimi");
    members = [members[0]];
    fireEvent.click(remove);
    await screen.findByTestId("add-kimi");
    expect(toast.info).not.toHaveBeenCalled();

    // 别处改了默认那家的模型：回到窗口时重查，提示重启
    members = [
      {
        providerId: "route",
        modelIds: ["m-route-1", "m-route-2"],
        route: true,
      },
    ];
    try {
      act(() => {
        focusManager.setFocused(false);
        focusManager.setFocused(true);
      });
      await waitFor(() =>
        expect(toast.info).toHaveBeenCalledWith(
          "provider.stackModelsChanged",
          expect.anything(),
        ),
      );
    } finally {
      focusManager.setFocused(undefined);
    }
  });

  it("warns about Codex clients on an old model list only in Stack mode", async () => {
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
    await screen.findByTestId("card-route");
    expect(
      screen.queryByText("proxy.stackMode.codexStale.title"),
    ).not.toBeInTheDocument();
  });

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
    // 行上只有名单的添加：没有「设为默认」，也没有会切模式的按钮
    const row = await screen.findByTestId("card-kimi");
    expect(
      Array.from(row.querySelectorAll("button")).map((b) => b.dataset.testid),
    ).toEqual(["add-kimi"]);
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

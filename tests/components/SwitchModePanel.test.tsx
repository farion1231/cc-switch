import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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

function mockMode(mode: AppMode, routeProviderId: string | null) {
  server.use(
    http.post(`${TAURI_ENDPOINT}/get_app_mode`, () =>
      HttpResponse.json({
        mode,
        attached: mode !== "direct",
        routeProviderId,
        directProviderId: routeProviderId,
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

  it("starts Stack from a preview row with that provider as the fixed default", async () => {
    mockMode("direct", null);
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: false, members: [] }),
      ),
    );
    const calls = captureTakeover();
    renderPanel("claude", { a: provider("a"), kimi: provider("kimi") });

    fireEvent.click(
      await screen.findByRole("button", { name: /mode\.names\.stack/ }),
    );
    // 名单的添加照旧能点；没有不能点的「设为默认」
    expect(await screen.findByTestId("add-kimi")).not.toBeDisabled();
    expect(screen.queryByTestId("setDefault-kimi")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("startStackFrom-kimi"));

    expect(await screen.findByText("mode.dialog.stackTitle")).toBeVisible();
    // 默认那家就是点的这家，框里不再让选
    expect(screen.getByTestId("stack-default")).toHaveTextContent("kimi");
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("mode.dialog.confirmStack"));
    await waitFor(() =>
      expect(calls).toEqual([
        { appType: "claude", enabled: true, stack: true, route: "kimi" },
      ]),
    );
  });

  it("enters routing without an acknowledgement checkbox", async () => {
    mockMode("direct", null);
    const calls = captureTakeover();
    renderPanel("claude", { a: provider("a"), b: provider("b") });

    fireEvent.click(
      await screen.findByRole("button", { name: /mode\.names\.route/ }),
    );
    fireEvent.click(await screen.findByTestId("startRouteFrom-b"));
    expect(await screen.findByText("mode.dialog.routeTitle")).toBeVisible();
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

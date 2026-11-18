import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import type { UniversalProvider } from "@/types";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { AggregationPage } from "@/components/aggregation/AggregationPage";
import { AggregationProviderModal } from "@/components/aggregation/AggregationProviderModal";
import { server } from "../msw/server";
import { getSettings, setSettings } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

const TAURI_ENDPOINT = "http://tauri.local";

beforeEach(() => {
  const base = getSettings();
  setSettings({
    ...base,
    visibleApps: { ...DEFAULT_VISIBLE_APPS },
  });
});

function mockOverview(enabled = false) {
  const toggleCalls: unknown[] = [];
  const syncCalls: unknown[] = [];
  const unsyncCalls: unknown[] = [];

  const overviewPayload = (isEnabled: boolean) => ({
    enabled: isEnabled,
    proxyRunning: isEnabled,
    proxyAddress: "127.0.0.1",
    proxyPort: 15721,
    universalProviders: [
      {
        id: "univ-1",
        name: "Main Gateway",
        providerType: "custom_gateway",
        apps: {
          claude: true,
          claudeDesktop: true,
          codex: true,
          gemini: true,
          grokbuild: true,
          opencode: true,
          openclaw: true,
          hermes: true,
          pi: true,
          omp: true,
          mcode: true,
          kimi: true,
          dsh: true,
          copilot: true,
          workbuddy: true,
        },
        baseUrl: "https://relay.example.com",
        apiKey: "sk-1234567890abcd",
        apiFormat: "openai_chat",
        extraModels: ["gpt-4o"],
        models: {},
      },
    ],
    appProviders: [
      {
        appType: "claude",
        provider: {
          id: "relay-claude",
          name: "Claude Relay",
          settingsConfig: {},
        },
        baseUrl: "https://claude-relay.example.com",
        apiFormat: "anthropic",
        models: ["claude-sonnet-4-20250514"],
        isOfficial: false,
        canSync: true,
        isCurrentInApp: true,
        inStack: isEnabled,
        syncedApps: ["claude", "codex"],
      },
      {
        appType: "claude",
        provider: {
          id: "default",
          name: "default",
          settingsConfig: {},
        },
        baseUrl: "https://missing-key.example.com",
        apiFormat: "anthropic",
        models: [],
        isOfficial: false,
        canSync: false,
        isCurrentInApp: false,
        inStack: false,
        syncedApps: ["claude"],
      },
    ],
    appStates: [
      {
        appType: "claude",
        supportsLocalProxy: true,
        supportsStackCatalog: true,
        additive: false,
        mode: isEnabled ? "stack" : "direct",
        attached: isEnabled,
        routeProviderId: "relay-claude",
        stackMemberCount: isEnabled ? 1 : 0,
        syncedProviderCount: 2,
      },
      {
        appType: "opencode",
        supportsLocalProxy: false,
        supportsStackCatalog: false,
        additive: true,
        mode: "additive",
        attached: false,
        routeProviderId: null,
        stackMemberCount: 0,
        syncedProviderCount: 1,
      },
    ],
  });

  server.use(
    http.post(`${TAURI_ENDPOINT}/get_aggregation_overview`, () =>
      HttpResponse.json(overviewPayload(enabled)),
    ),
    http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
      HttpResponse.json({ active: enabled, members: [] }),
    ),
    http.post(
      `${TAURI_ENDPOINT}/set_global_aggregation_mode`,
      async ({ request }) => {
        const body = (await request.json()) as { enabled: boolean };
        toggleCalls.push(body);
        return HttpResponse.json(overviewPayload(body.enabled));
      },
    ),
    http.post(
      `${TAURI_ENDPOINT}/sync_aggregation_provider_to_all_apps`,
      async ({ request }) => {
        syncCalls.push(await request.json());
        return HttpResponse.json({
          syncedApps: ["claude", "codex", "gemini"],
          warnings: [],
        });
      },
    ),
    http.post(
      `${TAURI_ENDPOINT}/unsync_aggregation_provider`,
      async ({ request }) => {
        unsyncCalls.push(await request.json());
        return HttpResponse.json({
          syncedApps: ["codex"],
          warnings: [],
        });
      },
    ),
  );

  return { toggleCalls, syncCalls, unsyncCalls };
}

function renderPage() {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <AggregationPage
        onOpenApp={vi.fn()}
        onOpenRoutingSettings={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("AggregationPage", () => {
  it("renders a single global switch, masks API key, and folds missing-credential providers", async () => {
    const user = userEvent.setup();
    const { toggleCalls } = mockOverview(false);
    renderPage();

    const switches = await screen.findAllByRole("switch");
    expect(switches).toHaveLength(1);
    expect(switches[0]).not.toBeChecked();
    expect(
      screen.queryByRole("button", { name: /立即开启聚合模式|turnOnNow/ }),
    ).not.toBeInTheDocument();

    expect(await screen.findByText("Main Gateway")).toBeInTheDocument();
    expect(screen.getByText("sk-1•••abcd")).toBeInTheDocument();
    expect(screen.getByText("Claude Relay")).toBeInTheDocument();

    // 默认折叠仅原生可用/缺少凭据条目，展开后显示原因
    expect(screen.queryByText("default")).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: /仅原生应用可用|secondarySectionTitle/,
      }),
    );
    expect(await screen.findByText("default")).toBeInTheDocument();

    await user.click(switches[0]);
    await waitFor(() =>
      expect(toggleCalls).toEqual([{ enabled: true }]),
    );
  });

  it("syncs a ready app provider across all compatible apps", async () => {
    const user = userEvent.setup();
    const { syncCalls, unsyncCalls } = mockOverview(true);
    renderPage();

    const syncButtons = await screen.findAllByRole("button", {
      name: /^aggregation\.syncAllApps$|^兼容全|^Sync to all apps$/,
    });
    expect(syncButtons.length).toBeGreaterThanOrEqual(2);

    // 累加式应用在全局聚合开启且已同步时，与路由类应用保持一致显示“已接入”
    const attachedBadges = screen.getAllByText(
      /^(已接入|Connected|aggregation\.appAttached)$/,
    );
    expect(attachedBadges.length).toBe(2);

    await user.click(syncButtons[1]);
    await waitFor(() =>
      expect(syncCalls).toEqual([
        { sourceApp: "claude", providerId: "relay-claude" },
      ]),
    );

    const unsyncButtons = await screen.findAllByRole("button", {
      name: /取消兼容|Cancel compatibility|aggregation\.unsyncApps/,
    });
    expect(unsyncButtons.length).toBe(2);

    await user.click(unsyncButtons[0]);
    await user.click(unsyncButtons[1]);
    await waitFor(() =>
      expect(unsyncCalls).toEqual([
        { sourceApp: null, providerId: "univ-1" },
        { sourceApp: "claude", providerId: "relay-claude" },
      ]),
    );
  });

  it("supports preset search & simplified key-only creation while hiding disabled apps", async () => {
    const user = userEvent.setup();
    const base = getSettings();
    setSettings({
      ...base,
      visibleApps: {
        ...DEFAULT_VISIBLE_APPS,
        claude: true,
        codex: false,
        opencode: false,
      },
    });
    const saveMock = vi.fn<(provider: UniversalProvider) => Promise<void>>(
      async () => {},
    );

    server.use(
      http.post(`${TAURI_ENDPOINT}/get_aggregation_overview`, () =>
        HttpResponse.json({
          enabled: true,
          proxyRunning: true,
          proxyAddress: "127.0.0.1",
          proxyPort: 15721,
          universalProviders: [],
          appProviders: [
            {
              appType: "claude",
              provider: { id: "c1", name: "Visible Claude", settingsConfig: {} },
              baseUrl: "https://claude.example.com",
              apiFormat: "anthropic",
              models: ["claude-sonnet-4-20250514"],
              isOfficial: false,
              canSync: true,
              isCurrentInApp: true,
              inStack: true,
              syncedApps: ["claude", "codex"],
            },
            {
              appType: "codex",
              provider: { id: "x1", name: "Hidden Codex", settingsConfig: {} },
              baseUrl: "https://codex.example.com",
              apiFormat: "openai_responses",
              models: ["gpt-4o"],
              isOfficial: false,
              canSync: true,
              isCurrentInApp: false,
              inStack: false,
              syncedApps: ["codex"],
            },
          ],
          appStates: [
            {
              appType: "claude",
              supportsLocalProxy: true,
              supportsStackCatalog: true,
              additive: false,
              mode: "stack",
              attached: true,
              routeProviderId: "c1",
              stackMemberCount: 1,
              syncedProviderCount: 1,
            },
            {
              appType: "opencode",
              supportsLocalProxy: false,
              supportsStackCatalog: false,
              additive: true,
              mode: "additive",
              attached: false,
              routeProviderId: null,
              stackMemberCount: 0,
              syncedProviderCount: 1,
            },
          ],
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: true, members: [] }),
      ),
    );

    const queryClient = createTestQueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <div data-testid="agg-page">
          <AggregationPage
            onOpenApp={vi.fn()}
            onOpenRoutingSettings={vi.fn()}
          />
        </div>
        <AggregationProviderModal
          isOpen
          onClose={vi.fn()}
          onSaveAndSync={saveMock}
        />
      </QueryClientProvider>,
    );

    const pageContainer = within(screen.getByTestId("agg-page"));
    expect(await pageContainer.findByText("Visible Claude")).toBeInTheDocument();
    expect(pageContainer.queryByText("Hidden Codex")).not.toBeInTheDocument();
    expect(pageContainer.queryByText("OpenCode")).not.toBeInTheDocument();
    expect(
      pageContainer.queryByRole("button", { name: /Codex 聚合设置|codexStackSettings/ }),
    ).not.toBeInTheDocument();

    // 在预设选择面板中搜索「深度求索」并选中 DeepSeek（只需填写 API Key！）
    await user.type(
      screen.getByPlaceholderText(/搜索 DeepSeek|Search DeepSeek/),
      "深度求索",
    );
    await user.click(screen.getByRole("button", { name: /DeepSeek/ }));

    // 主视图只要求填 Key（名称与 URL 默认折叠在高级配置里）
    expect(screen.queryByPlaceholderText("https://api.example.com")).not.toBeInTheDocument();
    await user.type(screen.getByPlaceholderText("sk-..."), "sk-deepseek-test");
    await user.click(
      screen.getByRole("button", { name: /保存并.*|saveAndSync|Save and sync/i }),
    );

    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(1));
    const saved = saveMock.mock.calls[0][0];
    expect(saved.name).toBe("DeepSeek");
    expect(saved.baseUrl).toBe("https://api.deepseek.com/anthropic");
    expect(saved.apiKey).toBe("sk-deepseek-test");
    expect(saved.apps.claude).toBe(true);
    expect(saved.apps.codex).toBe(false);
    expect(saved.apps.opencode).toBe(false);
  });

  it("supports fetching models and advanced endpoint settings in AggregationProviderModal", async () => {
    const user = userEvent.setup();
    const saveMock = vi.fn<(provider: UniversalProvider) => Promise<void>>(
      async () => {},
    );
    const fetchPayloads: unknown[] = [];

    server.use(
      http.post(
        `${TAURI_ENDPOINT}/fetch_models_for_config`,
        async ({ request }) => {
          fetchPayloads.push(await request.json());
          return HttpResponse.json([
            { id: "deepseek-r1", ownedBy: "deepseek" },
            { id: "gpt-5.4", ownedBy: "openai" },
          ]);
        },
      ),
    );

    const queryClient = createTestQueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <AggregationProviderModal
          isOpen
          onClose={vi.fn()}
          onSaveAndSync={saveMock}
        />
      </QueryClientProvider>,
    );

    await user.click(
      screen.getByRole("button", { name: /自定义配置|Custom provider/ }),
    );
    await user.type(
      screen.getByPlaceholderText(/主用中转站|Primary Gateway/),
      "YOSHI",
    );
    await user.type(
      screen.getByPlaceholderText("https://api.example.com"),
      "https://yoshiyoshi.qzz.io",
    );
    await user.type(screen.getByPlaceholderText("sk-..."), "sk-yoshi");

    // 点击“获取模型列表”并勾选全部返回模型
    await user.click(
      screen.getByRole("button", {
        name: /获取模型|Fetch models|providerForm\.fetchModels/i,
      }),
    );

    await waitFor(() => expect(fetchPayloads.length).toBe(1));
    expect(fetchPayloads[0]).toMatchObject({
      baseUrl: "https://yoshiyoshi.qzz.io",
      apiKey: "sk-yoshi",
      apiFormat: "openai_chat",
    });

    const applyBtn = await screen.findByRole("button", {
      name: /应用|Apply|providerForm\.modelPicker\.apply/i,
    });
    await user.click(await screen.findByText("deepseek-r1"));
    await user.click(applyBtn);

    // 展开高级配置并启用“完整端点 URL”
    await user.click(
      screen.getByRole("button", { name: /高级配置|Advanced settings/ }),
    );
    await user.click(
      screen.getByRole("switch", {
        name: /完整端点 URL|Full endpoint URL/,
      }),
    );

    await user.click(
      screen.getByRole("button", { name: /保存并.*|saveAndSync|Save and sync/i }),
    );

    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(1));
    const saved = saveMock.mock.calls[0][0];
    expect(saved.name).toBe("YOSHI");
    expect(saved.extraModels).toContain("deepseek-r1");
    expect(saved.meta?.isFullUrl).toBe(true);
  });
});

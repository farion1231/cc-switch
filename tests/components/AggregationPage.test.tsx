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
  const assignCalls: unknown[] = [];
  const deleteCalls: unknown[] = [];

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
          claudeDesktop: false,
          codex: true,
          gemini: false,
          grokbuild: false,
          opencode: false,
          openclaw: false,
          hermes: false,
          pi: false,
          omp: false,
          mcode: false,
          kimi: false,
          dsh: false,
          copilot: false,
          workbuddy: false,
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
        syncedApps: ["claude"],
      },
      {
        appType: "claude",
        provider: {
          id: "claude-official",
          name: "Claude Official",
          category: "official",
          settingsConfig: {},
        },
        baseUrl: null,
        apiFormat: "official",
        models: [],
        isOfficial: true,
        canSync: false,
        isCurrentInApp: false,
        inStack: false,
        syncedApps: ["claude"],
      },
      {
        appType: "claude",
        provider: {
          id: "incomplete-1",
          name: "Incomplete Relay",
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
        syncedProviderCount: 1,
        assignedProviderCount: 2,
      },
      {
        appType: "codex",
        supportsLocalProxy: true,
        supportsStackCatalog: true,
        additive: false,
        mode: isEnabled ? "stack" : "direct",
        attached: isEnabled,
        routeProviderId: "universal-codex-univ-1",
        stackMemberCount: isEnabled ? 1 : 0,
        syncedProviderCount: 1,
        assignedProviderCount: 1,
      },
      {
        appType: "gemini",
        supportsLocalProxy: true,
        supportsStackCatalog: false,
        additive: false,
        mode: "direct",
        attached: false,
        routeProviderId: null,
        stackMemberCount: 0,
        syncedProviderCount: 0,
        assignedProviderCount: 0,
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
        syncedProviderCount: 0,
        assignedProviderCount: 0,
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
      `${TAURI_ENDPOINT}/set_aggregation_provider_apps`,
      async ({ request }) => {
        assignCalls.push(await request.json());
        return HttpResponse.json({
          syncedApps: ["claude", "gemini"],
          warnings: [],
        });
      },
    ),
    http.post(
      `${TAURI_ENDPOINT}/delete_aggregation_provider`,
      async ({ request }) => {
        deleteCalls.push(await request.json());
        return HttpResponse.json({
          syncedApps: ["claude", "codex"],
          warnings: [],
        });
      },
    ),
  );

  return { toggleCalls, assignCalls, deleteCalls };
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
  it("renders external providers with per-app assignments, excludes official accounts, and folds incomplete items", async () => {
    const user = userEvent.setup();
    const { toggleCalls } = mockOverview(false);
    renderPage();

    expect(await screen.findByText("Main Gateway")).toBeInTheDocument();
    expect(screen.getByText("Claude Relay")).toBeInTheDocument();
    // Masked API key
    expect(screen.getByText("sk-1•••abcd")).toBeInTheDocument();
    // Official login accounts must NOT appear on the Aggregation page
    expect(screen.queryByText("Claude Official")).not.toBeInTheDocument();
    // No "sync to all apps" button anymore
    expect(
      screen.queryByRole("button", { name: /兼容全应用|兼容全部应用|syncAllApps/ }),
    ).not.toBeInTheDocument();

    // Incomplete external provider is folded by default
    expect(screen.queryByText("Incomplete Relay")).not.toBeInTheDocument();
    const foldBtn = screen.getByRole("button", {
      name: /待补全凭据的外部供应商|incompleteSectionTitle/,
    });
    await user.click(foldBtn);
    expect(screen.getByText("Incomplete Relay")).toBeInTheDocument();

    // Global switch toggles aggregation mode
    const toggle = screen.getByRole("switch", {
      name: /开启聚合模式|enableMode/,
    });
    await user.click(toggle);
    await waitFor(() => expect(toggleCalls).toEqual([{ enabled: true }]));
  });

  it("allows assigning specific apps to a provider inline instead of syncing to all apps", async () => {
    const user = userEvent.setup();
    const { assignCalls } = mockOverview(true);
    renderPage();

    expect(await screen.findByText("Main Gateway")).toBeInTheDocument();

    const assignBtns = screen.getAllByRole("button", {
      name: /分配应用|assignApps/,
    });
    await user.click(assignBtns[0]);

    // Currently assigned: claude + codex. Uncheck Codex, check Gemini.
    const codexToggle = await screen.findByRole("button", {
      name: "Codex",
      pressed: true,
    });
    await user.click(codexToggle);

    const geminiToggle = screen.getByRole("button", {
      name: "Gemini CLI",
      pressed: false,
    });
    await user.click(geminiToggle);

    await user.click(
      screen.getByRole("button", { name: /保存分配|saveAssignment/ }),
    );

    await waitFor(() =>
      expect(assignCalls).toEqual([
        {
          sourceApp: null,
          providerId: "univ-1",
          targetApps: ["claude", "gemini"],
        },
      ]),
    );
  });

  it("deletes an external provider via delete_aggregation_provider", async () => {
    const user = userEvent.setup();
    const { deleteCalls } = mockOverview(false);
    renderPage();

    expect(await screen.findByText("Main Gateway")).toBeInTheDocument();

    const deleteBtns = screen.getAllByRole("button", {
      name: /common\.delete|删除/,
    });
    await user.click(deleteBtns[0]);

    const dialog = await screen.findByRole("dialog");
    await user.click(
      within(dialog).getByRole("button", { name: /common\.confirm|确定/ }),
    );

    await waitFor(() =>
      expect(deleteCalls).toEqual([
        {
          sourceApp: null,
          providerId: "univ-1",
        },
      ]),
    );
  });

  it("filters providers by assigned app pill", async () => {
    const user = userEvent.setup();
    mockOverview(false);
    renderPage();

    expect(await screen.findByText("Main Gateway")).toBeInTheDocument();
    expect(screen.getByText("Claude Relay")).toBeInTheDocument();

    // Filter by Codex: only Main Gateway is assigned to Codex, Claude Relay is not
    await user.click(screen.getByRole("button", { name: /^Codex \(1\)$/ }));
    expect(screen.getByText("Main Gateway")).toBeInTheDocument();
    expect(screen.queryByText("Claude Relay")).not.toBeInTheDocument();
  });

  it("AggregationProviderModal shows Target Apps directly in main form and defaults new providers to none selected", async () => {
    const user = userEvent.setup();
    const saved: UniversalProvider[] = [];
    const queryClient = createTestQueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <AggregationProviderModal
          isOpen
          onClose={vi.fn()}
          onSaveAndSync={async (p) => {
            saved.push(p);
          }}
        />
      </QueryClientProvider>,
    );

    // Step 1: open custom config
    await user.click(
      screen.getByRole("button", { name: /自定义配置|customProvider/ }),
    );

    // Target apps section must be visible immediately (NOT hidden in Advanced)
    expect(
      screen.getByText(/指定适用应用|targetAppsTitle/),
    ).toBeInTheDocument();

    // Defaults to 0 selected apps -> submit button says "仅保存供应商"
    expect(
      screen.getByRole("button", { name: /仅保存供应商|saveOnly/ }),
    ).toBeInTheDocument();

    await user.type(screen.getByLabelText(/provider\.name|名称/), "My Relay");
    await user.type(
      screen.getByLabelText(/universalProvider\.baseUrl|API 地址/),
      "https://my-relay.example.com",
    );
    await user.type(
      screen.getByLabelText(/universalProvider\.apiKey|API Key/),
      "sk-test-secret",
    );

    // Explicitly choose ONLY Claude Code and Codex
    await user.click(
      screen.getByRole("button", { name: "Claude Code", pressed: false }),
    );
    await user.click(
      screen.getByRole("button", { name: "Codex", pressed: false }),
    );

    const submitBtn = screen.getByRole("button", {
      name: /保存并应用到 2 个应用|saveAndApply/,
    });
    await user.click(submitBtn);

    await waitFor(() => expect(saved).toHaveLength(1));
    expect(saved[0].apps.claude).toBe(true);
    expect(saved[0].apps.codex).toBe(true);
    expect(saved[0].apps.gemini).toBe(false);
    expect(saved[0].apps.opencode).toBe(false);
  });

  it("requires manual per-app connection instead of auto-attaching Claude Code and Codex", async () => {
    const user = userEvent.setup();
    const appModeCalls: unknown[] = [];

    const baseOverview = (globalEnabled: boolean, claudeAttached: boolean) => ({
      enabled: globalEnabled,
      proxyRunning: claudeAttached,
      proxyAddress: "127.0.0.1",
      proxyPort: 15721,
      universalProviders: [],
      appProviders: [],
      appStates: [
        {
          appType: "claude",
          supportsLocalProxy: true,
          supportsStackCatalog: true,
          additive: false,
          mode: claudeAttached ? "stack" : "direct",
          attached: claudeAttached,
          routeProviderId: "agg-claude-u1",
          stackMemberCount: claudeAttached ? 1 : 0,
          syncedProviderCount: 1,
          assignedProviderCount: 1,
        },
        {
          appType: "codex",
          supportsLocalProxy: true,
          supportsStackCatalog: true,
          additive: false,
          mode: "direct",
          attached: false,
          routeProviderId: "agg-codex-u1",
          stackMemberCount: 0,
          syncedProviderCount: 1,
          assignedProviderCount: 1,
        },
      ],
    });

    let globalEnabled = false;
    let claudeAttached = false;

    server.use(
      http.post(`${TAURI_ENDPOINT}/get_aggregation_overview`, () =>
        HttpResponse.json(baseOverview(globalEnabled, claudeAttached)),
      ),
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: claudeAttached, members: [] }),
      ),
      http.post(
        `${TAURI_ENDPOINT}/set_global_aggregation_mode`,
        async ({ request }) => {
          const body = (await request.json()) as { enabled: boolean };
          globalEnabled = body.enabled;
          if (!globalEnabled) {
            claudeAttached = false;
          }
          return HttpResponse.json(baseOverview(globalEnabled, claudeAttached));
        },
      ),
      http.post(
        `${TAURI_ENDPOINT}/set_aggregation_app_mode`,
        async ({ request }) => {
          const body = (await request.json()) as {
            appType: string;
            enabled: boolean;
            routeProviderId?: string | null;
          };
          appModeCalls.push(body);
          if (body.appType === "claude") {
            claudeAttached = body.enabled;
          }
          return HttpResponse.json(null);
        },
      ),
    );

    renderPage();

    // Global mode OFF -> both attach buttons are disabled
    const attachBtnsOff = await screen.findAllByRole("button", {
      name: /接入|Connect|attachApp/,
    });
    expect(attachBtnsOff).toHaveLength(2);
    expect(attachBtnsOff[0]).toBeDisabled();
    expect(attachBtnsOff[1]).toBeDisabled();

    // Turn ON global Aggregation -> Claude & Codex remain unattached (still showing Connect/接入, now enabled)
    await user.click(
      screen.getByRole("switch", { name: /开启聚合模式|enableMode/ }),
    );
    await waitFor(() => {
      const btns = screen.getAllByRole("button", {
        name: /接入|Connect|attachApp/,
      });
      expect(btns).toHaveLength(2);
      expect(btns[0]).toBeEnabled();
      expect(btns[1]).toBeEnabled();
    });
    expect(appModeCalls).toHaveLength(0);

    // Manually connect ONLY Claude Code
    const attachBtnsOn = screen.getAllByRole("button", {
      name: /接入|Connect|attachApp/,
    });
    await user.click(attachBtnsOn[0]);

    await waitFor(() =>
      expect(appModeCalls).toEqual([
        {
          appType: "claude",
          enabled: true,
          routeProviderId: "agg-claude-u1",
        },
      ]),
    );
  });

  it("shows Unassigned instead of Official direct for non-official-login apps like pi", async () => {
    const base = getSettings();
    setSettings({
      ...base,
      visibleApps: {
        ...DEFAULT_VISIBLE_APPS,
        gemini: true,
        pi: true,
      },
    });

    server.use(
      http.post(`${TAURI_ENDPOINT}/get_aggregation_overview`, () =>
        HttpResponse.json({
          enabled: false,
          proxyRunning: false,
          proxyAddress: "127.0.0.1",
          proxyPort: 15721,
          universalProviders: [],
          appProviders: [],
          appStates: [
            {
              appType: "gemini",
              supportsLocalProxy: true,
              supportsStackCatalog: false,
              additive: false,
              mode: "direct",
              attached: false,
              routeProviderId: null,
              stackMemberCount: 0,
              syncedProviderCount: 0,
              assignedProviderCount: 0,
            },
            {
              appType: "pi",
              supportsLocalProxy: false,
              supportsStackCatalog: false,
              additive: true,
              mode: "additive",
              attached: false,
              routeProviderId: null,
              stackMemberCount: 0,
              syncedProviderCount: 0,
              assignedProviderCount: 0,
            },
          ],
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: false, members: [] }),
      ),
    );

    renderPage();

    const geminiChip = (
      await screen.findByRole("button", { name: "Gemini CLI" })
    ).parentElement!;
    expect(geminiChip).toHaveTextContent(/官方直连|Official direct/);

    const piChip = screen.getByRole("button", { name: "Pi" }).parentElement!;
    expect(piChip).toHaveTextContent(/未分配|Unassigned/);
    expect(piChip).not.toHaveTextContent(/官方直连|Official direct/);
  });

  it("allows cancelling the last connected app (including additive apps like OpenCode) and refreshes provider apps and counts", async () => {
    const user = userEvent.setup();
    const base = getSettings();
    setSettings({
      ...base,
      visibleApps: {
        ...DEFAULT_VISIBLE_APPS,
        codex: true,
        opencode: true,
      },
    });

    const appModeCalls: unknown[] = [];
    let codexAssigned = true;
    let codexAttached = true;
    let opencodeAssigned = true;

    const buildOverview = () => ({
      enabled: true,
      proxyRunning: codexAttached,
      proxyAddress: "127.0.0.1",
      proxyPort: 15721,
      universalProviders: [
        {
          id: "u-1",
          name: "Relay One",
          providerType: "custom_gateway",
          apps: {
            claude: false,
            claudeDesktop: false,
            codex: codexAssigned,
            gemini: false,
            grokbuild: false,
            opencode: opencodeAssigned,
            openclaw: false,
            hermes: false,
            pi: false,
            omp: false,
            mcode: false,
            kimi: false,
            dsh: false,
            copilot: false,
            workbuddy: false,
          },
          baseUrl: "https://relay.example.com",
          apiKey: "sk-test-1234",
          apiFormat: "openai_responses",
          extraModels: ["gpt-5"],
          models: {},
        },
      ],
      appProviders: [],
      appStates: [
        {
          appType: "codex",
          supportsLocalProxy: true,
          supportsStackCatalog: true,
          additive: false,
          mode: codexAttached ? "stack" : "direct",
          attached: codexAttached,
          routeProviderId: codexAssigned ? "universal-codex-u-1" : null,
          stackMemberCount: codexAttached ? 1 : 0,
          syncedProviderCount: codexAssigned ? 1 : 0,
          assignedProviderCount: codexAssigned ? 1 : 0,
        },
        {
          appType: "opencode",
          supportsLocalProxy: false,
          supportsStackCatalog: false,
          additive: true,
          mode: "additive",
          attached: opencodeAssigned,
          routeProviderId: null,
          stackMemberCount: 0,
          syncedProviderCount: opencodeAssigned ? 1 : 0,
          assignedProviderCount: opencodeAssigned ? 1 : 0,
        },
      ],
    });

    server.use(
      http.post(`${TAURI_ENDPOINT}/get_aggregation_overview`, () =>
        HttpResponse.json(buildOverview()),
      ),
      http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
        HttpResponse.json({ active: codexAttached, members: [] }),
      ),
      http.post(
        `${TAURI_ENDPOINT}/set_aggregation_app_mode`,
        async ({ request }) => {
          const body = (await request.json()) as {
            appType: string;
            enabled: boolean;
          };
          appModeCalls.push(body);
          if (body.appType === "codex" && !body.enabled) {
            codexAttached = false;
            codexAssigned = false;
          }
          if (body.appType === "opencode" && !body.enabled) {
            opencodeAssigned = false;
          }
          return HttpResponse.json(null);
        },
      ),
    );

    renderPage();

    // Both Codex and OpenCode have clickable 已接入 / Connected buttons
    const attachedBtns = await screen.findAllByRole("button", {
      name: /已接入|Connected|appAttached/,
    });
    expect(attachedBtns).toHaveLength(2);
    expect(screen.getByRole("button", { name: /^Codex \(1\)$/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^OpenCode \(1\)$/ })).toBeInTheDocument();

    // Disconnect Codex first
    await user.click(attachedBtns[0]);
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /^Codex \(1\)$/ })).not.toBeInTheDocument(),
    );
    const codexChip = screen.getByRole("button", { name: "Codex" }).parentElement!;
    expect(codexChip).toHaveTextContent(/官方直连|Official direct/);

    // Now OpenCode is the LAST connected ("已接入") button — must still be clickable and cancellable!
    const remainingAttached = screen.getAllByRole("button", {
      name: /已接入|Connected|appAttached/,
    });
    expect(remainingAttached).toHaveLength(1);
    await user.click(remainingAttached[0]);

    await waitFor(() =>
      expect(appModeCalls).toEqual([
        { appType: "codex", enabled: false, routeProviderId: "universal-codex-u-1" },
        { appType: "opencode", enabled: false, routeProviderId: null },
      ]),
    );
    expect(screen.queryByRole("button", { name: /^OpenCode \(1\)$/ })).not.toBeInTheDocument();
    const opencodeChip = screen.getByRole("button", { name: "OpenCode" }).parentElement!;
    expect(opencodeChip).toHaveTextContent(/未分配|Unassigned/);
    expect(screen.getByText(/未分配应用|No apps assigned/)).toBeInTheDocument();
  });
});

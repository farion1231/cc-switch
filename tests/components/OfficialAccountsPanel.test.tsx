import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import {
  OfficialAccountsPanel,
  type OfficialPanelAppId,
} from "@/components/providers/OfficialAccountsPanel";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

const TAURI_ENDPOINT = "http://tauri.local";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@/lib/authBinding", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/authBinding")>()),
  canSwitchAgyAccount: () => true,
}));

function renderPanel(
  appId: OfficialPanelAppId,
  options?: {
    onOpenAuthCenter?: () => void;
    onOpenAggregation?: () => void;
  },
) {
  const queryClient = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <OfficialAccountsPanel
        appId={appId}
        onOpenAuthCenter={options?.onOpenAuthCenter}
        onOpenAggregation={options?.onOpenAggregation}
      />
    </QueryClientProvider>,
  );
  return { ...view, queryClient };
}

beforeEach(() => {
  server.use(
    http.post(`${TAURI_ENDPOINT}/agy_account_quota`, () =>
      HttpResponse.json({ success: false }),
    ),
    http.post(`${TAURI_ENDPOINT}/agy_account_is_running`, () =>
      HttpResponse.json(false),
    ),
    http.post(`${TAURI_ENDPOINT}/copilot_get_usage_for_account`, () =>
      HttpResponse.json({ success: false }),
    ),
    http.post(`${TAURI_ENDPOINT}/workbuddy_account_live_state`, () =>
      HttpResponse.json({ running: false }),
    ),
    http.post(`${TAURI_ENDPOINT}/workbuddy_account_quota`, () =>
      HttpResponse.json({}),
    ),
    http.post(`${TAURI_ENDPOINT}/kimi_oauth_quota`, () =>
      HttpResponse.json({ success: false }),
    ),
    http.post(`${TAURI_ENDPOINT}/mcode_oauth_quota`, () =>
      HttpResponse.json({ success: false }),
    ),
  );
});

describe("OfficialAccountsPanel", () => {
  it("renders only logged-in accounts for agy and supports switching in direct mode", async () => {
    const user = userEvent.setup();
    let defaultId = "agy-1";
    const switchedTo: string[] = [];

    server.use(
      http.post(`${TAURI_ENDPOINT}/auth_get_status`, () =>
        HttpResponse.json({
          provider: "agy_oauth",
          authenticated: true,
          default_account_id: defaultId,
          accounts: [
            { id: "agy-1", login: "alice@example.com", authenticated_at: 100 },
            { id: "agy-2", login: "bob@example.com", authenticated_at: 200 },
          ],
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/auth_set_default_account`, async ({ request }) => {
        const body = (await request.json()) as {
          authProvider: string;
          accountId: string;
        };
        switchedTo.push(`${body.authProvider}:${body.accountId}`);
        defaultId = body.accountId;
        return HttpResponse.json(null);
      }),
    );

    renderPanel("agy");

    expect(
      await screen.findByRole("heading", { name: "alice@example.com" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "bob@example.com" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/providerCard\.status\.inUse|使用中/),
    ).toBeInTheDocument();

    const switchButton = screen.getByRole("button", {
      name: /providerCard\.action\.switch|切换/,
    });
    await user.click(switchButton);

    await waitFor(() => {
      expect(switchedTo).toEqual(["agy_oauth:agy-2"]);
    });
  });

  it("renders logged-in accounts for copilot and switches default account", async () => {
    const user = userEvent.setup();
    const switchedTo: string[] = [];

    server.use(
      http.post(`${TAURI_ENDPOINT}/auth_get_status`, () =>
        HttpResponse.json({
          provider: "github_copilot",
          authenticated: true,
          default_account_id: "gh-1",
          accounts: [
            { id: "gh-1", login: "octocat", authenticated_at: 100 },
            { id: "gh-2", login: "hubot", authenticated_at: 200 },
          ],
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/auth_set_default_account`, async ({ request }) => {
        const body = (await request.json()) as {
          authProvider: string;
          accountId: string;
        };
        switchedTo.push(`${body.authProvider}:${body.accountId}`);
        return HttpResponse.json(null);
      }),
    );

    renderPanel("copilot");

    expect(
      await screen.findByRole("heading", { name: "octocat" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "hubot" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", {
        name: /providerCard\.action\.switch|切换/,
      }),
    );
    await waitFor(() => {
      expect(switchedTo).toEqual(["github_copilot:gh-2"]);
    });
  });

  it("renders logged-in accounts for workbuddy and switches active account", async () => {
    const user = userEvent.setup();
    const switchedTo: string[] = [];

    server.use(
      http.post(`${TAURI_ENDPOINT}/auth_get_status`, () =>
        HttpResponse.json({
          provider: "workbuddy_account",
          authenticated: true,
          default_account_id: "wb-1",
          accounts: [
            { id: "wb-1", login: "wb-alice", authenticated_at: 100 },
            { id: "wb-2", login: "wb-bob", authenticated_at: 200 },
          ],
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/auth_set_default_account`, async ({ request }) => {
        const body = (await request.json()) as {
          authProvider: string;
          accountId: string;
        };
        switchedTo.push(`${body.authProvider}:${body.accountId}`);
        return HttpResponse.json(null);
      }),
    );

    renderPanel("workbuddy");

    expect(
      await screen.findByRole("heading", { name: "wb-alice" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "wb-bob" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", {
        name: /providerCard\.action\.switch|切换/,
      }),
    );
    await waitFor(() => {
      expect(switchedTo).toEqual(["workbuddy_account:wb-2"]);
    });
  });

  it("shows only authenticated kimi accounts and calls kimi_oauth_use_account on switch", async () => {
    const user = userEvent.setup();
    const switchedTo: string[] = [];

    server.use(
      http.post(`${TAURI_ENDPOINT}/kimi_oauth_status`, () =>
        HttpResponse.json({
          currentAccountId: "kimi-1",
          primaryHidden: false,
          accounts: [
            {
              id: "kimi-1",
              name: "kimi-one",
              nickname: "Kimi One",
              avatarUrl: null,
              primary: true,
              authenticated: true,
              region: "mainland-cn",
              expiresAt: null,
              error: null,
            },
            {
              id: "kimi-2",
              name: "kimi-two",
              nickname: null,
              avatarUrl: null,
              primary: false,
              authenticated: true,
              region: "global",
              expiresAt: null,
              error: null,
            },
            {
              id: "kimi-unauth",
              name: "kimi-unauth",
              nickname: "Signed Out",
              avatarUrl: null,
              primary: false,
              authenticated: false,
              region: "global",
              expiresAt: null,
              error: null,
            },
          ],
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/kimi_oauth_use_account`, async ({ request }) => {
        const body = (await request.json()) as { accountId: string };
        switchedTo.push(body.accountId);
        return HttpResponse.json(null);
      }),
    );

    renderPanel("kimi");

    expect(
      await screen.findByRole("heading", { name: "Kimi One" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "kimi-two" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Signed Out" }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", {
        name: /providerCard\.action\.switch|切换/,
      }),
    );
    await waitFor(() => {
      expect(switchedTo).toEqual(["kimi-2"]);
    });
  });

  it("renders signed-in dsh account and empty state routes to Auth Center when signed out", async () => {
    const user = userEvent.setup();
    const onOpenAuthCenter = vi.fn();

    server.use(
      http.post(`${TAURI_ENDPOINT}/dsh_account_status`, () =>
        HttpResponse.json({
          signedIn: true,
          expired: false,
          issuer: "https://platform.deepseek.com",
          profile: { id: "u1", name: "Jason", contact: null, avatarUrl: null },
          wallets: [{ currency: "CNY", balance: "88.00" }],
          bonusWallets: [],
          usageUrl: null,
          topUpUrl: null,
          error: null,
        }),
      ),
    );

    const view = renderPanel("dsh", { onOpenAuthCenter });
    expect(
      await screen.findByRole("heading", { name: "Jason" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/providerCard\.status\.inUse|使用中/),
    ).toBeInTheDocument();
    view.unmount();

    server.use(
      http.post(`${TAURI_ENDPOINT}/dsh_account_status`, () =>
        HttpResponse.json({
          signedIn: false,
          expired: false,
          issuer: "https://platform.deepseek.com",
          profile: null,
          wallets: [],
          bonusWallets: [],
          usageUrl: null,
          topUpUrl: null,
          error: null,
        }),
      ),
    );

    renderPanel("dsh", { onOpenAuthCenter });
    expect(
      await screen.findByText(/provider\.officialEmptyTitle|尚未登录官方账号/),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: /provider\.goToAuthCenter|前往授权中心/,
      }),
    );
    expect(onOpenAuthCenter).toHaveBeenCalledTimes(1);
  });

  it("shows only authenticated mcode accounts and calls mcode_oauth_use_account on switch", async () => {
    const user = userEvent.setup();
    const switchedTo: string[] = [];

    server.use(
      http.post(`${TAURI_ENDPOINT}/mcode_oauth_status`, () =>
        HttpResponse.json({
          currentAccountId: "mcode-1",
          accounts: [
            {
              id: "mcode-1",
              name: "Alice MiniMax",
              email: "alice@minimax.cn",
              avatarUrl: null,
              region: "cn",
              authenticated: true,
              expiresAt: Date.now() + 3600_000,
              error: null,
            },
            {
              id: "mcode-2",
              name: "Bob Global",
              email: null,
              avatarUrl: null,
              region: "en",
              authenticated: true,
              expiresAt: Date.now() + 3600_000,
              error: null,
            },
            {
              id: "mcode-expired",
              name: "Expired Account",
              email: null,
              avatarUrl: null,
              region: "cn",
              authenticated: false,
              expiresAt: null,
              error: null,
            },
          ],
        }),
      ),
      http.post(`${TAURI_ENDPOINT}/mcode_oauth_use_account`, async ({ request }) => {
        const body = (await request.json()) as { accountId: string };
        switchedTo.push(body.accountId);
        return HttpResponse.json(null);
      }),
    );

    renderPanel("mcode");

    expect(
      await screen.findByRole("heading", { name: "Alice MiniMax" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Bob Global" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Expired Account" }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", {
        name: /providerCard\.action\.switch|切换/,
      }),
    );
    await waitFor(() => {
      expect(switchedTo).toEqual(["mcode-2"]);
    });
  });
});

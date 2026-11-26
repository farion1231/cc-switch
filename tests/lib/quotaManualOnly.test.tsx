import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useClaudeAccountQuota,
  useCodebuddyAccountQuota,
} from "@/lib/query/subscription";
import { useWorkBuddyAccountQuota } from "@/components/providers/workbuddyQuota";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import { AutoQuotaToggle } from "@/components/settings/auth/AutoQuotaToggle";
import { RefreshAllQuotaButton } from "@/components/settings/auth/RefreshAllQuotaButton";
import zh from "@/i18n/locales/zh.json";

const mocks = vi.hoisted(() => ({
  getClaudeAccountQuota: vi.fn(),
  workbuddyAccountQuota: vi.fn(),
  codebuddyAccountQuota: vi.fn(),
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
}));
vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: { getClaudeAccountQuota: mocks.getClaudeAccountQuota },
}));
vi.mock("@/lib/api/settings", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/settings")>()),
  settingsApi: { get: mocks.getSettings, save: mocks.saveSettings },
}));
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  authApi: {
    workbuddyAccountQuota: mocks.workbuddyAccountQuota,
    codebuddyAccountQuota: mocks.codebuddyAccountQuota,
  },
  settingsApi: { get: mocks.getSettings, save: mocks.saveSettings },
}));

const i18n = createInstance();
beforeAll(async () => {
  await i18n.use(initReactI18next).init({
    lng: "zh",
    resources: { zh: { translation: zh } },
    interpolation: { escapeValue: false },
  });
});

const quota = {
  tool: "claude",
  credentialStatus: "valid",
  credentialMessage: null,
  success: true,
  tiers: [],
  extraUsage: null,
  error: null,
  queriedAt: 1,
};

function wrapperFor(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <I18nextProvider i18n={i18n}>{children}</I18nextProvider>
    </QueryClientProvider>
  );
}

const newClient = () =>
  new QueryClient({ defaultOptions: { queries: { retry: false } } });

describe("manual-only official quota (#8990)", () => {
  beforeEach(() => {
    mocks.getClaudeAccountQuota.mockReset().mockResolvedValue(quota);
    mocks.workbuddyAccountQuota.mockReset().mockResolvedValue({
      snapshot: null,
      current: false,
      unavailable: "not_running",
    });
    mocks.codebuddyAccountQuota.mockReset().mockResolvedValue({
      queriedAt: 1,
    });
    mocks.getSettings.mockReset();
    mocks.saveSettings.mockReset().mockResolvedValue(true);
  });

  it("queries on its own when automatic queries are on", async () => {
    mocks.getSettings.mockResolvedValue({});
    const { result } = renderHook(() => useClaudeAccountQuota("acc"), {
      wrapper: wrapperFor(newClient()),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(mocks.getClaudeAccountQuota).toHaveBeenCalledTimes(1);
    expect(result.current.manualIdle).toBe(false);
  });

  it("waits for a refresh click when automatic queries are off", async () => {
    mocks.getSettings.mockResolvedValue({ officialQuotaManualOnly: true });
    const { result } = renderHook(
      () => useClaudeAccountQuota("acc", { autoQuery: true }),
      { wrapper: wrapperFor(newClient()) },
    );
    await waitFor(() => expect(result.current.manualIdle).toBe(true));
    expect(mocks.getClaudeAccountQuota).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.refetch();
    });
    expect(mocks.getClaudeAccountQuota).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(result.current.data?.success).toBe(true));
    expect(result.current.manualIdle).toBe(false);
  });

  it("does not query before the setting has been read", async () => {
    let resolveSettings: (value: unknown) => void = () => {};
    mocks.getSettings.mockReturnValue(
      new Promise((resolve) => {
        resolveSettings = resolve;
      }),
    );
    const { result } = renderHook(() => useClaudeAccountQuota("acc"), {
      wrapper: wrapperFor(newClient()),
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(mocks.getClaudeAccountQuota).not.toHaveBeenCalled();
    expect(result.current.manualIdle).toBe(false);
    await act(async () => resolveSettings({ officialQuotaManualOnly: true }));
    await waitFor(() => expect(result.current.manualIdle).toBe(true));
    expect(mocks.getClaudeAccountQuota).not.toHaveBeenCalled();
  });

  it("lets Refresh all query rows that wait for a manual refresh", async () => {
    mocks.getSettings.mockResolvedValue({ officialQuotaManualOnly: true });
    function QuotaRow() {
      const { manualIdle, data } = useClaudeAccountQuota("acc");
      return (
        <span>
          {manualIdle ? "idle" : data?.success ? "loaded" : "pending"}
        </span>
      );
    }
    render(
      <>
        <QuotaRow />
        <RefreshAllQuotaButton />
      </>,
      { wrapper: wrapperFor(newClient()) },
    );
    await screen.findByText("idle");
    expect(mocks.getClaudeAccountQuota).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", { name: zh.authCenter.quota.refreshAll }),
    );
    await screen.findByText("loaded");
    expect(mocks.getClaudeAccountQuota).toHaveBeenCalledTimes(1);
  });

  it("shows a refresh hint instead of nothing while idle", () => {
    const refetch = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <SubscriptionQuotaView
          quota={undefined}
          loading={false}
          refetch={refetch}
          inline
          idle
        />
      </I18nextProvider>,
    );
    expect(screen.getByText(zh.quota.manualIdle)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button"));
    expect(refetch).toHaveBeenCalled();
  });

  it("keeps the idle placeholder spinning during the initial manual fetch", () => {
    const refetch = vi.fn(() => new Promise(() => {}));
    const { rerender } = render(
      <I18nextProvider i18n={i18n}>
        <SubscriptionQuotaView
          quota={undefined}
          loading={false}
          refetch={refetch}
          inline
          idle
        />
      </I18nextProvider>,
    );
    fireEvent.click(screen.getByRole("button"));
    rerender(
      <I18nextProvider i18n={i18n}>
        <SubscriptionQuotaView
          quota={undefined}
          loading
          refetch={refetch}
          inline
          idle
        />
      </I18nextProvider>,
    );
    expect(screen.getByText(zh.quota.manualIdle)).toBeInTheDocument();
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("quota-refresh-icon")).toHaveClass(
      "motion-safe:animate-spin",
    );
  });

  it("gates WorkBuddy account quota queries behind the manual-only policy", async () => {
    mocks.getSettings.mockResolvedValue({ officialQuotaManualOnly: true });
    const { result } = renderHook(
      () => useWorkBuddyAccountQuota("wb-1", { autoRefresh: true }),
      { wrapper: wrapperFor(newClient()) },
    );
    await waitFor(() => expect(result.current.manualIdle).toBe(true));
    expect(mocks.workbuddyAccountQuota).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.refresh();
    });
    expect(mocks.workbuddyAccountQuota).toHaveBeenCalledWith("wb-1", true);
    await waitFor(() => expect(result.current.manualIdle).toBe(false));
  });

  it("gates CodeBuddy account quota queries behind the manual-only policy", async () => {
    mocks.getSettings.mockResolvedValue({ officialQuotaManualOnly: true });
    const { result } = renderHook(
      () => useCodebuddyAccountQuota("cb-1", "zh"),
      { wrapper: wrapperFor(newClient()) },
    );
    await waitFor(() => expect(result.current.manualIdle).toBe(true));
    expect(mocks.codebuddyAccountQuota).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.refresh();
    });
    expect(mocks.codebuddyAccountQuota).toHaveBeenCalledWith(
      "cb-1",
      "zh",
      true,
    );
    await waitFor(() => expect(result.current.manualIdle).toBe(false));
  });

  it("makes Refresh all skip the WorkBuddy and CodeBuddy backend caches", async () => {
    mocks.getSettings.mockResolvedValue({ officialQuotaManualOnly: true });
    function Rows() {
      const workbuddy = useWorkBuddyAccountQuota("wb-1");
      const codebuddy = useCodebuddyAccountQuota("cb-1", "zh");
      return (
        <span>
          {workbuddy.manualIdle || codebuddy.manualIdle ? "idle" : "loaded"}
        </span>
      );
    }
    render(
      <>
        <Rows />
        <RefreshAllQuotaButton />
      </>,
      { wrapper: wrapperFor(newClient()) },
    );
    await screen.findByText("idle");

    fireEvent.click(
      screen.getByRole("button", { name: zh.authCenter.quota.refreshAll }),
    );
    await screen.findByText("loaded");
    expect(mocks.workbuddyAccountQuota).toHaveBeenCalledWith("wb-1", true);
    expect(mocks.codebuddyAccountQuota).toHaveBeenCalledWith(
      "cb-1",
      "zh",
      true,
    );
  });

  it("saves the Auth Center switch into settings", async () => {
    mocks.getSettings.mockResolvedValue({ language: "zh" });
    render(<AutoQuotaToggle />, { wrapper: wrapperFor(newClient()) });
    const toggle = await screen.findByRole("switch", {
      name: zh.authCenter.autoQuota.title,
    });
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(mocks.saveSettings).toHaveBeenCalledWith({
        language: "zh",
        officialQuotaManualOnly: true,
      }),
    );
  });
});

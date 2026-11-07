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
import { useClaudeAccountQuota } from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import { AutoQuotaToggle } from "@/components/settings/auth/AutoQuotaToggle";
import zh from "@/i18n/locales/zh.json";

const mocks = vi.hoisted(() => ({
  getClaudeAccountQuota: vi.fn(),
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
}));
vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: { getClaudeAccountQuota: mocks.getClaudeAccountQuota },
}));
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
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

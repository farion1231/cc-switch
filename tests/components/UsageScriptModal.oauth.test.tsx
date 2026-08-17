import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import UsageScriptModal from "@/components/UsageScriptModal";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const subscriptionApiMocks = vi.hoisted(() => ({
  getQuota: vi.fn(),
  getCodexOauthQuota: vi.fn(),
}));

vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: subscriptionApiMocks,
}));
vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    children,
    footer,
  }: {
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) => (
    <div>
      {children}
      {footer}
    </div>
  ),
}));
vi.mock("@/components/JsonEditor", () => ({ default: () => null }));
vi.mock("@/hooks/useDarkMode", () => ({ useDarkMode: () => false }));
vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: { usageConfirmed: true } }),
}));

function codexOfficial(bound = true): Provider {
  return {
    id: "managed-official",
    name: "Codex Official",
    category: "official",
    settingsConfig: {},
    meta: {
      providerType: "codex_oauth",
      authBinding: bound
        ? {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: "account-1",
          }
        : undefined,
    },
  };
}

function renderModal(provider: Provider, onSave = vi.fn()) {
  const queryClient = createTestQueryClient();
  render(
    <QueryClientProvider client={queryClient}>
      <UsageScriptModal
        provider={provider}
        appId="codex"
        isOpen
        onClose={vi.fn()}
        onSave={onSave}
      />
    </QueryClientProvider>,
  );
  return { onSave, queryClient };
}

describe("UsageScriptModal bound Codex Official usage", () => {
  it("defaults to the enabled official quota query and can disable it", async () => {
    const user = userEvent.setup();
    const provider = codexOfficial();
    delete provider.meta!.providerType;
    const { onSave } = renderModal(provider);
    const toggle = screen.getByRole("switch", {
      name: "usageScript.enableUsageQuery",
    });

    expect(toggle).toBeChecked();
    expect(
      screen.getByRole("button", {
        name: "usageScript.templateOfficialSubscription",
      }),
    ).toBeInTheDocument();

    await user.click(toggle);
    await user.click(
      screen.getByRole("button", { name: /usageScript\.saveConfig/ }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        enabled: false,
        templateType: "official_subscription",
      }),
    );
  });

  it("tests and caches the bound account quota", async () => {
    const user = userEvent.setup();
    const quota = {
      tool: "codex",
      credentialStatus: "valid" as const,
      credentialMessage: null,
      success: true,
      tiers: [{ name: "Primary", utilization: 25, resetsAt: null }],
      extraUsage: null,
      error: null,
      queriedAt: 1,
    };
    subscriptionApiMocks.getCodexOauthQuota.mockResolvedValueOnce(quota);
    const { queryClient } = renderModal(codexOfficial());

    await user.click(
      screen.getByRole("button", { name: "usageScript.testScript" }),
    );

    expect(subscriptionApiMocks.getCodexOauthQuota).toHaveBeenCalledWith(
      "account-1",
    );
    expect(
      queryClient.getQueryData(["codex_oauth", "quota", "account-1"]),
    ).toEqual(quota);
    expect(subscriptionApiMocks.getQuota).not.toHaveBeenCalled();
  });

  it("keeps a legacy polling interval when selecting the native template", async () => {
    const user = userEvent.setup();
    const provider = codexOfficial();
    provider.meta!.usage_script = {
      enabled: true,
      language: "javascript",
      code: "return {};",
      templateType: "custom",
      autoQueryInterval: 17,
    };
    const { onSave } = renderModal(provider);

    await user.click(
      screen.getByRole("button", { name: /usageScript\.saveConfig/ }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        templateType: "official_subscription",
        autoQueryInterval: 17,
      }),
    );
  });

  it("leaves an unbound Official provider on the existing default", () => {
    renderModal(codexOfficial(false));

    expect(
      screen.getByRole("switch", { name: "usageScript.enableUsageQuery" }),
    ).not.toBeChecked();
  });
});

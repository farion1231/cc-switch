import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/ProviderCardActions", () => ({
  ProviderCardActions: () => null,
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: ({ appId }: { appId: string }) => (
    <span data-testid="subscription-quota">{appId}</span>
  ),
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/AgyAccountQuotaFooter", () => ({
  default: ({ accountId }: { accountId: string }) => (
    <span data-testid="agy-account-quota">{accountId}</span>
  ),
}));
const platform = vi.hoisted(() => ({ mac: true }));
vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isMac: () => platform.mac,
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));
vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

function renderCard(provider: Provider, appId: "agy" | "gemini" = "agy") {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderCard
        provider={provider}
        appId={appId}
        isCurrent={false}
        presentation={{ chips: [], buttons: [] }}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
        onDuplicate={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

const relay: Provider = {
  id: "relay",
  name: "Relay",
  settingsConfig: {
    env: {
      GEMINI_API_KEY: "k",
      GOOGLE_GEMINI_BASE_URL: "https://relay.example.com",
    },
    config: { model: { name: "Gemini 3.1 Pro (High)" } },
  },
};

describe("ProviderCard Antigravity model", () => {
  it("shows the configured model instead of the endpoint", () => {
    renderCard(relay);
    expect(screen.getByText("Gemini 3.1 Pro (High)")).toBeInTheDocument();
    expect(
      screen.queryByText("https://relay.example.com"),
    ).not.toBeInTheDocument();
  });

  it("falls back to GEMINI_MODEL and leaves other apps alone", () => {
    renderCard({
      ...relay,
      settingsConfig: { env: { GEMINI_MODEL: "Gemini 3.8 Flash (Low)" } },
    });
    expect(screen.getByText("Gemini 3.8 Flash (Low)")).toBeInTheDocument();
  });

  it("keeps showing the endpoint for Gemini CLI cards", () => {
    renderCard(relay, "gemini");
    expect(screen.queryByText("Gemini 3.1 Pro (High)")).not.toBeInTheDocument();
  });
});

describe("ProviderCard Antigravity official quota", () => {
  const official: Provider = {
    id: "official",
    name: "Antigravity",
    category: "official",
    settingsConfig: { config: {} },
    meta: {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "",
        templateType: "official_subscription",
      },
    },
  };

  it("shows the signed-in quota once official usage is turned on", () => {
    renderCard(official);
    expect(screen.getByTestId("subscription-quota")).toHaveTextContent("agy");
  });

  it("stays hidden while usage is off", () => {
    renderCard({
      ...official,
      meta: {
        usage_script: { ...official.meta!.usage_script!, enabled: false },
      },
    });
    expect(screen.queryByTestId("subscription-quota")).not.toBeInTheDocument();
  });

  const bound: Provider = {
    ...official,
    meta: {
      ...official.meta,
      authBinding: {
        source: "managed_account",
        authProvider: "agy_oauth",
        accountId: "sub-b",
      },
    },
  };

  it("shows the bound account's quota, not agy's current sign-in", () => {
    platform.mac = true;
    renderCard(bound);
    expect(screen.getByTestId("agy-account-quota")).toHaveTextContent("sub-b");
    expect(screen.queryByTestId("subscription-quota")).not.toBeInTheDocument();
  });

  it("ignores the binding where accounts cannot be switched", () => {
    platform.mac = false;
    renderCard(bound);
    expect(screen.getByTestId("subscription-quota")).toHaveTextContent("agy");
    expect(screen.queryByTestId("agy-account-quota")).not.toBeInTheDocument();
    platform.mac = true;
  });
});

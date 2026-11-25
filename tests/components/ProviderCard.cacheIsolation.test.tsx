import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { APP_IDS } from "@/config/appSpecs";
import type { ManagedAuthProvider, ManagedAuthStatus } from "@/lib/api";
import { managedAuthStatusQueryKey } from "@/lib/query/managedAuth";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/ProviderCardActions", () => ({
  ProviderCardActions: () => null,
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isLinux: () => true,
}));

const relayUrl = "https://relay.example.test";

const unrelatedAccounts = {
  agy_oauth: "agy@example.test",
  claude_official: "claude@example.test",
  codebuddy_oauth: "codebuddy@example.test",
  codex_oauth: "codex@example.test",
  github_copilot: "copilot@example.test",
  workbuddy_account: "workbuddy@example.test",
  xai_oauth: "grok@example.test",
} satisfies Record<ManagedAuthProvider, string>;

const providers = Object.keys(unrelatedAccounts) as ManagedAuthProvider[];

it.each(APP_IDS)("keeps cached logins off unrelated %s cards", (appId) => {
  const queryClient = createTestQueryClient();
  for (const provider of providers) {
    queryClient.setQueryData<ManagedAuthStatus>(
      managedAuthStatusQueryKey(provider),
      {
        provider,
        authenticated: true,
        default_account_id: "default",
        accounts: [
          {
            id: "default",
            provider,
            login: unrelatedAccounts[provider],
            avatar_url: null,
            authenticated_at: 1,
            is_default: true,
            github_domain: "",
            requires_reauth: true,
          },
        ],
      },
    );
  }

  const { container } = render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        appId={appId}
        provider={{
          id: "relay",
          name: "Relay",
          category: "third_party",
          websiteUrl: relayUrl,
          settingsConfig: {
            env: {
              ANTHROPIC_BASE_URL: relayUrl,
              GOOGLE_GEMINI_BASE_URL: relayUrl,
            },
            auth: { OPENAI_API_KEY: "sk-test" },
          },
        }}
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

  expect(screen.getByText(relayUrl)).toBeInTheDocument();
  expect(screen.queryByText("authCenter.inUseBy")).not.toBeInTheDocument();
  for (const login of Object.values(unrelatedAccounts)) {
    expect(container).not.toHaveTextContent(login);
  }
});

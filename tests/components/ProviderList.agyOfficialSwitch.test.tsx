import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderList } from "@/components/providers/ProviderList";
import { useAgyOfficialCards } from "@/lib/query/agy";
import { agyAuthStatusQuery } from "@/lib/query/subscription";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const authMocks = vi.hoisted(() => ({
  authGetStatus: vi.fn(),
  authSetDefaultAccount: vi.fn(),
  agyAccountIsRunning: vi.fn(),
}));

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    authApi: {
      ...actual.authApi,
      authGetStatus: authMocks.authGetStatus,
      authSetDefaultAccount: authMocks.authSetDefaultAccount,
      agyAccountIsRunning: authMocks.agyAccountIsRunning,
    },
  };
});

vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isMac: () => true,
  isWindows: () => false,
  isLinux: () => false,
}));

vi.mock("@/lib/toast", () => ({
  toast: toastMocks,
}));

vi.mock("@/hooks/useDragSort", () => ({
  useDragSort: (providers: Record<string, Provider>) => ({
    sortedProviders: Object.values(providers),
    sensors: [],
    handleDragEnd: vi.fn(),
  }),
}));

vi.mock("@/hooks/useStreamCheck", () => ({
  useStreamCheck: () => ({
    checkProvider: vi.fn(),
    isChecking: () => false,
  }),
}));

vi.mock("@/components/providers/ProviderCard", () => ({
  ProviderCard: ({
    provider,
    presentation,
    onOpenTerminal,
  }: {
    provider: Provider;
    presentation: {
      status?: { label: string };
      buttons: {
        key: string;
        label: string;
        onClick: () => void;
        disabledReason?: string;
      }[];
    };
    onOpenTerminal?: (provider: Provider) => void;
  }) => (
    <div data-testid={`provider-card-${provider.id}`}>
      <span>{provider.name}</span>
      {presentation.status && (
        <span data-testid={`status-${provider.id}`}>
          {presentation.status.label}
        </span>
      )}
      {presentation.buttons.map((button) => (
        <button
          key={button.key}
          type="button"
          data-testid={`${button.key}-${provider.id}`}
          disabled={Boolean(button.disabledReason)}
          onClick={button.onClick}
        >
          {button.label}
        </button>
      ))}
      {onOpenTerminal && (
        <button
          type="button"
          data-testid={`icon-terminal-${provider.id}`}
          onClick={() => onOpenTerminal(provider)}
        >
          terminal
        </button>
      )}
    </div>
  ),
}));

const boundAgyCard = (
  id: string,
  name: string,
  accountId: string,
): Provider => ({
  id,
  name,
  category: "official",
  settingsConfig: { env: {}, config: {} },
  meta: {
    authBinding: {
      source: "managed_account",
      authProvider: "agy_oauth",
      accountId,
    },
  },
});

const keyAgyCard: Provider = {
  id: "agy-key",
  name: "Relay Key",
  category: "third_party",
  settingsConfig: {
    env: {
      GEMINI_API_KEY: "test-key",
      GOOGLE_GEMINI_BASE_URL: "https://relay.example.com",
    },
  },
};

describe("ProviderList Antigravity official account cards", () => {
  beforeEach(() => {
    authMocks.authGetStatus.mockReset().mockResolvedValue({
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: "acct-a",
      accounts: [
        {
          id: "acct-a",
          provider: "agy_oauth",
          login: "alice@example.com",
          avatar_url: null,
          authenticated_at: 1,
          is_default: true,
          github_domain: "google.com",
          requires_reauth: false,
        },
        {
          id: "acct-b",
          provider: "agy_oauth",
          login: "bob@example.com",
          avatar_url: null,
          authenticated_at: 2,
          is_default: false,
          github_domain: "google.com",
          requires_reauth: false,
        },
      ],
    });
    authMocks.authSetDefaultAccount.mockReset().mockResolvedValue(undefined);
    authMocks.agyAccountIsRunning.mockReset().mockResolvedValue(false);
    toastMocks.success.mockReset();
    toastMocks.warning.mockReset();
    toastMocks.error.mockReset();
  });

  it("shows active status on in-use card, 1-click switch on others, and keeps terminal action", async () => {
    const onOpenTerminal = vi.fn();
    const providers = {
      "agy-a": boundAgyCard("agy-a", "Antigravity (alice)", "acct-a"),
      "agy-b": boundAgyCard("agy-b", "Antigravity (bob)", "acct-b"),
      "agy-key": keyAgyCard,
    };

    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderList
          providers={providers}
          currentProviderId=""
          appId="agy"
          onSwitch={vi.fn()}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
          onOpenTerminal={onOpenTerminal}
        />
      </QueryClientProvider>,
    );

    expect(await screen.findByTestId("status-agy-a")).toHaveTextContent(
      "authCenter.inUseBy",
    );
    expect(screen.queryByTestId("switch-agy-a")).not.toBeInTheDocument();

    const switchBtn = screen.getByTestId("switch-agy-b");
    expect(switchBtn).toBeEnabled();
    expect(screen.getByTestId("icon-terminal-agy-a")).toBeInTheDocument();
    expect(screen.getByTestId("icon-terminal-agy-b")).toBeInTheDocument();

    // Key card keeps primary openTerminal button and does not duplicate icon terminal button
    expect(screen.getByTestId("openTerminal-agy-key")).toBeInTheDocument();
    expect(
      screen.queryByTestId("icon-terminal-agy-key"),
    ).not.toBeInTheDocument();

    fireEvent.click(switchBtn);

    await waitFor(() => {
      expect(authMocks.authSetDefaultAccount).toHaveBeenCalledWith(
        "agy_oauth",
        "acct-b",
      );
    });
  });

  it("does not leak pre-warmed cache when useAgyOfficialCards is disabled", () => {
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(agyAuthStatusQuery.queryKey, {
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: "acct-a",
      accounts: [
        {
          id: "acct-a",
          provider: "agy_oauth",
          login: "alice@example.com",
          avatar_url: null,
          authenticated_at: 1,
          is_default: true,
          github_domain: "google.com",
          requires_reauth: false,
        },
      ],
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => useAgyOfficialCards(false), {
      wrapper,
    });
    expect(result.current.state).toEqual({
      status: "loading",
      currentAccountId: null,
      accounts: [],
      switchingId: null,
    });
  });
});

import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderList } from "@/components/providers/ProviderList";
import {
  AGY_RUNNING_STATE_KEY,
  agyAuthStatusQuery,
} from "@/lib/query/subscription";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  authGetStatus: vi.fn(),
  authSetDefaultAccount: vi.fn(),
  agyAccountRunningState: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  platformWindows: true,
}));

vi.mock("@/lib/toast", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: mocks.toastError,
    info: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isWindows: () => mocks.platformWindows,
  isMac: () => false,
  isLinux: () => false,
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    authApi: {
      ...actual.authApi,
      authGetStatus: mocks.authGetStatus,
      authSetDefaultAccount: mocks.authSetDefaultAccount,
      agyAccountRunningState: mocks.agyAccountRunningState,
    },
  };
});

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
        disabledReason?: string;
        onClick: () => void;
      }[];
    };
    onOpenTerminal?: (p: Provider) => void;
  }) => (
    <div data-testid={`card-${provider.id}`}>
      {presentation.status && (
        <span data-testid={`status-${provider.id}`}>
          {presentation.status.label}
        </span>
      )}
      {presentation.buttons.map((button) => (
        <button
          key={button.key}
          data-testid={`btn-${button.key}-${provider.id}`}
          data-disabled-reason={button.disabledReason ?? ""}
          onClick={button.onClick}
        >
          {button.label}
        </button>
      ))}
      {onOpenTerminal && (
        <button
          data-testid={`terminal-${provider.id}`}
          onClick={() => onOpenTerminal(provider)}
        >
          open-terminal
        </button>
      )}
    </div>
  ),
}));

const boundCard = (id: string, name: string, accountId: string): Provider => ({
  id,
  name,
  category: "official",
  settingsConfig: { config: {} },
  meta: {
    authBinding: {
      source: "managed_account",
      authProvider: "agy_oauth",
      accountId,
    },
  },
});

const relayCard: Provider = {
  id: "relay",
  name: "Relay",
  settingsConfig: {
    env: {
      GEMINI_API_KEY: "sk-test",
      GOOGLE_GEMINI_BASE_URL: "https://relay.example.com",
    },
  },
};

const providers: Record<string, Provider> = {
  a: boundCard("a", "Google A", "acc-a"),
  b: boundCard("b", "Google B", "acc-b"),
  relay: relayCard,
};

function renderList() {
  return render(
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
        onOpenTerminal={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderList Antigravity account switching", () => {
  beforeEach(() => {
    mocks.authGetStatus.mockReset();
    mocks.authSetDefaultAccount.mockReset();
    mocks.agyAccountRunningState.mockReset();
    mocks.toastSuccess.mockReset();
    mocks.toastError.mockReset();
    mocks.platformWindows = true;
    mocks.authGetStatus.mockResolvedValue({
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: "acc-a",
      accounts: [
        {
          id: "acc-a",
          provider: "agy_oauth",
          login: "a@example.com",
          avatar_url: null,
          authenticated_at: 1,
          is_default: true,
          github_domain: "google.com",
          requires_reauth: false,
        },
        {
          id: "acc-b",
          provider: "agy_oauth",
          login: "b@example.com",
          avatar_url: null,
          authenticated_at: 2,
          is_default: false,
          github_domain: "google.com",
          requires_reauth: false,
        },
      ],
    });
    mocks.agyAccountRunningState.mockResolvedValue({
      cli: false,
      desktop: false,
    });
    mocks.authSetDefaultAccount.mockResolvedValue(undefined);
  });

  it("shows inUse on the active bound card, keeps terminal launch, and switches inactive bound cards when stopped", async () => {
    renderList();

    expect(await screen.findByTestId("status-a")).toHaveTextContent(
      "providerCard.status.inUse",
    );
    expect(screen.getByTestId("terminal-a")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-b")).toBeInTheDocument();
    expect(screen.getByTestId("btn-openTerminal-relay")).toBeInTheDocument();
    expect(screen.queryByTestId("terminal-relay")).not.toBeInTheDocument();

    const switchBtn = screen.getByTestId("btn-switch-b");
    expect(switchBtn).toHaveAttribute("data-disabled-reason", "");
    await userEvent.click(switchBtn);

    await waitFor(() => {
      expect(mocks.authSetDefaultAccount).toHaveBeenCalledWith(
        "agy_oauth",
        "acc-b",
      );
    });
    expect(mocks.toastSuccess).toHaveBeenCalledTimes(1);
  });

  it("disables and blocks card switching when Antigravity Desktop is running", async () => {
    mocks.agyAccountRunningState.mockResolvedValue({
      cli: false,
      desktop: true,
    });
    renderList();

    const switchBtn = await screen.findByTestId("btn-switch-b");
    await waitFor(() => {
      expect(switchBtn.getAttribute("data-disabled-reason")).toContain(
        "Antigravity",
      );
    });

    await userEvent.click(switchBtn);
    await waitFor(() => {
      expect(mocks.toastError).toHaveBeenCalledTimes(1);
    });
    expect(mocks.authSetDefaultAccount).not.toHaveBeenCalled();
  });

  it("disables switching when auth status fails to load", async () => {
    mocks.authGetStatus.mockRejectedValue(new Error("keychain read error"));
    renderList();

    const switchBtn = await screen.findByTestId("btn-switch-b");
    await waitFor(() => {
      expect(switchBtn.getAttribute("data-disabled-reason")).toBe(
        "无法读取 Antigravity 账号，请重试。",
      );
    });
  });

  it("does not leak pre-warmed agy cache onto unbound or relay cards", () => {
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(agyAuthStatusQuery.queryKey, {
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: "acc-a",
      accounts: [
        {
          id: "acc-a",
          provider: "agy_oauth",
          login: "a@example.com",
          avatar_url: null,
          authenticated_at: 1,
          is_default: true,
          github_domain: "google.com",
          requires_reauth: false,
        },
      ],
    });
    queryClient.setQueryData(AGY_RUNNING_STATE_KEY, {
      cli: true,
      desktop: true,
    });

    const unboundOfficial: Provider = {
      id: "unbound",
      name: "Unbound Official",
      category: "official",
      settingsConfig: { config: {} },
    };

    render(
      <QueryClientProvider client={queryClient}>
        <ProviderList
          providers={{ unbound: unboundOfficial, relay: relayCard }}
          currentProviderId=""
          appId="agy"
          onSwitch={vi.fn()}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
          onOpenTerminal={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(screen.queryByTestId("status-unbound")).not.toBeInTheDocument();
    expect(screen.queryByTestId("btn-switch-unbound")).not.toBeInTheDocument();
    expect(screen.getByTestId("btn-openTerminal-unbound")).toBeInTheDocument();
    expect(screen.getByTestId("btn-openTerminal-relay")).toBeInTheDocument();
  });
});

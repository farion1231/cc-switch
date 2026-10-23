import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { KimiAccountLogin } from "@/components/providers/KimiAccountLogin";
import { kimiOAuthApi, type KimiOAuthStatus } from "@/lib/api/kimiOAuth";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({ openExternal: vi.fn(), copy: vi.fn() }));
vi.mock("@/lib/api/kimiOAuth", () => ({
  kimiOAuthApi: {
    status: vi.fn(),
    start: vi.fn(),
    poll: vi.fn(),
    cancel: vi.fn(),
    logout: vi.fn(),
    useAccount: vi.fn(),
  },
}));
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    settingsApi: { ...actual.settingsApi, openExternal: mocks.openExternal },
  };
});
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copy }));

const account = {
  id: "kimi-code-ccs-first",
  name: "Kimi Official (first)",
  authenticated: true,
  region: "mainland-cn" as const,
  expiresAt: null,
  error: null,
};
const signedOut: KimiOAuthStatus = { accounts: [], currentAccountId: null };
const signedIn: KimiOAuthStatus = {
  accounts: [account],
  currentAccountId: account.id,
};
const device = {
  flowId: "flow-1",
  userCode: "ABCD-EFGH",
  verificationUri: "https://auth.kimi.com/verify?code=ABCD-EFGH",
  expiresIn: 600,
  interval: 1,
};
const pending = { authenticated: false, interval: 1, ended: null };

const flush = async () => {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
};

async function setup(status = signedOut) {
  const client = createTestQueryClient();
  client.setQueryData(["kimi-oauth-status"], status);
  vi.mocked(kimiOAuthApi.status).mockResolvedValue(status);
  const view = render(
    <QueryClientProvider client={client}>
      <KimiAccountLogin />
    </QueryClientProvider>,
  );
  await flush();
  return { ...view, client };
}

async function start() {
  fireEvent.click(screen.getByRole("button", { name: "kimi.oauth.signIn" }));
  await flush();
}

describe("Kimi official account login", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(kimiOAuthApi.status).mockResolvedValue(signedOut);
    vi.mocked(kimiOAuthApi.start).mockResolvedValue(device);
    vi.mocked(kimiOAuthApi.poll).mockResolvedValue(pending);
    vi.mocked(kimiOAuthApi.cancel).mockResolvedValue(true);
    vi.mocked(kimiOAuthApi.useAccount).mockResolvedValue(undefined);
    vi.mocked(kimiOAuthApi.logout).mockResolvedValue(undefined);
    mocks.openExternal.mockResolvedValue(undefined);
    mocks.copy.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("selects and reauthenticates the requested account while allowing another account to be added", async () => {
    const second = {
      ...account,
      id: "kimi-code-ccs-second",
      name: "Kimi Official (second)",
      region: "global" as const,
    };
    await setup({ accounts: [account, second], currentAccountId: account.id });
    const row = screen
      .getByText(second.name)
      .closest('[role="listitem"]')! as HTMLElement;
    fireEvent.click(
      within(row).getByRole("button", { name: "kimi.oauth.useAccount" }),
    );
    await flush();
    expect(kimiOAuthApi.useAccount).toHaveBeenCalledWith(second.id);
    fireEvent.click(
      within(row).getByRole("button", { name: "kimi.oauth.signInAgain" }),
    );
    await flush();
    expect(kimiOAuthApi.start).toHaveBeenCalledWith("global", second.id);
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await flush();
    fireEvent.click(
      screen.getByRole("button", { name: "kimi.oauth.addAccount" }),
    );
    await flush();
    expect(kimiOAuthApi.start).toHaveBeenLastCalledWith(
      "mainland-cn",
      undefined,
    );
  });

  it("keeps the authorization code and manual link available when opening the browser fails", async () => {
    mocks.openExternal.mockRejectedValue(new Error("No browser"));
    await setup();
    await start();
    expect(kimiOAuthApi.start).toHaveBeenCalledWith("mainland-cn", undefined);
    expect(screen.getByText(device.userCode)).toBeInTheDocument();
    expect(screen.getByRole("link")).toHaveAttribute(
      "href",
      device.verificationUri,
    );
    expect(mocks.openExternal).toHaveBeenCalledWith(device.verificationUri);
    fireEvent.click(
      screen.getByRole("button", { name: "kimi.oauth.copyCode" }),
    );
    await flush();
    expect(mocks.copy).toHaveBeenCalledWith(device.userCode);
    expect(kimiOAuthApi.poll).not.toHaveBeenCalled();
  });

  it("refreshes both native login status and the supplier list on successful authorization", async () => {
    const { client } = await setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await start();
    vi.mocked(kimiOAuthApi.poll).mockResolvedValue({
      authenticated: true,
      interval: 0,
      ended: null,
    });
    vi.mocked(kimiOAuthApi.status).mockResolvedValue(signedIn);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    await flush();
    expect(screen.queryByText(device.userCode)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "kimi.oauth.addAccount" }),
    ).toBeEnabled();
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["providers", "kimi"],
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["kimi-oauth-status"],
    });
  });

  it("honors a longer server polling interval without overlapping requests", async () => {
    await setup();
    await start();
    vi.mocked(kimiOAuthApi.poll).mockResolvedValue({ ...pending, interval: 6 });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(kimiOAuthApi.poll).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5999);
    });
    expect(kimiOAuthApi.poll).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(kimiOAuthApi.poll).toHaveBeenCalledTimes(2);
  });

  it("cancels on user action and ignores a poll response that arrives later", async () => {
    let resolvePoll!: (value: {
      authenticated: boolean;
      interval: number;
      ended: null;
    }) => void;
    vi.mocked(kimiOAuthApi.poll).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvePoll = resolve;
        }),
    );
    await setup();
    await start();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await flush();
    expect(kimiOAuthApi.cancel).toHaveBeenCalledWith(device.flowId);
    await act(async () => {
      resolvePoll({ authenticated: true, interval: 0, ended: null });
    });
    expect(screen.queryByText(device.userCode)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "kimi.oauth.signIn" }),
    ).toBeEnabled();
  });

  it("cancels a device flow received after the card is unmounted", async () => {
    let resolveStart!: (value: typeof device) => void;
    vi.mocked(kimiOAuthApi.start).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveStart = resolve;
        }),
    );
    const { unmount } = await setup();
    await start();
    unmount();
    await act(async () => {
      resolveStart(device);
    });
    expect(kimiOAuthApi.cancel).toHaveBeenCalledWith(device.flowId);
    expect(mocks.openExternal).not.toHaveBeenCalled();
  });

  it("retries model discovery within the existing flow after a transient failure", async () => {
    await setup();
    await start();
    vi.mocked(kimiOAuthApi.poll).mockRejectedValueOnce(
      "Model discovery failed",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Model discovery failed",
    );
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await flush();
    expect(kimiOAuthApi.start).toHaveBeenCalledTimes(1);
    expect(kimiOAuthApi.poll).toHaveBeenCalledTimes(2);
    expect(kimiOAuthApi.poll).toHaveBeenLastCalledWith(device.flowId);
  });

  it.each(["expired", "denied", "cancelled"] as const)(
    "allows a fresh sign-in after authorization is %s",
    async (ended) => {
      await setup();
      await start();
      vi.mocked(kimiOAuthApi.poll).mockResolvedValue({ ...pending, ended });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
      expect(screen.getByRole("alert")).toHaveTextContent(
        `kimi.oauth.${ended}`,
      );
      expect(screen.queryByText(device.userCode)).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "kimi.oauth.signIn" }),
      ).toBeEnabled();
    },
  );

  it("requires confirmation before removing the native login", async () => {
    await setup(signedIn);
    fireEvent.click(screen.getByRole("button", { name: "kimi.oauth.signOut" }));
    expect(kimiOAuthApi.logout).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "kimi.oauth.signOutConfirm",
    );
    fireEvent.click(
      screen.getAllByRole("button", { name: "kimi.oauth.signOut" }).at(-1)!,
    );
    await flush();
    expect(kimiOAuthApi.logout).toHaveBeenCalledWith(account.id);
  });
});

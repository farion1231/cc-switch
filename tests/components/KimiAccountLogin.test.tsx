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
    remove: vi.fn(),
    quota: vi.fn(),
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

// 账号操作收在每行的「…」菜单里（按钮名带账号名）；Radix 菜单在 jsdom 里用回车打开最稳
async function openMenu(login: string) {
  fireEvent.keyDown(
    screen.getByRole("button", { name: `${login} 的更多操作` }),
    { key: "Enter" },
  );
  await flush();
}

const menuItem = (name: string | RegExp) =>
  screen.getByRole("menuitem", { name });

const account = {
  id: "kimi-code-ccs-first",
  name: "Kimi Official (first)",
  authenticated: true,
  region: "mainland-cn" as const,
  expiresAt: null,
  error: null,
  nickname: null,
  avatarUrl: null,
  primary: false,
};
const signedOut: KimiOAuthStatus = {
  accounts: [],
  currentAccountId: null,
  primaryHidden: false,
};
const signedIn: KimiOAuthStatus = {
  accounts: [account],
  currentAccountId: account.id,
  primaryHidden: false,
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

// 「添加账号」先打开区域选择，再点选择块里的「登录 Kimi」
async function start() {
  fireEvent.click(screen.getByRole("button", { name: "添加账号" }));
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
    vi.mocked(kimiOAuthApi.remove).mockResolvedValue(undefined);
    vi.mocked(kimiOAuthApi.quota).mockResolvedValue({
      tool: "kimi_oauth",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [],
      extraUsage: null,
      error: null,
      queriedAt: 0,
    });
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
    await setup({
      accounts: [account, second],
      currentAccountId: account.id,
      primaryHidden: false,
    });
    expect(screen.getByText("kimi.oauth.current")).toBeInTheDocument();
    await openMenu(second.name);
    fireEvent.click(menuItem("kimi.oauth.useAccount"));
    await flush();
    expect(kimiOAuthApi.useAccount).toHaveBeenCalledWith(second.id);
    await openMenu(second.name);
    fireEvent.click(menuItem("重新登录"));
    await flush();
    // 重新登录沿用账号自己的区域，不弹区域选择
    expect(kimiOAuthApi.start).toHaveBeenCalledWith("global", second.id);
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "添加账号" }));
    fireEvent.click(screen.getByRole("button", { name: "kimi.oauth.signIn" }));
    await flush();
    expect(kimiOAuthApi.start).toHaveBeenLastCalledWith(
      "mainland-cn",
      undefined,
    );
  });

  it("signs in to the region chosen before starting", async () => {
    await setup();
    fireEvent.click(screen.getByRole("button", { name: "添加账号" }));
    fireEvent.click(screen.getByRole("button", { name: "kimi.oauth.global" }));
    fireEvent.click(screen.getByRole("button", { name: "kimi.oauth.signIn" }));
    await flush();
    expect(kimiOAuthApi.start).toHaveBeenCalledWith("global", undefined);
  });

  it("keeps the authorization code available when opening the browser fails", async () => {
    mocks.openExternal.mockRejectedValue(new Error("No browser"));
    await setup();
    await start();
    expect(kimiOAuthApi.start).toHaveBeenCalledWith("mainland-cn", undefined);
    expect(screen.getByText(device.userCode)).toBeInTheDocument();
    expect(mocks.openExternal).toHaveBeenCalledWith(device.verificationUri);
    fireEvent.click(screen.getByRole("button", { name: "打开授权页面" }));
    expect(mocks.openExternal).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "复制验证码" }));
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
    expect(screen.getByRole("button", { name: "添加账号" })).toBeEnabled();
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
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await flush();
    expect(kimiOAuthApi.cancel).toHaveBeenCalledWith(device.flowId);
    await act(async () => {
      resolvePoll({ authenticated: true, interval: 0, ended: null });
    });
    expect(screen.queryByText(device.userCode)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "添加账号" }),
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
    expect(screen.getByText("Model discovery failed")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    await flush();
    expect(kimiOAuthApi.start).toHaveBeenCalledTimes(1);
    expect(kimiOAuthApi.poll).toHaveBeenCalledTimes(2);
    expect(kimiOAuthApi.poll).toHaveBeenLastCalledWith(device.flowId);
  });

  it.each(["expired", "denied", "cancelled"] as const)(
    "starts a fresh sign-in when retrying after authorization is %s",
    async (ended) => {
      await setup();
      await start();
      vi.mocked(kimiOAuthApi.poll).mockResolvedValue({ ...pending, ended });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
      expect(screen.getByText(`kimi.oauth.${ended}`)).toBeInTheDocument();
      expect(screen.queryByText(device.userCode)).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "重试" }));
      await flush();
      expect(kimiOAuthApi.start).toHaveBeenCalledTimes(2);
      expect(kimiOAuthApi.start).toHaveBeenLastCalledWith(
        "mainland-cn",
        undefined,
      );
    },
  );

  it("deletes a separate account only after confirmation and never while it is in use", async () => {
    const second = {
      ...account,
      id: "kimi-code-ccs-second",
      name: "Kimi Official · second",
    };
    await setup({
      accounts: [account, second],
      currentAccountId: account.id,
      primaryHidden: false,
    });
    await openMenu(account.name);
    // 正在用的号菜单里没有「使用此账号」，删除项置灰并说明原因
    expect(
      screen.queryByRole("menuitem", { name: "kimi.oauth.useAccount" }),
    ).not.toBeInTheDocument();
    expect(menuItem(/kimi\.oauth\.removeAccount/)).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(
      screen.getByText("kimi.oauth.removeCurrentHint"),
    ).toBeInTheDocument();
    fireEvent.keyDown(document.activeElement ?? document.body, {
      key: "Escape",
    });
    await flush();

    await openMenu(second.name);
    fireEvent.click(menuItem("kimi.oauth.removeAccount"));
    expect(kimiOAuthApi.remove).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "kimi.oauth.removeConfirm",
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "kimi.oauth.removeAccount",
      }),
    );
    await flush();
    expect(kimiOAuthApi.remove).toHaveBeenCalledWith(second.id);
  });

  it("only hides Kimi's own login from the list, even while it is in use", async () => {
    const primary = {
      ...account,
      id: "kimi-code",
      name: "Kimi Official · main",
      nickname: "main",
      primary: true,
    };
    await setup({
      accounts: [primary],
      currentAccountId: primary.id,
      primaryHidden: false,
    });
    // 行名用昵称，第二行注明这是 Kimi Code 自己的登录
    expect(screen.getByText(/kimi\.oauth\.primaryShort/)).toBeInTheDocument();
    await openMenu("main");
    expect(
      screen.queryByRole("menuitem", { name: "kimi.oauth.removeAccount" }),
    ).not.toBeInTheDocument();
    fireEvent.click(menuItem("kimi.oauth.dismissAccount"));
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "kimi.oauth.dismissConfirm",
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "kimi.oauth.dismissAccount",
      }),
    );
    await flush();
    expect(kimiOAuthApi.remove).toHaveBeenCalledWith(primary.id);
  });

  it("says Kimi's own login was only hidden when the list is empty", async () => {
    await setup({ accounts: [], currentAccountId: null, primaryHidden: true });
    expect(screen.getByText("kimi.oauth.primaryHidden")).toBeInTheDocument();
    expect(screen.queryByText("kimi.oauth.empty")).not.toBeInTheDocument();
  });

  it("marks a signed-out account for re-login and queries quota only for signed-in ones", async () => {
    const second = {
      ...account,
      id: "kimi-code-ccs-second",
      name: "Kimi Official · second",
      authenticated: false,
    };
    await setup({
      accounts: [account, second],
      currentAccountId: account.id,
      primaryHidden: false,
    });
    expect(screen.getByText("需要重新登录")).toBeInTheDocument();
    expect(kimiOAuthApi.quota).toHaveBeenCalledWith(account.id);
    expect(kimiOAuthApi.quota).not.toHaveBeenCalledWith(second.id);
    // 默认 mock 查成功但一档都没有，即没订阅 Kimi Code 的号
    expect(screen.getByText("kimi.oauth.noSubscription")).toBeInTheDocument();
  });
});

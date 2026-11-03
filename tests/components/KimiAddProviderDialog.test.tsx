import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { kimiOAuthApi, type KimiOAuthStatus } from "@/lib/api/kimiOAuth";
import { createTestQueryClient } from "../utils/testQueryClient";

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
vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/AgyOAuthSection", () => ({
  AgyOAuthSection: () => null,
}));
vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (text: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
  window.matchMedia ??= vi.fn().mockReturnValue({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
});

const status: KimiOAuthStatus = {
  accounts: [],
  currentAccountId: null,
  primaryHidden: false,
};
const device = {
  flowId: "kimi-flow",
  userCode: "TEST-CODE",
  verificationUri: "https://auth.kimi.com/verify",
  expiresIn: 1800,
  interval: 300,
};

function setup() {
  const client = createTestQueryClient();
  client.setQueryData(["kimi-oauth-status"], status);
  const onOpenChange = vi.fn();
  const onSubmit = vi.fn();
  const view = render(
    <QueryClientProvider client={client}>
      <AddProviderDialog
        open
        appId="kimi"
        onOpenChange={onOpenChange}
        onSubmit={onSubmit}
      />
    </QueryClientProvider>,
  );
  return { ...view, onOpenChange, onSubmit };
}

async function selectAccount() {
  fireEvent.click(
    await screen.findByRole("button", { name: "kimi.oauth.title" }),
  );
  await screen.findByRole("button", { name: "kimi.oauth.manageAccounts" });
}
async function openAuthCenter() {
  await selectAccount();
  fireEvent.click(
    screen.getByRole("button", { name: "kimi.oauth.manageAccounts" }),
  );
  return screen.findByRole("region", { name: "Kimi Code" });
}

describe("Kimi account entry in the add-provider dialog", () => {
  beforeEach(() => {
    vi.mocked(kimiOAuthApi.status).mockResolvedValue(status);
    vi.mocked(kimiOAuthApi.start).mockResolvedValue(device);
    vi.mocked(kimiOAuthApi.cancel).mockResolvedValue(true);
    vi.mocked(kimiOAuthApi.useAccount).mockResolvedValue(undefined);
  });

  it("directs official sign-in to Auth Center without API-key fields or an add footer", async () => {
    const { onSubmit } = setup();
    await selectAccount();
    expect(
      screen.queryByRole("button", { name: "common.add" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "kimi.oauth.signIn" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "kimi.oauth.manageAccounts" }),
    );
    const group = await screen.findByRole("region", { name: "Kimi Code" });
    expect(
      within(group).getByRole("button", { name: "添加账号" }),
    ).toBeEnabled();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("starts and cancels the device login inside Auth Center", async () => {
    const { onSubmit } = setup();
    const group = await openAuthCenter();
    // 先选登录区域，再在选择块里点登录
    fireEvent.click(
      within(group).getByRole("button", { name: "添加账号" }),
    );
    fireEvent.click(
      within(group).getByRole("button", { name: "kimi.oauth.signIn" }),
    );
    await screen.findByText(device.userCode);
    expect(kimiOAuthApi.start).toHaveBeenCalledWith("mainland-cn", undefined);
    fireEvent.click(within(group).getByRole("button", { name: "取消" }));
    await waitFor(() =>
      expect(kimiOAuthApi.cancel).toHaveBeenCalledWith(device.flowId),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("selects an existing account by ID and keeps account management open", async () => {
    const account = {
      id: "kimi-code-ccs-existing",
      name: "Kimi Official (existing)",
      authenticated: true,
      region: "global" as const,
      expiresAt: null,
      error: null,
      nickname: null,
      avatarUrl: null,
      primary: false,
    };
    vi.mocked(kimiOAuthApi.status).mockResolvedValue({
      accounts: [account],
      currentAccountId: null,
      primaryHidden: false,
    });
    const { onSubmit, onOpenChange } = setup();
    const group = await openAuthCenter();
    // 「使用此账号」收在账号行的「…」菜单里
    fireEvent.keyDown(
      await within(group).findByRole("button", {
        name: /accountMenu|更多操作/,
      }),
      { key: "Enter" },
    );
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "kimi.oauth.useAccount" }),
    );
    await waitFor(() =>
      expect(kimiOAuthApi.useAccount).toHaveBeenCalledWith(account.id),
    );
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { QoderPanel } from "@/components/providers/QoderPanel";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";
import type {
  QoderAppId,
  QoderModels,
  QoderProvider,
  QoderAuthStatus,
  QoderLoginProgress,
} from "@/lib/api/qoder";
import { qoderProviderPresets } from "@/config/qoderProviderPresets";
import type { UsageScript } from "@/types";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
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
vi.mock("@/components/UsageScriptModal", () => ({
  default: ({
    onSave,
    onClose,
  }: {
    onSave: (script: UsageScript) => void;
    onClose: () => void;
  }) => (
    <div role="dialog" aria-label="usage editor">
      <button
        onClick={() => {
          onSave({
            enabled: true,
            language: "javascript",
            code: "return { remaining: 42 }",
            autoQueryInterval: 15,
          });
          onClose();
        }}
      >
        Save usage
      </button>
    </div>
  ),
}));

const provider: QoderProvider = {
  displayName: "Native provider",
  baseUrl: "https://example.test/v1",
  apiKey: "${NATIVE_API_KEY}",
  protocol: "anthropic",
  authType: "api-key",
  model: "model-one",
  routing: { summary: "model-one" },
  anthropic: { cache: { mode: "off", ttl: "1h" } },
  models: [
    {
      model: "model-one",
      displayName: "Model One",
      contextWindow: 200000,
      maxOutputTokens: 8192,
      capabilities: {
        tools: true,
        vision: true,
        thinking: { modes: ["adaptive"], supportedEffortLevels: ["high"] },
      },
      customNativeOption: "keep",
    },
  ],
};
let state: Record<QoderAppId, QoderModels>;
let authState: Record<QoderAppId, QoderAuthStatus>;
let loginProgress: Record<QoderAppId, QoderLoginProgress>;

function mount(app: QoderAppId = "qoder-cn", authCenter = false) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      {authCenter ? <AuthCenterPanel /> : <QoderPanel key={app} app={app} />}
    </QueryClientProvider>,
  );
}

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
  window.matchMedia ??= vi.fn().mockReturnValue({
    matches: false,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
});
beforeEach(() => {
  authState = {
    qoder: { installed: true, loggedIn: false },
    "qoder-cn": { installed: true, loggedIn: false },
  };
  loginProgress = {
    qoder: { complete: false },
    "qoder-cn": { complete: false },
  };
  state = {
    qoder: { providers: {}, currentModel: "auto" },
    "qoder-cn": { providers: {}, currentModel: "auto" },
  };
  mocks.invoke
    .mockReset()
    .mockImplementation(
      async (command: string, args: Record<string, unknown> = {}) => {
        const app = args.app as QoderAppId;
        if (command === "get_qoder_models") return structuredClone(state[app]);
        if (command === "get_qoder_auth_status")
          return structuredClone(authState[app]);
        if (command === "use_qoder_account") {
          const account = authState[app].accounts!.find(
            (item) => item.id === args.accountId,
          )!;
          authState[app] = {
            ...authState[app],
            loggedIn: true,
            username: account.username,
            email: account.email,
            plan: account.plan,
            currentAccountId: account.id,
            accounts: authState[app].accounts!.map((item) => ({
              ...item,
              isCurrent: item.id === account.id,
            })),
          };
          state[app].currentModel = "auto";
          return structuredClone(authState[app]);
        }
        if (command === "delete_qoder_account") {
          const account = authState[app].accounts!.find(
            (item) => item.id === args.accountId,
          )!;
          authState[app].accounts = authState[app].accounts!.filter(
            (item) => item.id !== account.id,
          );
          if (account.isCurrent)
            Object.assign(authState[app], {
              loggedIn: false,
              currentAccountId: null,
              username: null,
              email: null,
              plan: null,
            });
          return structuredClone(authState[app]);
        }
        if (command === "start_qoder_login") return `${app}-login`;
        if (command === "poll_qoder_login")
          return structuredClone(loginProgress[app]);
        if (command === "cancel_qoder_login" || command === "open_external")
          return;
        if (command === "get_settings")
          return { usageConfirmed: true, qoderEdition: "global" };
        if (command === "queryProviderUsage")
          return {
            success: true,
            data: [{ remaining: 42, total: 100, used: 58, unit: "USD" }],
          };
        if (command === "save_qoder_usage_script") {
          state[app].metadata = {
            ...state[app].metadata,
            [args.id as string]: { usage_script: args.script as UsageScript },
          };
          return structuredClone(state[app]);
        }
        if (command === "save_qoder_provider") {
          state[app].providers[args.id as string] =
            args.provider as QoderProvider;
          if (args.makeDefault)
            state[app].currentModel =
              `${args.id}/${(args.provider as QoderProvider).model}`;
          return structuredClone(state[app]);
        }
        if (command === "set_qoder_model") {
          state[app].currentModel = args.model as string;
          return structuredClone(state[app]);
        }
        if (command === "delete_qoder_provider") {
          delete state[app].providers[args.id as string];
          state[app].currentModel = "auto";
          return structuredClone(state[app]);
        }
        throw new Error(`Unexpected command: ${command}`);
      },
    );
});

describe("Qoder model management", () => {
  async function openAccountLogin(app: QoderAppId) {
    const user = userEvent.setup();
    mount(app);
    await user.click(
      (
        await screen.findAllByRole("button", { name: "provider.addProvider" })
      )[0],
    );
    await user.click(
      screen.getByRole("button", { name: /^providerPreset.group.login / }),
    );
    await user.click(
      screen.getByRole("button", {
        name:
          app === "qoder-cn"
            ? "qoder.officialAccountCn"
            : "qoder.officialAccount",
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "qoder.manageAccounts" }),
    );
    return user;
  }

  it.each(["qoder", "qoder-cn"] as const)(
    "signs in to the official %s account from the login category and enables native models",
    async (app) => {
      state[app] = {
        providers: { native: structuredClone(provider) },
        currentModel: "native/model-one",
      };
      const providersBefore = structuredClone(state[app].providers);
      const otherEdition = app === "qoder" ? "qoder-cn" : "qoder";
      const url =
        app === "qoder"
          ? "https://qoder.com/login?code=fixture"
          : "https://qoder.com.cn/login?code=fixture";
      loginProgress[app] = { complete: false, authUrl: url };
      const user = await openAccountLogin(app);
      expect(screen.queryByLabelText("qoder.baseUrl")).not.toBeInTheDocument();
      expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
      await user.click(
        screen.getByRole("button", { name: "qoder.loginWithBrowser" }),
      );
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("start_qoder_login", {
          app,
          accountId: null,
        }),
      );
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("open_external", { url }),
      );
      authState[app] = {
        installed: true,
        loggedIn: false,
        accounts: [
          {
            id: "new-account",
            username: "Fixture Account",
            plan: "Pro",
            isCurrent: false,
          },
        ],
      };
      loginProgress[app] = { complete: true, accountId: "new-account" };
      await screen.findByText("Fixture Account", {}, { timeout: 4000 });
      expect(state[app].currentModel).toBe("native/model-one");
      await user.click(
        screen.getByRole("button", { name: "qoder.useAccount" }),
      );
      await waitFor(
        () =>
          expect(mocks.invoke).toHaveBeenCalledWith("use_qoder_account", {
            app,
            accountId: "new-account",
          }),
        { timeout: 4000 },
      );
      await screen.findByText("qoder.currentAccount");
      expect(screen.getByRole("heading", { name: "nav.auth" })).toBeVisible();
      await user.click(screen.getByRole("button", { name: "common.back" }));
      await user.click(
        screen.getByRole("button", { name: "providerPreset.backToPick" }),
      );
      await user.click(
        screen.getByRole("button", { name: "provider.backToList" }),
      );
      await waitFor(() =>
        expect(
          screen.queryByRole("heading", { name: "nav.auth" }),
        ).not.toBeInTheDocument(),
      );
      expect(screen.getByText("Fixture Account · Pro")).toBeInTheDocument();
      expect(state[app].providers).toEqual(providersBefore);
      expect(authState[otherEdition].loggedIn).toBe(false);
      expect(
        mocks.invoke.mock.calls.filter(
          ([command]) => command === "open_external",
        ),
      ).toHaveLength(1);
      expect(
        screen.queryByText("provider.addNewProvider"),
      ).not.toBeInTheDocument();
    },
  );

  function savedAccounts(app: QoderAppId) {
    authState[app] = {
      installed: true,
      loggedIn: true,
      username: "Existing account",
      currentAccountId: "existing",
      accounts: [
        {
          id: "existing",
          username: "Existing account",
          plan: "Pro",
          isCurrent: true,
        },
        {
          id: "saved",
          username: "Saved account",
          plan: "Free",
          isCurrent: false,
        },
      ],
    };
    state[app] = {
      providers: { native: structuredClone(provider) },
      currentModel: "native/model-one",
    };
  }
  const accountRow = (name: string) =>
    within(screen.getByRole("list"))
      .getAllByRole("listitem")
      .find((row) => within(row).queryByText(name))!;

  it("manages both editions from the sidebar Authorization Center using the saved edition", async () => {
    savedAccounts("qoder");
    authState["qoder-cn"].accounts = [
      { id: "cn-only", username: "China account", isCurrent: false },
    ];
    const user = userEvent.setup();
    mount("qoder", true);
    await screen.findByText("Existing account");
    expect(
      screen.getByRole("combobox", { name: "qoder.edition" }),
    ).toHaveTextContent("qoder.globalEdition");
    await user.click(screen.getByRole("combobox", { name: "qoder.edition" }));
    await user.click(
      screen.getByRole("option", { name: "qoder.chinaEdition" }),
    );
    await screen.findByText("China account");
    expect(screen.queryByText("Existing account")).not.toBeInTheDocument();
    await user.click(
      within(accountRow("China account")).getByRole("button", {
        name: "qoder.useAccount",
      }),
    );
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("use_qoder_account", {
        app: "qoder-cn",
        accountId: "cn-only",
      }),
    );
    expect(authState.qoder.currentAccountId).toBe("existing");
    expect(state.qoder.currentModel).toBe("native/model-one");
  });

  it.each(["qoder", "qoder-cn"] as const)(
    "reauthenticates the selected %s account without switching models or creating another row",
    async (app) => {
      savedAccounts(app);
      const user = await openAccountLogin(app);
      await user.click(
        within(accountRow("Saved account")).getByRole("button", {
          name: "qoder.signInAgain",
        }),
      );
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("start_qoder_login", {
          app,
          accountId: "saved",
        }),
      );
      expect(
        screen.getByRole("combobox", { name: "qoder.edition" }),
      ).toBeDisabled();
      authState[app].accounts![1].plan = "Pro";
      loginProgress[app] = { complete: true, accountId: "saved" };
      await waitFor(
        () =>
          expect(
            within(accountRow("Saved account")).getByText("Pro"),
          ).toBeInTheDocument(),
        { timeout: 4000 },
      );
      expect(authState[app].accounts).toHaveLength(2);
      expect(authState[app].currentAccountId).toBe("existing");
      expect(state[app].currentModel).toBe("native/model-one");
      expect(screen.getByRole("heading", { name: "nav.auth" })).toBeVisible();
    },
  );

  it("refreshes the supplier's active status after switching accounts in the Authorization Center", async () => {
    savedAccounts("qoder");
    const user = await openAccountLogin("qoder");
    await user.click(
      within(accountRow("Saved account")).getByRole("button", {
        name: "qoder.useAccount",
      }),
    );
    await waitFor(() => expect(authState.qoder.currentAccountId).toBe("saved"));
    await user.click(screen.getByRole("button", { name: "common.back" }));
    await user.click(
      screen.getByRole("button", { name: "providerPreset.backToPick" }),
    );
    await user.click(
      screen.getByRole("button", { name: "provider.backToList" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "nav.auth" }),
      ).not.toBeInTheDocument(),
    );
    const native = within(
      screen
        .getByRole("heading", { name: "Native provider" })
        .closest(".group") as HTMLElement,
    );
    expect(
      native.queryByText("providerCard.status.inUse"),
    ).not.toBeInTheDocument();
    expect(
      native.getByRole("button", { name: "providerCard.action.switch" }),
    ).toBeVisible();
    expect(screen.getByText("Saved account · Free")).toBeInTheDocument();
  });

  it("returns to the account preset after Escape without closing the provider dialog", async () => {
    const user = await openAccountLogin("qoder");
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "nav.auth" }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("heading", { name: "provider.addNewProvider" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "qoder.manageAccounts" }),
    ).toBeVisible();
  });

  it.each(["qoder", "qoder-cn"] as const)(
    "returns directly to the %s configuration page after editing official accounts",
    async (app) => {
      savedAccounts(app);
      const user = userEvent.setup();
      mount(app);
      const officialCard = within(
        (
          await screen.findByRole("heading", {
            name: app === "qoder" ? "Qoder CLI" : "Qoder CLI CN",
          })
        ).closest(".group") as HTMLElement,
      );
      await user.click(
        officialCard.getByRole("button", { name: "providerCard.action.edit" }),
      );
      expect(
        await screen.findByRole("button", { name: "qoder.addAccount" }),
      ).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "common.back" }));
      await waitFor(() =>
        expect(
          screen.queryByRole("button", { name: "qoder.addAccount" }),
        ).not.toBeInTheDocument(),
      );
      expect(
        screen.queryByRole("textbox", {
          name: "providerPreset.searchAriaLabel",
        }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("heading", { name: "Native provider" }),
      ).toBeVisible();
      expect(
        screen.getByRole("button", { name: "provider.addProvider" }),
      ).toBeVisible();
    },
  );

  it.each(["qoder", "qoder-cn"] as const)(
    "adds another %s account on this page without changing the current login",
    async (app) => {
      savedAccounts(app);
      const user = await openAccountLogin(app);
      expect(screen.getByRole("heading", { name: "nav.auth" })).toBeVisible();
      await user.click(
        screen.getByRole("button", { name: "qoder.addAccount" }),
      );
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("start_qoder_login", {
          app,
          accountId: null,
        }),
      );
      authState[app].accounts!.push({
        id: "added",
        username: "Added account",
        isCurrent: false,
      });
      loginProgress[app] = { complete: true, accountId: "added" };
      await screen.findByText("Added account", {}, { timeout: 4000 });
      expect(screen.getByText("provider.addNewProvider")).toBeInTheDocument();
      expect(authState[app].currentAccountId).toBe("existing");
      expect(authState[app].accounts).toHaveLength(3);
      expect(state[app].currentModel).toBe("native/model-one");
      expect(mocks.invoke).not.toHaveBeenCalledWith(
        "use_qoder_account",
        expect.anything(),
      );
    },
  );

  it.each(["qoder", "qoder-cn"] as const)(
    "confirms removal of a saved %s account and preserves the current login and other edition",
    async (app) => {
      savedAccounts(app);
      const other = app === "qoder" ? "qoder-cn" : "qoder";
      savedAccounts(other);
      const untouched = structuredClone(authState[other]);
      const user = await openAccountLogin(app);
      const trash = () =>
        within(accountRow("Saved account")).getByRole("button", {
          name: "qoder.deleteAccount",
        });
      await user.click(trash());
      expect(await screen.findByRole("dialog")).toHaveTextContent(
        "qoder.deleteAccountConfirm",
      );
      expect(mocks.invoke).not.toHaveBeenCalledWith(
        "delete_qoder_account",
        expect.anything(),
      );
      await user.click(
        within(screen.getByRole("dialog")).getByRole("button", {
          name: "common.cancel",
        }),
      );
      expect(authState[app].accounts).toHaveLength(2);
      await user.click(trash());
      await user.click(
        within(screen.getByRole("dialog")).getByRole("button", {
          name: "common.delete",
        }),
      );
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("delete_qoder_account", {
          app,
          accountId: "saved",
        }),
      );
      expect(screen.queryByText("Saved account")).not.toBeInTheDocument();
      expect(authState[app].currentAccountId).toBe("existing");
      expect(authState[other]).toEqual(untouched);
      expect(state[app].currentModel).toBe("native/model-one");
    },
  );

  it("warns before deleting the current account and keeps the account page open afterwards", async () => {
    savedAccounts("qoder-cn");
    const user = await openAccountLogin("qoder-cn");
    await user.click(
      within(accountRow("Existing account")).getByRole("button", {
        name: "qoder.deleteAccount",
      }),
    );
    expect(await screen.findByRole("dialog")).toHaveTextContent(
      "qoder.deleteCurrentAccountConfirm",
    );
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.delete",
      }),
    );
    await waitFor(() => expect(authState["qoder-cn"].loggedIn).toBe(false));
    expect(screen.queryByText("Existing account")).not.toBeInTheDocument();
    expect(screen.getByText("Saved account")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "qoder.addAccount" }),
    ).toBeEnabled();
  });

  it("keeps a failed deletion in its confirmation dialog for retry", async () => {
    savedAccounts("qoder-cn");
    const invoke = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command, args) =>
      command === "delete_qoder_account"
        ? Promise.reject(new Error("Native credential is busy"))
        : invoke(command, args),
    );
    const user = await openAccountLogin("qoder-cn");
    await user.click(
      within(accountRow("Saved account")).getByRole("button", {
        name: "qoder.deleteAccount",
      }),
    );
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.delete",
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("dialog")).toHaveTextContent(
        "Native credential is busy",
      ),
    );
    expect(authState["qoder-cn"].accounts).toHaveLength(2);
    expect(authState["qoder-cn"].currentAccountId).toBe("existing");
  });

  it("cancels the selected edition's pending login when leaving the Authorization Center", async () => {
    const user = await openAccountLogin("qoder-cn");
    await user.click(
      screen.getByRole("button", { name: "qoder.loginWithBrowser" }),
    );
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("poll_qoder_login", {
        app: "qoder-cn",
        flowId: "qoder-cn-login",
      }),
    );
    await user.click(screen.getByRole("button", { name: "common.back" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("cancel_qoder_login", {
        app: "qoder-cn",
        flowId: "qoder-cn-login",
      }),
    );
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "set_qoder_model",
      expect.anything(),
    );
  });

  it("cancels a late start response after leaving the account screen", async () => {
    let resolveStart!: (id: string) => void;
    const delayed = new Promise<string>((resolve) => {
      resolveStart = resolve;
    });
    const invoke = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command, args) =>
      command === "start_qoder_login" ? delayed : invoke(command, args),
    );
    const user = await openAccountLogin("qoder");
    await user.click(
      screen.getByRole("button", { name: "qoder.loginWithBrowser" }),
    );
    await user.click(screen.getByRole("button", { name: "common.back" }));
    await act(async () => resolveStart("late-login"));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("cancel_qoder_login", {
        app: "qoder",
        flowId: "late-login",
      }),
    );
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "open_external",
      expect.anything(),
    );
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "set_qoder_model",
      expect.anything(),
    );
  });

  it("shows native login failures without switching away from the current provider", async () => {
    state["qoder-cn"] = {
      providers: { native: structuredClone(provider) },
      currentModel: "native/model-one",
    };
    loginProgress["qoder-cn"] = {
      complete: false,
      error: "Native browser login denied",
    };
    const user = await openAccountLogin("qoder-cn");
    await user.click(
      screen.getByRole("button", { name: "qoder.loginWithBrowser" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Native browser login denied",
    );
    expect(state["qoder-cn"].currentModel).toBe("native/model-one");
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "set_qoder_model",
      expect.anything(),
    );
  });

  it("requires the selected edition to be installed before starting browser login", async () => {
    authState["qoder-cn"].installed = false;
    await openAccountLogin("qoder-cn");
    expect(
      await screen.findByText("qoder.loginInstallRequired"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "qoder.loginWithBrowser" }),
    ).toBeDisabled();
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "start_qoder_login",
      expect.anything(),
    );
  });

  it.each([
    ["qoder", true],
    ["qoder-cn", true],
    ["qoder", false],
    ["qoder-cn", false],
  ] as const)(
    "switches %s directly using its default model (declared: %s) and moves the active status",
    async (app, hasDefault) => {
      state[app] = {
        providers: {
          native: {
            ...structuredClone(provider),
            model: hasDefault ? "vendor/model-two" : undefined,
            models: [
              { model: "model-one", displayName: "Model One" },
              { model: "vendor/model-two", displayName: "Model Two" },
            ],
          },
          other: {
            ...structuredClone(provider),
            displayName: "Other provider",
          },
        },
        currentModel: "other/model-one",
      };
      const otherEdition = app === "qoder" ? "qoder-cn" : "qoder";
      state[otherEdition].providers.untouched = structuredClone(provider);
      const untouched = structuredClone(state[otherEdition]);
      const user = userEvent.setup();
      mount(app);
      const target = within(
        (
          await screen.findByRole("heading", { name: "Native provider" })
        ).closest(".group") as HTMLElement,
      );
      const previous = within(
        screen
          .getByRole("heading", { name: "Other provider" })
          .closest(".group") as HTMLElement,
      );
      expect(
        previous.getByText("providerCard.status.inUse"),
      ).toBeInTheDocument();
      expect(
        previous.queryByRole("button", { name: "providerCard.action.switch" }),
      ).not.toBeInTheDocument();
      await user.click(
        target.getByRole("button", { name: "providerCard.action.switch" }),
      );
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("set_qoder_model", {
          app,
          model: hasDefault ? "native/vendor/model-two" : "native/model-one",
        }),
      );
      await waitFor(() =>
        expect(
          target.getByText("providerCard.status.inUse"),
        ).toBeInTheDocument(),
      );
      expect(
        target.queryByRole("button", { name: "providerCard.action.switch" }),
      ).not.toBeInTheDocument();
      expect(
        previous.queryByText("providerCard.status.inUse"),
      ).not.toBeInTheDocument();
      expect(
        previous.getByRole("button", { name: "providerCard.action.switch" }),
      ).toBeInTheDocument();
      expect(state[otherEdition]).toEqual(untouched);
    },
  );

  it("prevents repeated switches while saving and keeps the active provider on failure", async () => {
    state["qoder-cn"] = {
      providers: {
        native: structuredClone(provider),
        other: { ...structuredClone(provider), displayName: "Other provider" },
      },
      currentModel: "other/model-one",
    };
    let rejectSwitch!: (error: Error) => void;
    const pending = new Promise<QoderModels>((_resolve, reject) => {
      rejectSwitch = reject;
    });
    const invoke = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command, args) =>
      command === "set_qoder_model" ? pending : invoke(command, args),
    );
    const user = userEvent.setup();
    mount();
    const button = await screen.findByRole("button", {
      name: "providerCard.action.switch",
    });
    await user.click(button);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "providerCard.action.switch" }),
      ).toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(
      screen.getByRole("button", { name: "providerCard.action.switch" }),
    );
    expect(
      mocks.invoke.mock.calls.filter(
        ([command]) => command === "set_qoder_model",
      ),
    ).toHaveLength(1);
    await act(async () =>
      rejectSwitch(new Error("Could not save Qoder settings")),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "providerCard.action.switch" }),
      ).not.toHaveAttribute("aria-disabled", "true"),
    );
    expect(state["qoder-cn"].currentModel).toBe("other/model-one");
    const previous = within(
      screen
        .getByRole("heading", { name: "Other provider" })
        .closest(".group") as HTMLElement,
    );
    expect(previous.getByText("providerCard.status.inUse")).toBeInTheDocument();
  });

  it.each(["qoder", "qoder-cn"] as const)(
    "uses the shared active status and configures usage for %s",
    async (app) => {
      state[app] = {
        providers: { native: structuredClone(provider) },
        currentModel: "native/model-one",
      };
      const user = userEvent.setup();
      mount(app);
      expect(
        await screen.findByText("providerCard.status.inUse"),
      ).toBeInTheDocument();
      expect(screen.queryByText("qoder.currentModel")).not.toBeInTheDocument();
      expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
      await user.click(
        screen.getByRole("button", { name: "providerCard.action.more" }),
      );
      await user.click(
        await screen.findByRole("menuitem", {
          name: "provider.configureUsage",
        }),
      );
      await user.click(screen.getByRole("button", { name: "Save usage" }));
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("save_qoder_usage_script", {
          app,
          id: "native",
          script: expect.objectContaining({
            enabled: true,
            autoQueryInterval: 15,
          }),
        }),
      );
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("queryProviderUsage", {
          app,
          providerId: "native",
        }),
      );
      expect(state[app].providers.native).toEqual(provider);
    },
  );
  it("uses the common provider empty state and imports native configuration without CLI instructions", async () => {
    const user = userEvent.setup();
    mount();
    expect(
      await screen.findByRole("heading", { name: "provider.noProviders" }),
    ).toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "provider.addProvider" }),
    ).toHaveLength(2);
    expect(
      screen.queryByText("qoder.modelsDescription"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/\/model|Add custom model/),
    ).not.toBeInTheDocument();
    state["qoder-cn"].providers.native = structuredClone(provider);
    await user.click(
      screen.getByRole("button", { name: "provider.importCurrent" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Native provider" }),
    ).toBeInTheDocument();
    expect(mocks.invoke).toHaveBeenCalledWith("get_qoder_models", {
      app: "qoder-cn",
    });
    expect(
      mocks.invoke.mock.calls.some(([command]) =>
        /terminal|lifecycle|shell/.test(command),
      ),
    ).toBe(false);
  });

  it("creates and activates a native provider entirely through the form", async () => {
    const user = userEvent.setup();
    mount();
    await user.click(
      (
        await screen.findAllByRole("button", { name: "provider.addProvider" })
      )[0],
    );
    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    const dialog = within(screen.getByRole("form"));
    await user.type(dialog.getByLabelText("qoder.providerName"), "My API");
    await user.type(dialog.getByLabelText("qoder.providerId"), "my-api");
    await user.type(
      dialog.getByLabelText("qoder.baseUrl"),
      "https://example.test/v1",
    );
    await user.type(dialog.getByLabelText("API Key"), "example-key");
    await user.type(dialog.getByLabelText("qoder.modelId"), "vendor/model");
    await user.type(dialog.getByLabelText("qoder.contextWindow"), "128000");
    await user.type(dialog.getByLabelText("qoder.maxOutputTokens"), "8192");
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith(
        "save_qoder_provider",
        expect.objectContaining({
          app: "qoder-cn",
          id: "my-api",
          expected: null,
          makeDefault: true,
          provider: expect.objectContaining({
            displayName: "My API",
            baseUrl: "https://example.test/v1",
            protocol: "openai",
            model: "vendor/model",
            models: [
              expect.objectContaining({
                model: "vendor/model",
                contextWindow: 128000,
                maxOutputTokens: 8192,
              }),
            ],
          }),
        }),
      ),
    );
    expect(
      await screen.findByRole("heading", { name: "My API" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("combobox", { name: "qoder.currentModel" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("providerCard.status.inUse")).toBeInTheDocument();
    expect(
      mocks.invoke.mock.calls.some(([command]) =>
        /terminal|lifecycle|shell/.test(command),
      ),
    ).toBe(false);
  });

  it.each(["qoder", "qoder-cn"] as const)(
    "opens the shared picker first and saves the selected preset to %s",
    async (app) => {
      const user = userEvent.setup();
      mount(app);
      await screen.findByRole("heading", { name: "provider.noProviders" });
      const add = screen.getAllByRole("button", {
        name: "provider.addProvider",
      });
      // Cover both the header action and the empty-state entry.
      await user.click(app === "qoder" ? add[0] : add[1]);
      const search = screen.getByRole("textbox", {
        name: "providerPreset.searchAriaLabel",
      });
      const categories = screen.getByRole("group", {
        name: "providerPreset.categoriesLabel",
      });
      for (const group of ["vendor", "thirdparty", "cloud", "local"]) {
        expect(categories).toHaveTextContent(`providerPreset.group.${group}`);
      }
      expect(screen.queryByRole("form")).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "common.save" }),
      ).not.toBeInTheDocument();
      await user.type(search, "deepseek");
      await user.click(screen.getByRole("button", { name: "DeepSeek" }));
      const form = within(screen.getByRole("form"));
      const preset = qoderProviderPresets.find(
        (preset) => preset.name === "DeepSeek",
      )!;
      expect(form.getByLabelText("qoder.providerName")).toHaveValue("DeepSeek");
      expect(form.getByLabelText("qoder.providerId")).toHaveValue(
        preset.providerKey,
      );
      expect(form.getByLabelText("qoder.baseUrl")).toHaveValue(
        preset.settingsConfig.baseUrl,
      );
      expect(
        form
          .getAllByLabelText("qoder.modelId")
          .map((input) => (input as HTMLInputElement).value),
      ).toEqual(preset.settingsConfig.models!.map((model) => model.model));
      await user.type(form.getByLabelText("API Key"), "example-key");
      await user.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenCalledWith("save_qoder_provider", {
          app,
          id: preset.providerKey,
          provider: { ...preset.settingsConfig, apiKey: "example-key" },
          expected: null,
          makeDefault: true,
        }),
      );
      expect(
        await screen.findByRole("heading", { name: "DeepSeek" }),
      ).toBeInTheDocument();
    },
  );

  it("filters by category and returns from the form to choose a custom configuration", async () => {
    const user = userEvent.setup();
    mount();
    await user.click(
      (
        await screen.findAllByRole("button", { name: "provider.addProvider" })
      )[0],
    );
    await user.click(
      screen.getByRole("button", { name: /^providerPreset.group.local / }),
    );
    expect(
      screen.queryByRole("button", { name: "DeepSeek" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Ollama" }));
    expect(screen.getByLabelText("qoder.baseUrl")).toHaveValue(
      "http://localhost:11434/v1",
    );
    await user.click(
      screen.getByRole("button", { name: "providerPreset.backToPick" }),
    );
    expect(
      screen.getByRole("textbox", { name: "providerPreset.searchAriaLabel" }),
    ).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    expect(screen.getByLabelText("qoder.providerName")).toHaveValue("");
    expect(screen.getByLabelText("qoder.providerId")).toHaveValue("");
    expect(screen.getByLabelText("qoder.baseUrl")).toHaveValue("");
    expect(screen.getAllByLabelText("qoder.modelId")).toHaveLength(1);
    expect(screen.getByLabelText("qoder.modelId")).toHaveValue("");
    await user.click(
      screen.getByRole("button", { name: "providerPreset.change" }),
    );
    await user.click(
      screen.getByRole("button", { name: "provider.backToList" }),
    );
    expect(
      screen.queryByRole("heading", { name: "provider.addNewProvider" }),
    ).not.toBeInTheDocument();
  });

  it("switches a supplier's region and plan using the common version controls", async () => {
    const user = userEvent.setup();
    mount();
    await user.click(
      (
        await screen.findAllByRole("button", { name: "provider.addProvider" })
      )[0],
    );
    await user.click(screen.getByRole("button", { name: "Kimi" }));
    expect(screen.getByLabelText("qoder.baseUrl")).toHaveValue(
      "https://api.moonshot.cn/v1",
    );
    const regions = within(
      screen.getByRole("group", { name: "providerPreset.regionLabel" }),
    ).getAllByRole("button");
    await user.click(regions[1]);
    expect(screen.getByLabelText("qoder.baseUrl")).toHaveValue(
      "https://api.moonshot.ai/v1",
    );
    const plans = within(
      screen.getByRole("group", { name: "providerPreset.planLabel" }),
    ).getAllByRole("button");
    await user.click(plans[1]);
    expect(screen.getByLabelText("qoder.providerId")).toHaveValue(
      "cc-switch-kimi-for-coding-global",
    );
    expect(screen.getByLabelText("qoder.baseUrl")).toHaveValue(
      "https://api.kimi.ai/coding",
    );
    expect(
      screen.getByRole("combobox", { name: "qoder.protocol" }),
    ).toHaveTextContent("Anthropic Messages");
    expect(
      screen.getByRole("combobox", { name: "qoder.authType" }),
    ).toHaveTextContent("API Key (x-api-key)");
  });

  it("edits native fields while retaining environment keys, model capabilities and unknown settings", async () => {
    state.qoder = {
      providers: { native: structuredClone(provider) },
      currentModel: "native/model-one",
    };
    const user = userEvent.setup();
    mount("qoder");
    await user.click(
      await screen.findByRole("button", { name: "providerCard.action.edit" }),
    );
    const dialog = within(screen.getByRole("form"));
    expect(dialog.getByLabelText("qoder.providerId")).toHaveAttribute(
      "readonly",
    );
    await user.clear(dialog.getByLabelText("qoder.providerName"));
    await user.type(
      dialog.getByLabelText("qoder.providerName"),
      "Updated provider",
    );
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("save_qoder_provider", {
        app: "qoder",
        id: "native",
        provider: { ...provider, displayName: "Updated provider" },
        expected: provider,
        makeDefault: true,
      }),
    );
  });

  it("selects a default model and deletes a provider without affecting the other edition", async () => {
    state["qoder-cn"].providers.native = structuredClone(provider);
    const user = userEvent.setup();
    mount();
    await user.click(
      await screen.findByRole("button", { name: "qoder.selectModelAction" }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: /^Model One/ }),
    );
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("set_qoder_model", {
        app: "qoder-cn",
        model: "native/model-one",
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "providerCard.action.more" }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: "common.delete" }),
    );
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.delete",
      }),
    );
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("delete_qoder_provider", {
        app: "qoder-cn",
        id: "native",
        expected: provider,
      }),
    );
    expect(
      await screen.findByRole("heading", { name: "provider.noProviders" }),
    ).toBeInTheDocument();
    expect(
      mocks.invoke.mock.calls
        .filter(([command]) => command.includes("qoder"))
        .every(([, args]) => args.app === "qoder-cn"),
    ).toBe(true);
  });

  it("keeps the editor open when a concurrent external update is reported", async () => {
    state.qoder.providers.native = structuredClone(provider);
    const invoke = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command, args) =>
      command === "save_qoder_provider"
        ? Promise.reject(new Error("Provider changed outside this editor"))
        : invoke(command, args),
    );
    const user = userEvent.setup();
    mount("qoder");
    await user.click(
      await screen.findByRole("button", { name: "providerCard.action.edit" }),
    );
    await user.click(screen.getByRole("button", { name: "common.save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Provider changed outside this editor",
    );
    expect(screen.getByRole("form")).toBeInTheDocument();
  });

  it("rejects duplicate model IDs and invalid token limits before saving", async () => {
    const user = userEvent.setup();
    state.qoder.providers.native = structuredClone(provider);
    mount("qoder");
    await user.click(
      await screen.findByRole("button", { name: "providerCard.action.edit" }),
    );
    const dialog = within(screen.getByRole("form"));
    fireEvent.change(dialog.getByLabelText("qoder.contextWindow"), {
      target: { value: "0" },
    });
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    fireEvent.change(dialog.getByLabelText("qoder.contextWindow"), {
      target: { value: "128000" },
    });
    await user.click(dialog.getByRole("button", { name: "qoder.addModel" }));
    const ids = dialog.getAllByLabelText("qoder.modelId");
    await user.type(ids[1], "model-one");
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "save_qoder_provider",
      expect.anything(),
    );
  });
});

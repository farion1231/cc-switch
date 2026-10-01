import { createRef } from "react";
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
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createInstance, type i18n } from "i18next";
import { I18nextProvider } from "react-i18next";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";
import VpsPanel, { type VpsPanelHandle } from "@/components/vps/VpsPanel";
import type { VpsServer } from "@/lib/api/vps";
import { SKILLS_APP_IDS } from "@/config/appConfig";

const mocks = vi.hoisted(() => ({
  getServers: vi.fn(),
  saveServer: vi.fn(),
  deleteServer: vi.fn(),
  testConnection: vi.fn(),
  cancelConnectionTest: vi.fn(),
  confirmHostKey: vi.fn(),
  error: vi.fn(),
  success: vi.fn(),
  info: vi.fn(),
}));
vi.mock("@/lib/api/vps", async (original) => ({
  ...(await original<typeof import("@/lib/api/vps")>()),
  vpsApi: mocks,
}));
vi.mock("sonner", () => ({
  toast: { error: mocks.error, success: mocks.success, info: mocks.info },
}));

const host: VpsServer = {
  id: "00000000-0000-4000-8000-000000000001",
  name: "Test host",
  purpose: "Preview",
  host: "192.0.2.10",
  port: 22,
  user: "deploy",
  apps: {
    claude: false,
    codex: false,
    gemini: false,
    opencode: false,
    openclaw: false,
    hermes: false,
    pi: false,
  },
};

function renderPanel(translations?: i18n) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const ref = createRef<VpsPanelHandle>();
  const navigation = vi.fn();
  const panel = (
    <QueryClientProvider client={client}>
      <VpsPanel ref={ref} onNavigationBlockedChange={navigation} />
    </QueryClientProvider>
  );
  const result = render(
    translations ? (
      <I18nextProvider i18n={translations}>{panel}</I18nextProvider>
    ) : (
      panel
    ),
  );
  return { ...result, ref, navigation, client };
}

async function openNew(ref: React.RefObject<VpsPanelHandle>) {
  await act(async () => ref.current?.openAdd());
  fireEvent.change(screen.getByLabelText("vps.fields.name"), {
    target: { value: "New host" },
  });
  fireEvent.change(screen.getByLabelText("vps.fields.host"), {
    target: { value: "192.0.2.12" },
  });
  fireEvent.change(screen.getByLabelText("vps.fields.user"), {
    target: { value: "deploy" },
  });
  fireEvent.change(screen.getByLabelText("vps.auth.password"), {
    target: { value: "test-only password" },
  });
}

function editorPanel(): HTMLElement {
  return screen
    .getByRole("form", { name: "vps.add" })
    .closest(".fixed") as HTMLElement;
}

describe("VpsPanel", () => {
  let scrollIntoViewDescriptor: PropertyDescriptor | undefined;

  beforeEach(() => {
    scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "scrollIntoView",
    );
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    Object.values(mocks).forEach((mock) => mock.mockReset());
    mocks.getServers.mockResolvedValue([host]);
    mocks.saveServer.mockImplementation(async (server: VpsServer) => [server]);
    mocks.deleteServer.mockResolvedValue([]);
    mocks.testConnection.mockResolvedValue({ status: "success" });
    mocks.cancelConnectionTest.mockResolvedValue(undefined);
    mocks.confirmHostKey.mockResolvedValue(undefined);
  });

  afterEach(() => {
    if (scrollIntoViewDescriptor) {
      Object.defineProperty(
        HTMLElement.prototype,
        "scrollIntoView",
        scrollIntoViewDescriptor,
      );
    } else {
      Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
    }
  });

  it.each(["button", "escape"] as const)(
    "closes client-access help with %s and restores navigation and focus",
    async (method) => {
      const { ref, navigation } = renderPanel();
      await screen.findByText(host.name);
      const user = userEvent.setup();
      const help = screen.getByRole("button", { name: "vps.help.title" });
      await user.click(help);
      const dialog = await screen.findByRole("dialog", {
        name: "vps.help.title",
      });
      expect(navigation).toHaveBeenLastCalledWith(true);
      if (method === "button") {
        await user.click(
          within(dialog).getByRole("button", { name: "common.close" }),
        );
      } else {
        await user.keyboard("{Escape}");
      }
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
      expect(navigation).toHaveBeenLastCalledWith(false);
      await waitFor(() => expect(help).toHaveFocus());
      await act(async () => ref.current?.openAdd());
      expect(screen.getByRole("form", { name: "vps.add" })).toBeInTheDocument();
      expect(mocks.saveServer).not.toHaveBeenCalled();
      expect(mocks.testConnection).not.toHaveBeenCalled();
    },
  );

  it("groups host details, SSH authentication and clients without changing the saved data", async () => {
    const { ref } = renderPanel();
    await screen.findByText(host.name);
    await openNew(ref);
    const details = screen.getByRole("group", { name: "vps.sections.host" });
    const authentication = screen.getByRole("group", {
      name: "vps.sections.authentication",
    });
    const clients = screen.getByRole("group", { name: "vps.fields.clients" });
    expect(within(details).getByLabelText("vps.fields.name")).toHaveValue(
      "New host",
    );
    expect(within(details).getByLabelText("vps.fields.host")).toHaveValue(
      "192.0.2.12",
    );
    expect(within(details).getByLabelText("vps.fields.port")).toHaveValue(22);
    expect(
      within(details).getByLabelText("vps.fields.purpose"),
    ).toBeInTheDocument();
    expect(
      within(authentication).getByLabelText("vps.fields.user"),
    ).toHaveValue("deploy");
    expect(
      within(authentication).getByRole("combobox", { name: "vps.auth.method" }),
    ).toBeInTheDocument();
    expect(
      within(authentication).getByLabelText("vps.auth.password"),
    ).toHaveAttribute("type", "password");
    expect(within(clients).getAllByRole("checkbox")).toHaveLength(
      SKILLS_APP_IDS.length,
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.saveServer).toHaveBeenCalledTimes(1));
    expect(mocks.saveServer.mock.calls[0][0]).toMatchObject({
      name: "New host",
      host: "192.0.2.12",
      port: 22,
      user: "deploy",
      authMethod: "password",
    });
    expect(mocks.saveServer.mock.calls[0][1]).toBe("test-only password");
  });

  it("shows supported client controls without a second Skill switch or status", async () => {
    renderPanel();
    await screen.findByText(host.name);
    const group = screen.getByRole("group", { name: "vps.clientsFor" });
    expect(within(group).getAllByRole("button")).toHaveLength(
      SKILLS_APP_IDS.length,
    );
    expect(
      within(group).queryByRole("button", { name: "OpenClaw" }),
    ).toBeNull();
    expect(
      within(group).queryByRole("button", { name: "Claude Desktop" }),
    ).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByText("Skill configured")).toBeNull();
  });

  it("selects a client by saving once and waits for success before checking it", async () => {
    let resolve!: (servers: VpsServer[]) => void;
    mocks.saveServer.mockReturnValue(
      new Promise<VpsServer[]>((done) => {
        resolve = done;
      }),
    );
    renderPanel();
    await screen.findByText(host.name);
    const button = within(
      screen.getByRole("group", { name: "vps.clientsFor" }),
    ).getByRole("button", { name: "Claude" });
    fireEvent.click(button);
    fireEvent.click(button);
    await waitFor(() => expect(mocks.saveServer).toHaveBeenCalledTimes(1));
    expect(button).toHaveAttribute("aria-pressed", "false");
    expect(button).toBeDisabled();
    const saved = { ...host, apps: { ...host.apps, claude: true } };
    await act(async () => resolve([saved]));
    await waitFor(() => expect(button).toHaveAttribute("aria-pressed", "true"));
    expect(mocks.saveServer).toHaveBeenCalledTimes(1);
  });

  it("keeps a failed client binding unchecked and reports the failure", async () => {
    mocks.saveServer.mockRejectedValue(new Error("deployment conflict"));
    renderPanel();
    await screen.findByText(host.name);
    const button = screen.getByRole("button", { name: "Claude" });
    fireEvent.click(button);
    await waitFor(() => expect(mocks.error).toHaveBeenCalled());
    expect(button).toHaveAttribute("aria-pressed", "false");
    expect(mocks.success).not.toHaveBeenCalled();
  });

  it("blocks stale actions after a partial save and preserves committed bindings after reconciliation", async () => {
    const committed = { ...host, apps: { ...host.apps, claude: true } };
    mocks.getServers
      .mockResolvedValueOnce([host])
      .mockRejectedValueOnce(new Error("reconciliation still blocked"))
      .mockResolvedValueOnce([committed]);
    mocks.saveServer.mockRejectedValueOnce(
      new Error("post-commit deployment failed"),
    );
    renderPanel();
    await screen.findByText(host.name);
    fireEvent.click(screen.getByRole("button", { name: "Claude" }));
    await screen.findByRole("alert");
    const codex = screen.getByRole("button", { name: "Codex" });
    expect(codex).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "vps.testConnection" }),
    ).toBeDisabled();
    fireEvent.click(codex);
    expect(mocks.saveServer).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "vps.retry" }));
    await waitFor(() => expect(codex).toBeEnabled());
    expect(screen.getByRole("button", { name: "Claude" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(codex);
    await waitFor(() => expect(mocks.saveServer).toHaveBeenCalledTimes(2));
    expect(mocks.saveServer.mock.calls[1][0].apps).toMatchObject({
      claude: true,
      codex: true,
    });
    expect(mocks.testConnection).not.toHaveBeenCalled();
  });

  it("retains the form and generated ID after a rejected save", async () => {
    mocks.saveServer.mockRejectedValueOnce(new Error("write failed"));
    const { ref } = renderPanel();
    await screen.findByText(host.name);
    await openNew(ref);
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.error).toHaveBeenCalled());
    expect(screen.getByLabelText("vps.fields.name")).toHaveValue("New host");
    const first = mocks.saveServer.mock.calls[0][0];
    expect(first.id).toMatch(/^[0-9a-f-]{36}$/);
    expect(Object.values(first.apps).every((enabled) => !enabled)).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.saveServer).toHaveBeenCalledTimes(2));
    expect(mocks.saveServer.mock.calls[1][0].id).toBe(first.id);
  });

  it.each([
    ["en", en],
    ["zh", zh],
    ["zh-TW", zhTW],
    ["ja", ja],
  ] as const)(
    "localizes SSH results without raw backend messages in %s",
    async (language, locale) => {
      const translations = createInstance();
      await translations.init({
        lng: language,
        resources: { [language]: { translation: locale } },
        interpolation: { escapeValue: false },
      });
      renderPanel(translations);
      await screen.findByText(host.name);
      for (const status of [
        "success",
        "timeout",
        "authenticationFailed",
        "credentialsUnavailable",
        "passwordRequired",
        "sshNotFound",
        "hostKeyChanged",
        "targetChanged",
      ] as const) {
        mocks.testConnection.mockResolvedValueOnce({
          status,
          message: "Untranslated backend message",
        });
        const button = screen.getByRole("button", {
          name: locale.vps.testConnection,
        });
        await waitFor(() => expect(button).toBeEnabled());
        fireEvent.click(button);
        await waitFor(() =>
          expect(
            status === "success" ? mocks.success : mocks.error,
          ).toHaveBeenLastCalledWith(locale.vps.connection[status], {
            closeButton: true,
          }),
        );
      }
    },
  );

  it.each([
    "SSH fingerprint confirmation expired or was cancelled; test the connection again",
    "SSH trust could not be saved",
  ])(
    "recovers from confirmation failure without reusing the token: %s",
    async (message) => {
      mocks.testConnection
        .mockResolvedValueOnce({
          status: "hostKeyConfirmationRequired",
          fingerprint: "SHA256:old",
          confirmationToken: "old-token",
        })
        .mockResolvedValueOnce({
          status: "hostKeyConfirmationRequired",
          fingerprint: "SHA256:new",
          confirmationToken: "new-token",
        });
      mocks.confirmHostKey.mockRejectedValueOnce(new Error(message));
      const { ref } = renderPanel();
      await screen.findByText(host.name);
      await openNew(ref);
      fireEvent.click(
        within(editorPanel()).getByRole("button", {
          name: "vps.testConnection",
        }),
      );
      await screen.findByText("SHA256:old");
      const firstRequest = mocks.testConnection.mock.calls[0][1];
      fireEvent.click(
        screen.getByRole("button", { name: "vps.hostKey.confirm" }),
      );
      await waitFor(() =>
        expect(mocks.error).toHaveBeenCalledWith("vps.hostKey.confirmFailed", {
          description: "vps.hostKey.retryHint",
          closeButton: true,
        }),
      );
      await waitFor(() => expect(screen.queryByText("SHA256:old")).toBeNull());
      expect(mocks.cancelConnectionTest).toHaveBeenCalledWith(firstRequest);
      expect(mocks.testConnection).toHaveBeenCalledTimes(1);
      expect(screen.getByLabelText("vps.fields.name")).toHaveValue("New host");
      fireEvent.click(
        within(editorPanel()).getByRole("button", {
          name: "vps.testConnection",
        }),
      );
      await screen.findByText("SHA256:new");
      fireEvent.click(
        screen.getByRole("button", { name: "vps.hostKey.confirm" }),
      );
      await waitFor(() =>
        expect(mocks.confirmHostKey).toHaveBeenLastCalledWith("new-token"),
      );
      await waitFor(() =>
        expect(mocks.testConnection).toHaveBeenCalledTimes(3),
      );
    },
  );

  it("requires an explicit first fingerprint confirmation before retrying", async () => {
    mocks.testConnection.mockResolvedValueOnce({
      status: "hostKeyConfirmationRequired",
      fingerprint: "SHA256:test",
      confirmationToken: "challenge",
    });
    renderPanel();
    await screen.findByText(host.name);
    fireEvent.click(screen.getByRole("button", { name: "vps.testConnection" }));
    await screen.findByText("SHA256:test");
    expect(mocks.confirmHostKey).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "vps.hostKey.confirm" }),
    );
    await waitFor(() =>
      expect(mocks.confirmHostKey).toHaveBeenCalledWith("challenge"),
    );
    await waitFor(() => expect(mocks.testConnection).toHaveBeenCalledTimes(2));
  });

  it.each(["cancel", "unmount"] as const)(
    "revokes a pending fingerprint challenge on %s",
    async (action) => {
      mocks.testConnection.mockResolvedValueOnce({
        status: "hostKeyConfirmationRequired",
        fingerprint: "SHA256:pending",
        confirmationToken: "pending-confirmation",
      });
      const { unmount } = renderPanel();
      await screen.findByText(host.name);
      fireEvent.click(
        screen.getByRole("button", { name: "vps.testConnection" }),
      );
      await screen.findByText("SHA256:pending");
      const requestId = mocks.testConnection.mock.calls[0][1];
      if (action === "unmount") unmount();
      else
        fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
      await waitFor(() =>
        expect(mocks.cancelConnectionTest).toHaveBeenCalledWith(requestId),
      );
      expect(mocks.confirmHostKey).not.toHaveBeenCalled();
    },
  );

  it("blocks changed fingerprints without offering a trust override", async () => {
    mocks.testConnection.mockResolvedValueOnce({
      status: "hostKeyChanged",
      fingerprint: "SHA256:changed",
      confirmationToken: "must-not-use",
    });
    renderPanel();
    await screen.findByText(host.name);
    fireEvent.click(screen.getByRole("button", { name: "vps.testConnection" }));
    await waitFor(() =>
      expect(mocks.error).toHaveBeenCalledWith(
        "vps.connection.hostKeyChanged",
        expect.anything(),
      ),
    );
    expect(
      screen.queryByRole("button", { name: "vps.hostKey.confirm" }),
    ).toBeNull();
    expect(mocks.confirmHostKey).not.toHaveBeenCalled();
  });

  it("cancels only its pending connection request and suppresses late success", async () => {
    let resolve!: (result: { status: "success" }) => void;
    mocks.testConnection.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    renderPanel();
    await screen.findByText(host.name);
    fireEvent.click(screen.getByRole("button", { name: "vps.testConnection" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "vps.cancelTest" }),
    );
    const requestId = mocks.testConnection.mock.calls[0][1];
    await waitFor(() =>
      expect(mocks.cancelConnectionTest).toHaveBeenCalledWith(requestId),
    );
    await act(async () => resolve({ status: "success" }));
    expect(mocks.success).not.toHaveBeenCalled();
  });

  it("shows load failures with a retry action instead of an empty success state", async () => {
    mocks.getServers.mockRejectedValueOnce(new Error("owned file changed"));
    renderPanel();
    await screen.findByRole("alert");
    expect(screen.queryByText("vps.empty.title")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "vps.retry" }));
    await screen.findByText(host.name);
  });

  it("tests unsaved connection details without saving or assigning a default client", async () => {
    const { ref } = renderPanel();
    await screen.findByText(host.name);
    await openNew(ref);
    const editor = editorPanel();
    fireEvent.click(
      within(editor).getByRole("button", { name: "vps.testConnection" }),
    );
    await waitFor(() => expect(mocks.testConnection).toHaveBeenCalledTimes(1));
    const draft = mocks.testConnection.mock.calls[0][0];
    expect(draft.host).toBe("192.0.2.12");
    expect(draft.id).toMatch(/^[0-9a-f-]{36}$/);
    expect(Object.values(draft.apps).every((enabled) => !enabled)).toBe(true);
    expect(mocks.saveServer).not.toHaveBeenCalled();
  });

  it("cancels a pending probe on unmount and does not show a late result", async () => {
    let resolve!: (result: { status: "success" }) => void;
    mocks.testConnection.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const { unmount } = renderPanel();
    await screen.findByText(host.name);
    fireEvent.click(screen.getByRole("button", { name: "vps.testConnection" }));
    const requestId = mocks.testConnection.mock.calls[0][1];
    unmount();
    expect(mocks.cancelConnectionTest).toHaveBeenCalledWith(requestId);
    await act(async () => resolve({ status: "success" }));
    expect(mocks.success).not.toHaveBeenCalled();
  });

  it("deletes only the confirmed host and updates the list after success", async () => {
    renderPanel();
    await screen.findByText(host.name);
    fireEvent.click(screen.getByRole("button", { name: "vps.deleteServer" }));
    expect(mocks.deleteServer).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    await waitFor(() =>
      expect(mocks.deleteServer).toHaveBeenCalledWith(host.id),
    );
    await screen.findByText("vps.empty.title");
  });

  it("sends a new password separately and retains its exact characters through host confirmation", async () => {
    mocks.testConnection.mockResolvedValueOnce({
      status: "hostKeyConfirmationRequired",
      fingerprint: "SHA256:password-host",
      confirmationToken: "password-challenge",
    });
    const { ref } = renderPanel();
    await screen.findByText(host.name);
    await openNew(ref);
    fireEvent.change(screen.getByLabelText("vps.auth.password"), {
      target: { value: "  example password  " },
    });
    fireEvent.click(
      within(editorPanel()).getByRole("button", { name: "vps.testConnection" }),
    );
    await screen.findByText("SHA256:password-host");
    fireEvent.click(
      screen.getByRole("button", { name: "vps.hostKey.confirm" }),
    );
    await waitFor(() => expect(mocks.testConnection).toHaveBeenCalledTimes(2));
    expect(mocks.testConnection.mock.calls[1][2]).toBe("  example password  ");
    expect(mocks.testConnection.mock.calls[1][0]).not.toHaveProperty(
      "password",
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.saveServer).toHaveBeenCalledTimes(1));
    expect(mocks.saveServer.mock.calls[0][1]).toBe("  example password  ");
    expect(mocks.saveServer.mock.calls[0][0]).not.toHaveProperty("password");
  });

  it("keeps an existing saved password when the edit field is left empty", async () => {
    mocks.getServers.mockResolvedValue([{ ...host, authMethod: "password" }]);
    renderPanel();
    await screen.findByText(host.name);
    fireEvent.click(screen.getByRole("button", { name: "vps.editServer" }));
    expect(screen.getByLabelText("vps.auth.password")).toHaveValue("");
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.saveServer).toHaveBeenCalledTimes(1));
    expect(mocks.saveServer.mock.calls[0][1]).toBeUndefined();
  });

  it("offers password, private key and certificate without an agent option", async () => {
    const { ref } = renderPanel();
    await screen.findByText(host.name);
    await openNew(ref);
    const user = userEvent.setup();
    await user.click(screen.getByRole("combobox", { name: "vps.auth.method" }));
    expect(screen.getAllByRole("option")).toHaveLength(3);
    await user.click(
      screen.getByRole("option", { name: "vps.auth.certificate" }),
    );
    expect(screen.queryByLabelText("vps.auth.password")).toBeNull();
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("vps.fields.identityFile"), {
      target: { value: "C:/test-only/key" },
    });
    fireEvent.change(screen.getByLabelText("vps.auth.certificateFile"), {
      target: { value: "C:/test-only/key-cert.pub" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.saveServer).toHaveBeenCalledTimes(1));
    expect(mocks.saveServer.mock.calls[0][0]).toMatchObject({
      authMethod: "certificate",
      identityFile: "C:/test-only/key",
      certificateFile: "C:/test-only/key-cert.pub",
    });
    expect(mocks.saveServer.mock.calls[0][1]).toBeUndefined();
  });

  it("opens a full-screen editor with its own working back button", async () => {
    const { ref, navigation } = renderPanel();
    await screen.findByText(host.name);
    await act(async () => ref.current?.openAdd());
    const back = screen.getByRole("button", { name: "common.back" });
    expect(back).toBeEnabled();
    const form = screen.getByRole("form", { name: "vps.add" });
    expect(form.closest(".fixed")).toHaveClass("inset-0", "z-[60]");
    fireEvent.click(back);
    await waitFor(() =>
      expect(screen.queryByLabelText("vps.fields.name")).toBeNull(),
    );
    expect(navigation).toHaveBeenLastCalledWith(false);
    expect(mocks.saveServer).not.toHaveBeenCalled();
  });

  it("asks before discarding a draft through the editor back button", async () => {
    const { ref } = renderPanel();
    await screen.findByText(host.name);
    await openNew(ref);
    fireEvent.click(screen.getByRole("button", { name: "common.back" }));
    const confirmation = await screen.findByRole("dialog", {
      name: "vps.discard.title",
    });
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    );
    expect(screen.getByLabelText("vps.fields.name")).toHaveValue("New host");
    fireEvent.click(screen.getByRole("button", { name: "common.back" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "vps.discard.confirm" }),
    );
    await waitFor(() =>
      expect(screen.queryByLabelText("vps.fields.name")).toBeNull(),
    );
    expect(mocks.saveServer).not.toHaveBeenCalled();
  });

  it("uses labeled client checkboxes in the editor like the MCP form", async () => {
    const { ref } = renderPanel();
    await screen.findByText(host.name);
    await openNew(ref);
    const clients = screen.getByRole("group", { name: "vps.fields.clients" });
    expect(within(clients).getAllByRole("checkbox")).toHaveLength(
      SKILLS_APP_IDS.length,
    );
    for (const checkbox of within(clients).getAllByRole("checkbox"))
      expect(checkbox).not.toBeChecked();
    expect(within(clients).queryByText("apps.openclaw")).toBeNull();
    expect(within(clients).queryByText("apps.claude-desktop")).toBeNull();
    fireEvent.click(
      within(clients).getByRole("checkbox", { name: "apps.codex" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.saveServer).toHaveBeenCalledTimes(1));
    expect(mocks.saveServer.mock.calls[0][0].apps.codex).toBe(true);
    expect(mocks.saveServer.mock.calls[0][0].apps.claude).toBe(false);
  });

  it("protects unsaved form data and blocks navigation until explicitly discarded", async () => {
    const { ref, navigation } = renderPanel();
    await screen.findByText(host.name);
    await openNew(ref);
    await waitFor(() => expect(navigation).toHaveBeenLastCalledWith(true));
    const editor = editorPanel();
    await userEvent
      .setup()
      .click(within(editor).getByRole("button", { name: "common.cancel" }));
    await screen.findByText("vps.discard.title");
    fireEvent.click(
      screen.getByRole("button", { name: "vps.discard.confirm" }),
    );
    await waitFor(() => expect(navigation).toHaveBeenLastCalledWith(false));
  });
});

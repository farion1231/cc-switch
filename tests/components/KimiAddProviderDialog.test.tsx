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
import { kimiOAuthApi } from "@/lib/api/kimiOAuth";
import { createTestQueryClient } from "../utils/testQueryClient";

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
});

const status = {
  authenticated: false,
  region: "mainland-cn" as const,
  expiresAt: null,
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
  await screen.findByLabelText("kimi.oauth.region");
}

describe("Kimi add-provider account login", () => {
  beforeEach(() => {
    vi.mocked(kimiOAuthApi.status).mockResolvedValue(status);
    vi.mocked(kimiOAuthApi.start).mockResolvedValue(device);
    vi.mocked(kimiOAuthApi.cancel).mockResolvedValue(true);
    vi.mocked(kimiOAuthApi.useAccount).mockResolvedValue(undefined);
  });

  it("opens official login from the account group without showing API-key fields or the add footer", async () => {
    const { onSubmit } = setup();
    fireEvent.click(
      await screen.findByRole("button", {
        name: /^providerPreset.group.login /,
      }),
    );
    expect(
      screen.getByText("providerPreset.loginWith.kimi"),
    ).toBeInTheDocument();
    await selectAccount();
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "common.add" }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("provider.configJson"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("opencode.providerKey *"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "providerPreset.change" }),
    ).not.toBeInTheDocument();
    const presetBar = screen
      .getByRole("button", { name: "kimi.oauth.signIn" })
      .closest("[data-preset-selector]");
    expect(presetBar).not.toBeNull();
    expect(
      within(presetBar as HTMLElement).getByText("kimi.oauth.title"),
    ).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("cancels authorization on returning to the picker and restores the normal custom-provider footer", async () => {
    const { onSubmit } = setup();
    await selectAccount();
    fireEvent.click(screen.getByRole("button", { name: "kimi.oauth.signIn" }));
    await screen.findByText(device.userCode);
    expect(kimiOAuthApi.start).toHaveBeenCalledWith("mainland-cn");
    expect(onSubmit).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "providerPreset.backToPick" }),
    );
    await waitFor(() =>
      expect(kimiOAuthApi.cancel).toHaveBeenCalledWith(device.flowId),
    );
    expect(screen.queryByText(device.userCode)).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    expect(
      await screen.findByRole("button", { name: "common.add" }),
    ).toBeDisabled();
    expect(screen.getByLabelText("API Key")).toBeInTheDocument();
  });

  it("returns to the supplier list when using an existing native account without creating an API-key provider", async () => {
    vi.mocked(kimiOAuthApi.status).mockResolvedValue({
      ...status,
      authenticated: true,
    });
    const { onOpenChange, onSubmit } = setup();
    await selectAccount();
    fireEvent.click(
      await screen.findByRole("button", { name: "kimi.oauth.useAccount" }),
    );
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(kimiOAuthApi.useAccount).toHaveBeenCalledOnce();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("keeps the login page open when selecting the official model fails", async () => {
    vi.mocked(kimiOAuthApi.status).mockResolvedValue({
      ...status,
      authenticated: true,
    });
    vi.mocked(kimiOAuthApi.useAccount).mockRejectedValueOnce(
      "Cannot update Kimi model selection",
    );
    const { onOpenChange, onSubmit } = setup();
    await selectAccount();
    fireEvent.click(
      await screen.findByRole("button", { name: "kimi.oauth.useAccount" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Cannot update Kimi model selection",
    );
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
  });
});

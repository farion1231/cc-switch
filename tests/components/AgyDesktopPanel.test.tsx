import { QueryClientProvider } from "@tanstack/react-query";
import type { DragEndEvent } from "@dnd-kit/core";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgyDesktopPanel } from "@/components/apps/AgyDesktopPanel";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { authApi } from "@/lib/api";
import type { ManagedAuthStatus } from "@/lib/api/auth";
import { providersApi } from "@/lib/api";
import { useAddProviderMutation } from "@/lib/query/mutations";
import { toast } from "@/lib/toast";
import { createTestQueryClient } from "../utils/testQueryClient";

const drag = vi.hoisted(() => ({
  onEnd: undefined as ((event: DragEndEvent) => void) | undefined,
}));

vi.mock("@dnd-kit/core", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@dnd-kit/core")>();
  return {
    ...actual,
    DndContext: (props: React.ComponentProps<typeof actual.DndContext>) => {
      drag.onEnd = props.onDragEnd;
      return <actual.DndContext {...props} />;
    },
  };
});

vi.mock("@/components/AgyAccountQuotaFooter", () => ({
  default: () => null,
  AgyAccountQuotaFooter: () => null,
}));

describe("Antigravity Desktop providers", () => {
  beforeEach(() => {
    vi.spyOn(authApi, "authGetStatus").mockResolvedValue({
      provider: "agy_oauth",
      authenticated: false,
      default_account_id: null,
      accounts: [],
    });
    Element.prototype.scrollIntoView ??= vi.fn();
  });

  it("shows the shared provider empty state with the add action", async () => {
    const onCreate = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AgyDesktopPanel
          onCreate={onCreate}
          onDelete={vi.fn()}
          onOpenWebsite={vi.fn()}
        />
      </QueryClientProvider>,
    );

    await userEvent.click(
      await screen.findByRole("button", { name: "provider.addProvider" }),
    );
    expect(onCreate).toHaveBeenCalledOnce();
    expect(
      screen.queryByText("provider.importCurrent"),
    ).not.toBeInTheDocument();
  });

  it("uses the shared provider cards to reorder and switch Desktop accounts", async () => {
    const status: ManagedAuthStatus = {
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: "first",
      accounts: ["first", "second"].map((id) => ({
        id,
        login: `${id}@example.com`,
        provider: "agy_oauth",
        avatar_url: null,
        authenticated_at: 0,
        is_default: id === "first",
        github_domain: "google.com",
        requires_reauth: false,
      })),
    };
    const getStatus = vi
      .spyOn(authApi, "authGetStatus")
      .mockImplementation(async () => ({ ...status }));
    const switchAccount = vi
      .spyOn(authApi, "authSetDefaultAccount")
      .mockImplementation(async (_provider, accountId) => {
        status.default_account_id = accountId;
      });
    const switched = vi.spyOn(toast, "success");
    const openWebsite = vi.fn();
    const updateSortOrder = vi
      .spyOn(providersApi, "updateSortOrder")
      .mockResolvedValue(true);
    const client = createTestQueryClient();
    const invalidateQueries = vi.spyOn(client, "invalidateQueries");

    render(
      <QueryClientProvider client={client}>
        <AgyDesktopPanel
          savedProviders={{
            first: {
              id: "first",
              name: "Desktop Work",
              category: "official",
              settingsConfig: {},
              sortIndex: 0,
              websiteUrl: "https://example.com",
            },
            second: {
              id: "second",
              name: "second@example.com",
              category: "official",
              settingsConfig: {},
              sortIndex: 1,
            },
          }}
          onDelete={vi.fn()}
          onOpenWebsite={openWebsite}
        />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("Desktop Work")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "https://example.com" }),
    );
    expect(openWebsite).toHaveBeenCalledWith("https://example.com");
    expect(
      screen.queryByRole("heading", { name: "providerPreset.group.login" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "provider.dragHandle" }),
    ).toHaveLength(2);
    expect(drag.onEnd).toBeDefined();
    await act(async () =>
      drag.onEnd?.({
        active: { id: "first" },
        over: { id: "second" },
      } as DragEndEvent),
    );
    expect(updateSortOrder).toHaveBeenCalledWith(
      [
        { id: "second", sortIndex: 0 },
        { id: "first", sortIndex: 1 },
      ],
      "agy-desktop",
    );
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["providers", "agy-desktop"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["providers", "agy"],
    });

    expect(screen.getByText("providerCard.status.inUse")).toBeInTheDocument();
    expect(getStatus).toHaveBeenCalledWith("agy_oauth", true);
    await userEvent.click(
      screen.getByRole("button", { name: "providerCard.action.switch" }),
    );
    expect(switchAccount).toHaveBeenCalledWith("agy_oauth", "second", true);
    expect(switched).toHaveBeenCalledWith(
      expect.stringMatching(/重启 Antigravity Desktop/),
    );
  });

  it("resolves bound accountId for CLI-created UUID cards when matching active, reauth, and switch", async () => {
    vi.spyOn(authApi, "authGetStatus").mockResolvedValue({
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: "acct-1",
      accounts: [
        {
          id: "acct-1",
          login: "one@example.com",
          provider: "agy_oauth",
          avatar_url: null,
          authenticated_at: 0,
          is_default: true,
          github_domain: "google.com",
          requires_reauth: false,
        },
        {
          id: "acct-2",
          login: "two@example.com",
          provider: "agy_oauth",
          avatar_url: null,
          authenticated_at: 0,
          is_default: false,
          github_domain: "google.com",
          requires_reauth: false,
        },
        {
          id: "acct-3",
          login: "three@example.com",
          provider: "agy_oauth",
          avatar_url: null,
          authenticated_at: 0,
          is_default: false,
          github_domain: "google.com",
          requires_reauth: true,
        },
      ],
    });
    const switchAccount = vi
      .spyOn(authApi, "authSetDefaultAccount")
      .mockResolvedValue(undefined);

    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AgyDesktopPanel
          savedProviders={{
            "cli-uuid-1": {
              id: "cli-uuid-1",
              name: "CLI Card One",
              category: "official",
              settingsConfig: {},
              sortIndex: 0,
              meta: {
                authBinding: {
                  source: "managed_account",
                  authProvider: "agy_oauth",
                  accountId: "acct-1",
                },
              },
            },
            "cli-uuid-2": {
              id: "cli-uuid-2",
              name: "CLI Card Two",
              category: "official",
              settingsConfig: {},
              sortIndex: 1,
              meta: {
                authBinding: {
                  source: "managed_account",
                  authProvider: "agy_oauth",
                  accountId: "acct-2",
                },
              },
            },
            "cli-uuid-3": {
              id: "cli-uuid-3",
              name: "CLI Card Expired",
              category: "official",
              settingsConfig: {},
              sortIndex: 2,
              meta: {
                authBinding: {
                  source: "managed_account",
                  authProvider: "agy_oauth",
                  accountId: "acct-3",
                },
              },
            },
          }}
          onDelete={vi.fn()}
          onOpenWebsite={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("CLI Card One")).toBeInTheDocument();
    expect(screen.getByText("providerCard.status.inUse")).toBeInTheDocument();

    const switchButtons = screen.getAllByRole("button", {
      name: "providerCard.action.switch",
    });
    expect(switchButtons).toHaveLength(2);
    expect(switchButtons[1]).toHaveAttribute("aria-disabled", "true");

    await userEvent.click(switchButtons[1]);
    expect(switchAccount).not.toHaveBeenCalled();

    await userEvent.click(switchButtons[0]);
    expect(switchAccount).toHaveBeenCalledWith("agy_oauth", "acct-2", true);
  });

  it("uses the agy provider form and saves its OAuth account binding", async () => {
    vi.spyOn(authApi, "authGetStatus").mockResolvedValue({
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: null,
      accounts: [
        {
          id: "new-account",
          provider: "agy_oauth",
          login: "new@example.com",
          avatar_url: null,
          authenticated_at: 0,
          is_default: false,
          github_domain: "google.com",
          requires_reauth: false,
        },
      ],
    });
    const addProvider = vi.spyOn(providersApi, "add").mockResolvedValue(true);
    const onOpenChange = vi.fn();
    function DesktopAddDialog() {
      const save = useAddProviderMutation("agy-desktop");
      return (
        <AddProviderDialog
          open
          appId="agy-desktop"
          onOpenChange={onOpenChange}
          onSubmit={async (provider) => {
            await save.mutateAsync(provider);
          }}
        />
      );
    }
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <DesktopAddDialog />
      </QueryClientProvider>,
    );

    const user = userEvent.setup();
    const preset = await screen.findByRole("button", {
      name: /Antigravity OAuth/,
    });
    expect(preset).toHaveTextContent("providerPreset.loginWith.google");
    expect(screen.getByText("provider.addNewProvider")).toBeInTheDocument();
    expect(screen.queryByText("providerPreset.custom")).not.toBeInTheDocument();
    expect(
      screen.queryByText("providerPreset.group.universal"),
    ).not.toBeInTheDocument();
    await user.click(preset);
    const name = screen.getByDisplayValue("Antigravity OAuth");
    await user.clear(name);
    await user.type(name, "Desktop Work");
    await user.click(screen.getByRole("combobox", { name: "账号" }));
    await user.click(
      await screen.findByRole("option", { name: "new@example.com" }),
    );
    await user.click(screen.getByRole("button", { name: "common.add" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(addProvider).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "new-account",
        name: "Desktop Work",
        category: "official",
        settingsConfig: {},
        meta: expect.objectContaining({
          authBinding: {
            source: "managed_account",
            authProvider: "agy_oauth",
            accountId: "new-account",
          },
        }),
      }),
      "agy-desktop",
      undefined,
      undefined,
    );
  });
});

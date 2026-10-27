import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderList } from "@/components/providers/ProviderList";
import { usageApi } from "@/lib/api";
import { subscriptionApi } from "@/lib/api/subscription";
import { usageKeys } from "@/lib/query/usage";
import type { Provider } from "@/types";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));

const native: Provider = {
  id: "openai-codex",
  name: "openai-codex",
  category: "official",
  icon: "openai",
  settingsConfig: {},
  meta: { nativeLogin: true },
};

const callbacks = () => ({
  onSwitch: vi.fn(),
  onEdit: vi.fn(),
  onDelete: vi.fn(),
  onRemoveFromConfig: vi.fn(),
  onDuplicate: vi.fn(),
  onConfigureUsage: vi.fn(),
  onOpenWebsite: vi.fn(),
  onOpenTerminal: vi.fn(),
});

function mockCurrentState(enabledProviderIds: string[] = []) {
  server.use(
    http.post("http://tauri.local/get_omp_current_state", () =>
      HttpResponse.json({ enabledProviderIds, defaultProviderId: null }),
    ),
  );
}

function card(name: string) {
  const element = screen
    .getByRole("heading", { level: 3, name })
    .closest(".group");
  if (!(element instanceof HTMLElement)) throw new Error(`No card for ${name}`);
  return element;
}

afterEach(() => vi.restoreAllMocks());

describe("OMP native login cards", () => {
  it("groups native OAuth as added without changing saved-provider membership or actions", async () => {
    const enabled: Provider = {
      id: "enabled",
      name: "Enabled API provider",
      settingsConfig: {},
    };
    const available: Provider = {
      id: "saved-not-enabled",
      name: "Saved API provider",
      settingsConfig: {},
    };
    mockCurrentState([enabled.id]);
    const actions = callbacks();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderList
          appId="omp"
          currentProviderId=""
          providers={{
            [native.id]: native,
            [enabled.id]: enabled,
            [available.id]: available,
          }}
          {...actions}
        />
      </QueryClientProvider>,
    );

    const nativeHeading = await screen.findByRole("heading", {
      name: native.name,
    });
    const nativeSection = nativeHeading.closest("section");
    expect(nativeSection).toBe(
      screen.getByRole("heading", { name: enabled.name }).closest("section"),
    );
    expect(nativeSection).not.toBe(
      screen.getByRole("heading", { name: available.name }).closest("section"),
    );
    const nativeCard = within(card(native.name));
    expect(nativeCard.getByText("omp.nativeLogin.badge")).toBeInTheDocument();
    expect(
      nativeCard.getByRole("button", {
        name: "omp.management.manageProviderNamed",
      }),
    ).toBeInTheDocument();
    for (const name of [
      "providerCard.action.edit",
      "providerCard.action.more",
      "provider.dragHandle",
    ]) {
      expect(
        nativeCard.queryByRole("button", { name }),
      ).not.toBeInTheDocument();
    }

    fireEvent.click(
      within(card(enabled.name)).getByRole("button", {
        name: "providerCard.action.remove",
      }),
    );
    expect(actions.onRemoveFromConfig).toHaveBeenCalledWith(enabled);
    fireEvent.click(
      within(card(available.name)).getByRole("button", {
        name: "providerCard.action.enable",
      }),
    );
    expect(actions.onSwitch).toHaveBeenCalledWith(available);
    fireEvent.click(
      within(card(available.name)).getByRole("button", {
        name: "providerCard.action.edit",
      }),
    );
    expect(actions.onEdit).toHaveBeenCalledWith(available);
    expect(
      within(card(available.name)).getByRole("button", {
        name: "provider.dragHandle",
      }),
    ).toBeInTheDocument();
  });

  it("keeps a saved Codex provider without the native marker mutable and available", async () => {
    const saved: Provider = { ...native, meta: undefined };
    mockCurrentState();
    const actions = callbacks();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderList
          appId="omp"
          currentProviderId=""
          providers={{ [saved.id]: saved }}
          {...actions}
        />
      </QueryClientProvider>,
    );

    await screen.findByRole("heading", { name: saved.name });
    const savedCard = within(card(saved.name));
    expect(
      savedCard.queryByText("omp.nativeLogin.badge"),
    ).not.toBeInTheDocument();
    expect(
      savedCard.queryByRole("button", {
        name: "providerCard.action.remove",
      }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      savedCard.getByRole("button", {
        name: "providerCard.action.enable",
      }),
    );
    expect(actions.onSwitch).toHaveBeenCalledWith(saved);
    fireEvent.click(
      savedCard.getByRole("button", {
        name: "providerCard.action.edit",
      }),
    );
    expect(actions.onEdit).toHaveBeenCalledWith(saved);
  });

  it.each(["script", "quota"] as const)(
    "suppresses %s queries, cached usage and controls on read-only cards",
    (kind) => {
      const usage = {
        success: true,
        data: [
          {
            planName: "Plan A",
            total: 100,
            used: 10,
            remaining: 90,
            unit: "%",
          },
          {
            planName: "Plan B",
            total: 100,
            used: 20,
            remaining: 80,
            unit: "%",
          },
        ],
      };
      const scriptQuery = vi.spyOn(usageApi, "query").mockResolvedValue(usage);
      const quotaQuery = vi
        .spyOn(subscriptionApi, "getCodexOauthQuota")
        .mockResolvedValue({
          tool: "codex_oauth",
          credentialStatus: "valid",
          credentialMessage: null,
          success: true,
          tiers: [],
          extraUsage: null,
          error: null,
          queriedAt: 1,
        });
      const provider: Provider = {
        ...native,
        category: "custom",
        meta: {
          nativeLogin: true,
          providerType: kind === "quota" ? "codex_oauth" : undefined,
          usage_script: {
            enabled: true,
            language: "javascript",
            code: "",
            templateType: "custom",
            autoQueryInterval: 1,
          },
        },
      };
      const client = createTestQueryClient();
      client.setQueryData(usageKeys.script(provider.id, "omp"), usage, {
        updatedAt: 1,
      });
      const actions = callbacks();
      render(
        <QueryClientProvider client={client}>
          <ProviderCard
            provider={provider}
            appId="omp"
            isCurrent={true}
            isInConfig={true}
            presentation={{ readOnly: true, chips: [], buttons: [] }}
            onTest={vi.fn()}
            dragHandleProps={{
              attributes: {
                role: "button",
                tabIndex: 0,
                "aria-disabled": false,
                "aria-pressed": undefined,
                "aria-roledescription": "sortable",
                "aria-describedby": "native-drag",
              },
              listeners: { onPointerDown: vi.fn() },
              isDragging: false,
            }}
            {...actions}
          />
        </QueryClientProvider>,
      );

      expect(scriptQuery).not.toHaveBeenCalled();
      expect(quotaQuery).not.toHaveBeenCalled();
      expect(screen.queryAllByRole("button")).toHaveLength(0);
      expect(screen.queryByText("Plan A")).not.toBeInTheDocument();
      expect(screen.queryByText("Plan B")).not.toBeInTheDocument();
      expect(screen.queryByText("usage.multiplePlans")).not.toBeInTheDocument();
    },
  );
});

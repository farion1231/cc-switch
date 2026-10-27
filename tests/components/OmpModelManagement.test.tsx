import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  within,
  waitFor,
} from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { ProviderList } from "@/components/providers/ProviderList";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";
import type { OmpModelManagementState } from "@/lib/api/omp";

const initial: OmpModelManagementState = {
  scope: "global",
  settingsPath: "config.yml",
  modelsPath: "models.yml",
  settingsRevision: "s1",
  modelsRevision: "m1",
  defaultModel: "cpa/old:high",
  defaultThinkingLevel: "high",
  modelRoles: { default: "cpa/old:high", slow: "@custom", custom: "cpa/old" },
  roleNames: ["default", "smol", "slow", "custom", "widget"],
  agentModelOverrides: {
    scout: ["@slow", "openai-codex/new:low"],
    untouched: "@slow",
  },
  modelOverrides: [
    {
      provider: "openai-codex",
      modelId: "new",
      parameters: { maxTokens: 2048 },
    },
  ],
  catalogStatus: "ready",
  catalogMessage: null,
  models: [
    {
      provider: "cpa",
      id: "old",
      kind: "chat",
      name: "CPA model",
      contextWindow: 32768,
      maxContextWindow: null,
      maxTokens: 4096,
      input: ["text"],
      reasoning: true,
      thinking: null,
      source: "configured",
      cacheUpdatedAt: null,
    },
    {
      provider: "openai-codex",
      id: "new",
      kind: "chat",
      name: "Native model",
      contextWindow: 128000,
      maxContextWindow: null,
      maxTokens: 8192,
      input: ["text", "image"],
      reasoning: true,
      thinking: {
        mode: "effort",
        efforts: ["low", "high"],
        defaultLevel: "high",
      },
      source: "nativeCache",
      cacheUpdatedAt: 1000,
    },
  ],
};

function mount(providers = {}, snapshot = initial) {
  server.use(
    http.post("http://tauri.local/get_omp_current_state", () =>
      HttpResponse.json({ enabledProviderIds: [], defaultProviderId: null }),
    ),
    http.post("http://tauri.local/get_omp_model_management_state", () =>
      HttpResponse.json(snapshot),
    ),
  );
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderList
        appId="omp"
        providers={providers}
        currentProviderId=""
        onSwitch={() => {}}
        onEdit={() => {}}
        onDelete={() => {}}
        onDuplicate={() => {}}
        onOpenWebsite={() => {}}
      />
    </QueryClientProvider>,
  );
}

async function open() {
  fireEvent.click(
    await screen.findByRole("button", { name: "omp.management.open" }),
  );
  await screen.findByLabelText("omp.management.defaultReference");
  fireEvent.click(
    screen.getByRole("button", { name: "omp.management.assignments" }),
  );
}

describe("OMP model management", () => {
  it("opens global management with no auth cards and stages a default before saving", async () => {
    const writes: unknown[] = [];
    server.use(
      http.post(
        "http://tauri.local/patch_omp_model_settings",
        async ({ request }) => {
          writes.push(await request.json());
          return HttpResponse.json({
            ...initial,
            defaultModel: "openai-codex/new:low",
            modelRoles: {
              ...initial.modelRoles,
              default: "openai-codex/new:low",
            },
            settingsRevision: "s2",
          });
        },
      ),
    );
    mount();
    await open();
    const save = screen.getByRole("button", {
      name: "omp.management.saveSettings",
    });
    expect(save).toBeDisabled();
    fireEvent.change(screen.getByLabelText("omp.management.defaultReference"), {
      target: { value: "openai-codex/new:low" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.setDefault" }),
    );
    expect(writes).toEqual([]);
    fireEvent.click(save);
    await waitFor(() =>
      expect(writes).toEqual([
        {
          patch: {
            expectedRevision: "s1",
            expectedPath: "config.yml",
            changes: [
              {
                section: "modelRoles",
                key: "default",
                expected: "cpa/old:high",
                value: "openai-codex/new:low",
              },
            ],
          },
        },
      ]),
    );
    await waitFor(() => expect(save).toBeDisabled());
  });
  it("preserves aliases and worker lists while patching only changed entries", async () => {
    const writes: unknown[] = [];
    server.use(
      http.post(
        "http://tauri.local/patch_omp_model_settings",
        async ({ request }) => {
          writes.push(await request.json());
          return HttpResponse.json({
            ...initial,
            agentModelOverrides: {
              ...initial.agentModelOverrides,
              scout: ["@custom", "cpa/old:high"],
            },
          });
        },
      ),
    );
    mount();
    await open();
    fireEvent.change(screen.getByLabelText("scout"), {
      target: { value: "@custom\ncpa/old:high" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.saveSettings" }),
    );
    await waitFor(() =>
      expect(writes).toEqual([
        {
          patch: {
            expectedRevision: "s1",
            expectedPath: "config.yml",
            changes: [
              {
                section: "agentModelOverrides",
                key: "scout",
                expected: ["@slow", "openai-codex/new:low"],
                value: ["@custom", "cpa/old:high"],
              },
            ],
          },
        },
      ]),
    );
    expect(screen.getByLabelText("untouched")).toHaveValue("@slow");
    expect(screen.getByLabelText("slow")).toHaveValue("@custom");
  });

  it("resets one parameter override without changing pending role edits", async () => {
    const writes: unknown[] = [];
    server.use(
      http.post(
        "http://tauri.local/patch_omp_model_override",
        async ({ request }) => {
          writes.push(await request.json());
          return HttpResponse.json({ ...initial, modelOverrides: [] });
        },
      ),
    );
    mount();
    await open();
    fireEvent.change(screen.getByLabelText("slow"), {
      target: { value: "@default" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Native model" }));
    const parameterForm = screen.getByRole("region", {
      name: "omp.management.parameters",
    });
    fireEvent.click(
      within(parameterForm).getByRole("button", {
        name: "omp.management.reset maxTokens",
      }),
    );
    fireEvent.click(
      within(parameterForm).getByRole("button", {
        name: "omp.management.saveParameters",
      }),
    );
    await waitFor(() =>
      expect(writes).toEqual([
        {
          patch: {
            expectedRevision: "m1",
            expectedPath: "models.yml",
            provider: "openai-codex",
            modelId: "new",
            changes: [{ field: "maxTokens", expected: 2048, value: null }],
          },
        },
      ]),
    );
    expect(screen.getByLabelText("slow")).toHaveValue("@default");
    expect(
      screen.getByRole("button", { name: "omp.management.saveSettings" }),
    ).toBeEnabled();
  });

  it("retains unsaved settings after a same-entry conflict and does not retry", async () => {
    let writes = 0;
    server.use(
      http.post("http://tauri.local/patch_omp_model_settings", () => {
        writes++;
        return HttpResponse.json("并发冲突: omp.errors.externalChange", {
          status: 500,
        });
      }),
    );
    mount();
    await open();
    fireEvent.change(screen.getByLabelText("slow"), {
      target: { value: "@default" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.saveSettings" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "omp.errors.externalChange",
    );
    expect(screen.getByLabelText("slow")).toHaveValue("@default");
    expect(writes).toBe(1);
    expect(
      screen.getByRole("button", { name: "omp.management.saveSettings" }),
    ).toBeDisabled();
  });

  it("shows indirect configured worker references on the native provider card without exposing generic actions", async () => {
    mount(
      {
        "openai-codex": {
          id: "openai-codex",
          name: "Native login",
          settingsConfig: {},
          meta: { nativeLogin: true },
        },
      },
      {
        ...initial,
        defaultModel: "openai-codex/new:high",
        modelRoles: {
          default: "openai-codex/new:high",
          slow: ["@custom:low"],
          custom: "openai-codex/new",
        },
        agentModelOverrides: { explorer: "@slow" },
      },
    );
    const heading = await screen.findByRole("heading", {
      name: "Native login",
    });
    const card = within(heading.closest(".group") as HTMLElement);
    await waitFor(() =>
      expect(
        card.getByText(/omp.management.workers: explorer/),
      ).toBeInTheDocument(),
    );
    expect(card.queryByText(/cpa\/old:high/)).not.toBeInTheDocument();
    expect(
      card.getByRole("button", { name: "omp.management.manageProvider" }),
    ).toBeEnabled();
    expect(
      card.queryByRole("button", { name: "providerCard.action.edit" }),
    ).not.toBeInTheDocument();
    expect(
      card.queryByRole("button", { name: "provider.dragHandle" }),
    ).not.toBeInTheDocument();
  });

  it("keeps role candidate arrays raw and patches a custom role list without touching default", async () => {
    const writes: unknown[] = [];
    const snapshot: OmpModelManagementState = {
      ...initial,
      modelRoles: {
        ...initial.modelRoles,
        default: ["cpa/old", "@slow"],
        custom: ["@slow"],
      },
    };
    server.use(
      http.post(
        "http://tauri.local/patch_omp_model_settings",
        async ({ request }) => {
          writes.push(await request.json());
          return HttpResponse.json(snapshot);
        },
      ),
    );
    mount({}, snapshot);
    await open();
    expect(screen.getByLabelText("default")).toHaveValue("cpa/old\n@slow");
    fireEvent.change(screen.getByLabelText("custom"), {
      target: { value: "@slow\nopenai-codex/new:inherit" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.saveSettings" }),
    );
    await waitFor(() =>
      expect(writes).toEqual([
        {
          patch: {
            expectedRevision: "s1",
            expectedPath: "config.yml",
            changes: [
              {
                section: "modelRoles",
                key: "custom",
                expected: ["@slow"],
                value: ["@slow", "openai-codex/new:inherit"],
              },
            ],
          },
        },
      ]),
    );
  });

  it("disables invalid parameter writes and confirms discarding dirty settings", async () => {
    mount();
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Native model" }));
    fireEvent.change(screen.getByLabelText("maxTokens"), {
      target: { value: "-1" },
    });
    expect(
      screen.getByRole("button", { name: "omp.management.saveParameters" }),
    ).toBeDisabled();
    fireEvent.change(screen.getByLabelText("slow"), {
      target: { value: "@default" },
    });
    const settings = screen.getByRole("region", {
      name: "omp.management.settings",
    });
    fireEvent.click(
      within(settings).getByRole("button", { name: "common.cancel" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "common.unsavedLeaveCancel" }),
    );
    expect(screen.getByLabelText("slow")).toHaveValue("@default");
  });

  it("offers one shared model and alias suggestion list to role and worker fields", async () => {
    mount();
    await open();
    const role = screen.getByLabelText("slow") as HTMLInputElement;
    const worker = screen.getByLabelText("untouched") as HTMLInputElement;
    expect(role.list).not.toBeNull();
    expect(worker.list).toBe(role.list);
    expect([...role.list!.options].map((option) => option.value)).toEqual(
      expect.arrayContaining([
        "cpa/old",
        "openai-codex/new",
        "@custom",
        "@slow",
      ]),
    );
    fireEvent.change(role, { target: { value: "openai-codex/new:high" } });
    expect(role).toHaveValue("openai-codex/new:high");
    expect(
      screen.getByRole("button", { name: "omp.management.saveSettings" }),
    ).toBeEnabled();
  });

  it("edits and resets explicit overrides when their model is absent from an unavailable catalog", async () => {
    const writes: unknown[] = [];
    server.use(
      http.post(
        "http://tauri.local/patch_omp_model_override",
        async ({ request }) => {
          writes.push(await request.json());
          return HttpResponse.json({
            ...initial,
            models: [],
            modelOverrides: [],
            catalogStatus: "unavailable",
          });
        },
      ),
    );
    mount({}, { ...initial, models: [], catalogStatus: "unavailable" });
    await open();
    fireEvent.click(screen.getByRole("button", { name: "openai-codex/new" }));
    expect(screen.getByLabelText("maxTokens")).toHaveValue(2048);
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.reset maxTokens" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.saveParameters" }),
    );
    await waitFor(() =>
      expect(writes).toEqual([
        {
          patch: {
            expectedRevision: "m1",
            expectedPath: "models.yml",
            provider: "openai-codex",
            modelId: "new",
            changes: [{ field: "maxTokens", expected: 2048, value: null }],
          },
        },
      ]),
    );
  });

  it("allows unrelated parameter changes while preserving an empty native input override", async () => {
    mount(
      {},
      {
        ...initial,
        modelOverrides: [
          {
            provider: "openai-codex",
            modelId: "new",
            parameters: { input: [] },
          },
        ],
      },
    );
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Native model" }));
    fireEvent.change(screen.getByLabelText("maxTokens"), {
      target: { value: "4096" },
    });
    expect(
      screen.getByRole("button", { name: "omp.management.saveParameters" }),
    ).toBeEnabled();
  });
});

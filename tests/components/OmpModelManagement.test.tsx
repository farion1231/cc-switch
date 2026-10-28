import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderList } from "@/components/providers/ProviderList";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";
import type { OmpModelManagementState } from "@/lib/api/omp";

beforeEach(() => {
  // cmdk/Radix scroll selected options; jsdom has no layout implementation.
  Element.prototype.scrollIntoView = vi.fn();
});

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
async function openManagement(tab: "models" | "assignments" = "assignments") {
  fireEvent.click(
    await screen.findByRole("button", { name: "omp.management.open" }),
  );
  await screen.findByRole("tab", { name: "omp.management.modelsTab" });
  if (tab === "assignments") showAssignments();
}
function showModels() {
  fireEvent.click(
    screen.getByRole("tab", { name: "omp.management.modelsTab" }),
  );
}
function showAssignments() {
  fireEvent.click(
    screen.getByRole("tab", { name: "omp.management.assignmentsTab" }),
  );
}
function chooseNative() {
  fireEvent.click(screen.getByRole("button", { name: /Native model/ }));
}
const settingsSave = () =>
  screen.getByRole("button", { name: "omp.management.saveSettings" });
const parameterSave = () =>
  screen.getByRole("button", { name: "omp.management.saveParameters" });

function settingsHandler(
  writes: unknown[],
  next: OmpModelManagementState = initial,
) {
  server.use(
    http.post(
      "http://tauri.local/patch_omp_model_settings",
      async ({ request }) => {
        writes.push(await request.json());
        return HttpResponse.json(next);
      },
    ),
  );
}
function parameterHandler(
  writes: unknown[],
  next: OmpModelManagementState = initial,
) {
  server.use(
    http.post(
      "http://tauri.local/patch_omp_model_override",
      async ({ request }) => {
        writes.push(await request.json());
        return HttpResponse.json(next);
      },
    ),
  );
}

describe("OMP native page navigation", () => {
  it("keeps empty-list management reachable with only the active file save action", async () => {
    mount();
    await openManagement("models");
    expect(parameterSave()).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "omp.management.saveSettings" }),
    ).not.toBeInTheDocument();
    showAssignments();
    expect(settingsSave()).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "omp.management.saveParameters" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByLabelText("omp.management.defaultReference"),
    ).toHaveValue("cpa/old:high");
    expect(
      screen.getByRole("combobox", { name: "omp.management.modelThinking" }),
    ).toBeInTheDocument();
  });

  it("shows current metadata without staging absent false or input overrides", async () => {
    const writes: unknown[] = [];
    parameterHandler(writes);
    mount(
      {},
      {
        ...initial,
        modelOverrides: [],
        models: initial.models.map((model) => ({ ...model, reasoning: false })),
      },
    );
    await openManagement("models");
    chooseNative();
    const window = screen.getByLabelText(
      "omp.management.parameterLabels.contextWindow",
    );
    expect(window).toHaveValue(128000);
    expect(parameterSave()).toBeDisabled();
    expect(writes).toEqual([]);
    fireEvent.change(window, { target: { value: "196608" } });
    fireEvent.click(parameterSave());
    await waitFor(() =>
      expect(writes).toEqual([
        {
          patch: {
            expectedRevision: "m1",
            expectedPath: "models.yml",
            provider: "openai-codex",
            modelId: "new",
            changes: [
              { field: "contextWindow", expected: null, value: 196608 },
            ],
          },
        },
      ]),
    );
  });

  it("retains model and assignment drafts across tabs and independent saves", async () => {
    const settingsWrites: unknown[] = [];
    const parameterWrites: unknown[] = [];
    const assigned = {
      ...initial,
      modelRoles: { ...initial.modelRoles, slow: "@default" },
    };
    settingsHandler(settingsWrites, assigned);
    parameterHandler(parameterWrites, assigned);
    mount();
    await openManagement("models");
    chooseNative();
    fireEvent.change(
      screen.getByLabelText("omp.management.parameterLabels.contextWindow"),
      { target: { value: "196608" } },
    );
    showAssignments();
    fireEvent.change(screen.getByLabelText("slow"), {
      target: { value: "@default" },
    });
    fireEvent.click(settingsSave());
    await waitFor(() =>
      expect(settingsWrites).toEqual([
        {
          patch: {
            expectedRevision: "s1",
            expectedPath: "config.yml",
            changes: [
              {
                section: "modelRoles",
                key: "slow",
                expected: "@custom",
                value: "@default",
              },
            ],
          },
        },
      ]),
    );
    expect(parameterWrites).toEqual([]);
    showModels();
    expect(
      screen.getByLabelText("omp.management.parameterLabels.contextWindow"),
    ).toHaveValue(196608);
    fireEvent.click(parameterSave());
    await waitFor(() =>
      expect(parameterWrites).toEqual([
        {
          patch: {
            expectedRevision: "m1",
            expectedPath: "models.yml",
            provider: "openai-codex",
            modelId: "new",
            changes: [
              { field: "contextWindow", expected: null, value: 196608 },
            ],
          },
        },
      ]),
    );
    showAssignments();
    expect(screen.getByLabelText("slow")).toHaveValue("@default");
  });
});

describe("OMP model management", () => {
  it("stages an exact default reference without writing until save", async () => {
    const user = userEvent.setup();
    const writes: unknown[] = [];
    settingsHandler(writes, {
      ...initial,
      defaultModel: "openai-codex/new:low",
      modelRoles: { ...initial.modelRoles, default: "openai-codex/new:low" },
      settingsRevision: "s2",
    });
    mount();
    await openManagement();
    expect(settingsSave()).toBeDisabled();
    const reference = screen.getByLabelText("omp.management.defaultReference");
    await user.clear(reference);
    await user.type(reference, "openai-codex/new:low");
    expect(reference).toHaveValue("openai-codex/new:low");
    showModels();
    showAssignments();
    expect(reference).toHaveValue("openai-codex/new:low");
    expect(writes).toEqual([]);
    fireEvent.click(settingsSave());
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
    await waitFor(() => expect(settingsSave()).toBeDisabled());
  });

  it("preserves unknown raw selector characters through typing and view changes", async () => {
    const user = userEvent.setup();
    const writes: unknown[] = [];
    settingsHandler(writes);
    mount();
    await openManagement();
    const reference = screen.getByLabelText("omp.management.defaultReference");
    const raw = "future-provider/custom.model:experimental";
    await user.clear(reference);
    await user.type(reference, raw);
    expect(reference).toHaveValue(raw);
    expect(
      screen.getByRole("combobox", { name: "omp.management.modelThinking" }),
    ).toBeDisabled();
    showModels();
    showAssignments();
    expect(reference).toHaveValue(raw);
    expect(writes).toEqual([]);
    fireEvent.click(settingsSave());
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
                value: raw,
              },
            ],
          },
        },
      ]),
    );
  });

  it("applies an explicitly chosen supported thinking effort without saving automatically", async () => {
    const user = userEvent.setup();
    const writes: unknown[] = [];
    settingsHandler(writes);
    mount();
    await openManagement();
    const reference = screen.getByLabelText("omp.management.defaultReference");
    await user.clear(reference);
    await user.type(reference, "openai-codex/new:high");
    await user.click(
      screen.getByRole("combobox", { name: "omp.management.modelThinking" }),
    );
    await user.click(await screen.findByRole("option", { name: /^low$/ }));
    expect(reference).toHaveValue("openai-codex/new:low");
    expect(writes).toEqual([]);
    fireEvent.click(settingsSave());
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
  });

  it("preserves aliases and worker arrays while patching only the changed worker", async () => {
    const writes: unknown[] = [];
    settingsHandler(writes, {
      ...initial,
      agentModelOverrides: {
        ...initial.agentModelOverrides,
        scout: ["@custom", "cpa/old:high"],
      },
    });
    mount();
    await openManagement();
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.workers" }),
    );
    fireEvent.change(screen.getByLabelText("scout"), {
      target: { value: "@custom\ncpa/old:high" },
    });
    fireEvent.click(settingsSave());
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
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.roles" }),
    );
    expect(screen.getByLabelText("slow")).toHaveValue("@custom");
  });

  it("keeps role candidate arrays raw and does not touch the default when editing another role", async () => {
    const writes: unknown[] = [];
    const snapshot = {
      ...initial,
      modelRoles: {
        ...initial.modelRoles,
        default: ["cpa/old", "@slow"],
        custom: ["@slow"],
      },
    };
    settingsHandler(writes, snapshot);
    mount({}, snapshot);
    await openManagement();
    expect(
      screen.getByLabelText("omp.management.defaultReference"),
    ).toHaveValue("cpa/old\n@slow");
    fireEvent.change(screen.getByLabelText("custom"), {
      target: { value: "@slow\nopenai-codex/new:inherit" },
    });
    fireEvent.click(settingsSave());
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

  it("resets a parameter override without discarding a pending role in the other tab", async () => {
    const writes: unknown[] = [];
    parameterHandler(writes, { ...initial, modelOverrides: [] });
    mount();
    await openManagement();
    fireEvent.change(screen.getByLabelText("slow"), {
      target: { value: "@default" },
    });
    showModels();
    chooseNative();
    fireEvent.click(
      screen.getByRole("button", {
        name: "omp.management.reset omp.management.parameterLabels.maxTokens",
      }),
    );
    fireEvent.click(parameterSave());
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
    showAssignments();
    expect(screen.getByLabelText("slow")).toHaveValue("@default");
    expect(settingsSave()).toBeEnabled();
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
    await openManagement();
    fireEvent.change(screen.getByLabelText("slow"), {
      target: { value: "@default" },
    });
    fireEvent.click(settingsSave());
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "omp.errors.externalChange",
    );
    expect(screen.getByLabelText("slow")).toHaveValue("@default");
    expect(writes).toBe(1);
    expect(settingsSave()).toBeDisabled();
  });

  it("disables invalid parameter saves and retains a draft when discard is cancelled", async () => {
    mount();
    await openManagement("models");
    chooseNative();
    fireEvent.change(
      screen.getByLabelText("omp.management.parameterLabels.maxTokens"),
      { target: { value: "-1" } },
    );
    expect(parameterSave()).toBeDisabled();
    showAssignments();
    fireEvent.change(screen.getByLabelText("slow"), {
      target: { value: "@default" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "common.unsavedLeaveCancel" }),
    );
    expect(screen.getByLabelText("slow")).toHaveValue("@default");
    showModels();
    expect(
      screen.getByLabelText("omp.management.parameterLabels.maxTokens"),
    ).toHaveValue(-1);
  });

  it("offers native model and alias suggestions without restricting raw references", async () => {
    mount();
    await openManagement();
    fireEvent.click(
      screen.getByRole("button", { name: "omp.management.modelChoice slow" }),
    );
    const picker = await screen.findByRole("dialog");
    expect(
      within(picker).getByRole("option", { name: "@custom" }),
    ).toBeInTheDocument();
    fireEvent.click(
      within(picker).getByRole("option", { name: /Native model/ }),
    );
    expect(screen.getByLabelText("slow")).toHaveValue("openai-codex/new");
    fireEvent.change(screen.getByLabelText("slow"), {
      target: { value: "openai-codex/new:xh" },
    });
    expect(screen.getByLabelText("slow")).toHaveValue("openai-codex/new:xh");
    expect(settingsSave()).toBeEnabled();
  });

  it("edits and resets explicit overrides absent from an unavailable catalog", async () => {
    const writes: unknown[] = [];
    parameterHandler(writes, {
      ...initial,
      models: [],
      modelOverrides: [],
      catalogStatus: "unavailable",
    });
    mount({}, { ...initial, models: [], catalogStatus: "unavailable" });
    await openManagement("models");
    fireEvent.click(screen.getByRole("button", { name: "openai-codex/new" }));
    expect(
      screen.getByLabelText("omp.management.parameterLabels.maxTokens"),
    ).toHaveValue(2048);
    fireEvent.click(
      screen.getByRole("button", {
        name: "omp.management.reset omp.management.parameterLabels.maxTokens",
      }),
    );
    fireEvent.click(parameterSave());
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

  it("allows unrelated parameter edits while preserving an explicit empty input array", async () => {
    const writes: unknown[] = [];
    parameterHandler(writes);
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
    await openManagement("models");
    chooseNative();
    fireEvent.change(
      screen.getByLabelText("omp.management.parameterLabels.maxTokens"),
      { target: { value: "4096" } },
    );
    fireEvent.click(parameterSave());
    await waitFor(() =>
      expect(writes).toEqual([
        {
          patch: {
            expectedRevision: "m1",
            expectedPath: "models.yml",
            provider: "openai-codex",
            modelId: "new",
            changes: [{ field: "maxTokens", expected: null, value: 4096 }],
          },
        },
      ]),
    );
  });

  it("exposes native reference details and model management without generic credential actions", async () => {
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
    await screen.findByRole("heading", { name: "Native login" });
    const details = await screen.findByRole("button", {
      name: "omp.management.referenceDetails Native login",
    });
    fireEvent.click(details);
    expect(await screen.findByText(/explorer/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "omp.management.manageProviderNamed",
      }),
    ).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "providerCard.action.edit" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "provider.dragHandle" }),
    ).not.toBeInTheDocument();
  });
});

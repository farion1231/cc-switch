import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormProps,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import type { CodexCopilotApiFormat, ProviderMeta } from "@/types";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: ({
    onAccountSelect,
  }: {
    onAccountSelect?: (accountId: string | null) => void;
  }) => (
    <button type="button" onClick={() => onAccountSelect?.("copilot-account")}>
      select-copilot-account
    </button>
  ),
}));
vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      data-testid="json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));
vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  return {
    ...actual,
    useCopilotAuth: () => ({
      isAuthenticated: true,
      isStatusSuccess: true,
      isStatusError: false,
      defaultAccountId: "copilot-account",
      accounts: [
        {
          id: "copilot-account",
          login: "copilot-user",
          is_default: true,
        },
      ],
    }),
    useCodexOauth: () => ({ isAuthenticated: false, accounts: [] }),
    useXaiOauth: () => ({ isAuthenticated: false, accounts: [] }),
  };
});

function getCopilotPreset() {
  const preset = codexProviderPresets.find(
    (item) => item.providerType === "github_copilot",
  );
  if (!preset) throw new Error("Missing Codex Copilot preset");
  return preset;
}

interface DraftProjectionRequest {
  app: string;
  settingsConfig: { auth: Record<string, unknown>; config: string };
  category?: string;
  providerId?: string;
  meta?: ProviderMeta;
}

function captureDraftProjections(liveToml = "") {
  const requests: DraftProjectionRequest[] = [];
  server.use(
    http.post(
      "http://tauri.local/get_provider_editor_view",
      async ({ request }) => {
        const body = (await request.json()) as DraftProjectionRequest;
        requests.push(body);
        return HttpResponse.json({
          settings: {
            ...body.settingsConfig,
            config: `${body.settingsConfig.config}${liveToml}`,
          },
          inactive: [],
        });
      },
    ),
  );
  return requests;
}

function renderForm(
  meta?: ProviderMeta,
  onEditorBaseChange?: ProviderFormProps["onEditorBaseChange"],
) {
  const onSubmit = vi.fn<(values: ProviderFormValues) => void>();
  const preset = getCopilotPreset();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="codex"
        submitLabel="save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        onEditorBaseChange={onEditorBaseChange}
        initialData={
          meta
            ? {
                name: "GitHub Copilot",
                category: "third_party",
                settingsConfig: { auth: {}, config: preset.config },
                meta,
              }
            : undefined
        }
      />
    </QueryClientProvider>,
  );
  if (!meta) {
    fireEvent.click(screen.getByRole("button", { name: /GitHub Copilot/ }));
  }
  return onSubmit;
}

function formatControl() {
  return screen.getByRole("combobox", { name: "上游格式" });
}

const formatLabels = {
  auto: "codexConfig.upstreamFormatAuto",
  openai_chat: "Chat Completions（需开启路由）",
  openai_responses: "codexConfig.upstreamFormatCopilotResponses",
};

async function selectFormat(format: CodexCopilotApiFormat) {
  fireEvent.keyDown(formatControl(), { key: "ArrowDown" });
  fireEvent.click(
    await screen.findByRole("option", { name: formatLabels[format] }),
  );
}

describe("Codex Copilot provider form", () => {
  const scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "scrollIntoView",
  );
  beforeAll(() => {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
  });
  afterAll(() => {
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

  it("defaults to GPT-6 Astra and shows all five preset model mappings", () => {
    renderForm();
    expect(screen.getAllByDisplayValue("gpt-6-astra")).toHaveLength(2);
    for (const model of [
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
    ]) {
      expect(screen.getByDisplayValue(model)).toBeVisible();
    }
    expect(screen.getAllByDisplayValue("1048576")).toHaveLength(5);
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(screen.getByText("模型映射")).toBeVisible();
  });

  it.each<CodexCopilotApiFormat>(["auto", "openai_chat", "openai_responses"])(
    "persists %s without changing the Codex client wire protocol",
    async (format) => {
      const onSubmit = renderForm();
      await selectFormat(format);
      expect(screen.getByText("模型映射")).toBeVisible();
      fireEvent.click(screen.getByRole("button", { name: "save" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
      const saved = onSubmit.mock.calls[0][0];
      expect(saved.meta?.codexCopilotApiFormat).toBe(
        format === "auto" ? undefined : format,
      );
      expect(saved.meta?.apiFormat).toBe(
        format === "auto" ? "openai_chat" : format,
      );
      expect(JSON.parse(saved.settingsConfig).config).toContain(
        'wire_api = "responses"',
      );
      expect(JSON.parse(saved.settingsConfig).modelCatalog.models).toEqual(
        getCopilotPreset().modelCatalog,
      );
    },
  );

  it("preserves account, protocol and model edits when saving a live-projected Copilot draft", async () => {
    const liveToml = '\n[ui]\ntheme = "dark"\n';
    const requests = captureDraftProjections(liveToml);
    const onEditorBaseChange = vi.fn();
    const onSubmit = renderForm(undefined, onEditorBaseChange);
    const preset = getCopilotPreset();
    const draft = { auth: preset.auth, config: preset.config };
    const base = { ...draft, config: `${preset.config}${liveToml}` };
    await waitFor(() =>
      expect(onEditorBaseChange).toHaveBeenLastCalledWith(base, draft),
    );
    expect(requests.at(-1)).toEqual({
      app: "codex",
      settingsConfig: draft,
      category: preset.category,
      meta: { providerType: "github_copilot", apiFormat: "openai_chat" },
    });
    await waitFor(() =>
      expect(
        screen
          .getAllByTestId("json-editor")
          .map((node) => (node as HTMLTextAreaElement).value),
      ).toContain(base.config),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "select-copilot-account" }),
    );
    await selectFormat("openai_responses");
    fireEvent.change(screen.getByLabelText("默认模型"), {
      target: { value: "gpt-5.6-luna" },
    });
    fireEvent.change(screen.getByDisplayValue("gpt-5.6-sol"), {
      target: { value: "custom-copilot-model" },
    });
    fireEvent.change(screen.getAllByDisplayValue("1048576")[0], {
      target: { value: "524288" },
    });
    fireEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const saved = onSubmit.mock.calls[0][0];
    expect(saved.meta).toEqual(
      expect.objectContaining({
        providerType: "github_copilot",
        authBinding: {
          source: "managed_account",
          authProvider: "github_copilot",
          accountId: "copilot-account",
        },
        githubAccountId: "copilot-account",
        apiFormat: "openai_responses",
        codexCopilotApiFormat: "openai_responses",
      }),
    );
    const settings = JSON.parse(saved.settingsConfig);
    expect(settings.auth).toEqual({});
    expect(settings.config).toContain(liveToml);
    expect(settings.config).toContain('model = "gpt-5.6-luna"');
    expect(settings.config).toContain('wire_api = "responses"');
    expect(settings.modelCatalog.models).toEqual(
      preset.modelCatalog?.map((model) =>
        model.model === "gpt-6-astra"
          ? { ...model, contextWindow: 524288 }
          : model.model === "gpt-5.6-sol"
            ? { ...model, model: "custom-copilot-model" }
            : model,
      ),
    );
    expect(onEditorBaseChange).toHaveBeenLastCalledWith(base, draft);
    expect(screen.queryByText("仍要保存")).not.toBeInTheDocument();
  });

  it("projects each preset with its own identity instead of stale Copilot metadata", async () => {
    const requests = captureDraftProjections();
    const onEditorBaseChange = vi.fn();
    renderForm(undefined, onEditorBaseChange);
    const copilot = getCopilotPreset();
    const otherPreset = codexProviderPresets.find(
      (preset) => preset.name === "DeepSeek",
    );
    if (!otherPreset) throw new Error("Missing DeepSeek preset");

    const waitForPreset = async (preset: typeof copilot) => {
      const draft = { auth: preset.auth, config: preset.config };
      await waitFor(() =>
        expect(onEditorBaseChange).toHaveBeenLastCalledWith(draft, draft),
      );
    };
    await waitForPreset(copilot);
    expect(requests.at(-1)?.meta).toEqual({
      providerType: "github_copilot",
      apiFormat: "openai_chat",
    });
    await selectFormat("openai_responses");

    fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
    await waitForPreset(otherPreset);
    expect(requests.at(-1)).not.toHaveProperty("meta");
    expect(requests.at(-1)).not.toHaveProperty("providerId");

    fireEvent.click(screen.getByRole("button", { name: /GitHub Copilot/ }));
    await waitForPreset(copilot);
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(requests.at(-1)?.meta).toEqual({
      providerType: "github_copilot",
      apiFormat: "openai_chat",
    });

    const previousRequests = requests.length;
    fireEvent.click(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    );
    await waitFor(() => {
      expect(requests.length).toBeGreaterThan(previousRequests);
      expect(onEditorBaseChange.mock.lastCall?.[0]).not.toBeNull();
    });
    expect(requests.at(-1)).not.toHaveProperty("meta");
    expect(requests.at(-1)).not.toHaveProperty("providerId");
  });

  it("keeps legacy cards automatic and shows mapping even with an empty catalog", () => {
    renderForm({ providerType: "github_copilot", apiFormat: "openai_chat" });
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(screen.getByText("模型映射")).toBeVisible();
  });

  it("does not offer preset switching while editing an existing Copilot card", () => {
    renderForm({ providerType: "github_copilot", apiFormat: "openai_chat" });
    expect(
      screen.queryByRole("button", { name: /DeepSeek/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /providerPreset.custom/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("providerPreset.label")).not.toBeInTheDocument();
  });

  it("loads a saved explicit protocol and can return it to automatic", async () => {
    const onSubmit = renderForm({
      providerType: "github_copilot",
      apiFormat: "openai_responses",
      codexCopilotApiFormat: "openai_responses",
    });
    expect(formatControl()).toHaveTextContent(formatLabels.openai_responses);
    await selectFormat("auto");
    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(
      onSubmit.mock.calls[0][0].meta?.codexCopilotApiFormat,
    ).toBeUndefined();
    expect(onSubmit.mock.calls[0][0].meta?.apiFormat).toBe("openai_chat");
  });

  it("excludes Messages only for Copilot and resets the override on preset changes", async () => {
    renderForm();
    fireEvent.keyDown(formatControl(), { key: "ArrowDown" });
    expect(await screen.findAllByRole("option")).toHaveLength(3);
    expect(
      screen.queryByRole("option", { name: /Anthropic Messages/ }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("option", { name: formatLabels.openai_chat }),
    );
    fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
    expect(formatControl()).toHaveTextContent("Responses（原生）");
    expect(screen.getByText("模型映射")).toBeVisible();
    fireEvent.keyDown(formatControl(), { key: "ArrowDown" });
    expect(
      await screen.findByRole("option", { name: /Anthropic Messages/ }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Responses（原生）" }));
    fireEvent.click(screen.getByRole("button", { name: /GitHub Copilot/ }));
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(screen.getByText("模型映射")).toBeVisible();
  });
});

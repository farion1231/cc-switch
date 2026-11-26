import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { CopilotProviderForm } from "@/components/providers/forms/CopilotProviderForm";
import type { ProviderCategory, ProviderMeta } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const fetchModels = vi.hoisted(() => vi.fn());

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
});

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: fetchModels,
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: ({
    onAccountSelect,
    noneOptionLabel,
  }: {
    onAccountSelect?: (id: string | null) => void;
    noneOptionLabel?: string;
  }) => (
    <>
      <span data-testid="copilot-none-option">{noneOptionLabel}</span>
      <button type="button" onClick={() => onAccountSelect?.("12345")}>
        bind-account
      </button>
    </>
  ),
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
      data-testid={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const byok = {
  provider: {
    type: "openai",
    wireApi: "completions",
    baseUrl: "http://localhost:18317/v1",
    apiKey: "fixture-key",
  },
  models: [{ id: "deepseek-v4-flash", maxContextWindowTokens: 128000 }],
};

function setup(
  settingsConfig: Record<string, unknown>,
  category: ProviderCategory = "custom",
  meta?: ProviderMeta,
) {
  const onSubmit = vi.fn();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <CopilotProviderForm
        appId="copilot"
        providerId="relay"
        initialData={{ name: "Relay", settingsConfig, category, meta }}
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />
    </QueryClientProvider>,
  );
  return onSubmit;
}

const save = async (submit: ReturnType<typeof vi.fn>) => {
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(submit).toHaveBeenCalledOnce());
  return submit.mock.calls[0][0];
};

describe("CopilotProviderForm", () => {
  it("saves BYOK cards as a providers.json entry keyed by the card id", async () => {
    const submit = setup(byok);
    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      { target: { value: "https://relay.example.com/v1" } },
    );
    const values = await save(submit);
    expect(values.providerKey).toBe("relay");
    expect(JSON.parse(values.settingsConfig)).toEqual({
      ...byok,
      provider: { ...byok.provider, baseUrl: "https://relay.example.com/v1" },
    });
  });

  it("keeps the card id read-only when editing", () => {
    setup(byok);
    const input = document.getElementById(
      "copilot-provider-id",
    ) as HTMLInputElement;
    expect(input.value).toBe("relay");
    expect(input).toBeDisabled();
  });

  it("opens legacy environment cards in the new shape", async () => {
    const submit = setup({
      env: {
        COPILOT_PROVIDER_BASE_URL: "http://localhost:18317/v1",
        COPILOT_PROVIDER_TYPE: "openai",
        COPILOT_PROVIDER_WIRE_API: "completions",
        COPILOT_PROVIDER_API_KEY: "fixture-key",
        COPILOT_MODEL: "deepseek-v4-flash",
      },
    });
    const values = await save(submit);
    expect(JSON.parse(values.settingsConfig)).toEqual({
      provider: byok.provider,
      models: [{ id: "deepseek-v4-flash" }],
    });
  });

  it("shows protocol and wire API as a single API format", () => {
    setup(byok);
    expect(
      screen.getByRole("combobox", { name: "copilot.form.apiFormat" }),
    ).toHaveTextContent("OpenAI Chat Completions");
  });

  it("keeps the card reasoning effort out of the provider entry", async () => {
    const submit = setup({ ...byok, reasoningEffort: "none" });
    expect(
      screen.getByRole("combobox", { name: "copilot.form.reasoningEffort" }),
    ).toHaveTextContent("none");
    const values = await save(submit);
    expect(JSON.parse(values.settingsConfig)).toEqual({
      ...byok,
      reasoningEffort: "none",
    });
    // 编辑框显示的 providers.json 片段里没有推理强度
    const text = (
      screen.getByTestId("copilot-settings-config") as HTMLTextAreaElement
    ).value;
    expect(text).not.toContain("reasoningEffort");
    expect(JSON.parse(text)).toEqual({
      providers: [{ name: "relay", ...byok.provider }],
      models: [{ ...byok.models[0], provider: "relay" }],
    });
  });

  it("keeps new header rows as drafts instead of saving them upstream", async () => {
    const submit = setup(byok);
    const add = screen.getByRole("button", { name: "Add header" });
    fireEvent.click(add);
    fireEvent.click(add);
    expect(screen.getAllByRole("textbox", { name: "Header" })).toHaveLength(2);
    const values = await save(submit);
    expect(JSON.parse(values.settingsConfig).provider.headers).toBeUndefined();
  });

  it("saves edited header rows as a headers object", async () => {
    const submit = setup(byok);
    fireEvent.click(screen.getByRole("button", { name: "Add header" }));
    const nameInput = screen.getByRole("textbox", { name: "Header" });
    fireEvent.change(nameInput, { target: { value: "X-Tenant" } });
    fireEvent.blur(nameInput);
    fireEvent.change(screen.getByRole("textbox", { name: "Value" }), {
      target: { value: "team-a" },
    });
    const values = await save(submit);
    expect(JSON.parse(values.settingsConfig).provider.headers).toEqual({
      "X-Tenant": "team-a",
    });
  });

  it("fetches models from the BYOK endpoint with the card's key", async () => {
    fetchModels.mockResolvedValueOnce([
      { id: "deepseek-v4-pro", ownedBy: null },
    ]);
    setup(byok);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() => expect(fetchModels).toHaveBeenCalledOnce());
    expect(fetchModels.mock.calls[0].slice(0, 3)).toEqual([
      "http://localhost:18317/v1",
      "fixture-key",
      false,
    ]);
    expect(fetchModels.mock.calls[0][5]?.apiFormat).toBeUndefined();
  });

  it("adds and removes model rows", async () => {
    const submit = setup(byok);
    fireEvent.click(screen.getByRole("button", { name: "opencode.addModel" }));
    const ids = screen.getAllByRole("textbox", { name: "opencode.modelId" });
    expect(ids).toHaveLength(2);
    // 空的模型 ID 不能保存
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(ids[1], { target: { value: "deepseek-v4-pro" } });
    fireEvent.change(
      screen.getAllByRole("textbox", { name: "opencode.modelName" })[1],
      { target: { value: "DeepSeek V4 Pro" } },
    );
    const values = await save(submit);
    expect(JSON.parse(values.settingsConfig).models).toEqual([
      byok.models[0],
      { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" },
    ]);
  });

  it("accepts a token limit of 0 and rejects limits above u32", async () => {
    const submit = setup({
      ...byok,
      models: [{ id: "m", maxOutputTokens: 4294967296 }],
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.click(
      screen.getByRole("button", { name: "opencode.toggleModelDetails" }),
    );
    fireEvent.change(
      screen.getByRole("spinbutton", { name: "copilot.form.maxOutputTokens" }),
      { target: { value: "0" } },
    );
    const values = await save(submit);
    expect(JSON.parse(values.settingsConfig).models).toEqual([
      { id: "m", maxOutputTokens: 0 },
    ]);
  });

  it("blocks saving without models", () => {
    setup({ ...byok, models: [] });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("blocks duplicate model ids", () => {
    setup({ ...byok, models: [{ id: "a" }, { id: "a" }] });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "copilot.form.duplicateModelId",
    );
  });

  it("applies edits from the providers.json editor and rejects other providers", async () => {
    const submit = setup(byok);
    const editor = screen.getByTestId("copilot-settings-config");
    fireEvent.change(editor, {
      target: {
        value: JSON.stringify({
          providers: [{ name: "other", baseUrl: "http://x/v1" }],
          models: [],
        }),
      },
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "providerForm.nativeConfig.scope",
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(editor, {
      target: {
        value: JSON.stringify({
          providers: [
            { name: "relay", type: "anthropic", baseUrl: "https://a.example" },
          ],
          models: [{ id: "claude-x", provider: "relay", wireModel: "c-x" }],
        }),
      },
    });
    const values = await save(submit);
    expect(JSON.parse(values.settingsConfig)).toEqual({
      provider: { type: "anthropic", baseUrl: "https://a.example" },
      models: [{ id: "claude-x", wireModel: "c-x" }],
    });
  });

  it("rejects wrongly typed fields in the providers.json editor without crashing", () => {
    setup(byok);
    const editor = screen.getByTestId("copilot-settings-config");
    for (const value of [
      {
        providers: [
          { name: "relay", baseUrl: "https://a.example", apiKey: 123 },
        ],
        models: [{ id: "m" }],
      },
      {
        providers: [
          { name: "relay", baseUrl: "https://a.example", headers: { A: 1 } },
        ],
        models: [{ id: "m" }],
      },
      {
        providers: [{ name: "relay", baseUrl: "https://a.example" }],
        models: [{ id: "m", maxOutputTokens: "8192" }],
      },
    ]) {
      fireEvent.change(editor, { target: { value: JSON.stringify(value) } });
      expect(screen.getByRole("alert")).toHaveTextContent(
        "providerForm.nativeConfig.schema",
      );
    }
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("official cards keep only the model and bind an Auth Center account", async () => {
    const submit = setup(
      { env: { COPILOT_MODEL: "claude-sonnet-4.6" } },
      "official",
    );
    expect(screen.queryByLabelText(/copilot\.form\.baseUrl/)).toBeNull();
    expect(document.getElementById("copilot-provider-id")).toBeNull();
    // 不绑账号时用的是 Copilot CLI 自己的登录，不是授权中心的默认账号
    expect(screen.getByTestId("copilot-none-option")).toHaveTextContent(
      "copilot.form.followCliLogin",
    );
    fireEvent.click(screen.getByRole("button", { name: "bind-account" }));
    const values = await save(submit);
    expect(values.providerKey).toBeUndefined();
    expect(JSON.parse(values.settingsConfig)).toEqual({
      env: { COPILOT_MODEL: "claude-sonnet-4.6" },
    });
    expect(values.meta.authBinding).toEqual({
      source: "managed_account",
      authProvider: "github_copilot",
      accountId: "12345",
    });
  });
});

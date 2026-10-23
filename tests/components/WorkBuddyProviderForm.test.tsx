import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { WorkBuddyProviderForm } from "@/components/providers/forms/WorkBuddyProviderForm";
import { setProviders } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

const modelFetchApiMock = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

const existing = {
  vendor: "DeepSeek",
  url: "https://api.deepseek.com/v1/chat/completions",
  apiKey: "local-test-key",
  models: [
    { id: "deepseek-chat", name: "DeepSeek Chat", supportsToolCall: true },
  ],
  futureSetting: { keep: true },
};

describe("WorkBuddyProviderForm", () => {
  it("edits fields while keeping the key and unknown settings", async () => {
    const submit = vi.fn();
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{ name: "DeepSeek", settingsConfig: existing }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByLabelText(/opencode.providerKey/)).toBeDisabled();
    fireEvent.change(screen.getByLabelText("opencode.modelName"), {
      target: { value: "Chat" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const call = submit.mock.calls[0][0];
    expect(call.providerKey).toBe("deepseek");
    expect(JSON.parse(call.settingsConfig)).toEqual({
      ...existing,
      models: [{ ...existing.models[0], name: "Chat" }],
    });
  });

  it("blocks saving until a model and endpoint are filled in", async () => {
    const submit = vi.fn();
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        initialData={undefined}
        submitLabel="Add"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    const save = screen.getByRole("button", { name: "Add" });
    expect(save).toBeDisabled();

    fireEvent.change(screen.getByLabelText(/opencode.providerKey/), {
      target: { value: "My Key" },
    });
    expect(screen.getByLabelText(/opencode.providerKey/)).toHaveValue("mykey");
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Mine" },
    });
    fireEvent.change(screen.getByLabelText(/workbuddy.form.url/), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    expect(save).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: /opencode.addModel/ }));
    fireEvent.change(screen.getByLabelText("opencode.modelId"), {
      target: { value: " my-model " },
    });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const call = submit.mock.calls[0][0];
    expect(call.providerKey).toBe("mykey");
    expect(JSON.parse(call.settingsConfig)).toMatchObject({
      url: "https://api.example.com/v1/chat/completions",
      models: [{ id: "my-model", supportsToolCall: true }],
    });
  });

  it("uses the shared API key, endpoint and format fields", () => {
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{
          name: "DeepSeek",
          settingsConfig: {
            ...existing,
            url: "https://api.example.com/v1",
            api: "openai-responses",
          },
        }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    expect(document.querySelector("#workbuddy-api-format")).toHaveAttribute(
      "role",
      "combobox",
    );
    expect(document.querySelector("#workbuddy-api-format")).toHaveTextContent(
      "OpenAI Responses",
    );
    expect(screen.getByLabelText("API Key")).toHaveValue("local-test-key");
    expect(screen.getByText("workbuddy.form.apiKeyHint")).toBeInTheDocument();
    expect(screen.getByLabelText("workbuddy.form.url")).toHaveAttribute(
      "aria-required",
      "true",
    );
    expect(
      screen.getByText("workbuddy.form.urlResponsesHint"),
    ).toBeInTheDocument();
  });

  it("lays models out as rows with details behind the expand arrow", () => {
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{
          name: "DeepSeek",
          settingsConfig: {
            ...existing,
            models: [
              { id: "first", maxInputTokens: 111 },
              { id: "second", maxInputTokens: 222 },
              { id: "third", maxInputTokens: 333 },
            ],
          },
        }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    expect(screen.getAllByLabelText("opencode.modelId")).toHaveLength(3);
    expect(
      screen.queryByLabelText("workbuddy.form.maxInputTokens"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "opencode.models" }),
    ).toBeInTheDocument();

    const toggles = screen.getAllByRole("button", {
      name: "opencode.toggleModelDetails",
    });
    fireEvent.click(toggles[2]);
    expect(toggles[2]).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByLabelText("workbuddy.form.maxInputTokens")).toHaveValue(
      333,
    );
    expect(
      screen.getByLabelText("workbuddy.form.maxOutputTokens"),
    ).toBeInTheDocument();

    // 删掉第一行，展开的仍是原来那一行。
    fireEvent.click(
      screen.getAllByRole("button", { name: "common.delete" })[0],
    );
    expect(screen.getAllByLabelText("opencode.modelId")).toHaveLength(2);
    expect(screen.getByLabelText("workbuddy.form.maxInputTokens")).toHaveValue(
      333,
    );
  });

  it("toggles model capabilities with switches", async () => {
    const submit = vi.fn();
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{ name: "DeepSeek", settingsConfig: existing }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "opencode.toggleModelDetails" }),
    );
    const reasoning = screen.getByRole("switch", {
      name: "kimi.capabilities.thinking",
    });
    expect(reasoning).not.toBeChecked();
    fireEvent.click(reasoning);
    expect(reasoning).toBeChecked();
    fireEvent.click(
      screen.getByRole("switch", { name: "kimi.capabilities.tool_use" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(
      JSON.parse(submit.mock.calls[0][0].settingsConfig).models[0],
    ).toMatchObject({ supportsReasoning: true, supportsToolCall: false });
  });

  it("rejects malformed and taken provider keys and non-http URLs", async () => {
    setProviders("workbuddy", {
      taken: { id: "taken", name: "Taken", settingsConfig: existing },
    });
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        initialData={undefined}
        submitLabel="Add"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    const save = screen.getByRole("button", { name: "Add" });
    const key = screen.getByLabelText(/opencode.providerKey/);
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Mine" },
    });
    fireEvent.change(screen.getByLabelText("workbuddy.form.url"), {
      target: { value: "api.example.com/v1/chat/completions" },
    });
    fireEvent.click(screen.getByRole("button", { name: /opencode.addModel/ }));
    fireEvent.change(screen.getByLabelText("opencode.modelId"), {
      target: { value: "m" },
    });
    fireEvent.change(key, { target: { value: "mine" } });
    expect(save).toBeDisabled();

    fireEvent.change(screen.getByLabelText("workbuddy.form.url"), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    await waitFor(() => expect(save).toBeEnabled());

    fireEvent.change(key, { target: { value: "-bad-" } });
    expect(screen.getByText("opencode.providerKeyInvalid")).toBeInTheDocument();
    expect(save).toBeDisabled();
    fireEvent.change(key, { target: { value: "taken" } });
    expect(
      await screen.findByText("opencode.providerKeyDuplicate"),
    ).toBeInTheDocument();
    expect(save).toBeDisabled();
  });

  it("fetches models from the base of the Chat Completions URL", async () => {
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([]);
    render(
      <WorkBuddyProviderForm
        appId="workbuddy"
        providerId="deepseek"
        initialData={{ name: "DeepSeek", settingsConfig: existing }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: /providerForm.fetchModels/ }),
    );
    await waitFor(() =>
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://api.deepseek.com/v1",
        "local-test-key",
        undefined,
        undefined,
        undefined,
        { apiFormat: "openai-completions" },
      ),
    );
  });
});

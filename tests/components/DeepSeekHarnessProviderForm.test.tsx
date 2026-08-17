import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  DeepSeekHarnessProviderForm,
  mergeDeepSeekHarnessConfig,
  nativeDeepSeekHarnessProfile,
} from "@/components/providers/forms/DeepSeekHarnessProviderForm";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="advanced-config"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

describe("mergeDeepSeekHarnessConfig", () => {
  it("retains unknown native profile and model fields", () => {
    const result = mergeDeepSeekHarnessConfig(
      {
        retryPolicy: { mode: "always" },
        streamIdleTimeoutMs: 30_000,
        models: [
          {
            id: "deepseek-v4-pro",
            name: "Pro",
            contextWindow: 1_000_000,
            maxTokens: 256_000,
          },
        ],
      },
      {
        apiKey: "  sk-test  ",
        includeApiKey: true,
        defaultModel: "  deepseek-v4-pro  ",
        apiKeyEnv: "  DEEPSEEK_API_KEY  ",
        baseURL: "  https://api.deepseek.com  ",
        thinking: "enabled",
        defaultReasoningEffort: "max",
        includeDefaultReasoningEffort: true,
        modelsText: "deepseek-v4-pro\nprivate-reasoner",
      },
    );

    expect(result).toMatchObject({
      apiKey: "sk-test",
      defaultModel: "deepseek-v4-pro",
      apiKeyEnv: "DEEPSEEK_API_KEY",
      baseURL: "https://api.deepseek.com",
      thinking: "enabled",
      defaultReasoningEffort: "max",
      retryPolicy: { mode: "always" },
      streamIdleTimeoutMs: 30_000,
    });
    expect(result.models).toEqual([
      {
        id: "deepseek-v4-pro",
        name: "Pro",
        contextWindow: 1_000_000,
        maxTokens: 256_000,
      },
      { id: "private-reasoner" },
    ]);
  });

  it("keeps structured edits when advanced fields change and never exposes the key", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const { container } = render(
      <DeepSeekHarnessProviderForm
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "DeepSeek Official",
          settingsConfig: {
            apiKey: "sk-private",
            apiKeyEnv: "DEEPSEEK_API_KEY",
            defaultModel: "deepseek-v4-pro",
            baseURL: "https://old.example.com",
            retryPolicy: { maxAttempts: 2 },
          },
        }}
      />,
    );

    const advanced = screen.getByLabelText("advanced-config");
    expect(advanced).not.toHaveValue(expect.stringContaining("sk-private"));

    const baseURL = container.querySelector<HTMLInputElement>(
      "#deepseek-harness-base-url",
    );
    expect(baseURL).not.toBeNull();
    fireEvent.change(baseURL!, {
      target: { value: "https://new.example.com" },
    });
    fireEvent.change(advanced, {
      target: {
        value: JSON.stringify({
          baseURL: "https://stale.example.com",
          retryPolicy: { maxAttempts: 4 },
        }),
      },
    });

    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));

    const submitted = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(submitted).toMatchObject({
      apiKey: "sk-private",
      baseURL: "https://new.example.com",
      defaultModel: "deepseek-v4-pro",
      retryPolicy: { maxAttempts: 4 },
    });
  });

  it("forces reasoning off when thinking is disabled", () => {
    const result = mergeDeepSeekHarnessConfig(
      {},
      {
        apiKey: "",
        includeApiKey: true,
        defaultModel: "deepseek-v4-flash",
        apiKeyEnv: "DEEPSEEK_API_KEY",
        baseURL: "https://api.deepseek.com",
        thinking: "disabled",
        defaultReasoningEffort: "max",
        includeDefaultReasoningEffort: true,
        modelsText: "deepseek-v4-flash\ndeepseek-v4-flash",
      },
    );

    expect(result.reasoningEffort).toBeUndefined();
    expect(result.defaultReasoningEffort).toBe("off");
    expect(result.models).toEqual([{ id: "deepseek-v4-flash" }]);
  });

  it("keeps absent credentials absent until the API key field is touched", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const { container } = render(
      <DeepSeekHarnessProviderForm
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "DeepSeek Official",
          settingsConfig: {
            apiKeyEnv: "DEEPSEEK_API_KEY",
            defaultModel: "deepseek-v4-flash",
          },
        }}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(
      JSON.parse(onSubmit.mock.calls[0][0].settingsConfig),
    ).not.toHaveProperty("apiKey");

    const keyInput = container.querySelector<HTMLInputElement>(
      "#deepseek-harness-api-key",
    );
    expect(keyInput).not.toBeNull();
    await user.type(keyInput!, "temporary");
    await user.clear(keyInput!);
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(2));
    expect(JSON.parse(onSubmit.mock.calls[1][0].settingsConfig).apiKey).toBe(
      "",
    );
  });

  it("preserves profile and default reasoning efforts independently", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const { container } = render(
      <DeepSeekHarnessProviderForm
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "DeepSeek Official",
          settingsConfig: {
            defaultModel: "deepseek-v4-flash",
            reasoningEffort: "max",
            defaultReasoningEffort: "high",
          },
        }}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
      reasoningEffort: "max",
      defaultReasoningEffort: "high",
    });

    const effortTrigger = container.querySelector<HTMLElement>(
      "#deepseek-harness-reasoning-effort",
    );
    expect(effortTrigger).not.toBeNull();
    Element.prototype.scrollIntoView = vi.fn();
    await user.click(effortTrigger!);
    await user.click(screen.getByRole("option", { name: "low" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(2));
    expect(JSON.parse(onSubmit.mock.calls[1][0].settingsConfig)).toMatchObject({
      reasoningEffort: "max",
      defaultReasoningEffort: "low",
    });
  });

  it("round-trips low at both reasoning levels without conflating them", () => {
    const result = mergeDeepSeekHarnessConfig(
      { reasoningEffort: "low" },
      {
        apiKey: "",
        includeApiKey: false,
        defaultModel: "deepseek-v4-flash",
        apiKeyEnv: "DEEPSEEK_API_KEY",
        baseURL: "",
        thinking: "enabled",
        defaultReasoningEffort: "max",
        includeDefaultReasoningEffort: true,
        modelsText: "",
      },
    );

    expect(result.reasoningEffort).toBe("low");
    expect(result.defaultReasoningEffort).toBe("max");
  });

  it("keeps private provider fields out of the advanced native profile", () => {
    expect(
      nativeDeepSeekHarnessProfile({
        apiKey: "sk-private",
        defaultModel: "deepseek-v4-pro",
        defaultReasoningEffort: "max",
        baseURL: "https://api.deepseek.com",
        retryPolicy: { maxAttempts: 3 },
      }),
    ).toEqual({
      baseURL: "https://api.deepseek.com",
      retryPolicy: { maxAttempts: 3 },
    });
  });
});

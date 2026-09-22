import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DevEcoProviderForm } from "@/components/providers/forms/DevEcoProviderForm";
import { devecoProviderPresets } from "@/config/devEcoProviderPresets";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";

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

const original = {
  kind: "custom",
  enabled: true,
  npm: "@ai-sdk/anthropic",
  options: {
    baseURL: "https://api.example.com/anthropic",
    apiKey: "local-test-key",
    headers: { "X-Test": "keep" },
  },
  models: {
    model: {
      name: "Model",
      limit: { context: 200000 },
    },
  },
  futureSetting: { keep: true },
};

describe("DevEcoProviderForm", () => {
  it.each([true, false])(
    "edits native fields while preserving model options and unknown settings (explicit API: %s)",
    async (explicitApi) => {
      const submit = vi.fn();
      const settings: Record<string, unknown> = { ...original };
      if (!explicitApi) delete settings.api;
      render(
        <DevEcoProviderForm
          appId="deveco"
          providerId="existing"
          initialData={{ name: "Existing", settingsConfig: settings }}
          submitLabel="Save"
          onSubmit={submit}
          onCancel={() => {}}
        />,
      );
      fireEvent.change(screen.getByLabelText("provider.name"), {
        target: { value: "Renamed" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
      expect(saved).toEqual({ ...settings, name: "Renamed" });
      expect(submit.mock.calls[0][0].providerKey).toBe("existing");
      expect(screen.getByLabelText("API Key")).toHaveAttribute(
        "type",
        "password",
      );
    },
  );

  it("keeps a rejected save open and displays the failure", async () => {
    render(
      <DevEcoProviderForm
        appId="deveco"
        initialData={{ name: "Existing", settingsConfig: original }}
        submitLabel="Save"
        onSubmit={async () => {
          throw new Error("configuration busy");
        }}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "configuration busy",
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("writes the SDK package as npm rather than reusing Mcode's api enum", async () => {
    const submit = vi.fn();
    render(
      <DevEcoProviderForm
        appId="deveco"
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    const presetButton = screen
      .getAllByRole("button")
      .find((button) => button.textContent?.includes("MiniMax"));
    expect(presetButton).toBeTruthy();
    fireEvent.click(presetButton!);
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "test-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    // DevEco 由 npm 选 SDK；把 Mcode 的协议枚举写进 `api` 不会改变 SDK 选择。
    expect(saved.npm).toBe("@ai-sdk/openai-compatible");
    expect(saved).not.toHaveProperty("api");
    expect(saved.options.apiKey).toBe("test-key");
    expect(saved.models).toHaveProperty("MiniMax-M3");
  });

  it.each([
    ["anthropic-messages", "@ai-sdk/anthropic"],
    ["openai-completions", "@ai-sdk/openai-compatible"],
    ["openai-responses", "@ai-sdk/openai"],
  ])("migrates a stored legacy api enum %s to npm %s", async (api, npm) => {
    const submit = vi.fn();
    render(
      <DevEcoProviderForm
        appId="deveco"
        providerId="existing"
        initialData={{
          name: "Existing",
          settingsConfig: { ...original, api, npm: undefined },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.npm).toBe(npm);
    expect(saved).not.toHaveProperty("api");
  });

  it("keeps a real DevEco api endpoint URL instead of treating it as a protocol", async () => {
    const submit = vi.fn();
    const settings = {
      ...original,
      npm: undefined,
      api: "https://gateway.example.com/v1",
    };
    render(
      <DevEcoProviderForm
        appId="deveco"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: settings }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.api).toBe("https://gateway.example.com/v1");
    expect(saved.npm).toBe("@ai-sdk/openai-compatible");
  });

  it("migrates a legacy api enum pasted into the config JSON editor", async () => {
    const submit = vi.fn();
    const settings = {
      ...original,
      npm: undefined,
      api: "anthropic-messages",
    };
    render(
      <DevEcoProviderForm
        appId="deveco"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: settings }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    // JSON 编辑器是另一条输入路径，必须与结构化字段走同一条归一化。
    fireEvent.change(screen.getByLabelText("provider.configJson"), {
      target: {
        value: JSON.stringify({
          ...original,
          npm: undefined,
          api: "openai-responses",
        }),
      },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.npm).toBe("@ai-sdk/openai");
    expect(saved).not.toHaveProperty("api");
  });

  it("keeps invalid JSON drafts from breaking the structured fields", () => {
    render(
      <DevEcoProviderForm
        appId="deveco"
        initialData={{ name: "Existing", settingsConfig: original }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("provider.configJson"), {
      target: { value: '{"options":{"baseURL":42}}' },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("provider.configJson"), {
      target: { value: JSON.stringify(original) },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("derives DevEco presets from Mcode presets with a real DevEco schema", () => {
    expect(devecoProviderPresets.length).toBe(mcodeProviderPresets.length);
    devecoProviderPresets.forEach((preset, index) => {
      expect(preset.name).toBe(mcodeProviderPresets[index].name);
      expect(preset.settingsConfig.models).toEqual(
        mcodeProviderPresets[index].settingsConfig.models,
      );
      // Mcode 的 api 枚举必须被翻译成 npm 包名，而不是照抄进 DevEco 配置。
      expect(preset.settingsConfig.npm).toMatch(/^@ai-sdk\//);
      expect(preset.settingsConfig).not.toHaveProperty("api");
    });
  });
});

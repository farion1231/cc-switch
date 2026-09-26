import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { parse as parseToml } from "smol-toml";
import { describe, expect, it, vi } from "vitest";
import { GrokBuildProviderForm } from "@/components/providers/forms/GrokBuildProviderForm";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="raw-config"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

describe("GrokBuildProviderForm", () => {
  it("offers curated Grok Build presets and applies one", async () => {
    const user = userEvent.setup();
    const { container } = render(
      <GrokBuildProviderForm
        submitLabel="Save"
        onSubmit={() => {}}
        onCancel={() => {}}
      />,
    );

    // 国产官方直连（cn_official）不在 Grok Build 预设列表里
    expect(screen.queryByRole("button", { name: /BytePlus/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Kimi/ })).toBeNull();

    await user.click(screen.getByRole("button", { name: /PatewayAI/ }));

    const baseUrlInput =
      container.querySelector<HTMLInputElement>("#codexBaseUrl");
    const nameInput =
      container.querySelector<HTMLInputElement>('input[name="name"]');
    expect(baseUrlInput?.value).toBe("https://api.pateway.ai/v1");
    expect(nameInput?.value).toBe("PatewayAI");
  });

  it("submits a complete config.toml payload with Grok defaults", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const { container } = render(
      <GrokBuildProviderForm
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    const nameInput =
      container.querySelector<HTMLInputElement>('input[name="name"]');
    const baseUrlInput =
      container.querySelector<HTMLInputElement>("#codexBaseUrl");
    expect(nameInput).not.toBeNull();
    expect(baseUrlInput).not.toBeNull();

    fireEvent.change(nameInput!, { target: { value: "Example Relay" } });
    fireEvent.change(baseUrlInput!, {
      target: { value: "https://relay.example.com/v1" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "secret-key" },
    });
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.icon).toBe("");
    const settings = JSON.parse(submitted.settingsConfig);
    const config = parseToml(settings.config) as any;

    expect(config.models.default).toBe("grok-4.5");
    expect(config.model["grok-4.5"]).toEqual({
      model: "grok-4.5",
      base_url: "https://relay.example.com/v1",
      name: "Example Relay",
      api_key: "secret-key",
      api_backend: "responses",
      context_window: 500000,
    });
  });

  it("uses the Codex-style advanced section without redundant Grok fields", () => {
    const { container } = render(
      <GrokBuildProviderForm
        submitLabel="Save"
        onSubmit={() => {}}
        onCancel={() => {}}
      />,
    );

    expect(container.querySelector("#grokbuild-profile")).toBeNull();
    expect(container.querySelector("#grokbuild-api-backend")).toBeNull();
    expect(screen.getByText("高级选项")).toBeInTheDocument();
    expect(container.querySelector("#grokbuild-context-window")).toHaveValue(
      500000,
    );
    expect(screen.getByText("上游格式")).toBeInTheDocument();
  });

  it("keeps the Grok client on Responses when the upstream uses Chat", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const configToml = `[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://relay.example.com/v1"
name = "Chat Relay"
api_key = "secret-key"
api_backend = "chat_completions"
context_window = 500000
`;
    render(
      <GrokBuildProviderForm
        providerId="chat-relay"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Chat Relay",
          category: "custom",
          settingsConfig: { config: configToml },
          meta: { apiFormat: "openai_chat" },
        }}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    const submitted = onSubmit.mock.calls[0][0];
    const settings = JSON.parse(submitted.settingsConfig);
    const config = parseToml(settings.config) as any;
    expect(submitted.meta.apiFormat).toBe("openai_chat");
    const selected = config.model[config.models.default];
    expect(selected.api_backend).toBe("responses");
    expect(selected.model).toBe("grok-4.5");
    expect(selected.base_url).toBe("https://relay.example.com/v1");
  });

  it("renders localized validation feedback for malformed TOML", async () => {
    const onSubmit = vi.fn();
    render(
      <GrokBuildProviderForm
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    fireEvent.change(screen.getByLabelText("raw-config"), {
      target: { value: "[models" },
    });

    expect(screen.getByText(/Invalid config\.toml:/)).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("loads edit-mode values and does not resubmit stale custom endpoints", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const config = `[models]
default = "existing-profile"

[model."existing-profile"]
model = "grok-upstream"
base_url = "https://existing.example.com/v1"
name = "Existing Relay"
api_key = "existing-key"
api_backend = "responses"
context_window = 250000
`;
    const { container } = render(
      <GrokBuildProviderForm
        providerId="existing-provider"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Existing Relay",
          settingsConfig: { config },
          meta: {
            custom_endpoints: {
              "https://deleted.example.com/v1": {
                url: "https://deleted.example.com/v1",
                addedAt: 1,
              },
            },
          },
        }}
      />,
    );

    expect(container.querySelector("#grokbuild-profile")).toBeNull();
    expect(
      container.querySelector<HTMLInputElement>("#codexDefaultModel")?.value,
    ).toBe("grok-upstream");
    expect(
      container.querySelector<HTMLInputElement>("#codexBaseUrl")?.value,
    ).toBe("https://existing.example.com/v1");

    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit.mock.calls[0][0].meta.custom_endpoints).toBeUndefined();
  });

  // #6427 复用 Codex 表单时把 Codex 专属文案原样带进了 Grok Build 表单。
  // 按 appId 分流后，Grok 表单不得再出现 Codex 字样或不适用条款（模型映射）。
  it("uses Grok-specific copy for the model field and collapsed advanced section", async () => {
    const user = userEvent.setup();
    const configToml = `[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://relay.example.com/v1"
name = "Chat Relay"
api_key = "secret-key"
api_backend = "chat_completions"
context_window = 500000
`;
    const { container } = render(
      <GrokBuildProviderForm
        providerId="chat-relay"
        submitLabel="Save"
        onSubmit={() => {}}
        onCancel={() => {}}
        initialData={{
          name: "Chat Relay",
          category: "custom",
          settingsConfig: { config: configToml },
          meta: { apiFormat: "openai_chat" },
        }}
      />,
    );

    const modelInput =
      container.querySelector<HTMLInputElement>("#codexDefaultModel");
    expect(modelInput?.placeholder).toBe("例如: grok-4.5");
    expect(screen.getByText(/Grok Build 默认请求的模型/)).toBeInTheDocument();
    expect(screen.queryByText(/Codex 默认请求的模型/)).toBeNull();
    // Grok 没有模型映射目录，不应出现"映射第一行"条款
    expect(screen.queryByText(/映射第一行/)).toBeNull();

    // Chat 格式且无已配置高级值时高级区默认折叠，折叠提示也应是 Grok 版本
    expect(
      screen.getByText(/Anthropic Messages 协议的供应商需开启路由接管/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/模型映射、思考能力/)).toBeNull();

    await user.click(screen.getByRole("button", { name: /高级选项/ }));
    expect(
      screen.getByText(/把请求中的 reasoning effort 转成上游 Chat 参数/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Codex 的 reasoning\.effort/)).toBeNull();
  });

  it("uses Grok-specific copy for the max output tokens hint", () => {
    const configToml = `[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://relay.example.com/v1"
name = "Anthropic Relay"
api_key = "secret-key"
api_backend = "messages"
context_window = 500000
`;
    render(
      <GrokBuildProviderForm
        providerId="anthropic-relay"
        submitLabel="Save"
        onSubmit={() => {}}
        onCancel={() => {}}
        initialData={{
          name: "Anthropic Relay",
          category: "custom",
          settingsConfig: { config: configToml },
          meta: { apiFormat: "anthropic" },
        }}
      />,
    );

    expect(screen.getByText(/^默认上限 8192 容易在长回答/)).toBeInTheDocument();
    expect(
      screen.queryByText(/Codex 不会把 model_max_output_tokens/),
    ).toBeNull();
  });

  it("saves an added model and both reasoning menus", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const configToml = `[models]
default = "grok-4.7"
default_reasoning_effort = "xhigh"

[model."grok-4.7"]
model = "grok-4.7"
base_url = "https://gateway.example/v1"
name = "Person"
api_key = "secret-key"
api_backend = "responses"
context_window = 500000

[[model."grok-4.7".reasoning_efforts]]
value = "xhigh"
label = "Extra High Effort"
default = true

[model."grok-4.6"]
model = "grok-4.6"
base_url = "https://gateway.example/v1"
name = "Person 4.6"
api_key = "secret-key"
api_backend = "responses"
context_window = 500000

[[model."grok-4.6".reasoning_efforts]]
value = "high"
label = "High Effort"
default = true
`;
    render(
      <GrokBuildProviderForm
        providerId="gateway"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Person",
          category: "custom",
          settingsConfig: { config: configToml },
        }}
      />,
    );

    expect(screen.getByLabelText("实际请求模型")).toHaveValue("grok-4.6");
    expect(screen.getByLabelText("菜单显示名")).toHaveValue("Person 4.6");
    expect(screen.getByText(/写入 config\.toml/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Save" }));

    const settings = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    const config = parseToml(settings.config) as any;
    expect(config.models.default).toBe("grok-4.7");
    expect(config.models.default_reasoning_effort).toBe("xhigh");
    expect(config.model["grok-4.6"].name).toBe("Person 4.6");
    expect(config.model["grok-4.6"].base_url).toBe(
      "https://gateway.example/v1",
    );
    expect(config.model["grok-4.7"].reasoning_efforts[0]).toMatchObject({
      value: "xhigh",
      default: true,
    });
    expect(config.model["grok-4.6"].reasoning_efforts[0]).toMatchObject({
      value: "high",
      default: true,
    });
  });

  it("keeps a newly added model row until an id is typed", async () => {
    const user = userEvent.setup();
    render(
      <GrokBuildProviderForm
        providerId="gateway"
        submitLabel="Save"
        onSubmit={() => {}}
        onCancel={() => {}}
        initialData={{
          name: "Person",
          category: "custom",
          settingsConfig: {
            config: `[models]
default = "grok-4.7"

[model."grok-4.7"]
model = "grok-4.7"
base_url = "https://gateway.example/v1"
name = "Person"
api_key = "dummy-key"
api_backend = "responses"
context_window = 500000
`,
          },
        }}
      />,
    );

    await user.click(screen.getByRole("button", { name: "添加模型" }));

    expect(screen.getByLabelText("实际请求模型")).toHaveValue("");
  });

  it("does not retarget another gateway or drop a global effort on save", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <GrokBuildProviderForm
        providerId="gateway"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Person",
          category: "custom",
          settingsConfig: {
            config: `[models]
default = "grok-4.7"
default_reasoning_effort = "low"

[model."grok-4.7"]
model = "grok-4.7"
base_url = "https://gateway.example/v1"
name = "Person"
api_key = "dummy-key"
api_backend = "responses"
context_window = 500000

[model."other"]
model = "other-model"
base_url = "https://other.example/v1"
name = "Other"
api_key = "dummy-other-key"
api_backend = "chat_completions"
context_window = 500000
`,
          },
        }}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Save" }));

    const config = parseToml(
      JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).config,
    ) as any;
    expect(config.models.default_reasoning_effort).toBe("low");
    expect(config.model.other).toMatchObject({
      model: "other-model",
      base_url: "https://other.example/v1",
      api_key: "dummy-other-key",
      api_backend: "chat_completions",
    });
  });

  it("keeps an alias profile and its extra fields when the label changes", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <GrokBuildProviderForm
        providerId="gateway"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Person",
          category: "custom",
          settingsConfig: {
            config: `[models]
default = "grok-4.7"

[model."grok-4.7"]
model = "grok-4.7"
base_url = "https://gateway.example/v1"
name = "Person"
api_key = "dummy-key"
api_backend = "responses"
context_window = 500000

[model."fast"]
model = "grok-4.6"
base_url = "https://gateway.example/v1"
name = "grok-4.6"
api_key = "dummy-key"
api_backend = "responses"
context_window = 500000
description = "must survive"
env_key = "DUMMY_ENV"
`,
          },
        }}
      />,
    );

    await user.clear(screen.getByLabelText("菜单显示名"));
    await user.type(screen.getByLabelText("菜单显示名"), "Renamed");
    await user.click(screen.getByRole("button", { name: "Save" }));

    const config = parseToml(
      JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).config,
    ) as any;
    expect(config.model["grok-4.6"]).toBeUndefined();
    expect(config.model.fast).toMatchObject({
      model: "grok-4.6",
      name: "Renamed",
      description: "must survive",
      env_key: "DUMMY_ENV",
      base_url: "https://gateway.example/v1",
    });
  });
});

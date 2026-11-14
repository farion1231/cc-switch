import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";
import { LiveAgentProviderForm } from "@/components/providers/forms/LiveAgentProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";
import {
  changeLiveAgentModel,
  liveagentProviderSchema,
  removeLiveAgentModel,
} from "@/lib/liveagentProvider";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (v: string) => void;
  }) => (
    <textarea
      aria-label="Advanced JSON"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  ),
}));
const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );
const config = {
  api: "openai-responses" as const,
  baseUrl: "https://example.com/v1",
  apiKey: "local-test-key",
  models: [
    {
      id: "model",
      name: "Model",
      contextWindow: 32000,
      reasoning: true,
      input: ["text", "image"],
    },
  ],
  activeModels: ["model"],
  customHeaders: [{ key: "X-Custom", value: "keep" }],
  usageQuery: { enabled: false },
};

describe("LiveAgent provider editing", () => {
  it("preserves native fields and updates active model references on rename", async () => {
    const submit = vi.fn();
    render(
      <LiveAgentProviderForm
        appId="liveagent"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: config }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("liveagent.modelId"), {
      target: { value: "new-model" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved).toEqual({
      ...config,
      name: "Existing",
      models: [{ ...config.models[0], id: "new-model" }],
      activeModels: ["new-model"],
    });
    expect(submit.mock.calls[0][0].providerKey).toBe("existing");
    expect(screen.getByLabelText("liveagent.apiKey")).toHaveAttribute(
      "type",
      "password",
    );
    expect(screen.getByLabelText("liveagent.providerKey")).toBeDisabled();
  });
  it("does not save invalid JSON or an unsupported protocol", async () => {
    const submit = vi.fn();
    render(
      <LiveAgentProviderForm
        appId="liveagent"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: config }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("Advanced JSON"), {
      target: { value: JSON.stringify({ ...config, api: "unsupported" }) },
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeDisabled(),
    );
    expect(submit).not.toHaveBeenCalled();
  });
  it("keeps inactive models inactive and rejects dangling selections", () => {
    const changed = changeLiveAgentModel({ ...config, activeModels: [] }, 0, {
      id: "new",
    });
    expect(changed.activeModels).toEqual([]);
    expect(
      liveagentProviderSchema.safeParse({
        ...config,
        activeModels: null,
        models: [{ id: "model", contextWindow: 0 }],
      }).success,
    ).toBe(true);
    expect(removeLiveAgentModel(config, 0).activeModels).toEqual([]);
    expect(
      liveagentProviderSchema.safeParse({
        ...config,
        activeModels: ["missing"],
      }).success,
    ).toBe(false);
    expect(
      liveagentProviderSchema.safeParse({
        ...config,
        models: [config.models[0], config.models[0]],
      }).success,
    ).toBe(false);
  });

  it("fetches /v1/models, adds picked models with filled metadata and removes unchecked ones", async () => {
    const user = userEvent.setup();
    const requests: Record<string, unknown>[] = [];
    server.use(
      http.post(
        "http://tauri.local/fetch_models_for_config",
        async ({ request }) => {
          requests.push((await request.json()) as Record<string, unknown>);
          return HttpResponse.json(
            ["model-x", "model-y"].map((id) => ({ id, ownedBy: "example" })),
          );
        },
      ),
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.json(
          JSON.stringify({
            example: {
              api: "https://example.com/v1",
              models: {
                "model-x": {
                  reasoning: true,
                  modalities: { input: ["text", "image"], output: ["text"] },
                  limit: { context: 123456, output: 32000 },
                },
              },
            },
          }),
        ),
      ),
    );
    const submit = vi.fn();
    render(
      <LiveAgentProviderForm
        appId="liveagent"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: config }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(requests[0]).toMatchObject({
      baseUrl: "https://example.com/v1",
      apiKey: "local-test-key",
      apiFormat: "openai-responses",
      requestHeaders: { "X-Custom": "keep" },
    });
    // "model" is configured but not returned: unchecking it removes it.
    for (const id of ["model-x", "model"]) {
      await user.click(within(dialog).getByRole("checkbox", { name: id }));
    }
    await user.click(
      within(dialog).getByRole("button", {
        name: "providerForm.modelPicker.apply",
      }),
    );
    await waitFor(() =>
      expect(screen.getAllByLabelText("liveagent.modelId")).toHaveLength(1),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.models).toEqual([
      {
        id: "model-x",
        contextWindow: 123456,
        maxTokens: 32000,
        reasoning: true,
        input: ["text", "image"],
      },
    ]);
    expect(saved.activeModels).toEqual(["model-x"]);
    expect(liveagentProviderSchema.safeParse(saved).success).toBe(true);
  });

  it("labels API protocols readably and explains the chosen one", () => {
    const { unmount } = render(
      <LiveAgentProviderForm
        appId="liveagent"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: config }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    expect(
      screen.getByRole("combobox", { name: "liveagent.api" }),
    ).toHaveTextContent("OpenAI Responses");
    expect(screen.getByText("liveagent.apiHint.responses")).toBeInTheDocument();
    unmount();
    render(
      <LiveAgentProviderForm
        appId="liveagent"
        providerId="typed"
        initialData={{
          name: "Typed",
          settingsConfig: {
            type: "claude_code",
            baseUrl: "",
            apiKey: "",
            models: [],
          },
        }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    expect(
      screen.getByRole("combobox", { name: "liveagent.api" }),
    ).toHaveTextContent("liveagent.apiFromType");
    expect(screen.getByText("liveagent.apiHint.fromType")).toBeInTheDocument();
  });

  it("edits sampling parameters and clears them when emptied", async () => {
    const submit = vi.fn();
    render(
      <LiveAgentProviderForm
        appId="liveagent"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: config }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "liveagent.toggleModelDetails" }),
    );
    fireEvent.change(screen.getByLabelText("liveagent.sampling.temperature"), {
      target: { value: "0.3" },
    });
    fireEvent.change(screen.getByLabelText("liveagent.sampling.top_p"), {
      target: { value: "0.9" },
    });
    fireEvent.change(screen.getByLabelText("liveagent.sampling.top_p"), {
      target: { value: "" },
    });
    expect(
      screen.getByRole("combobox", { name: "liveagent.modelApi" }),
    ).toHaveTextContent("liveagent.modelApiInherit");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.models[0].samplingParams).toEqual({ temperature: 0.3 });
    expect(saved.models[0].api).toBeUndefined();
    expect(liveagentProviderSchema.safeParse(saved).success).toBe(true);
  });

  it("explains that built-in types load models at runtime", () => {
    render(
      <LiveAgentProviderForm
        appId="liveagent"
        providerId="builtin-deepseek"
        initialData={{
          name: "DeepSeek",
          settingsConfig: {
            type: "deepseek",
            baseUrl: "",
            apiKey: "",
            models: [],
          },
        }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByText("liveagent.noModelsTyped")).toBeInTheDocument();
  });
});

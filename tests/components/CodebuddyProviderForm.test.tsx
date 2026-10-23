import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CodebuddyProviderForm } from "@/components/providers/forms/CodebuddyProviderForm";
import { codebuddyProviderPresets } from "@/config/codebuddyProviderPresets";
import { setProviders } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

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

describe("CodebuddyProviderForm", () => {
  it("edits native fields while preserving unknown model settings", async () => {
    const submit = vi.fn();
    render(
      <CodebuddyProviderForm
        appId="codebuddy"
        providerId="my-model"
        initialData={{
          name: "My Model",
          settingsConfig: {
            id: "my-model",
            name: "My Model",
            vendor: "OpenAI",
            url: "https://api.example.com/v1/chat/completions",
            apiKey: "local-test-key",
            maxOutputTokens: 4096,
            temperature: 0.2,
            reasoning: { defaultEffort: "high" },
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByLabelText(/codebuddy.modelId/)).toBeDisabled();
    expect(screen.getByLabelText("codebuddy.apiKey")).toHaveAttribute(
      "type",
      "password",
    );
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Renamed" },
    });
    fireEvent.change(screen.getByLabelText("codebuddy.maxOutputTokens"), {
      target: { value: "8192" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const values = submit.mock.calls[0][0];
    expect(values.providerKey).toBe("my-model");
    expect(JSON.parse(values.settingsConfig)).toEqual({
      name: "Renamed",
      vendor: "OpenAI",
      url: "https://api.example.com/v1/chat/completions",
      apiKey: "local-test-key",
      maxOutputTokens: 8192,
      temperature: 0.2,
      reasoning: { defaultEffort: "high" },
    });
  });

  it("requires a model id and a valid endpoint, and rejects taken ids", async () => {
    setProviders("codebuddy", {
      taken: {
        id: "taken",
        name: "Taken",
        settingsConfig: { url: "https://e.example" },
      },
    });
    const submit = vi.fn();
    render(
      <CodebuddyProviderForm
        appId="codebuddy"
        submitLabel="Add"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    const save = screen.getByRole("button", { name: "Add" });
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "New" },
    });
    fireEvent.change(screen.getByLabelText(/codebuddy.url/), {
      target: { value: "not a url" },
    });
    fireEvent.change(screen.getByLabelText(/codebuddy.modelId/), {
      target: { value: "new-model" },
    });
    expect(save).toBeDisabled();
    expect(screen.getByText("codebuddy.urlInvalid")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(/codebuddy.url/), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    fireEvent.change(screen.getByLabelText(/codebuddy.modelId/), {
      target: { value: "taken" },
    });
    await waitFor(() =>
      expect(
        screen.getByText("codebuddy.modelIdDuplicate"),
      ).toBeInTheDocument(),
    );
    expect(save).toBeDisabled();

    fireEvent.change(screen.getByLabelText(/codebuddy.modelId/), {
      target: { value: "new-model" },
    });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const values = submit.mock.calls[0][0];
    expect(values.providerKey).toBe("new-model");
    expect(JSON.parse(values.settingsConfig)).toEqual({
      name: "New",
      url: "https://api.example.com/v1/chat/completions",
      supportsToolCall: true,
    });
  });

  it("ships presets that use the documented chat-completions endpoints", () => {
    expect(codebuddyProviderPresets.length).toBeGreaterThan(0);
    for (const preset of codebuddyProviderPresets) {
      expect(preset.providerKey.trim()).toBe(preset.providerKey);
      expect(preset.settingsConfig.url).toMatch(
        /^https:\/\/.+\/chat\/completions$/,
      );
      expect(preset.settingsConfig).not.toHaveProperty("id");
    }
  });
});

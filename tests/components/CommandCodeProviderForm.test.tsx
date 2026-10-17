import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CommandCodeProviderForm } from "@/components/providers/forms/CommandCodeProviderForm";
import { commandcodeProviderPresets } from "@/config/commandcodeProviderPresets";
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

const original = {
  api: "anthropic-messages",
  baseURL: "https://api.example.com/anthropic",
  apiKey: "sk-existing",
  headers: { "X-Test": "keep" },
  models: {
    model: {
      name: "Model",
      contextWindow: 200000,
      maxOutput: 8192,
      reasoning: true,
    },
  },
  futureSetting: { keep: true },
};

describe("CommandCodeProviderForm", () => {
  it("edits native fields while preserving models and unknown settings", async () => {
    const submit = vi.fn();
    render(
      <CommandCodeProviderForm
        appId="commandcode"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: original }}
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
    expect(saved).toEqual({ ...original, name: "Renamed" });
    // The editor works in `limit`; the file keeps Command Code's own shape.
    expect(saved.models.model).toEqual({
      name: "Model",
      contextWindow: 200000,
      maxOutput: 8192,
      reasoning: true,
    });
    expect(submit.mock.calls[0][0].providerKey).toBe("existing");
  });

  it("marks a keyless endpoint with apiKey:false and omits the default api", async () => {
    const submit = vi.fn();
    render(
      <CommandCodeProviderForm
        appId="commandcode"
        providerId="existing"
        initialData={{
          name: "Existing",
          settingsConfig: {
            baseURL: "https://api.example.com/v1",
            models: { model: { name: "Model" } },
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.apiKey).toBe(false);
    expect(saved).not.toHaveProperty("api");
  });

  it("trims a typed secret so the backend can route it to auth.json", async () => {
    const submit = vi.fn();
    render(
      <CommandCodeProviderForm
        appId="commandcode"
        providerId="existing"
        initialData={{
          name: "Existing",
          settingsConfig: {
            baseURL: "https://api.example.com/v1",
            models: { model: { name: "Model" } },
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "  test-key  " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.apiKey).toBe("test-key");
  });

  it("keeps environment references verbatim in providers.json", async () => {
    const submit = vi.fn();
    render(
      <CommandCodeProviderForm
        appId="commandcode"
        providerId="existing"
        initialData={{
          name: "Existing",
          settingsConfig: {
            baseURL: "https://api.example.com/v1",
            apiKey: "$COMMANDCODE_TEST_KEY",
            models: { model: { name: "Model" } },
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.apiKey).toBe("$COMMANDCODE_TEST_KEY");
  });

  it("adds a preset under its Command Code provider id", async () => {
    const preset = commandcodeProviderPresets.find(
      (candidate) => !candidate.nameKey,
    )!;
    const submit = vi.fn();
    render(
      <CommandCodeProviderForm
        appId="commandcode"
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: preset.name }));
    expect(screen.getByLabelText(/opencode\.providerKey/)).toHaveValue(
      preset.providerKey,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const call = submit.mock.calls[0][0];
    const saved = JSON.parse(call.settingsConfig);
    expect(call.providerKey).toBe(preset.providerKey);
    expect(saved.baseURL).toBe(preset.settingsConfig.baseURL);
    expect(Object.keys(saved.models)).toEqual(
      Object.keys(preset.settingsConfig.models),
    );
    if (preset.settingsConfig.api === "openai-completions") {
      expect(saved).not.toHaveProperty("api");
    } else {
      expect(saved.api).toBe(preset.settingsConfig.api);
    }
    expect(saved.apiKey).toBe(false);
  });

  it("requires a well-formed, unused, non-reserved provider key", async () => {
    setProviders("commandcode", {
      taken: { id: "taken", name: "Taken", settingsConfig: original },
    });
    const preset = commandcodeProviderPresets.find(
      (candidate) => !candidate.nameKey,
    )!;
    const submit = vi.fn();
    render(
      <CommandCodeProviderForm
        appId="commandcode"
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: preset.name }));
    const key = screen.getByLabelText(/opencode\.providerKey/);
    const save = screen.getByRole("button", { name: "Save" });
    expect(key).toHaveValue(preset.providerKey);
    expect(save).toBeEnabled();

    fireEvent.change(key, { target: { value: "" } });
    expect(save).toBeDisabled();
    fireEvent.change(key, { target: { value: "-bad-" } });
    expect(screen.getByText("opencode.providerKeyInvalid")).toBeInTheDocument();
    expect(save).toBeDisabled();
    fireEvent.change(key, { target: { value: "anthropic" } });
    expect(screen.getByText("opencode.providerKeyInvalid")).toBeInTheDocument();
    expect(save).toBeDisabled();
    fireEvent.change(key, { target: { value: "taken" } });
    expect(
      await screen.findByText("opencode.providerKeyDuplicate"),
    ).toBeInTheDocument();
    expect(save).toBeDisabled();

    fireEvent.change(key, { target: { value: "My Key!" } });
    expect(key).toHaveValue("mykey");
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0].providerKey).toBe("mykey");
  });

  it("locks the provider key when editing", () => {
    render(
      <CommandCodeProviderForm
        appId="commandcode"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: original }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    const key = screen.getByLabelText(/opencode\.providerKey/);
    expect(key).toHaveValue("existing");
    expect(key).toBeDisabled();
    expect(
      screen.getByText("opencode.providerKeyLockedHint"),
    ).toBeInTheDocument();
  });

  it("keeps invalid JSON drafts from breaking the structured fields", () => {
    render(
      <CommandCodeProviderForm
        appId="commandcode"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: original }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("provider.configJson"), {
      target: { value: '{"baseURL":42}' },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("provider.configJson"), {
      target: { value: JSON.stringify(original) },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });
});

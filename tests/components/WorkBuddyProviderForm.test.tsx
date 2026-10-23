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
    fireEvent.change(screen.getByLabelText("workbuddy.form.modelName"), {
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

    fireEvent.click(
      screen.getByRole("button", { name: /workbuddy.form.addModel/ }),
    );
    fireEvent.change(screen.getByLabelText("workbuddy.form.modelId"), {
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
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn(), warning: vi.fn() },
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
      isAuthenticated: false,
      isStatusSuccess: true,
      accounts: [],
    }),
    useCodexOauth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      accounts: [],
    }),
    useXaiOauth: () => ({ isAuthenticated: false, accounts: [] }),
  };
});

describe("ProviderForm OpenClaw suggested defaults", () => {
  it("registers the models saved in the form when adding from a preset", async () => {
    const onSubmit = vi.fn();
    const { container } = render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="openclaw"
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const row = screen
      .getAllByRole("button")
      .find(
        (button) =>
          button.querySelector("span.truncate")?.textContent === "OpenRouter",
      );
    fireEvent.click(row!);
    fireEvent.change(container.querySelector("#openclaw-key")!, {
      target: { value: "my-or" },
    });
    const editor = screen.getByTestId("json-editor") as HTMLTextAreaElement;
    const config = JSON.parse(editor.value);
    config.apiKey = "sk-test";
    config.models = [
      { id: "anthropic/claude-sonnet-5.5", name: "Claude Sonnet 5.5" },
      { id: "stealth/ox-alpha", name: "Ox Alpha" },
    ];
    fireEvent.change(editor, { target: { value: JSON.stringify(config) } });
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].suggestedDefaults.modelCatalog).toEqual({
      "my-or/anthropic/claude-sonnet-5.5": { alias: "Sonnet" },
      "my-or/stealth/ox-alpha": {},
    });
  });
});

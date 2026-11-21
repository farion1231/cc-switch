import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

const toastError = vi.hoisted(() => vi.fn());
vi.mock("sonner", () => ({ toast: { error: toastError, success: vi.fn() } }));

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea readOnly value={value} />
  ),
}));

const renderHermesForm = (liveProviderIds: string[], providerId?: string) => {
  server.use(
    http.post("http://tauri.local/get_hermes_live_provider_ids", () =>
      HttpResponse.json(liveProviderIds),
    ),
  );
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="hermes"
        providerId={providerId}
        initialData={
          providerId
            ? {
                name: "MiniMax",
                settingsConfig: {
                  name: providerId,
                  base_url: "https://api.minimaxi.com/v1",
                  api_key: "test-key",
                },
              }
            : undefined
        }
        submitLabel="save-provider"
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
      />
    </QueryClientProvider>,
  );
};

describe("Hermes provider key shadowed by a built-in provider", () => {
  beforeEach(() => {
    toastError.mockClear();
  });

  it("warns while the typed key is a Hermes built-in id", async () => {
    // A saved card not yet in config.yaml keeps its key editable.
    renderHermesForm([], "my-card");
    const input = await screen.findByDisplayValue("my-card");
    await waitFor(() => expect(input).toBeEnabled());

    fireEvent.change(input, { target: { value: "deepseek" } });
    expect(
      await screen.findByTestId("hermes-key-builtin-warning"),
    ).toHaveTextContent("hermes.form.providerKeyBuiltin");
    expect(
      screen.queryByText(/^Lowercase letters, numbers, and hyphens only/),
    ).not.toBeInTheDocument();

    fireEvent.change(input, { target: { value: "cc-switch-deepseek" } });
    expect(
      screen.queryByTestId("hermes-key-builtin-warning"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(/^Lowercase letters, numbers, and hyphens only/),
    ).toBeInTheDocument();
  });

  it("does not warn for an alias that still reaches the custom provider", async () => {
    // A saved card not yet in config.yaml keeps its key editable.
    renderHermesForm([], "my-card");
    const input = await screen.findByDisplayValue("my-card");
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "kimi" } });
    expect(
      screen.queryByTestId("hermes-key-builtin-warning"),
    ).not.toBeInTheDocument();
  });

  it("tells the owner of a locked built-in key to re-add the provider", async () => {
    renderHermesForm(["minimax"], "minimax");
    expect(
      await screen.findByText("hermes.form.providerKeyBuiltinLocked"),
    ).toBeInTheDocument();
    expect(screen.getByDisplayValue("minimax")).toBeDisabled();
  });
});

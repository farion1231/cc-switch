import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";

describe("ProviderForm Codex meta", () => {
  it("preserves explicit image-generation strip opt-out when saving official Codex providers", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
      },
    });
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          submitLabel="Save"
          onSubmit={handleSubmit}
          onCancel={vi.fn()}
          showButtons={false}
          initialData={{
            name: "OpenAI",
            category: "official",
            settingsConfig: {
              auth: {},
              config: 'model_provider = "openai"\n',
            },
            meta: {
              stripCodexImageGenerationTools: false,
            },
          }}
        />
        <button type="submit" form="provider-form">
          Submit
        </button>
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(
      handleSubmit.mock.calls[0][0].meta?.stripCodexImageGenerationTools,
    ).toBe(false);
  });
});

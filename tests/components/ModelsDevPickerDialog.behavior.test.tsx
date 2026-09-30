import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ModelsDevPickerDialog } from "@/components/usage/ModelsDevPickerDialog";
import { fetchModelsDevPricing } from "@/lib/modelsDevPricing";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: string | { defaultValue?: string }) =>
      typeof options === "string" ? options : (options?.defaultValue ?? key),
  }),
}));

vi.mock("@/lib/modelsDevPricing", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/modelsDevPricing")>();
  return {
    ...actual,
    fetchModelsDevPricing: vi.fn(),
  };
});

const RESPONSE = {
  acme: {
    id: "acme",
    name: "Acme AI",
    models: {
      "acme-chat": {
        id: "acme-chat",
        name: "Acme Chat",
        release_date: "2025-01-01",
        cost: { input: 1.5, output: 3, cache_read: 0.3, cache_write: 0.6 },
      },
    },
  },
} as const;

function renderPicker(onSelect: (entry: unknown) => void, onClose: () => void) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <ModelsDevPickerDialog open onClose={onClose} onSelect={onSelect} />
    </QueryClientProvider>,
  );
}

describe("ModelsDevPickerDialog behavior", () => {
  it("calls onSelect with the selected entry without closing itself", async () => {
    vi.mocked(fetchModelsDevPricing).mockResolvedValue(RESPONSE);
    const onSelect = vi.fn();
    const onClose = vi.fn();

    renderPicker(onSelect, onClose);

    fireEvent.click(await screen.findByText("Acme Chat"));
    fireEvent.click(screen.getByRole("button", { name: "填入表单" }));

    expect(onSelect).toHaveBeenCalledTimes(1);
    const entry = onSelect.mock.calls[0][0] as {
      key: string;
      normalizedId: string;
      modelName: string;
      input: number;
      output: number;
      cacheRead: number;
      cacheWrite: number;
    };
    expect(entry).toMatchObject({
      key: "acme/acme-chat",
      normalizedId: "acme-chat",
      modelName: "Acme Chat",
      input: 1.5,
      output: 3,
      cacheRead: 0.3,
      cacheWrite: 0.6,
    });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("calls onClose when cancelling", async () => {
    vi.mocked(fetchModelsDevPricing).mockResolvedValue(RESPONSE);
    const onSelect = vi.fn();
    const onClose = vi.fn();

    renderPicker(onSelect, onClose);

    await screen.findByText("Acme Chat");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();
  });
});

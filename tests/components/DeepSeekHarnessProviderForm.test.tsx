import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { DEEPSEEK_HARNESS_DEFAULT_CONFIG } from "@/config/deepseekHarnessProviderPresets";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    providersApi: {
      ...actual.providersApi,
      getAll: vi.fn().mockResolvedValue({}),
    },
  };
});

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({
    data: { commonConfigConfirmed: true },
  }),
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
      aria-label="Config JSON"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

function renderForm(onSubmit: (values: ProviderFormValues) => void) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="deepseek-harness"
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("DeepSeek Harness provider form", () => {
  it("starts with the official preset and valid default config", async () => {
    const onSubmit = vi.fn();
    renderForm(onSubmit);

    expect(screen.getByTitle("官方")).toBeInTheDocument();

    expect(screen.getByLabelText("Config JSON")).toHaveValue(
      JSON.stringify(DEEPSEEK_HARNESS_DEFAULT_CONFIG, null, 2),
    );

    const nameInput = screen.getAllByRole("textbox")[0];
    fireEvent.change(nameInput, { target: { value: "DeepSeek" } });

    fireEvent.change(screen.getByLabelText("Config JSON"), {
      target: {
        value: JSON.stringify({
          apiKey: "sk-test",
          baseURL: "https://api.deepseek.com",
          profile: "desktop",
          models: [{ id: "deepseek-chat", name: "DeepSeek Chat" }],
        }),
      },
    });

    fireEvent.click(screen.getByRole("button", { name: /save-provider/u }));
    const submitted = await vi.waitFor(() => {
      const value = onSubmit.mock.calls[0]?.[0];
      expect(value).toBeDefined();
      return value;
    });
    expect(JSON.parse(submitted.settingsConfig)).toEqual({
      apiKey: "sk-test",
      baseURL: "https://api.deepseek.com",
      profile: "desktop",
      models: [{ id: "deepseek-chat", name: "DeepSeek Chat" }],
    });
  });
});

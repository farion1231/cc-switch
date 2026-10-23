import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { CopilotProviderForm } from "@/components/providers/forms/CopilotProviderForm";
import type { ProviderCategory, ProviderMeta } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const fetchModels = vi.hoisted(() => vi.fn());

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
});

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: fetchModels,
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: ({
    onAccountSelect,
  }: {
    onAccountSelect?: (id: string | null) => void;
  }) => (
    <button type="button" onClick={() => onAccountSelect?.("12345")}>
      bind-account
    </button>
  ),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (text: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const byok = {
  env: {
    COPILOT_PROVIDER_BASE_URL: "http://localhost:18317/v1",
    COPILOT_PROVIDER_TYPE: "openai",
    COPILOT_PROVIDER_WIRE_API: "completions",
    COPILOT_PROVIDER_API_KEY: "fixture-key",
    COPILOT_MODEL: "deepseek-v4-flash",
  },
};

function setup(
  settingsConfig: Record<string, unknown>,
  category: ProviderCategory = "custom",
  meta?: ProviderMeta,
) {
  const onSubmit = vi.fn();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <CopilotProviderForm
        appId="copilot"
        providerId="relay"
        initialData={{ name: "Relay", settingsConfig, category, meta }}
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />
    </QueryClientProvider>,
  );
  return onSubmit;
}

describe("CopilotProviderForm", () => {
  it("saves BYOK cards as Copilot CLI environment variables", async () => {
    const submit = setup(byok);
    fireEvent.change(screen.getByLabelText(/copilot\.form\.baseUrl/), {
      target: { value: "https://relay.example.com/v1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(JSON.parse(submit.mock.calls[0][0].settingsConfig)).toEqual({
      env: {
        ...byok.env,
        COPILOT_PROVIDER_BASE_URL: "https://relay.example.com/v1",
      },
    });
  });

  it("shows protocol and wire API as a single API format", () => {
    setup(byok);
    expect(
      screen.getByRole("combobox", { name: "copilot.form.apiFormat" }),
    ).toHaveTextContent("OpenAI Chat Completions");
    expect(screen.queryByText("copilot.form.wireApi")).toBeNull();
    expect(screen.queryByText("copilot.form.providerType")).toBeNull();
  });

  it("fetches models from the BYOK endpoint with the card's key", async () => {
    fetchModels.mockResolvedValueOnce([
      { id: "deepseek-v4-pro", ownedBy: null },
    ]);
    setup(byok);
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() => expect(fetchModels).toHaveBeenCalledOnce());
    expect(fetchModels.mock.calls[0].slice(0, 3)).toEqual([
      "http://localhost:18317/v1",
      "fixture-key",
      false,
    ]);
    expect(fetchModels.mock.calls[0][5]?.apiFormat).toBeUndefined();
  });

  it("blocks saving a BYOK card without a model", async () => {
    const submit = setup({
      env: { ...byok.env, COPILOT_MODEL: "" },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(submit).not.toHaveBeenCalled();
  });

  it("official cards keep only the model and bind an Auth Center account", async () => {
    const submit = setup(
      { env: { ...byok.env, COPILOT_MODEL: "claude-sonnet-4.6" } },
      "official",
    );
    expect(screen.queryByLabelText(/copilot\.form\.baseUrl/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "bind-account" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const values = submit.mock.calls[0][0];
    expect(JSON.parse(values.settingsConfig)).toEqual({
      env: { COPILOT_MODEL: "claude-sonnet-4.6" },
    });
    expect(values.meta.authBinding).toEqual({
      source: "managed_account",
      authProvider: "github_copilot",
      accountId: "12345",
    });
  });
});

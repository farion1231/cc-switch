import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea readOnly value={value} />
  ),
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  const signedOut = {
    isAuthenticated: false,
    isStatusSuccess: true,
    isStatusError: false,
    accounts: [],
  };
  return {
    ...actual,
    useCopilotAuth: () => signedOut,
    useCodexOauth: () => signedOut,
    useXaiOauth: () => signedOut,
  };
});

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
  window.matchMedia ??= vi.fn().mockReturnValue({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
});

describe("Antigravity model selection in the full add-provider page", () => {
  it("fetches a catalog, selects a display name, and saves the new provider", async () => {
    const requests: unknown[] = [];
    server.use(
      http.post("http://tauri.local/get_agy_models", async ({ request }) => {
        requests.push(await request.json());
        return HttpResponse.json([
          { id: "Gemini 4 Carbon (Low)", ownedBy: null },
          { id: "Gemini 4 Hydrogen (High)", ownedBy: null },
        ]);
      }),
    );
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const onOpenChange = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AddProviderDialog
          open
          appId="agy"
          onOpenChange={onOpenChange}
          onSubmit={onSubmit}
        />
      </QueryClientProvider>,
    );
    await user.click(await screen.findByText("providerPreset.custom"));
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "New Antigravity provider" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "fixture-key" },
    });
    fireEvent.change(document.getElementById("baseUrl")!, {
      target: { value: "https://relay.example/gemini" },
    });
    await user.click(
      screen.getByRole("button", {
        name: "provider.form.agy.readModelCatalog",
      }),
    );
    const selector = await screen.findByRole("button", {
      name: "Select model",
    });
    expect(requests).toEqual([
      { apiKey: "fixture-key", baseUrl: "https://relay.example/gemini" },
    ]);
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    await user.click(selector);
    const option = await screen.findByRole("option", {
      name: "Gemini 4 Carbon (Low)",
    });
    await user.click(option);
    expect(document.getElementById("gemini-model")).toHaveValue(
      "Gemini 4 Carbon (Low)",
    );
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "common.add" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "New Antigravity provider",
        settingsConfig: expect.objectContaining({
          env: expect.objectContaining({
            GEMINI_API_KEY: "fixture-key",
            GOOGLE_GEMINI_BASE_URL: "https://relay.example/gemini",
            GEMINI_MODEL: "Gemini 4 Carbon (Low)",
          }),
          config: expect.objectContaining({
            model: { name: "Gemini 4 Carbon (Low)" },
          }),
        }),
      }),
    );
  });
});

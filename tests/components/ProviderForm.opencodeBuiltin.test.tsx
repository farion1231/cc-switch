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

describe("OpenCode built-in provider editing", () => {
  beforeEach(() => {
    toastError.mockClear();
  });

  it.each(["https://opencode.ai/zen/go/v1", "https://custom.example/v1"])(
    "fetches models using the imported Base URL %s and saves the override",
    async (baseURL) => {
      const requests: unknown[] = [];
      server.use(
        http.post("http://tauri.local/get_opencode_live_provider_ids", () =>
          HttpResponse.json(["opencode-go"]),
        ),
        http.post(
          "http://tauri.local/fetch_models_for_config",
          async ({ request }) => {
            requests.push(await request.json());
            return HttpResponse.json([{ id: "glm-5.2", ownedBy: null }]);
          },
        ),
      );
      const settingsConfig = {
        name: "OpenCode Go",
        options: { apiKey: "test-key", baseURL },
      };
      const onSubmit = vi.fn();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId="opencode"
            providerId="opencode-go"
            initialData={{ name: "OpenCode Go", settingsConfig }}
            submitLabel="save-provider"
            onSubmit={onSubmit}
            onCancel={vi.fn()}
          />
        </QueryClientProvider>,
      );

      // The input is also disabled while loading; wait for the loaded lock hint.
      await screen.findByText("该供应商已添加到应用配置中，供应商标识不可修改");
      expect(screen.getByDisplayValue("opencode-go")).toBeDisabled();
      expect(screen.getByText("opencode.builtinDefaults")).toBeInTheDocument();
      expect(screen.getByDisplayValue(baseURL)).toBeInTheDocument();
      fireEvent.click(
        screen.getByRole("button", { name: "providerForm.fetchModels" }),
      );
      expect(
        await screen.findByRole("checkbox", { name: "glm-5.2" }),
      ).toBeInTheDocument();
      expect(requests).toEqual([
        expect.objectContaining({ baseUrl: baseURL, apiKey: "test-key" }),
      ]);
      const saveButton = screen.getByRole("button", { name: "save-provider" });
      expect(saveButton).toBeEnabled();
      fireEvent.click(saveButton);
      await waitFor(() => {
        expect(toastError).not.toHaveBeenCalled();
        expect(onSubmit).toHaveBeenCalledTimes(1);
      });
      expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(
        settingsConfig,
      );
    },
  );
});

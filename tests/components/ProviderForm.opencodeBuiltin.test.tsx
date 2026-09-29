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

  const renderOverride = (
    settingsConfig: Record<string, unknown>,
    providerId = "opencode-go",
  ) => {
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="opencode"
          providerId={providerId}
          initialData={{ name: "OpenCode Go", settingsConfig }}
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
        />
      </QueryClientProvider>,
    );
    return onSubmit;
  };

  it.each([undefined, "@ai-sdk/openai-compatible"])(
    "edits an existing key-only override with npm %s without requiring models",
    async (npm) => {
      server.use(
        http.post("http://tauri.local/get_opencode_live_provider_ids", () =>
          HttpResponse.json(["opencode-go"]),
        ),
      );
      const settingsConfig = {
        ...(npm ? { npm } : {}),
        options: { apiKey: "test-key" },
      };
      const onSubmit = renderOverride(settingsConfig);
      await screen.findByText("该供应商已添加到应用配置中，供应商标识不可修改");
      fireEvent.change(screen.getByDisplayValue("test-key"), {
        target: { value: "edited-test-key" },
      });
      fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
      expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual({
        ...settingsConfig,
        options: { apiKey: "edited-test-key" },
      });
      expect(toastError).not.toHaveBeenCalled();
    },
  );

  it.each([
    { options: { apiKey: "test-key" } },
    { npm: "@ai-sdk/openai-compatible", models: {} },
    { models: { "glm-5": { name: "GLM 5" } } },
  ])(
    "rejects incomplete copies outside the live config: %j",
    async (config) => {
      const onSubmit = renderOverride(config, "opencode-go-copy");
      await waitFor(() =>
        expect(screen.getByDisplayValue("opencode-go-copy")).toBeEnabled(),
      );
      expect(
        screen.queryByText("opencode.builtinDefaults"),
      ).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
      await waitFor(() =>
        expect(toastError).toHaveBeenCalledWith(
          "opencode.customProviderRequired",
        ),
      );
      expect(onSubmit).not.toHaveBeenCalled();
      expect(screen.queryByText("仍要保存")).not.toBeInTheDocument();
    },
  );

  it("does not grant the exception when an existing ID changes", async () => {
    server.use(
      http.post("http://tauri.local/get_opencode_live_provider_ids", () =>
        HttpResponse.json(["opencode-go"]),
      ),
    );
    const onSubmit = renderOverride({ options: { apiKey: "test-key" } });
    await screen.findByText("该供应商已添加到应用配置中，供应商标识不可修改");
    // Simulate a stale/programmatic ID change despite the normal input lock.
    fireEvent.change(screen.getByDisplayValue("opencode-go"), {
      target: { value: "opencode-go-copy" },
    });
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() =>
      expect(toastError).toHaveBeenCalledWith(
        "opencode.customProviderRequired",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("does not grant the exception when live membership cannot be read", async () => {
    server.use(
      http.post("http://tauri.local/get_opencode_live_provider_ids", () =>
        HttpResponse.json({ message: "read failed" }, { status: 500 }),
      ),
    );
    const onSubmit = renderOverride({ options: { apiKey: "test-key" } });
    await waitFor(() =>
      expect(screen.getByDisplayValue("opencode-go")).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() =>
      expect(toastError).toHaveBeenCalledWith(
        "opencode.customProviderRequired",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("requires models when creating a new provider", async () => {
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="opencode"
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
        />
      </QueryClientProvider>,
    );
    fireEvent.change(screen.getByLabelText(/opencode.providerKey/), {
      target: { value: "new-provider" },
    });
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() =>
      expect(toastError).toHaveBeenCalledWith(
        "opencode.customProviderRequired",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
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

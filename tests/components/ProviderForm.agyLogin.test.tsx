import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
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
    accounts: [],
  };
  return {
    ...actual,
    useCopilotAuth: () => signedOut,
    useCodexOauth: () => signedOut,
    useXaiOauth: () => signedOut,
  };
});

function renderLogin(
  env: Record<string, string> = {},
  appId: "agy" | "gemini" = "agy",
  category: "official" | "custom" = "custom",
) {
  const onSubmit = vi.fn();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId={appId}
        providerId="default"
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "Imported login",
          category,
          settingsConfig: {
            env,
            config: { model: { name: "Gemini 3.8 Flash (High)" } },
          },
        }}
      />
    </QueryClientProvider>,
  );
  return onSubmit;
}

describe("Antigravity login and API authentication", () => {
  it("saves an imported login after changing its model without a missing-key warning", async () => {
    // 导入的登录卡一律是 official（agy_config 导入时设定）
    const onSubmit = renderLogin({}, "agy", "official");
    fireEvent.change(screen.getByDisplayValue("Gemini 3.8 Flash (High)"), {
      target: { value: "Gemini 3.8 Flash (Low)" },
    });
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(
      screen.queryByRole("button", { name: "仍要保存" }),
    ).not.toBeInTheDocument();
    const settings = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(settings.config.model.name).toBe("Gemini 3.8 Flash (Low)");
    expect(settings.env.GEMINI_API_KEY).toBeUndefined();
    expect(settings.env.GOOGLE_GEMINI_BASE_URL).toBeUndefined();
  });

  it("saves a Google API key without requiring a custom endpoint", async () => {
    const onSubmit = renderLogin({ GEMINI_API_KEY: "fixture-key" });
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(
      screen.queryByRole("button", { name: "仍要保存" }),
    ).not.toBeInTheDocument();
  });

  it("warns when a non-official card has no API key", async () => {
    // 不填 Key 的登录模式就是官方卡，非官方卡必须带 Key
    const onSubmit = renderLogin();
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    expect(
      await screen.findByRole("button", { name: "仍要保存" }),
    ).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("still warns when a custom endpoint has no API key", async () => {
    const onSubmit = renderLogin({
      GOOGLE_GEMINI_BASE_URL: "https://relay.example",
    });
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    expect(
      await screen.findByRole("button", { name: "仍要保存" }),
    ).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("preserves Gemini CLI custom-provider validation", async () => {
    const onSubmit = renderLogin({}, "gemini");
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    expect(
      await screen.findByRole("button", { name: "仍要保存" }),
    ).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });
});

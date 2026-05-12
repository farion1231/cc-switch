import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";

vi.mock("@/lib/query", async () => {
  const actual = await vi.importActual<typeof import("@/lib/query")>(
    "@/lib/query",
  );
  return {
    ...actual,
    useSettingsQuery: () => ({ data: null }),
  };
});

vi.mock("@/hooks/useOpenClaw", () => ({
  useOpenClawLiveProviderIds: () => ({ data: [], isLoading: false }),
}));

vi.mock("@/hooks/useHermes", () => ({
  useHermesLiveProviderIds: () => ({ data: [], isLoading: false }),
}));

vi.mock("@/components/providers/forms/ProviderPresetSelector", () => ({
  ProviderPresetSelector: () => null,
}));

vi.mock("@/components/providers/forms/BasicFormFields", () => ({
  BasicFormFields: () => null,
}));

vi.mock("@/components/providers/forms/ClaudeFormFields", () => ({
  ClaudeFormFields: () => null,
}));

vi.mock("@/components/providers/forms/CodexFormFields", () => ({
  CodexFormFields: () => null,
}));

vi.mock("@/components/providers/forms/GeminiFormFields", () => ({
  GeminiFormFields: () => null,
}));

vi.mock("@/components/providers/forms/OpenCodeFormFields", () => ({
  OpenCodeFormFields: () => null,
}));

vi.mock("@/components/providers/forms/OpenClawFormFields", () => ({
  OpenClawFormFields: () => null,
}));

vi.mock("@/components/providers/forms/HermesFormFields", () => ({
  HermesFormFields: () => null,
}));

vi.mock("@/components/providers/forms/OmoFormFields", () => ({
  OmoFormFields: () => null,
}));

vi.mock("@/components/providers/forms/ProviderAdvancedConfig", () => ({
  ProviderAdvancedConfig: () => null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: () => null,
}));

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => <div data-testid="codex-config-editor" />,
}));

vi.mock("@/components/providers/forms/CommonConfigEditor", () => ({
  CommonConfigEditor: () => <div data-testid="claude-common-config-editor" />,
}));

vi.mock("@/components/providers/forms/GeminiConfigEditor", () => ({
  default: () => <div data-testid="gemini-config-editor" />,
}));

function renderProviderForm(appId: "claude" | "gemini") {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  render(
    <QueryClientProvider client={client}>
      <ProviderForm
        appId={appId}
        submitLabel="save"
        onSubmit={() => {}}
        onCancel={() => {}}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderForm editor routing", () => {
  it("renders CommonConfigEditor for Claude", () => {
    renderProviderForm("claude");

    expect(
      screen.getByTestId("claude-common-config-editor"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("gemini-config-editor"),
    ).not.toBeInTheDocument();
  });

  it("renders GeminiConfigEditor for Gemini", () => {
    renderProviderForm("gemini");

    expect(screen.getByTestId("gemini-config-editor")).toBeInTheDocument();
    expect(
      screen.queryByTestId("claude-common-config-editor"),
    ).not.toBeInTheDocument();
  });
});

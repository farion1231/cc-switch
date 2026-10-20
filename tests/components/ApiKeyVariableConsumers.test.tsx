import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { Form } from "@/components/ui/form";
import { HermesFormFields } from "@/components/providers/forms/HermesFormFields";
import { OpenCodeFormFields } from "@/components/providers/forms/OpenCodeFormFields";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import type { ApiKeyVar } from "@/lib/api/apiKeyVars";
import type { UniversalProvider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const apiKeyVariable: ApiKeyVar = {
  name: "MY_KEY",
  enabled: true,
  scriptDir: "C:/scripts/refresh",
  maskedValue: "demo…key",
  hasValue: true,
  state: { consecutiveFailures: 0, needsAttention: false },
  references: [],
};

vi.mock("@/lib/query/apiKeyVars", () => ({
  useApiKeyVars: () => ({ data: [apiKeyVariable] }),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: () => null,
}));

const FormShell = ({ children }: { children: React.ReactNode }) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

function renderWithQuery(ui: React.ReactElement) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );
}

describe("API Key variable consumers", () => {
  it.each(["opencode", "hermes"])(
    "%s renders a referenced API Key through its shared API Key section",
    (consumer) => {
      const onApiKeyChange = vi.fn();
      const fields =
        consumer === "opencode" ? (
          <OpenCodeFormFields
            npm="@ai-sdk/openai-compatible"
            onNpmChange={vi.fn()}
            apiKey="{{MY_KEY}}"
            onApiKeyChange={onApiKeyChange}
            category="custom"
            shouldShowApiKeyLink={false}
            websiteUrl=""
            baseUrl="https://api.example.com/v1"
            onBaseUrlChange={vi.fn()}
            headers={{}}
            onHeadersChange={vi.fn()}
            models={{}}
            onModelsChange={vi.fn()}
            extraOptions={{}}
            onExtraOptionsChange={vi.fn()}
          />
        ) : (
          <HermesFormFields
            baseUrl="https://api.example.com/v1"
            onBaseUrlChange={vi.fn()}
            apiKey="{{MY_KEY}}"
            onApiKeyChange={onApiKeyChange}
            category="custom"
            shouldShowApiKeyLink={false}
            websiteUrl=""
            apiMode="chat_completions"
            onApiModeChange={vi.fn()}
            models={[]}
            onModelsChange={vi.fn()}
            rateLimitDelay={0}
            onRateLimitDelayChange={vi.fn()}
          />
        );

      renderWithQuery(<FormShell>{fields}</FormShell>);

      expect(screen.getByDisplayValue("{{MY_KEY}}")).toHaveAttribute(
        "type",
        "text",
      );
      expect(screen.getByTestId("api-key-ref-status")).toHaveTextContent(
        "apiKeyVars.refOk",
      );
    },
  );

  it("keeps the universal provider reference literal when selecting a variable and saving", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    renderWithQuery(
      <UniversalProviderFormModal isOpen onClose={vi.fn()} onSave={onSave} />,
    );

    const apiKeyInput = screen.getByLabelText("API Key");
    fireEvent.focus(apiKeyInput);
    fireEvent.change(apiKeyInput, { target: { value: "{{" } });
    await user.click(await screen.findByRole("option", { name: /MY_KEY/ }));
    expect(screen.getByDisplayValue("{{MY_KEY}}")).toHaveAttribute(
      "type",
      "text",
    );
    expect(screen.getByTestId("api-key-ref-status")).toHaveTextContent(
      "apiKeyVars.refOk",
    );

    await user.type(screen.getByLabelText("名称"), "Variable gateway");
    await user.type(
      screen.getByLabelText("API 地址"),
      "https://api.example.com",
    );
    await user.click(screen.getByRole("button", { name: "添加" }));

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ apiKey: "{{MY_KEY}}" }),
    );
  });

  it("keeps a universal provider reference literal through save and sync", async () => {
    const user = userEvent.setup();
    const onSaveAndSync = vi.fn();
    const provider: UniversalProvider = {
      id: "universal-variable",
      name: "Variable gateway",
      providerType: "custom_gateway",
      baseUrl: "https://api.example.com",
      apiKey: "{{MY_KEY}}",
      apps: { claude: true, codex: true, gemini: true },
      models: {},
      createdAt: 1,
    };
    renderWithQuery(
      <UniversalProviderFormModal
        isOpen
        onClose={vi.fn()}
        onSave={vi.fn()}
        onSaveAndSync={onSaveAndSync}
        editingProvider={provider}
      />,
    );

    expect(screen.getByTestId("api-key-ref-status")).toHaveTextContent(
      "apiKeyVars.refOk",
    );
    await user.click(screen.getByRole("button", { name: "保存并同步" }));
    await user.click(
      (await screen.findAllByRole("button", { name: "保存并同步" })).at(-1)!,
    );

    expect(onSaveAndSync).toHaveBeenCalledWith(
      expect.objectContaining({ apiKey: "{{MY_KEY}}" }),
    );
  });
});

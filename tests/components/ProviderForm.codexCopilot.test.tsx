import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import type { CodexCopilotApiFormat, ProviderMeta } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => null,
}));
vi.mock("@/components/providers/forms/ProviderAdvancedConfig", () => ({
  ProviderAdvancedConfig: () => null,
}));
vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  const commonConfig = () => ({
    useCommonConfig: false,
    commonConfigSnippet: "",
    commonConfigError: null,
    isLoading: false,
    isExtracting: false,
    handleCommonConfigToggle: vi.fn(),
    handleCommonConfigSnippetChange: vi.fn(),
    handleExtract: vi.fn(),
    clearCommonConfigError: vi.fn(),
  });
  return {
    ...actual,
    useCopilotAuth: () => ({
      isAuthenticated: true,
      isStatusSuccess: true,
      isStatusError: false,
      defaultAccountId: "copilot-account",
      accounts: [
        {
          id: "copilot-account",
          login: "copilot-user",
          is_default: true,
        },
      ],
    }),
    useCodexOauth: () => ({ isAuthenticated: false, accounts: [] }),
    useXaiOauth: () => ({ isAuthenticated: false, accounts: [] }),
    useCommonConfigSnippet: commonConfig,
    useCodexCommonConfig: commonConfig,
    useGeminiCommonConfig: commonConfig,
  };
});
vi.mock("@/lib/query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/query")>();
  return {
    ...actual,
    useSettingsQuery: () => ({
      data: { commonConfigConfirmed: true },
    }),
  };
});

function renderForm(meta?: ProviderMeta) {
  const onSubmit = vi.fn<(values: ProviderFormValues) => void>();
  const preset = codexProviderPresets.find(
    (item) => item.providerType === "github_copilot",
  );
  if (!preset) throw new Error("Missing Codex Copilot preset");
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="codex"
        submitLabel="save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={
          meta
            ? {
                name: "GitHub Copilot",
                category: "third_party",
                settingsConfig: { auth: {}, config: preset.config },
                meta,
              }
            : undefined
        }
      />
    </QueryClientProvider>,
  );
  if (!meta) {
    fireEvent.click(screen.getByRole("button", { name: /GitHub Copilot/ }));
  }
  return onSubmit;
}

function formatControl() {
  return screen.getByRole("combobox", { name: "上游格式" });
}

const formatLabels = {
  auto: "codexConfig.upstreamFormatAuto",
  openai_chat: "Chat Completions（需开启路由）",
  openai_responses: "codexConfig.upstreamFormatCopilotResponses",
};

async function selectFormat(format: CodexCopilotApiFormat) {
  fireEvent.keyDown(formatControl(), { key: "ArrowDown" });
  fireEvent.click(
    await screen.findByRole("option", { name: formatLabels[format] }),
  );
}

describe("Codex Copilot provider form", () => {
  const scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "scrollIntoView",
  );
  beforeAll(() => {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
  });
  afterAll(() => {
    if (scrollIntoViewDescriptor) {
      Object.defineProperty(
        HTMLElement.prototype,
        "scrollIntoView",
        scrollIntoViewDescriptor,
      );
    } else {
      Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
    }
  });

  it("defaults to GPT-5.6 Astra and shows all five preset model mappings", () => {
    renderForm();
    expect(screen.getAllByDisplayValue("gpt-5.6-astra")).toHaveLength(2);
    for (const model of [
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
    ]) {
      expect(screen.getByDisplayValue(model)).toBeVisible();
    }
    expect(screen.getAllByDisplayValue("1048576")).toHaveLength(5);
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(screen.getByText("模型映射")).toBeVisible();
  });

  it.each<CodexCopilotApiFormat>(["auto", "openai_chat", "openai_responses"])(
    "persists %s without changing the Codex client wire protocol",
    async (format) => {
      const onSubmit = renderForm();
      await selectFormat(format);
      expect(screen.getByText("模型映射")).toBeVisible();
      fireEvent.click(screen.getByRole("button", { name: "save" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
      const saved = onSubmit.mock.calls[0][0];
      expect(saved.meta?.codexCopilotApiFormat).toBe(
        format === "auto" ? undefined : format,
      );
      expect(saved.meta?.apiFormat).toBe(
        format === "auto" ? "openai_chat" : format,
      );
      expect(JSON.parse(saved.settingsConfig).config).toContain(
        'wire_api = "responses"',
      );
    },
  );

  it("keeps legacy cards automatic and shows mapping even with an empty catalog", () => {
    renderForm({ providerType: "github_copilot", apiFormat: "openai_chat" });
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(screen.getByText("模型映射")).toBeVisible();
  });

  it("loads a saved explicit protocol and can return it to automatic", async () => {
    const onSubmit = renderForm({
      providerType: "github_copilot",
      apiFormat: "openai_responses",
      codexCopilotApiFormat: "openai_responses",
    });
    expect(formatControl()).toHaveTextContent(formatLabels.openai_responses);
    await selectFormat("auto");
    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(
      onSubmit.mock.calls[0][0].meta?.codexCopilotApiFormat,
    ).toBeUndefined();
    expect(onSubmit.mock.calls[0][0].meta?.apiFormat).toBe("openai_chat");
  });

  it("excludes Messages only for Copilot and resets the override on preset changes", async () => {
    renderForm();
    fireEvent.keyDown(formatControl(), { key: "ArrowDown" });
    expect(await screen.findAllByRole("option")).toHaveLength(3);
    expect(
      screen.queryByRole("option", { name: /Anthropic Messages/ }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("option", { name: formatLabels.openai_chat }),
    );
    fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
    expect(formatControl()).toHaveTextContent("Responses（原生）");
    expect(screen.getByText("模型映射")).toBeVisible();
    fireEvent.keyDown(formatControl(), { key: "ArrowDown" });
    expect(
      await screen.findByRole("option", { name: /Anthropic Messages/ }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Responses（原生）" }));
    fireEvent.click(screen.getByRole("button", { name: /GitHub Copilot/ }));
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(screen.getByText("模型映射")).toBeVisible();
  });
});

import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormProps,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import type { ProviderMeta } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const copilotAuthState = vi.hoisted(() => ({
  isAuthenticated: true,
  isStatusSuccess: true,
  isStatusError: false,
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
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
      data-testid="json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot-auth-section" />,
}));

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  return {
    ...actual,
    useCopilotAuth: () => ({
      ...copilotAuthState,
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
  };
});

const fixtureEnv = {
  ANTHROPIC_BASE_URL: "https://api.githubcopilot.com",
  ANTHROPIC_AUTH_TOKEN: "gh-token",
  ANTHROPIC_MODEL: "claude-sonnet-4",
};

function renderClaudeCopilotForm(
  meta: ProviderMeta,
  envOverrides: Record<string, string> = {},
) {
  const onSubmit = vi.fn<(values: ProviderFormValues) => void>();
  const props: ProviderFormProps = {
    appId: "claude",
    submitLabel: "save",
    onSubmit,
    onCancel: vi.fn(),
    initialData: {
      name: "GitHub Copilot",
      category: "third_party",
      settingsConfig: { env: { ...fixtureEnv, ...envOverrides } },
      meta,
    },
  };
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm {...props} />
    </QueryClientProvider>,
  );
  return onSubmit;
}

function savedEnv(atCall: number, onSubmit: ReturnType<typeof vi.fn>) {
  return JSON.parse(onSubmit.mock.calls[atCall][0].settingsConfig).env;
}

function submitMetaKeys(
  atCall: number,
  onSubmit: ReturnType<typeof vi.fn>,
): string[] {
  return Object.keys(
    (onSubmit.mock.calls[atCall][0].meta ?? {}) as Record<string, unknown>,
  );
}

describe("Claude Copilot provider form", () => {
  // Radix Popover 打开时会对选中项调用 scrollIntoView，jsdom 没有实现。
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

  it("preserves a manually configured Auto model without advertising it when capability is unknown", async () => {
    const onSubmit = renderClaudeCopilotForm(
      {
        providerType: "github_copilot",
        // 让高级区在挂载时自动展开（hasAnyAdvancedValue）。
        customUserAgent: "claude-copilot-test/1.0",
      },
      {
        ANTHROPIC_MODEL: "copilot-fusion-auto",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "copilot-fusion-auto",
      },
    );

    expect(
      await screen.findByRole("textbox", { name: "默认兜底模型" }),
    ).toHaveDisplayValue("copilot-fusion-auto");

    for (const id of ["claudeModel", "claudeDefaultSonnetModel"]) {
      const input = document.getElementById(id);
      expect(input).toHaveValue("copilot-fusion-auto");
      expect(
        within(input!.closest<HTMLElement>(".grid")!).getByRole("checkbox", {
          name: "1M",
        }),
      ).toBeDisabled();
    }

    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(savedEnv(0, onSubmit).ANTHROPIC_MODEL).toBe("copilot-fusion-auto");
    expect(submitMetaKeys(0, onSubmit)).not.toContain("copilotModelSelection");
    expect(submitMetaKeys(0, onSubmit)).not.toContain("copilotSavedFixedModel");
  });

  it("typing a concrete model over the virtual one saves the concrete model unchanged", async () => {
    const onSubmit = renderClaudeCopilotForm(
      { providerType: "github_copilot" },
      { ANTHROPIC_MODEL: "copilot-fusion-auto" },
    );

    const fallbackField = await screen.findByRole("textbox", {
      name: "默认兜底模型",
    });
    expect(fallbackField).toHaveDisplayValue("copilot-fusion-auto");
    fireEvent.change(fallbackField, { target: { value: "claude-sonnet-4-6" } });

    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(savedEnv(0, onSubmit).ANTHROPIC_MODEL).toBe("claude-sonnet-4-6");
  });

  it("legacy fusion strategy meta is inert: no strategy UI, the model field just shows the env value", async () => {
    const onSubmit = renderClaudeCopilotForm(
      {
        providerType: "github_copilot",
        copilotModelSelection: "fusion",
        copilotSavedFixedModel: "claude-opus-4",
        customUserAgent: "claude-copilot-test/1.0",
      } as unknown as ProviderMeta,
      { ANTHROPIC_MODEL: "copilot-fusion-auto" },
    );

    // 旧版策略选择器已不存在；虚拟模型还原样显示在模型输入框里。
    expect(screen.queryByRole("combobox", { name: "模型选择策略" })).toBeNull();
    expect(
      await screen.findByRole("textbox", { name: "默认兜底模型" }),
    ).toHaveDisplayValue("copilot-fusion-auto");

    fireEvent.change(screen.getByRole("textbox", { name: "默认兜底模型" }), {
      target: { value: "claude-opus-4" },
    });
    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(savedEnv(0, onSubmit).ANTHROPIC_MODEL).toBe("claude-opus-4");
  });
});

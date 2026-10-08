import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import type { ProviderCategory, ProviderMeta } from "@/types";
import type { AppMode } from "@/types/proxy";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => <div data-testid="codex-config-editor" />,
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

const LABEL = "原生 OpenAI 协议兼容";
const CONFIG =
  'model_provider = "custom"\nmodel = "gpt-6.1-sol"\n\n[model_providers.custom]\nname = "custom"\nwire_api = "responses"\nbase_url = "https://relay.example/v1"\nexperimental_bearer_token = "sk-test"\n';

function renderForm(
  onSubmit: (values: ProviderFormValues) => void,
  options: {
    meta?: ProviderMeta;
    category?: ProviderCategory;
    modeView?: AppMode;
  } = {},
) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="codex"
        providerId="relay"
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        modeView={options.modeView ?? "stack"}
        initialData={{
          name: "Relay",
          category: options.category ?? "third_party",
          settingsConfig: { auth: {}, config: CONFIG },
          meta: options.meta,
        }}
      />
    </QueryClientProvider>,
  );
}

async function save(onSubmit: ReturnType<typeof vi.fn>) {
  fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
  await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
  return onSubmit.mock.calls[0][0] as ProviderFormValues;
}

describe("ProviderForm native OpenAI compatibility opt-in", () => {
  let scrollIntoViewDescriptor: PropertyDescriptor | undefined;
  beforeEach(() => {
    scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "scrollIntoView",
    );
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
  });
  afterEach(() => {
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

  it.each(["stack", "route"] as const)(
    "is visible and persists only after opting in in %s mode",
    async (modeView) => {
      const onSubmit = vi.fn();
      renderForm(onSubmit, { modeView });

      const checkbox = screen.getByRole("checkbox", { name: LABEL });
      expect(checkbox).not.toBeChecked();
      if (modeView === "stack") {
        expect(screen.queryByTestId("codex-config-editor")).toBeNull();
      }
      fireEvent.click(checkbox);

      const values = await save(onSubmit);
      expect(values.meta?.codexOfficialCompatible).toBe(true);
      // The opt-in belongs in provider metadata; it must not rewrite upstream auth/URL.
      expect(JSON.parse(values.settingsConfig).config).toContain(
        'base_url = "https://relay.example/v1"',
      );
    },
  );

  it("preserves an existing opt-in when saved without changes", async () => {
    const onSubmit = vi.fn();
    renderForm(onSubmit, { meta: { codexOfficialCompatible: true } });

    expect(screen.getByRole("checkbox", { name: LABEL })).toBeChecked();
    const values = await save(onSubmit);
    expect(values.meta?.codexOfficialCompatible).toBe(true);
  });

  it("loads an enabled provider and removes its opt-in when disabled", async () => {
    const onSubmit = vi.fn();
    renderForm(onSubmit, { meta: { codexOfficialCompatible: true } });

    const checkbox = screen.getByRole("checkbox", { name: LABEL });
    expect(checkbox).toBeChecked();
    fireEvent.click(checkbox);

    const values = await save(onSubmit);
    expect(values.meta?.codexOfficialCompatible).toBeUndefined();
  });

  it.each(["openai_chat", "anthropic"] as const)(
    "does not expose or retain a stale opt-in for %s upstreams",
    async (apiFormat) => {
      const onSubmit = vi.fn();
      renderForm(onSubmit, {
        meta: { apiFormat, codexOfficialCompatible: true },
      });

      expect(screen.queryByRole("checkbox", { name: LABEL })).toBeNull();
      const values = await save(onSubmit);
      expect(values.meta?.apiFormat).toBe(apiFormat);
      expect(values.meta?.codexOfficialCompatible).toBeUndefined();
    },
  );

  it("clears compatibility when switching protocols, including switching back", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderForm(onSubmit, { meta: { codexOfficialCompatible: true } });

    await user.click(screen.getByRole("combobox", { name: "上游格式" }));
    await user.click(
      await screen.findByRole("option", {
        name: "Chat Completions（需开启路由）",
      }),
    );
    expect(screen.queryByRole("checkbox", { name: LABEL })).toBeNull();

    await user.click(screen.getByRole("combobox", { name: "上游格式" }));
    await user.click(
      await screen.findByRole("option", { name: "Responses（原生）" }),
    );
    expect(screen.getByRole("checkbox", { name: LABEL })).not.toBeChecked();
    const values = await save(onSubmit);
    expect(values.meta?.codexOfficialCompatible).toBeUndefined();
  });

  it("does not expose the third-party opt-in for official providers", () => {
    renderForm(vi.fn(), {
      category: "official",
      meta: { codexOfficialCompatible: true },
    });
    expect(screen.queryByRole("checkbox", { name: LABEL })).toBeNull();
  });

  it.each(["xai_oauth", "github_copilot"] as const)(
    "does not expose it for managed %s providers",
    (providerType) => {
      renderForm(vi.fn(), {
        meta: { providerType, codexOfficialCompatible: true },
      });
      expect(screen.queryByRole("checkbox", { name: LABEL })).toBeNull();
    },
  );
});

import type { ReactNode } from "react";
import { render } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import UsageScriptModal from "@/components/UsageScriptModal";
import { TEMPLATE_TYPES } from "@/config/constants";
import type { Provider, UsageScript } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: null }),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({ id, value }: { id: string; value: string }) => (
    <textarea data-testid={id} value={value} readOnly />
  ),
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: ReactNode;
    footer?: ReactNode;
  }) =>
    isOpen ? (
      <div>
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: () => null,
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

function createProvider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: overrides.id ?? "provider-1",
    name: overrides.name ?? "Test Provider",
    settingsConfig: overrides.settingsConfig ?? {},
    meta: {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "return true;",
        ...overrides.meta?.usage_script,
      },
      ...overrides.meta,
    },
  };
}

function renderNewApiModal(scriptOverrides: Partial<UsageScript> = {}) {
  const provider = createProvider({
    meta: {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "return true;",
        ...scriptOverrides,
      },
    },
  });

  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <UsageScriptModal
        provider={provider}
        appId="codex"
        isOpen
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("UsageScriptModal New API fields", () => {
  it("renders api key and base url fields for the New API template", () => {
    const { container } = renderNewApiModal({
      templateType: TEMPLATE_TYPES.NEW_API,
    });

    expect(container.querySelector("#usage-api-key")).toBeTruthy();
    expect(container.querySelector("#usage-newapi-base-url")).toBeTruthy();
    expect(container.querySelector("#usage-access-token")).toBeNull();
    expect(container.querySelector("#usage-user-id")).toBeNull();
  });

  it("keeps legacy accessToken/userId configs classified as New API", () => {
    const { container } = renderNewApiModal({
      accessToken: "legacy-token",
      userId: "legacy-user",
    });

    expect(container.querySelector("#usage-newapi-base-url")).toBeTruthy();
  });
});

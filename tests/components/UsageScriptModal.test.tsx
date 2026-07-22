import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import UsageScriptModal from "@/components/UsageScriptModal";
import { TEMPLATE_TYPES } from "@/config/constants";
import type { Provider, UsageScript } from "@/types";

const apiMocks = vi.hoisted(() => ({
  testScript: vi.fn(),
  setQueryData: vi.fn(),
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

vi.mock("@/components/JsonEditor", () => ({
  default: () => <textarea aria-label="script-editor" />,
}));

vi.mock("@/lib/api", () => ({
  usageApi: { testScript: apiMocks.testScript },
  settingsApi: { save: vi.fn(), openExternal: vi.fn() },
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: { usageConfirmed: true } }),
}));

vi.mock("@/hooks/useDarkMode", () => ({ useDarkMode: () => false }));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: vi.fn(),
    setQueryData: apiMocks.setQueryData,
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

const savedScript: UsageScript = {
  enabled: true,
  language: "javascript",
  code: "",
  timeout: 10,
  autoQueryInterval: 0,
  templateType: TEMPLATE_TYPES.SUB2API,
  baseUrl: "https://console.example.com",
  accountEmail: "person@example.com",
  accountPassword: "account-password",
};

const provider: Provider = {
  id: "sub2api-provider",
  name: "Sub2API Provider",
  settingsConfig: {
    env: { ANTHROPIC_BASE_URL: "https://inference.example.com" },
  },
  meta: { usage_script: savedScript },
};

describe("UsageScriptModal Sub2API template", () => {
  beforeEach(() => {
    apiMocks.testScript.mockResolvedValue({
      success: true,
      data: [{ used: 0.25, remaining: 9.75, unit: "USD" }],
    });
  });

  it("renders account credentials with a masked password and tests natively", async () => {
    render(
      <UsageScriptModal
        provider={provider}
        appId="claude"
        isOpen
        onClose={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: "usageScript.templateSub2API" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("usageScript.accountEmail")).toHaveValue(
      "person@example.com",
    );
    expect(
      screen.getByLabelText("usageScript.accountPassword"),
    ).toHaveAttribute("type", "password");

    fireEvent.click(
      screen.getByRole("button", { name: "usageScript.testScript" }),
    );

    await waitFor(() =>
      expect(apiMocks.testScript).toHaveBeenCalledWith(
        provider.id,
        "claude",
        "",
        10,
        undefined,
        "https://console.example.com",
        undefined,
        undefined,
        TEMPLATE_TYPES.SUB2API,
        "person@example.com",
        "account-password",
      ),
    );
  });

  it("clears account credentials when switching to another template", () => {
    const onSave = vi.fn();
    render(
      <UsageScriptModal
        provider={provider}
        appId="claude"
        isOpen
        onClose={vi.fn()}
        onSave={onSave}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "usageScript.templateGeneral" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "usageScript.saveConfig" }),
    );

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        templateType: TEMPLATE_TYPES.GENERAL,
        accountEmail: undefined,
        accountPassword: undefined,
      }),
    );
  });
});

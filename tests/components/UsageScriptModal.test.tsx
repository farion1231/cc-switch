import type { ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import UsageScriptModal from "@/components/UsageScriptModal";
import type { Provider, UsageScript } from "@/types";

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
  default: ({ value }: { value: string }) => (
    <textarea data-testid="usage-script-code" value={value} readOnly />
  ),
}));

const provider: Provider = {
  id: "sub2api-site",
  name: "Example Sub2API",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.example.com",
      ANTHROPIC_AUTH_TOKEN: "sk-test",
    },
  },
  meta: {
    usage_script: {
      enabled: true,
      language: "javascript",
      code: "({ request: { url: '' }, extractor: function () { return {}; } })",
    },
  },
};

function renderModal(
  onSave: (script: UsageScript) => void,
  usageScript?: UsageScript,
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <UsageScriptModal
        provider={
          usageScript
            ? { ...provider, meta: { usage_script: usageScript } }
            : provider
        }
        appId="claude"
        isOpen
        onClose={() => {}}
        onSave={onSave}
      />
    </QueryClientProvider>,
  );
}

describe("UsageScriptModal Sub2API template", () => {
  it("fills the /v1/usage script and saves it as the sub2api template", async () => {
    const onSave = vi.fn();
    renderModal(onSave);

    await userEvent.click(
      screen.getByRole("button", { name: "usageScript.templateSub2API" }),
    );

    const code = (
      screen.getByTestId("usage-script-code") as HTMLTextAreaElement
    ).value;
    expect(code).toContain('+ "/v1/usage"');
    expect(code).toContain("Bearer {{apiKey}}");
    // 与通用模板一样只用 API Key 和地址，不显示访问令牌、用户 ID
    expect(screen.getByLabelText(/API Key/)).toBeInTheDocument();
    expect(
      screen.queryByLabelText("usageScript.accessToken"),
    ).not.toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "usageScript.saveConfig" }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ templateType: "sub2api", code }),
    );
  });

  it("reopens a script saved with the sub2api template as that template", async () => {
    const onSave = vi.fn();
    renderModal(onSave, {
      ...provider.meta!.usage_script!,
      templateType: "sub2api",
    });

    await userEvent.click(
      screen.getByRole("button", { name: "usageScript.saveConfig" }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ templateType: "sub2api" }),
    );
  });
});

describe("UsageScriptModal NewAPI template", () => {
  it("queries the token usage API with the provider API key", async () => {
    const onSave = vi.fn();
    renderModal(onSave);

    await userEvent.click(
      screen.getByRole("button", { name: "usageScript.templateNewAPI" }),
    );

    const code = (
      screen.getByTestId("usage-script-code") as HTMLTextAreaElement
    ).value;
    expect(code).toContain('+ "/api/usage/token/"');
    expect(code).toContain('"Authorization": "Bearer {{apiKey}}"');
    expect(code).toContain('unit: "USD"');
    expect(code).toContain("unlimited_quota");
    const script = new Function(
      `return ${code.split("{{baseUrl}}").join("https://apipaths.com/v1").split("{{apiKey}}").join("sk-test")}`,
    )() as {
      request: { url: string; headers: Record<string, string> };
      extractor: (response: unknown) => Record<string, unknown>;
    };
    expect(script.request.url).toBe("https://apipaths.com/api/usage/token/");
    expect(script.request.headers.Authorization).toBe("Bearer sk-test");
    expect(
      script.extractor({
        code: true,
        data: {
          name: "Default",
          total_available: 12000000,
          total_used: 3000000,
          total_granted: 15000000,
          unlimited_quota: false,
        },
      }),
    ).toMatchObject({ remaining: 24, used: 6, total: 30, unit: "USD" });
    expect(screen.getByLabelText(/API Key/)).toBeInTheDocument();
    expect(screen.getAllByLabelText("usageScript.baseUrl")).toHaveLength(1);
    expect(
      screen.queryByLabelText("usageScript.accessToken"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("usageScript.userId"),
    ).not.toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "usageScript.saveConfig" }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ templateType: "newapi", code }),
    );
  });

  it("preserves a saved user-profile script until the user selects a new template", async () => {
    const onSave = vi.fn();
    const oldCode =
      '({ request: { url: "{{baseUrl}}/api/user/self" }, extractor: function (response) { return response; } })';
    renderModal(onSave, {
      ...provider.meta!.usage_script!,
      templateType: "newapi",
      code: oldCode,
      accessToken: "old-token",
      userId: "123",
    });

    expect(
      screen.getByTestId("usage-script-code") as HTMLTextAreaElement,
    ).toHaveValue(oldCode);

    await userEvent.click(
      screen.getByRole("button", { name: "usageScript.saveConfig" }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        templateType: "newapi",
        code: oldCode,
        accessToken: "old-token",
        userId: "123",
      }),
    );
  });
});

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

describe("UsageScriptModal official subscription hint", () => {
  // agy 的额度靠它自己的 /usage 命令查，不读登录凭据
  it("explains Antigravity's quota check instead of reading OAuth credentials", () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <UsageScriptModal
          provider={{
            id: "agy-official",
            name: "Google Official",
            category: "official",
            settingsConfig: { env: {}, config: {} },
            meta: {
              usage_script: {
                enabled: true,
                language: "javascript",
                code: "",
                templateType: "official_subscription",
              },
            },
          }}
          appId="agy"
          isOpen
          onClose={() => {}}
          onSave={vi.fn()}
        />
      </QueryClientProvider>,
    );
    expect(
      screen.getByText("usageScript.officialSubscriptionHintAgy"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("usageScript.officialSubscriptionHint"),
    ).not.toBeInTheDocument();
  });
});

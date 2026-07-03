import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import UsageScriptModal from "@/components/UsageScriptModal";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const getBalanceMock = vi.fn();
const getCodingPlanQuotaMock = vi.fn();
const getQuotaMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: {
    getBalance: (...args: unknown[]) => getBalanceMock(...args),
    getCodingPlanQuota: (...args: unknown[]) => getCodingPlanQuotaMock(...args),
    getQuota: (...args: unknown[]) => getQuotaMock(...args),
  },
}));

vi.mock("@/types", () => ({
  createUsageScript: (overrides: Record<string, unknown> = {}) => ({
    enabled: true,
    language: "javascript",
    code: "",
    timeout: 10,
    autoQueryInterval: 5,
    ...overrides,
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({
    data: {
      usageConfirmed: true,
    },
  }),
}));

vi.mock("@/hooks/useDarkMode", () => ({
  useDarkMode: () => false,
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({ children, footer, title }: any) => (
    <div>
      <h1>{title}</h1>
      <div>{children}</div>
      <div>{footer}</div>
    </div>
  ),
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: () => null,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: () => null,
}));

describe("UsageScriptModal aliyun balance preset", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    getBalanceMock.mockReset();
    getCodingPlanQuotaMock.mockReset();
    getQuotaMock.mockReset();
  });

  it("migrates old Bailian token plan configs to Official balance", async () => {
    const onSave = vi.fn();
    const onClose = vi.fn();
    const client = new QueryClient();

    render(
      <QueryClientProvider client={client}>
        <UsageScriptModal
          provider={{
            id: "bailian-provider",
            name: "Bailian",
            settingsConfig: {
              env: {
                ANTHROPIC_BASE_URL:
                  "https://dashscope.aliyuncs.com/compatible-mode/v1",
                ANTHROPIC_AUTH_TOKEN: "pk-test",
              },
            },
            meta: {
              usage_script: {
                enabled: true,
                language: "javascript",
                code: "",
                timeout: 10,
                templateType: "token_plan",
                codingPlanProvider: "bailian",
                apiKey: "LTAI-old",
                secretAccessKey: "secret-123",
              },
            },
          }}
          appId="claude"
          isOpen
          onClose={onClose}
          onSave={onSave}
        />
      </QueryClientProvider>,
    );

    expect(
      await screen.findByRole(
        "button",
        { name: "usageScript.templateBalance" },
      ),
    ).toBeInTheDocument();

    expect(screen.getByLabelText("AccessKey ID")).toHaveValue("LTAI-old");
    expect(screen.getByLabelText("AccessKey Secret")).toHaveValue("secret-123");

    fireEvent.click(screen.getByRole("button", { name: "usageScript.saveConfig" }));

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        enabled: true,
        templateType: "balance",
        accessKeyId: "LTAI-old",
        secretAccessKey: "secret-123",
      }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows Alibaba Cloud balance credentials and saves them", async () => {
    const onSave = vi.fn();
    const onClose = vi.fn();
    const client = new QueryClient();

    render(
      <QueryClientProvider client={client}>
        <UsageScriptModal
          provider={{
            id: "aliyun-balance-provider",
            name: "Alibaba Cloud",
            settingsConfig: {
              env: {
                ANTHROPIC_BASE_URL:
                  "https://dashscope.aliyuncs.com/compatible-mode/v1",
                ANTHROPIC_AUTH_TOKEN: "ak-test",
              },
            },
            meta: {
              usage_script: {
                enabled: true,
                language: "javascript",
                code: "",
                timeout: 10,
                templateType: "balance",
              },
            },
          }}
          appId="claude"
          isOpen
          onClose={onClose}
          onSave={onSave}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "usageScript.templateBalance" }),
    );

    expect(screen.getByLabelText("AccessKey ID")).toBeInTheDocument();
    expect(screen.getByLabelText("AccessKey Secret")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("AccessKey ID"), {
      target: { value: "LTAI123" },
    });
    fireEvent.change(screen.getByLabelText("AccessKey Secret"), {
      target: { value: "secret-456" },
    });

    fireEvent.click(screen.getByRole("button", { name: "usageScript.saveConfig" }));

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        enabled: true,
        templateType: "balance",
        accessKeyId: "LTAI123",
        secretAccessKey: "secret-456",
      }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("tests Bailian with the Alibaba Cloud balance endpoint", async () => {
    getBalanceMock.mockResolvedValue({
      success: true,
      data: [
        {
          planName: "Alibaba Cloud",
          remaining: 123.45,
          total: 200,
          used: 76.55,
          unit: "CNY",
        },
      ],
    });

    const onSave = vi.fn();
    const onClose = vi.fn();
    const client = new QueryClient();

    render(
      <QueryClientProvider client={client}>
        <UsageScriptModal
          provider={{
            id: "bailian-provider",
            name: "Bailian",
            settingsConfig: {
              env: {
                ANTHROPIC_BASE_URL:
                  "https://dashscope.aliyuncs.com/compatible-mode/v1",
                ANTHROPIC_AUTH_TOKEN: "ak-test",
              },
            },
            meta: {
              usage_script: {
                enabled: true,
                language: "javascript",
                code: "",
                timeout: 10,
                templateType: "balance",
              },
            },
          }}
          appId="claude"
          isOpen
          onClose={onClose}
          onSave={onSave}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(
      await screen.findByRole(
        "button",
        { name: "usageScript.templateBalance" },
      ),
    );
    fireEvent.change(screen.getByLabelText("AccessKey ID"), {
      target: { value: "LTAI123" },
    });
    fireEvent.change(screen.getByLabelText("AccessKey Secret"), {
      target: { value: "secret-123" },
    });
    fireEvent.click(screen.getByRole("button", { name: "usageScript.testScript" }));

    await waitFor(() =>
      expect(getBalanceMock).toHaveBeenCalledWith(
        "https://business.aliyuncs.com",
        "LTAI123",
        "secret-123",
      ),
    );
    expect(toastSuccessMock).toHaveBeenCalled();
    expect(String(toastSuccessMock.mock.calls[0]?.[0] ?? "")).toContain(
      "usage.availableCashAmount",
    );
  });
});

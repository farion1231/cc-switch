import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";
import type { AppProxyConfig } from "@/types/proxy";

const { useAppProxyConfig, mutateAsync, toastError } = vi.hoisted(() => ({
  useAppProxyConfig: vi.fn(),
  mutateAsync: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?: string | { defaultValue?: string; index?: number },
    ) => {
      if (typeof options === "string") return options;
      return (options?.defaultValue ?? key).replace(
        "{{index}}",
        String(options?.index ?? ""),
      );
    },
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: toastError },
}));

vi.mock("@/lib/query/proxy", () => ({
  useAppProxyConfig: (appType: string) => useAppProxyConfig(appType),
  useUpdateAppProxyConfig: () => ({
    mutateAsync,
    isPending: false,
  }),
}));

const baseConfig: AppProxyConfig = {
  appType: "claude",
  enabled: true,
  autoFailoverEnabled: false,
  maxRetries: 6,
  retryRules: [
    {
      enabled: true,
      statusCodes: [503],
      errorCodes: ["server_is_overloaded"],
      messageContains: null,
      retryCount: 3,
    },
  ],
  streamingFirstByteTimeout: 90,
  streamingIdleTimeout: 180,
  nonStreamingTimeout: 600,
  circuitFailureThreshold: 8,
  circuitSuccessThreshold: 3,
  circuitTimeoutSeconds: 90,
  circuitErrorRateThreshold: 0.7,
  circuitMinRequests: 15,
};

describe("AutoFailoverConfigPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mutateAsync.mockResolvedValue(undefined);
    useAppProxyConfig.mockReturnValue({
      data: baseConfig,
      isLoading: false,
      error: null,
    });
  });

  it("loads existing rules into structured inputs", async () => {
    render(<AutoFailoverConfigPanel appType="claude" />);

    expect(await screen.findByLabelText("HTTP 状态码")).toHaveValue("503");
    expect(screen.getByLabelText("错误码")).toHaveValue("server_is_overloaded");
    expect(
      screen.queryByRole("textbox", { name: /JSON/i }),
    ).not.toBeInTheDocument();
  });

  it("shows the retry relationship explanation on demand", async () => {
    render(<AutoFailoverConfigPanel appType="claude" />);

    expect(screen.queryByText("两类重试如何配合")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看重试次数说明" }));

    expect(await screen.findByText("两类重试如何配合")).toBeVisible();
    expect(screen.getByText("执行顺序")).toBeVisible();
    expect(screen.getByText("计算示例")).toBeVisible();
    expect(screen.getByText(/最坏共请求 12 次/)).toBeVisible();
  });

  it("adds, normalizes, and saves a retry rule", async () => {
    useAppProxyConfig.mockReturnValue({
      data: { ...baseConfig, retryRules: [] },
      isLoading: false,
      error: null,
    });
    render(<AutoFailoverConfigPanel appType="claude" />);

    fireEvent.click(screen.getByRole("button", { name: "添加规则" }));
    fireEvent.change(screen.getByLabelText("HTTP 状态码"), {
      target: { value: "503；429 503" },
    });
    fireEvent.change(screen.getByLabelText("错误码"), {
      target: { value: "slow_down, server_is_overloaded slow_down" },
    });
    fireEvent.blur(screen.getByLabelText("HTTP 状态码"));
    fireEvent.blur(screen.getByLabelText("错误码"));

    expect(screen.getByLabelText("HTTP 状态码")).toHaveValue("503, 429");
    expect(screen.getByLabelText("错误码")).toHaveValue(
      "slow_down, server_is_overloaded",
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(mutateAsync).toHaveBeenCalledTimes(1));
    expect(mutateAsync).toHaveBeenCalledWith({
      ...baseConfig,
      retryRules: [
        {
          enabled: true,
          statusCodes: [503, 429],
          errorCodes: ["slow_down", "server_is_overloaded"],
          messageContains: null,
          retryCount: 3,
        },
      ],
    });
  });

  it("blocks an unconditional rule and allows deleting the last rule", async () => {
    useAppProxyConfig.mockReturnValue({
      data: { ...baseConfig, retryRules: [] },
      isLoading: false,
      error: null,
    });
    render(<AutoFailoverConfigPanel appType="claude" />);

    fireEvent.click(screen.getByRole("button", { name: "添加规则" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(toastError).toHaveBeenCalledTimes(1);
    expect(mutateAsync).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "删除重试规则 1" }));
    expect(await screen.findByText("暂无特定错误重试规则。")).toBeVisible();
  });

  it("resets rule drafts when switching applications", async () => {
    const configs = {
      claude: baseConfig,
      codex: {
        ...baseConfig,
        appType: "codex",
        retryRules: [
          {
            enabled: true,
            statusCodes: [429],
            errorCodes: [],
            messageContains: "rate limit",
            retryCount: 2,
          },
        ],
      },
    };
    useAppProxyConfig.mockImplementation((appType: "claude" | "codex") => ({
      data: configs[appType],
      isLoading: false,
      error: null,
    }));

    const { rerender } = render(<AutoFailoverConfigPanel appType="claude" />);
    fireEvent.change(await screen.findByLabelText("HTTP 状态码"), {
      target: { value: "500" },
    });

    rerender(<AutoFailoverConfigPanel appType="codex" />);

    await waitFor(() =>
      expect(screen.getByLabelText("HTTP 状态码")).toHaveValue("429"),
    );
    expect(screen.getByLabelText("消息包含")).toHaveValue("rate limit");
  });
});

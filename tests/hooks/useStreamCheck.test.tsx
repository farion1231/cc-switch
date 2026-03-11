import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStreamCheck } from "@/hooks/useStreamCheck";

const toastSuccessMock = vi.fn();
const toastWarningMock = vi.fn();
const toastErrorMock = vi.fn();
const streamCheckProviderMock = vi.fn();
const resetCircuitBreakerMutateMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/lib/api/model-test", () => ({
  streamCheckProvider: (...args: unknown[]) => streamCheckProviderMock(...args),
}));

vi.mock("@/lib/query/failover", () => ({
  useResetCircuitBreaker: () => ({
    mutate: (...args: unknown[]) => resetCircuitBreakerMutateMock(...args),
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      switch (key) {
        case "streamCheck.operational":
          return `${options?.providerName} 运行正常 (${options?.responseTimeMs}ms)`;
        case "streamCheck.degraded":
          return `${options?.providerName} 响应较慢 (${options?.responseTimeMs}ms)`;
        case "streamCheck.failed":
          return `${options?.providerName} 检查失败: ${options?.message}`;
        case "streamCheck.error":
          return `${options?.providerName} 检查出错: ${options?.error}`;
        default:
          return typeof options?.defaultValue === "string"
            ? options.defaultValue
            : key;
      }
    },
  }),
}));

describe("useStreamCheck", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastWarningMock.mockReset();
    toastErrorMock.mockReset();
    streamCheckProviderMock.mockReset();
    resetCircuitBreakerMutateMock.mockReset();
  });

  it("shows interpolated provider name and latency for operational result", async () => {
    streamCheckProviderMock.mockResolvedValue({
      status: "operational",
      success: true,
      message: "ok",
      responseTimeMs: 123,
      modelUsed: "test-model",
      testedAt: 1,
      retryCount: 0,
    });

    const { result } = renderHook(() => useStreamCheck("gemini"));

    await act(async () => {
      await result.current.checkProvider("provider-1", "Gemini");
    });

    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Gemini 运行正常 (123ms)",
      { closeButton: true },
    );
    expect(resetCircuitBreakerMutateMock).toHaveBeenCalledWith({
      providerId: "provider-1",
      appType: "gemini",
    });
  });

  it("shows interpolated provider name and latency for degraded result", async () => {
    streamCheckProviderMock.mockResolvedValue({
      status: "degraded",
      success: true,
      message: "slow",
      responseTimeMs: 456,
      modelUsed: "test-model",
      testedAt: 1,
      retryCount: 0,
    });

    const { result } = renderHook(() => useStreamCheck("claude"));

    await act(async () => {
      await result.current.checkProvider("provider-2", "Claude");
    });

    expect(toastWarningMock).toHaveBeenCalledWith("Claude 响应较慢 (456ms)");
    expect(resetCircuitBreakerMutateMock).toHaveBeenCalledWith({
      providerId: "provider-2",
      appType: "claude",
    });
  });

  it("shows interpolated provider name and message for failed result", async () => {
    streamCheckProviderMock.mockResolvedValue({
      status: "failed",
      success: false,
      message: "HTTP 500",
      responseTimeMs: 0,
      modelUsed: "test-model",
      testedAt: 1,
      retryCount: 0,
    });

    const { result } = renderHook(() => useStreamCheck("codex"));

    await act(async () => {
      await result.current.checkProvider("provider-3", "Codex");
    });

    expect(toastErrorMock).toHaveBeenCalledWith("Codex 检查失败: HTTP 500");
    expect(resetCircuitBreakerMutateMock).not.toHaveBeenCalled();
  });

  it("shows interpolated provider name and error when request throws", async () => {
    streamCheckProviderMock.mockRejectedValue(new Error("network down"));

    const { result } = renderHook(() => useStreamCheck("claude"));

    await act(async () => {
      await result.current.checkProvider("provider-4", "Claude");
    });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "Claude 检查出错: Error: network down",
    );
    expect(resetCircuitBreakerMutateMock).not.toHaveBeenCalled();
  });
});
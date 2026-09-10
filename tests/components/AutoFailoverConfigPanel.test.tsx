import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";

const mockConfig = {
  appType: "claude",
  enabled: true,
  autoFailoverEnabled: false,
  maxRetries: 3,
  streamingFirstByteTimeout: 60,
  streamingIdleTimeout: 120,
  nonStreamingTimeout: 600,
  circuitFailureThreshold: 4,
  circuitSuccessThreshold: 2,
  circuitTimeoutSeconds: 60,
  circuitErrorRateThreshold: 0.6,
  circuitMinRequests: 10,
  // 故障转移面板保存时必须原样保留该字段，不能被清空
  subagentRoute: {
    providerId: "b",
    model: "glm-5.5-flash",
  } as { providerId: string; model: string | null } | null,
};

const updateMock = vi.fn();

vi.mock("@/lib/query/proxy", () => ({
  useAppProxyConfig: () => ({ data: mockConfig, isLoading: false, error: null }),
  useUpdateAppProxyConfig: () => ({
    mutateAsync: updateMock.mockResolvedValue(undefined),
    isPending: false,
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("AutoFailoverConfigPanel", () => {
  beforeEach(() => {
    updateMock.mockClear();
    mockConfig.subagentRoute = { providerId: "b", model: "glm-5.5-flash" };
  });

  it("保存故障转移配置时保留 subagentRoute 不被清空", async () => {
    const user = userEvent.setup();
    render(<AutoFailoverConfigPanel appType="claude" />);

    // 修改一个合法范围内的字段（0-10）以驱动真实的保存路径
    const maxRetriesInput = screen.getByLabelText(
      "proxy.autoFailover.maxRetries",
    );
    await user.clear(maxRetriesInput);
    await user.type(maxRetriesInput, "5");

    fireEvent.click(screen.getByRole("button", { name: /save/i }));

    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    const saved = updateMock.mock.calls[0][0];
    expect(saved.subagentRoute).toEqual({
      providerId: "b",
      model: "glm-5.5-flash",
    });
    // 其余字段按表单值正常写入（百分比还原为 0-1 的小数）
    expect(saved.maxRetries).toBe(5);
    expect(saved.circuitErrorRateThreshold).toBeCloseTo(0.6);
  });

  it("配置中无路由规则时保存不会凭空写入 subagentRoute", async () => {
    mockConfig.subagentRoute = null;
    render(<AutoFailoverConfigPanel appType="claude" />);

    fireEvent.click(screen.getByRole("button", { name: /save/i }));

    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    const saved = updateMock.mock.calls[0][0];
    expect(saved.subagentRoute).toBeNull();
  });
});

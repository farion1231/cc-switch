import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { SubagentRouteConfigPanel } from "@/components/proxy/SubagentRouteConfigPanel";

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
  subagentRoute: null as null | { providerId: string; model: string | null },
};

const updateMock = vi.fn();

vi.mock("@/lib/query/proxy", () => ({
  useAppProxyConfig: () => ({ data: mockConfig, isLoading: false, error: null }),
  useUpdateAppProxyConfig: () => ({
    mutateAsync: updateMock.mockResolvedValue(undefined),
    isPending: false,
  }),
}));

vi.mock("@/lib/query/queries", () => ({
  useProvidersQuery: () => ({
    data: {
      providers: {
        a: { id: "a", name: "Provider A", settingsConfig: { env: {} } },
        b: { id: "b", name: "Provider B", settingsConfig: { env: {} } },
      },
      currentProviderId: "a",
    },
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("SubagentRouteConfigPanel", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
    updateMock.mockClear();
    mockConfig.subagentRoute = null;
  });

  it("目标供应商下拉不包含当前供应商", async () => {
    const user = userEvent.setup();
    render(<SubagentRouteConfigPanel appType="claude" disabled={false} />);
    fireEvent.click(screen.getByRole("switch"));
    await user.click(screen.getByTestId("subagent-route-provider-trigger"));
    const options = await screen.findAllByRole("option");
    const values = options.map((o) => o.textContent ?? "");
    expect(values.join()).toContain("Provider B");
    expect(values.join()).not.toContain("Provider A");
  });

  it("保存时把规则写入 subagentRoute", async () => {
    const user = userEvent.setup();
    render(<SubagentRouteConfigPanel appType="claude" disabled={false} />);
    fireEvent.click(screen.getByRole("switch"));
    await user.click(screen.getByTestId("subagent-route-provider-trigger"));
    await user.click(
      await screen.findByRole("option", { name: "Provider B" }),
    );
    fireEvent.change(screen.getByTestId("subagent-route-model-input"), {
      target: { value: "glm-5.5-flash" },
    });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    const saved = updateMock.mock.calls[0][0];
    expect(saved.subagentRoute).toEqual({
      providerId: "b",
      model: "glm-5.5-flash",
    });
  });

  it("规则指向已删除供应商时显示失效警告", () => {
    mockConfig.subagentRoute = { providerId: "gone", model: "x" };
    render(<SubagentRouteConfigPanel appType="claude" disabled={false} />);
    expect(
      screen.getByText(/proxy\.subagentRoute\.targetMissingWarning/),
    ).toBeDefined();
  });

  it("面板可用（接管生效）时保存后提示重启 Claude Code", async () => {
    const infoSpy = vi.spyOn(toast, "info").mockReturnValue("");
    const user = userEvent.setup();
    render(<SubagentRouteConfigPanel appType="claude" disabled={false} />);
    fireEvent.click(screen.getByRole("switch"));
    await user.click(screen.getByTestId("subagent-route-provider-trigger"));
    await user.click(
      await screen.findByRole("option", { name: "Provider B" }),
    );
    fireEvent.change(screen.getByTestId("subagent-route-model-input"), {
      target: { value: "glm-5.5-flash" },
    });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    expect(infoSpy).toHaveBeenCalledWith(
      "proxy.subagentRoute.restartHint",
      expect.objectContaining({ duration: 10000 }),
    );
    infoSpy.mockRestore();
  });
});

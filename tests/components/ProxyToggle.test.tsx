import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";

const useProxyStatusMock = vi.hoisted(() => vi.fn());
const useProxyPoolMock = vi.hoisted(() => vi.fn());

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: useProxyStatusMock,
}));

vi.mock("@/lib/query/proxy", () => ({
  useProxyPool: useProxyPoolMock,
}));

describe("ProxyToggle", () => {
  beforeEach(() => {
    useProxyStatusMock.mockReset();
    useProxyPoolMock.mockReset();
    useProxyPoolMock.mockReturnValue({ data: undefined });
  });

  it("waits for initial proxy status before allowing takeover", () => {
    const proxyState = {
      isRunning: false,
      takeoverStatus: undefined,
      setTakeoverForApp: vi.fn(),
      isPending: false,
      isInitialStatusPending: true,
      status: undefined,
    };
    useProxyStatusMock.mockImplementation(() => proxyState);
    const { rerender } = render(<ProxyToggle activeApp="claude" />);

    expect(screen.getByRole("switch")).toBeDisabled();

    proxyState.isInitialStatusPending = false;
    rerender(<ProxyToggle activeApp="claude" />);

    expect(screen.getByRole("switch")).toBeEnabled();
  });

  it("is on in attached mode only, and turning it on enters attached mode", async () => {
    const user = userEvent.setup();
    const setTakeoverForApp = vi.fn().mockResolvedValue(undefined);
    useProxyStatusMock.mockReturnValue({
      isRunning: true,
      takeoverStatus: { claude: true },
      setTakeoverForApp,
      isPending: false,
      isInitialStatusPending: false,
      status: undefined,
    });
    // 在代理模式但是路由模式（比如刚在设置里换过来）：附加模式开关算关着。
    useProxyPoolMock.mockReturnValue({ data: { active: false, members: [] } });
    const { rerender } = render(<ProxyToggle activeApp="claude" pool />);

    expect(useProxyPoolMock).toHaveBeenLastCalledWith("claude", true);
    expect(screen.getByRole("switch")).not.toBeChecked();
    await user.click(screen.getByRole("switch"));
    expect(setTakeoverForApp).toHaveBeenLastCalledWith({
      appType: "claude",
      enabled: true,
      pool: true,
    });

    useProxyPoolMock.mockReturnValue({ data: { active: true, members: [] } });
    rerender(<ProxyToggle activeApp="claude" pool />);
    expect(screen.getByRole("switch")).toBeChecked();
    await user.click(screen.getByRole("switch"));
    expect(setTakeoverForApp).toHaveBeenLastCalledWith({
      appType: "claude",
      enabled: false,
      pool: true,
    });
  });
});

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";

const useProxyStatusMock = vi.hoisted(() => vi.fn());
const useProxyStackMock = vi.hoisted(() => vi.fn());

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: useProxyStatusMock,
}));

vi.mock("@/lib/query/proxy", () => ({
  useProxyStack: useProxyStackMock,
}));

describe("ProxyToggle", () => {
  beforeEach(() => {
    useProxyStatusMock.mockReset();
    useProxyStackMock.mockReset();
    useProxyStackMock.mockReturnValue({ data: undefined });
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

  it("is on in Stack mode only, and turning it on enters Stack mode", async () => {
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
    // 在代理模式但是路由模式（比如刚在设置里换过来）：Stack 模式开关算关着。
    useProxyStackMock.mockReturnValue({ data: { active: false, members: [] } });
    const { rerender } = render(<ProxyToggle activeApp="claude" stack />);

    expect(useProxyStackMock).toHaveBeenLastCalledWith("claude", true);
    expect(screen.getByRole("switch")).not.toBeChecked();
    await user.click(screen.getByRole("switch"));
    expect(setTakeoverForApp).toHaveBeenLastCalledWith({
      appType: "claude",
      enabled: true,
      stack: true,
    });

    useProxyStackMock.mockReturnValue({ data: { active: true, members: [] } });
    rerender(<ProxyToggle activeApp="claude" stack />);
    expect(screen.getByRole("switch")).toBeChecked();
    await user.click(screen.getByRole("switch"));
    expect(setTakeoverForApp).toHaveBeenLastCalledWith({
      appType: "claude",
      enabled: false,
      stack: true,
    });
  });
});

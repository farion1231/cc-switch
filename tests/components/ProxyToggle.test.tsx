import { render, screen, fireEvent } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import type { ProxyAppId } from "@/config/appConfig";
import type { ProxyTakeoverStatus } from "@/types/proxy";

const useProxyStatusMock = vi.hoisted(() => vi.fn());

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: useProxyStatusMock,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (typeof options?.defaultValue === "string") {
        return options.defaultValue;
      }
      return key;
    },
  }),
}));

const setTakeoverForAppMock = vi.fn();

let mockTakeoverStatus: ProxyTakeoverStatus = {
  claude: false,
  codex: false,
  gemini: false,
  grokbuild: false,
  opencode: false,
  openclaw: false,
  hermes: false,
  cursor: false,
};

function defaultProxyStatus() {
  return {
    isRunning: true,
    takeoverStatus: mockTakeoverStatus,
    setTakeoverForApp: setTakeoverForAppMock,
    isPending: false,
    isInitialStatusPending: false,
    status: { address: "127.0.0.1", port: 15721 },
  };
}

describe("ProxyToggle", () => {
  beforeEach(() => {
    useProxyStatusMock.mockReset();
    setTakeoverForAppMock.mockReset();
    mockTakeoverStatus = {
      claude: false,
      codex: false,
      gemini: false,
      grokbuild: false,
      opencode: false,
      openclaw: false,
      hermes: false,
      cursor: false,
    };
  });

  function renderForApp(activeApp: ProxyAppId, cursorEnabled: boolean) {
    mockTakeoverStatus = { ...mockTakeoverStatus, cursor: cursorEnabled };
    useProxyStatusMock.mockImplementation(defaultProxyStatus);
    return render(<ProxyToggle activeApp={activeApp} />);
  }

  it("shows Cursor label and checked state when cursor takeover is enabled", () => {
    renderForApp("cursor", true);

    const switchControl = screen.getByRole("switch");
    expect(switchControl).toHaveAttribute("aria-checked", "true");

    const container = screen.getByTitle(/Cursor/);
    expect(container).toBeInTheDocument();
    expect(container.textContent).not.toContain("OpenCode");
  });

  it("shows Cursor label and unchecked state when cursor takeover is disabled", () => {
    renderForApp("cursor", false);

    const switchControl = screen.getByRole("switch");
    expect(switchControl).toHaveAttribute("aria-checked", "false");

    const container = screen.getByTitle(/Cursor/);
    expect(container).toBeInTheDocument();
    expect(container.textContent).not.toContain("OpenCode");
  });

  it("calls setTakeoverForApp with cursor app type when toggled", () => {
    renderForApp("cursor", false);

    const switchControl = screen.getByRole("switch");
    fireEvent.click(switchControl);

    expect(setTakeoverForAppMock).toHaveBeenCalledWith({
      appType: "cursor",
      enabled: true,
    });
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
});

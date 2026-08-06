import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import type { AppId } from "@/lib/api";
import type { ProxyTakeoverStatus } from "@/types/proxy";

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

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: true,
    takeoverStatus: mockTakeoverStatus,
    setTakeoverForApp: setTakeoverForAppMock,
    isPending: false,
    status: { address: "127.0.0.1", port: 15721 },
  }),
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

describe("ProxyToggle", () => {
  beforeEach(() => {
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

  function renderForApp(activeApp: AppId, cursorEnabled: boolean) {
    mockTakeoverStatus = { ...mockTakeoverStatus, cursor: cursorEnabled };
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
});

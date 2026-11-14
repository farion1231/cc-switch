import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppTargetsIndicator } from "@/components/providers/AppTargetsIndicator";
import type { AppTargetsView } from "@/lib/api/appTargets";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.target
        ? `${key}:${options.target}`
        : options?.detail
          ? `${key}:${options.detail}`
          : key,
  }),
}));

const setRouteMock = vi.fn();
const syncMock = vi.fn();
vi.mock("@/lib/api/appTargets", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/appTargets")>();
  return {
    ...actual,
    appTargetsApi: {
      setRoute: (...args: unknown[]) => setRouteMock(...args),
      sync: (...args: unknown[]) => syncMock(...args),
    },
  };
});

const view: AppTargetsView = {
  app: "claude",
  source: "saved",
  config: {
    primary: "windows",
    windows: { enabled: true },
    wsl: [{ distro: "Alpine", enabled: true, route: false }],
  },
  resolvedDirs: {},
  records: {
    "wsl:Alpine": {
      status: "ok",
      providerId: "b",
      at: 1,
      routed: false,
    },
  },
};
let wslRunning = true;

vi.mock("@/hooks/useAppTargets", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/hooks/useAppTargets")>();
  return {
    ...actual,
    useAppTargetsOverview: () => ({
      data: {
        apps: [view],
        inventory: {
          hostWindows: true,
          wslAvailable: true,
          distros: [
            {
              distro: "Alpine",
              available: true,
              running: wslRunning,
              cached: !wslRunning,
            },
          ],
        },
      },
    }),
    useInvalidateAppTargets: () => () => Promise.resolve(),
  };
});

const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  info: vi.fn(),
}));
vi.mock("@/lib/toast", () => ({ toast: toastMock }));

describe("AppTargetsIndicator routing toggle", () => {
  beforeEach(() => {
    wslRunning = true;
    setRouteMock.mockReset();
    syncMock.mockReset();
    view.records = {
      "wsl:Alpine": { status: "ok", providerId: "b", at: 1, routed: false },
    };
    Object.values(toastMock).forEach((fn) => fn.mockReset());
  });

  it("lets a WSL environment join the shared route", async () => {
    setRouteMock.mockResolvedValue({
      target: "wsl:Alpine",
      status: "ok",
      routed: true,
      at: 2,
    });
    render(<AppTargetsIndicator app="claude" currentProviderId="a" />);
    fireEvent.click(
      screen.getByRole("button", { name: "appTargets.indicator.title" }),
    );
    const toggle = await screen.findByRole("switch", {
      name: "appTargets.route.joinFor:WSL · Alpine",
    });
    expect(screen.queryAllByRole("switch")).toHaveLength(1);
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(setRouteMock).toHaveBeenCalledWith("claude", "wsl:Alpine", true);
    expect(toastMock.success).toHaveBeenCalledWith(
      "appTargets.route.joined:WSL · Alpine",
    );
  });

  it("has no routing toggle for OpenCode", async () => {
    render(<AppTargetsIndicator app="opencode" />);
    fireEvent.click(
      screen.getByRole("button", { name: "appTargets.indicator.title" }),
    );
    expect(screen.queryAllByRole("switch")).toHaveLength(0);
  });

  it("hides saved WSL targets after shutdown and shows them again after startup", () => {
    const { rerender } = render(<AppTargetsIndicator app="claude" />);
    expect(
      screen.getByRole("button", { name: "appTargets.indicator.title" }),
    ).toBeInTheDocument();
    wslRunning = false;
    rerender(<AppTargetsIndicator app="claude" />);
    expect(
      screen.queryByRole("button", { name: "appTargets.indicator.title" }),
    ).toBeNull();
    expect(view.config.wsl[0].enabled).toBe(true);
    wslRunning = true;
    rerender(<AppTargetsIndicator app="claude" />);
    expect(
      screen.getByRole("button", { name: "appTargets.indicator.title" }),
    ).toBeInTheDocument();
  });

  it.each(["partial", "ok"] as const)(
    "retries shared-content failures even with a %s record",
    async (status) => {
      view.records["wsl:Alpine"] = {
        status,
        providerId: "b",
        at: 1,
        routed: true,
        warnings: ["mcp_failed:permission denied"],
      };
      syncMock.mockResolvedValue({
        targets: [
          {
            target: "wsl:Alpine",
            ...view.records["wsl:Alpine"],
            status: "partial",
          },
        ],
      });
      render(<AppTargetsIndicator app="claude" />);
      fireEvent.click(
        screen.getByRole("button", { name: "appTargets.indicator.title" }),
      );
      expect(
        await screen.findByText(
          "appTargets.syncFailure.mcp_failed:permission denied",
        ),
      ).toBeInTheDocument();
      const retry = screen.getByRole("button", {
        name: "appTargets.indicator.retry",
      });
      expect(retry).toBeEnabled();
      await act(async () => {
        fireEvent.click(retry);
      });
      expect(syncMock).toHaveBeenCalledWith("claude", ["wsl:Alpine"]);
      expect(toastMock.warning).toHaveBeenCalledWith(
        "appTargets.toast.partial",
        {
          description: "appTargets.syncFailure.mcp_failed:permission denied",
        },
      );
      expect(toastMock.success).not.toHaveBeenCalled();
    },
  );
});

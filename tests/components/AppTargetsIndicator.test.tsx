import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppTargetsIndicator } from "@/components/providers/AppTargetsIndicator";
import type { AppTargetsView } from "@/lib/api/appTargets";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.target ? `${key}:${options.target}` : key,
  }),
}));

const setRouteMock = vi.fn();
vi.mock("@/lib/api/appTargets", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/appTargets")>();
  return {
    ...actual,
    appTargetsApi: {
      setRoute: (...args: unknown[]) => setRouteMock(...args),
      sync: vi.fn(),
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

vi.mock("@/hooks/useAppTargets", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/hooks/useAppTargets")>();
  return {
    ...actual,
    useAppTargetView: () => view,
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
    setRouteMock.mockReset();
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
});

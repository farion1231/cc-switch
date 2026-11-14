import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState, type ReactNode } from "react";
import type { AppTargetsOverview } from "@/lib/api/appTargets";
import { WslInstallRows } from "@/components/apps/WslInstallRows";
import { InstallTargetsDialog } from "@/components/apps/InstallTargetsDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
const actionMock = vi.fn();
const overviewMock = vi.fn();
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@/lib/toast", () => ({ toast: toastMock }));
vi.mock("@/lib/api/appTargets", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/appTargets")>()),
  appTargetsApi: {
    overview: (...args: unknown[]) => overviewMock(...args),
    wslToolAction: (...args: unknown[]) => actionMock(...args),
  },
}));

let overview: AppTargetsOverview;
vi.mock("@/hooks/useAppTargets", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/hooks/useAppTargets")>()),
  useAppTargetsOverview: () => ({ data: overview }),
}));

function wrapper({ children }: { children: ReactNode }) {
  const [client] = useState(() => new QueryClient());
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("WSL client repair", () => {
  beforeEach(() => {
    actionMock.mockReset().mockResolvedValue(undefined);
    overviewMock.mockReset();
    Object.values(toastMock).forEach((mock) => mock.mockReset());
    overview = {
      inventory: {
        hostWindows: true,
        wslAvailable: true,
        distros: [
          { distro: "Ubuntu", available: true, running: true, cached: false },
        ],
      },
      installs: {
        Ubuntu: {
          claude: {
            status: "broken",
            path: "/usr/local/bin/claude",
            error: "Node version unsupported",
            detectedAt: 1,
            cached: false,
          },
        },
      },
      multiEnv: true,
      apps: [],
    };
    overviewMock.mockImplementation(async () => overview);
  });

  it("shows the client error and reinstalls a broken client", async () => {
    actionMock.mockImplementation(async () => {
      overview.installs.Ubuntu.claude = {
        status: "installed",
        version: "2.1.0",
        detectedAt: 2,
        cached: false,
      };
    });
    render(
      <WslInstallRows
        tool="claude"
        appName="Claude Code"
        gridClassName="grid"
      />,
      { wrapper },
    );
    expect(screen.getByText("Node version unsupported")).toBeInTheDocument();
    expect(
      screen.getByText("appTargets.installs.repairHint"),
    ).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", {
          name: "appTargets.installs.reinstall · WSL · Ubuntu",
        }),
      );
    });
    expect(actionMock).toHaveBeenCalledWith("claude", "Ubuntu", "install");
    expect(overviewMock).toHaveBeenCalledWith(false, true);
    expect(toastMock.success).toHaveBeenCalledWith(
      "appTargets.installs.reinstalled",
    );
  });

  it("warns when reinstall finishes but the client is still broken", async () => {
    render(
      <WslInstallRows
        tool="claude"
        appName="Claude Code"
        gridClassName="grid"
      />,
      { wrapper },
    );
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", {
          name: "appTargets.installs.reinstall · WSL · Ubuntu",
        }),
      );
    });
    expect(toastMock.warning).toHaveBeenCalledWith(
      "appTargets.installs.notReady",
      {
        description: "Node version unsupported",
        duration: 8000,
      },
    );
    expect(toastMock.success).not.toHaveBeenCalled();
  });

  it("allows selecting a broken client in the install dialog", async () => {
    render(
      <InstallTargetsDialog
        open
        tool="claude"
        appName="Claude Code"
        windowsInstalled
        onClose={vi.fn()}
        onInstallWindows={vi.fn()}
      />,
      { wrapper },
    );
    const target = await screen.findByRole("checkbox", {
      name: "WSL · Ubuntu",
    });
    expect(target).toBeEnabled();
    expect(target).not.toBeChecked();
    fireEvent.click(target);
    await act(async () => {
      fireEvent.click(screen.getByText("appTargets.installTargets.confirm"));
    });
    await waitFor(() =>
      expect(actionMock).toHaveBeenCalledWith("claude", "Ubuntu", "install"),
    );
  });

  it("shows a failed detection alongside the previous installed version", () => {
    overview.installs.Ubuntu.claude = {
      status: "installed",
      version: "2.1.0",
      detectedAt: 2,
      cached: true,
      probeError: "Command timed out after 10s",
    };
    render(
      <WslInstallRows
        tool="claude"
        appName="Claude Code"
        gridClassName="grid"
      />,
      { wrapper },
    );
    expect(screen.getByText("2.1.0")).toBeInTheDocument();
    expect(
      screen.getByText("appTargets.installs.probeFailedCached"),
    ).toBeInTheDocument();
    expect(screen.getByText("Command timed out after 10s")).toBeInTheDocument();
  });

  it("hides cached WSL installation rows on shutdown and restores them on startup", () => {
    const row = (
      <WslInstallRows
        tool="claude"
        appName="Claude Code"
        gridClassName="grid"
      />
    );
    const { rerender } = render(row, { wrapper });
    expect(screen.getByText("WSL · Ubuntu")).toBeInTheDocument();
    overview.inventory.distros[0].running = false;
    rerender(
      <WslInstallRows
        tool="claude"
        appName="Claude Code"
        gridClassName="grid"
      />,
    );
    expect(screen.queryByText("WSL · Ubuntu")).toBeNull();
    overview.inventory.distros[0].running = true;
    rerender(
      <WslInstallRows
        tool="claude"
        appName="Claude Code"
        gridClassName="grid"
      />,
    );
    expect(screen.getByText("WSL · Ubuntu")).toBeInTheDocument();
  });

  it("removes stopped targets from an open install dialog", async () => {
    const onClose = vi.fn();
    const onInstallWindows = vi.fn();
    const dialog = () => (
      <InstallTargetsDialog
        open
        tool="claude"
        appName="Claude Code"
        windowsInstalled
        onClose={onClose}
        onInstallWindows={onInstallWindows}
      />
    );
    const { rerender } = render(dialog(), { wrapper });
    const target = await screen.findByRole("checkbox", {
      name: "WSL · Ubuntu",
    });
    fireEvent.click(target);
    overview.inventory.distros[0].running = false;
    rerender(dialog());
    expect(screen.queryByRole("checkbox", { name: "WSL · Ubuntu" })).toBeNull();
    expect(
      screen.getByRole("button", { name: "appTargets.installTargets.confirm" }),
    ).toBeDisabled();
    expect(actionMock).not.toHaveBeenCalled();
  });
});

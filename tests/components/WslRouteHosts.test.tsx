import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { WslRouteHosts } from "@/components/settings/WslRouteHosts";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const testWslRouteMock = vi.fn();
const wslRouteInfoMock = vi.fn();
const setWslClientHostMock = vi.fn();

vi.mock("@/lib/api/appTargets", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/appTargets")>()),
  appTargetsApi: {
    wslRouteInfo: (...args: unknown[]) => wslRouteInfoMock(...args),
    testWslRoute: (...args: unknown[]) => testWslRouteMock(...args),
    setWslClientHost: (...args: unknown[]) => setWslClientHostMock(...args),
  },
}));

vi.mock("@/hooks/useAppTargets", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/hooks/useAppTargets")>()),
  useAppTargetsOverview: () => ({
    data: {
      multiEnv: true,
      inventory: {
        hostWindows: true,
        wslAvailable: true,
        distros: [
          { distro: "Ubuntu", available: true, running: true, cached: false },
        ],
      },
      installs: {},
      apps: [],
    },
  }),
}));

vi.mock("@/lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("WslRouteHosts", () => {
  beforeEach(() => {
    wslRouteInfoMock.mockReset().mockResolvedValue([
      {
        distro: "Ubuntu",
        savedHost: "172.20.0.1",
        detectedHost: "172.20.0.1",
        networkingMode: "nat",
        listenAddress: "0.0.0.0",
        listenUnreachable: false,
      },
    ]);
    testWslRouteMock.mockReset().mockResolvedValue({
      ok: true,
      url: "http://172.25.0.1:15721/health",
      detail: "ok",
    });
    setWslClientHostMock.mockReset().mockResolvedValue(undefined);
  });

  it("tests the unsaved draft host and clears previous test status when edited", async () => {
    render(<WslRouteHosts />, { wrapper });
    const input = await screen.findByLabelText("appTargets.routeHosts.hostFor");

    fireEvent.change(input, { target: { value: "172.25.0.1" } });
    await act(async () => {
      fireEvent.click(screen.getByText("appTargets.routeHosts.test"));
    });

    await waitFor(() =>
      expect(testWslRouteMock).toHaveBeenCalledWith("Ubuntu", "172.25.0.1"),
    );
    expect(
      await screen.findByText("appTargets.routeHosts.testOk"),
    ).toBeInTheDocument();

    fireEvent.change(input, { target: { value: "172.26.0.1" } });
    expect(screen.queryByText("appTargets.routeHosts.testOk")).toBeNull();
  });
});

import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { ProviderEnvSwitches } from "@/components/providers/ProviderEnvSwitches";
import type { AppTargetsView, WslInventory } from "@/lib/api/appTargets";
import type { Provider } from "@/types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.target ? `${key}:${options.target}` : key,
  }),
}));

const activateMock = vi.fn();
const deactivateMock = vi.fn();
vi.mock("@/lib/api/appTargets", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/appTargets")>();
  return {
    ...actual,
    appTargetsApi: {
      activate: (...args: unknown[]) => activateMock(...args),
      deactivate: (...args: unknown[]) => deactivateMock(...args),
    },
  };
});

const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  info: vi.fn(),
}));
vi.mock("@/lib/toast", () => ({ toast: toastMock }));

const view: AppTargetsView = {
  app: "claude",
  source: "saved",
  config: {
    primary: "windows",
    windows: { enabled: true },
    wsl: [
      { distro: "Alpine", enabled: true },
      { distro: "Debian", enabled: false },
    ],
  },
  resolvedDirs: {},
  records: { "wsl:Alpine": { status: "ok", providerId: "b", at: 1 } },
};

const provider = (id: string) => ({ id, name: id.toUpperCase() }) as Provider;
const inventory: WslInventory = {
  hostWindows: true,
  wslAvailable: true,
  distros: ["Alpine", "Debian", "Ubuntu"].map((distro) => ({
    distro,
    available: true,
    running: true,
    cached: false,
  })),
};

function renderSwitches(id: string, mainActive: boolean, onMain = vi.fn()) {
  const client = new QueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  render(
    <ProviderEnvSwitches
      app="claude"
      provider={provider(id)}
      view={view}
      inventory={inventory}
      mainActive={mainActive}
      mainAction={{ key: "switch", label: "switch", onClick: onMain }}
    />,
    { wrapper },
  );
  return onMain;
}

describe("ProviderEnvSwitches", () => {
  beforeEach(() => {
    activateMock.mockReset();
    deactivateMock.mockReset();
  });
  it("only switches Windows and shows WSL as a read-only follower", () => {
    const onMain = renderSwitches("a", false);
    const [windows, alpine] = screen.getAllByRole("button");
    expect(windows).toBeEnabled();
    expect(alpine).toBeDisabled();
    fireEvent.click(windows);
    fireEvent.click(alpine);
    expect(onMain).toHaveBeenCalledTimes(1);
    expect(activateMock).not.toHaveBeenCalled();
  });
  it("uses the Windows selection instead of the last WSL sync result", () => {
    renderSwitches("a", true);
    const [windows, alpine] = screen.getAllByRole("button");
    expect(windows).toHaveAttribute("aria-pressed", "true");
    expect(alpine).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(alpine);
    expect(deactivateMock).not.toHaveBeenCalled();
  });
});

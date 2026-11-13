import { act, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { ProviderEnvSwitches } from "@/components/providers/ProviderEnvSwitches";
import type { AppTargetsView } from "@/lib/api/appTargets";
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
    Object.values(toastMock).forEach((fn) => fn.mockReset());
  });

  it("shows one toggle per enabled environment with its own active state", () => {
    renderSwitches("b", false);
    const toggles = screen.getAllByRole("button");
    expect(toggles.map((b) => b.textContent)).toEqual(["Windows", "Alpine"]);
    expect(toggles[0]).toHaveAttribute("aria-pressed", "false");
    expect(toggles[1]).toHaveAttribute("aria-pressed", "true");
  });

  it("uses the existing switch for Windows and the per-environment call for WSL", async () => {
    activateMock.mockResolvedValue({
      target: "wsl:Alpine",
      status: "ok",
      at: 1,
    });
    const onMain = renderSwitches("a", false);
    const [windows, alpine] = screen.getAllByRole("button");
    fireEvent.click(windows);
    expect(onMain).toHaveBeenCalledTimes(1);
    expect(activateMock).not.toHaveBeenCalled();

    await act(async () => {
      fireEvent.click(alpine);
    });
    expect(activateMock).toHaveBeenCalledWith("claude", "wsl:Alpine", "a");
    expect(toastMock.success).toHaveBeenCalled();
  });

  it("reports a failed switch and keeps the previous provider", async () => {
    activateMock.mockResolvedValue({
      target: "wsl:Alpine",
      status: "error",
      reason: "write_failed",
      at: 1,
    });
    renderSwitches("a", true);
    const [windows, alpine] = screen.getAllByRole("button");
    fireEvent.click(windows);
    await act(async () => {
      fireEvent.click(alpine);
    });
    expect(toastMock.warning).toHaveBeenCalled();
  });

  it("offers installed but not yet added distros behind a confirmation", async () => {
    activateMock.mockResolvedValue({
      target: "wsl:Ubuntu",
      status: "ok",
      at: 1,
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <ProviderEnvSwitches
          app="claude"
          provider={provider("a")}
          view={view}
          mainActive={false}
          mainAction={{ key: "switch", label: "switch", onClick: vi.fn() }}
          candidates={["Ubuntu"]}
        />
      </QueryClientProvider>,
    );
    const ubuntu = screen.getByRole("button", { name: /Ubuntu/ });
    expect(ubuntu.className).toContain("border-dashed");
    fireEvent.click(ubuntu);
    expect(activateMock).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByText("appTargets.switch.addConfirm"));
    });
    expect(activateMock).toHaveBeenCalledWith("claude", "wsl:Ubuntu", "a");
  });

  it("disables WSL toggles for cards bound to a managed account", () => {
    const client = new QueryClient();
    const bound = {
      ...provider("official"),
      category: "official",
      meta: {
        authBinding: {
          source: "managed_account",
          authProvider: "claude_official",
          accountId: "acct",
        },
      },
    } as Provider;
    render(
      <QueryClientProvider client={client}>
        <ProviderEnvSwitches
          app="claude"
          provider={bound}
          view={view}
          mainActive={false}
          mainAction={{ key: "switch", label: "switch", onClick: vi.fn() }}
        />
      </QueryClientProvider>,
    );
    const [windows, alpine] = screen.getAllByRole("button");
    expect(windows).not.toBeDisabled();
    expect(alpine).toBeDisabled();
  });

  it("lets an active WSL toggle be unchecked after confirming, but not Windows", async () => {
    deactivateMock.mockResolvedValue({
      target: "wsl:Alpine",
      status: "skipped",
      reason: "no_provider",
      at: 2,
    });
    const onMain = renderSwitches("b", true);
    const [windows, alpine] = screen.getAllByRole("button");
    fireEvent.click(windows);
    expect(onMain).not.toHaveBeenCalled();
    expect(deactivateMock).not.toHaveBeenCalled();

    fireEvent.click(alpine);
    expect(deactivateMock).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByText("appTargets.switch.releaseConfirm"));
    });
    expect(deactivateMock).toHaveBeenCalledWith("claude", "wsl:Alpine");
    expect(toastMock.success).toHaveBeenCalled();
  });
});

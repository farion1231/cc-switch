import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyTabContent } from "@/components/settings/ProxyTabContent";
import type { SettingsFormState } from "@/hooks/useSettings";

const exitAppsInMode = vi.hoisted(() => vi.fn());
const panelProps = vi.hoisted(() => ({ current: undefined as any }));
const proxyStatus = vi.hoisted(() => ({
  isRunning: false,
  takeoverStatus: undefined as Record<string, boolean> | undefined,
}));
const stackActive = vi.hoisted(() => ({
  current: {} as Record<string, boolean>,
}));
const queueProps = vi.hoisted(() => ({ current: {} as Record<string, any> }));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    ...proxyStatus,
    startProxyServer: vi.fn(),
    stopWithRestore: vi.fn(),
    exitAppsInMode,
    isPending: false,
  }),
}));

vi.mock("@/components/proxy", () => ({
  ProxyPanel: (props: unknown) => {
    panelProps.current = props;
    return null;
  },
}));

vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: () => null,
}));
vi.mock("@/components/proxy/FailoverQueueManager", () => ({
  FailoverQueueManager: (props: { appType: string }) => {
    queueProps.current[props.appType] = props;
    return null;
  },
}));
vi.mock("@/lib/query/proxy", () => ({
  useProxyStack: (appType: string, enabled: boolean) => ({
    data: enabled
      ? { active: stackActive.current[appType] ?? false, members: [] }
      : undefined,
  }),
}));
vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => null,
}));
vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => null,
}));

async function openProxySection() {
  const onAutoSave = vi.fn().mockResolvedValue(true);
  render(
    <ProxyTabContent
      settings={{ enableLocalProxy: true } as SettingsFormState}
      onAutoSave={onAutoSave}
    />,
  );
  await userEvent
    .setup()
    .click(
      screen.getByRole("button", { name: /settings.advanced.proxy.title/ }),
    );
  await waitFor(() => expect(panelProps.current).toBeDefined());
  return onAutoSave;
}

describe("ProxyTabContent main-page switches", () => {
  beforeEach(() => {
    exitAppsInMode.mockReset();
    panelProps.current = undefined;
    proxyStatus.isRunning = false;
    proxyStatus.takeoverStatus = undefined;
  });

  it("sends apps in routing mode back to direct before turning on Stack mode", async () => {
    exitAppsInMode.mockResolvedValue(["claude"]);
    const onAutoSave = await openProxySection();

    await panelProps.current.onEnableStackModeChange(true);
    await waitFor(() =>
      expect(onAutoSave).toHaveBeenCalledWith({
        enableStackMode: true,
        enableLocalProxy: false,
      }),
    );
    expect(exitAppsInMode).toHaveBeenCalledWith(false);
    expect(exitAppsInMode.mock.invocationCallOrder[0]).toBeLessThan(
      onAutoSave.mock.invocationCallOrder[0],
    );
  });

  it("keeps the setting when the apps cannot go back to direct", async () => {
    exitAppsInMode.mockRejectedValue(new Error("busy"));
    const onAutoSave = await openProxySection();

    await panelProps.current.onEnableLocalProxyChange(true);
    await waitFor(() => expect(exitAppsInMode).toHaveBeenCalledWith(true));
    expect(onAutoSave).not.toHaveBeenCalled();
  });

  it("turning a switch off leaves the apps alone", async () => {
    const onAutoSave = await openProxySection();

    await panelProps.current.onEnableLocalProxyChange(false);
    await waitFor(() =>
      expect(onAutoSave).toHaveBeenCalledWith({ enableLocalProxy: false }),
    );
    expect(exitAppsInMode).not.toHaveBeenCalled();
  });
});

describe("ProxyTabContent failover settings", () => {
  beforeEach(() => {
    proxyStatus.isRunning = true;
    proxyStatus.takeoverStatus = { claude: true };
    stackActive.current = {};
    queueProps.current = {};
  });

  async function openFailoverSection() {
    render(
      <ProxyTabContent
        settings={{ enableFailoverToggle: true } as SettingsFormState}
        onAutoSave={vi.fn()}
      />,
    );
    await userEvent
      .setup()
      .click(
        screen.getByRole("button", {
          name: /settings.advanced.failover.title/,
        }),
      );
    await waitFor(() => expect(queueProps.current.claude).toBeDefined());
  }

  it("can be changed in routing mode", async () => {
    await openFailoverSection();

    expect(queueProps.current.claude.disabled).toBe(false);
    expect(
      screen.queryByText("proxy.stackMode.failoverUnavailable"),
    ).not.toBeInTheDocument();
  });

  it("cannot be changed in Stack mode, which has no failover", async () => {
    stackActive.current = { claude: true };
    await openFailoverSection();

    expect(queueProps.current.claude.disabled).toBe(true);
    expect(
      screen.getByText("proxy.stackMode.failoverUnavailable"),
    ).toBeInTheDocument();
  });
});

import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import type { ComponentProps } from "react";
import { ProxyTabContent } from "@/components/settings/ProxyTabContent";

const tMock = vi.fn((key: string) => key);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: false,
    takeoverStatus: null,
    startProxyServer: vi.fn(),
    stopWithRestore: vi.fn(),
    isPending: false,
  }),
}));

vi.mock("@/components/proxy", () => ({
  ProxyPanel: () => <div data-testid="proxy-panel" />,
}));

// Radix accordion renders closed by default (defaultValue={[]}); flatten it so
// the accordion content under test is always mounted.
vi.mock("@/components/ui/accordion", () => ({
  Accordion: ({ children }: any) => <div>{children}</div>,
  AccordionItem: ({ children }: any) => <div>{children}</div>,
  AccordionTrigger: ({ children }: any) => <div>{children}</div>,
  AccordionContent: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: () => <div />,
}));

vi.mock("@/components/proxy/FailoverQueueManager", () => ({
  FailoverQueueManager: () => <div />,
}));

vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => <div />,
}));

vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => <div />,
}));

const baseSettings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  enableClaudePluginIntegration: false,
  language: "zh" as const,
};

type ProxyTabContentProps = ComponentProps<typeof ProxyTabContent>;

const renderProxyTabContent = (
  overrides: Partial<ProxyTabContentProps> = {},
) => {
  const props: ProxyTabContentProps = {
    settings: { ...baseSettings },
    onAutoSave: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  return { onAutoSave: props.onAutoSave, ...render(<ProxyTabContent {...props} />) };
};

describe("ProxyTabContent autoRestoreOnStartup", () => {
  beforeEach(() => {
    tMock.mockClear();
    tMock.mockImplementation((key: string) => key);
  });

  it("renders the toggle with the setting off by default", () => {
    renderProxyTabContent();

    const toggle = screen.getByRole("switch", {
      name: "settings.advanced.proxy.autoRestoreOnStartup",
    });
    expect(toggle).not.toBeChecked();
    expect(
      screen.queryByText(
        "settings.advanced.proxy.autoRestoreOnStartupNeedsAutostart",
      ),
    ).not.toBeInTheDocument();
  });

  it("calls onAutoSave with the new value when toggled", () => {
    const { onAutoSave } = renderProxyTabContent();

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.advanced.proxy.autoRestoreOnStartup",
      }),
    );

    expect(onAutoSave).toHaveBeenCalledWith({ proxyRestoreOnStartup: true });
  });

  it("shows the autostart hint when enabled without launchOnStartup", () => {
    renderProxyTabContent({
      settings: {
        ...baseSettings,
        proxyRestoreOnStartup: true,
        launchOnStartup: false,
      },
    });

    expect(
      screen.getByText(
        "settings.advanced.proxy.autoRestoreOnStartupNeedsAutostart",
      ),
    ).toBeInTheDocument();

    const toggle = screen.getByRole("switch", {
      name: "settings.advanced.proxy.autoRestoreOnStartup",
    });
    expect(toggle).toBeChecked();
  });

  it("hides the autostart hint when launchOnStartup is on", () => {
    renderProxyTabContent({
      settings: {
        ...baseSettings,
        proxyRestoreOnStartup: true,
        launchOnStartup: true,
      },
    });

    expect(
      screen.queryByText(
        "settings.advanced.proxy.autoRestoreOnStartupNeedsAutostart",
      ),
    ).not.toBeInTheDocument();
  });
});

import { fireEvent, render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeSettings } from "@/components/settings/ThemeSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      typeof values?.mode === "string" ? `${key}:${values.mode}` : key,
  }),
}));

const themeState = vi.hoisted(() => {
  const customPalette = {
    background: "#ffffff",
    foreground: "#09090b",
    card: "#ffffff",
    cardForeground: "#09090b",
    popover: "#ffffff",
    popoverForeground: "#09090b",
    primary: "#1f9cff",
    primaryForeground: "#ffffff",
    secondary: "#f4f4f5",
    secondaryForeground: "#18181b",
    muted: "#f4f4f5",
    mutedForeground: "#71717a",
    accent: "#f4f4f5",
    accentForeground: "#18181b",
    destructive: "#ef4444",
    destructiveForeground: "#fafafa",
    border: "#e4e4e7",
    input: "#e4e4e7",
    ring: "#1f9cff",
  };

  return {
    setTheme: vi.fn(),
    setThemePreset: vi.fn(),
    setCustomThemeColor: vi.fn(),
    setCustomThemeColors: vi.fn(),
    resetCustomTheme: vi.fn(),
    customTheme: {
      light: customPalette,
      dark: customPalette,
    },
  };
});

vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({
    theme: "system",
    themePreset: "custom",
    customTheme: themeState.customTheme,
    setTheme: themeState.setTheme,
    setThemePreset: themeState.setThemePreset,
    setCustomThemeColor: themeState.setCustomThemeColor,
    setCustomThemeColors: themeState.setCustomThemeColors,
    resetCustomTheme: themeState.resetCustomTheme,
  }),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, ...props }: any) => <button {...props}>{children}</button>,
}));

vi.mock("@/components/ui/collapsible", () => ({
  Collapsible: ({ children }: any) => <div>{children}</div>,
  CollapsibleContent: ({ children }: any) => <div>{children}</div>,
  CollapsibleTrigger: ({ children }: any) => <>{children}</>,
}));

vi.mock("@/components/ui/input", () => ({
  Input: (props: any) => <input {...props} />,
}));

vi.mock("@/components/ui/popover", () => ({
  Popover: ({ children }: any) => <div>{children}</div>,
  PopoverTrigger: ({ children }: any) => <>{children}</>,
  PopoverContent: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("@/components/ui/scroll-area", () => ({
  ScrollArea: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("@/components/ui/tooltip", () => ({
  TooltipProvider: ({ children }: any) => <div>{children}</div>,
  Tooltip: ({ children }: any) => <div>{children}</div>,
  TooltipTrigger: ({ children }: any) => <>{children}</>,
  TooltipContent: ({ children }: any) => <div>{children}</div>,
}));

describe("ThemeSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: vi.fn().mockImplementation(() => ({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
  });

  it("labels the custom editor as editing the base palette even in dark preview mode", () => {
    render(<ThemeSettings />);

    expect(
      screen.getByText("settings.themeEditingBasePalette"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "settings.themeCustomPaletteResetBase",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "settings.themeCustomPaletteHintCollapsed:settings.themePaletteDark",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/settings\.themeEditingCurrentMode/),
    ).not.toBeInTheDocument();
  });

  it("uses dedicated label keys for linked foreground and surface fields", () => {
    render(<ThemeSettings />);

    expect(
      screen.getAllByText("settings.themeColorPrimaryForeground").length,
    ).toBeGreaterThan(0);
    expect(
      screen.getAllByText("settings.themeColorPopoverForeground").length,
    ).toBeGreaterThan(0);
    expect(screen.getAllByText("settings.themeColorMuted").length).toBeGreaterThan(
      0,
    );
    expect(
      screen.getAllByText("settings.themeColorDestructiveForeground").length,
    ).toBeGreaterThan(0);
  });

  it("allows partial hex editing and only commits when the value becomes valid", () => {
    render(<ThemeSettings />);

    const input = screen.getByLabelText(
      "settings.themeColorPrimary settings.themeColorHex",
    );

    fireEvent.change(input, { target: { value: "#12" } });
    expect(input).toHaveValue("#12");
    expect(themeState.setCustomThemeColor).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: "123456" } });
    fireEvent.blur(input);

    expect(themeState.setCustomThemeColor).toHaveBeenCalledWith(
      "light",
      "primary",
      "#123456",
    );
  });

  it("adds accessible labels to picker controls and clears drag listeners on unmount", () => {
    const addSpy = vi.spyOn(window, "addEventListener");
    const removeSpy = vi.spyOn(window, "removeEventListener");

    const { unmount } = render(<ThemeSettings />);

    const fieldSlider = screen.getByLabelText(
      "settings.themeColorPrimary settings.themeColorField",
    );
    const hueSlider = screen.getByLabelText(
      "settings.themeColorPrimary settings.themeColorHue",
    );
    const quickPick = screen.getByLabelText(
      "settings.themeColorPrimary #FFFFFF",
    );

    expect(fieldSlider).toHaveAttribute("role", "slider");
    expect(hueSlider).toHaveAttribute("role", "slider");
    expect(quickPick).toBeInTheDocument();

    fireEvent.pointerDown(fieldSlider, { clientX: 12, clientY: 18 });
    unmount();

    expect(
      addSpy.mock.calls.some(([type]) => type === "pointermove"),
    ).toBe(true);
    expect(
      removeSpy.mock.calls.some(([type]) => type === "pointermove"),
    ).toBe(true);
    expect(
      removeSpy.mock.calls.some(([type]) => type === "pointerup"),
    ).toBe(true);
    expect(
      removeSpy.mock.calls.some(([type]) => type === "pointercancel"),
    ).toBe(true);
  });
});

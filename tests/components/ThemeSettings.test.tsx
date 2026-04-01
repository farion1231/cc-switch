import { render, screen } from "@testing-library/react";
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
});

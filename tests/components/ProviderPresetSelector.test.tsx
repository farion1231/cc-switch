import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { describe, expect, it, vi } from "vitest";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values?.defaultValue ?? key,
  }),
}));

vi.mock("@/components/ui/form", () => ({
  FormLabel: ({ children }: { children: React.ReactNode }) => (
    <label>{children}</label>
  ),
}));

vi.mock("@/components/BrandIcons", () => ({
  ClaudeIcon: () => <span>ClaudeIcon</span>,
  CodexIcon: () => <span>CodexIcon</span>,
  GeminiIcon: () => <span>GeminiIcon</span>,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: ({ name }: { name: string }) => <span>{name}</span>,
}));

vi.mock("@/config/universalProviderPresets", () => ({
  universalProviderPresets: [
    {
      providerType: "shared-demo",
      icon: "apps",
      name: "Shared Demo",
    },
  ],
}));

describe("ProviderPresetSelector", () => {
  it("uses shared readable text fallback for selected preset colors", () => {
    render(
      <ProviderPresetSelector
        selectedPresetId="preset-light"
        groupedPresets={{
          official: [
            {
              id: "preset-light",
              preset: {
                name: "Pastel",
                theme: {
                  backgroundColor: "#fff1f7",
                },
              } as any,
            },
          ],
        }}
        categoryKeys={["official"]}
        presetCategoryLabels={{ official: "Official" }}
        onPresetChange={() => {}}
      />,
    );

    const button = screen.getByRole("button", { name: /Pastel/ });
    expect(button.style.backgroundColor).toBe("rgb(255, 241, 247)");
    expect(button.style.color).toBe("rgb(17, 24, 39)");
  });

  it("renders the universal provider badge with a supported primary tint class", () => {
    const { container } = render(
      <ProviderPresetSelector
        selectedPresetId={null}
        groupedPresets={{}}
        categoryKeys={[]}
        presetCategoryLabels={{}}
        onPresetChange={() => {}}
        onUniversalPresetSelect={() => {}}
      />,
    );

    const badge = container.querySelector(".bg-primary\\/10");
    expect(badge).toHaveClass("bg-primary/10");
  });
});

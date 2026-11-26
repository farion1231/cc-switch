import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import type { UniversalProvider } from "@/types";

vi.mock("@/hooks/useDarkMode", () => ({
  useDarkMode: () => false,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => (
    <pre data-testid="json-editor">{value}</pre>
  ),
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => null,
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) =>
    isOpen ? (
      <div>
        {children}
        {footer}
      </div>
    ) : null,
}));

const makeProvider = (id: string, apiKey: string): UniversalProvider => ({
  id,
  name: `Provider ${id}`,
  providerType: "newapi",
  baseUrl: "https://api.example.com",
  apiKey,
  apps: { claude: true, codex: true, gemini: true },
  models: {},
});

describe("UniversalProviderFormModal state reset", () => {
  it("resets showApiKey to password mode when switching providers or reopening", () => {
    const first = makeProvider("p1", "sk-first");
    const second = makeProvider("p2", "sk-second");

    const { rerender } = render(
      <UniversalProviderFormModal
        isOpen
        onClose={vi.fn()}
        onSave={vi.fn()}
        editingProvider={first}
      />,
    );

    const apiKeyInput = () =>
      document.getElementById("apiKey") as HTMLInputElement;
    expect(apiKeyInput()).toHaveAttribute("type", "password");

    const toggleButton = apiKeyInput().parentElement?.querySelector("button");
    expect(toggleButton).not.toBeNull();
    fireEvent.click(toggleButton!);
    expect(apiKeyInput()).toHaveAttribute("type", "text");

    rerender(
      <UniversalProviderFormModal
        isOpen
        onClose={vi.fn()}
        onSave={vi.fn()}
        editingProvider={second}
      />,
    );
    expect(apiKeyInput()).toHaveAttribute("type", "password");
    expect(apiKeyInput()).toHaveValue("sk-second");

    fireEvent.click(apiKeyInput().parentElement!.querySelector("button")!);
    expect(apiKeyInput()).toHaveAttribute("type", "text");

    rerender(
      <UniversalProviderFormModal
        isOpen={false}
        onClose={vi.fn()}
        onSave={vi.fn()}
        editingProvider={second}
      />,
    );
    rerender(
      <UniversalProviderFormModal
        isOpen
        onClose={vi.fn()}
        onSave={vi.fn()}
        editingProvider={second}
      />,
    );
    expect(apiKeyInput()).toHaveAttribute("type", "password");
  });
});

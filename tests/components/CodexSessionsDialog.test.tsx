import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { CodexSessionsDialog } from "@/components/providers/CodexSessionsDialog";

const useProviderCodexSessionsMock = vi.hoisted(() => vi.fn());
const useCodexSessionUsageSummariesMock = vi.hoisted(() => vi.fn());
const useSetCodexSessionProviderLinksMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string; name?: string }) => {
      if (key === "codexSessions.title") {
        return `${options?.name ?? "Codex"} Codex Sessions`;
      }
      return options?.defaultValue ?? key;
    },
  }),
}));

vi.mock("@/lib/query/codexSessions", () => ({
  useProviderCodexSessions: (providerId?: string) =>
    useProviderCodexSessionsMock(providerId),
  useCodexSessionUsageSummaries: () => useCodexSessionUsageSummariesMock(),
  useSetCodexSessionProviderLinks: (providerId: string) =>
    useSetCodexSessionProviderLinksMock(providerId),
}));

function createProvider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: overrides.id ?? "provider-a",
    name: overrides.name ?? "Provider A",
    settingsConfig: overrides.settingsConfig ?? {},
    category: overrides.category,
  };
}

describe("CodexSessionsDialog", () => {
  it("loads provider-scoped sessions and renders empty state", () => {
    const provider = createProvider();
    useProviderCodexSessionsMock.mockReturnValue({
      data: [],
      isLoading: false,
      refetch: vi.fn(),
    });
    useCodexSessionUsageSummariesMock.mockReturnValue({
      data: [],
    });
    useSetCodexSessionProviderLinksMock.mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    });

    render(
      <CodexSessionsDialog
        open
        provider={provider}
        providers={[provider]}
        onOpenChange={vi.fn()}
      />,
    );

    expect(useProviderCodexSessionsMock).toHaveBeenCalledWith("provider-a");
    expect(screen.getByText("Provider A Codex Sessions")).toBeInTheDocument();
    expect(screen.getByText("No Codex sessions found.")).toBeInTheDocument();
  });
});

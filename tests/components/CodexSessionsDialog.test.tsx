import { fireEvent, render, screen } from "@testing-library/react";
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
      if (key === "common.close") {
        return options?.defaultValue ?? "Close";
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

function createSession(overrides: Record<string, unknown> = {}) {
  return {
    session: {
      providerId: "codex",
      sessionId: "session-1",
      title: "Session 1",
      sourcePath: "C:/Users/Test/.codex/sessions/session-1.jsonl",
      projectDir: "C:/project",
      resumeCommand: "codex resume session-1",
      ...overrides,
    },
    linkedProviderIds: [],
    visibleToCurrentProvider: false,
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

  it("exposes a close button that closes the dialog", () => {
    const provider = createProvider();
    const onOpenChange = vi.fn();
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
        onOpenChange={onOpenChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("syncs an unlinked session to all Codex providers for native visibility", () => {
    const providerA = createProvider({ id: "provider-a", name: "Provider A" });
    const providerB = createProvider({ id: "provider-b", name: "Provider B" });
    const mutateAsync = vi.fn();
    useProviderCodexSessionsMock.mockReturnValue({
      data: [createSession()],
      isLoading: false,
      refetch: vi.fn(),
    });
    useCodexSessionUsageSummariesMock.mockReturnValue({
      data: [],
    });
    useSetCodexSessionProviderLinksMock.mockReturnValue({
      mutateAsync,
      isPending: false,
    });

    render(
      <CodexSessionsDialog
        open
        provider={providerA}
        providers={[providerA, providerB]}
        onOpenChange={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Sync visibility" }));

    expect(mutateAsync).toHaveBeenCalledWith({
      sessionId: "session-1",
      sourcePath: "C:/Users/Test/.codex/sessions/session-1.jsonl",
      providerIds: ["provider-a", "provider-b"],
      linkMode: "all",
      syncToCodex: true,
    });
  });

  it("does not run native Codex sync for selected-provider links", () => {
    const providerA = createProvider({ id: "provider-a", name: "Provider A" });
    const providerB = createProvider({ id: "provider-b", name: "Provider B" });
    const mutateAsync = vi.fn();
    useProviderCodexSessionsMock.mockReturnValue({
      data: [
        {
          ...createSession(),
          linkedProviderIds: ["provider-a"],
        },
      ],
      isLoading: false,
      refetch: vi.fn(),
    });
    useCodexSessionUsageSummariesMock.mockReturnValue({
      data: [],
    });
    useSetCodexSessionProviderLinksMock.mockReturnValue({
      mutateAsync,
      isPending: false,
    });

    render(
      <CodexSessionsDialog
        open
        provider={providerA}
        providers={[providerA, providerB]}
        onOpenChange={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Sync visibility" }));

    expect(mutateAsync).toHaveBeenCalledWith({
      sessionId: "session-1",
      sourcePath: "C:/Users/Test/.codex/sessions/session-1.jsonl",
      providerIds: ["provider-a"],
      linkMode: "manual",
      syncToCodex: false,
    });
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AuthSettingsPanel } from "@/components/providers/AuthSettingsPanel";

vi.mock("@/components/settings/AuthCenterPanel", () => ({
  AuthCenterPanel: ({
    authScrollTarget,
  }: {
    authScrollTarget?: string | null;
  }) => <div data-testid="auth-center-panel">{authScrollTarget}</div>,
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
  }: {
    isOpen: boolean;
    children: React.ReactNode;
  }) => (isOpen ? <div>{children}</div> : null),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("AuthSettingsPanel", () => {
  it("invalidates the target app provider list when closed", () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const onClose = vi.fn();

    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <AuthSettingsPanel target="claude_official" onClose={onClose} />
      </QueryClientProvider>,
    );

    expect(invalidateSpy).not.toHaveBeenCalled();

    rerender(
      <QueryClientProvider client={queryClient}>
        <AuthSettingsPanel target={null} onClose={onClose} />
      </QueryClientProvider>,
    );

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["providers", "claude"],
      refetchType: "all",
    });
  });
});

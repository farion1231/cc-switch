import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { Sidebar } from "@/components/shell/Sidebar";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { appPageBelongsTo, parseView } from "@/lib/navigation";
import { promptFileName } from "@/components/prompts/promptUtils";
import { SESSION_APP_IDS } from "@/components/sessions/utils";
import { PROMPT_APP_IDS } from "@/lib/query/prompts";

vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: () => ({ mode: "direct", mapping: false, alert: false }),
    todayCost: 0,
    authNeedsAttention: false,
  }),
}));
vi.mock("@/hooks/useSidebarCollapsed", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/hooks/useSidebarCollapsed")>()),
  useSidebarCollapsed: () => ({ collapsed: false, toggle: vi.fn() }),
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false }),
}));

describe("Copilot navigation in the current shell", () => {
  it("selects each app independently and respects its visibility", () => {
    const onSelectApp = vi.fn();
    const props = {
      activeApp: "copilot-byok" as const,
      view: "providers" as const,
      settingsSection: "general" as const,
      visibleApps: DEFAULT_VISIBLE_APPS,
      onSelectApp,
      onSelectPage: vi.fn(),
      onSelectSettingsSection: vi.fn(),
      onExitSettings: vi.fn(),
    };
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrap = (visibleApps = DEFAULT_VISIBLE_APPS) => (
      <QueryClientProvider client={client}>
        <Sidebar {...props} visibleApps={visibleApps} />
      </QueryClientProvider>
    );
    const view = render(wrap());
    fireEvent.click(screen.getByRole("button", { name: "Copilot CLI" }));
    expect(onSelectApp).toHaveBeenCalledWith("copilot-cli");
    fireEvent.click(screen.getByRole("button", { name: "VS Code Copilot" }));
    expect(onSelectApp).toHaveBeenCalledWith("copilot-byok");
    view.rerender(wrap({ ...DEFAULT_VISIBLE_APPS, "copilot-byok": false }));
    expect(
      screen.queryByRole("button", { name: "VS Code Copilot" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Copilot CLI" }),
    ).toBeInTheDocument();
  });

  it("retains the target page and registers prompts and sessions", () => {
    expect(parseView("copilotTargets")).toBe("copilotTargets");
    expect(appPageBelongsTo("copilotTargets", "copilot-byok")).toBe(true);
    expect(appPageBelongsTo("copilotTargets", "copilot-cli")).toBe(false);
    for (const app of ["copilot-byok", "copilot-cli"] as const) {
      expect(PROMPT_APP_IDS).toContain(app);
      expect(SESSION_APP_IDS).toContain(app);
      expect(promptFileName(app)).toBe("copilot-instructions.md");
    }
  });
});

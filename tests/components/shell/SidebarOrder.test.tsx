import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "@/components/shell/Sidebar";
import { APP_IDS } from "@/config/appConfig";
import type { Settings, VisibleApps } from "@/types";
import { createTestQueryClient } from "../../utils/testQueryClient";

const status = vi.hoisted(() => ({
  todayCost: 47.2,
  todayTokens: 120_400_000,
  todayRequests: 663,
  todayHitRate: 0.986,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { value?: string }) =>
      options?.value != null ? `${key}:${options.value}` : key,
    i18n: { resolvedLanguage: "en" },
  }),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, updateInfo: null }),
}));

vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: () => ({ mode: "direct", alert: false }),
    todayCost: status.todayCost,
    todayTokens: status.todayTokens,
    todayRequests: status.todayRequests,
    todayHitRate: status.todayHitRate,
    authNeedsAttention: false,
  }),
}));

const ordering = vi.hoisted(() => ({
  settings: { appOrder: [] } as Partial<Settings>,
  update: vi.fn(),
  save: vi.fn(),
  drag: null as any,
}));
vi.mock("@/lib/query", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/lib/query")>();
  return {
    ...original,
    useSettingsQuery: () => ({ data: ordering.settings }),
    useSaveSettingsMutation: () => ({ mutateAsync: ordering.save }),
  };
});
vi.mock("@/lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...original,
    providersApi: {
      ...original.providersApi,
      updateTrayMenu: vi.fn().mockResolvedValue(true),
    },
  };
});
vi.mock("@dnd-kit/core", async (importOriginal) => {
  const original = await importOriginal<typeof import("@dnd-kit/core")>();
  return {
    ...original,
    DndContext: ({ children, onDragEnd }: any) => {
      ordering.drag = onDragEnd;
      return children;
    },
  };
});
const renderSidebar = () => {
  const client = createTestQueryClient();
  client.setQueryData(["settings"], ordering.settings);
  const select = vi.fn();
  render(
    <QueryClientProvider client={client}>
      <Sidebar
        activeApp="claude"
        view="providers"
        visibleApps={{ claude: true, codex: true, kimi: true } as VisibleApps}
        settingsSection="general"
        onSelectApp={select}
        onSelectPage={vi.fn()}
        onSelectSettingsSection={vi.fn()}
        onExitSettings={vi.fn()}
      />
    </QueryClientProvider>,
  );
  return select;
};
describe("sidebar app ordering", () => {
  beforeEach(() => {
    ordering.settings = { appOrder: [...APP_IDS] };
    ordering.update.mockReset();
    ordering.save.mockReset().mockResolvedValue({ requiresRestart: false });
    ordering.drag = null;
  });
  it("offers a drag handle without changing normal application navigation", async () => {
    const select = renderSidebar();
    expect(
      screen.getAllByRole("button", { name: "appsPage.dragHandle" }),
    ).toHaveLength(3);
    await userEvent.click(screen.getByText("Kimi Code"));
    expect(select).toHaveBeenCalledWith("kimi");
    expect(ordering.save).not.toHaveBeenCalled();
  });
  it("persists the full order, retaining hidden apps", async () => {
    renderSidebar();
    await act(async () =>
      ordering.drag({ active: { id: "kimi" }, over: { id: "claude" } }),
    );
    const expected = ["kimi", ...APP_IDS.filter((id) => id !== "kimi")];
    expect(ordering.save).toHaveBeenCalledWith({ appOrder: expected });
  });
  it("restores the previous order when saving fails", async () => {
    ordering.save.mockRejectedValue(new Error("disk full"));
    renderSidebar();
    await act(async () =>
      ordering.drag({ active: { id: "kimi" }, over: { id: "claude" } }),
    );
    expect(
      screen
        .getAllByText(/^(Claude Code|Codex|Kimi Code)$/)
        .map((node) => node.textContent),
    ).toEqual(["Claude Code", "Codex", "Kimi Code"]);
    await waitFor(() =>
      expect(
        screen.getAllByRole("button", { name: "appsPage.dragHandle" })[0],
      ).not.toBeDisabled(),
    );
  });
  it("ignores cancelled drops", async () => {
    renderSidebar();
    await act(async () =>
      ordering.drag({ active: { id: "kimi" }, over: null }),
    );
    expect(ordering.save).not.toHaveBeenCalled();
  });
});

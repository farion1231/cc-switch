import { QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "@/components/shell/Sidebar";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { createTestQueryClient } from "../../utils/testQueryClient";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en" },
  }),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, updateInfo: null }),
}));

vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: () => ({ mode: "direct", alert: false }),
    todayCost: 0,
    todayTokens: 0,
    todayRequests: 0,
    todayHitRate: 0,
    authNeedsAttention: false,
  }),
}));

const animate = vi.fn();
const originalAnimate = HTMLElement.prototype.animate;

beforeEach(() => {
  animate.mockReset();
  animate.mockImplementation(() => ({
    pause: vi.fn(),
    play: vi.fn(),
    cancel: vi.fn(),
  }));
  HTMLElement.prototype.animate =
    animate as unknown as typeof HTMLElement.prototype.animate;
});

afterEach(() => {
  HTMLElement.prototype.animate = originalAnimate;
  vi.restoreAllMocks();
});

function renderSidebar(
  view: "providers" | "mcp",
  activeApp: "claude" | "codex",
) {
  const client = createTestQueryClient();
  client.setQueryDefaults(["settings"], { staleTime: Infinity });
  client.setQueryData(["settings"], {});
  return {
    client,
    ...render(
      <QueryClientProvider client={client}>
        <Sidebar
          activeApp={activeApp}
          view={view}
          visibleApps={DEFAULT_VISIBLE_APPS}
          settingsSection="general"
          onSelectApp={vi.fn()}
          onSelectPage={vi.fn()}
          onSelectSettingsSection={vi.fn()}
          onExitSettings={vi.fn()}
        />
      </QueryClientProvider>,
    ),
  };
}

describe("Sidebar selection slide", () => {
  it("clips directory transitions and skips sliding when the previous pill is scrolled out of view", () => {
    const { container, rerender, client } = renderSidebar(
      "providers",
      "claude",
    );
    const scroll = container.querySelector("[data-sidebar-scroll]")!;
    expect(scroll.parentElement?.className).toContain("overflow-hidden");

    const rectOf = (top: number, height = 28): DOMRect =>
      ({
        top,
        bottom: top + height,
        left: 8,
        right: 192,
        width: 184,
        height,
        x: 8,
        y: top,
        toJSON: () => ({}),
      }) as DOMRect;

    vi.spyOn(scroll, "getBoundingClientRect").mockReturnValue(rectOf(44, 200));

    // 旧选中项完全滚到滚动区顶边上方（bottom <= bounds.top）
    const oldPill = container.querySelector("[data-selection-pill]")!;
    vi.spyOn(oldPill, "getBoundingClientRect").mockReturnValue(rectOf(10, 28));

    rerender(
      <QueryClientProvider client={client}>
        <Sidebar
          activeApp="claude"
          view="mcp"
          visibleApps={DEFAULT_VISIBLE_APPS}
          settingsSection="general"
          onSelectApp={vi.fn()}
          onSelectPage={vi.fn()}
          onSelectSettingsSection={vi.fn()}
          onExitSettings={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const newPill = container.querySelector("[data-selection-pill]")!;
    vi.spyOn(newPill, "getBoundingClientRect").mockReturnValue(rectOf(260, 28));
    expect(animate).not.toHaveBeenCalled();
  });

  it("slides when the previous and next pills are both within view", () => {
    const rectOf = (top: number, height = 28): DOMRect =>
      ({
        top,
        bottom: top + height,
        left: 8,
        right: 192,
        width: 184,
        height,
        x: 8,
        y: top,
        toJSON: () => ({}),
      }) as DOMRect;

    const protoRect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect");
    protoRect.mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-sidebar-scroll")) return rectOf(44, 200);
      if (this.hasAttribute("data-selection-pill")) {
        return this.closest("[data-sidebar-scroll]")
          ? rectOf(80, 28)
          : rectOf(260, 28);
      }
      return rectOf(0, 0);
    });

    const { rerender, client } = renderSidebar("providers", "claude");
    animate.mockClear();

    rerender(
      <QueryClientProvider client={client}>
        <Sidebar
          activeApp="claude"
          view="mcp"
          visibleApps={DEFAULT_VISIBLE_APPS}
          settingsSection="general"
          onSelectApp={vi.fn()}
          onSelectPage={vi.fn()}
          onSelectSettingsSection={vi.fn()}
          onExitSettings={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(animate).toHaveBeenCalledTimes(1);
    const keyframes = animate.mock.calls[0][0] as Keyframe[];
    expect(keyframes[0].transform).toContain("translate(0px, -180px)");
  });
});

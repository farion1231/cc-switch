import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  save: vi.fn(),
  settings: undefined as { firstRunNoticeConfirmed?: boolean } | undefined,
}));

vi.mock("@/lib/api", () => ({
  settingsApi: { save: mocks.save },
}));
vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: mocks.settings }),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: async () => "4.0.25",
}));

import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";
import { OnboardingTour, TOUR_STEPS } from "@/components/OnboardingTour";
import { startOnboardingTour } from "@/lib/onboardingTour";

function withClient(ui: React.ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{ui}</QueryClientProvider>;
}

const next = () =>
  fireEvent.click(screen.getByRole("button", { name: "tutorial.next" }));

describe("FirstRunNoticeDialog", () => {
  beforeEach(() => {
    mocks.settings = { firstRunNoticeConfirmed: undefined };
    mocks.save.mockReset().mockResolvedValue(true);
  });

  it("walks a fresh install through the tour and records it once finished", async () => {
    const onBeforeOpen = vi.fn();
    render(withClient(<FirstRunNoticeDialog onBeforeOpen={onBeforeOpen} />));
    expect(screen.getByText("firstRunNotice.title")).toBeInTheDocument();
    // 首次启动不切页面：设置是异步读到的，那时切会把用户刚点开的页面拽回去
    expect(onBeforeOpen).not.toHaveBeenCalled();

    // 第一步只有「跳过」，没有「上一步」
    expect(
      screen.queryByRole("button", { name: "tutorial.back" }),
    ).not.toBeInTheDocument();

    for (const step of TOUR_STEPS.slice(1)) {
      next();
      expect(screen.getByText(step.titleKey)).toBeInTheDocument();
    }

    // 最后一步没有「下一步」，按钮换成「开始使用」
    expect(
      screen.queryByRole("button", { name: "tutorial.next" }),
    ).not.toBeInTheDocument();
    expect(mocks.save).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "tutorial.done" }));
    await waitFor(() =>
      expect(screen.queryByText("tutorial.more.title")).not.toBeInTheDocument(),
    );
    // 新装用户看的就是当前版本，「界面改版了」和更新摘要一并记成已看
    expect(mocks.save).toHaveBeenCalledTimes(1);
    expect(mocks.save).toHaveBeenCalledWith(
      expect.objectContaining({
        firstRunNoticeConfirmed: true,
        newLayoutNoticeConfirmed: true,
        whatsNewSeenVersion: "4.0.25",
      }),
    );
  });

  it("records the tour as seen when skipped", async () => {
    render(withClient(<FirstRunNoticeDialog />));
    fireEvent.click(screen.getByRole("button", { name: "tutorial.skip" }));
    await waitFor(() =>
      expect(mocks.save).toHaveBeenCalledWith(
        expect.objectContaining({ firstRunNoticeConfirmed: true }),
      ),
    );
    expect(screen.queryByText("firstRunNotice.title")).not.toBeInTheDocument();
  });

  it("records the tour as seen when dismissed with Escape", async () => {
    render(withClient(<FirstRunNoticeDialog />));
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("goes back a step", () => {
    render(withClient(<FirstRunNoticeDialog />));
    next();
    fireEvent.click(screen.getByRole("button", { name: "tutorial.back" }));
    expect(screen.getByText("firstRunNotice.title")).toBeInTheDocument();
  });

  it("moves between steps with the arrow keys", () => {
    render(withClient(<FirstRunNoticeDialog />));
    const dialog = screen.getByRole("dialog");
    fireEvent.keyDown(dialog, { key: "ArrowRight" });
    expect(screen.getByText(TOUR_STEPS[1].titleKey)).toBeInTheDocument();
    fireEvent.keyDown(dialog, { key: "ArrowLeft" });
    expect(screen.getByText("firstRunNotice.title")).toBeInTheDocument();
  });

  it("stays hidden once confirmed", () => {
    mocks.settings = { firstRunNoticeConfirmed: true };
    render(withClient(<FirstRunNoticeDialog />));
    expect(screen.queryByText("firstRunNotice.title")).not.toBeInTheDocument();
  });

  it("stays hidden until settings load", () => {
    mocks.settings = undefined;
    render(withClient(<FirstRunNoticeDialog />));
    expect(screen.queryByText("firstRunNotice.title")).not.toBeInTheDocument();
  });

  it("replays on request without touching settings", async () => {
    mocks.settings = { firstRunNoticeConfirmed: true };
    const onBeforeOpen = vi.fn();
    render(withClient(<FirstRunNoticeDialog onBeforeOpen={onBeforeOpen} />));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    act(() => startOnboardingTour());
    expect(screen.getByText("firstRunNotice.title")).toBeInTheDocument();
    expect(onBeforeOpen).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "tutorial.skip" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(mocks.save).not.toHaveBeenCalled();
  });
});

describe("OnboardingTour", () => {
  afterEach(() => {
    document.querySelectorAll("[data-tour]").forEach((el) => el.remove());
  });

  const spotlights = () => screen.queryAllByTestId("tour-spotlight");
  const spotlight = () => {
    const list = spotlights();
    expect(list.length).toBeGreaterThan(0);
    return list[0];
  };

  it("restarts from the first step when reopened", () => {
    const onClose = vi.fn();
    const { rerender } = render(<OnboardingTour open onClose={onClose} />);
    next();
    expect(screen.getByText(TOUR_STEPS[1].titleKey)).toBeInTheDocument();

    rerender(<OnboardingTour open={false} onClose={onClose} />);
    rerender(<OnboardingTour open onClose={onClose} />);
    expect(screen.getByText("firstRunNotice.title")).toBeInTheDocument();
  });

  it("cuts the spotlight out around the anchored element", () => {
    const anchor = document.createElement("div");
    anchor.setAttribute("data-tour", "apps");
    anchor.getBoundingClientRect = () =>
      ({
        top: 50,
        left: 10,
        right: 210,
        bottom: 250,
        width: 200,
        height: 200,
      }) as DOMRect;
    document.body.appendChild(anchor);

    render(<OnboardingTour open onClose={vi.fn()} />);
    // 欢迎页没有要圈的元素：不渲染亮框
    expect(spotlights()).toHaveLength(0);

    next();
    expect(screen.getByText("tutorial.apps.title")).toBeInTheDocument();
    // 四周各外扩 6px
    expect(spotlight().style.top).toBe("44px");
    expect(spotlight().style.left).toBe("4px");
    expect(spotlight().style.width).toBe("212px");
    expect(spotlight().style.height).toBe("212px");
  });

  const addAnchor = (
    id: string,
    rect: Partial<DOMRect>,
    parent: HTMLElement = document.body,
  ) => {
    const el = document.createElement("div");
    el.setAttribute("data-tour", id);
    el.getBoundingClientRect = () =>
      ({
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        width: 0,
        height: 0,
        ...rect,
      }) as DOMRect;
    parent.appendChild(el);
    return el;
  };
  const goTo = (stepId: string) => {
    const target = TOUR_STEPS.findIndex((step) => step.id === stepId);
    for (let i = 0; i < target; i++) next();
  };

  it("drops the previous highlight when the next step has nothing to point at", () => {
    // 加供应商按钮在、模式切换不在（如 OpenCode）：模式那一步不能还圈着按钮
    addAnchor("add-provider", {
      top: 10,
      left: 600,
      right: 700,
      bottom: 40,
      width: 100,
      height: 30,
    });
    render(<OnboardingTour open onClose={vi.fn()} />);
    goTo("add");
    expect(spotlight().style.width).toBe("112px");

    next();
    expect(screen.getByText("tutorial.modes.title")).toBeInTheDocument();
    expect(spotlights()).toHaveLength(0);
    expect(screen.getByText("tutorial.modes.unavailable")).toBeInTheDocument();
  });

  it("opens on the centered welcome step after closing on a highlighted one", () => {
    addAnchor("nav-settings", {
      top: 400,
      left: 10,
      right: 100,
      bottom: 430,
      width: 90,
      height: 30,
    });
    const onClose = vi.fn();
    const { rerender } = render(<OnboardingTour open onClose={onClose} />);
    goTo("more");
    expect(spotlights()).toHaveLength(1);

    rerender(<OnboardingTour open={false} onClose={onClose} />);
    rerender(<OnboardingTour open onClose={onClose} />);
    expect(screen.getByText("firstRunNotice.title")).toBeInTheDocument();
    expect(spotlights()).toHaveLength(0);
  });

  it("points at the first provider card, or the empty list on a fresh install", () => {
    const empty = addAnchor("provider-empty", {
      top: 100,
      left: 220,
      right: 800,
      bottom: 400,
      width: 580,
      height: 300,
    });
    const { unmount } = render(<OnboardingTour open onClose={vi.fn()} />);
    goTo("providers");
    expect(screen.getByText("tutorial.providers.empty")).toBeInTheDocument();
    expect(
      screen.queryByText("tutorial.providers.body"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("firstRunNotice.bodyOfficial"),
    ).not.toBeInTheDocument();
    expect(spotlight().style.top).toBe("94px");
    unmount();
    empty.remove();

    addAnchor("provider-card", {
      top: 120,
      left: 220,
      right: 800,
      bottom: 180,
      width: 580,
      height: 60,
    });
    render(<OnboardingTour open onClose={vi.fn()} />);
    goTo("providers");
    expect(screen.getByText("tutorial.providers.body")).toBeInTheDocument();
    expect(screen.getByText("firstRunNotice.bodyOfficial")).toBeInTheDocument();
    expect(
      screen.queryByText("tutorial.providers.empty"),
    ).not.toBeInTheDocument();
    expect(spotlight().style.top).toBe("114px");
  });

  it("highlights the mode switch when the app has one", () => {
    addAnchor("mode-tabs", {
      top: 60,
      left: 220,
      right: 420,
      bottom: 96,
      width: 200,
      height: 36,
    });
    render(<OnboardingTour open onClose={vi.fn()} />);
    goTo("modes");
    expect(spotlight().style.width).toBe("212px");
    expect(screen.getByText("tutorial.modes.directTitle")).toBeInTheDocument();
    expect(screen.getByText("tutorial.modes.routeTitle")).toBeInTheDocument();
    expect(screen.getByText("tutorial.modes.stackTitle")).toBeInTheDocument();
    expect(
      screen.queryByText("tutorial.modes.unavailable"),
    ).not.toBeInTheDocument();
  });

  it("falls back to a centered card when the anchor is missing", () => {
    render(<OnboardingTour open onClose={vi.fn()} />);
    next();
    expect(screen.getByText("tutorial.apps.title")).toBeInTheDocument();
    expect(spotlights()).toHaveLength(0);
  });

  it("allows closing via the header close button on any step", () => {
    const onClose = vi.fn();
    render(<OnboardingTour open onClose={onClose} />);
    goTo("add");
    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("jumps directly to a step when clicking progress dots", () => {
    render(<OnboardingTour open onClose={vi.fn()} />);
    const dots = screen.getByRole("group").querySelectorAll("button");
    expect(dots).toHaveLength(TOUR_STEPS.length);
    fireEvent.click(dots[3]);
    expect(screen.getByText(TOUR_STEPS[3].titleKey)).toBeInTheDocument();
  });

  it("renders separate spotlight boxes for distinct anchor groups on the last step", () => {
    addAnchor("nav-auth", {
      top: 400,
      left: 8,
      right: 192,
      bottom: 428,
      width: 184,
      height: 28,
    });
    addAnchor("nav-usage", {
      top: 428,
      left: 8,
      right: 192,
      bottom: 456,
      width: 184,
      height: 28,
    });
    addAnchor("nav-settings", {
      top: 470,
      left: 102,
      right: 192,
      bottom: 498,
      width: 90,
      height: 28,
    });

    render(<OnboardingTour open onClose={vi.fn()} />);
    goTo("more");
    const list = spotlights();
    expect(list).toHaveLength(2);
    expect(list[0].style.top).toBe("394px");
    expect(list[0].style.height).toBe("68px");
    expect(list[1].style.left).toBe("96px");
    expect(list[1].style.width).toBe("102px");
  });

  it("clips overflowing anchors to their scrollable ancestor bounds", () => {
    const scroller = document.createElement("div");
    scroller.style.overflowY = "auto";
    scroller.getBoundingClientRect = () =>
      ({
        top: 44,
        left: 0,
        right: 200,
        bottom: 300,
        width: 200,
        height: 256,
      }) as DOMRect;
    document.body.appendChild(scroller);

    addAnchor(
      "apps",
      {
        top: 44,
        left: 8,
        right: 192,
        bottom: 480,
        width: 184,
        height: 436,
      },
      scroller,
    );

    render(<OnboardingTour open onClose={vi.fn()} />);
    next();
    expect(spotlight().style.top).toBe("38px");
    expect(spotlight().style.height).toBe("268px");
    scroller.remove();
  });
});

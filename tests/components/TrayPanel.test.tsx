import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TrayPanel, formatResetIn } from "@/tray-panel/TrayPanel";
import {
  SHOW_EVENT,
  trayPanelApi,
  type PanelPlacement,
  type TrayPanelState,
} from "@/tray-panel/api";
import { emitTauriEvent } from "../msw/tauriMocks";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/tray-panel/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/tray-panel/api")>()),
  trayPanelApi: {
    state: vi.fn(),
    placement: vi.fn(),
    layout: vi.fn().mockResolvedValue(undefined),
    action: vi.fn().mockResolvedValue(undefined),
    hide: vi.fn().mockResolvedValue(undefined),
  },
}));
vi.mock("@/tray-panel/UsageTab", () => ({ UsageTab: () => <div>Usage</div> }));

const state: TrayPanelState = {
  texts: {
    switchProvider: "Switch provider",
    searchPlaceholder: "Search providers",
    inUse: "In use",
    noMatch: "No match",
    openMain: "Open main window",
    quit: "Quit",
    settings: "Settings",
    lightweightMode: "Lightweight mode",
    openPanel: "Open panel",
    configTab: "Configuration",
    usageTab: "Usage",
    resetsIn: "Resets in {when}",
    durationDays: "{n}d",
    durationHours: "{n}h",
    durationMinutes: "{n}m",
    durationSep: " ",
    resetCredits: "{count} resets",
  },
  problems: [],
  feedback: null,
  apps: [
    {
      app: "claude",
      name: "Claude Code",
      current: "default",
      mode: "Direct",
      modeKind: "direct",
      header: null,
      attention: null,
      quota: null,
      addId: null,
      addText: "Add provider",
      rows: [
        {
          kind: "item",
          id: "nav:claude",
          text: "Open Claude Code page",
          enabled: true,
          checked: false,
          provider: null,
        },
      ],
    },
  ],
  lightweight: false,
  quitHint: null,
};

const windows: PanelPlacement = {
  seq: 1,
  platform: "windows",
  scaleFactor: 1,
  side: "left",
  edge: "bottom",
  boundsTop: -1000,
  boundsBottom: 0,
  mainWidth: 320,
  subWidth: 300,
  gap: 0,
  radius: 8,
  layout: "joined",
  glass: "frosted",
};

const frames = new Map<number, FrameRequestCallback>();
let frameId = 0;

async function paint() {
  // 两帧：第一帧完成 DOM 绘制，第二帧才能让原生窗口区域露出新卡片。
  await act(async () => {
    for (let i = 0; i < 2; i++) {
      const pending = [...frames.values()];
      frames.clear();
      pending.forEach((callback) => callback(performance.now()));
    }
  });
}

function renderPanel(client = createTestQueryClient()) {
  return render(<TrayPanel />, {
    wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}

async function openPanel(placement: PanelPlacement) {
  vi.mocked(trayPanelApi.placement).mockResolvedValue(placement);
  const view = renderPanel();
  await screen.findByRole("button", { name: "Claude Code default · Direct" });
  await paint();
  return view;
}

describe("TrayPanel window layout", () => {
  beforeEach(() => {
    frames.clear();
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.set(++frameId, callback);
      return frameId;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
      frames.delete(id);
    });
    window.localStorage.removeItem("cc-switch-tray-tab");
    vi.mocked(trayPanelApi.state).mockResolvedValue(state);
    vi.mocked(trayPanelApi.layout).mockResolvedValue(undefined);
    // jsdom 不排版；分别提供主卡片和较高子卡片的实测高度。
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(
      function (this: HTMLElement) {
        const natural = this.classList.contains("tp-sub") ? 400 : 320;
        return this.style.height
          ? Number.parseFloat(this.style.height)
          : Math.max(natural, Number.parseFloat(this.style.minHeight) || 0);
      },
    );
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        return { top: 0, height: this.offsetHeight } as DOMRect;
      },
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    window.localStorage.removeItem("cc-switch-tray-tab");
    delete document.documentElement.dataset.glass;
    delete document.documentElement.dataset.layout;
  });

  it("colors only the balance amount green, like the provider card", async () => {
    vi.mocked(trayPanelApi.state).mockResolvedValue({
      ...state,
      apps: [
        {
          ...state.apps[0],
          quota: {
            lines: [
              {
                text: "Balance 82.10 CNY",
                left: null,
                warn: false,
                resetsAt: null,
                emphasis: "82.10 CNY",
              },
              {
                text: "Balance -24.96 CNY",
                left: 0,
                warn: true,
                resetsAt: null,
                emphasis: null,
              },
            ],
            note: null,
            resetCredits: null,
          },
        },
      ],
    });
    vi.mocked(trayPanelApi.placement).mockResolvedValue(windows);
    renderPanel();

    const amount = await screen.findByText("82.10 CNY");
    expect(amount).toHaveClass("text-green-600");
    expect(amount.parentElement).toHaveTextContent("Balance 82.10 CNY");
    expect(amount.parentElement).not.toHaveClass("text-green-600");
    expect(screen.getByText("Balance -24.96 CNY")).toHaveClass(
      "text-amber-500",
    );
  });

  it("clips the first Windows frame without waiting for animation frames in a hidden window", async () => {
    vi.mocked(trayPanelApi.placement).mockResolvedValue(windows);
    renderPanel();
    await screen.findByRole("button", { name: "Claude Code default · Direct" });
    expect(trayPanelApi.layout).toHaveBeenCalledWith(
      expect.objectContaining({
        width: 620,
        height: 760,
        cards: [{ x: 300, y: 440, width: 320, height: 320 }],
      }),
    );
    expect(frames.size).toBe(0);
  });

  it("cancels a pending Windows reveal when the provider list is closed before painting", async () => {
    await openPanel(windows);
    const calls = vi.mocked(trayPanelApi.layout).mock.calls.length;
    fireEvent.click(
      screen.getByRole("button", { name: "Claude Code default · Direct" }),
    );
    fireEvent.keyDown(window, { key: "Escape" });
    await paint();
    expect(
      vi
        .mocked(trayPanelApi.layout)
        .mock.calls.slice(calls)
        .every(([layout]) => layout.cards.length === 1),
    ).toBe(true);
  });

  it("keeps the Windows WebView frame fixed while revealing a painted provider list", async () => {
    const { container } = await openPanel(windows);
    const main = container.querySelector<HTMLElement>(".tp-card")!;
    expect(main).toHaveStyle({ left: "300px", top: "440px" });
    const collapsed = vi.mocked(trayPanelApi.layout).mock.lastCall![0];
    const calls = vi.mocked(trayPanelApi.layout).mock.calls.length;

    fireEvent.click(
      screen.getByRole("button", { name: "Claude Code default · Direct" }),
    );
    const sub = container.querySelector<HTMLElement>(".tp-sub")!;
    expect(main).toHaveStyle({
      left: "300px",
      top: "440px",
    });
    expect(main.style.minHeight).toBe("");
    expect(sub).toHaveStyle({ left: "0px", top: "440px", height: "320px" });
    expect(trayPanelApi.layout).toHaveBeenCalledTimes(calls);
    await paint();
    expect(trayPanelApi.layout).toHaveBeenLastCalledWith(
      expect.objectContaining({
        width: collapsed.width,
        height: collapsed.height,
        offsetX: collapsed.offsetX,
        offsetY: collapsed.offsetY,
        cards: [
          { x: 300, y: 440, width: 320, height: 320 },
          { x: 0, y: 440, width: 300, height: 320 },
        ],
      }),
    );

    fireEvent.keyDown(window, { key: "Escape" });
    expect(container.querySelector(".tp-sub")).toBeNull();
    expect(main).toHaveStyle({ left: "300px", top: "440px" });
    await paint();
    expect(trayPanelApi.layout).toHaveBeenLastCalledWith(
      expect.objectContaining({
        width: collapsed.width,
        height: collapsed.height,
        offsetX: collapsed.offsetX,
        offsetY: collapsed.offsetY,
        cards: collapsed.cards,
      }),
    );
  });

  it.each(["windows", "linux"])(
    "keeps %s app rows stationary while switching and filtering different provider counts",
    async (platform) => {
      const codex = {
        ...state.apps[0],
        app: "codex" as const,
        name: "Codex",
        current: "OpenAI",
        rows: Array.from({ length: 20 }, (_, index) => ({
          kind: "provider" as const,
          id: `prov:codex:p${index}`,
          text: `Provider ${index}`,
          enabled: true,
          checked: false,
          provider: {
            name: `Provider ${index}`,
            hint: null,
            icon: null,
            iconColor: null,
            quota: null,
          },
        })),
      };
      vi.mocked(trayPanelApi.state).mockResolvedValue({
        ...state,
        apps: [state.apps[0], codex],
      });
      vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(
        function (this: HTMLElement) {
          const natural = this.classList.contains("tp-sub")
            ? this.textContent?.includes("Codex")
              ? 540
              : 180
            : 320;
          return this.style.height
            ? Number.parseFloat(this.style.height)
            : Math.max(natural, Number.parseFloat(this.style.minHeight) || 0);
        },
      );
      const { container } = await openPanel({
        ...windows,
        platform,
        side: "right",
      });
      const main = container.querySelector<HTMLElement>(".tp-card")!;
      const position = { left: main.style.left, top: main.style.top };
      const initial = vi.mocked(trayPanelApi.layout).mock.lastCall![0];
      const claudeRow = screen.getByRole("button", {
        name: "Claude Code default · Direct",
      });
      const codexRow = screen.getByRole("button", {
        name: "Codex OpenAI · Direct",
      });

      fireEvent.click(claudeRow);
      await paint();
      expect(main).toHaveStyle(position);
      expect(container.querySelector(".tp-sub")).toHaveStyle({
        height: "320px",
      });

      fireEvent.mouseEnter(codexRow);
      await paint();
      expect(main).toHaveStyle(position);
      expect(container.querySelector(".tp-sub")).toHaveStyle({
        height: "320px",
      });
      expect(
        screen.getByRole("button", { name: /Provider 19$/ }),
      ).toBeVisible();

      fireEvent.change(screen.getByPlaceholderText("Search providers"), {
        target: { value: "does not exist" },
      });
      await paint();
      expect(screen.getByText("No match")).toBeVisible();
      expect(main).toHaveStyle(position);
      expect(container.querySelector(".tp-sub")).toHaveStyle({
        height: "320px",
      });
      expect(trayPanelApi.layout).toHaveBeenLastCalledWith(
        expect.objectContaining({
          cards: [
            initial.cards[0],
            { x: 320, y: initial.cards[0].y, width: 300, height: 320 },
          ],
        }),
      );

      fireEvent.mouseEnter(claudeRow);
      fireEvent.keyDown(window, { key: "Escape" });
      await paint();
      expect(main).toHaveStyle(position);
      expect(container.querySelector(".tp-sub")).toBeNull();
    },
  );

  it("re-reads settings every time it pops up so changes made in the main window show", async () => {
    const client = createTestQueryClient();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    vi.mocked(trayPanelApi.placement).mockResolvedValue(windows);
    renderPanel(client);
    await screen.findByRole("button", { name: "Claude Code default · Direct" });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["settings"] });

    invalidate.mockClear();
    await act(async () => {
      emitTauriEvent(SHOW_EVENT, { ...windows, seq: 2 });
    });
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ["settings"] }),
    );
  });

  it.each([1, 1.25, 1.5, 2])(
    "keeps native bounds consistent at %s display scaling with larger WebView text",
    async (scaleFactor) => {
      const ratio = vi.spyOn(window, "devicePixelRatio", "get");
      ratio.mockReturnValue(scaleFactor * 1.1);
      const { container } = await openPanel({ ...windows, scaleFactor });
      const main = container.querySelector<HTMLElement>(".tp-card")!;
      expect(Number.parseFloat(main.style.width)).toBeCloseTo(320 / 1.1);
      expect(Number.parseFloat(main.style.maxHeight)).toBeCloseTo(620 / 1.1);
      const scaled = vi.mocked(trayPanelApi.layout).mock.lastCall![0];
      expect(scaled.width).toBeCloseTo(620);
      expect(scaled.height).toBe(760);
      expect(scaled.offsetX).toBeCloseTo(300);
      expect(scaled.cards[0].width).toBeCloseTo(320);
      expect(scaled.cards[0].height).toBeCloseTo(352);

      fireEvent.click(
        screen.getByRole("button", { name: "Claude Code default · Direct" }),
      );
      await paint();
      const expanded = vi.mocked(trayPanelApi.layout).mock.lastCall![0];
      expect(expanded.cards[0]).toEqual(scaled.cards[0]);
      expect(expanded.cards[1].width).toBeCloseTo(300);
      expect(expanded.cards[1].height).toBeCloseTo(352);

      // 跨屏 / 缩放变化后，同一常驻 WebView 要重新量尺寸和裁剪区域。
      ratio.mockReturnValue(scaleFactor);
      fireEvent.resize(window);
      await paint();
      expect(main).toHaveStyle({ width: "320px", maxHeight: "620px" });
      expect(trayPanelApi.layout).toHaveBeenLastCalledWith(
        expect.objectContaining({
          cards: [
            { x: 300, y: 440, width: 320, height: 320 },
            { x: 0, y: 440, width: 300, height: 320 },
          ],
        }),
      );
    },
  );

  it.each([
    [1, 1.5, 2],
    [2, 1.5, 1],
  ])(
    "updates clipping without resize when DPR arrives after show (%s → %s → %s)",
    async (initialScale, secondScale, thirdScale) => {
      const ratio = vi.spyOn(window, "devicePixelRatio", "get");
      ratio.mockReturnValue(initialScale);
      const mediaQueries: Array<
        EventTarget & { media: string; matches: boolean }
      > = [];
      vi.stubGlobal("matchMedia", (query: string) => {
        const media = Object.assign(new EventTarget(), {
          media: query,
          matches: query === `(resolution: ${window.devicePixelRatio}dppx)`,
        });
        mediaQueries.push(media);
        return media as MediaQueryList;
      });
      const { container } = await openPanel({
        ...windows,
        scaleFactor: initialScale,
      });
      const main = container.querySelector<HTMLElement>(".tp-card")!;

      for (const [index, scaleFactor] of [secondScale, thirdScale].entries()) {
        const seq = index + 2;
        // 原生窗口收到布局后才跨屏：显示事件仍读到旧屏的 DPR。
        await act(async () => {
          emitTauriEvent(SHOW_EVENT, { ...windows, seq, scaleFactor });
        });
        await paint();
        const calls = vi.mocked(trayPanelApi.layout).mock.calls.length;

        // 画布尺寸不变，没有 resize；只分发匹配状态确实改变的媒体查询事件。
        await act(async () => {
          ratio.mockReturnValue(scaleFactor);
          for (const media of [...mediaQueries]) {
            const matches = media.media === `(resolution: ${scaleFactor}dppx)`;
            if (media.matches !== matches) {
              media.matches = matches;
              media.dispatchEvent(new Event("change"));
            }
          }
        });
        await paint();

        expect(main).toHaveStyle({ width: "320px", maxHeight: "620px" });
        expect(trayPanelApi.layout).toHaveBeenCalledTimes(calls + 1);
        expect(trayPanelApi.layout).toHaveBeenLastCalledWith(
          expect.objectContaining({
            seq,
            width: 620,
            height: 760,
            cards: [{ x: 300, y: 440, width: 320, height: 320 }],
          }),
        );
      }
    },
  );

  it("limits both cards to the available height on small scaled displays", async () => {
    const { container } = await openPanel({ ...windows, boundsTop: -180 });
    fireEvent.click(
      screen.getByRole("button", { name: "Claude Code default · Direct" }),
    );
    expect(container.querySelector(".tp-card")).toHaveStyle({
      maxHeight: "180px",
    });
    expect(container.querySelector(".tp-sub")).toHaveStyle({
      maxHeight: "180px",
    });
  });

  it.each([
    ["direct", "Direct", "text-direct-text"],
    ["route", "Route", "text-route-text"],
    ["stack", "Aggregation", "text-stack-text"],
  ] as const)(
    "colors the %s mode name with its theme color",
    async (modeKind, mode, className) => {
      vi.mocked(trayPanelApi.state).mockResolvedValue({
        ...state,
        apps: [{ ...state.apps[0], mode, modeKind }],
      });
      vi.mocked(trayPanelApi.placement).mockResolvedValue(windows);
      renderPanel();
      await screen.findByRole("button", {
        name: `Claude Code default · ${mode}`,
      });
      expect(screen.getByText(mode)).toHaveClass(className);
    },
  );

  it("opens Windows top/left taskbars toward the opposite edges", async () => {
    const { container } = await openPanel(windows);
    await act(async () => {
      emitTauriEvent(SHOW_EVENT, {
        ...windows,
        seq: 2,
        side: "right",
        edge: "top",
      });
    });
    const main = container.querySelector<HTMLElement>(".tp-card")!;
    await waitFor(() => expect(main).toHaveStyle({ left: "0px", top: "0px" }));
    expect(main.style.right).toBe("");
    expect(main.style.bottom).toBe("");
    fireEvent.click(
      screen.getByRole("button", { name: "Claude Code default · Direct" }),
    );
    expect(container.querySelector(".tp-sub")).toHaveStyle({
      left: "320px",
      top: "0px",
    });
  });

  it.each([
    {
      side: "left",
      edge: "top",
      mainX: 324,
      mainY: 16,
      subX: 16,
      subY: 70,
      offsetY: -16,
    },
    {
      side: "right",
      edge: "top",
      mainX: 16,
      mainY: 16,
      subX: 344,
      subY: 70,
      offsetY: -16,
    },
    {
      side: "left",
      edge: "bottom",
      mainX: 324,
      mainY: 456,
      subX: 16,
      subY: 376,
      offsetY: -776,
    },
    {
      side: "right",
      edge: "bottom",
      mainX: 16,
      mainY: 456,
      subX: 344,
      subY: 376,
      offsetY: -776,
    },
  ] as const)(
    "preserves macOS floating layout for $side/$edge expansion and collapse",
    async ({ side, edge, mainX, mainY, subX, subY, offsetY }) => {
      vi.spyOn(
        HTMLElement.prototype,
        "getBoundingClientRect",
      ).mockImplementation(function (this: HTMLElement) {
        return {
          top: this.dataset.appRow ? 110 : 50,
          height: this.offsetHeight,
        } as DOMRect;
      });
      const { container } = await openPanel({
        ...windows,
        platform: "macos",
        layout: "floating",
        glass: "native",
        side,
        edge,
        gap: 8,
        radius: 14,
        boundsTop: edge === "top" ? 0 : -900,
        boundsBottom: edge === "top" ? 900 : 0,
      });
      const main = container.querySelector<HTMLElement>(".tp-card")!;
      expect(main).toHaveStyle({ left: `${mainX}px`, top: `${mainY}px` });
      const collapsed = vi.mocked(trayPanelApi.layout).mock.lastCall![0];
      expect(collapsed).toEqual(
        expect.objectContaining({
          width: 660,
          height: 792,
          offsetX: mainX,
          offsetY,
        }),
      );

      fireEvent.click(
        screen.getByRole("button", { name: "Claude Code default · Direct" }),
      );
      expect(main).toHaveStyle({ left: `${mainX}px`, top: `${mainY}px` });
      expect(main.style.minHeight).toBe("");
      expect(main.style.right).toBe("");
      expect(main.style.bottom).toBe("");
      expect(container.querySelector(".tp-sub")).toHaveStyle({
        left: `${subX}px`,
        top: `${subY}px`,
      });
      expect(trayPanelApi.layout).toHaveBeenLastCalledWith(
        expect.objectContaining({
          width: collapsed.width,
          height: collapsed.height,
          offsetX: collapsed.offsetX,
          offsetY: collapsed.offsetY,
          cards: [
            { x: mainX, y: mainY, width: 320, height: 320 },
            { x: subX, y: subY, width: 300, height: 400 },
          ],
        }),
      );
      fireEvent.keyDown(window, { key: "Escape" });
      expect(container.querySelector(".tp-sub")).toBeNull();
      expect(trayPanelApi.layout).toHaveBeenLastCalledWith(collapsed);
      expect(window.requestAnimationFrame).not.toHaveBeenCalled();
      expect(window.cancelAnimationFrame).not.toHaveBeenCalled();
    },
  );

  it("switches apps at once on a vertical move and waits while heading for the sub panel", async () => {
    const codex = {
      ...state.apps[0],
      app: "codex" as const,
      name: "Codex",
      current: "OpenAI",
      rows: [
        { ...state.apps[0].rows[0], id: "nav:codex", text: "Open Codex page" },
      ],
    };
    vi.mocked(trayPanelApi.state).mockResolvedValue({
      ...state,
      apps: [state.apps[0], codex],
    });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        return {
          ...(this.classList.contains("tp-sub")
            ? { top: 0, bottom: 400, left: 344, right: 644 }
            : { top: 50, bottom: 100, left: 16, right: 336 }),
          height: this.offsetHeight,
        } as DOMRect;
      },
    );
    const { container } = await openPanel({
      ...windows,
      platform: "macos",
      layout: "floating",
      glass: "native",
      side: "right",
      edge: "top",
      gap: 8,
      boundsTop: 0,
      boundsBottom: 900,
    });
    const subTitle = () =>
      container.querySelector(".tp-sub")?.firstElementChild?.textContent;
    const claudeRow = screen.getByRole("button", {
      name: "Claude Code default · Direct",
    });
    const codexRow = screen.getByRole("button", {
      name: "Codex OpenAI · Direct",
    });

    fireEvent.mouseEnter(claudeRow);
    await waitFor(() => expect(subTitle()).toContain("Claude Code"));
    // 刚展开：卡片自己有展开动画，内容不另做淡入
    expect(container.querySelector(".tp-sub")).not.toHaveAttribute("data-swap");

    // 竖着移到下一行：不等，立即换
    fireEvent.mouseMove(window, { clientX: 200, clientY: 80 });
    fireEvent.mouseMove(window, { clientX: 200, clientY: 120 });
    fireEvent.mouseLeave(claudeRow);
    fireEvent.mouseEnter(codexRow);
    expect(subTitle()).toContain("Codex");
    // 从另一个应用换过来：内容做一次短淡入
    expect(container.querySelector(".tp-sub")).toHaveAttribute("data-swap");

    // 斜着朝子面板移动、途经 Claude 那行：先不换；鼠标停下后再换
    fireEvent.mouseMove(window, { clientX: 200, clientY: 120 });
    fireEvent.mouseMove(window, { clientX: 230, clientY: 110 });
    fireEvent.mouseLeave(codexRow);
    fireEvent.mouseEnter(claudeRow);
    expect(subTitle()).toContain("Codex");
    await waitFor(() => expect(subTitle()).toContain("Claude Code"));

    // 移出应用行（比如移出整个面板）：子面板留着
    fireEvent.mouseLeave(claudeRow);
    expect(container.querySelector(".tp-sub")).not.toBeNull();

    // 竖着移到底部「打开主窗口」：和原生菜单一样收起子面板
    fireEvent.mouseMove(window, { clientX: 200, clientY: 300 });
    fireEvent.mouseMove(window, { clientX: 200, clientY: 340 });
    fireEvent.mouseOver(
      screen.getByRole("button", { name: "Open main window" }),
    );
    expect(container.querySelector(".tp-sub")).toBeNull();
  });
});

describe("formatResetIn", () => {
  const now = new Date("2026-10-20T10:00:00Z");
  const at = (ms: number) => new Date(now.getTime() + ms).toISOString();
  const MIN = 60_000;

  it("keeps at most two units and drops a zero second unit", () => {
    expect(formatResetIn(at((47 * 60 + 30) * MIN), now, state.texts)).toBe(
      "Resets in 1d 23h",
    );
    expect(formatResetIn(at(2 * 1440 * MIN), now, state.texts)).toBe(
      "Resets in 2d",
    );
    expect(formatResetIn(at((4 * 60 + 5) * MIN), now, state.texts)).toBe(
      "Resets in 4h 5m",
    );
    expect(formatResetIn(at(3 * 60 * MIN), now, state.texts)).toBe(
      "Resets in 3h",
    );
    expect(formatResetIn(at(12 * MIN), now, state.texts)).toBe("Resets in 12m");
  });

  it("rounds sub-minute waits up and hides past or invalid times", () => {
    expect(formatResetIn(at(20_000), now, state.texts)).toBe("Resets in 1m");
    expect(formatResetIn(at(-MIN), now, state.texts)).toBeNull();
    expect(formatResetIn("not a date", now, state.texts)).toBeNull();
  });
});

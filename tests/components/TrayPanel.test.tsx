import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TrayPanel } from "@/tray-panel/TrayPanel";
import {
  SHOW_EVENT,
  trayPanelApi,
  type PanelPlacement,
  type TrayPanelState,
} from "@/tray-panel/api";
import { emitTauriEvent } from "../msw/tauriMocks";

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
    resetsAt: "Resets {when}",
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

async function openPanel(placement: PanelPlacement) {
  vi.mocked(trayPanelApi.placement).mockResolvedValue(placement);
  const view = render(<TrayPanel />);
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
        return this.classList.contains("tp-sub") ? 400 : 320;
      },
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
    window.localStorage.removeItem("cc-switch-tray-tab");
    delete document.documentElement.dataset.glass;
    delete document.documentElement.dataset.layout;
  });

  it("clips the first Windows frame without waiting for animation frames in a hidden window", async () => {
    vi.mocked(trayPanelApi.placement).mockResolvedValue(windows);
    render(<TrayPanel />);
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
      top: "360px",
      minHeight: "400px",
    });
    expect(sub).toHaveStyle({ left: "0px", top: "360px" });
    expect(trayPanelApi.layout).toHaveBeenCalledTimes(calls);
    await paint();
    expect(trayPanelApi.layout).toHaveBeenLastCalledWith(
      expect.objectContaining({
        width: collapsed.width,
        height: collapsed.height,
        offsetX: collapsed.offsetX,
        offsetY: collapsed.offsetY,
        cards: [
          { x: 300, y: 360, width: 320, height: 400 },
          { x: 0, y: 360, width: 300, height: 400 },
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
        return { top: this.dataset.appRow ? 110 : 50 } as DOMRect;
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
});

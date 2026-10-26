import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "@/lib/api/types";
import type { AppMode } from "@/types/proxy";

/** 托盘面板的数据（后端 `tray_panel::TrayPanelState`）。 */
export interface PanelTexts {
  switchProvider: string;
  searchPlaceholder: string;
  inUse: string;
  noMatch: string;
  openMain: string;
  quit: string;
  settings: string;
  lightweightMode: string;
  openPanel: string;
  configTab: string;
  usageTab: string;
  /** 某一档还有多久重置：`{when}` 是下面几个单位拼出的时长 */
  resetsIn: string;
  /** 时长单位：`{n}` 是数字；最多取两级，中间用 `durationSep` 隔开 */
  durationDays: string;
  durationHours: string;
  durationMinutes: string;
  durationSep: string;
  /** 可用的额度重置次数：`{count}` */
  resetCredits: string;
}

export interface PanelLink {
  id: string;
  text: string;
}

export interface PanelQuotaLine {
  text: string;
  /** 剩余百分比；余额这种没有总额的为空 */
  left: number | null;
  warn: boolean;
  /** 这一档什么时候重置（ISO 8601） */
  resetsAt: string | null;
  /** `text` 里要上绿色的那段数值（还有余额时的金额） */
  emphasis: string | null;
}

export interface PanelQuota {
  lines: PanelQuotaLine[];
  note: string | null;
  /** ChatGPT 订阅（Codex）可用的额度重置次数 */
  resetCredits: number | null;
}

export interface PanelProvider {
  name: string;
  hint: string | null;
  icon: string | null;
  iconColor: string | null;
  quota: PanelQuota | null;
}

export interface PanelRow {
  kind: "provider" | "check" | "item" | "label" | "separator";
  id: string;
  text: string;
  enabled: boolean;
  checked: boolean;
  provider: PanelProvider | null;
}

export interface PanelApp {
  app: AppId;
  name: string;
  current: string | null;
  mode: string;
  modeKind: AppMode;
  header: string | null;
  attention: string | null;
  quota: PanelQuota | null;
  addId: string | null;
  addText: string;
  rows: PanelRow[];
}

export interface TrayPanelState {
  texts: PanelTexts;
  problems: PanelLink[];
  feedback: string | null;
  apps: PanelApp[];
  lightweight: boolean;
  quitHint: string | null;
}

/** 这次弹出的方位（后端 `tray_panel::PanelPlacement`），坐标为逻辑像素。 */
export interface PanelPlacement {
  seq: number;
  platform: string;
  /** 图标所在显示器的 DPI 缩放；可能与 WebView 的 devicePixelRatio 不同。 */
  scaleFactor: number;
  side: "left" | "right";
  edge: "top" | "bottom";
  boundsTop: number;
  boundsBottom: number;
  mainWidth: number;
  subWidth: number;
  gap: number;
  radius: number;
  layout: "floating" | "joined";
  glass: "native" | "frosted";
}

export interface CardRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface PanelLayout {
  seq: number;
  width: number;
  height: number;
  offsetX: number;
  offsetY: number;
  cards: CardRect[];
  dark: boolean | null;
}

export const SHOW_EVENT = "tray-panel-show";
export const CHANGED_EVENT = "tray-panel-changed";

export const trayPanelApi = {
  state: () => invoke<TrayPanelState>("tray_panel_state"),
  placement: () => invoke<PanelPlacement | null>("tray_panel_placement"),
  layout: (layout: PanelLayout) =>
    invoke<void>("tray_panel_layout", { layout }),
  hide: () => invoke<void>("tray_panel_hide"),
  action: (id: string) => invoke<void>("tray_panel_action", { id }),
};

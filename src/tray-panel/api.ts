import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "@/lib/api/types";

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
  /** 某一档的重置时间：`{when}` */
  resetsAt: string;
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

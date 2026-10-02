import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppId } from "@/lib/api/types";
import { APP_IDS } from "@/config/appConfig";
import { SETTINGS_SECTIONS, type SettingsSection } from "@/lib/navigation";

/** 托盘让主界面去的地方（后端 `tray::TrayNavigation`）。 */
export interface TrayNavigation {
  app?: AppId;
  section?: SettingsSection;
  /** `needsRoute`：弹「需要路由」对话框；`add`：开始添加供应商 */
  intent?: "needsRoute" | "add";
  providerId?: string;
}

interface RawTrayNavigation {
  app?: string;
  section?: string;
  intent?: string;
  providerId?: string;
}

export function parseTrayNavigation(
  raw: RawTrayNavigation | null | undefined,
): TrayNavigation | null {
  if (!raw) return null;
  const section = SETTINGS_SECTIONS.find((s) => s === raw.section);
  if (section) return { section };
  const app = APP_IDS.find((id) => id === raw.app);
  if (!app) return null;
  const intent =
    raw.intent === "needsRoute" || raw.intent === "add"
      ? raw.intent
      : undefined;
  if (intent === "needsRoute" && !raw.providerId) return { app };
  return { app, intent, providerId: raw.providerId };
}

/**
 * 托盘里点「打开 X 页面」「（需要路由）…」「添加供应商…」和问题区：后端先打开主窗口，留下一条
 * 导航再发 `tray-navigate`。轻量模式下窗口是新建的，事件可能早于监听，所以挂载时也取一次。
 */
export function useTrayNavigation(
  onNavigate: (navigation: TrayNavigation) => void,
): void {
  const handlerRef = useRef(onNavigate);
  handlerRef.current = onNavigate;

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    const take = async () => {
      try {
        const raw = await invoke<RawTrayNavigation | null>(
          "take_tray_navigation",
        );
        const navigation = parseTrayNavigation(raw);
        if (navigation && !disposed) handlerRef.current(navigation);
      } catch (error) {
        console.error("Failed to take tray navigation", error);
      }
    };

    void (async () => {
      try {
        const off = await listen("tray-navigate", () => void take());
        if (disposed) {
          off();
          return;
        }
        unlisten = off;
      } catch (error) {
        console.error("Failed to subscribe tray-navigate event", error);
      }
      await take();
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}

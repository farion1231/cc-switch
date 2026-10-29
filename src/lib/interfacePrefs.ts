import { useSyncExternalStore } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

/**
 * 界面密度与缩放（设置 › 通用 › 外观）。只影响主窗口，存在 localStorage 里，和明暗一样不进后端设置。
 *
 * - 密度：写到 <html data-density>，src/index.css 按它换一组字号 / 控件高度变量。
 *   main.tsx 在渲染任何界面之前先调 initializeInterfacePrefs。不放进 index.html 的内联脚本：
 *   tauri.conf.json 的 CSP 只按哈希放行明暗那一段内联脚本，新加的会被拦。
 * - 缩放：交给 webview 原生缩放（整页等比，弹层定位不受影响）；Ctrl / ⌘ + = − 0 也能调。
 */

export type Density = "comfortable" | "compact";

export const DENSITY_STORAGE_KEY = "cc-switch-density";
export const ZOOM_STORAGE_KEY = "cc-switch-zoom";
export const DEFAULT_DENSITY: Density = "comfortable";
export const ZOOM_LEVELS = [0.9, 1, 1.1, 1.25] as const;
export type ZoomLevel = (typeof ZOOM_LEVELS)[number];

interface InterfacePrefs {
  density: Density;
  zoom: ZoomLevel;
}

function readDensity(): Density {
  try {
    const saved = localStorage.getItem(DENSITY_STORAGE_KEY);
    if (saved === "comfortable" || saved === "compact") return saved;
  } catch {
    // 读不到就用默认
  }
  return DEFAULT_DENSITY;
}

function readZoom(): ZoomLevel {
  try {
    const saved = Number(localStorage.getItem(ZOOM_STORAGE_KEY));
    const level = ZOOM_LEVELS.find((z) => z === saved);
    if (level) return level;
  } catch {
    // 读不到就用默认
  }
  return 1;
}

let prefs: InterfacePrefs = { density: readDensity(), zoom: readZoom() };
const listeners = new Set<() => void>();

function emit(next: InterfacePrefs) {
  prefs = next;
  listeners.forEach((listener) => listener());
}

function applyZoom(zoom: ZoomLevel) {
  try {
    void getCurrentWebview()
      .setZoom(zoom)
      .catch(() => {
        // 不在 Tauri 里（测试、纯浏览器预览）时没有 webview
      });
  } catch {
    // 同上
  }
}

export function setDensity(density: Density) {
  document.documentElement.dataset.density = density;
  try {
    localStorage.setItem(DENSITY_STORAGE_KEY, density);
  } catch {
    // 记不住也不影响这次切换
  }
  emit({ ...prefs, density });
}

export function setZoom(zoom: ZoomLevel) {
  applyZoom(zoom);
  try {
    localStorage.setItem(ZOOM_STORAGE_KEY, String(zoom));
  } catch {
    // 记不住也不影响这次切换
  }
  emit({ ...prefs, zoom });
}

/** 按 ZOOM_LEVELS 的档位放大 / 缩小一档，到头不动。 */
export function stepZoom(direction: 1 | -1) {
  const index = ZOOM_LEVELS.indexOf(prefs.zoom);
  const next = ZOOM_LEVELS[index + direction];
  if (next !== undefined) setZoom(next);
}

/** 主窗口启动时调一次：把记住的密度和缩放用上。 */
export function initializeInterfacePrefs() {
  document.documentElement.dataset.density = prefs.density;
  if (prefs.zoom !== 1) applyZoom(prefs.zoom);
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useInterfacePrefs(): InterfacePrefs {
  return useSyncExternalStore(subscribe, () => prefs);
}

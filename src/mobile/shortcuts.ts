/**
 * User-customizable bottom shortcut bar model & localStorage persistence.
 *
 * Design rules:
 * - Total mobile views: 6 (`overview`, `providers`, `logs`, `proxy`, `models`, `preferences`).
 * - Bottom bar holds at most 5 slots total (iOS tab bar ergonomics on 360px+ phones).
 * - Whenever any view is unpinned (`pinned.length < totalViews`), the rightmost slot is
 *   reserved for the "More" button, so at most 4 views can be pinned simultaneously
 *   (4 pinned + 1 More = 5 slots). When `totalViews <= 5`, up to 5 views may be pinned.
 * - At least 1 view must remain pinned at all times.
 */

export type MobileViewId =
  | "overview"
  | "providers"
  | "logs"
  | "proxy"
  | "models"
  | "preferences";

export interface MobileViewMeta {
  id: MobileViewId;
  labelEn: string;
  labelZh: string;
  shortEn: string;
  shortZh: string;
}

export const ALL_MOBILE_VIEWS: readonly MobileViewMeta[] = [
  {
    id: "overview",
    labelEn: "Overview",
    labelZh: "概览",
    shortEn: "Overview",
    shortZh: "概览",
  },
  {
    id: "providers",
    labelEn: "Providers",
    labelZh: "供应商",
    shortEn: "Providers",
    shortZh: "供应商",
  },
  {
    id: "logs",
    labelEn: "Request Logs",
    labelZh: "请求日志",
    shortEn: "Logs",
    shortZh: "日志",
  },
  {
    id: "proxy",
    labelEn: "Local Proxy",
    labelZh: "本地代理",
    shortEn: "Proxy",
    shortZh: "代理",
  },
  {
    id: "models",
    labelEn: "Models & Breakdown",
    labelZh: "模型与明细",
    shortEn: "Models",
    shortZh: "模型",
  },
  {
    id: "preferences",
    labelEn: "Preferences",
    labelZh: "偏好设置",
    shortEn: "Prefs",
    shortZh: "设置",
  },
] as const;

export const DEFAULT_PINNED_VIEWS: readonly MobileViewId[] = [
  "overview",
  "providers",
  "logs",
  "proxy",
] as const;

export const SHORTCUTS_STORAGE_KEY = "cc-switch-mobile-shortcuts-v1";
export const MIN_PINNED_SHORTCUTS = 1;
export const MAX_BOTTOM_BAR_SLOTS = 5;

const VALID_VIEW_IDS = new Set<string>(ALL_MOBILE_VIEWS.map((v) => v.id));

/**
 * Maximum number of views that may be pinned simultaneously for a given view catalog size.
 * Reserves 1 slot for "More" whenever not all views can fit directly in 5 slots.
 */
export function maxPinnedShortcuts(
  totalViews: number = ALL_MOBILE_VIEWS.length,
): number {
  if (totalViews <= MAX_BOTTOM_BAR_SLOTS) {
    return Math.max(MIN_PINNED_SHORTCUTS, totalViews);
  }
  return MAX_BOTTOM_BAR_SLOTS - 1;
}

export function isMobileViewId(value: unknown): value is MobileViewId {
  return typeof value === "string" && VALID_VIEW_IDS.has(value);
}

/**
 * Validate and normalize an arbitrary persisted value into a safe pinned view list.
 */
export function normalizePinnedShortcuts(raw: unknown): MobileViewId[] {
  if (!Array.isArray(raw)) {
    return [...DEFAULT_PINNED_VIEWS];
  }
  const max = maxPinnedShortcuts();
  const seen = new Set<MobileViewId>();
  const valid: MobileViewId[] = [];
  for (const item of raw) {
    if (isMobileViewId(item) && !seen.has(item)) {
      seen.add(item);
      valid.push(item);
      if (valid.length >= max) break;
    }
  }
  if (valid.length < MIN_PINNED_SHORTCUTS) {
    return [...DEFAULT_PINNED_VIEWS];
  }
  return valid;
}

export function getUnpinnedViews(
  pinned: readonly MobileViewId[],
): MobileViewId[] {
  const pinnedSet = new Set(pinned);
  return ALL_MOBILE_VIEWS.map((v) => v.id).filter((id) => !pinnedSet.has(id));
}

export function toggleShortcutPin(
  pinned: readonly MobileViewId[],
  id: MobileViewId,
): MobileViewId[] {
  const idx = pinned.indexOf(id);
  if (idx >= 0) {
    if (pinned.length <= MIN_PINNED_SHORTCUTS) {
      return [...pinned];
    }
    return pinned.filter((item) => item !== id);
  }
  if (pinned.length >= maxPinnedShortcuts()) {
    return [...pinned];
  }
  return [...pinned, id];
}

export function moveShortcut(
  pinned: readonly MobileViewId[],
  id: MobileViewId,
  delta: -1 | 1,
): MobileViewId[] {
  const idx = pinned.indexOf(id);
  if (idx < 0) return [...pinned];
  const target = idx + delta;
  if (target < 0 || target >= pinned.length) {
    return [...pinned];
  }
  return reorderShortcut(pinned, idx, target);
}

export function reorderShortcut(
  pinned: readonly MobileViewId[],
  fromIndex: number,
  toIndex: number,
): MobileViewId[] {
  if (
    fromIndex === toIndex ||
    fromIndex < 0 ||
    toIndex < 0 ||
    fromIndex >= pinned.length ||
    toIndex >= pinned.length
  ) {
    return [...pinned];
  }
  const next = [...pinned];
  const [moved] = next.splice(fromIndex, 1);
  next.splice(toIndex, 0, moved);
  return next;
}

export function resetShortcuts(): MobileViewId[] {
  return [...DEFAULT_PINNED_VIEWS];
}

export function loadShortcuts(
  storage?: Pick<Storage, "getItem">,
): MobileViewId[] {
  try {
    const store =
      storage ??
      (typeof window !== "undefined" ? window.localStorage : undefined);
    const raw = store?.getItem(SHORTCUTS_STORAGE_KEY);
    if (!raw) return [...DEFAULT_PINNED_VIEWS];
    const parsed = JSON.parse(raw) as unknown;
    // Support either a raw array or versioned object `{ version: 1, pinned: [...] }`.
    if (
      parsed &&
      typeof parsed === "object" &&
      !Array.isArray(parsed) &&
      "pinned" in parsed
    ) {
      return normalizePinnedShortcuts((parsed as { pinned: unknown }).pinned);
    }
    return normalizePinnedShortcuts(parsed);
  } catch {
    return [...DEFAULT_PINNED_VIEWS];
  }
}

export function saveShortcuts(
  pinned: readonly MobileViewId[],
  storage?: Pick<Storage, "setItem">,
): MobileViewId[] {
  const normalized = normalizePinnedShortcuts(pinned);
  try {
    const store =
      storage ??
      (typeof window !== "undefined" ? window.localStorage : undefined);
    store?.setItem(
      SHORTCUTS_STORAGE_KEY,
      JSON.stringify({ version: 1, pinned: normalized }),
    );
  } catch {
    // Ignore storage quota or private browsing errors cleanly.
  }
  return normalized;
}

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

type FontFamily =
  | "system"
  | "inter"
  | "segoe"
  | "sf-pro"
  | "noto-sans"
  | "ibm-plex";

interface AppearancePreferences {
  fontFamily: FontFamily;
  fontSize: number;
  pageZoom: number;
}

interface AppearanceContextValue extends AppearancePreferences {
  setFontFamily: (value: FontFamily) => void;
  setFontSize: (value: number) => void;
  setPageZoom: (value: number) => void;
  zoomIn: () => void;
  zoomOut: () => void;
  resetPageZoom: () => void;
}

const STORAGE_KEY = "cc-switch-appearance";
const DEFAULT_FONT_FAMILY: FontFamily = "system";
const DEFAULT_FONT_SIZE = 13;
const DEFAULT_PAGE_ZOOM = 1;
const MIN_FONT_SIZE = 12;
const MAX_FONT_SIZE = 18;
const MIN_PAGE_ZOOM = 0.5;
const MAX_PAGE_ZOOM = 2;
const PAGE_ZOOM_STEP = 0.2;

export const FONT_SIZE_OPTIONS = [12, 13, 14, 15, 16, 17, 18] as const;

export const FONT_FAMILY_OPTIONS: Array<{
  value: FontFamily;
  label: string;
  css: string;
}> = [
  {
    value: "system",
    label: "System UI",
    css: 'var(--cc-system-font-family)',
  },
  {
    value: "inter",
    label: "Inter",
    css: '"Inter", var(--cc-system-font-family)',
  },
  {
    value: "segoe",
    label: "Segoe UI",
    css: '"Segoe UI Variable Text", "Segoe UI", var(--cc-system-font-family)',
  },
  {
    value: "sf-pro",
    label: "SF Pro",
    css: '"SF Pro Text", -apple-system, BlinkMacSystemFont, var(--cc-system-font-family)',
  },
  {
    value: "noto-sans",
    label: "Noto Sans",
    css: '"Noto Sans", "Noto Sans CJK SC", "Source Han Sans SC", var(--cc-system-font-family)',
  },
  {
    value: "ibm-plex",
    label: "IBM Plex Sans",
    css: '"IBM Plex Sans", var(--cc-system-font-family)',
  },
];

const FONT_FAMILY_MAP = Object.fromEntries(
  FONT_FAMILY_OPTIONS.map((item) => [item.value, item.css]),
) as Record<FontFamily, string>;

const isFontFamily = (value: unknown): value is FontFamily =>
  typeof value === "string" &&
  FONT_FAMILY_OPTIONS.some((item) => item.value === value);

const clampFontSize = (value: number) =>
  Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, Math.round(value)));

const clampPageZoom = (value: number) =>
  Math.min(
    MAX_PAGE_ZOOM,
    Math.max(MIN_PAGE_ZOOM, Math.round(value / PAGE_ZOOM_STEP) * PAGE_ZOOM_STEP),
  );

const readPreferences = (): AppearancePreferences => {
  if (typeof window === "undefined") {
    return {
      fontFamily: DEFAULT_FONT_FAMILY,
      fontSize: DEFAULT_FONT_SIZE,
      pageZoom: DEFAULT_PAGE_ZOOM,
    };
  }

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) throw new Error("missing");

    const parsed = JSON.parse(raw) as Partial<AppearancePreferences>;
    return {
      fontFamily: isFontFamily(parsed.fontFamily)
        ? parsed.fontFamily
        : DEFAULT_FONT_FAMILY,
      fontSize:
        typeof parsed.fontSize === "number"
          ? clampFontSize(parsed.fontSize)
          : DEFAULT_FONT_SIZE,
      pageZoom:
        typeof parsed.pageZoom === "number"
          ? clampPageZoom(parsed.pageZoom)
          : DEFAULT_PAGE_ZOOM,
    };
  } catch {
    return {
      fontFamily: DEFAULT_FONT_FAMILY,
      fontSize: DEFAULT_FONT_SIZE,
      pageZoom: DEFAULT_PAGE_ZOOM,
    };
  }
};

function applyFontPreferences(preferences: AppearancePreferences) {
  const root = document.documentElement;
  root.style.setProperty(
    "--ui-font-family",
    FONT_FAMILY_MAP[preferences.fontFamily],
  );
  root.style.setProperty(
    "--ui-font-scale",
    String(preferences.fontSize / DEFAULT_FONT_SIZE),
  );
}

async function applyPageZoom(pageZoom: number) {
  try {
    await getCurrentWebview().setZoom(pageZoom);
    document.documentElement.style.removeProperty("zoom");
  } catch {
    // Browser/Vite fallback for local development outside Tauri.
    document.documentElement.style.zoom = String(pageZoom);
  }
}

const AppearanceContext = createContext<AppearanceContextValue | undefined>(
  undefined,
);

export function AppearanceProvider({ children }: { children: ReactNode }) {
  const [preferences, setPreferences] =
    useState<AppearancePreferences>(readPreferences);

  useLayoutEffect(() => {
    applyFontPreferences(preferences);
  }, [preferences.fontFamily, preferences.fontSize]);

  useEffect(() => {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
    } catch {
      // Appearance preferences are best-effort local state.
    }
  }, [preferences]);

  useEffect(() => {
    void applyPageZoom(preferences.pageZoom);
  }, [preferences.pageZoom]);

  const setFontFamily = useCallback((fontFamily: FontFamily) => {
    setPreferences((prev) => ({ ...prev, fontFamily }));
  }, []);

  const setFontSize = useCallback((fontSize: number) => {
    setPreferences((prev) => ({
      ...prev,
      fontSize: clampFontSize(fontSize),
    }));
  }, []);

  const setPageZoom = useCallback((pageZoom: number) => {
    setPreferences((prev) => ({
      ...prev,
      pageZoom: clampPageZoom(pageZoom),
    }));
  }, []);

  const zoomIn = useCallback(() => {
    setPreferences((prev) => ({
      ...prev,
      pageZoom: clampPageZoom(prev.pageZoom + PAGE_ZOOM_STEP),
    }));
  }, []);

  const zoomOut = useCallback(() => {
    setPreferences((prev) => ({
      ...prev,
      pageZoom: clampPageZoom(prev.pageZoom - PAGE_ZOOM_STEP),
    }));
  }, []);

  const resetPageZoom = useCallback(() => {
    setPreferences((prev) => ({
      ...prev,
      pageZoom: DEFAULT_PAGE_ZOOM,
    }));
  }, []);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;

      if (event.code === "Equal" || event.key === "+") {
        event.preventDefault();
        zoomIn();
        return;
      }

      if (event.code === "Minus" || event.key === "-") {
        event.preventDefault();
        zoomOut();
        return;
      }

      if (event.code === "Digit0" || event.code === "Numpad0") {
        event.preventDefault();
        resetPageZoom();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [resetPageZoom, zoomIn, zoomOut]);

  const value = useMemo<AppearanceContextValue>(
    () => ({
      ...preferences,
      setFontFamily,
      setFontSize,
      setPageZoom,
      zoomIn,
      zoomOut,
      resetPageZoom,
    }),
    [
      preferences,
      resetPageZoom,
      setFontFamily,
      setFontSize,
      setPageZoom,
      zoomIn,
      zoomOut,
    ],
  );

  return (
    <AppearanceContext.Provider value={value}>
      {children}
    </AppearanceContext.Provider>
  );
}

export function useAppearance() {
  const context = useContext(AppearanceContext);
  if (!context) {
    throw new Error("useAppearance must be used within an AppearanceProvider");
  }
  return context;
}

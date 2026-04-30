import {
  useRef,
  useState,
  useEffect,
  useCallback,
  type MouseEvent,
} from "react";
import {
  motion,
  AnimatePresence,
  useMotionValue,
  useTransform,
  useSpring,
} from "framer-motion";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { ProviderIcon } from "@/components/ProviderIcon";
import { cn } from "@/lib/utils";

interface AppSwitcherProps {
  activeApp: AppId;
  onSwitch: (app: AppId) => void;
  visibleApps?: VisibleApps;
  compact?: boolean;
}

const ALL_APPS: AppId[] = [
  "claude", "codex", "gemini", "opencode", "openclaw", "hermes",
];
const STORAGE_KEY = "cc-switch-last-app";
const MODE_KEY = "cc-switch-app-switcher-mode";

const APP_META: Record<AppId, { icon: string; label: string }> = {
  claude: { icon: "claude", label: "Claude" },
  codex: { icon: "openai", label: "Codex" },
  gemini: { icon: "gemini", label: "Gemini" },
  opencode: { icon: "opencode", label: "OpenCode" },
  openclaw: { icon: "openclaw", label: "OpenClaw" },
  hermes: { icon: "hermes", label: "Hermes" },
};

type SwitcherMode = "island" | "dock";

// ── Dock physics config ──
const ITEM_W = 36;           // base item width (wider hit area)
const ICON_BASE = 16;        // base icon size
const SCALE_MAX = 1.5;       // hovered icon scale (less extreme)
const PUSH_RANGE = 200;      // px range for push effect (wider)
const PUSH_MAX = 8;          // max px an icon gets pushed (gentler)

// ── Dock hook: track mouse X ──
function useDockMouse(containerRef: React.RefObject<HTMLDivElement | null>) {
  const mouseX = useMotionValue(-999);

  const onMove = useCallback(
    (e: MouseEvent) => {
      const rect = containerRef.current?.getBoundingClientRect();
      if (rect) mouseX.set(e.clientX - rect.left);
    },
    [containerRef, mouseX],
  );

  const onLeave = useCallback(() => { mouseX.set(-999); }, [mouseX]);

  return { mouseX, onMove, onLeave };
}

// ── Single Dock icon with 3D scale + push displacement ──
function DockIcon({
  meta,
  isActive,
  mouseX,
  onClick,
}: {
  meta: { icon: string; label: string };
  isActive: boolean;
  mouseX: ReturnType<typeof useMotionValue<number>>;
  onClick: () => void;
}) {
  const ref = useRef<HTMLButtonElement>(null);

  // Distance from mouse to this item's center
  const distance = useTransform(mouseX, (mx: number) => {
    const el = ref.current;
    if (!el || mx < -900) return 999;
    const container = el.parentElement?.parentElement;
    if (!container) return 999;
    const cr = container.getBoundingClientRect();
    const er = el.getBoundingClientRect();
    const center = (er.left - cr.left) + er.width / 2;
    return mx - center; // signed: negative = mouse is left of icon
  });

  // Scale: magnify based on absolute distance (gentle curve)
  const rawScale = useTransform(distance, (d: number) => {
    const abs = Math.abs(d);
    if (abs > PUSH_RANGE) return 1;
    const t = 1 - abs / PUSH_RANGE;
    // Cubic ease-out — fast start, gentle tail
    const curve = 1 - (1 - t) * (1 - t) * (1 - t);
    return 1 + (SCALE_MAX - 1) * curve;
  });
  const scale = useSpring(rawScale, { stiffness: 380, damping: 26, mass: 0.3 });

  // Push displacement: icons shift away from hovered
  const rawX = useTransform(distance, (d: number) => {
    const abs = Math.abs(d);
    if (abs < ITEM_W / 2) return 0; // hovered icon stays put
    if (abs > PUSH_RANGE) return 0;
    const t = 1 - (abs - ITEM_W / 2) / (PUSH_RANGE - ITEM_W / 2);
    const curve = t * t * t; // cubic ease-in — sharp near hovered, gentle far away
    const direction = d > 0 ? -1 : 1;
    return direction * PUSH_MAX * curve;
  });
  const x = useSpring(rawX, { stiffness: 350, damping: 28, mass: 0.35 });

  // Y bounce: hovered icon rises
  const rawY = useTransform(distance, (d: number) => {
    const abs = Math.abs(d);
    if (abs > PUSH_RANGE * 0.5) return 0;
    const t = 1 - abs / (PUSH_RANGE * 0.5);
    return -5 * t * t * t;
  });
  const y = useSpring(rawY, { stiffness: 400, damping: 22, mass: 0.25 });

  // Shadow: diffusion shadow grows with magnification
  const shadowScale = useTransform(scale, (s: number) => {
    const t = (s - 1) / (SCALE_MAX - 1); // 0..1
    return t;
  });

  return (
    <motion.button
      ref={ref}
      type="button"
      onClick={onClick}
      style={{ x, width: ITEM_W }}
      className={cn(
        "relative z-10 inline-flex items-center justify-center h-10 cursor-pointer",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/30",
        isActive ? "text-foreground" : "text-muted-foreground/50",
      )}
    >
      {/* Diffusion shadow */}
      <motion.div
        className="absolute bottom-0 left-1/2 -translate-x-1/2 w-5 h-1.5 rounded-full bg-black/15 dark:bg-black/30 blur-sm"
        style={{
          opacity: shadowScale,
          scaleX: useTransform(shadowScale, (t) => 0.6 + t * 0.6),
        }}
      />

      {/* Icon with 3D magnification */}
      <motion.div
        style={{
          scale,
          y,
          transformPerspective: 500,
          rotateX: useTransform(distance, (d: number) => {
            const abs = Math.abs(d);
            if (abs > PUSH_RANGE * 0.4) return 0;
            const t = 1 - abs / (PUSH_RANGE * 0.4);
            return -2 * t * t; // subtle 3D tilt
          }),
        }}
        className="relative flex items-center justify-center origin-bottom"
      >
        <ProviderIcon icon={meta.icon} name={meta.label} size={ICON_BASE} />
      </motion.div>
    </motion.button>
  );
}

// ── Island mode item ──
function IslandItem({
  meta,
  isActive,
  onClick,
}: {
  meta: { icon: string; label: string };
  isActive: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "relative z-10 inline-flex items-center h-6 rounded-lg text-xs font-medium cursor-pointer px-2.5",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/30",
      )}
    >
      <motion.div
        className="flex items-center gap-1.5"
        layout
        transition={{ type: "spring", stiffness: 500, damping: 28, mass: 0.5 }}
      >
        <motion.div
          animate={{ scale: isActive ? 1 : 0.85, opacity: isActive ? 1 : 0.5 }}
          transition={
            isActive
              ? { type: "spring", stiffness: 600, damping: 22, mass: 0.4 }
              : { type: "spring", stiffness: 400, damping: 28, mass: 0.5 }
          }
        >
          <ProviderIcon icon={meta.icon} name={meta.label} size={16} />
        </motion.div>
        <AnimatePresence mode="popLayout">
          <motion.span
            key={meta.label}
            initial={{ opacity: 0, width: 0, marginLeft: 0 }}
            animate={{ opacity: isActive ? 1 : 0.4, width: "auto", marginLeft: 6 }}
            exit={{ opacity: 0, width: 0, marginLeft: 0 }}
            transition={{ type: "spring", stiffness: 500, damping: 28, mass: 0.4 }}
            className={cn(
              "whitespace-nowrap overflow-hidden block text-xs",
              isActive ? "text-foreground font-medium" : "text-muted-foreground",
            )}
          >
            {meta.label}
          </motion.span>
        </AnimatePresence>
      </motion.div>
    </button>
  );
}

// ── Main ──
export function AppSwitcher({
  activeApp,
  onSwitch,
  visibleApps,
}: AppSwitcherProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const { mouseX, onMove, onLeave } = useDockMouse(containerRef);

  const [mode, setMode] = useState<SwitcherMode>(() => {
    const saved = localStorage.getItem(MODE_KEY);
    return saved === "dock" ? "dock" : "island";
  });

  // Island pill
  const [pill, setPill] = useState({ left: 0, width: 0 });
  const [isInitial, setIsInitial] = useState(true);
  const [isSwitching, setIsSwitching] = useState(false);

  const appsToShow = ALL_APPS.filter((app) => {
    if (!visibleApps) return true;
    return visibleApps[app];
  });

  const measure = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    const btn = container.querySelector(`[data-app="${activeApp}"]`) as HTMLElement | null;
    if (!btn) return;
    const cr = container.getBoundingClientRect();
    const br = btn.getBoundingClientRect();
    setPill({ left: br.left - cr.left, width: br.width });
  }, [activeApp]);

  useEffect(() => {
    measure();
    if (isInitial) requestAnimationFrame(() => setIsInitial(false));
  }, [measure, isInitial]);

  useEffect(() => {
    if (isInitial) return;
    setIsSwitching(true);
    const t = setTimeout(() => setIsSwitching(false), 350);
    return () => clearTimeout(t);
  }, [activeApp, isInitial]);

  const handleSwitch = (app: AppId) => {
    if (app === activeApp) return;
    localStorage.setItem(STORAGE_KEY, app);
    onSwitch(app);
  };

  const toggleMode = () => {
    const next = mode === "island" ? "dock" : "island";
    setMode(next);
    localStorage.setItem(MODE_KEY, next);
  };

  const isDock = mode === "dock";

  return (
    <div className="inline-flex items-center gap-1.5">
      <div
        ref={containerRef}
        onMouseMove={isDock ? onMove : undefined}
        onMouseLeave={isDock ? onLeave : undefined}
        className={cn(
          "relative inline-flex items-center liquid-glass-subtle",
          isDock
            ? "h-10 rounded-2xl px-2 items-end pb-1.5"
            : "h-8 rounded-xl p-1",
        )}
      >
        {/* ── Island pill ── */}
        {!isDock && (
          <motion.div
            className="absolute rounded-lg overflow-hidden"
            style={{ zIndex: 0, top: 4, height: 24 }}
            initial={false}
            animate={{
              left: pill.left - 1,
              width: pill.width + 2,
              scaleY: isSwitching ? 1.06 : 1,
            }}
            transition={
              isInitial
                ? { duration: 0 }
                : isSwitching
                  ? { type: "spring", stiffness: 600, damping: 20, mass: 0.4 }
                  : { type: "spring", stiffness: 450, damping: 28, mass: 0.5 }
            }
          >
            <div className="absolute inset-0 bg-white/60 dark:bg-white/10 backdrop-blur-sm" />
            <div className="absolute inset-x-0 top-0 h-px bg-white/40 dark:bg-white/10" />
            <AnimatePresence>
              {isSwitching && (
                <motion.div
                  className="absolute inset-0 rounded-lg border border-primary/20"
                  initial={{ opacity: 0.6, scale: 1 }}
                  animate={{ opacity: 0, scale: 1.25 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.4, ease: "easeOut" }}
                />
              )}
            </AnimatePresence>
          </motion.div>
        )}

        {/* ── Dock bottom dot ── */}
        {isDock && (
          <motion.div
            className="absolute bottom-1 h-1 rounded-full bg-primary/40"
            initial={false}
            animate={{ left: pill.left + 2, width: Math.max(pill.width - 4, 8) }}
            transition={isInitial ? { duration: 0 } : { type: "spring", stiffness: 500, damping: 28, mass: 0.5 }}
            style={{ zIndex: 0 }}
          />
        )}

        {/* ── Items ── */}
        {appsToShow.map((app) => {
          const meta = APP_META[app];
          return (
            <div key={app} data-app={app} className="flex items-end">
              {isDock ? (
                <DockIcon
                  meta={meta}
                  isActive={activeApp === app}
                  mouseX={mouseX}
                  onClick={() => handleSwitch(app)}
                />
              ) : (
                <IslandItem
                  meta={meta}
                  isActive={activeApp === app}
                  onClick={() => handleSwitch(app)}
                />
              )}
            </div>
          );
        })}
      </div>

      {/* ── Mode toggle ── */}
      <button
        type="button"
        onClick={toggleMode}
        className="flex items-center justify-center w-5 h-5 rounded-md text-muted-foreground/40 hover:text-muted-foreground hover:bg-white/20 dark:hover:bg-white/5 transition-colors cursor-pointer"
        title={isDock ? "Switch to Island" : "Switch to Dock"}
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
          {isDock ? (
            <>
              <rect x="1" y="3" width="10" height="6" rx="3" />
              <circle cx="6" cy="6" r="1.5" fill="currentColor" stroke="none" />
            </>
          ) : (
            <>
              <line x1="1" y1="9" x2="11" y2="9" />
              <circle cx="3" cy="6" r="1.5" fill="currentColor" stroke="none" />
              <circle cx="6" cy="6" r="1.5" fill="currentColor" stroke="none" />
              <circle cx="9" cy="6" r="1.5" fill="currentColor" stroke="none" />
            </>
          )}
        </svg>
      </button>
    </div>
  );
}

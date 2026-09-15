import { useEffect, useRef, useState } from "react";
import { motion, useReducedMotion } from "framer-motion";
import { cn } from "@/lib/utils";

const BURST_LIFETIME_MS = 980;

const PARTICLES = [
  { x: -76, y: -16, size: 3, delay: 0.01, color: "#60a5fa" },
  { x: -63, y: 17, size: 2, delay: 0.05, color: "#22d3ee" },
  { x: -51, y: -25, size: 2, delay: 0.09, color: "#34d399" },
  { x: -38, y: 24, size: 4, delay: 0.03, color: "#34d399" },
  { x: -24, y: -19, size: 2, delay: 0.12, color: "#93c5fd" },
  { x: -11, y: 27, size: 3, delay: 0.08, color: "#5eead4" },
  { x: 8, y: -26, size: 3, delay: 0.04, color: "#22d3ee" },
  { x: 19, y: 24, size: 2, delay: 0.13, color: "#34d399" },
  { x: 34, y: -22, size: 4, delay: 0.07, color: "#6ee7b7" },
  { x: 47, y: 21, size: 2, delay: 0.02, color: "#60a5fa" },
  { x: 61, y: -15, size: 3, delay: 0.11, color: "#2dd4bf" },
  { x: 76, y: 13, size: 2, delay: 0.06, color: "#34d399" },
] as const;

interface RoutingActivationBrandProps {
  active: boolean;
  contextKey: string;
  ready: boolean;
}

/**
 * Keeps the brand's existing blue/emerald status semantics, then adds a
 * short-lived confirmation burst only when the current app successfully
 * transitions from direct mode to route takeover.
 */
export function RoutingActivationBrand({
  active,
  contextKey,
  ready,
}: RoutingActivationBrandProps) {
  const prefersReducedMotion = useReducedMotion();
  const previousState = useRef({ active, contextKey, ready });
  const [burstSequence, setBurstSequence] = useState(0);
  const [showBurst, setShowBurst] = useState(false);

  useEffect(() => {
    const previous = previousState.current;
    const sameContext = previous.contextKey === contextKey;
    const justActivated =
      previous.ready && ready && sameContext && !previous.active && active;
    previousState.current = { active, contextKey, ready };

    if (!ready || !sameContext || !active || prefersReducedMotion) {
      setShowBurst(false);
      return;
    }

    if (!justActivated) return;

    setBurstSequence((sequence) => sequence + 1);
    setShowBurst(true);
    const timeoutId = window.setTimeout(
      () => setShowBurst(false),
      BURST_LIFETIME_MS,
    );
    return () => window.clearTimeout(timeoutId);
  }, [active, contextKey, prefersReducedMotion, ready]);

  return (
    <div className="relative isolate inline-flex items-center">
      {showBurst && (
        <motion.span
          key={`glow-${burstSequence}`}
          aria-hidden="true"
          className="pointer-events-none absolute -inset-x-3 -inset-y-2 -z-10 rounded-full bg-emerald-400/20 blur-md"
          initial={{ opacity: 0, scaleX: 0.3, scaleY: 0.65 }}
          animate={{
            opacity: [0, 0.8, 0],
            scaleX: [0.3, 1.05, 1.45],
            scaleY: [0.65, 1, 1.25],
          }}
          transition={{
            duration: 0.82,
            times: [0, 0.24, 1],
            ease: [0.16, 1, 0.3, 1],
          }}
        />
      )}

      <motion.a
        href="https://ccswitch.io"
        target="_blank"
        rel="noreferrer"
        className={cn(
          "relative z-10 text-xl font-semibold transition-colors duration-500",
          active
            ? "text-emerald-500 hover:text-emerald-600 dark:text-emerald-400 dark:hover:text-emerald-300"
            : "text-blue-500 hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300",
        )}
        animate={
          showBurst
            ? {
                scale: [1, 0.96, 1.075, 1],
                y: [0, 1, -1.5, 0],
                filter: [
                  "drop-shadow(0 0 0 rgba(52, 211, 153, 0))",
                  "drop-shadow(0 0 7px rgba(52, 211, 153, 0.75))",
                  "drop-shadow(0 0 3px rgba(52, 211, 153, 0.28))",
                  "drop-shadow(0 0 0 rgba(52, 211, 153, 0))",
                ],
              }
            : {
                scale: 1,
                y: 0,
                filter: "drop-shadow(0 0 0 rgba(52, 211, 153, 0))",
              }
        }
        transition={
          showBurst
            ? {
                duration: 0.72,
                times: [0, 0.13, 0.52, 1],
                ease: [0.16, 1, 0.3, 1],
              }
            : { duration: 0.28, ease: [0.22, 1, 0.36, 1] }
        }
      >
        CC Switch
      </motion.a>

      {showBurst && (
        <motion.span
          key={`particles-${burstSequence}`}
          data-testid="routing-activation-particles"
          aria-hidden="true"
          className="pointer-events-none absolute left-1/2 top-1/2 z-20 h-0 w-0"
        >
          {PARTICLES.map((particle, index) => (
            <motion.span
              key={`${particle.x}-${particle.y}`}
              className="absolute left-0 top-0 block rounded-full"
              style={{
                width: particle.size,
                height: particle.size,
                backgroundColor: particle.color,
                boxShadow: `0 0 ${particle.size * 2 + 2}px ${particle.color}`,
              }}
              initial={{ x: 0, y: 0, opacity: 0, scale: 0.2 }}
              animate={{
                x: [0, particle.x * 0.72, particle.x],
                y: [0, particle.y * 0.62, particle.y],
                opacity: [0, 1, 0],
                scale: [0.2, index % 3 === 0 ? 1.45 : 1, 0.25],
              }}
              transition={{
                duration: 0.66 + (index % 4) * 0.055,
                delay: particle.delay,
                times: [0, 0.2, 1],
                ease: [0.16, 1, 0.3, 1],
              }}
            />
          ))}
        </motion.span>
      )}
    </div>
  );
}

import { useLayoutEffect, useRef, type ReactNode } from "react";

/** Animate the page header only: moving long content layers stalls native WebKit. */
export function PageFrame({
  children,
  transitionKey,
}: {
  children: ReactNode;
  transitionKey: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = ref.current?.querySelector("header");
    const preference = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!element?.animate || preference?.matches) return;
    const animation = element.animate(
      [
        { opacity: 0.72, transform: "translateY(4px)" },
        { opacity: 1, transform: "translateY(0)" },
      ],
      { duration: 140, easing: "cubic-bezier(0.2, 0, 0, 1)" },
    );
    const stop = () => animation.cancel();
    preference?.addEventListener("change", stop);
    return () => {
      stop();
      preference?.removeEventListener("change", stop);
    };
  }, [transitionKey]);
  return (
    <div ref={ref} className="flex min-h-0 flex-1 flex-col">
      {children}
    </div>
  );
}

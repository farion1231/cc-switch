import { useLayoutEffect, useRef, useState } from "react";
import { Portal as TooltipPortal } from "@radix-ui/react-tooltip";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export function ToolErrorMessage({ message }: { message: string }) {
  const textRef = useRef<HTMLDivElement>(null);
  const [isTruncated, setIsTruncated] = useState(false);
  const [isOpen, setIsOpen] = useState(false);

  useLayoutEffect(() => {
    const element = textRef.current;
    if (!element) return;

    const measure = () => {
      const truncated = element.scrollWidth > element.clientWidth;
      setIsTruncated(truncated);
      if (!truncated) setIsOpen(false);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [message]);

  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip
        open={isTruncated && isOpen}
        onOpenChange={(open) => setIsOpen(isTruncated && open)}
      >
        <TooltipTrigger asChild>
          <div
            ref={textRef}
            tabIndex={isTruncated ? 0 : undefined}
            className={`truncate text-[11px] text-muted-foreground${isTruncated ? " cursor-help" : ""}`}
          >
            {message}
          </div>
        </TooltipTrigger>
        <TooltipPortal>
          <TooltipContent
            side="bottom"
            align="start"
            className="max-h-[min(24rem,var(--radix-tooltip-content-available-height))] max-w-[min(36rem,calc(100vw_-_2rem))] select-text overflow-y-auto whitespace-pre-wrap break-words [overflow-wrap:anywhere]"
          >
            {message}
          </TooltipContent>
        </TooltipPortal>
      </Tooltip>
    </TooltipProvider>
  );
}

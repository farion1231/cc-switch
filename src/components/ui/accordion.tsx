import * as React from "react";
import { ChevronDown } from "lucide-react";

import { cn } from "@/lib/utils";

type AccordionType = "single" | "multiple";

interface AccordionContextValue {
  openValues: string[];
  toggle: (value: string) => void;
}

const AccordionContext = React.createContext<AccordionContextValue | null>(
  null,
);
const AccordionItemContext = React.createContext<string | null>(null);

export interface AccordionProps
  extends Omit<
    React.HTMLAttributes<HTMLDivElement>,
    "onChange" | "defaultValue"
  > {
  type?: AccordionType;
  defaultValue?: string | string[];
  value?: string | string[];
  onValueChange?: (value: string | string[]) => void;
}

function toArray(value?: string | string[]): string[] {
  if (value == null) return [];
  return Array.isArray(value) ? value : [value];
}

function useOpen(value: string | null): boolean {
  const context = React.useContext(AccordionContext);
  return value != null && (context?.openValues.includes(value) ?? false);
}

export function Accordion({
  type = "single",
  defaultValue,
  value,
  onValueChange,
  className,
  children,
  ...props
}: AccordionProps) {
  const [uncontrolled, setUncontrolled] = React.useState<string[]>(() =>
    toArray(defaultValue),
  );
  const controlled = value !== undefined;
  const openValues = controlled ? toArray(value) : uncontrolled;

  const toggle = (item: string) => {
    const isOpen = openValues.includes(item);
    const next =
      type === "multiple"
        ? isOpen
          ? openValues.filter((open) => open !== item)
          : [...openValues, item]
        : isOpen
          ? []
          : [item];
    if (!controlled) setUncontrolled(next);
    onValueChange?.(type === "multiple" ? next : (next[0] ?? ""));
  };

  return (
    <AccordionContext.Provider value={{ openValues, toggle }}>
      <div className={className} {...props}>
        {children}
      </div>
    </AccordionContext.Provider>
  );
}

export function AccordionItem({
  value,
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { value: string }) {
  const open = useOpen(value);
  return (
    <AccordionItemContext.Provider value={value}>
      <div
        data-state={open ? "open" : "closed"}
        className={cn("border-b", className)}
        {...props}
      />
    </AccordionItemContext.Provider>
  );
}

export function AccordionTrigger({
  className,
  children,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const item = React.useContext(AccordionItemContext);
  const context = React.useContext(AccordionContext);
  const open = useOpen(item);

  return (
    <div className="flex">
      <button
        type="button"
        aria-expanded={open}
        data-state={open ? "open" : "closed"}
        className={cn(
          "flex flex-1 items-center justify-between py-4 font-medium transition-all hover:underline [&[data-state=open]>svg]:rotate-180",
          className,
        )}
        onClick={() => {
          if (item != null) context?.toggle(item);
        }}
        {...props}
      >
        {children}
        <ChevronDown className="h-4 w-4 shrink-0 transition-transform duration-200" />
      </button>
    </div>
  );
}

export function AccordionContent({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  const item = React.useContext(AccordionItemContext);
  const open = useOpen(item);
  if (!open) return null;

  return (
    <div
      data-state="open"
      className="overflow-hidden text-sm transition-all"
      {...props}
    >
      <div className={cn("pb-4 pt-0", className)}>{children}</div>
    </div>
  );
}

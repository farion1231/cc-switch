import { Check } from "lucide-react";
import { cn } from "@/lib/utils";

export interface ManagementFilterOption {
  value: string;
  label: string;
  /** Optional per-option count, rendered after the label. */
  count?: number;
}

interface ManagementFilterChipsProps {
  label: string;
  options: ManagementFilterOption[];
  selected: string[];
  onSelectionChange: (selected: string[]) => void;
  /**
   * Selection semantics for items carrying several tags:
   * - "any" (default): show items tagged with at least one selected value
   * - "all": show only items tagged with every selected value
   *
   * The chips component itself treats both the same — the list applies this
   * when filtering. Declared here so a caller cannot mix modes accidentally.
   */
  mode?: "any" | "all";
  disabled?: boolean;
  className?: string;
}

/**
 * Multi-select filter chip row for local management lists.
 *
 * Empty selection means "no filter" (every option shown), which is the state the
 * list starts in — the chips are refinements, not required choices.
 *
 * `mode` decides what the selection means for items carrying several tags:
 * - "any" (default): show items tagged with at least one selected value
 * - "all": show only items tagged with every selected value
 */
export function ManagementFilterChips({
  label,
  options,
  selected,
  onSelectionChange,
  disabled = false,
  className,
}: ManagementFilterChipsProps) {
  const toggle = (value: string) => {
    onSelectionChange(
      selected.includes(value)
        ? selected.filter((item) => item !== value)
        : [...selected, value],
    );
  };

  return (
    <div
      role="group"
      aria-label={label}
      className={cn("flex flex-wrap items-center gap-1.5", className)}
    >
      {options.map((option) => {
        const checked = selected.includes(option.value);
        return (
          <button
            key={option.value}
            type="button"
            disabled={disabled}
            aria-pressed={checked}
            onClick={() => toggle(option.value)}
            className={cn(
              "inline-flex h-6 items-center gap-1 rounded-full border px-2.5 text-xs transition-colors",
              checked
                ? "border-primary/40 bg-primary/10 text-foreground"
                : "border-border-default bg-background text-muted-foreground hover:bg-muted hover:text-foreground",
              disabled && "cursor-not-allowed opacity-50",
            )}
            title={option.label}
          >
            <Check
              aria-hidden="true"
              className={cn("h-3 w-3", checked ? "opacity-100" : "opacity-0")}
            />
            <span className="truncate">{option.label}</span>
            {typeof option.count === "number" && (
              <span className="tabular-nums opacity-60">{option.count}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}

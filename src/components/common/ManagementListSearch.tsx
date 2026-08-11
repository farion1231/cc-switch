import { Search, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

interface ManagementListSearchProps {
  value: string;
  onValueChange: (value: string) => void;
  placeholder: string;
  ariaLabel: string;
  clearLabel: string;
  className?: string;
}

/** Shared, presentation-only search field for local management lists. */
export function ManagementListSearch({
  value,
  onValueChange,
  placeholder,
  ariaLabel,
  clearLabel,
  className,
}: ManagementListSearchProps) {
  return (
    <div role="search" className={cn("relative flex-shrink-0 mb-4", className)}>
      <Search
        aria-hidden="true"
        className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
      />
      <Input
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape" && value) {
            event.stopPropagation();
            onValueChange("");
          }
        }}
        placeholder={placeholder}
        aria-label={ariaLabel}
        className="pl-9 pr-9"
      />
      {value && (
        <button
          type="button"
          onClick={() => onValueChange("")}
          aria-label={clearLabel}
          title={clearLabel}
          className="absolute right-2 top-1/2 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <X aria-hidden="true" className="h-4 w-4" />
        </button>
      )}
    </div>
  );
}

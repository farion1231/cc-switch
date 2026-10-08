import { SearchField } from "@/components/ui/search-field";
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
    <SearchField
      value={value}
      onValueChange={onValueChange}
      placeholder={placeholder}
      aria-label={ariaLabel}
      clearLabel={clearLabel}
      containerClassName={cn("mb-4 flex-shrink-0", className)}
    />
  );
}

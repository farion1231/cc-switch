import React from "react";
import { X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface ManagementBulkBarProps {
  selectedCount: number;
  totalCount: number;
  /** Select-all / clear control; omitted when the list does not support it. */
  onSelectAll?: () => void;
  onClear: () => void;
  /** Localized labels — the shared component must not hardcode copy. */
  selectAllLabel: string;
  clearLabel: string;
  toolbarLabel: string;
  disabled?: boolean;
  children?: React.ReactNode;
  className?: string;
}

/**
 * Action bar shown while a management list has a selection.
 *
 * Renders nothing when the selection is empty, so lists can keep it mounted
 * without reserving layout space.
 */
export function ManagementBulkBar({
  selectedCount,
  totalCount,
  onSelectAll,
  onClear,
  selectAllLabel,
  clearLabel,
  toolbarLabel,
  disabled = false,
  children,
  className,
}: ManagementBulkBarProps) {
  if (selectedCount === 0) return null;

  const allSelected = selectedCount >= totalCount && totalCount > 0;

  return (
    <div
      role="toolbar"
      aria-label={toolbarLabel}
      className={cn(
        "mb-3 flex flex-wrap items-center gap-2 rounded-lg border border-border-default bg-card/60 px-3 py-2",
        className,
      )}
    >
      <span className="text-xs text-muted-foreground">
        {selectedCount} / {totalCount}
      </span>
      {onSelectAll && !allSelected && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 text-xs"
          disabled={disabled}
          onClick={onSelectAll}
        >
          {selectAllLabel}
        </Button>
      )}
      {children}
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="ml-auto h-7 w-7"
        disabled={disabled}
        onClick={onClear}
        aria-label={clearLabel}
        title={clearLabel}
      >
        <X className="h-4 w-4" />
      </Button>
    </div>
  );
}

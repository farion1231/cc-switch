import React from "react";
import { X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface ManagementBulkBarProps {
  selectedCount: number;
  /**
   * Selected rows the active filter hides. While they have no visible row the
   * fraction reads 0 — this count is what keeps the bar (and with it the
   * hidden-selection disclosure and the scope-declaring action labels)
   * mounted instead of unmounting exactly when they matter.
   */
  hiddenCount?: number;
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
 * Renders nothing while nothing is selected or hidden, so lists can keep it
 * mounted without reserving layout space.
 */
export function ManagementBulkBar({
  selectedCount,
  hiddenCount,
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
  if (selectedCount === 0 && (hiddenCount ?? 0) === 0) return null;

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

import { Columns3 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ColumnOption, ColumnVisibility } from "@/types/table";
import { Button } from "./button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./dropdown-menu";

export interface ColumnVisibilityMenuProps<K extends string> {
  columns: readonly ColumnOption<K>[];
  visibility: ColumnVisibility<K>;
  onVisibleChange: (id: K, visible: boolean) => void;
  onReset: () => void;
  compact?: boolean;
}

export function ColumnVisibilityMenu<K extends string>({
  columns,
  visibility,
  onVisibleChange,
  onReset,
  compact = false,
}: ColumnVisibilityMenuProps<K>) {
  const { t } = useTranslation();
  const label = t("common.columnVisibility.label");

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="quiet"
          size={compact ? "icon-compact" : "compact"}
          aria-label={label}
          title={label}
        >
          <Columns3 aria-hidden="true" data-icon="inline-start" size={14} />
          {!compact && label}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        aria-label={label}
        className="min-w-[200px] max-w-[320px] rounded-panel p-1 shadow-v7-md"
      >
        <DropdownMenuGroup>
          {columns.map((column) => (
            <DropdownMenuCheckboxItem
              key={column.id}
              checked={
                column.required ||
                (visibility[column.id] ?? column.defaultVisible ?? true)
              }
              disabled={column.required}
              className="h-[30px] rounded-control"
              onCheckedChange={(checked) =>
                onVisibleChange(column.id, checked === true)
              }
              onSelect={(event) => event.preventDefault()}
            >
              <span>{column.label}</span>
              {column.required && (
                <span className="ms-auto ps-3 text-caption text-fg-3">
                  {t("common.columnVisibility.required")}
                </span>
              )}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuItem
            className="h-[30px] rounded-control"
            onSelect={(event) => {
              event.preventDefault();
              onReset();
            }}
          >
            {t("common.columnVisibility.reset")}
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

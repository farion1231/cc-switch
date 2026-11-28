import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { UnmanagedMcpServer } from "@/lib/api/mcp";
import { appHas } from "@/config/appSpecs";
import { cn } from "@/lib/utils";
import { MatrixSearch } from "./AppMatrix";
import { CHECKBOX_CLASS, V7Dialog } from "./formBits";

interface McpDiscoveryImportDialogProps {
  servers: UnmanagedMcpServer[];
  isImporting: boolean;
  onImport: (ids: string[]) => void;
  onClose: () => void;
}

/**
 * 「导入本机 MCP」（宽 560）：勾选要导入的未纳管 MCP，和 Skills 的导入对话框同一套交互。
 * 不选应用：导入后每个 MCP 在发现它的应用里启用，和「从应用导入」一致。
 */
export function McpDiscoveryImportDialog({
  servers,
  isImporting,
  onImport,
  onClose,
}: McpDiscoveryImportDialogProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(servers.map((server) => server.id)),
  );

  const appLabel = (app: string) =>
    appHas(app, "mcp") ? APP_DISPLAY_NAME[app] : app;
  const normalized = query.trim().toLowerCase();
  const visible = servers.filter(
    (server) =>
      !normalized ||
      [server.id, ...server.apps.map(appLabel)].some((value) =>
        value.toLowerCase().includes(normalized),
      ),
  );

  const toggle = (id: string, checked: boolean) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });

  return (
    <V7Dialog
      open
      width={560}
      onOpenChange={(open) => {
        if (!open && !isImporting) onClose();
      }}
    >
      <div className="flex shrink-0 flex-col gap-1">
        <DialogTitle className="text-section">
          {t("mcpPage.discovery.dialogTitle")}
        </DialogTitle>
        <DialogDescription className="text-caption text-fg-2">
          {t("mcpPage.discovery.dialogLead")}
        </DialogDescription>
      </div>

      <div className="flex shrink-0 items-center gap-2">
        <MatrixSearch
          className="flex-1"
          value={query}
          onValueChange={setQuery}
          placeholder={t("mcpPage.discovery.searchPlaceholder")}
          ariaLabel={t("mcpPage.discovery.searchAria")}
        />
        <Button
          type="button"
          variant="quiet"
          size="compact"
          onClick={() =>
            setSelected(new Set(servers.map((server) => server.id)))
          }
        >
          {t("mcpPage.discovery.selectAll")}
        </Button>
        <Button
          type="button"
          variant="quiet"
          size="compact"
          onClick={() => setSelected(new Set())}
        >
          {t("mcpPage.discovery.selectNone")}
        </Button>
      </div>

      <ul className="m-0 max-h-[260px] min-h-0 list-none overflow-y-auto rounded-panel border border-border p-0">
        {visible.map((server, index) => {
          const id = `mcp-imp-${index}`;
          return (
            <li
              key={server.id}
              className={cn(
                "flex items-center gap-2.5 py-2 pe-3 ps-3.5",
                index > 0 && "border-t border-border",
              )}
            >
              <input
                id={id}
                type="checkbox"
                className={CHECKBOX_CLASS}
                checked={selected.has(server.id)}
                onChange={(event) => toggle(server.id, event.target.checked)}
              />
              <label htmlFor={id} className="flex min-w-0 flex-1 flex-col">
                <span className="truncate font-mono text-body font-medium">
                  {server.id}
                </span>
                <span className="truncate text-caption text-fg-2">
                  {t("mcpPage.discovery.foundIn", {
                    apps: server.apps
                      .map(appLabel)
                      .join(t("mcpPage.listSeparator")),
                  })}
                </span>
              </label>
            </li>
          );
        })}
      </ul>

      <div className="flex shrink-0 justify-end gap-2 pt-1">
        <Button
          type="button"
          variant="neutral"
          size="regular"
          disabled={isImporting}
          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="regular"
          disabled={selected.size === 0 || isImporting}
          onClick={() =>
            onImport(
              servers
                .map((server) => server.id)
                .filter((id) => selected.has(id)),
            )
          }
        >
          {t("mcpPage.discovery.submit", { count: selected.size })}
        </Button>
      </div>
    </V7Dialog>
  );
}

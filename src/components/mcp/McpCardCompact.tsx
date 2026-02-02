import React from "react";
import { useTranslation } from "react-i18next";
import { Edit3, Trash2, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import type { McpServer } from "@/types";
import type { AppId } from "@/lib/api/types";
import { settingsApi } from "@/lib/api";
import { mcpPresets } from "@/config/mcpPresets";
import { cn } from "@/lib/utils";

interface McpCardCompactProps {
  id: string;
  server: McpServer;
  onToggleApp: (serverId: string, app: AppId, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
}

export const McpCardCompact: React.FC<McpCardCompactProps> = ({
  id,
  server,
  onToggleApp,
  onEdit,
  onDelete,
}) => {
  const { t } = useTranslation();
  const name = server.name || id;
  const description = server.description || "";

  // Match preset metadata
  const meta = mcpPresets.find((p) => p.id === id);
  const docsUrl = server.docs || meta?.docs;
  const homepageUrl = server.homepage || meta?.homepage;
  const tags = server.tags || meta?.tags;

  const openDocs = async () => {
    const url = docsUrl || homepageUrl;
    if (!url) return;
    try {
      await settingsApi.openExternal(url);
    } catch {
      // ignore
    }
  };

  // Count enabled apps
  const enabledCount = [
    server.apps.claude,
    server.apps.codex,
    server.apps.gemini,
    server.apps.opencode,
  ].filter(Boolean).length;

  return (
    <div className="group relative flex flex-col h-full p-3 rounded-xl border border-border bg-card hover:bg-muted/50 hover:border-border-active hover:shadow-sm transition-all duration-300">
      {/* Header: Name + Actions */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="font-medium text-sm text-foreground truncate">
              {name}
            </h3>
            {(docsUrl || homepageUrl) && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                onClick={openDocs}
                title={t("mcp.presets.docs")}
              >
                <ExternalLink className="h-3 w-3" />
              </Button>
            )}
          </div>
          {description && (
            <p className="text-xs text-muted-foreground line-clamp-2 mt-1">
              {description}
            </p>
          )}
          {!description && tags && tags.length > 0 && (
            <p className="text-xs text-muted-foreground/70 truncate mt-1">
              {tags.slice(0, 3).join(", ")}
            </p>
          )}
        </div>

        {/* Quick actions */}
        <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 w-7 p-0"
            onClick={() => onEdit(id)}
            title={t("common.edit")}
          >
            <Edit3 size={14} />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 w-7 p-0 hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
            onClick={() => onDelete(id)}
            title={t("common.delete")}
          >
            <Trash2 size={14} />
          </Button>
        </div>
      </div>

      {/* Spacer */}
      <div className="flex-1" />

      {/* App toggles - compact grid */}
      <div className="mt-3 pt-2 border-t border-border/50">
        <div className="flex items-center justify-between text-xs text-muted-foreground mb-2">
          <span>{t("mcp.unifiedPanel.enabledApps", { defaultValue: "Enabled" })}</span>
          <span className="font-medium">{enabledCount}/4</span>
        </div>
        <div className="grid grid-cols-2 gap-x-3 gap-y-1.5">
          <AppToggle
            label="Claude"
            checked={server.apps.claude}
            onChange={(checked) => onToggleApp(id, "claude", checked)}
          />
          <AppToggle
            label="Codex"
            checked={server.apps.codex}
            onChange={(checked) => onToggleApp(id, "codex", checked)}
          />
          <AppToggle
            label="Gemini"
            checked={server.apps.gemini}
            onChange={(checked) => onToggleApp(id, "gemini", checked)}
          />
          <AppToggle
            label="OpenCode"
            checked={server.apps.opencode}
            onChange={(checked) => onToggleApp(id, "opencode", checked)}
          />
        </div>
      </div>
    </div>
  );
};

// Compact app toggle component
interface AppToggleProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

const AppToggle: React.FC<AppToggleProps> = ({ label, checked, onChange }) => {
  return (
    <div className="flex items-center justify-between">
      <span
        className={cn(
          "text-xs",
          checked ? "text-foreground" : "text-muted-foreground"
        )}
      >
        {label}
      </span>
      <Switch
        checked={checked}
        onCheckedChange={onChange}
        className="scale-75 origin-right"
      />
    </div>
  );
};

export default McpCardCompact;

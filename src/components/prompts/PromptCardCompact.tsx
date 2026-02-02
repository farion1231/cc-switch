import React from "react";
import { useTranslation } from "react-i18next";
import { Edit3, Trash2, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

interface Prompt {
  name: string;
  description?: string;
  content: string;
  enabled: boolean;
}

interface PromptCardCompactProps {
  id: string;
  prompt: Prompt;
  onToggle: (id: string, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
}

export const PromptCardCompact: React.FC<PromptCardCompactProps> = ({
  id,
  prompt,
  onToggle,
  onEdit,
  onDelete,
}) => {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        "group relative flex flex-col h-full p-3 rounded-xl border bg-card hover:bg-muted/50 hover:shadow-sm transition-all duration-300",
        prompt.enabled
          ? "border-green-500/50 bg-green-500/5"
          : "border-border hover:border-border-active",
      )}
    >
      {/* Header: Name + Actions */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="font-medium text-sm text-foreground truncate">
              {prompt.name}
            </h3>
            {prompt.enabled && (
              <span className="flex items-center gap-1 text-xs text-green-600 dark:text-green-400">
                <Check size={12} />
                {t("prompts.enabled")}
              </span>
            )}
          </div>
          {prompt.description && (
            <p className="text-xs text-muted-foreground line-clamp-2 mt-1">
              {prompt.description}
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

      {/* Content preview */}
      <div className="flex-1 mt-2">
        <p className="text-xs text-muted-foreground/70 line-clamp-3 font-mono">
          {prompt.content.slice(0, 150)}
          {prompt.content.length > 150 && "..."}
        </p>
      </div>

      {/* Footer: Enable toggle */}
      <div className="mt-3 pt-2 border-t border-border/50">
        <div className="flex items-center justify-between">
          <span className="text-xs text-muted-foreground">
            {t("prompts.enable")}
          </span>
          <Switch
            checked={prompt.enabled}
            onCheckedChange={(checked) => onToggle(id, checked)}
            className="scale-75 origin-right"
          />
        </div>
      </div>
    </div>
  );
};

export default PromptCardCompact;

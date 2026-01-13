import React from "react";
import { useTranslation } from "react-i18next";
import { Edit3, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { Prompt } from "@/lib/api";
import PromptToggle from "./PromptToggle";

interface PromptListItemProps {
  id: string;
  prompt: Prompt;
  onToggle: (id: string, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
}

const PromptListItem: React.FC<PromptListItemProps> = ({
  id,
  prompt,
  onToggle,
  onEdit,
  onDelete,
}) => {
  const { t } = useTranslation();

  const enabled = prompt.enabled === true;

  return (
    <div className="group relative h-16 rounded-xl border border-border-default bg-muted/50 p-4 transition-all duration-300 hover:bg-muted hover:border-border-default/80 hover:shadow-sm">
      <div className="flex items-center gap-4 h-full">
        {/* Toggle 开关 */}
        <div className="flex-shrink-0">
          <PromptToggle
            enabled={enabled}
            onChange={(newEnabled) => onToggle(id, newEnabled)}
          />
        </div>

        <div className="flex-1 min-w-0">
          <h3 className="font-medium text-foreground mb-1">{prompt.name}</h3>
          {prompt.description && (
            <p className="text-sm text-muted-foreground truncate">
              {prompt.description}
            </p>
          )}
        </div>

        <div className="flex items-center gap-2 flex-shrink-0">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={() => onEdit(id)}
            title={t("common.edit")}
          >
            <Edit3 size={16} />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={() => onDelete(id)}
            className="hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
            title={t("common.delete")}
          >
            <Trash2 size={16} />
          </Button>
        </div>
      </div>
    </div>
  );
};

export default PromptListItem;

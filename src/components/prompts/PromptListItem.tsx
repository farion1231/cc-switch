import React from "react";
import { useTranslation } from "react-i18next";
import { Edit3, Trash2, Circle, CheckCircle2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { Prompt } from "@/lib/api";

interface PromptListItemProps {
  id: string;
  prompt: Prompt;
  onEnable: (id: string) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
}

const PromptListItem: React.FC<PromptListItemProps> = ({
  id,
  prompt,
  onEnable,
  onEdit,
  onDelete,
}) => {
  const { t } = useTranslation();

  return (
    <div className="h-16 rounded-lg border border-border-default bg-card p-4 transition-[border-color,box-shadow] duration-200 hover:border-border-hover hover:shadow-sm">
      <div className="flex items-center gap-4 h-full">
        <button
          onClick={() => !prompt.enabled && onEnable(id)}
          className="flex-shrink-0"
          disabled={prompt.enabled}
          title={prompt.enabled ? t("prompts.enabled") : t("prompts.enable")}
        >
          {prompt.enabled ? (
            <CheckCircle2 size={20} className="text-blue-500" />
          ) : (
            <Circle
              size={20}
              className="text-gray-400 hover:text-blue-500 cursor-pointer"
            />
          )}
        </button>

        <div className="flex-1 min-w-0">
          <h3 className="font-medium text-gray-900 dark:text-gray-100 mb-1">
            {prompt.name}
          </h3>
          {prompt.description && (
            <p className="text-sm text-gray-500 dark:text-gray-400 truncate">
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
            disabled={prompt.enabled}
            className="hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10 disabled:opacity-50"
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

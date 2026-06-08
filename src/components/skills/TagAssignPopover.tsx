import { useState, useMemo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Tag, Check, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  useSkillTags,
  useAllTagAssignments,
  useSetSkillTags,
  useCreateTag,
} from "@/hooks/useSkills";
import { toast } from "sonner";

interface TagAssignPopoverProps {
  skillId: string;
}

export function TagAssignPopover({ skillId }: TagAssignPopoverProps) {
  const { t } = useTranslation();
  const [newTagName, setNewTagName] = useState("");

  const { data: allTags = [] } = useSkillTags();
  const { data: rawAssignments = [] } = useAllTagAssignments();
  const setSkillTagsMutation = useSetSkillTags();
  const createTagMutation = useCreateTag();

  // 将 [skillId, tagId][] 转换为 Record<skillId, tagId[]>
  const assignmentsMap = useMemo(() => {
    const map: Record<string, number[]> = {};
    for (const [sid, tid] of rawAssignments) {
      if (!map[sid]) map[sid] = [];
      map[sid].push(tid);
    }
    return map;
  }, [rawAssignments]);

  // 当前技能已分配的标签 ID 集合
  const assignedTagIds = useMemo(() => {
    const ids = assignmentsMap[skillId] ?? [];
    return new Set<number>(ids);
  }, [assignmentsMap, skillId]);

  // 切换标签分配（单选：每个技能只属于一个分组）
  const handleToggleTag = useCallback(
    async (tagId: number) => {
      const isAssigned = assignedTagIds.has(tagId);
      // 取消勾选 → 清空分组；勾选 → 替换为当前分组
      const newTagIds = isAssigned ? [] : [tagId];

      try {
        await setSkillTagsMutation.mutateAsync({
          skillId,
          tagIds: newTagIds,
        });
      } catch (error) {
        toast.error(t("common.error"), { description: String(error) });
      }
    },
    [assignedTagIds, skillId, setSkillTagsMutation, t],
  );

  // 创建新标签并分配（替换现有分组）
  const handleCreateAndAssign = useCallback(async () => {
    const name = newTagName.trim();
    if (!name) return;

    try {
      const newTag = await createTagMutation.mutateAsync(name);
      await setSkillTagsMutation.mutateAsync({
        skillId,
        tagIds: [newTag.id],
      });
      setNewTagName("");
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  }, [newTagName, createTagMutation, setSkillTagsMutation, skillId, t]);

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="relative text-muted-foreground/60 hover:text-foreground flex-shrink-0"
          title={t("skills.tags.assignTag")}
        >
          <Tag size={12} />
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-60 overflow-hidden p-0">
        <div className="border-b border-border-default px-3 py-2">
          <p className="text-xs font-medium text-muted-foreground">
            {t("skills.tags.assignTag")}
          </p>
        </div>

        <div className="max-h-52 overflow-y-auto p-1.5 [scrollbar-width:thin]">
          <div className="space-y-0.5">
            {allTags.map((tag) => {
              const isChecked = assignedTagIds.has(tag.id);
              return (
                <button
                  key={tag.id}
                  type="button"
                  onClick={() => handleToggleTag(tag.id)}
                  className={`flex h-9 w-full items-center gap-2 rounded-md px-2.5 text-left text-sm transition-colors ${
                    isChecked
                      ? "bg-blue-500/10 text-foreground"
                      : "text-muted-foreground hover:bg-muted/50 hover:text-foreground"
                  }`}
                >
                  <div
                    className={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full border transition-colors ${
                      isChecked
                        ? "border-blue-500 bg-blue-500 text-white"
                        : "border-muted-foreground/30"
                    }`}
                  >
                    {isChecked && <Check className="h-3 w-3" />}
                  </div>
                  <span className="truncate">{tag.name}</span>
                </button>
              );
            })}
          </div>

          {allTags.length === 0 && (
            <p className="py-6 text-center text-xs text-muted-foreground">
              {t("skills.tags.noTags")}
            </p>
          )}
        </div>

        {/* 快速创建标签 */}
        <div className="flex gap-1.5 border-t border-border-default bg-muted/10 p-2">
          <input
            type="text"
            value={newTagName}
            onChange={(e) => setNewTagName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                handleCreateAndAssign();
              }
            }}
            placeholder={t("skills.tags.tagNamePlaceholder")}
            className="h-8 min-w-0 flex-1 rounded-md border border-border-default bg-background px-2.5 text-xs text-foreground outline-none transition-colors placeholder:text-muted-foreground focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20"
          />
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0 cursor-pointer"
            onClick={handleCreateAndAssign}
            disabled={!newTagName.trim()}
            title={t("skills.tags.create")}
          >
            <Plus className="h-3.5 w-3.5" />
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}

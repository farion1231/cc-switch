import { useState, useMemo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Tag, Check, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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
  skillName: string;
}

export function TagAssignPopover({
  skillId,
  skillName,
}: TagAssignPopoverProps) {
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

  // 切换标签分配
  const handleToggleTag = useCallback(
    async (tagId: number) => {
      const isAssigned = assignedTagIds.has(tagId);
      const idsArray = Array.from(assignedTagIds);
      const newTagIds = isAssigned
        ? idsArray.filter((id) => id !== tagId)
        : [...idsArray, tagId];

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

  // 创建新标签并分配
  const handleCreateAndAssign = useCallback(async () => {
    const name = newTagName.trim();
    if (!name) return;

    try {
      const newTag = await createTagMutation.mutateAsync(name);
      const newTagIds = [...Array.from(assignedTagIds), newTag.id];
      await setSkillTagsMutation.mutateAsync({
        skillId,
        tagIds: newTagIds,
      });
      setNewTagName("");
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  }, [
    newTagName,
    createTagMutation,
    assignedTagIds,
    setSkillTagsMutation,
    skillId,
    t,
  ]);

  const tagCount = assignedTagIds.size;

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="relative text-muted-foreground/60 hover:text-foreground flex-shrink-0"
          title={t("skills.assignTags")}
        >
          <Tag size={12} />
          {tagCount > 0 && (
            <Badge
              variant="secondary"
              className="absolute -top-1.5 -right-1.5 h-3.5 min-w-3.5 px-0.5 text-[9px] leading-none"
            >
              {tagCount}
            </Badge>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-56">
        <div className="space-y-2">
          <p className="text-xs font-medium text-muted-foreground">
            {t("skills.tagsFor", { name: skillName })}
          </p>

          {/* 标签列表 */}
          <div className="max-h-48 overflow-y-auto space-y-1">
            {allTags.map((tag) => {
              const isChecked = assignedTagIds.has(tag.id);
              return (
                <button
                  key={tag.id}
                  type="button"
                  onClick={() => handleToggleTag(tag.id)}
                  className="flex items-center gap-2 w-full px-2 py-1.5 text-sm rounded-sm hover:bg-accent transition-colors text-left"
                >
                  <div
                    className={`flex h-4 w-4 items-center justify-center rounded-sm border ${
                      isChecked
                        ? "bg-primary border-primary text-primary-foreground"
                        : "border-muted-foreground/30"
                    }`}
                  >
                    {isChecked && <Check className="h-3 w-3" />}
                  </div>
                  <span className="truncate">{tag.name}</span>
                </button>
              );
            })}

            {allTags.length === 0 && (
              <p className="text-xs text-muted-foreground py-2 text-center">
                {t("skills.noTags")}
              </p>
            )}
          </div>

          {/* 快速创建标签 */}
          <div className="flex gap-1 pt-1 border-t">
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
              placeholder={t("skills.newTagPlaceholder")}
              className="flex-1 h-7 text-xs px-2 rounded-sm border bg-transparent outline-none focus:ring-1 focus:ring-ring"
            />
            <Button
              variant="ghost"
              size="sm"
              className="h-7 w-7 p-0"
              onClick={handleCreateAndAssign}
              disabled={!newTagName.trim()}
            >
              <Plus className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { V7Dialog } from "@/components/mcp/formBits";
import type { InstalledSkill, SkillProjectGroup } from "@/lib/api/skills";
import { isLocalSkill } from "./skillGroups";

export function SkillProjectGroupDialog({
  group,
  skills,
  projects,
  initialIds,
  pending,
  onSave,
  onDelete,
  onClose,
}: {
  group?: SkillProjectGroup;
  skills: InstalledSkill[];
  projects: SkillProjectGroup[];
  initialIds: string[];
  pending: boolean;
  onSave: (name: string, ids: string[]) => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(group?.name ?? "");
  const [query, setQuery] = useState("");
  const [chosen, setChosen] = useState(new Set(group?.skillIds ?? initialIds));
  const matches = skills
    .filter(
      (skill) => isLocalSkill(skill) || group?.skillIds.includes(skill.id),
    )
    .filter((skill) =>
      `${skill.name} ${skill.directory}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
    );
  return (
    <V7Dialog
      open
      width={560}
      onOpenChange={(open) => {
        if (!open && !pending) onClose();
      }}
    >
      <div>
        <DialogTitle className="text-section">
          {t(group ? "skillsPage.groups.edit" : "skillsPage.groups.create")}
        </DialogTitle>
        <DialogDescription className="text-caption text-fg-2">
          {t("skillsPage.groups.description")}
        </DialogDescription>
      </div>
      <label className="flex flex-col gap-2 text-body">
        {t("skillsPage.groups.name")}
        <Input
          value={name}
          onChange={(e) => setName(e.target.value)}
          disabled={pending}
        />
      </label>
      <Input
        aria-label={t("skillsPage.groups.search")}
        placeholder={t("skillsPage.groups.search")}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        disabled={pending}
      />
      <div className="max-h-[320px] overflow-auto rounded-panel border border-border p-2">
        {matches.length === 0 && (
          <p className="px-2 text-caption text-fg-2">
            {t("skillsPage.groups.noLocal")}
          </p>
        )}
        {matches.map((skill) => {
          const previous = projects.find(
            (project) =>
              project.id !== group?.id && project.skillIds.includes(skill.id),
          );
          return (
            <label
              key={skill.id}
              className="flex cursor-pointer items-center gap-2 rounded-control p-2 text-body hover:bg-subtle"
            >
              <Checkbox
                checked={chosen.has(skill.id)}
                disabled={pending}
                onCheckedChange={(checked) =>
                  setChosen((prev) => {
                    const next = new Set(prev);
                    if (checked) next.add(skill.id);
                    else next.delete(skill.id);
                    return next;
                  })
                }
              />
              <span className="min-w-0 flex-1 truncate">{skill.name}</span>
              {previous && (
                <span className="text-caption text-fg-2">
                  {t("skillsPage.groups.moveFrom", { name: previous.name })}
                </span>
              )}
            </label>
          );
        })}
      </div>
      <div className="flex items-center gap-2">
        {group && (
          <Button
            variant="quiet"
            size="regular"
            disabled={pending}
            onClick={onDelete}
          >
            {t("skillsPage.groups.dissolve")}
          </Button>
        )}
        <div className="flex-1" />
        <Button
          variant="neutral"
          size="regular"
          disabled={pending}
          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
        <Button
          size="regular"
          disabled={pending || !name.trim()}
          onClick={() => onSave(name.trim(), [...chosen])}
        >
          {t("common.save")}
        </Button>
      </div>
    </V7Dialog>
  );
}

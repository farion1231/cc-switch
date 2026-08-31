import React from "react";
import { useTranslation } from "react-i18next";
import { Search } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  InstalledSkill,
  SkillGroup,
  SkillGroupColor,
} from "@/lib/api/skills";
import { SKILL_GROUP_COLORS } from "@/lib/api/skills";
import { cn } from "@/lib/utils";
import { SKILL_GROUP_COLOR_STYLES } from "./SkillGroupSection";

function matchesSkill(skill: InstalledSkill, query: string) {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return true;
  return [
    skill.name,
    skill.description,
    skill.directory,
    skill.repoOwner,
    skill.repoName,
  ].some((value) => value?.toLocaleLowerCase().includes(normalized));
}

interface SkillChecklistProps {
  skills: InstalledSkill[];
  groups: SkillGroup[];
  selected: Set<string>;
  onSelectedChange: (selected: Set<string>) => void;
}

function SkillChecklist({
  skills,
  groups,
  selected,
  onSelectedChange,
}: SkillChecklistProps) {
  const { t } = useTranslation();
  const [query, setQuery] = React.useState("");
  const groupNames = React.useMemo(
    () => new Map(groups.map((group) => [group.id, group.name])),
    [groups],
  );
  const filtered = skills.filter((skill) => matchesSkill(skill, query));

  const toggle = (id: string, checked: boolean) => {
    const next = new Set(selected);
    if (checked) next.add(id);
    else next.delete(id);
    onSelectedChange(next);
  };

  return (
    <div className="space-y-3">
      <div className="relative">
        <Search
          size={14}
          className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground"
        />
        <Input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="h-8 pl-8"
          placeholder={t("skills.groups.searchMembers")}
          aria-label={t("skills.groups.searchMembers")}
        />
      </div>
      <div className="max-h-64 overflow-y-auto rounded-lg border border-border-default">
        {filtered.length === 0 ? (
          <p className="px-4 py-8 text-center text-sm text-muted-foreground">
            {t("skills.groups.noMembersFound")}
          </p>
        ) : (
          filtered.map((skill, index) => (
            <label
              key={skill.id}
              className={cn(
                "flex cursor-pointer items-center gap-3 px-3 py-2.5 hover:bg-muted/50",
                index !== filtered.length - 1 &&
                  "border-b border-border-default",
              )}
            >
              <Checkbox
                checked={selected.has(skill.id)}
                onCheckedChange={(checked) => toggle(skill.id, checked)}
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium text-foreground">
                  {skill.name}
                </span>
                <span className="block truncate text-xs text-muted-foreground">
                  {skill.groupId
                    ? (groupNames.get(skill.groupId) ??
                      t("skills.groups.ungrouped"))
                    : t("skills.groups.ungrouped")}
                </span>
              </span>
            </label>
          ))
        )}
      </div>
    </div>
  );
}

interface CreateSkillGroupDialogProps {
  open: boolean;
  skills: InstalledSkill[];
  groups: SkillGroup[];
  defaultColor: SkillGroupColor;
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (input: {
    name: string;
    color: SkillGroupColor;
    skillIds: string[];
  }) => Promise<void>;
}

export function CreateSkillGroupDialog({
  open,
  skills,
  groups,
  defaultColor,
  pending,
  onOpenChange,
  onCreate,
}: CreateSkillGroupDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = React.useState("");
  const [color, setColor] = React.useState<SkillGroupColor>(defaultColor);
  const [selected, setSelected] = React.useState<Set<string>>(new Set());
  const [error, setError] = React.useState<string>();

  React.useEffect(() => {
    if (!open) return;
    setName("");
    setColor(defaultColor);
    setSelected(new Set());
    setError(undefined);
  }, [defaultColor, open]);

  const submit = async () => {
    const normalized = name.trim();
    if (
      Array.from(normalized).length < 1 ||
      Array.from(normalized).length > 50
    ) {
      setError(t("skills.groups.nameLengthError"));
      return;
    }
    if (
      groups.some(
        (group) =>
          group.name.localeCompare(normalized, undefined, {
            sensitivity: "accent",
          }) === 0,
      )
    ) {
      setError(t("skills.groups.nameDuplicateError"));
      return;
    }
    try {
      await onCreate({ name: normalized, color, skillIds: [...selected] });
    } catch {
      setError(t("skills.groups.createFailed"));
    }
  };

  const movedCount = skills.filter(
    (skill) => selected.has(skill.id) && Boolean(skill.groupId),
  ).length;

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-xl" zIndex="alert">
        <DialogHeader>
          <DialogTitle>{t("skills.groups.createTitle")}</DialogTitle>
          <DialogDescription>
            {t("skills.groups.createDescription")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-5 px-6 py-5">
          <div className="space-y-2">
            <label className="text-sm font-medium" htmlFor="skill-group-name">
              {t("skills.groups.name")}
            </label>
            <ImeSafeInput
              id="skill-group-name"
              value={name}
              onValueChange={(value) => {
                setName(value);
                setError(undefined);
              }}
              maxLength={50}
              placeholder={t("skills.groups.namePlaceholder")}
              aria-invalid={Boolean(error)}
              autoFocus
            />
            {error && <p className="text-xs text-destructive">{error}</p>}
          </div>

          <div className="space-y-2">
            <span className="text-sm font-medium">
              {t("skills.groups.color")}
            </span>
            <div className="flex flex-wrap gap-2" role="radiogroup">
              {SKILL_GROUP_COLORS.map((paletteColor) => (
                <button
                  key={paletteColor}
                  type="button"
                  role="radio"
                  aria-checked={color === paletteColor}
                  aria-label={t(`skills.groups.colors.${paletteColor}`)}
                  onClick={() => setColor(paletteColor)}
                  className={cn(
                    "flex h-8 w-8 items-center justify-center rounded-full border border-transparent transition-transform hover:scale-105 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    color === paletteColor &&
                      "border-foreground/30 bg-muted ring-2 ring-ring",
                  )}
                >
                  <span
                    className={cn(
                      "h-4 w-4 rounded-full",
                      SKILL_GROUP_COLOR_STYLES[paletteColor].dot,
                    )}
                  />
                </button>
              ))}
            </div>
          </div>

          <div className="space-y-2">
            <div className="flex items-center justify-between gap-3">
              <span className="text-sm font-medium">
                {t("skills.groups.initialMembers")}
              </span>
              <span className="text-xs text-muted-foreground">
                {t("skills.groups.selectedCount", { count: selected.size })}
              </span>
            </div>
            <SkillChecklist
              skills={skills}
              groups={groups}
              selected={selected}
              onSelectedChange={setSelected}
            />
            {movedCount > 0 && (
              <p className="text-xs text-amber-600 dark:text-amber-400">
                {t("skills.groups.crossGroupWarning", { count: movedCount })}
              </p>
            )}
          </div>
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={pending}
          >
            {t("common.cancel")}
          </Button>
          <Button onClick={() => void submit()} disabled={pending}>
            {t("skills.groups.create")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface ManageSkillGroupMembersDialogProps {
  open: boolean;
  group: SkillGroup;
  skills: InstalledSkill[];
  groups: SkillGroup[];
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onSave: (skillIds: string[]) => Promise<void>;
}

export function ManageSkillGroupMembersDialog({
  open,
  group,
  skills,
  groups,
  pending,
  onOpenChange,
  onSave,
}: ManageSkillGroupMembersDialogProps) {
  const { t } = useTranslation();
  const [selected, setSelected] = React.useState<Set<string>>(new Set());
  const [error, setError] = React.useState<string>();

  React.useEffect(() => {
    if (!open) return;
    setSelected(
      new Set(
        skills
          .filter((skill) => skill.groupId === group.id)
          .map((skill) => skill.id),
      ),
    );
    setError(undefined);
  }, [group.id, open, skills]);

  const movedCount = skills.filter(
    (skill) =>
      selected.has(skill.id) && skill.groupId && skill.groupId !== group.id,
  ).length;

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-xl" zIndex="alert">
        <DialogHeader>
          <DialogTitle>
            {t("skills.groups.manageMembersTitle", { name: group.name })}
          </DialogTitle>
          <DialogDescription>
            {t("skills.groups.manageMembersDescription", {
              count: selected.size,
            })}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 px-6 py-5">
          <SkillChecklist
            skills={skills}
            groups={groups}
            selected={selected}
            onSelectedChange={setSelected}
          />
          {movedCount > 0 && (
            <p className="text-xs text-amber-600 dark:text-amber-400">
              {t("skills.groups.crossGroupWarning", { count: movedCount })}
            </p>
          )}
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={pending}
          >
            {t("common.cancel")}
          </Button>
          <Button
            disabled={pending}
            onClick={async () => {
              try {
                await onSave([...selected]);
              } catch {
                setError(t("skills.groups.membersSaveFailed"));
              }
            }}
          >
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface OrganizeUngroupedDialogProps {
  open: boolean;
  skills: InstalledSkill[];
  groups: SkillGroup[];
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onMove: (skillIds: string[], groupId: string) => Promise<void>;
}

export function OrganizeUngroupedDialog({
  open,
  skills,
  groups,
  pending,
  onOpenChange,
  onMove,
}: OrganizeUngroupedDialogProps) {
  const { t } = useTranslation();
  const ungrouped = skills.filter((skill) => !skill.groupId);
  const [selected, setSelected] = React.useState<Set<string>>(new Set());
  const [targetGroupId, setTargetGroupId] = React.useState("");
  const [error, setError] = React.useState<string>();

  React.useEffect(() => {
    if (!open) return;
    setSelected(new Set());
    setTargetGroupId(groups[0]?.id ?? "");
    setError(undefined);
  }, [groups, open]);

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-xl" zIndex="alert">
        <DialogHeader>
          <DialogTitle>{t("skills.groups.organizeUngrouped")}</DialogTitle>
          <DialogDescription>
            {t("skills.groups.organizeDescription")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4 px-6 py-5">
          <div className="space-y-2">
            <span className="text-sm font-medium">
              {t("skills.groups.targetGroup")}
            </span>
            <Select value={targetGroupId} onValueChange={setTargetGroupId}>
              <SelectTrigger>
                <SelectValue
                  placeholder={t("skills.groups.selectTargetGroup")}
                />
              </SelectTrigger>
              <SelectContent>
                {groups.map((group) => (
                  <SelectItem key={group.id} value={group.id}>
                    {group.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <SkillChecklist
            skills={ungrouped}
            groups={groups}
            selected={selected}
            onSelectedChange={setSelected}
          />
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={pending}
          >
            {t("common.cancel")}
          </Button>
          <Button
            disabled={pending || selected.size === 0 || !targetGroupId}
            onClick={async () => {
              try {
                await onMove([...selected], targetGroupId);
              } catch {
                setError(t("skills.groups.moveFailed"));
              }
            }}
          >
            {t("skills.groups.moveSelected", { count: selected.size })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

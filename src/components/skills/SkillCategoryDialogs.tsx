import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { DialogTitle } from "@/components/ui/dialog";
import {
  FIELD_CLASS,
  V7ConfirmDialog,
  V7Dialog,
} from "@/components/mcp/formBits";
import type { InstalledSkill, SkillCategory } from "@/lib/api/skills";
import type { SkillCategoryAction } from "@/hooks/useSkills";

export function SkillCategoryManager({
  categories,
  skills,
  pending,
  onAction,
  onClose,
}: {
  categories: SkillCategory[];
  skills: InstalledSkill[];
  pending: boolean;
  onAction: (action: SkillCategoryAction) => Promise<boolean>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<SkillCategory | null>(null);
  const trimmed = name.trim();
  const duplicate = categories.some(
    (category) =>
      category.id !== editingId &&
      category.name.toLowerCase() === trimmed.toLowerCase(),
  );
  const valid =
    [...trimmed].length >= 1 && [...trimmed].length <= 50 && !duplicate;
  const reset = () => {
    setName("");
    setEditingId(null);
  };
  return (
    <>
      <V7Dialog
        open
        onOpenChange={(open) => {
          if (!open && !pending) onClose();
        }}
      >
        <DialogTitle>{t("skillsPage.categories.manage")}</DialogTitle>
        <p className="m-0 text-body text-fg-2">
          {t("skillsPage.categories.help")}
        </p>
        <form
          className="flex flex-col gap-2"
          onSubmit={async (event) => {
            event.preventDefault();
            if (!valid || pending) return;
            const ok = await onAction(
              editingId
                ? { kind: "rename", id: editingId, name: trimmed }
                : { kind: "create", name: trimmed },
            );
            if (ok) reset();
          }}
        >
          <label
            htmlFor="skill-category-name"
            className="text-body font-medium"
          >
            {t("skillsPage.categories.name")}
          </label>
          <div className="flex gap-2">
            <input
              id="skill-category-name"
              className={FIELD_CLASS}
              value={name}
              disabled={pending}
              aria-invalid={duplicate || [...trimmed].length > 50}
              aria-describedby="skill-category-name-hint"
              onChange={(event) => setName(event.target.value)}
            />
            <Button
              type="submit"
              variant="solid"
              size="compact"
              disabled={!valid || pending}
            >
              {t(editingId ? "common.save" : "skillsPage.categories.create")}
            </Button>
            {editingId && (
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={pending}
                onClick={reset}
              >
                {t("common.cancel")}
              </Button>
            )}
          </div>
          <p
            id="skill-category-name-hint"
            className={
              duplicate
                ? "m-0 text-caption text-danger-text"
                : "m-0 text-caption text-fg-2"
            }
          >
            {t(
              duplicate
                ? "skillsPage.categories.duplicate"
                : "skillsPage.categories.nameHint",
            )}
          </p>
        </form>
        <ul className="m-0 max-h-64 list-none overflow-y-auto p-0">
          {categories.map((category) => (
            <li
              key={category.id}
              className="flex items-center gap-2 border-t border-border py-2"
            >
              <span className="min-w-0 flex-1 truncate" title={category.name}>
                {category.name}
              </span>
              <span className="text-caption text-fg-2">
                {
                  skills.filter((skill) => skill.categoryId === category.id)
                    .length
                }
              </span>
              <Button
                type="button"
                variant="quiet"
                size="compact"
                disabled={pending}
                aria-label={t("skillsPage.categories.renameAria", {
                  name: category.name,
                })}
                onClick={() => {
                  setEditingId(category.id);
                  setName(category.name);
                  document.getElementById("skill-category-name")?.focus();
                }}
              >
                {t("skillsPage.categories.rename")}
              </Button>
              <Button
                type="button"
                variant="quiet"
                size="compact"
                disabled={pending}
                aria-label={t("skillsPage.categories.deleteAria", {
                  name: category.name,
                })}
                className="text-danger-text"
                onClick={() => setDeleting(category)}
              >
                {t("common.delete")}
              </Button>
            </li>
          ))}
          {categories.length === 0 && (
            <li className="py-4 text-body text-fg-2">
              {t("skillsPage.categories.empty")}
            </li>
          )}
        </ul>
        <div className="flex justify-end">
          <Button
            type="button"
            variant="neutral"
            size="regular"
            disabled={pending}
            onClick={onClose}
          >
            {t("common.close")}
          </Button>
        </div>
      </V7Dialog>
      <V7ConfirmDialog
        open={deleting !== null}
        title={t("skillsPage.categories.deleteTitle", { name: deleting?.name })}
        body={t("skillsPage.categories.deleteBody", {
          count: skills.filter((skill) => skill.categoryId === deleting?.id)
            .length,
        })}
        confirmLabel={t("common.delete")}
        pending={pending}
        onCancel={() => setDeleting(null)}
        onConfirm={async () => {
          if (
            deleting &&
            (await onAction({ kind: "delete", id: deleting.id }))
          ) {
            if (editingId === deleting.id) reset();
            setDeleting(null);
          }
        }}
      />
    </>
  );
}

export function SkillCategoryPicker({
  categories,
  currentId,
  count,
  pending,
  onAssign,
  onClose,
}: {
  categories: SkillCategory[];
  currentId: string | null;
  count: number;
  pending: boolean;
  onAssign: (id: string | null) => Promise<boolean>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [value, setValue] = useState(currentId ?? "");
  return (
    <V7Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) onClose();
      }}
    >
      <DialogTitle>{t("skillsPage.categories.assign")}</DialogTitle>
      <p className="m-0 text-body text-fg-2">
        {t("skillsPage.categories.assignCount", { count })}
      </p>
      <label htmlFor="skill-category-choice">
        {t("skillsPage.categories.label")}
      </label>
      <select
        id="skill-category-choice"
        className={FIELD_CLASS}
        disabled={pending}
        value={value}
        onChange={(event) => setValue(event.target.value)}
      >
        <option value="">{t("skillsPage.categories.uncategorized")}</option>
        {categories.map((category) => (
          <option key={category.id} value={category.id}>
            {category.name}
          </option>
        ))}
      </select>
      <div className="flex justify-end gap-2">
        <Button
          type="button"
          variant="neutral"
          size="regular"
          disabled={pending}
          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="regular"
          disabled={pending}
          onClick={async () => {
            if (await onAssign(value || null)) onClose();
          }}
        >
          {t("common.save")}
        </Button>
      </div>
    </V7Dialog>
  );
}

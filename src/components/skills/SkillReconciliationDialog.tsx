import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { CHECKBOX_CLASS, V7Dialog } from "@/components/mcp/formBits";
import { cn } from "@/lib/utils";
import type { SkillReconciliationCandidate } from "@/lib/api/skills";

interface SkillReconciliationDialogProps {
  candidates: SkillReconciliationCandidate[];
  isRemoving: boolean;
  onRemove: (candidates: SkillReconciliationCandidate[]) => void;
  onClose: () => void;
}

export function SkillReconciliationDialog({
  candidates,
  isRemoving,
  onRemove,
  onClose,
}: SkillReconciliationDialogProps) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(
    () =>
      new Set(
        candidates.map(
          (candidate) => `${candidate.app}:${candidate.directory}`,
        ),
      ),
  );

  const keyOf = (candidate: SkillReconciliationCandidate) =>
    `${candidate.app}:${candidate.directory}`;

  const toggle = (key: string, checked: boolean) =>
    setSelected((current) => {
      const next = new Set(current);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });

  return (
    <V7Dialog
      open
      width={560}
      onOpenChange={(open) => {
        if (!open && !isRemoving) onClose();
      }}
    >
      <div className="flex shrink-0 flex-col gap-1">
        <DialogTitle className="text-section">
          {t("skillsPage.reconciliation.title")}
        </DialogTitle>
        <DialogDescription className="text-caption text-fg-2">
          {t("skillsPage.reconciliation.lead")}
        </DialogDescription>
      </div>

      <div className="flex shrink-0 justify-end gap-2">
        <Button
          type="button"
          variant="quiet"
          size="compact"
          onClick={() => setSelected(new Set(candidates.map(keyOf)))}
        >
          {t("skillsPage.import.selectAll")}
        </Button>
        <Button
          type="button"
          variant="quiet"
          size="compact"
          onClick={() => setSelected(new Set())}
        >
          {t("skillsPage.import.selectNone")}
        </Button>
      </div>

      <ul className="m-0 max-h-[260px] min-h-0 list-none overflow-y-auto rounded-panel border border-border p-0">
        {candidates.map((candidate, index) => (
          <li
            key={keyOf(candidate)}
            className={cn(
              "flex items-center gap-2.5 py-2 pe-3 ps-3.5",
              index > 0 && "border-t border-border",
            )}
          >
            <input
              type="checkbox"
              className={CHECKBOX_CLASS}
              checked={selected.has(keyOf(candidate))}
              onChange={(event) =>
                toggle(keyOf(candidate), event.target.checked)
              }
            />
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-body font-medium">
                {candidate.name}
              </span>
              <span className="truncate font-mono text-caption text-fg-3">
                {APP_DISPLAY_NAME[
                  candidate.app as keyof typeof APP_DISPLAY_NAME
                ] ?? candidate.app}
                {" · "}
                {candidate.directory}
              </span>
              <span
                className="truncate text-caption text-fg-2"
                title={candidate.path}
              >
                {t("skillsPage.reconciliation.path", { path: candidate.path })}
              </span>
            </span>
          </li>
        ))}
      </ul>

      <div className="flex shrink-0 justify-end gap-2 pt-1">
        <Button
          type="button"
          variant="neutral"
          size="regular"
          disabled={isRemoving}
          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="regular"
          disabled={selected.size === 0 || isRemoving}
          onClick={() =>
            onRemove(
              candidates.filter((candidate) => selected.has(keyOf(candidate))),
            )
          }
        >
          {t("skillsPage.reconciliation.remove", { count: selected.size })}
        </Button>
      </div>
    </V7Dialog>
  );
}

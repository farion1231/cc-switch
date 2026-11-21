import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useProxyStack, useSetProxyStackRoleBindings } from "@/lib/query/proxy";
import {
  STACK_ROLES,
  type StackRole,
  type StackRoleBinding,
  type StackRoleBindings,
} from "@/types/proxy";

/** Select 里「跟随默认」那项的值；档位绑定用 U+001F 分隔 providerId 和 modelId */
const DEFAULT_VALUE = "__follow_default__";
const SEP = "\u001f";

/** 一档的选择：`null` 表示跟随当前默认供应商 */
type RolePick = StackRoleBinding | null;
type RoleDraft = Record<StackRole, RolePick>;

const emptyDraft = (): RoleDraft => ({
  haiku: null,
  sonnet: null,
  opus: null,
  fable: null,
});

const encodePick = (pick: RolePick): string =>
  pick ? `${pick.providerId}${SEP}${pick.modelId}` : DEFAULT_VALUE;

const samePick = (a: RolePick, b: RolePick): boolean =>
  (a === null && b === null) ||
  (a !== null &&
    b !== null &&
    a.providerId === b.providerId &&
    a.modelId === b.modelId);

const roleLabel = (role: StackRole): string =>
  role.charAt(0).toUpperCase() + role.slice(1);

interface RoleBindingsSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * Claude Code 聚合页签上的「模型绑定」：四个档位分别用哪个模型，一次提交保存；
 * 选「跟随默认」的档用当前默认供应商。保存的绑定失效时保留原值并提示，不悄悄改掉。
 */
export function RoleBindingsSheet({
  open,
  onOpenChange,
}: RoleBindingsSheetProps) {
  const { t } = useTranslation();
  const { data: stack, isPending: loading } = useProxyStack("claude", open);
  const setRoleBindings = useSetProxyStackRoleBindings();

  const candidates = useMemo(
    () => stack?.roleCandidates ?? [],
    [stack?.roleCandidates],
  );
  const invalidRoles = useMemo(
    () => new Set(stack?.roleInvalid ?? []),
    [stack?.roleInvalid],
  );
  const candidateKeys = useMemo(
    () => new Set(candidates.map((item) => encodePick(item))),
    [candidates],
  );

  // 打开后按后端返回的绑定起稿；中途查询刷新不覆盖正在编辑的草稿，关掉再开重新起稿
  const [draft, setDraft] = useState<RoleDraft | null>(null);
  useEffect(() => {
    if (!open) {
      setDraft(null);
      return;
    }
    if (draft === null && stack) {
      const next = emptyDraft();
      for (const role of STACK_ROLES) {
        next[role] = stack.roleBindings?.[role] ?? null;
      }
      setDraft(next);
    }
  }, [open, stack, draft]);

  const dirty = useMemo(() => {
    if (!draft) return false;
    return STACK_ROLES.some(
      (role) => !samePick(draft[role], stack?.roleBindings?.[role] ?? null),
    );
  }, [draft, stack?.roleBindings]);

  const pickFor = (role: StackRole, value: string) => {
    if (value !== DEFAULT_VALUE && !candidateKeys.has(value)) return;
    setDraft((current) => {
      if (!current) return current;
      const next = { ...current };
      if (value === DEFAULT_VALUE) {
        next[role] = null;
      } else {
        const [providerId, modelId] = value.split(SEP);
        next[role] = { providerId, modelId };
      }
      return next;
    });
  };

  const save = () => {
    if (!draft || !stack) return;
    const bindings: StackRoleBindings = {};
    for (const role of STACK_ROLES) {
      const pick = draft[role];
      if (pick) bindings[role] = pick;
    }
    setRoleBindings.mutate(
      { appType: "claude", bindings },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent width={420} closeLabel={t("common.close")}>
        <SheetHeader>
          <SheetTitle>{t("mode.roleBindingsTitle")}</SheetTitle>
          <SheetDescription>{t("mode.roleBindingsLead")}</SheetDescription>
        </SheetHeader>
        <SheetBody>
          <div className="flex flex-col gap-4">
            {!stack && !loading && (
              <p className="text-caption text-warning-text">
                {t("mode.roleBindingsUnavailable")}
              </p>
            )}
            {STACK_ROLES.map((role) => {
              const current = draft?.[role] ?? null;
              const inCandidates =
                current !== null &&
                candidates.some(
                  (candidate) =>
                    candidate.providerId === current.providerId &&
                    candidate.modelId === current.modelId,
                );
              const invalid =
                (invalidRoles.has(role) &&
                  samePick(current, stack?.roleBindings?.[role] ?? null)) ||
                (current !== null && !inCandidates);
              return (
                <div key={role} className="flex flex-col gap-1.5">
                  <span className="text-sm font-medium text-fg-1">
                    {roleLabel(role)}
                  </span>
                  <Select
                    value={encodePick(current)}
                    onValueChange={(value) => pickFor(role, value)}
                    disabled={!draft}
                  >
                    <SelectTrigger aria-label={roleLabel(role)}>
                      <SelectValue placeholder={t("mode.roleBindingDefault")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={DEFAULT_VALUE}>
                        {t("mode.roleBindingDefault")}
                      </SelectItem>
                      {candidates.map((candidate) => (
                        <SelectItem
                          key={`${candidate.providerId}${SEP}${candidate.modelId}`}
                          value={`${candidate.providerId}${SEP}${candidate.modelId}`}
                        >
                          {candidate.displayName}
                        </SelectItem>
                      ))}
                      {current !== null && !inCandidates && (
                        <SelectItem value={encodePick(current)}>
                          {`${current.modelId} · ${t("mode.roleBindingInvalidTag")}`}
                        </SelectItem>
                      )}
                    </SelectContent>
                  </Select>
                  {invalid && (
                    <p className="text-caption text-warning-text">
                      {t("mode.roleBindingInvalid")}
                    </p>
                  )}
                </div>
              );
            })}
          </div>
        </SheetBody>
        <SheetFooter className="justify-end">
          <Button
            variant="solid"
            size="compact"
            disabled={!stack || !dirty || setRoleBindings.isPending}
            onClick={save}
          >
            {setRoleBindings.isPending ? t("common.saving") : t("common.save")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  groupUsersByApp,
  type ManagedAccountUser,
} from "@/lib/managedAccountUsage";

export type ManagedAccountRemoveTarget =
  | { kind: "one"; accountId: string; login: string }
  | { kind: "all"; accountIds: string[] };

interface ManagedAccountRemoveDialogProps {
  target: ManagedAccountRemoveTarget | null;
  /** 服务名：GitHub Copilot / ChatGPT / xAI */
  serviceName: string;
  /** 在用要删的账号的供应商（findManagedAccountUsers 的结果） */
  users: ManagedAccountUser[];
  /** 删掉这一个之后还剩别的账号：跟着默认账号的供应商会改用新的默认账号 */
  othersRemain: boolean;
  pending?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * 删除授权中心账号的确认框：写清哪些供应商会受影响（照会话删除确认框的写法）。
 * 登录凭据删了就没了，确认键用红底。
 */
export function ManagedAccountRemoveDialog({
  target,
  serviceName,
  users,
  othersRemain,
  pending = false,
  onConfirm,
  onCancel,
}: ManagedAccountRemoveDialogProps) {
  const { t } = useTranslation();
  const one = target?.kind === "one";
  const count = target?.kind === "all" ? target.accountIds.length : 1;

  // 只删一个、且还剩别的账号时，跟着默认账号的供应商不会坏，而是换到新的默认账号
  const moved = one && othersRemain ? users.filter((u) => u.viaDefault) : [];
  const broken =
    one && othersRemain ? users.filter((u) => !u.viaDefault) : users;

  const appSeparator = t("managedAuth.remove.appSeparator", {
    defaultValue: "：",
  });
  const nameSeparator = t("managedAuth.remove.nameSeparator", {
    defaultValue: "、",
  });

  const renderList = (lead: string, list: ManagedAccountUser[]) => (
    <div className="flex flex-col gap-1.5">
      <p className="m-0 text-body text-fg-1">{lead}</p>
      <ul className="m-0 flex list-none flex-col gap-2 rounded-panel bg-subtle px-3.5 py-3">
        {groupUsersByApp(list).map((group) => (
          <li
            key={group.appId}
            className="flex items-start gap-2.5 text-caption"
          >
            <span className="flex h-[18px] w-4 shrink-0 items-center">
              <AppGlyph
                app={group.appId}
                size={14}
                badgeClassName="bg-subtle"
              />
            </span>
            <span className="min-w-0 [overflow-wrap:anywhere]">
              <span className="font-semibold text-fg-1">
                {APP_DISPLAY_NAME[group.appId]}
              </span>
              <span className="text-fg-2">
                {appSeparator}
                {group.names.join(nameSeparator)}
              </span>
            </span>
          </li>
        ))}
      </ul>
    </div>
  );

  return (
    <Dialog
      open={target !== null}
      onOpenChange={(open) => {
        if (!open && !pending) onCancel();
      }}
    >
      {target && (
        <DialogContent
          role="alertdialog"
          zIndex="alert"
          className="max-w-[480px] gap-4 rounded-dialog border-border bg-surface p-6 shadow-v7-lg sm:rounded-dialog"
        >
          <div className="flex flex-col gap-1.5">
            <DialogTitle className="text-title text-fg-1 [overflow-wrap:anywhere]">
              {target.kind === "one"
                ? t("managedAuth.remove.titleOne", {
                    defaultValue: "删除账号「{{login}}」？",
                    login: target.login,
                  })
                : t("managedAuth.remove.titleAll", {
                    defaultValue: "删除全部 {{service}} 账号？",
                    service: serviceName,
                  })}
            </DialogTitle>
            <DialogDescription className="text-body text-fg-2">
              {one
                ? t("managedAuth.remove.leadOne", {
                    defaultValue:
                      "会删除这个账号保存在 CC Switch 里的登录凭据，无法撤销。",
                  })
                : t("managedAuth.remove.leadAll", {
                    defaultValue:
                      "会删除这 {{count}} 个账号保存在 CC Switch 里的登录凭据，无法撤销。",
                    count,
                  })}
            </DialogDescription>
          </div>

          {users.length === 0 ? (
            <p className="m-0 text-body text-fg-2">
              {one
                ? t("managedAuth.remove.noneOne", {
                    defaultValue: "没有供应商在用这个账号。",
                  })
                : t("managedAuth.remove.noneAll", {
                    defaultValue: "没有供应商在用这些账号。",
                  })}
            </p>
          ) : (
            <>
              {broken.length > 0 &&
                renderList(
                  one
                    ? t("managedAuth.remove.brokenOne", {
                        defaultValue:
                          "这些供应商会无法使用，直到重新登录或改选别的账号：",
                      })
                    : t("managedAuth.remove.brokenAll", {
                        defaultValue: "这些供应商会无法使用，直到重新登录：",
                      }),
                  broken,
                )}
              {moved.length > 0 &&
                renderList(
                  t("managedAuth.remove.moved", {
                    defaultValue:
                      "这些供应商用的是默认账号，删除后改用新的默认账号：",
                  }),
                  moved,
                )}
            </>
          )}

          <div className="flex flex-wrap justify-end gap-2 pt-1">
            <Button
              type="button"
              variant="neutral"
              size="regular"
              autoFocus
              disabled={pending}
              onClick={onCancel}
            >
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
            <Button
              type="button"
              variant="destructive"
              size="regular"
              disabled={pending}
              onClick={onConfirm}
            >
              {one
                ? t("managedAuth.remove.confirmOne", {
                    defaultValue: "删除账号",
                  })
                : t("managedAuth.remove.confirmAll", {
                    defaultValue: "删除 {{count}} 个账号",
                    count,
                  })}
            </Button>
          </div>
        </DialogContent>
      )}
    </Dialog>
  );
}

/** 账号行上的「N 个供应商在用」；没有供应商在用时不显示 */
export function ManagedAccountUsage({
  users,
}: {
  users: ManagedAccountUser[];
}) {
  const { t } = useTranslation();
  if (users.length === 0) return null;
  const names = users.map((user) => user.name);
  return (
    <span
      className="shrink-0 whitespace-nowrap text-caption text-fg-2"
      title={t("managedAuth.usedByTitle", {
        defaultValue: "在用的供应商：{{names}}",
        names: Array.from(new Set(names)).join(
          t("managedAuth.remove.nameSeparator", { defaultValue: "、" }),
        ),
      })}
    >
      {t("managedAuth.usedBy", {
        defaultValue: "{{count}} 个供应商在用",
        count: users.length,
      })}
    </span>
  );
}

import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { SessionMeta } from "@/types";
import {
  formatSessionTitle,
  isSessionAppId,
  SESSION_APP_IDS,
  SESSION_SOURCE_PATHS,
  type SessionAppId,
} from "./utils";

const dialogClass =
  "max-w-[480px] gap-4 rounded-dialog border-border bg-surface p-6 shadow-v7-lg sm:rounded-dialog";

interface SessionDeleteDialogProps {
  targets: SessionMeta[] | null;
  pending: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** 删除确认：按应用写清删了什么、没删什么。会话删除不经过废纸篓，不给撤销。 */
export function SessionDeleteDialog({
  targets,
  pending,
  onConfirm,
  onCancel,
}: SessionDeleteDialogProps) {
  const { t } = useTranslation();
  const list = targets ?? [];
  const single = list.length === 1;

  const perApp = new Map<string, number>();
  list.forEach((session) =>
    perApp.set(session.providerId, (perApp.get(session.providerId) ?? 0) + 1),
  );
  const apps = [
    ...SESSION_APP_IDS.filter((app) => perApp.has(app)),
    ...Array.from(perApp.keys()).filter((app) => !isSessionAppId(app)),
  ];

  const consequence = (app: string) => {
    const name = isSessionAppId(app) ? APP_DISPLAY_NAME[app] : app;
    switch (app) {
      case "claude":
        return t("sessionManager.consequenceClaude", {
          defaultValue:
            "删除会话文件和同名附属目录。Claude Code 的 Agents 面板里可能还会留一条记录。",
        });
      case "codex":
        return t("sessionManager.consequenceCodex", {
          defaultValue:
            "只删除这一个会话文件。Codex 桌面版的任务列表里可能仍显示它，点开会提示无法继续。",
        });
      case "grokbuild":
        return t("sessionManager.consequenceGrok", {
          defaultValue: "删除这个会话的整个目录。",
        });
      default:
        return t("sessionManager.consequenceGeneric", {
          defaultValue: "从 {{app}} 的本地记录中永久删除。",
          app: name,
        });
    }
  };

  return (
    <Dialog
      open={list.length > 0}
      onOpenChange={(open) => {
        if (!open && !pending) onCancel();
      }}
    >
      {list.length > 0 && (
        <DialogContent
          role="alertdialog"
          zIndex="alert"
          className={dialogClass}
        >
          <div className="flex flex-col gap-1.5">
            <DialogTitle className="text-title text-fg-1 [overflow-wrap:anywhere]">
              {single
                ? t("sessionManager.deleteOneTitle", {
                    defaultValue: "删除会话「{{title}}」？",
                    title: formatSessionTitle(list[0]),
                  })
                : t("sessionManager.deleteManyTitle", {
                    defaultValue: "删除 {{count}} 个会话？",
                    count: list.length,
                  })}
            </DialogTitle>
            <DialogDescription className="text-body text-fg-2">
              {t("sessionManager.deleteLead", {
                defaultValue: "会从磁盘上永久删除，不经过废纸篓，无法撤销。",
              })}
            </DialogDescription>
          </div>
          <ul className="m-0 flex list-none flex-col gap-2.5 rounded-panel bg-subtle px-3.5 py-3">
            {apps.map((app) => {
              const name = isSessionAppId(app)
                ? APP_DISPLAY_NAME[app as SessionAppId]
                : app;
              return (
                <li key={app} className="flex items-start gap-2.5 text-caption">
                  <span className="flex h-[18px] w-4 shrink-0 items-center">
                    {isSessionAppId(app) && (
                      <AppGlyph
                        app={app}
                        size={14}
                        badgeClassName="bg-subtle"
                      />
                    )}
                  </span>
                  <span>
                    <span className="font-semibold text-fg-1">
                      {single
                        ? name
                        : t("sessionManager.deleteAppCount", {
                            defaultValue: "{{app}} {{count}} 个",
                            app: name,
                            count: perApp.get(app),
                          })}
                    </span>
                    <span className="text-fg-2">：{consequence(app)}</span>
                  </span>
                </li>
              );
            })}
          </ul>
          <div className="flex flex-wrap justify-end gap-2 pt-1">
            <Button
              variant="neutral"
              size="regular"
              autoFocus
              disabled={pending}
              onClick={onCancel}
            >
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
            <Button
              variant="destructive"
              size="regular"
              disabled={pending}
              onClick={onConfirm}
            >
              {single
                ? t("sessionManager.deleteConfirmAction", {
                    defaultValue: "删除会话",
                  })
                : t("sessionManager.deleteManyAction", {
                    defaultValue: "删除 {{count}} 个会话",
                    count: list.length,
                  })}
            </Button>
          </div>
        </DialogContent>
      )}
    </Dialog>
  );
}

interface SessionSourcesDialogProps {
  open: boolean;
  onClose: () => void;
}

/** 「会话记录在哪里」：列出 9 个来源的默认位置。 */
export function SessionSourcesDialog({
  open,
  onClose,
}: SessionSourcesDialogProps) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      {open && (
        <DialogContent
          zIndex="alert"
          className={`${dialogClass} max-w-[520px]`}
          onInteractOutside={() => onClose()}
        >
          <div className="flex flex-col gap-1.5">
            <DialogTitle className="text-title text-fg-1">
              {t("sessionManager.whereTitle", {
                defaultValue: "会话记录在哪里",
              })}
            </DialogTitle>
            <DialogDescription className="text-body text-fg-2">
              {t("sessionManager.whereLead", {
                defaultValue:
                  "CC Switch 从这些位置读取会话。Claude Desktop 没有单独的会话记录。",
              })}
            </DialogDescription>
          </div>
          <ul className="m-0 flex list-none flex-col gap-2 p-0">
            {SESSION_APP_IDS.map((app) => (
              <li key={app} className="flex items-start gap-2.5 text-body">
                <span className="flex h-5 w-4 shrink-0 items-center">
                  <AppGlyph app={app} size={14} badgeClassName="bg-surface" />
                </span>
                <span className="w-[104px] shrink-0 font-medium text-fg-1">
                  {APP_DISPLAY_NAME[app]}
                </span>
                <span className="flex min-w-0 flex-wrap gap-x-3">
                  {SESSION_SOURCE_PATHS[app].map((path) => (
                    <code
                      key={path}
                      className="font-mono text-caption leading-5 text-fg-2 [overflow-wrap:anywhere]"
                    >
                      {path}
                    </code>
                  ))}
                </span>
              </li>
            ))}
          </ul>
          <p className="m-0 text-caption text-fg-2">
            {t("sessionManager.whereNote", {
              defaultValue: "在设置里改过配置目录的应用，按改过的目录读取。",
            })}
          </p>
          <div className="flex justify-end">
            <Button
              variant="neutral"
              size="regular"
              autoFocus
              onClick={onClose}
            >
              {t("common.close", { defaultValue: "关闭" })}
            </Button>
          </div>
        </DialogContent>
      )}
    </Dialog>
  );
}

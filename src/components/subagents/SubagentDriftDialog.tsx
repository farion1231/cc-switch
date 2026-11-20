import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { V7Dialog } from "@/components/mcp/formBits";
import { useSubagentDiskContentQuery } from "@/hooks/useSubagents";
import type { Subagent, SubagentDriftStrategy } from "@/lib/api/subagents";
import { extractErrorMessage } from "@/utils/errorUtils";

interface SubagentDriftDialogProps {
  subagent: Subagent;
  pending: boolean;
  onResolve: (strategy: SubagentDriftStrategy) => void;
  onClose: () => void;
}

function Pane({
  label,
  meta,
  children,
}: {
  label: string;
  meta?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col gap-1.5">
      <header className="flex items-baseline gap-2">
        <h3 className="m-0 text-body font-medium text-fg-1">{label}</h3>
        {meta ? (
          <span className="min-w-0 truncate font-mono text-caption text-fg-2">
            {meta}
          </span>
        ) : null}
      </header>
      <div className="min-h-[200px] flex-1 overflow-auto rounded-control border border-border bg-subtle">
        {children}
      </div>
    </section>
  );
}

const preClass =
  "m-0 whitespace-pre-wrap px-3 py-2.5 font-mono text-caption leading-[18px] text-fg-1 [overflow-wrap:anywhere]";

/**
 * 磁盘上的文件在 cc-switch 之外被改过：并排给出两个版本，让用户选留哪个。
 * 磁盘版本每次打开都现读，不用缓存。
 */
export function SubagentDriftDialog({
  subagent,
  pending,
  onResolve,
  onClose,
}: SubagentDriftDialogProps) {
  const { t } = useTranslation();
  const disk = useSubagentDiskContentQuery(subagent.id);
  const diskMissing = disk.isSuccess && disk.data === null;

  return (
    <V7Dialog
      open
      width={960}
      onOpenChange={(open) => {
        if (!open && !pending) onClose();
      }}
      className="h-[min(720px,calc(100vh-48px))]"
    >
      <div className="flex flex-col gap-1.5">
        <DialogTitle className="text-section [overflow-wrap:anywhere]">
          {t("subagents.drift.title", { name: subagent.name })}
        </DialogTitle>
        <DialogDescription className="text-body text-fg-2">
          {t("subagents.drift.body", { path: subagent.filePath })}
        </DialogDescription>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-3 md:flex-row">
        <Pane label={t("subagents.drift.ccSwitchVersion")}>
          <pre className={preClass}>{subagent.content}</pre>
        </Pane>
        <Pane label={t("subagents.drift.diskVersion")} meta={subagent.filePath}>
          {disk.isLoading ? (
            <p className="m-0 px-3 py-2.5 text-caption text-fg-2">
              {t("common.loading")}
            </p>
          ) : disk.isError ? (
            <p className="m-0 px-3 py-2.5 text-caption text-danger-text">
              {t("subagents.drift.loadFailed", {
                error: extractErrorMessage(disk.error) || t("common.unknown"),
              })}
            </p>
          ) : diskMissing ? (
            <p className="m-0 px-3 py-2.5 text-caption text-fg-2">
              {t("subagents.drift.diskMissing")}
            </p>
          ) : (
            <pre className={preClass}>{disk.data}</pre>
          )}
        </Pane>
      </div>

      <div className="flex flex-wrap items-center justify-end gap-2">
        <Button
          type="button"
          variant="neutral"
          size="regular"
          autoFocus
          disabled={pending}
          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="regular"
          disabled={pending || !disk.isSuccess || diskMissing}
          onClick={() => onResolve("adopt_disk")}
        >
          {t("subagents.drift.adoptDisk")}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="regular"
          disabled={pending}
          onClick={() => onResolve("overwrite_disk")}
        >
          {t("subagents.drift.overwriteDisk")}
        </Button>
      </div>
    </V7Dialog>
  );
}

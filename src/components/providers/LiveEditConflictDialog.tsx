import { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import type { EditorConflictPolicy } from "@/lib/api/providers";
import { parseLiveEditConflict } from "@/lib/errors/liveEditConflict";

interface LiveEditConflictDialogProps {
  /** 编辑期间被别的程序改过的键；为 null 时不显示。 */
  keys: string[] | null;
  pending?: boolean;
  onResolve: (policy: Exclude<EditorConflictPolicy, "refuse">) => void;
  onCancel: () => void;
}

/** 编辑器保存时发现配置文件在编辑期间被改过：让用户选保留哪一边。 */
export function LiveEditConflictDialog({
  keys,
  pending = false,
  onResolve,
  onCancel,
}: LiveEditConflictDialogProps) {
  const { t } = useTranslation();

  return (
    <Dialog
      open={keys !== null}
      onOpenChange={(open) => {
        if (!open && !pending) onCancel();
      }}
    >
      <DialogContent className="max-w-md" zIndex="alert">
        <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
          <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
            <AlertTriangle className="h-5 w-5 text-warning-text" />
            {t("provider.editConflict.title", {
              defaultValue: "配置文件在编辑期间被修改",
            })}
          </DialogTitle>
          <DialogDescription className="whitespace-pre-line text-sm leading-relaxed">
            {t("provider.editConflict.message", {
              defaultValue:
                "打开编辑器之后，下面这些设置被其他程序（比如 Claude Code）改过。要用你的修改覆盖，还是保留外部的修改？其余改动都会照常保存。",
            })}
          </DialogDescription>
        </DialogHeader>
        <ul className="px-6 pt-2 font-mono text-xs text-fg-2">
          {(keys ?? []).map((key) => (
            <li key={key}>• {key}</li>
          ))}
        </ul>
        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button variant="outline" onClick={onCancel} disabled={pending}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="outline"
            onClick={() => onResolve("keepTheirs")}
            disabled={pending}
          >
            {t("provider.editConflict.keepTheirs", {
              defaultValue: "保留外部修改",
            })}
          </Button>
          <Button onClick={() => onResolve("keepMine")} disabled={pending}>
            {t("provider.editConflict.keepMine", {
              defaultValue: "用我的修改覆盖",
            })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * 编辑器保存的冲突重试：先按 `refuse` 提交；配置文件在编辑期间被改过时弹出
 * {@link LiveEditConflictDialog}，按用户选的策略再提交一次。重试失败的提示由保存的
 * mutation 负责，编辑器保持打开。
 */
export function useLiveEditConflict(sessionToken = 0) {
  const currentSessionToken = useRef(sessionToken);
  currentSessionToken.current = sessionToken;
  const [pendingConflict, setPendingConflict] = useState<{
    sessionToken: number;
    keys: string[];
    retry: (policy: EditorConflictPolicy) => Promise<void>;
    resolving: boolean;
  } | null>(null);
  // Closing/reopening an editor invalidates both the prompt and an in-flight
  // retry immediately, without waiting for an effect or the old request.
  const conflict =
    pendingConflict?.sessionToken === sessionToken ? pendingConflict : null;

  const submitWithConflictRetry = useCallback(
    async (submit: (policy: EditorConflictPolicy) => Promise<void>) => {
      try {
        await submit("refuse");
      } catch (error) {
        const conflict = parseLiveEditConflict(error);
        if (!conflict) throw error;
        if (currentSessionToken.current === sessionToken) {
          setPendingConflict({
            sessionToken,
            keys: conflict.keys,
            retry: submit,
            resolving: false,
          });
        }
      }
    },
    [sessionToken],
  );

  const resolve = useCallback(
    async (policy: EditorConflictPolicy) => {
      if (!conflict || currentSessionToken.current !== sessionToken) return;
      setPendingConflict({ ...conflict, resolving: true });
      try {
        await conflict.retry(policy);
      } catch {
        // 失败提示由保存的 mutation 负责。
      } finally {
        if (currentSessionToken.current === sessionToken) {
          setPendingConflict(null);
        }
      }
    },
    [conflict, sessionToken],
  );

  const conflictDialog = (
    <LiveEditConflictDialog
      keys={conflict?.keys ?? null}
      pending={conflict?.resolving ?? false}
      onResolve={(policy) => void resolve(policy)}
      onCancel={() => {
        if (currentSessionToken.current === sessionToken) {
          setPendingConflict(null);
        }
      }}
    />
  );

  return { submitWithConflictRetry, conflictDialog };
}

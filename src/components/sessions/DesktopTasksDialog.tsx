import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
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
import { fieldClass } from "@/components/ui/input";
import {
  desktopTasksApi,
  profileId,
  type Profile,
  type Request,
  type Preview,
  type Receipt,
} from "@/lib/api/desktopTasks";
import { extractErrorMessage } from "@/utils/errorUtils";
import { sessionsApi } from "@/lib/api/sessions";
interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}
type Operation =
  | { kind: "idle" }
  | { kind: "busy" }
  | { kind: "preview"; request: Request; preview: Preview }
  | { kind: "receipt"; receipt: Receipt }
  | { kind: "error"; message: string };
export function DesktopTasksDialog({ open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const inventory = useQuery({
    queryKey: ["desktopTasks"],
    queryFn: desktopTasksApi.inventory,
    enabled: open,
    staleTime: 0,
  });
  const [sourceId, setSourceId] = useState("");
  const [targetId, setTargetId] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [operation, setOperation] = useState<Operation>({ kind: "idle" });
  const [prompt, setPrompt] = useState<{ id: string; text: string } | null>(
    null,
  );
  const profiles = inventory.data?.profiles ?? [];
  const source = profiles.find((p) => profileId(p.key) === sourceId);
  const target = profiles.find((p) => profileId(p.key) === targetId);
  const busy = operation.kind === "busy";
  const label = (p: Profile) =>
    `${p.key.root === "managed" ? "Claude-3p" : "Claude"} · ${p.key.account} / ${p.key.org}${p.lastKnownAccount ? ` · ${t("desktopTasks.lastKnown")}` : ""}`;
  const fail = (e: unknown) =>
    setOperation({ kind: "error", message: extractErrorMessage(e) });
  const reset = () => {
    setOperation({ kind: "idle" });
    setSelected(new Set());
    setPrompt(null);
  };
  const toggle = (id: string, checked: boolean) => {
    setOperation({ kind: "idle" });
    setSelected((before) => {
      const next = new Set(before);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  };
  const preview = async () => {
    if (!source || !target) return;
    const request: Request = {
      source: source.key,
      target: target.key,
      taskIds: [...selected],
    };
    setOperation({ kind: "busy" });
    try {
      setOperation({
        kind: "preview",
        request,
        preview: await desktopTasksApi.preview(request),
      });
    } catch (e) {
      fail(e);
    }
  };
  const apply = async () => {
    if (operation.kind !== "preview" || !operation.preview.canApply) return;
    const { request, preview } = operation;
    setOperation({ kind: "busy" });
    try {
      const receipt = await desktopTasksApi.apply(request, preview.token);
      setOperation({ kind: "receipt", receipt });
      setSelected(new Set());
      await inventory.refetch();
    } catch (e) {
      fail(e);
    }
  };
  const showPrompt = async (id: string) => {
    setOperation({ kind: "busy" });
    try {
      setPrompt({ id, text: await desktopTasksApi.prompt(id) });
      setOperation({ kind: "idle" });
    } catch (e) {
      fail(e);
    }
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!busy) {
          setOperation({ kind: "idle" });
          onOpenChange(next);
        }
      }}
    >
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <DialogTitle>{t("desktopTasks.title")}</DialogTitle>
          <DialogDescription>{t("desktopTasks.description")}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-4 overflow-y-auto px-6 py-2">
          {inventory.isPending && <p role="status">{t("common.loading")}</p>}
          {inventory.error && (
            <p role="alert">{extractErrorMessage(inventory.error)}</p>
          )}
          {inventory.data && profiles.length === 0 && (
            <p>{t("desktopTasks.empty")}</p>
          )}
          {inventory.data?.warnings.map((w) => (
            <p key={w} role="status" className="text-sm text-fg-2">
              {w}
            </p>
          ))}
          {profiles.length > 0 && (
            <>
              <div className="grid gap-3 sm:grid-cols-2">
                <label className="min-w-0 space-y-1 text-sm">
                  <span>{t("desktopTasks.source")}</span>
                  <select
                    className={`${fieldClass} h-control`}
                    value={sourceId}
                    disabled={busy}
                    onChange={(e) => {
                      setSourceId(e.target.value);
                      setTargetId("");
                      reset();
                    }}
                  >
                    <option value="">{t("desktopTasks.chooseProfile")}</option>
                    {profiles.map((p) => (
                      <option key={profileId(p.key)} value={profileId(p.key)}>
                        {label(p)}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="min-w-0 space-y-1 text-sm">
                  <span>{t("desktopTasks.target")}</span>
                  <select
                    className={`${fieldClass} h-control`}
                    value={targetId}
                    disabled={busy || !source}
                    onChange={(e) => {
                      setTargetId(e.target.value);
                      setOperation({ kind: "idle" });
                    }}
                  >
                    <option value="">{t("desktopTasks.chooseProfile")}</option>
                    {profiles
                      .filter((p) => profileId(p.key) !== sourceId)
                      .map((p) => (
                        <option key={profileId(p.key)} value={profileId(p.key)}>
                          {label(p)}
                        </option>
                      ))}
                  </select>
                </label>
              </div>
              <p className="text-caption text-fg-2">
                {t("desktopTasks.profileHint")}
              </p>
            </>
          )}
          {source && (
            <>
              {source.warnings.map((w) => (
                <p key={w} role="status" className="text-sm text-fg-2">
                  {w}
                </p>
              ))}
              <p className="text-caption text-fg-2">
                {t("desktopTasks.taskHint")}
              </p>
              <div className="divide-y divide-border rounded-panel border border-border">
                {source.tasks.map((task) => (
                  <div key={task.id} className="space-y-2 px-3 py-3">
                    <label className="flex items-start gap-3 text-sm">
                      <Checkbox
                        checked={selected.has(task.id)}
                        disabled={busy || task.problem !== null}
                        onCheckedChange={(checked) => toggle(task.id, checked)}
                      />
                      <span className="min-w-0 flex-1">
                        <span className="block font-medium">
                          {task.title || task.id}
                        </span>
                        <span className="block truncate text-caption text-fg-2">
                          {task.cwd}
                        </span>
                      </span>
                      <span className="text-caption text-fg-2">
                        {t(
                          task.enabled
                            ? "desktopTasks.enabled"
                            : "desktopTasks.paused",
                        )}
                      </span>
                    </label>
                    <div className="flex flex-wrap items-center gap-3 ps-7 text-caption text-fg-2">
                      <span className="tabular-nums">
                        {task.schedule.kind === "cron"
                          ? task.schedule.expression
                          : task.schedule.kind === "once"
                            ? new Date(task.schedule.at).toLocaleString()
                            : t("desktopTasks.manual")}
                      </span>
                      {task.promptAvailable && (
                        <Button
                          variant="quiet"
                          size="compact"
                          disabled={busy}
                          onClick={() => void showPrompt(task.id)}
                        >
                          {t("desktopTasks.viewPrompt")}
                        </Button>
                      )}
                      {task.problem && (
                        <span>{t(`desktopTasks.problem.${task.problem}`)}</span>
                      )}
                    </div>
                  </div>
                ))}
                {source.tasks.length === 0 && (
                  <p className="px-3 py-4 text-sm text-fg-2">
                    {t("desktopTasks.noTasks")}
                  </p>
                )}
              </div>
            </>
          )}
          {(inventory.data?.orphanPrompts.length ?? 0) > 0 && (
            <div className="space-y-2">
              <h3 className="text-sm font-medium">
                {t("desktopTasks.orphans")}
              </h3>
              <p className="text-caption text-fg-2">
                {t("desktopTasks.orphanHint")}
              </p>
              <div className="flex flex-wrap gap-2">
                {inventory.data?.orphanPrompts.map((id) => (
                  <Button
                    key={id}
                    variant="neutral"
                    size="compact"
                    disabled={busy}
                    onClick={() => void showPrompt(id)}
                  >
                    {id}
                  </Button>
                ))}
              </div>
            </div>
          )}
          {prompt && (
            <section
              className="space-y-2 rounded-panel border border-border p-3"
              aria-label={t("desktopTasks.viewPrompt")}
            >
              <div className="flex items-center justify-between gap-2">
                <h3 className="text-sm font-medium">{prompt.id}</h3>
                <Button
                  variant="quiet"
                  size="compact"
                  disabled={busy}
                  onClick={() =>
                    void sessionsApi
                      .exportMarkdown(prompt.id, prompt.text)
                      .catch(fail)
                  }
                >
                  {t("desktopTasks.exportPrompt")}
                </Button>
              </div>
              <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words text-xs">
                {prompt.text}
              </pre>
            </section>
          )}

          {operation.kind === "error" && (
            <p role="alert">{operation.message}</p>
          )}
          {operation.kind === "preview" && (
            <section
              className="space-y-2 rounded-panel border border-border p-3"
              aria-label={t("desktopTasks.preview")}
            >
              <h3 className="text-sm font-medium">
                {t("desktopTasks.preview")}
              </h3>
              <p className="text-caption text-fg-2">
                {t("desktopTasks.applyHint")}
              </p>
              <ul className="space-y-1 text-sm">
                {operation.preview.items.map((item) => (
                  <li
                    key={item.id}
                    className="flex flex-wrap justify-between gap-2"
                  >
                    <span className="break-all">{item.id}</span>
                    <span>
                      {item.disposition.kind === "blocked"
                        ? t(`desktopTasks.problem.${item.disposition.reason}`)
                        : t(
                            `desktopTasks.disposition.${item.disposition.kind}`,
                          )}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          )}
          {operation.kind === "receipt" && (
            <div role="status" className="space-y-2 text-sm">
              <p>
                {t("desktopTasks.receipt", {
                  tasks: operation.receipt.restoredTasks.length,
                  skipped: operation.receipt.skipped.length,
                })}
              </p>
              {operation.receipt.error ? (
                <p role="alert">{operation.receipt.error}</p>
              ) : (
                <p className="text-fg-2">{t("desktopTasks.verifyHint")}</p>
              )}
              {operation.receipt.backupPath && (
                <p className="break-all text-caption text-fg-2">
                  {t("desktopTasks.backup", {
                    path: operation.receipt.backupPath,
                  })}
                </p>
              )}
            </div>
          )}
        </div>
        <DialogFooter className="flex-wrap">
          <Button
            variant="quiet"
            disabled={busy || inventory.isFetching}
            onClick={() => {
              reset();
              void inventory.refetch();
            }}
          >
            {t("common.refresh")}
          </Button>
          <span className="me-auto text-caption tabular-nums text-fg-2">
            {t("desktopTasks.selected", { count: selected.size })}
          </span>
          <Button
            variant="neutral"
            disabled={busy}
            onClick={() => onOpenChange(false)}
          >
            {t("common.close")}
          </Button>
          {operation.kind === "preview" ? (
            <Button
              disabled={
                !operation.preview.canApply || !inventory.data?.writesSupported
              }
              onClick={() => void apply()}
            >
              {t("desktopTasks.apply")}
            </Button>
          ) : (
            <Button
              disabled={busy || !source || !target || selected.size === 0}
              onClick={() => void preview()}
            >
              {t("desktopTasks.preview")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

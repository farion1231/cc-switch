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
  desktopMigrationApi,
  profileId,
  type Profile,
  type Request,
  type Preview,
  type Receipt,
} from "@/lib/api/desktopMigration";
import { extractErrorMessage } from "@/utils/errorUtils";

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
export function DesktopSessionMigrationDialog({ open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const inventory = useQuery({
    queryKey: ["desktopMigration"],
    queryFn: desktopMigrationApi.inventory,
    enabled: open,
    staleTime: 0,
  });
  const [sourceId, setSourceId] = useState("");
  const [targetId, setTargetId] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [operation, setOperation] = useState<Operation>({ kind: "idle" });
  const [archived, setArchived] = useState(false);
  const profiles = inventory.data?.profiles ?? [];
  const source = profiles.find((p) => profileId(p.key) === sourceId);
  const target = profiles.find((p) => profileId(p.key) === targetId);
  const busy = operation.kind === "busy";
  const label = (p: Profile) =>
    `${p.key.root === "managed" ? "Claude-3p" : "Claude"} · ${p.key.account} / ${p.key.org}${p.lastKnownAccount ? ` · ${t("desktopMigration.lastKnown")}` : ""}`;
  const fail = (e: unknown) =>
    setOperation({ kind: "error", message: extractErrorMessage(e) });
  const reset = () => {
    setOperation({ kind: "idle" });
    setSelected(new Set());
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
      sessionIds: [...selected],
    };
    setOperation({ kind: "busy" });
    try {
      setOperation({
        kind: "preview",
        request,
        preview: await desktopMigrationApi.preview(request),
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
      const receipt = await desktopMigrationApi.apply(request, preview.token);
      setOperation({ kind: "receipt", receipt });
      setSelected(new Set());
      await inventory.refetch();
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
          <DialogTitle>{t("desktopMigration.title")}</DialogTitle>
          <DialogDescription>
            {t("desktopMigration.description")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-4 overflow-y-auto px-6 py-2">
          {inventory.isPending && <p role="status">{t("common.loading")}</p>}
          {inventory.error && (
            <p role="alert">{extractErrorMessage(inventory.error)}</p>
          )}
          {inventory.data && profiles.length === 0 && (
            <p>{t("desktopMigration.empty")}</p>
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
                  <span>{t("desktopMigration.source")}</span>
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
                    <option value="">
                      {t("desktopMigration.chooseProfile")}
                    </option>
                    {profiles.map((p) => (
                      <option key={profileId(p.key)} value={profileId(p.key)}>
                        {label(p)}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="min-w-0 space-y-1 text-sm">
                  <span>{t("desktopMigration.target")}</span>
                  <select
                    className={`${fieldClass} h-control`}
                    value={targetId}
                    disabled={busy || !source}
                    onChange={(e) => {
                      setTargetId(e.target.value);
                      setOperation({ kind: "idle" });
                    }}
                  >
                    <option value="">
                      {t("desktopMigration.chooseProfile")}
                    </option>
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
                {t("desktopMigration.profileHint")}
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
              <label className="flex items-center gap-2 text-sm">
                <Checkbox checked={archived} onCheckedChange={setArchived} />
                {t("desktopMigration.showArchived")}
              </label>
              <p className="text-caption text-fg-2">
                {t("desktopMigration.transcriptHint")}
              </p>
              <div className="divide-y divide-border rounded-panel border border-border">
                {source.sessions
                  .filter((session) => archived || !session.archived)
                  .map((session) => (
                    <div key={session.id} className="space-y-2 px-3 py-3">
                      <label className="flex items-start gap-3 text-sm">
                        <Checkbox
                          checked={selected.has(session.id)}
                          disabled={busy}
                          onCheckedChange={(checked) =>
                            toggle(session.id, checked)
                          }
                        />
                        <span className="min-w-0 flex-1">
                          <span className="block break-words font-medium">
                            {session.title || session.id}
                          </span>
                          <span className="block truncate text-caption text-fg-2">
                            {session.cwd}
                          </span>
                        </span>
                        <span className="text-caption text-fg-2">
                          {t(
                            session.archived
                              ? "desktopMigration.archived"
                              : "desktopMigration.active",
                          )}
                        </span>
                      </label>
                    </div>
                  ))}
                {source.sessions.filter(
                  (session) => archived || !session.archived,
                ).length === 0 && (
                  <p className="px-3 py-4 text-sm text-fg-2">
                    {t("desktopMigration.noSessions")}
                  </p>
                )}
              </div>
            </>
          )}

          {operation.kind === "error" && (
            <p role="alert">{operation.message}</p>
          )}
          {operation.kind === "preview" && (
            <section
              className="space-y-2 rounded-panel border border-border p-3"
              aria-label={t("desktopMigration.preview")}
            >
              <h3 className="text-sm font-medium">
                {t("desktopMigration.preview")}
              </h3>
              <p className="text-caption text-fg-2">
                {t("desktopMigration.applyHint")}
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
                        ? t(
                            `desktopMigration.problem.${item.disposition.reason}`,
                          )
                        : t(
                            `desktopMigration.disposition.${item.disposition.kind}`,
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
                {t("desktopMigration.receipt", {
                  sessions: operation.receipt.copiedSessions.length,
                  skipped: operation.receipt.skipped.length,
                })}
              </p>
              {operation.receipt.error ? (
                <p role="alert">{operation.receipt.error}</p>
              ) : (
                <p className="text-fg-2">{t("desktopMigration.verifyHint")}</p>
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
            {t("desktopMigration.selected", { count: selected.size })}
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
              {t("desktopMigration.apply")}
            </Button>
          ) : (
            <Button
              disabled={busy || !source || !target || selected.size === 0}
              onClick={() => void preview()}
            >
              {t("desktopMigration.preview")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

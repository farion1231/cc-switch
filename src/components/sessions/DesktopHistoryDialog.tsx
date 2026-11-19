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
import { desktopHistoryApi, profileId } from "@/lib/api/desktopHistory";
import type { SessionMeta } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onOpenTranscript: (session: SessionMeta) => void;
}
export function DesktopHistoryDialog({
  open,
  onOpenChange,
  onOpenTranscript,
}: Props) {
  const { t } = useTranslation();
  const inventory = useQuery({
    queryKey: ["desktopHistory"],
    queryFn: desktopHistoryApi.inventory,
    enabled: open,
    staleTime: 0,
  });
  const [selected, setSelected] = useState("");
  const [archived, setArchived] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const profiles = inventory.data?.profiles ?? [];
  const profile = profiles.find((p) => profileId(p.key) === selected);
  const sessions =
    profile?.sessions.filter((s) => archived || !s.archived) ?? [];
  const read = async (sessionId: string, transcriptId: string) => {
    if (!profile) return;
    setBusy(true);
    setError(null);
    try {
      const session = await desktopHistoryApi.openTranscript(
        profile.key,
        sessionId,
        transcriptId,
      );
      onOpenChange(false);
      onOpenTranscript(session);
    } catch (e) {
      setError(extractErrorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!busy) onOpenChange(next);
      }}
    >
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t("desktopHistory.title")}</DialogTitle>
          <DialogDescription>
            {t("desktopHistory.description")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-4 overflow-y-auto px-6 py-2">
          {inventory.isPending && <p role="status">{t("common.loading")}</p>}
          {(error || inventory.error) && (
            <p role="alert">{error ?? extractErrorMessage(inventory.error)}</p>
          )}
          {inventory.data?.warnings.map((w) => (
            <p key={w} role="status" className="text-sm text-fg-2">
              {w}
            </p>
          ))}
          {inventory.data && profiles.length === 0 && (
            <p>{t("desktopHistory.empty")}</p>
          )}
          {profiles.length > 0 && (
            <>
              <label className="block space-y-1 text-sm">
                <span>{t("desktopHistory.profile")}</span>
                <select
                  className={`${fieldClass} h-control`}
                  value={selected}
                  disabled={busy}
                  onChange={(e) => {
                    setSelected(e.target.value);
                    setError(null);
                  }}
                >
                  <option value="">{t("desktopHistory.chooseProfile")}</option>
                  {profiles.map((p) => (
                    <option key={profileId(p.key)} value={profileId(p.key)}>
                      {p.key.root === "managed" ? "Claude-3p" : "Claude"} ·{" "}
                      {p.key.account} / {p.key.org}
                      {p.lastKnownAccount
                        ? ` · ${t("desktopHistory.lastKnown")}`
                        : ""}
                    </option>
                  ))}
                </select>
              </label>
              <p className="text-caption text-fg-2">
                {t("desktopHistory.profileHint")}
              </p>
            </>
          )}
          {profile && (
            <>
              {profile.warnings.map((w) => (
                <p key={w} role="status" className="text-sm text-fg-2">
                  {w}
                </p>
              ))}
              <label className="flex items-center gap-2 text-sm">
                <Checkbox checked={archived} onCheckedChange={setArchived} />
                {t("desktopHistory.showArchived")}
              </label>
              <p className="text-caption text-fg-2">
                {t("desktopHistory.transcriptHint")}
              </p>
              <div className="divide-y divide-border rounded-panel border border-border">
                {sessions.map((session) => (
                  <div key={session.id} className="space-y-2 px-3 py-3">
                    <div className="flex items-start justify-between gap-3 text-sm">
                      <div className="min-w-0">
                        <p className="break-words font-medium">
                          {session.title || session.id}
                        </p>
                        <p className="truncate text-caption text-fg-2">
                          {session.cwd}
                        </p>
                      </div>
                      <span className="text-caption text-fg-2">
                        {t(
                          session.archived
                            ? "desktopHistory.archived"
                            : "desktopHistory.active",
                        )}
                      </span>
                    </div>
                    <div className="flex flex-wrap items-center gap-2">
                      {session.transcripts.length === 0 && (
                        <span className="text-caption text-fg-2">
                          {t("desktopHistory.noTranscript")}
                        </span>
                      )}
                      {session.transcripts.map((link, index) =>
                        link.kind === "available" ? (
                          <Button
                            key={link.id}
                            variant="quiet"
                            size="compact"
                            disabled={busy}
                            onClick={() => void read(session.id, link.id)}
                          >
                            {t("desktopHistory.readTranscript", {
                              number: index + 1,
                            })}
                          </Button>
                        ) : (
                          <span
                            key={link.id}
                            className="text-caption text-fg-2"
                          >
                            {t(`desktopHistory.transcript.${link.kind}`, {
                              number: index + 1,
                            })}
                          </span>
                        ),
                      )}
                    </div>
                  </div>
                ))}
                {sessions.length === 0 && (
                  <p className="px-3 py-4 text-sm text-fg-2">
                    {t("desktopHistory.noSessions")}
                  </p>
                )}
              </div>
            </>
          )}
        </div>
        <DialogFooter>
          <Button
            variant="quiet"
            disabled={busy || inventory.isFetching}
            onClick={() => {
              setError(null);
              void inventory.refetch();
            }}
          >
            {t("common.refresh")}
          </Button>
          <Button
            variant="neutral"
            disabled={busy}
            onClick={() => onOpenChange(false)}
          >
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

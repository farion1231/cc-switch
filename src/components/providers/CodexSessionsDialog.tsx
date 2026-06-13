import { Copy, Loader2, RefreshCw, Share2 } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { Provider } from "@/types";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  useCodexSessionUsageSummaries,
  useProviderCodexSessions,
  useSetCodexSessionProviderLinks,
} from "@/lib/query/codexSessions";

interface CodexSessionsDialogProps {
  open: boolean;
  provider: Provider | null;
  providers: Provider[];
  onOpenChange: (open: boolean) => void;
}

function formatTokens(value: number) {
  return new Intl.NumberFormat().format(value);
}

export function CodexSessionsDialog({
  open,
  provider,
  providers,
  onOpenChange,
}: CodexSessionsDialogProps) {
  const { t } = useTranslation();
  const providerId = provider?.id;
  const {
    data: sessions = [],
    isLoading,
    refetch,
  } = useProviderCodexSessions(providerId);
  const { data: usageSummaries = [] } = useCodexSessionUsageSummaries();
  const mutation = useSetCodexSessionProviderLinks(providerId ?? "");

  const codexProviders = useMemo(
    () => providers.filter((candidate) => candidate.id),
    [providers],
  );
  const usageBySession = useMemo(
    () => new Map(usageSummaries.map((summary) => [summary.sessionId, summary])),
    [usageSummaries],
  );

  const handleShareAll = async (sessionId: string, sourcePath?: string) => {
    if (!sourcePath) return;
    await mutation.mutateAsync({
      sessionId,
      sourcePath,
      providerIds: codexProviders.map((candidate) => candidate.id),
      linkMode: "all",
      syncToCodex: true,
    });
    toast.success(
      t("codexSessions.shareAllDone", {
        defaultValue: "Session shared to all Codex providers.",
      }),
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="h-[80vh] max-w-5xl overflow-hidden p-0">
        <DialogHeader>
          <DialogTitle>
            {t("codexSessions.title", {
              name: provider?.name ?? "Codex",
              defaultValue: "{{name}} Codex Sessions",
            })}
          </DialogTitle>
        </DialogHeader>

        <div className="flex items-center justify-between gap-3 border-b border-border-default px-6 py-3">
          <p className="min-w-0 text-sm text-muted-foreground">
            {t("codexSessions.subtitle", {
              defaultValue:
                "Manage Codex conversations visible to this provider.",
            })}
          </p>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void refetch()}
            disabled={isLoading}
          >
            <RefreshCw className="h-4 w-4" />
            {t("common.refresh", { defaultValue: "Refresh" })}
          </Button>
        </div>

        <ScrollArea className="min-h-0 flex-1 px-6 py-4">
          {isLoading ? (
            <div className="flex h-40 items-center justify-center text-sm text-muted-foreground">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              {t("common.loading", { defaultValue: "Loading..." })}
            </div>
          ) : sessions.length === 0 ? (
            <div className="flex h-40 items-center justify-center text-sm text-muted-foreground">
              {t("codexSessions.empty", {
                defaultValue: "No Codex sessions found.",
              })}
            </div>
          ) : (
            <div className="space-y-2">
              {sessions.map((item) => {
                const session = item.session;
                const sourcePath = session.sourcePath;
                const usage = usageBySession.get(session.sessionId);
                return (
                  <div
                    key={`${session.sessionId}:${sourcePath ?? ""}`}
                    className="rounded-lg border border-border-default bg-card p-3"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <div className="truncate text-sm font-medium">
                          {session.title || session.sessionId}
                        </div>
                        <div className="mt-1 truncate text-xs text-muted-foreground">
                          {session.projectDir || sourcePath || session.sessionId}
                        </div>
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => {
                            if (!sourcePath) return;
                            void mutation.mutateAsync({
                              sessionId: session.sessionId,
                              sourcePath,
                              providerIds: item.linkedProviderIds,
                              linkMode: "manual",
                              syncToCodex: true,
                            });
                          }}
                          disabled={!sourcePath || mutation.isPending}
                        >
                          <RefreshCw className="h-4 w-4" />
                          {t("codexSessions.syncVisibility", {
                            defaultValue: "Sync visibility",
                          })}
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() =>
                            void handleShareAll(session.sessionId, sourcePath)
                          }
                          disabled={!sourcePath || mutation.isPending}
                        >
                          <Share2 className="h-4 w-4" />
                          {t("codexSessions.shareAll", {
                            defaultValue: "Share all",
                          })}
                        </Button>
                        {session.resumeCommand && (
                          <Button
                            size="icon"
                            variant="ghost"
                            onClick={() => {
                              void navigator.clipboard
                                ?.writeText(session.resumeCommand!)
                                .catch(() => undefined);
                              toast.success(
                                t("sessionManager.resumeCommandCopied", {
                                  defaultValue: "Resume command copied",
                                }),
                              );
                            }}
                            title={t("sessionManager.copyCommand", {
                              defaultValue: "Copy command",
                            })}
                          >
                            <Copy className="h-4 w-4" />
                          </Button>
                        )}
                      </div>
                    </div>

                    <div className="mt-3 flex flex-wrap gap-2 text-xs text-muted-foreground">
                      <span>
                        {t("codexSessions.modelProvider", {
                          defaultValue: "Provider bucket",
                        })}
                        : {session.modelProvider || "unknown"}
                      </span>
                      <span>
                        {t("codexSessions.visible", {
                          defaultValue: "Visible here",
                        })}
                        :{" "}
                        {item.visibleToCurrentProvider
                          ? t("common.yes", { defaultValue: "Yes" })
                          : t("common.no", { defaultValue: "No" })}
                      </span>
                      <span>
                        {t("codexSessions.linkedProviders", {
                          defaultValue: "Linked providers",
                        })}
                        : {item.linkedProviderIds.length}
                      </span>
                    </div>

                    <div className="mt-3 grid grid-cols-2 gap-2 text-xs sm:grid-cols-4">
                      <div className="rounded-md bg-muted/60 p-2">
                        <div className="text-muted-foreground">
                          {t("usage.inputTokens")}
                        </div>
                        <div className="font-medium">
                          {formatTokens(usage?.totalInputTokens ?? 0)}
                        </div>
                      </div>
                      <div className="rounded-md bg-muted/60 p-2">
                        <div className="text-muted-foreground">
                          {t("usage.outputTokens")}
                        </div>
                        <div className="font-medium">
                          {formatTokens(usage?.totalOutputTokens ?? 0)}
                        </div>
                      </div>
                      <div className="rounded-md bg-muted/60 p-2">
                        <div className="text-muted-foreground">
                          {t("usage.cacheReadTokens")}
                        </div>
                        <div className="font-medium">
                          {formatTokens(usage?.totalCacheReadTokens ?? 0)}
                        </div>
                      </div>
                      <div className="rounded-md bg-muted/60 p-2">
                        <div className="text-muted-foreground">
                          {t("usage.cost")}
                        </div>
                        <div className="font-medium">
                          ${usage?.totalCostUsd ?? "0.000000"}
                        </div>
                      </div>
                    </div>

                    <div className="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                      {codexProviders.map((candidate) => {
                        const checked = item.linkedProviderIds.includes(
                          candidate.id,
                        );
                        return (
                          <label
                            key={candidate.id}
                            className="flex min-w-0 items-center gap-2 rounded-md border border-border-default px-2 py-1.5 text-sm"
                          >
                            <Checkbox
                              checked={checked}
                              disabled={!sourcePath || mutation.isPending}
                              onCheckedChange={(next) => {
                                if (!sourcePath) return;
                                const nextIds = new Set(item.linkedProviderIds);
                                if (next === true) {
                                  nextIds.add(candidate.id);
                                } else {
                                  nextIds.delete(candidate.id);
                                }
                                void mutation.mutateAsync({
                                  sessionId: session.sessionId,
                                  sourcePath,
                                  providerIds: Array.from(nextIds),
                                  linkMode: "manual",
                                  syncToCodex: false,
                                });
                              }}
                            />
                            <span className="truncate">{candidate.name}</span>
                          </label>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </ScrollArea>
      </DialogContent>
    </Dialog>
  );
}

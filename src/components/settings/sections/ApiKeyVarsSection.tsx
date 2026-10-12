import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { useApiKeyVars } from "@/lib/query/apiKeyVars";
import {
  ChevronDown,
  Download,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  SettingsBlock,
  SettingsCard,
  SettingsRow,
} from "@/components/settings/SettingsLayout";
import { ApiKeyVarDialog } from "@/components/settings/ApiKeyVarDialog";
import {
  ApiKeyVarTemplateGuide,
  ApiKeyVarTemplateGuideDialog,
} from "@/components/settings/ApiKeyVarTemplateGuide";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  API_KEY_VARS_QUERY_KEY,
  apiKeyVarsApi,
  type ApiKeyVar,
} from "@/lib/api/apiKeyVars";
import type { SettingsFormState } from "@/hooks/useSettings";
import { extractErrorMessage } from "@/utils/errorUtils";

interface ApiKeyVarsSectionProps {
  settings: SettingsFormState;
  onAutoSave: (updates: Partial<SettingsFormState>) => Promise<boolean>;
}

type VarStatusKind =
  | "ok"
  | "expired"
  | "failed"
  | "attention"
  | "disabled"
  | "pending";

function statusOf(v: ApiKeyVar, now: number): VarStatusKind {
  if (!v.enabled) return "disabled";
  if (v.state.needsAttention) return "attention";
  if (v.state.consecutiveFailures > 0) return "failed";
  if (!v.hasValue) return "pending";
  if (v.state.expiresAt && v.state.expiresAt <= now) return "expired";
  return "ok";
}

const STATUS_CLASS: Record<VarStatusKind, string> = {
  ok: "bg-success-soft text-success-text",
  expired: "bg-danger-soft text-danger-text",
  failed: "bg-warning-soft text-warning-text",
  attention: "bg-danger-soft text-danger-text",
  disabled: "bg-subtle text-fg-3",
  pending: "bg-subtle text-fg-2",
};

export function ApiKeyVarsSection({
  settings,
  onAutoSave,
}: ApiKeyVarsSectionProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: vars, isLoading } = useApiKeyVars();
  const [editing, setEditing] = useState<ApiKeyVar | undefined>(undefined);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [deleting, setDeleting] = useState<ApiKeyVar | null>(null);
  const [refreshing, setRefreshing] = useState<string | null>(null);
  const [guideExpanded, setGuideExpanded] = useState(false);
  const [guideOpen, setGuideOpen] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const downloadInFlight = useRef(false);
  const [nodePath, setNodePath] = useState(
    settings.apiKeyRefreshNodePath ?? "",
  );

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: API_KEY_VARS_QUERY_KEY });

  const downloadTemplate = async () => {
    if (downloadInFlight.current) return;
    downloadInFlight.current = true;
    setDownloading(true);
    try {
      const path = await apiKeyVarsApi.downloadTemplate();
      if (path) toast.success(t("apiKeyVars.template.downloaded", { path }));
    } catch (e) {
      toast.error(
        t("apiKeyVars.template.downloadFailed", {
          error: extractErrorMessage(e),
        }),
      );
    } finally {
      downloadInFlight.current = false;
      setDownloading(false);
    }
  };

  const refresh = async (name: string) => {
    setRefreshing(name);
    try {
      const outcome = await apiKeyVarsApi.refresh(name);
      if (outcome.success) {
        toast.success(
          t("apiKeyVars.refreshSuccess", {
            name,
            value: outcome.maskedValue ?? "",
          }),
        );
      }
      // 失败由全局的刷新失败事件统一提示
    } catch (e) {
      toast.error(
        t("apiKeyVars.refreshFailed", {
          name,
          error: extractErrorMessage(e),
        }),
      );
    } finally {
      setRefreshing(null);
      void invalidate();
    }
  };

  const handleDelete = async () => {
    if (!deleting) return;
    try {
      await apiKeyVarsApi.remove(deleting.name);
      toast.success(t("apiKeyVars.deleted", { name: deleting.name }));
    } catch (e) {
      toast.error(extractErrorMessage(e));
    } finally {
      setDeleting(null);
      void invalidate();
    }
  };

  const now = Date.now();
  const formatTime = (ms?: number) =>
    ms ? new Date(ms).toLocaleString() : t("apiKeyVars.noExpiry");

  return (
    <>
      <SettingsBlock
        title={t("apiKeyVars.title")}
        help={{
          title: t("apiKeyVars.title"),
          body: t("apiKeyVars.description"),
        }}
        actions={
          <>
            <Button
              variant="neutral"
              size="compact"
              disabled={downloading}
              onClick={() => void downloadTemplate()}
            >
              {downloading ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Download className="h-3.5 w-3.5" />
              )}
              {t("apiKeyVars.template.download")}
            </Button>
            <Button
              variant="solid"
              size="compact"
              onClick={() => {
                setEditing(undefined);
                setDialogOpen(true);
              }}
            >
              <Plus className="h-3.5 w-3.5" />
              {t("apiKeyVars.add")}
            </Button>
          </>
        }
      >
        {isLoading ? (
          <div className="flex justify-center py-6">
            <Loader2 className="h-5 w-5 animate-spin text-fg-3" />
          </div>
        ) : !vars?.length ? (
          <div className="rounded-panel border border-dashed border-border p-6 text-center text-caption text-fg-2">
            {t("apiKeyVars.empty")}
          </div>
        ) : (
          <SettingsCard>
            {vars.map((v) => {
              const status = statusOf(v, now);
              return (
                <div key={v.name} className="flex items-start gap-3 px-4 py-3">
                  <div className="min-w-0 flex-1 space-y-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <code className="font-mono text-body font-semibold text-fg-1">
                        {`{{${v.name}}}`}
                      </code>
                      <span
                        className={`rounded-full px-2 py-0.5 text-caption ${STATUS_CLASS[status]}`}
                      >
                        {t(`apiKeyVars.status.${status}`)}
                      </span>
                    </div>
                    <div className="text-caption text-fg-2">
                      {v.hasValue
                        ? t("apiKeyVars.valueLine", {
                            value: v.maskedValue ?? "",
                            time: formatTime(v.state.expiresAt),
                          })
                        : t("apiKeyVars.noValue")}
                    </div>
                    <div className="truncate font-mono text-caption text-fg-3">
                      {v.scriptDir || t("apiKeyVars.noScriptDir")}
                    </div>
                    {v.state.lastError &&
                      (v.state.consecutiveFailures > 0 ||
                        v.state.needsAttention) && (
                        <div className="break-all text-caption text-danger-text">
                          {v.state.lastError}
                        </div>
                      )}
                    <div className="text-caption text-fg-3">
                      {v.references.length
                        ? t("apiKeyVars.referencedBy", {
                            list: v.references
                              .map((r) =>
                                r.app === "universal"
                                  ? t("apiKeyVars.universalProvider", {
                                      name: r.providerName,
                                    })
                                  : `${r.app}/${r.providerName}`,
                              )
                              .join(", "),
                          })
                        : t("apiKeyVars.unreferenced")}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon-compact"
                      title={t("apiKeyVars.refreshNow")}
                      aria-label={t("apiKeyVars.refreshNow")}
                      disabled={!v.scriptDir || refreshing === v.name}
                      onClick={() => void refresh(v.name)}
                    >
                      <RefreshCw
                        className={`h-3.5 w-3.5 ${refreshing === v.name ? "animate-spin" : ""}`}
                      />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-compact"
                      title={t("common.edit")}
                      aria-label={t("common.edit")}
                      onClick={() => {
                        setEditing(v);
                        setDialogOpen(true);
                      }}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-compact"
                      title={
                        v.references.length
                          ? t("apiKeyVars.deleteBlocked")
                          : t("common.delete")
                      }
                      aria-label={t("common.delete")}
                      disabled={v.references.length > 0}
                      onClick={() => setDeleting(v)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
              );
            })}
          </SettingsCard>
        )}
      </SettingsBlock>

      <Collapsible
        open={guideExpanded}
        onOpenChange={setGuideExpanded}
        className="rounded-panel border border-border bg-surface"
      >
        <CollapsibleTrigger asChild>
          <Button
            variant="quiet"
            size="regular"
            className="h-auto w-full justify-between whitespace-normal px-4 py-3 text-start"
          >
            {t("apiKeyVars.template.title")}
            <ChevronDown
              className={`h-4 w-4 shrink-0 transition-transform ${guideExpanded ? "rotate-180" : ""}`}
              aria-hidden="true"
            />
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent className="min-w-0 border-t border-border px-4 py-4">
          {guideExpanded && <ApiKeyVarTemplateGuide />}
        </CollapsibleContent>
      </Collapsible>

      <SettingsBlock title={t("apiKeyVars.advanced")}>
        <SettingsCard>
          <SettingsRow
            label={t("apiKeyVars.nodePath")}
            help={{
              title: t("apiKeyVars.nodePath"),
              body: t("apiKeyVars.nodePathHelp"),
            }}
          >
            <Input
              value={nodePath}
              placeholder={t("apiKeyVars.nodePathPlaceholder")}
              className="font-mono"
              onChange={(e) => setNodePath(e.target.value)}
              onBlur={() => {
                if (
                  (settings.apiKeyRefreshNodePath ?? "") !== nodePath.trim()
                ) {
                  void onAutoSave({ apiKeyRefreshNodePath: nodePath.trim() });
                }
              }}
            />
          </SettingsRow>
        </SettingsCard>
      </SettingsBlock>

      <ApiKeyVarDialog
        open={dialogOpen}
        variable={editing}
        existingNames={(vars ?? []).map((v) => v.name)}
        onClose={() => setDialogOpen(false)}
        onOpenGuide={() => setGuideOpen(true)}
        onSaved={(saved, wasEnabled) => {
          setDialogOpen(false);
          void invalidate();
          void queryClient.invalidateQueries({ queryKey: ["providers"] });
          toast.success(t("apiKeyVars.saved", { name: saved.name }));
          // 新启用或尚无值时立即获取一次
          if (saved.enabled && (!wasEnabled || !saved.hasValue)) {
            void refresh(saved.name);
          }
        }}
      />
      <ApiKeyVarTemplateGuideDialog
        open={guideOpen}
        onOpenChange={setGuideOpen}
        downloading={downloading}
        onDownload={() => void downloadTemplate()}
      />
      <ConfirmDialog
        isOpen={Boolean(deleting)}
        title={t("apiKeyVars.deleteTitle")}
        message={t("apiKeyVars.deleteMessage", { name: deleting?.name ?? "" })}
        onConfirm={() => void handleDelete()}
        onCancel={() => setDeleting(null)}
      />
    </>
  );
}

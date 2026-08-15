import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ExternalLink,
  KeyRound,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import type { DshProvider } from "@/lib/api/dsh";
import { dshErrorMessage, isDshConflictError } from "@/lib/api/dsh";
import { useDshActions, useDshSnapshot } from "@/lib/query/dsh";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { DshDefaultModelPicker } from "./DshDefaultModelPicker";
import { DshProviderDialog } from "./DshProviderDialog";

interface DshProvidersPageProps {
  onUnsupportedFeature?: (feature: string) => void;
}

function displayError(
  error: unknown,
  fallback: string,
  conflictMessage: string,
): string {
  if (isDshConflictError(error)) return conflictMessage;
  return dshErrorMessage(error, fallback);
}

/** Live DeepSeek Harness provider manager; it never uses SQLite provider state. */
export function DshProvidersPage({
  onUnsupportedFeature,
}: DshProvidersPageProps) {
  const { t } = useTranslation();
  const query = useDshSnapshot();
  const actions = useDshActions();
  const [editing, setEditing] = useState<DshProvider | null | undefined>();
  const [confirmDelete, setConfirmDelete] = useState<DshProvider | null>(null);
  const [deleting, setDeleting] = useState(false);
  const snapshot = query.data;

  const sortedProviders = useMemo(() => {
    if (!snapshot) return [];
    return [...snapshot.providers].sort((left, right) =>
      left.kind === right.kind
        ? left.displayName.localeCompare(right.displayName)
        : left.kind === "native"
          ? -1
          : 1,
    );
  }, [snapshot]);

  const showFailure = (error: unknown, fallback: string) =>
    toast.error(displayError(error, fallback, t("dsh.errors.conflict")));
  const reload = async () => {
    try {
      await actions.refresh();
      toast.success(t("dsh.messages.refreshed"));
    } catch (error) {
      showFailure(error, t("dsh.errors.read"));
    }
  };
  const openHome = async () => {
    try {
      await actions.openHome();
    } catch (error) {
      toast.error(t("common.error"), {
        description: dshErrorMessage(error, t("dsh.errors.read")),
      });
    }
  };
  const saveCredentialIfNeeded = async (key?: {
    ref: string;
    value: string;
    expectedRevision?: string;
  }) => {
    if (key) await actions.setCredential(key);
  };
  const saveNative = async (
    input: Parameters<typeof actions.upsertNative>[0],
    key?: { ref: string; value: string; expectedRevision?: string },
  ): Promise<void> => {
    await actions.upsertNative(input);
    try {
      await saveCredentialIfNeeded(key);
    } catch (error) {
      void actions.refresh().catch(() => undefined);
      toast.warning(t("dsh.messages.profileSavedKeyFailed"), {
        description: displayError(
          error,
          t("dsh.errors.credentialSave"),
          t("dsh.errors.conflict"),
        ),
      });
      return;
    }
    toast.success(t("dsh.messages.nativeSaved"));
  };
  const saveCustom = async (
    input: Parameters<typeof actions.createCustom>[0],
    key?: { ref: string; value: string; expectedRevision?: string },
  ): Promise<void> => {
    if (editing?.kind === "custom") await actions.updateCustom(input);
    else await actions.createCustom(input);
    try {
      await saveCredentialIfNeeded(key);
    } catch (error) {
      void actions.refresh().catch(() => undefined);
      toast.warning(t("dsh.messages.profileSavedKeyFailed"), {
        description: displayError(
          error,
          t("dsh.errors.credentialSave"),
          t("dsh.errors.conflict"),
        ),
      });
      return;
    }
    toast.success(
      t(
        editing ? "dsh.messages.providerUpdated" : "dsh.messages.providerAdded",
      ),
    );
  };
  const removeProvider = async () => {
    if (!confirmDelete || !snapshot) return;
    setDeleting(true);
    try {
      if (snapshot.defaultModel?.provider === confirmDelete.route)
        throw new Error(t("dsh.errors.defaultProviderDelete"));
      await actions.removeCustom(
        confirmDelete.route,
        confirmDelete.revision ?? snapshot.settingsRevision,
      );
      setConfirmDelete(null);
      toast.success(t("dsh.messages.providerRemoved"));
    } catch (error) {
      showFailure(error, t("dsh.errors.providerRemove"));
    } finally {
      setDeleting(false);
    }
  };
  const resetNative = async () => {
    if (!snapshot) return;
    try {
      await actions.resetNative(snapshot.settingsRevision);
      toast.success(t("dsh.messages.nativeReset"));
    } catch (error) {
      showFailure(error, t("dsh.errors.nativeReset"));
    }
  };

  if (query.isLoading && !snapshot)
    return (
      <div className="p-6 text-sm text-muted-foreground">
        {t("dsh.loading")}
      </div>
    );
  if (query.error && !snapshot)
    return (
      <div className="space-y-4 p-6">
        <Alert variant="destructive">
          <AlertTitle>{t("dsh.errors.readTitle")}</AlertTitle>
          <AlertDescription>
            {displayError(
              query.error,
              t("dsh.errors.readHint"),
              t("dsh.errors.conflict"),
            )}
          </AlertDescription>
        </Alert>
        <Button type="button" onClick={() => void reload()}>
          <RefreshCw className="h-4 w-4" />
          {t("dsh.actions.retry")}
        </Button>
      </div>
    );
  if (!snapshot) return null;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-y-auto px-6 pb-12 pt-4"
      data-testid="dsh-providers-page"
    >
      <div className="mb-5 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">
            {t("apps.dsh", { defaultValue: "DeepSeek Harness" })}
          </h1>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("dsh.description")}
          </p>
          <p className="mt-1 break-all text-xs text-muted-foreground">
            {t("dsh.home", { path: snapshot.home })}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void openHome()}
          >
            <ExternalLink className="h-4 w-4" />
            {t("dsh.actions.openHome")}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void reload()}
            disabled={query.isFetching}
          >
            <RefreshCw
              className={query.isFetching ? "h-4 w-4 animate-spin" : "h-4 w-4"}
            />
            {t("common.refresh")}
          </Button>
          <Button
            type="button"
            size="sm"
            onClick={() => setEditing(null)}
            disabled={snapshot.readOnly}
          >
            <Plus className="h-4 w-4" />
            {t("dsh.actions.addProvider")}
          </Button>
        </div>
      </div>
      {snapshot.readOnly && (
        <Alert className="mb-4">
          <AlertTitle>{t("dsh.readOnly.title")}</AlertTitle>
          <AlertDescription>{t("dsh.readOnly.description")}</AlertDescription>
        </Alert>
      )}
      {snapshot.unsupported && snapshot.unsupported.length > 0 && (
        <Alert className="mb-4">
          <AlertTriangle className="h-4 w-4" />
          <AlertTitle>{t("dsh.unsupported.title")}</AlertTitle>
          <AlertDescription>
            {t("dsh.unsupported.description")}
            {onUnsupportedFeature && (
              <Button
                variant="link"
                className="h-auto p-0"
                onClick={() => onUnsupportedFeature("dsh-unsupported")}
              >
                {t("dsh.unsupported.learnMore")}
              </Button>
            )}
          </AlertDescription>
        </Alert>
      )}
      <div className="space-y-4">
        <DshDefaultModelPicker
          providers={sortedProviders}
          value={snapshot.defaultModel}
          disabled={snapshot.readOnly}
          onSave={async (selection) => {
            await actions.setDefaultModel(selection, snapshot.settingsRevision);
            toast.success(t("dsh.messages.defaultModelSaved"));
          }}
        />
        <div className="grid gap-4">
          {sortedProviders.map((provider) => {
            const isDefault =
              snapshot.defaultModel?.provider === provider.route;
            return (
              <Card key={provider.route}>
                <CardHeader className="flex-row items-start justify-between gap-4 space-y-0 pb-3">
                  <div className="min-w-0">
                    <CardTitle className="flex flex-wrap items-center gap-2 text-base">
                      <span className="truncate">{provider.displayName}</span>
                      <Badge
                        variant={
                          provider.kind === "native" ? "default" : "secondary"
                        }
                      >
                        {provider.kind === "native"
                          ? t("dsh.providers.native")
                          : (provider.api ?? t("dsh.providers.custom"))}
                      </Badge>
                      {isDefault && (
                        <Badge variant="outline">
                          {t("dsh.providers.defaultBadge")}
                        </Badge>
                      )}
                    </CardTitle>
                    <p className="mt-1 break-all text-xs text-muted-foreground">
                      {provider.route}
                      {provider.baseURL ? ` · ${provider.baseURL}` : ""}
                    </p>
                  </div>
                  <div className="flex shrink-0 gap-1">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      title={t("common.edit")}
                      onClick={() => setEditing(provider)}
                    >
                      <Pencil className="h-4 w-4" />
                    </Button>
                    {provider.kind === "native" ? (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title={t("dsh.actions.resetNative")}
                        onClick={() => void resetNative()}
                        disabled={snapshot.readOnly || !provider.customized}
                      >
                        <RotateCcw className="h-4 w-4" />
                      </Button>
                    ) : (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title={t("common.delete")}
                        onClick={() => setConfirmDelete(provider)}
                        disabled={snapshot.readOnly}
                      >
                        <Trash2 className="h-4 w-4 text-destructive" />
                      </Button>
                    )}
                  </div>
                </CardHeader>
                <CardContent className="space-y-2 pt-0">
                  <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                    <span>
                      {t("dsh.providers.modelCount", {
                        count:
                          provider.models.length || provider.modelCount || 0,
                      })}
                    </span>
                    <span>·</span>
                    <span className="inline-flex items-center gap-1">
                      <KeyRound className="h-3.5 w-3.5" />
                      {provider.credential?.configured
                        ? provider.credential.source === "process"
                          ? t("dsh.providers.environmentKey")
                          : t("dsh.providers.keyConfigured")
                        : t("dsh.providers.keyMissing")}
                    </span>
                    {provider.credential?.source === "process" &&
                      !provider.credential.writable && (
                        <span>{t("dsh.providers.readOnlySuffix")}</span>
                      )}
                  </div>
                  {isDefault && snapshot.defaultModel && (
                    <p className="text-xs text-muted-foreground">
                      {t("dsh.providers.defaultModel", {
                        model: snapshot.defaultModel.model,
                      })}
                    </p>
                  )}
                </CardContent>
              </Card>
            );
          })}
        </div>
      </div>
      <DshProviderDialog
        open={editing !== undefined}
        provider={editing ?? null}
        protocols={snapshot.protocols}
        credentialsRevision={snapshot.credentialsRevision}
        readOnly={snapshot.readOnly}
        onClose={() => setEditing(undefined)}
        onSaveNative={saveNative}
        onSaveCustom={saveCustom}
        onUnsetCredential={async (ref, expectedRevision) => {
          await actions.unsetCredential(ref, expectedRevision);
          toast.success(t("dsh.messages.keyRemoved"));
        }}
        onDiscover={async (input) =>
          (await actions.discoverModels(input)).models
        }
      />
      {confirmDelete && (
        <div
          className="fixed inset-0 z-[70] flex items-center justify-center bg-black/50 p-4"
          role="dialog"
          aria-modal="true"
          aria-label={t("dsh.deleteDialog.ariaLabel")}
        >
          <div className="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg">
            <h2 className="font-semibold">
              {t("dsh.deleteDialog.title", {
                name: confirmDelete.displayName,
              })}
            </h2>
            <p className="mt-2 text-sm text-muted-foreground">
              {t("dsh.deleteDialog.description")}
            </p>
            <div className="mt-5 flex justify-end gap-2">
              <Button
                type="button"
                variant="outline"
                onClick={() => setConfirmDelete(null)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant="destructive"
                onClick={() => void removeProvider()}
                disabled={deleting}
              >
                {deleting
                  ? t("dsh.deleteDialog.deleting")
                  : t("dsh.deleteDialog.confirm")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default DshProvidersPage;

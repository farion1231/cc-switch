import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  HelpCircle,
  Loader2,
  Pencil,
  PlugZap,
  Plus,
  Server,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { TooltipProvider } from "@/components/ui/tooltip";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import {
  useDeleteVpsServer,
  useSaveVpsServer,
  useVpsServers,
} from "@/hooks/useVps";
import {
  emptyVpsApps,
  vpsApi,
  type VpsConnectionResult,
  type VpsServer,
} from "@/lib/api/vps";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  prepareVpsServer,
  VpsServerForm,
  type VpsServerDraft,
} from "./VpsServerForm";

export interface VpsPanelHandle {
  openAdd: () => void;
}

interface VpsPanelProps {
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
}

interface EditorState {
  draft: VpsServerDraft;
  original: string;
  isNew: boolean;
  password: string;
  hasSavedPassword: boolean;
}

interface HostKeyChallenge {
  requestId: string;
  server: VpsServer;
  fingerprint: string;
  confirmationToken: string;
  password?: string;
}

function connectionAddress(server: VpsServer): string {
  return `${server.host.includes(":") ? `[${server.host}]` : server.host}:${server.port}`;
}

const VpsPanel = React.forwardRef<VpsPanelHandle, VpsPanelProps>(
  ({ onInteractionBlockedChange, onNavigationBlockedChange }, ref) => {
    const { t } = useTranslation();
    const query = useVpsServers();
    const save = useSaveVpsServer();
    const remove = useDeleteVpsServer();
    const [search, setSearch] = useState("");
    const [editor, setEditor] = useState<EditorState | null>(null);
    const editorRef = useRef<EditorState | null>(null);
    const [discardOpen, setDiscardOpen] = useState(false);
    const [deleting, setDeleting] = useState<VpsServer | null>(null);
    const [helpOpen, setHelpOpen] = useState(false);
    const helpTriggerRef = useRef<HTMLButtonElement>(null);
    const [challenge, setChallenge] = useState<HostKeyChallenge | null>(null);
    const challengeRef = useRef<HostKeyChallenge | null>(null);
    const [writePending, setWritePending] = useState(false);
    const [confirmingKey, setConfirmingKey] = useState(false);
    const [testingId, setTestingId] = useState<string | null>(null);
    const [cancelling, setCancelling] = useState(false);
    const writeLock = useRef(false);
    const request = useRef<{ id: string; cancelled: boolean } | null>(null);
    const mounted = useRef(true);

    const busy = writePending || confirmingKey || testingId !== null;
    const overlayOpen = Boolean(
      editor || deleting || challenge || helpOpen || discardOpen,
    );
    const navigationBlocked = busy || overlayOpen;
    const interactionBlocked =
      navigationBlocked || query.isFetching || query.isError;

    useEffect(() => {
      onInteractionBlockedChange?.(interactionBlocked);
    }, [interactionBlocked, onInteractionBlockedChange]);
    useEffect(() => {
      onNavigationBlockedChange?.(navigationBlocked);
    }, [navigationBlocked, onNavigationBlockedChange]);
    useEffect(() => {
      mounted.current = true;
      return () => {
        mounted.current = false;
        const active = request.current;
        request.current = null;
        if (active)
          void vpsApi.cancelConnectionTest(active.id).catch(() => undefined);
        const pendingChallenge = challengeRef.current;
        challengeRef.current = null;
        if (pendingChallenge && pendingChallenge.requestId !== active?.id)
          void vpsApi
            .cancelConnectionTest(pendingChallenge.requestId)
            .catch(() => undefined);
        onInteractionBlockedChange?.(false);
        onNavigationBlockedChange?.(false);
      };
    }, [onInteractionBlockedChange, onNavigationBlockedChange]);

    const changeEditor = (next: EditorState | null) => {
      editorRef.current = next;
      setEditor(next);
    };

    const changeChallenge = (next: HostKeyChallenge | null) => {
      challengeRef.current = next;
      setChallenge(next);
    };

    const openEditor = (server?: VpsServer) => {
      if (
        writeLock.current ||
        request.current ||
        editorRef.current ||
        interactionBlocked
      )
        return;
      const draft: VpsServerDraft = server
        ? { ...server, apps: { ...server.apps }, port: String(server.port) }
        : {
            id: crypto.randomUUID(),
            name: "",
            purpose: "",
            host: "",
            port: "22",
            user: "",
            authMethod: "password",
            apps: emptyVpsApps(),
          };
      changeEditor({
        draft,
        original: JSON.stringify(draft),
        isNew: !server,
        password: "",
        hasSavedPassword: server?.authMethod === "password",
      });
    };

    React.useImperativeHandle(ref, () => ({ openAdd: () => openEditor() }));

    const requestCloseEditor = () => {
      if (
        writeLock.current ||
        request.current ||
        challengeRef.current ||
        discardOpen
      )
        return;
      const current = editorRef.current;
      if (
        current &&
        (current.password !== "" ||
          JSON.stringify(current.draft) !== current.original)
      ) {
        setDiscardOpen(true);
      } else {
        changeEditor(null);
      }
    };

    const showError = (title: string, error: unknown) => {
      toast.error(t(title), {
        description: extractErrorMessage(error) || t("common.error"),
        closeButton: true,
      });
    };

    const closeChallenge = () => {
      if (writeLock.current) return;
      const pending = challengeRef.current;
      changeChallenge(null);
      if (pending) {
        void vpsApi.cancelConnectionTest(pending.requestId).catch((error) => {
          if (mounted.current) showError("vps.cancelFailed", error);
        });
      }
    };

    const saveServer = async (
      server: VpsServer,
      closeEditor = false,
      password?: string,
    ) => {
      if (writeLock.current || request.current) return;
      writeLock.current = true;
      setWritePending(true);
      try {
        await save.mutateAsync({ server, password });
        if (mounted.current && closeEditor) changeEditor(null);
      } catch (error) {
        if (mounted.current) showError("vps.saveFailed", error);
      } finally {
        save.reset();
        writeLock.current = false;
        if (mounted.current) setWritePending(false);
      }
    };

    const deleteServer = async () => {
      if (!deleting || writeLock.current || request.current) return;
      writeLock.current = true;
      setWritePending(true);
      try {
        await remove.mutateAsync(deleting.id);
        if (mounted.current) setDeleting(null);
      } catch (error) {
        if (mounted.current) showError("vps.deleteFailed", error);
      } finally {
        writeLock.current = false;
        if (mounted.current) setWritePending(false);
      }
    };

    const reportConnection = (result: VpsConnectionResult) => {
      const options = { closeButton: true };
      if (result.status === "success") {
        toast.success(t("vps.connection.success"), options);
      } else if (result.status !== "cancelled") {
        toast.error(t(`vps.connection.${result.status}`), options);
      }
    };

    const testConnection = async (server: VpsServer, password?: string) => {
      if (writeLock.current || request.current) return;
      const active = { id: crypto.randomUUID(), cancelled: false };
      request.current = active;
      setTestingId(server.id);
      setCancelling(false);
      try {
        const result = await vpsApi.testConnection(server, active.id, password);
        if (!mounted.current || request.current !== active || active.cancelled)
          return;
        if (result.status === "hostKeyConfirmationRequired") {
          if (!result.fingerprint || !result.confirmationToken) {
            reportConnection({ status: "failed" });
            return;
          }
          changeChallenge({
            requestId: active.id,
            server,
            fingerprint: result.fingerprint,
            confirmationToken: result.confirmationToken,
            password,
          });
        } else {
          reportConnection(result);
        }
      } catch (error) {
        if (mounted.current && request.current === active && !active.cancelled)
          showError("vps.connection.failed", error);
      } finally {
        if (request.current === active) {
          request.current = null;
          if (mounted.current) {
            setTestingId(null);
            setCancelling(false);
          }
        }
      }
    };

    const cancelTest = async () => {
      const active = request.current;
      if (!active || active.cancelled) return;
      active.cancelled = true;
      setCancelling(true);
      try {
        await vpsApi.cancelConnectionTest(active.id);
      } catch (error) {
        if (mounted.current && request.current === active) {
          active.cancelled = false;
          setCancelling(false);
          showError("vps.cancelFailed", error);
        }
      }
    };

    const confirmHostKey = async () => {
      if (!challenge || writeLock.current || request.current) return;
      const current = challenge;
      writeLock.current = true;
      setConfirmingKey(true);
      let confirmed = false;
      try {
        await vpsApi.confirmHostKey(current.confirmationToken);
        if (mounted.current) {
          changeChallenge(null);
          confirmed = true;
        }
      } catch {
        if (mounted.current) {
          toast.error(t("vps.hostKey.confirmFailed"), {
            description: t("vps.hostKey.retryHint"),
            closeButton: true,
          });
        }
      } finally {
        writeLock.current = false;
        if (mounted.current) setConfirmingKey(false);
      }
      if (confirmed) await testConnection(current.server, current.password);
      else if (mounted.current) closeChallenge();
    };

    const servers = useMemo(() => {
      const term = search.trim().toLocaleLowerCase();
      return (query.data ?? []).filter(
        (server) =>
          !term ||
          [server.name, server.host, server.user, server.purpose].some(
            (value) => value.toLocaleLowerCase().includes(term),
          ),
      );
    }, [query.data, search]);

    const listDisabled =
      busy || overlayOpen || query.isFetching || query.isError;
    const editorDisabled = busy || Boolean(challenge) || discardOpen;

    return (
      <TooltipProvider>
        <div className="px-6 pb-6 flex flex-col flex-1 min-h-0">
          <div className="flex items-start gap-2">
            <ManagementListSearch
              value={search}
              onValueChange={setSearch}
              placeholder={t("vps.search")}
              ariaLabel={t("vps.search")}
              clearLabel={t("common.clear")}
              className="flex-1"
            />
            <Button
              ref={helpTriggerRef}
              variant="ghost"
              size="icon"
              aria-label={t("vps.help.title")}
              title={t("vps.help.title")}
              disabled={navigationBlocked}
              onClick={() => setHelpOpen(true)}
            >
              <HelpCircle className="h-4 w-4" aria-hidden="true" />
            </Button>
          </div>
          {query.isError && (
            <div
              role="alert"
              className="mb-4 rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm"
            >
              <p className="font-medium">{t("vps.loadFailed")}</p>
              <p className="mt-1 break-words">
                {extractErrorMessage(query.error)}
              </p>
              <Button
                variant="outline"
                size="sm"
                className="mt-3"
                disabled={query.isFetching || busy}
                onClick={() => void query.refetch()}
              >
                {t("vps.retry")}
              </Button>
            </div>
          )}
          {query.isLoading ? (
            <div
              role="status"
              aria-label={t("common.loading")}
              className="space-y-3 py-2"
            >
              {[0, 1, 2].map((item) => (
                <div
                  key={item}
                  className="h-16 rounded-lg bg-muted animate-pulse motion-reduce:animate-none"
                />
              ))}
            </div>
          ) : !query.isError && !query.data?.length ? (
            <div className="flex flex-col items-center justify-center gap-3 py-16 text-center">
              <Server
                className="h-9 w-9 text-muted-foreground"
                aria-hidden="true"
              />
              <h2 className="text-lg font-semibold">{t("vps.empty.title")}</h2>
              <p className="max-w-md text-sm text-muted-foreground">
                {t("vps.empty.description")}
              </p>
              <Button
                variant="outline"
                onClick={() => openEditor()}
                disabled={interactionBlocked}
              >
                <Plus className="mr-2 h-4 w-4" aria-hidden="true" />
                {t("vps.add")}
              </Button>
            </div>
          ) : servers.length === 0 && !query.isError ? (
            <p className="py-12 text-center text-sm text-muted-foreground">
              {t("vps.noResults")}
            </p>
          ) : (
            <div className="rounded-lg border border-border-default">
              {servers.map((server, index) => (
                <ListItemRow
                  key={server.id}
                  isLast={index === servers.length - 1}
                >
                  <div className="flex w-full min-w-0 flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                    <div className="min-w-0 flex-1">
                      <h2
                        id={`vps-host-${server.id}`}
                        className="truncate font-medium"
                        title={server.name}
                      >
                        {server.name}
                      </h2>
                      <p className="truncate font-mono text-xs text-muted-foreground">
                        {server.user}@{connectionAddress(server)}
                      </p>
                      {server.purpose && (
                        <p
                          className="mt-1 truncate text-sm text-muted-foreground"
                          title={server.purpose}
                        >
                          {server.purpose}
                        </p>
                      )}
                    </div>
                    <div className="flex flex-wrap items-center gap-3">
                      <div
                        role="group"
                        aria-label={t("vps.clientsFor", { name: server.name })}
                      >
                        <AppToggleGroup
                          apps={server.apps}
                          appIds={SKILLS_APP_IDS}
                          disabled={listDisabled}
                          onToggle={(app, enabled) =>
                            void saveServer({
                              ...server,
                              apps: { ...server.apps, [app]: enabled },
                            })
                          }
                        />
                      </div>
                      <div className="flex items-center gap-1">
                        {testingId === server.id && !editor ? (
                          <Button
                            variant="outline"
                            size="sm"
                            disabled={cancelling}
                            onClick={() => void cancelTest()}
                          >
                            <Loader2
                              className="mr-2 h-4 w-4 animate-spin motion-reduce:animate-none"
                              aria-hidden="true"
                            />
                            {t("vps.cancelTest")}
                          </Button>
                        ) : (
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label={t("vps.testConnection")}
                            aria-describedby={`vps-host-${server.id}`}
                            title={t("vps.testConnection")}
                            disabled={listDisabled}
                            onClick={() => void testConnection(server)}
                          >
                            <PlugZap className="h-4 w-4" aria-hidden="true" />
                          </Button>
                        )}
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t("vps.editServer", {
                            name: server.name,
                          })}
                          title={t("vps.edit")}
                          disabled={listDisabled}
                          onClick={() => openEditor(server)}
                        >
                          <Pencil className="h-4 w-4" aria-hidden="true" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t("vps.deleteServer", {
                            name: server.name,
                          })}
                          title={t("common.delete")}
                          disabled={listDisabled}
                          onClick={() => setDeleting(server)}
                        >
                          <Trash2 className="h-4 w-4" aria-hidden="true" />
                        </Button>
                      </div>
                    </div>
                  </div>
                </ListItemRow>
              ))}
            </div>
          )}
        </div>

        {editor && (
          <VpsServerForm
            title={t(editor.isNew ? "vps.add" : "vps.edit")}
            draft={editor.draft}
            password={editor.password}
            hasSavedPassword={editor.hasSavedPassword}
            onPasswordChange={(password) =>
              changeEditor({ ...editor, password })
            }
            disabled={editorDisabled}
            saving={writePending}
            testing={testingId === editor.draft.id}
            cancelling={cancelling}
            onChange={(draft) =>
              changeEditor({
                ...editor,
                draft,
                password:
                  draft.authMethod === editor.draft.authMethod
                    ? editor.password
                    : "",
              })
            }
            onSave={() => {
              const server =
                editorRef.current && prepareVpsServer(editorRef.current.draft);
              if (server)
                void saveServer(
                  server,
                  true,
                  editorRef.current?.password || undefined,
                );
            }}
            onCancel={requestCloseEditor}
            onTest={() => {
              const server =
                editorRef.current && prepareVpsServer(editorRef.current.draft);
              if (server)
                void testConnection(
                  server,
                  editorRef.current?.password || undefined,
                );
            }}
            onCancelTest={() => void cancelTest()}
          />
        )}

        <ConfirmDialog
          isOpen={discardOpen}
          zIndex="top"
          title={t("vps.discard.title")}
          message={t("vps.discard.message")}
          confirmText={t("vps.discard.confirm")}
          onConfirm={() => {
            setDiscardOpen(false);
            changeEditor(null);
          }}
          onCancel={() => setDiscardOpen(false)}
        />
        <ConfirmDialog
          isOpen={Boolean(deleting)}
          title={t("vps.deleteServer", { name: deleting?.name })}
          message={t("vps.deleteConfirm", { name: deleting?.name })}
          pending={writePending}
          onConfirm={() => void deleteServer()}
          onCancel={() => {
            if (!writeLock.current) setDeleting(null);
          }}
        />

        <Dialog
          open={Boolean(challenge)}
          onOpenChange={(open) => {
            if (!open) closeChallenge();
          }}
        >
          <DialogContent className="max-w-lg" zIndex="top">
            <DialogHeader>
              <DialogTitle>{t("vps.hostKey.title")}</DialogTitle>
              <DialogDescription>
                {t("vps.hostKey.description", { name: challenge?.server.name })}
              </DialogDescription>
            </DialogHeader>
            <p className="px-6 break-all font-mono text-sm">
              {challenge && connectionAddress(challenge.server)}
            </p>
            <pre className="mx-6 whitespace-pre-wrap break-all rounded-md bg-muted p-3 font-mono text-sm select-text">
              {challenge?.fingerprint}
            </pre>
            <DialogFooter>
              <Button
                variant="outline"
                disabled={confirmingKey}
                onClick={closeChallenge}
              >
                {t("common.cancel")}
              </Button>
              <Button
                disabled={confirmingKey}
                onClick={() => void confirmHostKey()}
              >
                {confirmingKey && (
                  <Loader2
                    className="mr-2 h-4 w-4 animate-spin motion-reduce:animate-none"
                    aria-hidden="true"
                  />
                )}
                {t("vps.hostKey.confirm")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog open={helpOpen} onOpenChange={setHelpOpen}>
          <DialogContent
            className="max-w-lg"
            zIndex="alert"
            onCloseAutoFocus={(event) => {
              event.preventDefault();
              helpTriggerRef.current?.focus();
            }}
          >
            <DialogHeader>
              <DialogTitle>{t("vps.help.title")}</DialogTitle>
              <DialogDescription>{t("vps.help.access")}</DialogDescription>
            </DialogHeader>
            <div className="min-h-0 space-y-3 overflow-y-auto px-6 py-4 text-sm leading-relaxed">
              <p>{t("vps.help.scope")}</p>
              <p>{t("vps.help.sessions")}</p>
              <p>{t("vps.help.cloud")}</p>
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setHelpOpen(false)}
              >
                {t("common.close")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </TooltipProvider>
    );
  },
);
VpsPanel.displayName = "VpsPanel";
export default VpsPanel;

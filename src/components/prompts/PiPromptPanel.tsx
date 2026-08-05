import React, { useEffect, useMemo, useRef, useState } from "react";
import { Check, Edit3, FileText, Loader2, Power, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ListItemRow } from "@/components/common/ListItemRow";
import { usePromptActions } from "@/hooks/usePromptActions";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import type { Prompt } from "@/lib/api";
import PromptFormPanel from "./PromptFormPanel";
import {
  PiPromptTemplates,
  PiSystemPromptFiles,
  type PiPromptTemplatesHandle,
} from "./PiNativePromptResources";

export type PiPromptTab = "global" | "system" | "templates";
export type PromptPrimaryAction = "prompt" | "template" | null;

interface PiPromptPanelProps {
  open: boolean;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
  onPrimaryActionChange?: (action: PromptPrimaryAction) => void;
}

export interface PiPromptPanelHandle {
  openAdd: () => void;
}

const actionForTab = (tab: PiPromptTab): PromptPrimaryAction => {
  if (tab === "global") return "prompt";
  if (tab === "templates") return "template";
  return null;
};

const PiPromptPanel = React.forwardRef<PiPromptPanelHandle, PiPromptPanelProps>(
  (
    {
      open,
      onInteractionBlockedChange,
      onNavigationBlockedChange,
      onPrimaryActionChange,
    },
    ref,
  ) => {
    const { t } = useTranslation();
    const [activeTab, setActiveTab] = useState<PiPromptTab>("global");
    const [isFormOpen, setIsFormOpen] = useState(false);
    const [editingId, setEditingId] = useState<string | null>(null);
    const [deletingPrompt, setDeletingPrompt] = useState<Prompt | null>(null);
    const templatesRef = useRef<PiPromptTemplatesHandle>(null);

    const {
      prompts,
      loading,
      currentFileContent,
      togglingId,
      reload,
      savePrompt,
      deletePrompt,
      toggleEnabled,
    } = usePromptActions("pi");
    const dialogOpen = deletingPrompt !== null;
    const writePending = Boolean(togglingId);
    const interactionBlocked =
      loading || writePending || isFormOpen || dialogOpen;
    const navigationBlocked = writePending || isFormOpen || dialogOpen;

    useEffect(() => {
      if (open) void reload();
    }, [open, reload]);

    useEffect(() => {
      onPrimaryActionChange?.(actionForTab(activeTab));
    }, [activeTab, onPrimaryActionChange]);

    useEffect(() => {
      onInteractionBlockedChange?.(interactionBlocked);
    }, [interactionBlocked, onInteractionBlockedChange]);

    useEffect(() => {
      onNavigationBlockedChange?.(navigationBlocked);
    }, [navigationBlocked, onNavigationBlockedChange]);

    useEffect(
      () => () => {
        onInteractionBlockedChange?.(false);
        onNavigationBlockedChange?.(false);
      },
      [onInteractionBlockedChange, onNavigationBlockedChange],
    );

    useEffect(() => {
      const handlePromptImported = (event: Event) => {
        const customEvent = event as CustomEvent;
        if (customEvent.detail?.app === "pi") {
          void reload();
        }
      };

      window.addEventListener("prompt-imported", handlePromptImported);
      return () =>
        window.removeEventListener("prompt-imported", handlePromptImported);
    }, [reload]);

    useTauriEvent("profile-applied", () => {
      void reload();
    });

    const openGlobalPromptForm = (id?: string) => {
      setEditingId(id ?? null);
      setIsFormOpen(true);
    };

    React.useImperativeHandle(
      ref,
      () => ({
        openAdd: () => {
          if (activeTab === "global") {
            openGlobalPromptForm();
          } else if (activeTab === "templates") {
            templatesRef.current?.openCreate();
          }
        },
      }),
      [activeTab],
    );

    const promptEntries = useMemo(() => Object.entries(prompts), [prompts]);
    const activePrompt = promptEntries.find(([, prompt]) => prompt.enabled);
    const hasExternalPrompt =
      currentFileContent !== null && activePrompt === undefined;
    const handleDelete = async () => {
      if (!deletingPrompt) return;
      try {
        await deletePrompt(deletingPrompt.id);
        setDeletingPrompt(null);
      } catch {
        // usePromptActions owns the error toast.
      }
    };

    return (
      <div className="flex min-h-0 flex-1 flex-col px-6">
        <Tabs
          value={activeTab}
          onValueChange={(value) => setActiveTab(value as PiPromptTab)}
          className="flex min-h-0 flex-1 flex-col"
        >
          <div className="flex shrink-0 py-4">
            <TabsList className="self-start">
              <TabsTrigger value="global">
                {t("pi.prompts.globalTab")}
              </TabsTrigger>
              <TabsTrigger value="system">
                {t("pi.prompts.systemTab")}
              </TabsTrigger>
              <TabsTrigger value="templates">
                {t("pi.prompts.templatesTab")}
              </TabsTrigger>
            </TabsList>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto pb-16">
            <TabsContent value="global" className="m-0">
              <section>
                <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <div className="flex flex-wrap items-center gap-2">
                      <h3 className="text-sm font-semibold">
                        {t("pi.prompts.agentsLibrary")}
                      </h3>
                      <Badge
                        variant={
                          activePrompt || hasExternalPrompt
                            ? "secondary"
                            : "outline"
                        }
                        className="font-normal"
                      >
                        {activePrompt ? (
                          <>
                            <Check
                              className="mr-1 h-3 w-3"
                              aria-hidden="true"
                            />
                            {t("pi.prompts.writtenToAgents")}
                          </>
                        ) : hasExternalPrompt ? (
                          t("pi.prompts.externalAgents")
                        ) : (
                          t("pi.prompts.noGlobalPrompt")
                        )}
                      </Badge>
                    </div>
                    <p className="mt-1 max-w-3xl text-xs leading-relaxed text-muted-foreground">
                      {t("pi.prompts.agentsLibraryDescription")}
                    </p>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {t("prompts.count", { count: promptEntries.length })}
                  </span>
                </div>

                {loading ? (
                  <div className="flex min-h-52 items-center justify-center gap-2 text-sm text-muted-foreground">
                    <Loader2
                      className="h-4 w-4 animate-spin"
                      aria-hidden="true"
                    />
                    {t("prompts.loading")}
                  </div>
                ) : promptEntries.length === 0 ? (
                  <div className="flex min-h-52 flex-col items-center justify-center rounded-xl border border-dashed px-6 text-center">
                    <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-muted">
                      <FileText
                        className="h-5 w-5 text-muted-foreground"
                        aria-hidden="true"
                      />
                    </div>
                    <h4 className="text-sm font-medium">
                      {t("pi.prompts.noGlobalPrompts")}
                    </h4>
                    <p className="mt-1 max-w-sm text-xs leading-relaxed text-muted-foreground">
                      {t("pi.prompts.noGlobalPromptsDescription")}
                    </p>
                  </div>
                ) : (
                  <div className="overflow-hidden rounded-xl border border-border bg-card">
                    {promptEntries.map(([id, prompt], index) => {
                      const busy = togglingId === id;
                      return (
                        <ListItemRow
                          key={id}
                          isLast={index === promptEntries.length - 1}
                        >
                          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                            <FileText className="h-4 w-4" aria-hidden="true" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="truncate text-sm font-medium">
                                {prompt.name}
                              </span>
                              {prompt.enabled && (
                                <Badge
                                  variant="secondary"
                                  className="shrink-0 font-normal"
                                >
                                  {t("pi.prompts.writtenToAgents")}
                                </Badge>
                              )}
                            </div>
                            <p className="mt-0.5 truncate text-xs text-muted-foreground">
                              {prompt.description ||
                                t("pi.prompts.noPromptDescription")}
                            </p>
                          </div>
                          <Button
                            type="button"
                            variant={prompt.enabled ? "outline" : "default"}
                            size="sm"
                            className="min-w-[72px] shrink-0"
                            disabled={Boolean(togglingId)}
                            onClick={() => {
                              void toggleEnabled(id, !prompt.enabled).catch(
                                () => undefined,
                              );
                            }}
                          >
                            {busy ? (
                              <Loader2
                                className="h-3.5 w-3.5 animate-spin"
                                aria-hidden="true"
                              />
                            ) : (
                              <Power
                                className="h-3.5 w-3.5"
                                aria-hidden="true"
                              />
                            )}
                            {prompt.enabled
                              ? t("pi.prompts.stopUsing")
                              : t("pi.prompts.usePrompt")}
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="shrink-0"
                            onClick={() => openGlobalPromptForm(id)}
                            title={t("common.edit")}
                          >
                            <Edit3 className="h-4 w-4" aria-hidden="true" />
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="shrink-0 hover:text-destructive"
                            disabled={prompt.enabled}
                            onClick={() => setDeletingPrompt(prompt)}
                            title={
                              prompt.enabled
                                ? t("pi.prompts.stopBeforeDelete")
                                : t("common.delete")
                            }
                          >
                            <Trash2 className="h-4 w-4" aria-hidden="true" />
                          </Button>
                        </ListItemRow>
                      );
                    })}
                  </div>
                )}
              </section>
            </TabsContent>

            <TabsContent value="system" className="m-0">
              <PiSystemPromptFiles />
            </TabsContent>

            <TabsContent value="templates" className="m-0">
              <PiPromptTemplates ref={templatesRef} />
            </TabsContent>
          </div>
        </Tabs>

        {isFormOpen && (
          <PromptFormPanel
            appId="pi"
            editingId={editingId ?? undefined}
            initialData={editingId ? prompts[editingId] : undefined}
            onSave={savePrompt}
            onClose={() => setIsFormOpen(false)}
          />
        )}

        <ConfirmDialog
          isOpen={Boolean(deletingPrompt)}
          title={t("prompts.confirm.deleteTitle")}
          message={t("prompts.confirm.deleteMessage", {
            name: deletingPrompt?.name,
          })}
          onConfirm={() => void handleDelete()}
          onCancel={() => setDeletingPrompt(null)}
        />
      </div>
    );
  },
);

PiPromptPanel.displayName = "PiPromptPanel";

export default PiPromptPanel;

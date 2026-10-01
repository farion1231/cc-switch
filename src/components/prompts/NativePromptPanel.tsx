import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { usePromptActions } from "@/hooks/usePromptActions";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import type { Prompt } from "@/lib/api";
import PromptFormPanel from "./PromptFormPanel";
import { PromptLibrary } from "./PromptLibrary";
import {
  NATIVE_PROMPT_RESOURCES_CONFIGS,
  NativePromptTemplates,
  NativeSystemPromptFiles,
  type NativePromptResourcesConfig,
  type NativePromptTemplatesHandle,
} from "./NativePromptResources";

export type NativePromptTab = "global" | "system" | "templates";
export type PromptPrimaryAction = "prompt" | "template" | null;

interface NativePromptPanelProps {
  appId: "pi" | "ohmypi";
  open: boolean;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
  onPrimaryActionChange?: (action: PromptPrimaryAction) => void;
}

export interface NativePromptPanelHandle {
  openAdd: () => void;
}

const actionForTab = (tab: NativePromptTab): PromptPrimaryAction => {
  if (tab === "global") return "prompt";
  if (tab === "templates") return "template";
  return null;
};

const NativePromptPanel = React.forwardRef<
  NativePromptPanelHandle,
  NativePromptPanelProps
>(
  (
    {
      appId,
      open,
      onInteractionBlockedChange,
      onNavigationBlockedChange,
      onPrimaryActionChange,
    },
    ref,
  ) => {
    const { t } = useTranslation();
    const config: NativePromptResourcesConfig =
      NATIVE_PROMPT_RESOURCES_CONFIGS[appId];
    const [activeTab, setActiveTab] = useState<NativePromptTab>("global");
    const [isFormOpen, setIsFormOpen] = useState(false);
    const [editingId, setEditingId] = useState<string | null>(null);
    const [searchQuery, setSearchQuery] = useState("");
    const [deletingPrompt, setDeletingPrompt] = useState<Prompt | null>(null);
    const templatesRef = useRef<NativePromptTemplatesHandle>(null);

    const {
      prompts,
      loading,
      currentFileContent,
      togglingId,
      reload,
      savePrompt,
      deletePrompt,
      toggleEnabled,
    } = usePromptActions(appId);
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
        if (customEvent.detail?.app === appId) {
          void reload();
        }
      };

      window.addEventListener("prompt-imported", handlePromptImported);
      return () =>
        window.removeEventListener("prompt-imported", handlePromptImported);
    }, [appId, reload]);

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

    const promptEntries = Object.entries(prompts);
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
          onValueChange={(value) => setActiveTab(value as NativePromptTab)}
          className="flex min-h-0 flex-1 flex-col"
        >
          <div className="flex shrink-0 py-4">
            <TabsList className="self-start">
              <TabsTrigger value="global">
                {t(`${config.i18nKey}.globalTab`)}
              </TabsTrigger>
              <TabsTrigger value="system">
                {t(`${config.i18nKey}.systemTab`)}
              </TabsTrigger>
              <TabsTrigger value="templates">
                {t(`${config.i18nKey}.templatesTab`)}
              </TabsTrigger>
            </TabsList>
          </div>

          <TabsContent
            value="global"
            className="m-0 min-h-0 flex-1 data-[state=active]:flex data-[state=active]:flex-col"
          >
            <PromptLibrary
              prompts={prompts}
              loading={loading}
              searchQuery={searchQuery}
              statusText={
                activePrompt
                  ? t("prompts.enabledName", { name: activePrompt[1].name })
                  : hasExternalPrompt
                    ? t(`${config.i18nKey}.externalAgents`)
                    : t("prompts.noneEnabled")
              }
              disabled={interactionBlocked}
              onSearchQueryChange={setSearchQuery}
              onToggle={(id, enabled) => {
                void toggleEnabled(id, enabled).catch(() => undefined);
              }}
              onEdit={openGlobalPromptForm}
              onDelete={(id) => {
                const prompt = prompts[id];
                if (prompt) setDeletingPrompt(prompt);
              }}
              isDeleteDisabled={(_id, prompt) => prompt.enabled}
              getDeleteTitle={(_id, prompt) =>
                prompt.enabled
                  ? t(`${config.i18nKey}.stopBeforeDelete`)
                  : t("common.delete")
              }
            />
          </TabsContent>

          <TabsContent
            value="system"
            className="m-0 min-h-0 flex-1 overflow-hidden"
          >
            <ScrollArea className="-mr-3 h-full" type="auto">
              <div className="pb-16 pr-3">
                <NativeSystemPromptFiles config={config} />
              </div>
            </ScrollArea>
          </TabsContent>

          <TabsContent value="templates" className="m-0 min-h-0 min-w-0 flex-1">
            <NativePromptTemplates ref={templatesRef} config={config} />
          </TabsContent>
        </Tabs>

        {isFormOpen && (
          <PromptFormPanel
            appId={appId}
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

NativePromptPanel.displayName = "NativePromptPanel";

export default NativePromptPanel;

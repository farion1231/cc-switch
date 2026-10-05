import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { promptsApi, type AppId, type Prompt } from "@/lib/api";
import { usePromptActions } from "@/hooks/usePromptActions";
import { useDelayedFlag } from "@/hooks/useDelayedFlag";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import PiPromptPanel from "./PiPromptPanel";
import PromptFormPanel from "./PromptFormPanel";
import {
  PromptEmptyState,
  PromptLibrary,
  filterPromptEntries,
} from "./PromptLibrary";
import { PromptCopyDialog } from "./PromptCopyDialog";
import { PromptPageFrame } from "./PromptPageFrame";
import { toggleEffectText, usePromptPageCommon } from "./usePromptPageCommon";
import {
  copyText,
  formatSize,
  renderSegs,
  showPromptToast,
  utf8Bytes,
} from "./promptUtils";

export interface PromptPanelProps {
  appId: AppId;
  /** 应用下拉里的应用（只列「应用」页里设为显示、且支持提示词的） */
  apps: AppId[];
  onAppChange: (app: AppId) => void;
  open?: boolean;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
}

const StandardPromptPanel: React.FC<PromptPanelProps> = ({
  open = true,
  appId,
  apps,
  onAppChange,
  onInteractionBlockedChange,
  onNavigationBlockedChange,
}) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [copyingId, setCopyingId] = useState<string | null>(null);
  const [writePending, setWritePending] = useState(false);
  const [reloadPending, setReloadPending] = useState(false);
  const writeLockRef = React.useRef(false);
  const reloadLockRef = React.useRef(false);
  const reloadRunGenerationRef = React.useRef(0);
  const overlayOpenRef = React.useRef(false);
  const externalReloadQueuedRef = React.useRef(false);
  const appIdRef = React.useRef(appId);
  appIdRef.current = appId;

  const {
    prompts,
    loading,
    currentFileContent,
    reload,
    savePrompt,
    deletePrompt,
    toggleEnabled,
    importFromFile,
    getLatestPrompts,
  } = usePromptActions(appId, { notify: false });
  const reloadRef = React.useRef(reload);
  reloadRef.current = reload;
  const { displayPath, location, fileName, invalidate, moreItems } =
    usePromptPageCommon(appId);

  const dialogOpen = copyingId !== null;
  const interactionBlocked =
    loading || reloadPending || writePending || isFormOpen || dialogOpen;
  const navigationBlocked = writePending || isFormOpen || dialogOpen;
  // 外观上的禁用晚 300ms 才出现：切回窗口时的重读、点一下启用这类很快的操作，
  // 不让整列按钮闪一下变灰。拦截仍看 interactionBlocked 和写锁；表单、对话框打开时照常立即禁用。
  const controlsDisabled =
    useDelayedFlag(loading || reloadPending || writePending) ||
    isFormOpen ||
    dialogOpen;

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

  const runExternalReload = React.useCallback(async () => {
    if (writeLockRef.current || overlayOpenRef.current) {
      externalReloadQueuedRef.current = true;
      return;
    }

    const runGeneration = ++reloadRunGenerationRef.current;
    externalReloadQueuedRef.current = false;
    reloadLockRef.current = true;
    setReloadPending(true);
    try {
      await reloadRef.current();
    } finally {
      if (reloadRunGenerationRef.current === runGeneration) {
        reloadLockRef.current = false;
        setReloadPending(false);
      }
    }
  }, []);

  const beginWrite = () => {
    if (loading || reloadLockRef.current || writeLockRef.current) return false;
    writeLockRef.current = true;
    setWritePending(true);
    return true;
  };

  const endWrite = () => {
    writeLockRef.current = false;
    setWritePending(false);
    invalidate();
    if (externalReloadQueuedRef.current) {
      void runExternalReload();
    }
  };

  /** 撤销等页外写入之后：刷新缓存；还停在同一个应用就重读列表。 */
  const afterUndo = (app: AppId) => {
    invalidate();
    if (appIdRef.current === app) void runExternalReload();
  };

  useEffect(() => {
    if (open) void runExternalReload();
  }, [appId, open, runExternalReload]);

  useEffect(() => {
    if (!open) return;
    const handleFocus = () => void runExternalReload();
    window.addEventListener("focus", handleFocus);
    return () => window.removeEventListener("focus", handleFocus);
  }, [open, runExternalReload]);

  useEffect(() => {
    setSearchQuery("");
    overlayOpenRef.current = false;
    setIsFormOpen(false);
    setEditingId(null);
    setCopyingId(null);
    if (externalReloadQueuedRef.current) {
      void runExternalReload();
    }
  }, [appId, runExternalReload]);

  useEffect(() => {
    const handlePromptImported = (event: Event) => {
      const customEvent = event as CustomEvent;
      if (customEvent.detail?.app === appId) {
        void runExternalReload();
      }
    };

    window.addEventListener("prompt-imported", handlePromptImported);
    return () => {
      window.removeEventListener("prompt-imported", handlePromptImported);
    };
  }, [appId, runExternalReload]);

  useTauriEvent("profile-applied", runExternalReload);

  const handleAdd = () => {
    if (reloadLockRef.current || writeLockRef.current || interactionBlocked) {
      return;
    }
    overlayOpenRef.current = true;
    setEditingId(null);
    setIsFormOpen(true);
  };

  const handleEdit = (id: string) => {
    if (reloadLockRef.current || writeLockRef.current || interactionBlocked) {
      return;
    }
    overlayOpenRef.current = true;
    setEditingId(id);
    setIsFormOpen(true);
  };

  const closeOverlay = () => {
    overlayOpenRef.current = false;
    setIsFormOpen(false);
    setEditingId(null);
    setCopyingId(null);
  };

  // 删除不弹确认框：只删库、不碰文件，toast 给「撤销」（原 id、原位置、原更新时间写回）
  const performDelete = async (id: string) => {
    const original = prompts[id];
    if (!original || original.enabled) return;
    if (!beginWrite()) return;
    const app = appId;
    try {
      const refreshed = await deletePrompt(id);
      if (refreshed === false) {
        externalReloadQueuedRef.current = true;
      }
      showPromptToast(t, {
        title: t("prompts.toast.deleted", { name: original.name }),
        description: t("prompts.toast.deletedSub", { path: displayPath }),
        undoneTitle: t("prompts.toast.restored", { name: original.name }),
        onUndo: async () => {
          await promptsApi.upsertPrompt(app, id, original);
          afterUndo(app);
        },
      });
    } catch {
      // Error handled by hook
    } finally {
      endWrite();
    }
  };

  const handleDelete = (id: string) => {
    if (reloadLockRef.current || writeLockRef.current || interactionBlocked) {
      return;
    }
    void performDelete(id);
  };

  const handleDeleteFromDrawer = (id: string) => {
    if (writeLockRef.current) return;
    closeOverlay();
    void performDelete(id);
  };

  const handleToggle = async (id: string, enabled: boolean) => {
    const before = prompts;
    const target = before[id];
    if (!target) return;
    const previousActive = Object.entries(before).find(
      ([key, prompt]) => key !== id && prompt.enabled,
    );
    const fileWasBlank = !currentFileContent?.trim();
    if (!beginWrite()) return;
    const app = appId;
    try {
      const refreshed = await toggleEnabled(id, enabled);
      if (refreshed === false) {
        externalReloadQueuedRef.current = true;
      }
      const effect = toggleEffectText(t, app, enabled);
      if (!enabled) {
        showPromptToast(t, {
          title: t("prompts.toast.disabled", {
            name: target.name,
            path: displayPath,
          }),
          description: effect || undefined,
          onUndo: async () => {
            await promptsApi.enablePrompt(app, id);
            afterUndo(app);
          },
        });
        return;
      }
      // 启用前文件里有库里没有的内容，后端会先存成一条「原始提示词」
      const after = getLatestPrompts() ?? {};
      const backup = Object.entries(after).find(
        ([key]) => !(key in before) && key.startsWith("backup-"),
      );
      const description = [
        backup ? t("prompts.toast.backup", { name: backup[1].name }) : "",
        effect,
      ]
        .filter(Boolean)
        .join(t("prompts.toast.joiner"));
      // 撤销只给能原样还原的：之前有启用中的就换回去；之前文件是空的就停用
      const onUndo = previousActive
        ? async () => {
            await promptsApi.enablePrompt(app, previousActive[0]);
            afterUndo(app);
          }
        : fileWasBlank && !backup
          ? async () => {
              const latest = (await promptsApi.getPrompts(app))[id] ?? target;
              await promptsApi.upsertPrompt(app, id, {
                ...latest,
                enabled: false,
              });
              afterUndo(app);
            }
          : undefined;
      showPromptToast(t, {
        title: t("prompts.toast.enabled", {
          name: target.name,
          path: displayPath,
        }),
        description: description || undefined,
        onUndo,
      });
    } catch {
      // Error handled by hook
    } finally {
      endWrite();
    }
  };

  const handleSave = async (id: string, prompt: Prompt) => {
    if (!beginWrite()) return false;
    const isNew = !(id in prompts);
    try {
      const refreshed = await savePrompt(id, prompt);
      if (refreshed === false) {
        externalReloadQueuedRef.current = true;
      }
      if (isNew) {
        showPromptToast(t, {
          title: t("prompts.toast.added", { name: prompt.name }),
        });
      } else if (prompt.enabled) {
        showPromptToast(t, {
          title: t("prompts.toast.savedActive", {
            name: prompt.name,
            path: displayPath,
          }),
          description: toggleEffectText(t, appId, true) || undefined,
        });
      } else {
        showPromptToast(t, {
          title: t("prompts.toast.saved", { name: prompt.name }),
        });
      }
      return true;
    } catch {
      // Error handled by hook
      return false;
    } finally {
      endWrite();
    }
  };

  const handleCloseForm = () => {
    if (writeLockRef.current) return;
    closeOverlay();
    if (externalReloadQueuedRef.current) {
      void runExternalReload();
    }
  };

  const handleImport = async () => {
    if (reloadLockRef.current || writeLockRef.current || interactionBlocked) {
      return;
    }
    if (!beginWrite()) return;
    const app = appId;
    try {
      const id = await importFromFile();
      showPromptToast(t, {
        title: t("prompts.toast.imported", { path: displayPath }),
        onUndo: async () => {
          await promptsApi.deletePrompt(app, id);
          afterUndo(app);
        },
      });
    } catch {
      // Error handled by hook
    } finally {
      endWrite();
    }
  };

  const handleCopyContent = (id: string) => {
    const prompt = prompts[id];
    if (!prompt) return;
    void copyText(prompt.content).then((ok) => {
      if (ok) {
        showPromptToast(t, {
          title: t("prompts.copiedContent", { name: prompt.name }),
        });
      } else {
        toast.error(t("prompts.copyFailed"));
      }
    });
  };

  const handleOpenCopy = (id: string) => {
    if (reloadLockRef.current || writeLockRef.current || interactionBlocked) {
      return;
    }
    overlayOpenRef.current = true;
    setCopyingId(id);
  };

  const handleCopyToApps = async (targets: AppId[]) => {
    const source = copyingId ? prompts[copyingId] : undefined;
    if (!source) return;
    if (!beginWrite()) return;
    const timestamp = Math.floor(Date.now() / 1000);
    const created: Array<{ app: AppId; id: string }> = [];
    const failed: AppId[] = [];
    for (const target of targets) {
      const id = `prompt-${Date.now()}`;
      try {
        await promptsApi.upsertPrompt(target, id, {
          id,
          name: source.name,
          description: source.description,
          content: source.content,
          enabled: false,
          createdAt: timestamp,
          updatedAt: timestamp,
        });
        created.push({ app: target, id });
      } catch {
        failed.push(target);
      }
    }
    closeOverlay();
    endWrite();
    if (created.length) {
      showPromptToast(t, {
        title: t("prompts.toast.copied", {
          name: source.name,
          apps: created
            .map((item) => APP_DISPLAY_NAME[item.app])
            .join(t("prompts.listSeparator")),
        }),
        onUndo: async () => {
          for (const item of created) {
            await promptsApi.deletePrompt(item.app, item.id);
          }
          invalidate();
        },
      });
    }
    if (failed.length) {
      toast.error(
        t("prompts.copyDialog.failed", {
          apps: failed
            .map((app) => APP_DISPLAY_NAME[app])
            .join(t("prompts.listSeparator")),
        }),
      );
    }
  };

  const promptEntries = useMemo(() => Object.entries(prompts), [prompts]);
  const listEmpty = promptEntries.length === 0;
  const fileText = currentFileContent ?? null;
  const fileHasContent = Boolean(fileText?.trim());
  const duplicate = fileHasContent
    ? promptEntries.find(
        ([, prompt]) => prompt.content.trim() === fileText!.trim(),
      )
    : undefined;
  const importReason =
    fileText === null
      ? t("prompts.importReason.missing", { file: fileName })
      : !fileText.trim()
        ? t("prompts.importReason.empty", { file: fileName })
        : duplicate
          ? t("prompts.importReason.duplicate", { name: duplicate[1].name })
          : undefined;
  const fileMeta =
    fileText === null
      ? t("prompts.fileMissing")
      : !fileText.trim()
        ? t("prompts.fileEmpty")
        : formatSize(utf8Bytes(fileText));
  const resultCount = filterPromptEntries(promptEntries, searchQuery).length;
  const isHermes = appId === "hermes";

  return (
    <PromptPageFrame
      app={appId}
      apps={apps}
      onAppChange={onAppChange}
      appSwitchDisabled={navigationBlocked}
      count={promptEntries.length}
      help={
        isHermes
          ? {
              title: t("prompts.helpHermesTitle"),
              text: t("prompts.helpHermesText"),
            }
          : { title: t("prompts.helpTitle"), text: t("prompts.helpText") }
      }
      primary={{
        label: t("prompts.add"),
        onClick: handleAdd,
        disabled: controlsDisabled,
      }}
      moreItems={moreItems(
        {
          key: "import",
          label: renderSegs(t("prompts.importCurrent", { file: fileName })),
          reason: importReason,
          onSelect: () => void handleImport(),
        },
        { label: t("prompts.copyFilePath"), path: location.data?.path },
      )}
      target={{
        label: t("prompts.targetFile"),
        path: displayPath,
        meta: ` · ${fileMeta}`,
      }}
      search={
        listEmpty
          ? null
          : {
              value: searchQuery,
              onChange: setSearchQuery,
              placeholder: t("prompts.searchPlaceholder"),
              ariaLabel: t("prompts.searchAriaLabel"),
              resultCount,
            }
      }
    >
      {listEmpty ? (
        loading ? (
          <div className="py-12 text-center text-body text-fg-2">
            {t("prompts.loading")}
          </div>
        ) : (
          <PromptEmptyState
            app={appId}
            displayPath={displayPath}
            fileText={fileText}
            disabled={controlsDisabled}
            onImport={() => void handleImport()}
            onAdd={handleAdd}
          />
        )
      ) : (
        <PromptLibrary
          prompts={prompts}
          searchQuery={searchQuery}
          listLabel={t("prompts.listLabel", { app: APP_DISPLAY_NAME[appId] })}
          disabled={controlsDisabled}
          onClearSearch={() => setSearchQuery("")}
          onToggle={(id, enabled) => void handleToggle(id, enabled)}
          onEdit={handleEdit}
          onDelete={handleDelete}
          onCopyToApps={apps.length > 1 ? handleOpenCopy : undefined}
          onCopyContent={handleCopyContent}
        />
      )}

      {isFormOpen && (
        <PromptFormPanel
          appId={appId}
          editingId={editingId || undefined}
          initialData={editingId ? prompts[editingId] : undefined}
          onSave={handleSave}
          onClose={handleCloseForm}
          onDelete={handleDeleteFromDrawer}
        />
      )}

      {copyingId && prompts[copyingId] ? (
        <PromptCopyDialog
          prompt={prompts[copyingId]}
          targets={apps.filter((app) => app !== appId)}
          pending={writePending}
          onCancel={() => {
            if (writeLockRef.current) return;
            closeOverlay();
            if (externalReloadQueuedRef.current) void runExternalReload();
          }}
          onConfirm={(targets) => void handleCopyToApps(targets)}
        />
      ) : null}
    </PromptPageFrame>
  );
};

const PromptPanel: React.FC<PromptPanelProps> = (props) => {
  if (props.appId === "pi") {
    return (
      <PiPromptPanel
        open={props.open ?? true}
        apps={props.apps}
        onAppChange={props.onAppChange}
        onInteractionBlockedChange={props.onInteractionBlockedChange}
        onNavigationBlockedChange={props.onNavigationBlockedChange}
      />
    );
  }

  return <StandardPromptPanel {...props} />;
};

export default PromptPanel;

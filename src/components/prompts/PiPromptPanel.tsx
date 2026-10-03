import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { promptsApi, type AppId, type Prompt } from "@/lib/api";
import { usePromptActions } from "@/hooks/usePromptActions";
import { useDelayedFlag } from "@/hooks/useDelayedFlag";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import PromptFormPanel from "./PromptFormPanel";
import { PromptCopyDialog } from "./PromptCopyDialog";
import {
  PromptEmptyState,
  PromptLibrary,
  filterPromptEntries,
} from "./PromptLibrary";
import { PromptPageFrame } from "./PromptPageFrame";
import {
  PiPromptTemplates,
  PiSystemPromptFiles,
  usePiPromptTemplatesQuery,
  type PiPromptTemplatesHandle,
} from "./PiNativePromptResources";
import { usePromptPageCommon } from "./usePromptPageCommon";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  copyText,
  formatSize,
  renderSegs,
  showPromptToast,
  utf8Bytes,
} from "./promptUtils";
import { toast } from "@/lib/toast";

export type PiPromptTab = "global" | "system" | "templates";

interface PiPromptPanelProps {
  open: boolean;
  apps: AppId[];
  onAppChange: (app: AppId) => void;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
}

/** 去掉路径最后一段（AGENTS.md），留下目录，末尾带分隔符。 */
function parentDir(path: string): string {
  const index = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return index >= 0 ? path.slice(0, index + 1) : path;
}

/** Pi 的提示词页（PromptsPi.dc.html）：提示库 | 系统提示 | 模板 三段。 */
const PiPromptPanel: React.FC<PiPromptPanelProps> = ({
  open,
  apps,
  onAppChange,
  onInteractionBlockedChange,
  onNavigationBlockedChange,
}) => {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<PiPromptTab>("global");
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [copyingId, setCopyingId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [templateQuery, setTemplateQuery] = useState("");
  const [busy, setBusy] = useState(false);
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
    importFromFile,
    getLatestPrompts,
  } = usePromptActions("pi", { notify: false });
  const { displayPath, location, invalidate, moreItems } =
    usePromptPageCommon("pi");
  const templates = usePiPromptTemplatesQuery();

  const dialogOpen = copyingId !== null;
  const writePending = Boolean(togglingId) || busy;
  const interactionBlocked =
    loading || writePending || isFormOpen || dialogOpen;
  const navigationBlocked = writePending || isFormOpen || dialogOpen;
  // 外观上的禁用晚 300ms 才出现：点一下启用这类很快的写入不让整列按钮闪一下变灰。
  // 拦截仍看 interactionBlocked；表单、对话框打开时照常立即禁用。
  const controlsDisabled =
    useDelayedFlag(loading || writePending) || isFormOpen || dialogOpen;

  useEffect(() => {
    if (open) void reload();
  }, [open, reload]);

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

  const afterUndo = () => {
    invalidate();
    void reload();
  };

  /** 包一层：写的过程中锁住页面，写完刷新别处的缓存。 */
  const runWrite = async (write: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    try {
      await write();
    } catch {
      // usePromptActions owns the error toast.
    } finally {
      setBusy(false);
      invalidate();
    }
  };

  const openGlobalPromptForm = (id?: string) => {
    if (interactionBlocked) return;
    setEditingId(id ?? null);
    setIsFormOpen(true);
  };

  const promptEntries = useMemo(() => Object.entries(prompts), [prompts]);
  const activePrompt = promptEntries.find(([, prompt]) => prompt.enabled);
  const fileText = currentFileContent ?? null;
  const fileHasContent = Boolean(fileText?.trim());
  const hasExternalPrompt = fileHasContent && activePrompt === undefined;

  const handleToggle = (id: string, enabled: boolean) => {
    if (interactionBlocked) return;
    const before = prompts;
    const target = before[id];
    if (!target) return;
    const previousActive = Object.entries(before).find(
      ([key, prompt]) => key !== id && prompt.enabled,
    );
    const fileWasMissing = currentFileContent === null;
    void runWrite(async () => {
      await toggleEnabled(id, enabled);
      if (!enabled) {
        showPromptToast(t, {
          title: t("prompts.toast.disabledPi", { name: target.name }),
          description: t("pi.prompts.reloadNotice"),
          onUndo: async () => {
            await promptsApi.enablePrompt("pi", id);
            afterUndo();
          },
        });
        return;
      }
      const after = getLatestPrompts() ?? {};
      const backup = Object.entries(after).find(
        ([key]) => !(key in before) && key.startsWith("backup-"),
      );
      const description = [
        backup ? t("prompts.toast.backup", { name: backup[1].name }) : "",
        t("pi.prompts.reloadNotice"),
      ]
        .filter(Boolean)
        .join(t("prompts.toast.joiner"));
      const onUndo = previousActive
        ? async () => {
            await promptsApi.enablePrompt("pi", previousActive[0]);
            afterUndo();
          }
        : fileWasMissing && !backup
          ? async () => {
              const latest = (await promptsApi.getPrompts("pi"))[id] ?? target;
              await promptsApi.upsertPrompt("pi", id, {
                ...latest,
                enabled: false,
              });
              afterUndo();
            }
          : undefined;
      showPromptToast(t, {
        title: t("prompts.toast.enabledPi", { name: target.name }),
        description,
        onUndo,
      });
    });
  };

  const performDelete = (id: string) => {
    const original = prompts[id];
    if (!original || original.enabled) return;
    void runWrite(async () => {
      await deletePrompt(id);
      showPromptToast(t, {
        title: t("prompts.toast.deleted", { name: original.name }),
        description: t("prompts.toast.deletedPiSub"),
        undoneTitle: t("prompts.toast.restored", { name: original.name }),
        onUndo: async () => {
          await promptsApi.upsertPrompt("pi", id, original);
          afterUndo();
        },
      });
    });
  };

  const handleSave = async (id: string, prompt: Prompt) => {
    const isNew = !(id in prompts);
    try {
      setBusy(true);
      await savePrompt(id, prompt);
      showPromptToast(t, {
        title: isNew
          ? t("prompts.toast.added", { name: prompt.name })
          : prompt.enabled
            ? t("prompts.toast.savedActivePi", { name: prompt.name })
            : t("prompts.toast.saved", { name: prompt.name }),
        description:
          !isNew && prompt.enabled ? t("pi.prompts.reloadNotice") : undefined,
      });
      return true;
    } catch {
      return false;
    } finally {
      setBusy(false);
      invalidate();
    }
  };

  const handleImport = () => {
    if (interactionBlocked) return;
    void runWrite(async () => {
      await importFromFile();
      // 不给「撤销」：导入的那条和 AGENTS.md 一字不差，Pi 按内容相等判定它就是
      // 启用中的，delete_pi_prompt 一定拒绝（无法删除已启用的提示词）
      showPromptToast(t, { title: t("prompts.toast.importedPi") });
    });
  };

  const handleCopyToApps = async (targets: AppId[]) => {
    const source = copyingId ? prompts[copyingId] : undefined;
    if (!source) return;
    setBusy(true);
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
    setBusy(false);
    setCopyingId(null);
    invalidate();
    const separator = t("prompts.listSeparator");
    if (created.length) {
      showPromptToast(t, {
        title: t("prompts.toast.copied", {
          name: source.name,
          apps: created
            .map((item) => APP_DISPLAY_NAME[item.app])
            .join(separator),
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
          apps: failed.map((app) => APP_DISPLAY_NAME[app]).join(separator),
        }),
      );
    }
  };

  const duplicate = fileHasContent
    ? promptEntries.find(([, prompt]) => prompt.content === fileText)
    : undefined;
  const importReason =
    fileText === null
      ? t("prompts.importReason.missing", { file: "AGENTS.md" })
      : !fileText.trim()
        ? t("prompts.importReason.empty", { file: "AGENTS.md" })
        : duplicate
          ? t("prompts.importReason.duplicate", { name: duplicate[1].name })
          : undefined;

  const agentDir = parentDir(displayPath);
  const agentDirFull = location.data
    ? parentDir(location.data.path)
    : undefined;
  const templateCount = templates.data?.length ?? 0;
  const libEmpty = promptEntries.length === 0;

  const target =
    activeTab === "global"
      ? {
          label: t("prompts.targetFile"),
          path: displayPath,
          meta: ` · ${
            fileText === null
              ? t("prompts.fileMissing")
              : !fileText.trim()
                ? t("prompts.fileEmpty")
                : formatSize(utf8Bytes(fileText))
          }`,
        }
      : {
          label: t("prompts.targetDir"),
          path: activeTab === "templates" ? `${agentDir}prompts/` : agentDir,
        };

  const search =
    activeTab === "global" && !libEmpty
      ? {
          value: searchQuery,
          onChange: setSearchQuery,
          placeholder: t("prompts.searchPiLibrary"),
          ariaLabel: t("prompts.searchAriaLabel"),
          resultCount: filterPromptEntries(promptEntries, searchQuery).length,
          width: 168,
        }
      : activeTab === "templates" && templateCount > 0
        ? {
            value: templateQuery,
            onChange: setTemplateQuery,
            placeholder: t("pi.prompts.searchTemplates"),
            ariaLabel: t("pi.prompts.searchTemplatesLabel"),
            width: 168,
          }
        : null;

  return (
    <PromptPageFrame
      app="pi"
      apps={apps}
      onAppChange={onAppChange}
      appSwitchDisabled={navigationBlocked}
      count={promptEntries.length}
      help={{ title: t("prompts.helpPiTitle"), text: t("prompts.helpPiText") }}
      primary={
        activeTab === "system"
          ? null
          : {
              label:
                activeTab === "templates"
                  ? t("pi.prompts.newTemplate")
                  : t("prompts.add"),
              disabled: controlsDisabled,
              onClick: () => {
                if (interactionBlocked) return;
                if (activeTab === "templates") {
                  templatesRef.current?.openCreate();
                } else {
                  openGlobalPromptForm();
                }
              },
            }
      }
      moreItems={moreItems(
        {
          key: "import",
          label: renderSegs(t("prompts.importCurrent", { file: "AGENTS.md" })),
          reason: importReason,
          onSelect: handleImport,
        },
        activeTab === "global"
          ? { label: t("prompts.copyFilePath"), path: location.data?.path }
          : {
              label: t("prompts.copyFolderPath"),
              path:
                agentDirFull &&
                (activeTab === "templates"
                  ? `${agentDirFull}prompts`
                  : agentDirFull),
            },
      )}
      segments={
        <SegmentedControl
          size="sm"
          aria-label={t("pi.prompts.kindLabel")}
          value={activeTab}
          onValueChange={(tab) => {
            setActiveTab(tab);
            setSearchQuery("");
            setTemplateQuery("");
          }}
          items={[
            { value: "global", label: t("pi.prompts.globalTab") },
            { value: "system", label: t("pi.prompts.systemTab") },
            { value: "templates", label: t("pi.prompts.templatesTab") },
          ]}
          className="h-8 shrink-0"
        />
      }
      target={target}
      search={search}
      notices={
        activeTab === "global" && hasExternalPrompt && !libEmpty ? (
          <Notice
            tone="warning"
            title={renderSegs(t("pi.prompts.externalTitle"))}
            actions={
              <Button
                variant="neutral"
                size="compact"
                disabled={controlsDisabled}
                onClick={handleImport}
              >
                {t("pi.prompts.saveToLibrary")}
              </Button>
            }
          >
            {renderSegs(
              t("pi.prompts.externalDetail", {
                size: formatSize(utf8Bytes(fileText ?? "")),
              }),
            )}
          </Notice>
        ) : null
      }
    >
      {activeTab === "global" ? (
        libEmpty ? (
          loading ? (
            <div className="py-12 text-center text-body text-fg-2">
              {t("prompts.loading")}
            </div>
          ) : (
            <PromptEmptyState
              app="pi"
              displayPath={displayPath}
              fileText={fileText}
              disabled={controlsDisabled}
              onImport={handleImport}
              onAdd={() => openGlobalPromptForm()}
            />
          )
        ) : (
          <PromptLibrary
            prompts={prompts}
            searchQuery={searchQuery}
            listLabel={t("pi.prompts.libraryLabel")}
            disabled={controlsDisabled}
            onClearSearch={() => setSearchQuery("")}
            onToggle={handleToggle}
            onEdit={openGlobalPromptForm}
            onDelete={(id) => {
              if (!interactionBlocked) performDelete(id);
            }}
            onCopyToApps={
              apps.length > 1
                ? (id) => {
                    if (!interactionBlocked) setCopyingId(id);
                  }
                : undefined
            }
            onCopyContent={(id) => {
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
            }}
          />
        )
      ) : activeTab === "system" ? (
        <PiSystemPromptFiles />
      ) : (
        <PiPromptTemplates
          ref={templatesRef}
          search={templateQuery}
          onClearSearch={() => setTemplateQuery("")}
        />
      )}

      {isFormOpen && (
        <PromptFormPanel
          appId="pi"
          editingId={editingId ?? undefined}
          initialData={editingId ? prompts[editingId] : undefined}
          onSave={handleSave}
          onClose={() => setIsFormOpen(false)}
          onDelete={(id) => {
            setIsFormOpen(false);
            performDelete(id);
          }}
        />
      )}

      {copyingId && prompts[copyingId] ? (
        <PromptCopyDialog
          prompt={prompts[copyingId]}
          targets={apps.filter((app) => app !== "pi")}
          pending={busy}
          onCancel={() => {
            if (!busy) setCopyingId(null);
          }}
          onConfirm={(targets) => void handleCopyToApps(targets)}
        />
      ) : null}
    </PromptPageFrame>
  );
};

PiPromptPanel.displayName = "PiPromptPanel";

export default PiPromptPanel;

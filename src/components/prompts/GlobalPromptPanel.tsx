import React, { useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { MoreHorizontal, Plus } from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HoverTip } from "@/components/ui/hover-tip";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { MatrixCell, NeutralBadge } from "@/components/mcp/AppMatrix";
import {
  promptsApi,
  type AppId,
  type GlobalPromptEntry,
  type Prompt,
} from "@/lib/api";
import {
  promptKeys,
  useGlobalPromptsQuery,
  type PromptScope,
} from "@/lib/query/prompts";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import PromptFormPanel from "./PromptFormPanel";
import {
  PromptPageFrame,
  promptMenuContentClass,
  promptMenuItemClass,
} from "./PromptPageFrame";
import { toggleEffectText } from "./usePromptPageCommon";
import {
  globalPromptApps,
  joinAppNames,
  reportGlobalSave,
} from "./useGlobalPromptLinks";
import {
  copyText,
  firstLine,
  formatSize,
  formatUpdated,
  promptFileName,
  showPromptToast,
  utf8Bytes,
} from "./promptUtils";

interface GlobalPromptPanelProps {
  open: boolean;
  /** 矩阵的列：应用下拉里列出的应用 */
  apps: AppId[];
  onAppChange: (scope: PromptScope) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
}

const failKey = (id: string, app: AppId) => `${id}:${app}`;

function matches(entry: GlobalPromptEntry, query: string): boolean {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return true;
  return [entry.name, entry.description, entry.content].some((value) =>
    value?.toLocaleLowerCase().includes(normalized),
  );
}

function asPrompt(entry: GlobalPromptEntry): Prompt {
  return {
    id: entry.id,
    name: entry.name,
    content: entry.content,
    description: entry.description,
    enabled: false,
    createdAt: entry.createdAt,
    updatedAt: entry.updatedAt,
  };
}

/** 所有应用里正在使用它的（含应用下拉里隐藏的），删除前要确认这里为空 */
function appliedAnywhere(entry: GlobalPromptEntry): AppId[] {
  return (Object.keys(entry.apps) as AppId[]).filter((app) => entry.apps[app]);
}

/**
 * 全局提示词（同 Skills 的统一管理）：一份内容，勾选要用它的应用。
 * 每个应用同时只启用一条提示词，所以勾上某个应用会替换它当前启用的那条；
 * 取消勾选和在应用列表里停用一样，会清空该应用的目标文件。
 */
const GlobalPromptPanel: React.FC<GlobalPromptPanelProps> = ({
  open,
  apps,
  onAppChange,
  onNavigationBlockedChange,
}) => {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const query = useGlobalPromptsQuery(open);
  const entries = useMemo(() => query.data ?? [], [query.data]);
  const [searchQuery, setSearchQuery] = useState("");
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<GlobalPromptEntry | null>(null);
  const [busy, setBusy] = useState(false);
  const [fails, setFails] = useState<Record<string, string>>({});
  const busyRef = useRef(false);

  const initialData = useMemo(
    () => (editing ? asPrompt(editing) : undefined),
    [editing],
  );
  const filtered = useMemo(
    () => entries.filter((entry) => matches(entry, searchQuery)),
    [entries, searchQuery],
  );
  const controlsDisabled = busy || formOpen;

  useEffect(() => {
    onNavigationBlockedChange?.(busy);
  }, [busy, onNavigationBlockedChange]);

  useEffect(
    () => () => onNavigationBlockedChange?.(false),
    [onNavigationBlockedChange],
  );

  // 切回窗口时重读：文件可能在别处改过，应用列表里也可能换了启用的那条
  const refetchRef = useRef(query.refetch);
  refetchRef.current = query.refetch;
  useEffect(() => {
    if (!open) return;
    const handleFocus = () => {
      if (!busyRef.current) void refetchRef.current();
    };
    window.addEventListener("focus", handleFocus);
    return () => window.removeEventListener("focus", handleFocus);
  }, [open]);

  const beginWrite = () => {
    if (busyRef.current) return false;
    busyRef.current = true;
    setBusy(true);
    return true;
  };

  const endWrite = () => {
    busyRef.current = false;
    setBusy(false);
    // 全局视图和各应用的列表都在 promptKeys.all 下
    void queryClient.invalidateQueries({ queryKey: promptKeys.all });
  };

  const setFail = (id: string, app: AppId, error?: unknown) =>
    setFails((prev) => {
      const next = { ...prev };
      const key = failKey(id, app);
      if (error === undefined) delete next[key];
      else next[key] = extractErrorMessage(error) || t("common.unknown");
      return next;
    });

  const afterUndo = () =>
    void queryClient.invalidateQueries({ queryKey: promptKeys.all });

  const handleCell = async (entry: GlobalPromptEntry, app: AppId) => {
    const enable = !entry.apps[app];
    if (!beginWrite()) return;
    const appName = APP_DISPLAY_NAME[app];
    const file = promptFileName(app);
    try {
      const before = enable ? await promptsApi.getPrompts(app) : {};
      const previous = Object.entries(before).find(
        ([, prompt]) => prompt.enabled,
      );
      await promptsApi.setGlobalPromptApp(entry.id, app, enable);
      setFail(entry.id, app);
      const effect = toggleEffectText(t, app, enable);
      if (!enable) {
        showPromptToast(t, {
          title: t(
            app === "pi"
              ? "prompts.global.toast.removedPi"
              : "prompts.global.toast.removed",
            { name: entry.name, app: appName, file },
          ),
          description: effect || undefined,
          onUndo: async () => {
            await promptsApi.setGlobalPromptApp(entry.id, app, true);
            afterUndo();
          },
        });
        return;
      }
      // 启用前文件里有库里没有的内容，后端会先存成一条「原始提示词」
      const after = await promptsApi.getPrompts(app).catch(() => before);
      const backup = Object.entries(after).find(
        ([key]) => !(key in before) && key.startsWith("backup-"),
      );
      const description = [
        previous
          ? t("prompts.global.toast.replaced", { name: previous[1].name })
          : "",
        backup ? t("prompts.toast.backup", { name: backup[1].name }) : "",
        effect,
      ]
        .filter(Boolean)
        .join(t("prompts.toast.joiner"));
      showPromptToast(t, {
        title: t(
          app === "pi"
            ? "prompts.global.toast.appliedPi"
            : "prompts.global.toast.applied",
          { name: entry.name, app: appName, file },
        ),
        description: description || undefined,
        // 撤销只给能原样还原的：之前有启用中的就换回去
        onUndo: previous
          ? async () => {
              await promptsApi.enablePrompt(app, previous[0]);
              afterUndo();
            }
          : undefined,
      });
    } catch (error) {
      setFail(entry.id, app, error);
      toast.error(
        t(
          enable
            ? "prompts.global.toast.applyFailed"
            : "prompts.global.toast.removeFailed",
          { app: appName },
        ),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      endWrite();
    }
  };

  const handleReapply = async (entry: GlobalPromptEntry) => {
    if (!beginWrite()) return;
    const done: AppId[] = [];
    try {
      for (const app of entry.modifiedApps) {
        try {
          await promptsApi.setGlobalPromptApp(entry.id, app, true);
          setFail(entry.id, app);
          done.push(app);
        } catch (error) {
          setFail(entry.id, app, error);
          toast.error(
            t("prompts.global.toast.applyFailed", {
              app: APP_DISPLAY_NAME[app],
            }),
            { description: extractErrorMessage(error) || undefined },
          );
        }
      }
      if (done.length) {
        showPromptToast(t, {
          title: t("prompts.global.toast.reapplied", {
            name: entry.name,
            apps: joinAppNames(t, done),
          }),
        });
      }
    } finally {
      endWrite();
    }
  };

  const handleSave = async (_id: string, prompt: Prompt) => {
    if (!beginWrite()) return false;
    try {
      const result = await promptsApi.upsertGlobalPrompt({
        id: editing?.id ?? "",
        name: prompt.name,
        content: prompt.content,
        description: prompt.description,
        createdAt: editing?.createdAt,
      });
      if (editing) {
        reportGlobalSave(t, prompt.name, globalPromptApps(editing), result);
      } else {
        showPromptToast(t, {
          title: t("prompts.toast.added", { name: prompt.name }),
          description: t("prompts.global.toast.addedSub"),
        });
      }
      return true;
    } catch (error) {
      toast.error(t("prompts.saveFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
      return false;
    } finally {
      endWrite();
    }
  };

  // 删除不弹确认框：还在用的不能删；删后 toast 给「撤销」（原 id 写回）
  const handleDelete = async (entry: GlobalPromptEntry) => {
    if (appliedAnywhere(entry).length) return;
    if (!beginWrite()) return;
    try {
      await promptsApi.deleteGlobalPrompt(entry.id);
      showPromptToast(t, {
        title: t("prompts.toast.deleted", { name: entry.name }),
        undoneTitle: t("prompts.toast.restored", { name: entry.name }),
        onUndo: async () => {
          await promptsApi.upsertGlobalPrompt(asPrompt(entry));
          afterUndo();
        },
      });
    } catch (error) {
      toast.error(t("prompts.deleteFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    } finally {
      endWrite();
    }
  };

  const openForm = (id: string | null) => {
    if (busyRef.current || formOpen) return;
    setEditing(entries.find((entry) => entry.id === id) ?? null);
    setFormOpen(true);
  };

  const closeForm = () => {
    if (busyRef.current) return;
    setFormOpen(false);
    setEditing(null);
  };

  const listEmpty = entries.length === 0;

  return (
    <PromptPageFrame
      app="global"
      apps={apps}
      onAppChange={onAppChange}
      appSwitchDisabled={busy}
      count={entries.length}
      help={{
        title: t("prompts.global.helpTitle"),
        text: t("prompts.global.helpText"),
      }}
      primary={{
        label: t("prompts.global.add"),
        onClick: () => openForm(null),
        disabled: controlsDisabled,
      }}
      moreItems={[]}
      segments={
        <span className="min-w-0 shrink truncate text-caption text-fg-2">
          {t("prompts.global.lead")}
        </span>
      }
      search={
        listEmpty
          ? null
          : {
              value: searchQuery,
              onChange: setSearchQuery,
              placeholder: t("prompts.searchPlaceholder"),
              ariaLabel: t("prompts.searchAriaLabel"),
              resultCount: filtered.length,
            }
      }
    >
      {listEmpty ? (
        query.isPending ? (
          <div className="py-12 text-center text-body text-fg-2">
            {t("prompts.loading")}
          </div>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 px-10 pb-10 text-center">
            <h2 className="m-0 text-section text-fg-1">
              {t("prompts.global.emptyTitle")}
            </h2>
            <p className="m-0 max-w-[460px] text-body text-fg-2">
              {t("prompts.global.emptyText")}
            </p>
            <div className="mt-2 flex gap-2">
              <Button
                variant="neutral"
                size="regular"
                disabled={controlsDisabled}
                onClick={() => openForm(null)}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("prompts.global.add")}
              </Button>
            </div>
          </div>
        )
      ) : (
        <div className="flex min-h-0 shrink flex-col rounded-panel border border-border bg-surface">
          <div className="flex h-11 shrink-0 items-center border-b border-border pe-2 ps-4">
            <span className="min-w-0 flex-1 text-caption text-fg-2">
              {t("prompts.global.columnName")}
            </span>
            <div className="flex shrink-0">
              {apps.map((app) => (
                <span key={app} className="flex w-9 shrink-0 justify-center">
                  <HoverTip content={APP_DISPLAY_NAME[app]} side="top">
                    <span
                      role="img"
                      aria-label={APP_DISPLAY_NAME[app]}
                      className="flex h-7 w-7 items-center justify-center"
                    >
                      <AppGlyph
                        app={app}
                        size={16}
                        badgeClassName="bg-surface"
                      />
                    </span>
                  </HoverTip>
                </span>
              ))}
            </div>
            <span className="w-11 shrink-0" />
          </div>
          {filtered.length === 0 ? (
            <div className="flex flex-col items-center gap-3 px-6 py-10 text-center">
              <span className="text-body text-fg-2">
                {t("prompts.noSearchResults", { query: searchQuery.trim() })}
              </span>
              <Button
                variant="neutral"
                size="compact"
                onClick={() => setSearchQuery("")}
              >
                {t("prompts.clearSearch")}
              </Button>
            </div>
          ) : (
            <ul
              aria-label={t("prompts.global.listLabel")}
              className="m-0 min-h-0 list-none overflow-y-auto scroll-stable p-0"
            >
              {filtered.map((entry, index) => (
                <GlobalPromptRow
                  key={entry.id}
                  entry={entry}
                  first={index === 0}
                  apps={apps}
                  fails={fails}
                  disabled={controlsDisabled}
                  detail={[
                    entry.description?.trim() ||
                      firstLine(entry.content) ||
                      t("prompts.emptyContent"),
                    formatSize(utf8Bytes(entry.content)),
                    formatUpdated(t, i18n.language, entry.updatedAt),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                  onEdit={() => openForm(entry.id)}
                  onCell={(app) => void handleCell(entry, app)}
                  onReapply={() => void handleReapply(entry)}
                  onDelete={() => void handleDelete(entry)}
                  onCopyContent={() => {
                    void copyText(entry.content).then((ok) => {
                      if (ok) {
                        showPromptToast(t, {
                          title: t("prompts.copiedContent", {
                            name: entry.name,
                          }),
                        });
                      } else {
                        toast.error(t("prompts.copyFailed"));
                      }
                    });
                  }}
                />
              ))}
            </ul>
          )}
        </div>
      )}

      {formOpen && (
        <PromptFormPanel
          globalApps={editing ? globalPromptApps(editing) : []}
          editingId={editing?.id}
          initialData={initialData}
          onSave={handleSave}
          onClose={closeForm}
        />
      )}
    </PromptPageFrame>
  );
};

function GlobalPromptRow({
  entry,
  first,
  apps,
  fails,
  disabled,
  detail,
  onEdit,
  onCell,
  onReapply,
  onDelete,
  onCopyContent,
}: {
  entry: GlobalPromptEntry;
  first: boolean;
  apps: AppId[];
  fails: Record<string, string>;
  disabled: boolean;
  detail: string;
  onEdit: () => void;
  onCell: (app: AppId) => void;
  onReapply: () => void;
  onDelete: () => void;
  onCopyContent: () => void;
}) {
  const { t } = useTranslation();
  const applied = appliedAnywhere(entry);
  const failedApp = apps.find((app) => fails[failKey(entry.id, app)]);
  const modifiedId = `gp-modified-${entry.id}`;
  const deleteBlocked = applied.length
    ? t("prompts.global.deleteBlocked", { apps: joinAppNames(t, applied) })
    : undefined;

  return (
    <li
      data-testid={`global-prompt-row-${entry.id}`}
      onClick={(event) => {
        if (disabled) return;
        const target = event.target as HTMLElement;
        if (target.closest("button, a, input, label, [role='menu']")) return;
        onEdit();
      }}
      className={cn(
        "flex h-[60px] cursor-pointer items-center pe-2 ps-4 transition-colors duration-150 hover:bg-subtle",
        !first && "border-t border-border",
      )}
    >
      <div className="flex min-w-0 flex-1 flex-col pe-3">
        <div className="flex min-w-0 items-center gap-1.5">
          <span
            title={entry.name}
            className="min-w-0 truncate text-body font-medium text-fg-1"
          >
            {entry.name}
          </span>
          {applied.length === 0 ? (
            <NeutralBadge>{t("mcpPage.notEnabled")}</NeutralBadge>
          ) : null}
        </div>
        <div className="flex min-w-0 whitespace-nowrap text-caption text-fg-2">
          {failedApp ? (
            <span className="min-w-0 truncate text-warning-text">
              {t("prompts.global.rowFail", {
                app: APP_DISPLAY_NAME[failedApp],
                error: fails[failKey(entry.id, failedApp)],
              })}
            </span>
          ) : entry.modifiedApps.length ? (
            <>
              <span
                id={modifiedId}
                className="min-w-0 truncate text-warning-text"
              >
                {t("prompts.global.modified", {
                  apps: joinAppNames(t, entry.modifiedApps),
                })}
                &nbsp;·&nbsp;
              </span>
              <button
                type="button"
                disabled={disabled}
                aria-describedby={modifiedId}
                onClick={onReapply}
                className="shrink-0 text-warning-text underline underline-offset-[3px] disabled:cursor-not-allowed disabled:opacity-60"
              >
                {t("prompts.global.reapply")}
              </button>
            </>
          ) : (
            <span title={detail} className="min-w-0 truncate">
              {detail}
            </span>
          )}
        </div>
      </div>
      <div className="flex shrink-0">
        {apps.map((app) => {
          const state = fails[failKey(entry.id, app)]
            ? "fail"
            : entry.apps[app]
              ? "on"
              : "off";
          return (
            <MatrixCell
              key={app}
              app={app}
              state={state}
              disabled={disabled}
              label={t(`appMatrix.cell.${state}`, {
                name: entry.name,
                app: APP_DISPLAY_NAME[app],
              })}
              onClick={() => onCell(app)}
            />
          );
        })}
      </div>
      <div className="flex w-11 shrink-0 justify-end">
        <DropdownMenu modal={false}>
          <HoverTip content={t("common.more")}>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="quiet"
                size="icon-compact"
                disabled={disabled}
                aria-label={t("prompts.rowMoreActions", { name: entry.name })}
              >
                <MoreHorizontal
                  className="h-[15px] w-[15px]"
                  strokeWidth={1.5}
                />
              </Button>
            </DropdownMenuTrigger>
          </HoverTip>
          <DropdownMenuContent
            align="end"
            className={cn(promptMenuContentClass, "w-[240px]")}
          >
            <DropdownMenuItem className={promptMenuItemClass} onSelect={onEdit}>
              {t("common.edit")}
            </DropdownMenuItem>
            <DropdownMenuItem
              className={promptMenuItemClass}
              onSelect={onCopyContent}
            >
              {t("prompts.copyContent")}
            </DropdownMenuItem>
            <DropdownMenuSeparator className="mx-1.5 my-1 bg-border" />
            <DropdownMenuItem
              disabled={Boolean(deleteBlocked)}
              onSelect={onDelete}
              className={cn(
                promptMenuItemClass,
                "flex-col items-start gap-0",
                deleteBlocked
                  ? "text-fg-3"
                  : "text-danger-text focus:text-danger-text",
              )}
            >
              <span>{t("common.delete")}</span>
              {deleteBlocked ? (
                <span className="text-caption text-fg-2">{deleteBlocked}</span>
              ) : null}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </li>
  );
}

export default GlobalPromptPanel;

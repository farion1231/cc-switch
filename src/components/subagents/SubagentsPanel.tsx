import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Bot, ChevronDown, Copy, Download, Plus } from "lucide-react";
import { toast } from "@/lib/toast";
import { copyText } from "@/lib/clipboard";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { PageTabs } from "@/components/ui/page-tabs";
import { SearchField } from "@/components/ui/search-field";
import { renderSegs } from "@/components/prompts/promptUtils";
import {
  useDeleteSubagent,
  useImportSubagents,
  useInvalidateSubagents,
  usePluginSubagentsQuery,
  useResolveSubagentDrift,
  useSubagentsDirQuery,
  useSubagentsQuery,
  useToggleSubagent,
  useUnmanagedSubagentsQuery,
  useUpsertSubagent,
} from "@/hooks/useSubagents";
import type {
  Subagent,
  SubagentDriftStrategy,
  SubagentErrorCode,
  SubagentInput,
} from "@/lib/api/subagents";
import { extractErrorMessage } from "@/utils/errorUtils";
import { updateSubagentFrontmatter } from "./frontmatter";
import SubagentFormPanel, {
  type SubagentSaveResult,
} from "./SubagentFormPanel";
import { SubagentDriftDialog } from "./SubagentDriftDialog";
import { SubagentListItem } from "./SubagentListItem";
import {
  NoSearchResults,
  SubagentPluginList,
  SubagentUnmanagedList,
} from "./SubagentUnmanagedList";
import {
  BLANK_SUBAGENT_CONTENT,
  SUBAGENT_TEMPLATES,
  templateContent,
  type SubagentTemplate,
} from "./templates";
import {
  describeSubagentError,
  fieldsOf,
  isForceableConflict,
  matchesQuery,
  parseSubagentError,
  uniqueCopyName,
} from "./subagentUtils";

type SubagentsTab = "managed" | "unmanaged" | "plugins";

interface EditorState {
  subagent?: Subagent;
  content: string;
  enabled: boolean;
}

interface ConflictState {
  code: SubagentErrorCode;
  detail: string;
  failTitle: string;
  run: () => Promise<void>;
}

interface SubagentsPanelProps {
  /** 写入进行中、确认框打开时锁住侧栏导航（卸载面板会丢掉进行中的状态） */
  onNavigationBlockedChange?: (blocked: boolean) => void;
}

const DEFAULT_DIR = "~/.claude/agents";

/** 全局页「Subagents」：管理 ~/.claude/agents 下的 Claude Code 用户级 subagent。 */
export default function SubagentsPanel({
  onNavigationBlockedChange,
}: SubagentsPanelProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<SubagentsTab>("managed");
  const [searchQuery, setSearchQuery] = useState("");
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [driftTarget, setDriftTarget] = useState<Subagent | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Subagent | null>(null);
  const [conflict, setConflict] = useState<ConflictState | null>(null);
  const [writePending, setWritePending] = useState(false);
  const writeLockRef = useRef(false);

  const listQuery = useSubagentsQuery();
  const dirQuery = useSubagentsDirQuery();
  const unmanagedQuery = useUnmanagedSubagentsQuery();
  const pluginsQuery = usePluginSubagentsQuery();
  const invalidate = useInvalidateSubagents();
  const upsertMutation = useUpsertSubagent();
  const deleteMutation = useDeleteSubagent();
  const toggleMutation = useToggleSubagent();
  const importMutation = useImportSubagents();
  const driftMutation = useResolveSubagentDrift();

  const subagents = useMemo(() => listQuery.data ?? [], [listQuery.data]);
  const unmanaged = unmanagedQuery.data ?? [];
  const plugins = pluginsQuery.data ?? [];
  const dirDisplayPath = dirQuery.data?.displayPath ?? DEFAULT_DIR;

  // 每行的 frontmatter 只解析一次（模型徽标、工具摘要、颜色都从这里取）
  const rows = useMemo(
    () =>
      subagents.map((subagent) => ({ subagent, fields: fieldsOf(subagent) })),
    [subagents],
  );
  const allNames = useMemo(
    () => new Set(rows.map((row) => row.fields.name || row.subagent.name)),
    [rows],
  );
  const filteredRows = rows.filter((row) =>
    matchesQuery(searchQuery, [
      row.fields.name,
      row.subagent.name,
      row.fields.description ?? row.subagent.description,
      row.subagent.fileName,
    ]),
  );

  const navigationBlocked =
    writePending ||
    conflict !== null ||
    driftTarget !== null ||
    deleteTarget !== null;
  useEffect(() => {
    onNavigationBlockedChange?.(navigationBlocked);
  }, [navigationBlocked, onNavigationBlockedChange]);
  useEffect(
    () => () => onNavigationBlockedChange?.(false),
    [onNavigationBlockedChange],
  );

  const beginWrite = () => {
    if (writeLockRef.current) return false;
    writeLockRef.current = true;
    setWritePending(true);
    return true;
  };
  const endWrite = () => {
    writeLockRef.current = false;
    setWritePending(false);
  };

  /**
   * 跑一次写操作。磁盘文件被改过 / 目标文件已存在时先确认，确认后带 force 重试；
   * 其它错误弹 toast（编辑器里的保存传 quiet，错误就地显示在编辑器里）。
   */
  const runWrite = async <T,>(
    op: (force: boolean) => Promise<T>,
    options: {
      failTitle: string;
      onSuccess: (result: T) => void;
      quiet?: boolean;
    },
  ): Promise<SubagentSaveResult> => {
    if (!beginWrite()) return { status: "failed", message: "" };
    try {
      const result = await op(false);
      options.onSuccess(result);
      return { status: "saved" };
    } catch (error) {
      const parsed = parseSubagentError(error);
      if (isForceableConflict(parsed.code)) {
        setConflict({
          code: parsed.code as SubagentErrorCode,
          detail: parsed.detail,
          failTitle: options.failTitle,
          run: async () => options.onSuccess(await op(true)),
        });
        return { status: "pending" };
      }
      const message = describeSubagentError(t, parsed);
      if (!options.quiet) {
        toast.error(options.failTitle, {
          description: message,
          closeButton: true,
        });
      }
      if (parsed.code === "SUBAGENT_NOT_FOUND") void invalidate();
      return { status: "failed", code: parsed.code, message };
    } finally {
      endWrite();
    }
  };

  const confirmConflict = async () => {
    const current = conflict;
    if (!current || !beginWrite()) return;
    try {
      await current.run();
    } catch (error) {
      toast.error(current.failTitle, {
        description: describeSubagentError(t, parseSubagentError(error)),
        closeButton: true,
      });
    } finally {
      setConflict(null);
      endWrite();
    }
  };

  // ─── 操作 ──────────────────────────────────────────────────────────────
  const busy = () => writeLockRef.current || editor !== null;

  const openNew = (content: string) => {
    if (busy()) return;
    setEditor({ content, enabled: true });
  };
  const openTemplate = (template: SubagentTemplate) => {
    const name = template.fields.name ?? template.id;
    // 模板名已被占用时自动加后缀，免得一保存就撞名
    const content = allNames.has(name)
      ? updateSubagentFrontmatter(templateContent(template), {
          name: uniqueCopyName(name, allNames),
        })
      : templateContent(template);
    openNew(content);
  };
  const openEdit = (subagent: Subagent) => {
    if (busy()) return;
    setEditor({
      subagent,
      content: subagent.content,
      enabled: subagent.enabled,
    });
  };
  const openDuplicate = (subagent: Subagent, name: string) => {
    if (busy()) return;
    let content = subagent.content;
    try {
      content = updateSubagentFrontmatter(content, {
        name: uniqueCopyName(name, allNames),
      });
    } catch {
      // frontmatter 坏了就原样带过去，在编辑器的源码模式里修
    }
    setEditor({ content, enabled: true });
  };

  const handleSave = (input: SubagentInput) => {
    const isNew = !input.id;
    return runWrite((force) => upsertMutation.mutateAsync({ input, force }), {
      failTitle: t("subagents.toast.saveFailed"),
      quiet: true,
      onSuccess: (saved) => {
        setEditor(null);
        toast.success(
          isNew
            ? t("subagents.toast.created", { name: saved.name })
            : t("subagents.toast.saved", { name: saved.name }),
          {
            description: saved.enabled
              ? t("subagents.toast.writtenTo", { path: saved.filePath })
              : undefined,
            closeButton: true,
          },
        );
      },
    });
  };

  const handleToggle = (subagent: Subagent, enabled: boolean) => {
    if (busy()) return;
    void runWrite(
      (force) =>
        toggleMutation.mutateAsync({ id: subagent.id, enabled, force }),
      {
        failTitle: enabled
          ? t("subagents.toast.enableFailed", { name: subagent.name })
          : t("subagents.toast.disableFailed", { name: subagent.name }),
        onSuccess: (saved) =>
          toast.success(
            enabled
              ? t("subagents.toast.enabled", { name: saved.name })
              : t("subagents.toast.disabled", { name: saved.name }),
            {
              description: enabled
                ? t("subagents.toast.writtenTo", { path: saved.filePath })
                : t("subagents.toast.removedFrom", { path: subagent.filePath }),
              closeButton: true,
            },
          ),
      },
    );
  };

  // 文件被删了：用库里的版本原样写回（一次 upsert，比先停用再启用少写一次）
  const handleRewrite = (subagent: Subagent) => {
    if (busy()) return;
    void runWrite(
      (force) =>
        upsertMutation.mutateAsync({
          input: {
            id: subagent.id,
            fileName: subagent.fileName,
            content: subagent.content,
            enabled: true,
          },
          force,
        }),
      {
        failTitle: t("subagents.toast.rewriteFailed", { name: subagent.name }),
        onSuccess: (saved) =>
          toast.success(t("subagents.toast.rewritten", { name: saved.name }), {
            description: t("subagents.toast.writtenTo", {
              path: saved.filePath,
            }),
            closeButton: true,
          }),
      },
    );
  };

  const handleDelete = async () => {
    const target = deleteTarget;
    if (!target) return;
    setDeleteTarget(null);
    await runWrite(
      (force) => deleteMutation.mutateAsync({ id: target.id, force }),
      {
        failTitle: t("subagents.toast.deleteFailed", { name: target.name }),
        onSuccess: () =>
          toast.success(t("subagents.toast.deleted", { name: target.name }), {
            description: target.enabled
              ? t("subagents.toast.removedFrom", { path: target.filePath })
              : undefined,
            closeButton: true,
          }),
      },
    );
  };

  const handleResolveDrift = async (strategy: SubagentDriftStrategy) => {
    const target = driftTarget;
    if (!target || !beginWrite()) return;
    try {
      const saved = await driftMutation.mutateAsync({
        id: target.id,
        strategy,
      });
      setDriftTarget(null);
      toast.success(
        strategy === "adopt_disk"
          ? t("subagents.toast.adoptedDisk", { name: saved.name })
          : t("subagents.toast.overwroteDisk", { name: saved.name }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(t("subagents.toast.resolveFailed", { name: target.name }), {
        description: describeSubagentError(t, parseSubagentError(error)),
        closeButton: true,
      });
    } finally {
      endWrite();
    }
  };

  const handleImport = (fileNames: string[]) => {
    if (busy() || fileNames.length === 0) return;
    void runWrite(() => importMutation.mutateAsync(fileNames), {
      failTitle: t("subagents.toast.importFailed"),
      onSuccess: (imported) => {
        toast.success(
          t("subagents.toast.imported", { count: imported.length }),
          { closeButton: true },
        );
        setTab("managed");
      },
    });
  };

  const copyWithToast = (text: string, success: string) => {
    void copyText(text).then(
      () => toast.success(success, { closeButton: true }),
      (error: unknown) =>
        toast.error(t("subagents.toast.copyFailed"), {
          description: extractErrorMessage(error) || undefined,
        }),
    );
  };

  // ─── 渲染 ──────────────────────────────────────────────────────────────
  const controlsDisabled = writePending || editor !== null;

  const newMenu = (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="solid"
          size="regular"
          className="pe-2.5"
          disabled={controlsDisabled}
        >
          <Plus className="h-4 w-4" strokeWidth={2} />
          {t("subagents.new")}
          <ChevronDown className="h-3.5 w-3.5" strokeWidth={2} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[240px]">
        <DropdownMenuItem onSelect={() => openNew(BLANK_SUBAGENT_CONTENT)}>
          {t("subagents.newBlank")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuLabel className="text-caption font-normal text-fg-2">
          {t("subagents.fromTemplate")}
        </DropdownMenuLabel>
        {SUBAGENT_TEMPLATES.map((template) => (
          <DropdownMenuItem
            key={template.id}
            onSelect={() => openTemplate(template)}
          >
            {t(`subagents.templates.${template.id}.title`)}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );

  const header = (
    <AppPageHeader
      icon={<Bot className="h-5 w-5" strokeWidth={1.5} />}
      title="Subagents"
      titleExtra={
        <>
          <HelpTip title={t("subagents.helpTitle")}>
            {t("subagents.helpText")}
          </HelpTip>
          <HoverTip
            content={
              dirQuery.data && !dirQuery.data.exists
                ? t("subagents.dirNotExists")
                : t("subagents.copyDir")
            }
          >
            <button
              type="button"
              onClick={() =>
                copyWithToast(
                  dirQuery.data?.path ?? dirDisplayPath,
                  t("subagents.toast.dirCopied"),
                )
              }
              aria-label={t("subagents.copyDirAria", { path: dirDisplayPath })}
              className="ms-2 inline-flex h-6 max-w-[280px] items-center gap-1.5 rounded-full bg-subtle px-2.5 font-mono text-caption text-fg-2 transition-colors hover:bg-selected hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <span className="truncate">{dirDisplayPath}</span>
              <Copy
                aria-hidden="true"
                className="h-3 w-3 shrink-0"
                strokeWidth={1.5}
              />
            </button>
          </HoverTip>
        </>
      }
      actions={
        <>
          <Button
            type="button"
            variant="neutral"
            size="regular"
            disabled={controlsDisabled}
            onClick={() => setTab("unmanaged")}
          >
            <Download className="h-4 w-4" strokeWidth={2} />
            {t("subagents.import")}
            {unmanaged.length > 0 ? (
              <span className="text-caption font-normal tabular-nums text-fg-2">
                {unmanaged.length}
              </span>
            ) : null}
          </Button>
          {newMenu}
        </>
      }
    />
  );

  const tabs = (
    <PageTabs<SubagentsTab>
      aria-label={t("subagents.tabs.aria")}
      idPrefix="subagents-tab"
      controls="subagents-tabpanel"
      className="h-11"
      value={tab}
      onValueChange={setTab}
      items={[
        {
          value: "managed",
          label: t("subagents.tabs.managedCount", { count: subagents.length }),
        },
        {
          value: "unmanaged",
          label: t("subagents.tabs.unmanagedCount", {
            count: unmanaged.length,
          }),
        },
        {
          value: "plugins",
          label: t("subagents.tabs.pluginsCount", { count: plugins.length }),
        },
      ]}
      trailing={
        <SearchField
          containerClassName="w-[240px]"
          value={searchQuery}
          onValueChange={setSearchQuery}
          clearLabel={t("subagents.clearSearch")}
          aria-label={t("subagents.searchAria")}
          placeholder={t("subagents.searchPlaceholder")}
          autoComplete="off"
        />
      }
    />
  );

  const renderEmpty = () => (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-10 pb-10 text-center">
      <h2 className="m-0 text-section text-fg-1">
        {t("subagents.empty.title")}
      </h2>
      <p className="m-0 max-w-[520px] text-body text-fg-2">
        {renderSegs(t("subagents.empty.body", { path: dirDisplayPath }))}
      </p>
      <div className="mt-2 flex gap-2">
        {unmanaged.length > 0 ? (
          <Button
            variant="neutral"
            size="regular"
            disabled={controlsDisabled}
            onClick={() => setTab("unmanaged")}
          >
            <Download className="h-3.5 w-3.5" />
            {t("subagents.empty.importCount", { count: unmanaged.length })}
          </Button>
        ) : null}
        <Button
          variant="neutral"
          size="regular"
          disabled={controlsDisabled}
          onClick={() => openNew(BLANK_SUBAGENT_CONTENT)}
        >
          <Plus className="h-3.5 w-3.5" />
          {t("subagents.newBlank")}
        </Button>
      </div>
      <p className="m-0 mt-5 text-caption text-fg-2">
        {t("subagents.fromTemplate")}
      </p>
      <div className="grid w-full max-w-[640px] grid-cols-1 gap-2 sm:grid-cols-2">
        {SUBAGENT_TEMPLATES.map((template) => (
          <button
            key={template.id}
            type="button"
            disabled={controlsDisabled}
            onClick={() => openTemplate(template)}
            className="flex flex-col items-start gap-0.5 rounded-panel border border-border bg-surface px-3.5 py-3 text-left transition-colors hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
          >
            <span className="text-body font-medium text-fg-1">
              {t(`subagents.templates.${template.id}.title`)}
            </span>
            <span className="text-caption text-fg-2">
              {t(`subagents.templates.${template.id}.summary`)}
            </span>
          </button>
        ))}
      </div>
    </div>
  );

  const renderManaged = () => {
    if (listQuery.isLoading) {
      return (
        <div className="py-12 text-center text-body text-fg-2">
          {t("common.loading")}
        </div>
      );
    }
    if (listQuery.isError) {
      return (
        <div className="flex flex-1 flex-col items-center justify-center gap-2.5 px-10 pb-12 text-center">
          <h2 className="m-0 text-section">{t("subagents.loadFailed")}</h2>
          <code className="max-w-[520px] rounded-[8px] bg-subtle px-3 py-2 text-left font-mono text-caption text-fg-2 [overflow-wrap:anywhere]">
            {extractErrorMessage(listQuery.error) || String(listQuery.error)}
          </code>
          <Button
            type="button"
            variant="neutral"
            size="regular"
            className="mt-1"
            onClick={() => void listQuery.refetch()}
          >
            {t("common.retry")}
          </Button>
        </div>
      );
    }
    if (rows.length === 0) return renderEmpty();
    if (filteredRows.length === 0) {
      return (
        <NoSearchResults
          query={searchQuery}
          onClear={() => setSearchQuery("")}
        />
      );
    }
    return (
      <ul
        aria-label={t("subagents.tabs.managed")}
        className="m-0 min-h-0 shrink list-none overflow-y-auto scroll-stable rounded-panel border border-border bg-surface p-0"
      >
        {filteredRows.map(({ subagent, fields }, index) => {
          const name = fields.name || subagent.name;
          return (
            <SubagentListItem
              key={subagent.id}
              subagent={subagent}
              fields={fields}
              first={index === 0}
              disabled={controlsDisabled}
              onEdit={() => openEdit(subagent)}
              onToggle={(enabled) => handleToggle(subagent, enabled)}
              onResolveDrift={() => {
                if (!busy()) setDriftTarget(subagent);
              }}
              onRewrite={() => handleRewrite(subagent)}
              onDuplicate={() => openDuplicate(subagent, name)}
              onExport={() =>
                copyWithToast(
                  subagent.content,
                  t("subagents.toast.exported", { name }),
                )
              }
              onCopyPath={() =>
                copyWithToast(
                  subagent.filePath,
                  t("subagents.toast.pathCopied"),
                )
              }
              onDelete={() => {
                if (!busy()) setDeleteTarget(subagent);
              }}
            />
          );
        })}
      </ul>
    );
  };

  const conflictText = (() => {
    if (!conflict) return { title: "", message: "", confirm: "" };
    if (conflict.code === "SUBAGENT_DRIFTED") {
      return {
        title: t("subagents.conflict.driftedTitle"),
        message: t("subagents.conflict.driftedMessage", {
          detail: conflict.detail,
        }),
        confirm: t("subagents.conflict.driftedConfirm"),
      };
    }
    return {
      title: t("subagents.conflict.fileExistsTitle"),
      message: t("subagents.conflict.fileExistsMessage", {
        detail: conflict.detail,
      }),
      confirm: t("subagents.conflict.fileExistsConfirm"),
    };
  })();

  return (
    <>
      {header}
      <div id="main-content" className="flex min-h-0 flex-1 flex-col">
        <div className="shrink-0 px-6 pt-1">{tabs}</div>
        <div
          id="subagents-tabpanel"
          role="tabpanel"
          aria-labelledby={`subagents-tab-${tab}`}
          className="flex min-h-0 flex-1 flex-col px-6 pb-5 pt-3"
        >
          {tab === "managed" ? (
            renderManaged()
          ) : tab === "unmanaged" ? (
            <SubagentUnmanagedList
              items={unmanaged}
              loading={unmanagedQuery.isLoading}
              searchQuery={searchQuery}
              dirDisplayPath={dirDisplayPath}
              pending={writePending}
              onClearSearch={() => setSearchQuery("")}
              onImport={handleImport}
            />
          ) : (
            <SubagentPluginList
              items={plugins}
              loading={pluginsQuery.isLoading}
              searchQuery={searchQuery}
              onClearSearch={() => setSearchQuery("")}
            />
          )}
        </div>
      </div>

      {editor ? (
        <SubagentFormPanel
          subagent={editor.subagent}
          initialContent={editor.content}
          initialEnabled={editor.enabled}
          dirDisplayPath={dirDisplayPath}
          takenNames={
            new Set(
              rows
                .filter((row) => row.subagent.id !== editor.subagent?.id)
                .map((row) => row.fields.name || row.subagent.name),
            )
          }
          onSave={handleSave}
          onClose={() => {
            if (!writeLockRef.current) setEditor(null);
          }}
        />
      ) : null}

      {driftTarget ? (
        <SubagentDriftDialog
          subagent={driftTarget}
          pending={writePending}
          onResolve={(strategy) => void handleResolveDrift(strategy)}
          onClose={() => setDriftTarget(null)}
        />
      ) : null}

      <ConfirmDialog
        isOpen={deleteTarget !== null}
        zIndex="top"
        title={t("subagents.confirmDelete.title", {
          name: deleteTarget?.name ?? "",
        })}
        message={
          deleteTarget?.enabled
            ? t("subagents.confirmDelete.messageEnabled", {
                path: deleteTarget.filePath,
              })
            : t("subagents.confirmDelete.message")
        }
        confirmText={t("common.delete")}
        onConfirm={() => void handleDelete()}
        onCancel={() => setDeleteTarget(null)}
      />

      <ConfirmDialog
        isOpen={conflict !== null}
        zIndex="top"
        title={conflictText.title}
        message={conflictText.message}
        confirmText={conflictText.confirm}
        pending={writePending}
        onConfirm={() => void confirmConflict()}
        onCancel={() => setConflict(null)}
      />
    </>
  );
}

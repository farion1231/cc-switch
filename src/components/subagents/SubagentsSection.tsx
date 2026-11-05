import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  Copy,
  Download,
  Plus,
} from "lucide-react";
import { toast } from "@/lib/toast";
import { copyText } from "@/lib/clipboard";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
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
  parseSubagentError,
  uniqueCopyName,
} from "./subagentUtils";

/** 区块里的子视图：已管理列表，或导入未管理的文件 */
type SectionView = "managed" | "unmanaged";

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

interface SubagentsSectionProps {
  /**
   * 正在编辑的供应商表单里「Subagent」一档的值（CLAUDE_CODE_SUBAGENT_MODEL）。
   * 这家供应商生效时它优先于各子代理文件里的 model，所以在列表上方提示。
   */
  subagentModelOverride?: string;
}

const DEFAULT_DIR = "~/.claude/agents";

/**
 * Claude 供应商编辑页里的「子代理」区块：管理 ~/.claude/agents 下的用户级 subagent。
 *
 * 子代理文件是全局的，所有 Claude 供应商共用；这里的增删改即时写盘，不跟着供应商表单的
 * 保存 / 取消走。区块渲染在供应商的 <form> 里：仓库的 Button 没有默认 type，这里每个按钮
 * 都必须写 type="button"，否则点一下就提交了供应商表单；也不放会响应回车的输入框
 * （编辑器、确认框都在 portal 里，不在 form 的 DOM 子树）。
 */
export default function SubagentsSection({
  subagentModelOverride,
}: SubagentsSectionProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [view, setView] = useState<SectionView>("managed");
  const [pluginsExpanded, setPluginsExpanded] = useState(false);
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
  // 折叠时也要看得出有没有要处理的：标题行上显示数量
  const attentionCount = rows.filter(
    (row) =>
      row.subagent.status === "drifted" || row.subagent.status === "missing",
  ).length;

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
        setView("managed");
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
          variant="neutral"
          size="compact"
          className="pe-2"
          disabled={controlsDisabled}
        >
          <Plus className="h-3.5 w-3.5" strokeWidth={2} />
          {t("subagents.new")}
          <ChevronDown className="h-3 w-3" strokeWidth={2} />
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

  const dirChip = (
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
        className="inline-flex h-6 max-w-[240px] items-center gap-1.5 rounded-full bg-subtle px-2.5 font-mono text-caption text-fg-2 transition-colors hover:bg-selected hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span className="truncate">{dirDisplayPath}</span>
        <Copy
          aria-hidden="true"
          className="h-3 w-3 shrink-0"
          strokeWidth={1.5}
        />
      </button>
    </HoverTip>
  );

  const toolbar = (
    <div className="flex flex-wrap items-center gap-2">
      <p className="m-0 min-w-0 flex-1 text-caption text-fg-2">
        {t("subagents.section.sharedHint")}
      </p>
      {dirChip}
      <Button
        type="button"
        variant="neutral"
        size="compact"
        disabled={controlsDisabled}
        onClick={() => setView("unmanaged")}
      >
        <Download className="h-3.5 w-3.5" strokeWidth={2} />
        {t("subagents.import")}
        {unmanaged.length > 0 ? (
          <span className="text-caption font-normal tabular-nums text-fg-2">
            {unmanaged.length}
          </span>
        ) : null}
      </Button>
      {newMenu}
    </div>
  );

  const overrideModel = subagentModelOverride?.trim();
  const overrideNotice = overrideModel ? (
    <p className="m-0 text-xs leading-relaxed text-warning-text">
      {t("subagents.section.overrideWarning", { model: overrideModel })}
    </p>
  ) : null;

  const renderEmpty = () => (
    <div className="flex flex-col items-center gap-2.5 rounded-panel border border-dashed border-border px-6 py-5 text-center">
      <p className="m-0 max-w-[520px] text-body text-fg-2">
        {renderSegs(t("subagents.empty.body", { path: dirDisplayPath }))}
      </p>
      <div className="flex flex-wrap justify-center gap-2">
        {unmanaged.length > 0 ? (
          <Button
            type="button"
            variant="neutral"
            size="compact"
            disabled={controlsDisabled}
            onClick={() => setView("unmanaged")}
          >
            <Download className="h-3.5 w-3.5" />
            {t("subagents.empty.importCount", { count: unmanaged.length })}
          </Button>
        ) : null}
        {SUBAGENT_TEMPLATES.map((template) => (
          <HoverTip
            key={template.id}
            content={t(`subagents.templates.${template.id}.summary`)}
          >
            <Button
              type="button"
              variant="neutral"
              size="compact"
              disabled={controlsDisabled}
              onClick={() => openTemplate(template)}
            >
              <Plus className="h-3.5 w-3.5" />
              {t(`subagents.templates.${template.id}.title`)}
            </Button>
          </HoverTip>
        ))}
      </div>
    </div>
  );

  const renderManaged = () => {
    if (listQuery.isLoading) {
      return (
        <div className="py-6 text-center text-body text-fg-2">
          {t("common.loading")}
        </div>
      );
    }
    if (listQuery.isError) {
      return (
        <div className="flex flex-col items-start gap-2">
          <p className="m-0 text-body text-danger-text">
            {t("subagents.loadFailed")}
          </p>
          <code className="max-w-full rounded-[8px] bg-subtle px-3 py-2 font-mono text-caption text-fg-2 [overflow-wrap:anywhere]">
            {extractErrorMessage(listQuery.error) || String(listQuery.error)}
          </code>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => void listQuery.refetch()}
          >
            {t("common.retry")}
          </Button>
        </div>
      );
    }
    if (rows.length === 0) return renderEmpty();
    return (
      <ul
        aria-label={t("subagents.tabs.managed")}
        className="m-0 list-none rounded-panel border border-border bg-surface p-0"
      >
        {rows.map(({ subagent, fields }, index) => {
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

  // 插件自带的子代理只读，默认收成一行，免得把供应商编辑页拉得太长
  const pluginsBlock =
    plugins.length > 0 ? (
      <div className="space-y-2">
        <button
          type="button"
          aria-expanded={pluginsExpanded}
          onClick={() => setPluginsExpanded((open) => !open)}
          className="inline-flex items-center gap-1 rounded text-caption text-fg-2 hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {pluginsExpanded ? (
            <ChevronDown className="h-3.5 w-3.5" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5" />
          )}
          {t("subagents.section.pluginsToggle", { count: plugins.length })}
        </button>
        {pluginsExpanded ? (
          <SubagentPluginList
            items={plugins}
            loading={pluginsQuery.isLoading}
            searchQuery=""
            onClearSearch={() => undefined}
          />
        ) : null}
      </div>
    ) : null;

  const summary =
    rows.length === 0
      ? t("subagents.section.summaryEmpty")
      : t("subagents.section.summary", { count: rows.length });

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
    <Collapsible
      open={expanded}
      onOpenChange={setExpanded}
      // 这里的开关、勾选是即时写盘的子代理操作，不是供应商表单的改动；编辑页的未保存跟踪
      // 按 DOM closest 判断豁免，所以 portal 出去的编辑器另外在 SubagentFormPanel 上标记
      data-unsaved-ignore=""
      className="rounded-lg border border-border p-4"
    >
      <div className="flex items-center gap-0.5">
        <CollapsibleTrigger asChild>
          <Button
            type="button"
            variant={null}
            size="sm"
            className="h-8 min-w-0 justify-start gap-1.5 px-0 text-sm font-medium text-fg-1 hover:opacity-70 active:scale-100"
          >
            {expanded ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
            {t("subagents.section.title")}
            <span className="text-caption font-normal text-fg-2">
              {summary}
            </span>
            {attentionCount > 0 ? (
              <span className="text-caption font-normal text-warning-text">
                {t("subagents.section.needsAttention", {
                  count: attentionCount,
                })}
              </span>
            ) : null}
          </Button>
        </CollapsibleTrigger>
        <HelpTip title={t("subagents.helpTitle")}>
          {t("subagents.helpText")}
        </HelpTip>
      </div>
      <CollapsibleContent className="space-y-3 pt-2">
        {view === "managed" ? (
          <>
            {toolbar}
            {overrideNotice}
            {renderManaged()}
            {pluginsBlock}
          </>
        ) : (
          <>
            <Button
              type="button"
              variant="neutral"
              size="compact"
              disabled={writePending}
              onClick={() => setView("managed")}
            >
              <ArrowLeft className="h-3.5 w-3.5" />
              {t("subagents.section.backToList")}
            </Button>
            <SubagentUnmanagedList
              items={unmanaged}
              loading={unmanagedQuery.isLoading}
              searchQuery=""
              dirDisplayPath={dirDisplayPath}
              pending={writePending}
              onClearSearch={() => undefined}
              onImport={handleImport}
            />
          </>
        )}
      </CollapsibleContent>

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
    </Collapsible>
  );
}

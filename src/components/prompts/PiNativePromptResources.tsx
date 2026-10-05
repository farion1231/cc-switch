import {
  forwardRef,
  useId,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
  RefreshCw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetTitle,
} from "@/components/ui/sheet";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  promptsApi,
  type PiPromptFileKind,
  type PiPromptFileSnapshot,
  type PiPromptTemplate,
} from "@/lib/api/prompts";
import {
  getPiPromptTemplateDescription,
  getPiPromptTemplateSummary,
  setPiPromptTemplateDescription,
  stripPiPromptTemplateDescription,
} from "@/lib/piPromptTemplate";
import { isValidPiPromptTemplateSlug } from "@/lib/piPromptSlug";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import { FieldError, promptFieldClass } from "./PromptFormPanel";
import { promptMenuContentClass, promptMenuItemClass } from "./PromptPageFrame";
import {
  charCount,
  copyText,
  formatSize,
  renderSegs,
  showPromptToast,
  utf8Bytes,
} from "./promptUtils";

type EditablePiPromptFileKind = PiPromptFileKind;

const EDITABLE_FILES: Array<{
  kind: EditablePiPromptFileKind;
  filename: "APPEND_SYSTEM.md" | "SYSTEM.md";
  titleKey: string;
  descriptionKey: string;
  recommended?: boolean;
}> = [
  {
    kind: "system_append",
    filename: "APPEND_SYSTEM.md",
    titleKey: "pi.prompts.systemAppend",
    descriptionKey: "pi.prompts.systemAppendDescription",
    recommended: true,
  },
  {
    kind: "system_override",
    filename: "SYSTEM.md",
    titleKey: "pi.prompts.systemOverride",
    descriptionKey: "pi.prompts.systemOverrideDescription",
  },
];

const promptFileKey = (kind: EditablePiPromptFileKind) =>
  ["pi", "promptFile", kind] as const;

export const promptTemplatesKey = ["pi", "promptTemplates"] as const;

export function usePiPromptTemplatesQuery() {
  return useQuery({
    queryKey: promptTemplatesKey,
    queryFn: () => promptsApi.listPiPromptTemplates(),
  });
}

function showMutationError(error: unknown, fallback: string) {
  toast.error(extractErrorMessage(error) || fallback);
}

/** 抽屉外壳（宽 560）：标题栏 52、正文滚动、底栏 56。 */
function PromptDrawer({
  title,
  description,
  onClose,
  busy,
  footer,
  children,
}: {
  title: string;
  description: string;
  onClose: () => void;
  busy: boolean;
  footer: React.ReactNode;
  children: React.ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
    >
      <SheetContent width={560} closeLabel={t("common.close")}>
        <div className="flex h-[52px] shrink-0 items-center border-b border-border pe-12 ps-6">
          <SheetTitle
            title={title}
            className="min-w-0 truncate text-title text-fg-1"
          >
            {title}
          </SheetTitle>
          <SheetDescription className="sr-only">{description}</SheetDescription>
        </div>
        <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto scroll-stable overscroll-contain px-6 pb-6 pt-5">
          {children}
        </div>
        <div className="flex h-14 shrink-0 items-center gap-2 border-t border-border px-6">
          {footer}
        </div>
      </SheetContent>
    </Sheet>
  );
}

function PiInstructionFileEditor({
  file,
  snapshot,
  onClose,
  onDelete,
}: {
  file: (typeof EDITABLE_FILES)[number];
  snapshot: PiPromptFileSnapshot;
  onClose: () => void;
  onDelete: (snapshot: PiPromptFileSnapshot) => void;
}) {
  const { t } = useTranslation();
  const baseId = useId();
  const queryClient = useQueryClient();
  const [baseSnapshot] = useState(() => snapshot);
  const [draft, setDraft] = useState(baseSnapshot.content);
  const [attempted, setAttempted] = useState(false);
  const [confirmCreate, setConfirmCreate] = useState(false);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const queryKey = promptFileKey(file.kind);

  const save = useMutation({
    mutationFn: () =>
      promptsApi.replacePiPromptFile(file.kind, baseSnapshot.revision, draft),
    onSuccess: (nextSnapshot) => {
      queryClient.setQueryData<PiPromptFileSnapshot>(queryKey, nextSnapshot);
      showPromptToast(t, {
        title: t("pi.prompts.fileSaved", { filename: file.filename }),
        description: t("pi.prompts.reloadNotice"),
      });
      setConfirmCreate(false);
      onClose();
    },
    onError: async (error) => {
      showMutationError(error, t("pi.prompts.saveFailed"));
      await queryClient.invalidateQueries({ queryKey });
    },
  });

  const busy = save.isPending;
  const blank = !draft.trim();
  const showBlankError = attempted && blank;
  const creatingOverride =
    file.kind === "system_override" && !baseSnapshot.exists;

  const requestSave = () => {
    if (blank) {
      setAttempted(true);
      bodyRef.current?.focus();
      return;
    }
    if (baseSnapshot.exists && draft === baseSnapshot.content) {
      onClose();
      return;
    }
    if (creatingOverride) {
      setConfirmCreate(true);
      return;
    }
    save.mutate();
  };

  const errorId = `${baseId}-error`;
  const countId = `${baseId}-count`;

  return (
    <>
      <PromptDrawer
        title={t(
          baseSnapshot.exists ? "pi.prompts.editFile" : "pi.prompts.createFile",
          { filename: file.filename },
        )}
        description={t(file.descriptionKey)}
        onClose={onClose}
        busy={busy}
        footer={
          <>
            {baseSnapshot.exists ? (
              <Button
                type="button"
                variant="quiet"
                size="regular"
                onClick={() => onDelete(baseSnapshot)}
                disabled={busy}
                className="-ms-2.5 text-danger-text"
              >
                {t("pi.prompts.deleteFile")}
              </Button>
            ) : null}
            <div className="flex-1" />
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={onClose}
              disabled={busy}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              variant="solid"
              size="regular"
              onClick={requestSave}
              disabled={busy}
            >
              {save.isPending && (
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
              )}
              {creatingOverride
                ? t("pi.prompts.saveAndConfigureEllipsis")
                : t("common.save")}
            </Button>
          </>
        }
      >
        {file.kind === "system_override" ? (
          <div className="flex shrink-0 items-start gap-2.5 rounded-panel bg-warning-soft px-3.5 py-2.5">
            <AlertTriangle
              aria-hidden="true"
              strokeWidth={1.5}
              className="mt-0.5 h-4 w-4 shrink-0 text-warning-text"
            />
            <span className="min-w-0 text-caption leading-5 text-fg-1">
              {renderSegs(t("pi.prompts.systemOverrideWarning"))}
            </span>
          </div>
        ) : null}
        <div className="flex min-h-[220px] flex-1 flex-col gap-1.5">
          <label
            htmlFor={`${baseId}-body`}
            className="text-body font-medium text-fg-1"
          >
            {t("prompts.content")}
          </label>
          <textarea
            ref={bodyRef}
            id={`${baseId}-body`}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            disabled={busy}
            spellCheck={false}
            placeholder={t("pi.prompts.instructionPlaceholder")}
            aria-invalid={showBlankError}
            aria-describedby={
              showBlankError ? `${errorId} ${countId}` : countId
            }
            className={cn(
              promptFieldClass,
              "min-h-[180px] flex-1 resize-none px-3 py-2.5 font-mono text-caption leading-[18px]",
            )}
          />
          <div className="flex items-start gap-3">
            {showBlankError ? (
              <FieldError id={errorId}>
                {t("pi.prompts.blankInstruction")}
              </FieldError>
            ) : null}
            <span
              id={countId}
              className="ms-auto shrink-0 whitespace-nowrap text-caption tabular-nums text-fg-2"
            >
              {t("prompts.charCount", {
                chars: charCount(draft).toLocaleString(),
                size: formatSize(utf8Bytes(draft)),
              })}
            </span>
          </div>
        </div>
      </PromptDrawer>

      <ConfirmDialog
        isOpen={confirmCreate}
        title={t("pi.prompts.activateOverrideTitle", {
          filename: file.filename,
        })}
        message={t("pi.prompts.activateOverrideMessage", {
          filename: file.filename,
        })}
        confirmText={t("pi.prompts.saveAndConfigure")}
        variant="info"
        zIndex="top"
        pending={busy}
        onConfirm={() => save.mutate()}
        onCancel={() => setConfirmCreate(false)}
      />
    </>
  );
}

/** Pi · 系统提示：两个固定文件，文件存在就生效，不加启用开关（Pi 规范 §5）。 */
export function PiSystemPromptFiles() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState<EditablePiPromptFileKind | null>(null);
  const files = [
    useQuery({
      queryKey: promptFileKey("system_append"),
      queryFn: () => promptsApi.getPiPromptFile("system_append"),
    }),
    useQuery({
      queryKey: promptFileKey("system_override"),
      queryFn: () => promptsApi.getPiPromptFile("system_override"),
    }),
  ];

  // 删除不弹确认框：直接删、关抽屉，toast 给「撤销」（原样写回）；原本是空白的文件不给撤销
  const remove = useMutation({
    mutationFn: async ({
      kind,
      snapshot,
    }: {
      kind: EditablePiPromptFileKind;
      snapshot: PiPromptFileSnapshot;
    }) => promptsApi.deletePiPromptFile(kind, snapshot.revision),
    onSuccess: async (_removed, { kind, snapshot }) => {
      const file = EDITABLE_FILES.find((item) => item.kind === kind)!;
      setEditing(null);
      await queryClient.invalidateQueries({ queryKey: promptFileKey(kind) });
      const blank = !snapshot.content.trim();
      showPromptToast(t, {
        title: t(
          kind === "system_append"
            ? "pi.prompts.appendDeleted"
            : "pi.prompts.overrideDeleted",
        ),
        description: `${t(
          blank ? "pi.prompts.deletedBlank" : "pi.prompts.deletedUndoable",
        )}${t("pi.prompts.reloadAfterDelete")}`,
        undoneTitle: t("pi.prompts.fileRestored", {
          filename: file.filename,
        }),
        onUndo: blank
          ? undefined
          : async () => {
              await promptsApi.replacePiPromptFile(
                kind,
                "missing",
                snapshot.content,
              );
              await queryClient.invalidateQueries({
                queryKey: promptFileKey(kind),
              });
            },
      });
    },
    onError: async (error, { kind }) => {
      showMutationError(error, t("pi.prompts.deleteFailed"));
      await queryClient.invalidateQueries({ queryKey: promptFileKey(kind) });
    },
  });

  return (
    <ul
      aria-label={t("pi.prompts.systemFilesLabel")}
      className="m-0 shrink-0 list-none rounded-panel border border-border bg-surface p-0"
    >
      {EDITABLE_FILES.map((file, index) => {
        const query = files[index];
        const data = query.data;
        const status = query.isLoading
          ? t("common.loading")
          : query.isError
            ? t("pi.prompts.unavailable")
            : !data?.exists
              ? t("pi.prompts.notConfigured")
              : !data.content.trim()
                ? t("pi.prompts.configuredEmpty")
                : t("pi.prompts.configuredSize", {
                    size: formatSize(utf8Bytes(data.content)),
                  });
        const statusId = `pi-sys-status-${file.kind}`;
        return (
          <li
            key={file.kind}
            className={cn(
              "flex min-h-[72px] items-center gap-4 py-3 pe-3 ps-4",
              index > 0 && "border-t border-border",
            )}
          >
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <div className="flex min-w-0 items-center gap-2">
                <span className="whitespace-nowrap text-body font-medium text-fg-1">
                  {t(file.titleKey)}
                </span>
                <code className="whitespace-nowrap font-mono text-caption text-fg-2">
                  {file.filename}
                </code>
                {file.recommended ? (
                  <span className="inline-flex h-[18px] shrink-0 items-center whitespace-nowrap rounded-full border border-border-strong px-[7px] text-badge text-fg-2">
                    {t("pi.prompts.recommended")}
                  </span>
                ) : null}
              </div>
              <span className="text-caption text-fg-2">
                {t(file.descriptionKey)}
              </span>
            </div>
            <span
              id={statusId}
              className={cn(
                "shrink-0 whitespace-nowrap text-caption tabular-nums",
                query.isError
                  ? "text-danger-text"
                  : data?.exists
                    ? "text-fg-1"
                    : "text-fg-2",
              )}
            >
              {status}
            </span>
            {query.isError ? (
              <Button
                type="button"
                variant="neutral"
                size="compact"
                onClick={() => void query.refetch()}
                disabled={query.isFetching}
                className="min-w-16 shrink-0"
              >
                <RefreshCw
                  className={cn(
                    "h-3.5 w-3.5",
                    query.isFetching && "animate-spin",
                  )}
                  aria-hidden="true"
                />
                {t("common.refresh")}
              </Button>
            ) : (
              <Button
                type="button"
                variant="neutral"
                size="compact"
                aria-describedby={statusId}
                aria-label={`${t(data?.exists ? "common.edit" : "pi.prompts.create")} ${file.filename}`}
                disabled={!data || remove.isPending}
                onClick={() => setEditing(file.kind)}
                className="min-w-16 shrink-0"
              >
                {t(data?.exists ? "common.edit" : "pi.prompts.create")}
              </Button>
            )}

            {editing === file.kind && data ? (
              <PiInstructionFileEditor
                file={file}
                snapshot={data}
                onClose={() => setEditing(null)}
                onDelete={(snapshot) =>
                  remove.mutate({ kind: file.kind, snapshot })
                }
              />
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

const SAVE_TO_PREFIX = "~/.pi/agent/prompts/";

interface PiPromptTemplateEditorProps {
  template?: PiPromptTemplate;
  existingSlugs: Set<string>;
  onClose: () => void;
  onChanged: () => Promise<void>;
  onDelete?: (template: PiPromptTemplate) => void;
}

function PiPromptTemplateEditor({
  template,
  existingSlugs,
  onClose,
  onChanged,
  onDelete,
}: PiPromptTemplateEditorProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const initialDescription = getPiPromptTemplateDescription(
    template?.content ?? "",
  );
  const initialContent = stripPiPromptTemplateDescription(
    template?.content ?? "",
  );
  const [slug, setSlug] = useState(template?.slug ?? "");
  const [description, setDescription] = useState(initialDescription ?? "");
  const [content, setContent] = useState(initialContent);
  const [attempted, setAttempted] = useState(false);
  const slugRef = useRef<HTMLInputElement>(null);
  const isCreate = !template;
  const normalizedSlug = slug.trim();
  const slugIsValid = isValidPiPromptTemplateSlug(normalizedSlug);
  const slugChanged = normalizedSlug !== template?.slug;
  const slugAlreadyExists =
    normalizedSlug.length > 0 &&
    slugChanged &&
    existingSlugs.has(normalizedSlug);
  const templateContentChanged =
    description !== (initialDescription ?? "") || content !== initialContent;
  const changed = isCreate || slugChanged || templateContentChanged;
  const serializedContent = templateContentChanged
    ? setPiPromptTemplateDescription(content, description)
    : (template?.content ?? content);

  const slugError = !normalizedSlug
    ? t("pi.prompts.templateSlugRequired")
    : !slugIsValid
      ? t("pi.prompts.templateSlugInvalid")
      : slugAlreadyExists
        ? t("pi.prompts.templateSlugExists", { slug: normalizedSlug })
        : "";
  const showSlugError = attempted && Boolean(slugError);
  // 合法时预告保存位置；编辑时改了名，写明保存会把命令改名
  const slugHint =
    slugError || !normalizedSlug
      ? ""
      : !isCreate && slugChanged
        ? t("pi.prompts.templateRename", {
            from: template.slug,
            to: normalizedSlug,
          })
        : t("pi.prompts.templateSaveTo", {
            path: `${SAVE_TO_PREFIX}${normalizedSlug}.md`,
          });

  const save = useMutation({
    mutationFn: () =>
      promptsApi.upsertPiPromptTemplate(
        normalizedSlug,
        template?.revision ?? "missing",
        serializedContent,
        template?.slug,
      ),
    onSuccess: async (saved) => {
      await onChanged();
      showPromptToast(t, {
        title: isCreate
          ? t("pi.prompts.templateCreated", { slug: saved.slug })
          : t("pi.prompts.templateSaved", { slug: saved.slug }),
        description: t("pi.prompts.reloadNotice"),
      });
      onClose();
    },
    onError: (error) =>
      showMutationError(error, t("pi.prompts.templateSaveFailed")),
  });

  const busy = save.isPending;
  const requestSave = () => {
    if (slugError) {
      setAttempted(true);
      slugRef.current?.focus();
      return;
    }
    if (!changed) {
      onClose();
      return;
    }
    save.mutate();
  };

  const slugHintId = `${baseId}-slug-hint`;
  const helpId = `${baseId}-help`;

  return (
    <PromptDrawer
      title={
        isCreate
          ? t("pi.prompts.newTemplate")
          : t("pi.prompts.editTemplate", { slug: template.slug })
      }
      description={t("pi.prompts.templatesDescription")}
      onClose={onClose}
      busy={busy}
      footer={
        <>
          {!isCreate && onDelete ? (
            <Button
              type="button"
              variant="quiet"
              size="regular"
              disabled={busy}
              onClick={() => onDelete(template)}
              className="-ms-2.5 text-danger-text"
            >
              {t("common.delete")}
            </Button>
          ) : null}
          <div className="flex-1" />
          <Button
            type="button"
            variant="neutral"
            size="regular"
            onClick={onClose}
            disabled={busy}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant="solid"
            size="regular"
            disabled={busy}
            onClick={requestSave}
          >
            {save.isPending && (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            )}
            {isCreate ? t("pi.prompts.createTemplate") : t("common.save")}
          </Button>
        </>
      }
    >
      <div className="flex shrink-0 flex-col gap-1.5">
        <label
          htmlFor={`${baseId}-slug`}
          className="text-body font-medium text-fg-1"
        >
          {t("pi.prompts.templateCommand")}
          <span aria-hidden="true" className="ms-0.5 text-danger-text">
            *
          </span>
          <span className="sr-only">{t("prompts.requiredMark")}</span>
        </label>
        <div className="relative">
          <span
            aria-hidden="true"
            className="pointer-events-none absolute left-[11px] top-1.5 font-mono text-caption leading-5 text-fg-2"
          >
            /
          </span>
          <input
            ref={slugRef}
            id={`${baseId}-slug`}
            type="text"
            value={slug}
            onChange={(event) => setSlug(event.target.value)}
            disabled={busy}
            placeholder={t("pi.prompts.templateSlug")}
            aria-required="true"
            aria-invalid={showSlugError}
            aria-describedby={
              showSlugError || slugHint ? slugHintId : undefined
            }
            autoComplete="off"
            spellCheck={false}
            className={cn(
              promptFieldClass,
              "h-8 ps-[19px] font-mono text-caption",
            )}
          />
        </div>
        {showSlugError ? (
          <FieldError id={slugHintId}>{slugError}</FieldError>
        ) : slugHint ? (
          <span id={slugHintId} className="text-caption text-fg-2">
            {renderSegs(slugHint)}
          </span>
        ) : null}
      </div>

      <div className="flex shrink-0 flex-col gap-1.5">
        <label
          htmlFor={`${baseId}-desc`}
          className="text-body font-medium text-fg-1"
        >
          {t("pi.prompts.templateDescription")}
        </label>
        <input
          id={`${baseId}-desc`}
          type="text"
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          disabled={busy}
          placeholder={t("pi.prompts.templateDescriptionPlaceholder")}
          autoComplete="off"
          className={cn(promptFieldClass, "h-8")}
        />
      </div>

      <div className="flex min-h-[220px] flex-1 flex-col gap-1.5">
        <div className="flex items-center gap-0.5">
          <label
            htmlFor={`${baseId}-body`}
            className="text-body font-medium text-fg-1"
          >
            {t("pi.prompts.templateContent")}
          </label>
          <HelpTip title={t("pi.prompts.templateSyntax")}>
            <span id={helpId}>
              {renderSegs(t("pi.prompts.templateSyntaxDescription"))}
            </span>
          </HelpTip>
        </div>
        <textarea
          id={`${baseId}-body`}
          value={content}
          onChange={(event) => setContent(event.target.value)}
          disabled={busy}
          spellCheck={false}
          placeholder={t("pi.prompts.templateContentPlaceholder")}
          className={cn(
            promptFieldClass,
            "min-h-[180px] flex-1 resize-none px-3 py-2.5 font-mono text-caption leading-[18px]",
          )}
        />
        <span className="ms-auto whitespace-nowrap text-caption tabular-nums text-fg-2">
          {t("prompts.charCount", {
            chars: charCount(content).toLocaleString(),
            size: formatSize(utf8Bytes(content)),
          })}
        </span>
      </div>
    </PromptDrawer>
  );
}

export interface PiPromptTemplatesHandle {
  openCreate: () => void;
}

interface PiPromptTemplatesProps {
  /** 搜索框在页面工具行里，这里只按它过滤 */
  search?: string;
  onClearSearch?: () => void;
}

/** Pi · 模板：~/.pi/agent/prompts/<命令>.md，不用启用，在 Pi 里输入 /命令名 调用。 */
export const PiPromptTemplates = forwardRef<
  PiPromptTemplatesHandle,
  PiPromptTemplatesProps
>(function PiPromptTemplates({ search = "", onClearSearch }, ref) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [editor, setEditor] = useState<
    { mode: "create" } | { mode: "edit"; template: PiPromptTemplate } | null
  >(null);

  const templates = usePiPromptTemplatesQuery();

  useImperativeHandle(ref, () => ({
    openCreate: () => setEditor({ mode: "create" }),
  }));

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: promptTemplatesKey });
  };

  // 删除不弹确认框：直接删，toast 给「撤销」（按 expectedRevision="missing" 原样写回）
  const remove = useMutation({
    mutationFn: (template: PiPromptTemplate) =>
      promptsApi.deletePiPromptTemplate(template.slug, template.revision),
    onSuccess: async (_removed, template) => {
      setEditor(null);
      await refresh();
      showPromptToast(t, {
        title: t("pi.prompts.templateDeletedUndo", { slug: template.slug }),
        description: `${t("pi.prompts.templateDeletedSub", {
          slug: template.slug,
        })}${t("pi.prompts.reloadAfterDelete")}`,
        undoneTitle: t("pi.prompts.templateRestored", { slug: template.slug }),
        onUndo: async () => {
          await promptsApi.upsertPiPromptTemplate(
            template.slug,
            "missing",
            template.content,
          );
          await refresh();
        },
      });
    },
    onError: (error) =>
      showMutationError(error, t("pi.prompts.templateDeleteFailed")),
  });

  const filteredTemplates = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    if (!query) return templates.data ?? [];
    return (templates.data ?? []).filter((template) => {
      const summary = getPiPromptTemplateSummary(template.content);
      return (
        template.slug.toLocaleLowerCase().includes(query) ||
        summary.description?.toLocaleLowerCase().includes(query) ||
        summary.argumentHint?.toLocaleLowerCase().includes(query) ||
        template.content.toLocaleLowerCase().includes(query)
      );
    });
  }, [search, templates.data]);

  const existingSlugs = useMemo(
    () => new Set((templates.data ?? []).map((template) => template.slug)),
    [templates.data],
  );

  const body = (() => {
    if (templates.isLoading) {
      return (
        <div className="flex min-h-48 items-center justify-center gap-2 text-body text-fg-2">
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          {t("common.loading")}
        </div>
      );
    }
    if (templates.isError) {
      return (
        <div className="flex min-h-48 flex-col items-center justify-center gap-3 rounded-panel border border-border bg-surface px-6 text-center">
          <span className="text-body text-fg-2">
            {t("pi.prompts.templateLoadFailed")}
          </span>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => void templates.refetch()}
          >
            <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
            {t("common.refresh")}
          </Button>
        </div>
      );
    }
    if ((templates.data ?? []).length === 0) {
      return (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 px-10 pb-10 text-center">
          <h2 className="m-0 text-section text-fg-1">
            {t("pi.prompts.noTemplates")}
          </h2>
          <p className="m-0 max-w-[460px] text-body text-fg-2">
            {renderSegs(t("pi.prompts.noTemplatesDescription"))}
          </p>
          <div className="mt-2 flex gap-2">
            <Button
              variant="neutral"
              size="regular"
              onClick={() => setEditor({ mode: "create" })}
            >
              <Plus className="h-3.5 w-3.5" />
              {t("pi.prompts.newTemplate")}
            </Button>
          </div>
        </div>
      );
    }
    if (filteredTemplates.length === 0) {
      return (
        <div className="flex flex-col items-center gap-3 rounded-panel border border-border bg-surface px-6 py-10 text-center">
          <span className="text-body text-fg-2">
            {t("pi.prompts.noTemplateResults", { query: search.trim() })}
          </span>
          {onClearSearch ? (
            <Button variant="neutral" size="compact" onClick={onClearSearch}>
              {t("prompts.clearSearch")}
            </Button>
          ) : null}
        </div>
      );
    }
    return (
      <ul
        aria-label={t("pi.prompts.templatesLabel")}
        className="m-0 min-h-0 shrink list-none overflow-y-auto scroll-stable rounded-panel border border-border bg-surface p-0"
      >
        {filteredTemplates.map((template, index) => {
          const summary = getPiPromptTemplateSummary(template.content);
          const desc =
            summary.description ||
            stripPiPromptTemplateDescription(template.content)
              .split("\n")
              .map((line) => line.trim())
              .find((line) => line && line !== "---") ||
            "";
          return (
            <li
              key={template.slug}
              onClick={(event) => {
                const el = event.target as HTMLElement;
                if (el.closest("button, a, input, label, [role='menu']")) {
                  return;
                }
                setEditor({ mode: "edit", template });
              }}
              className={cn(
                "flex h-[60px] cursor-pointer items-center gap-3 pe-2 ps-4 transition-colors duration-150 hover:bg-subtle",
                index > 0 && "border-t border-border",
              )}
            >
              <div className="flex min-w-0 flex-1 flex-col">
                <div className="flex min-w-0 items-baseline gap-2">
                  <code className="whitespace-nowrap font-mono text-body font-medium text-fg-1">
                    /{template.slug}
                  </code>
                  {summary.argumentHint ? (
                    <code className="truncate font-mono text-caption text-fg-2">
                      {summary.argumentHint}
                    </code>
                  ) : null}
                </div>
                {desc ? (
                  <span
                    title={desc}
                    className="min-w-0 truncate text-caption text-fg-2"
                  >
                    {desc}
                  </span>
                ) : null}
              </div>
              <div className="flex shrink-0 gap-1">
                <HoverTip content={t("common.edit")}>
                  <Button
                    type="button"
                    variant="quiet"
                    size="icon-compact"
                    aria-label={t("prompts.editAria", {
                      name: `/${template.slug}`,
                    })}
                    onClick={() => setEditor({ mode: "edit", template })}
                  >
                    <Pencil className="h-[15px] w-[15px]" strokeWidth={1.5} />
                  </Button>
                </HoverTip>
                <DropdownMenu modal={false}>
                  <HoverTip content={t("common.more")}>
                    <DropdownMenuTrigger asChild>
                      <Button
                        type="button"
                        variant="quiet"
                        size="icon-compact"
                        aria-label={t("prompts.rowMoreActions", {
                          name: `/${template.slug}`,
                        })}
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
                    <DropdownMenuItem
                      className={promptMenuItemClass}
                      onSelect={() => {
                        void copyText(`/${template.slug}`).then((ok) => {
                          if (ok) {
                            showPromptToast(t, {
                              title: t("pi.prompts.copiedCommand", {
                                slug: template.slug,
                              }),
                            });
                          } else {
                            toast.error(t("prompts.copyFailed"));
                          }
                        });
                      }}
                    >
                      {t("pi.prompts.copyCommand")}
                    </DropdownMenuItem>
                    <DropdownMenuSeparator className="mx-1.5 my-1 bg-border" />
                    <DropdownMenuItem
                      disabled={remove.isPending}
                      className={cn(
                        promptMenuItemClass,
                        "text-danger-text focus:text-danger-text",
                      )}
                      onSelect={() => remove.mutate(template)}
                    >
                      {t("common.delete")}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </li>
          );
        })}
      </ul>
    );
  })();

  return (
    <>
      {body}
      {editor && (
        <PiPromptTemplateEditor
          template={editor.mode === "edit" ? editor.template : undefined}
          existingSlugs={existingSlugs}
          onClose={() => setEditor(null)}
          onChanged={refresh}
          onDelete={(template) => remove.mutate(template)}
        />
      )}
    </>
  );
});

/**
 * Kept as a compatibility export for callers that render the native resources
 * directly. The Pi page itself places these sections in separate segments.
 */
export function PiNativePromptResources() {
  return (
    <div className="space-y-6">
      <PiSystemPromptFiles />
      <PiPromptTemplates />
    </div>
  );
}

import { forwardRef, useImperativeHandle, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Braces,
  ChevronDown,
  ChevronRight,
  Edit3,
  FilePlus2,
  FileText,
  Loader2,
  RefreshCw,
  Search,
  SquareTerminal,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import MarkdownEditor from "@/components/MarkdownEditor";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ListItemRow } from "@/components/common/ListItemRow";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  promptsApi,
  type PiPromptFileKind,
  type PiPromptFileSnapshot,
  type PiPromptTemplate,
} from "@/lib/api/prompts";
import { useDarkMode } from "@/hooks/useDarkMode";
import {
  getPiPromptTemplateDescription,
  getPiPromptTemplateSummary,
  setPiPromptTemplateDescription,
  stripPiPromptTemplateDescription,
} from "@/lib/piPromptTemplate";
import { isValidPiPromptTemplateSlug } from "@/lib/piPromptSlug";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";

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

const promptTemplatesKey = ["pi", "promptTemplates"] as const;

function showMutationError(error: unknown, fallback: string) {
  toast.error(extractErrorMessage(error) || fallback);
}

function PiInstructionFileEditor({
  file,
  snapshot,
  onClose,
}: {
  file: (typeof EDITABLE_FILES)[number];
  snapshot: PiPromptFileSnapshot;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const darkMode = useDarkMode();
  const queryClient = useQueryClient();
  const [baseSnapshot] = useState(() => snapshot);
  const [draft, setDraft] = useState(baseSnapshot.content);
  const [confirmCreate, setConfirmCreate] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const queryKey = promptFileKey(file.kind);

  const save = useMutation({
    mutationFn: () =>
      promptsApi.replacePiPromptFile(file.kind, baseSnapshot.revision, draft),
    onSuccess: (nextSnapshot) => {
      queryClient.setQueryData<PiPromptFileSnapshot>(queryKey, nextSnapshot);
      toast.success(t("pi.prompts.fileSaved", { filename: file.filename }), {
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

  const remove = useMutation({
    mutationFn: () =>
      promptsApi.deletePiPromptFile(file.kind, baseSnapshot.revision),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey });
      toast.success(t("pi.prompts.fileRemoved", { filename: file.filename }), {
        description: t("pi.prompts.reloadNotice"),
      });
      setConfirmDelete(false);
      onClose();
    },
    onError: async (error) => {
      showMutationError(error, t("pi.prompts.deleteFailed"));
      await queryClient.invalidateQueries({ queryKey });
    },
  });

  const busy = save.isPending || remove.isPending;
  const changed = draft !== baseSnapshot.content;
  const blank = !draft.trim();

  const requestSave = () => {
    if (file.kind === "system_override" && !baseSnapshot.exists) {
      setConfirmCreate(true);
      return;
    }
    save.mutate();
  };

  return (
    <>
      <FullScreenPanel
        isOpen
        title={file.filename}
        onClose={onClose}
        footer={
          <>
            {baseSnapshot.exists && (
              <Button
                type="button"
                variant="outline"
                onClick={() => setConfirmDelete(true)}
                disabled={busy}
                className="mr-auto text-destructive hover:text-destructive"
              >
                <Trash2 className="h-4 w-4" aria-hidden="true" />
                {t("pi.prompts.removeGlobalFile")}
              </Button>
            )}
            <Button
              type="button"
              onClick={requestSave}
              disabled={!changed || blank || busy}
            >
              {save.isPending && (
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
              )}
              {baseSnapshot.exists
                ? t("common.save")
                : t("pi.prompts.saveAndConfigure")}
            </Button>
          </>
        }
      >
        <div className="glass w-full space-y-6 rounded-xl border border-white/10 p-6">
          {file.kind === "system_override" && (
            <div className="flex gap-2.5 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2.5 text-sm">
              <AlertTriangle
                className="mt-0.5 h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400"
                aria-hidden="true"
              />
              <span className="leading-relaxed">
                {t("pi.prompts.systemOverrideWarning")}
              </span>
            </div>
          )}

          <div>
            <Label htmlFor={`pi-${file.kind}`} className="mb-2 block">
              {t("pi.prompts.markdownContent")}
            </Label>
            <MarkdownEditor
              value={draft}
              onChange={setDraft}
              placeholder={t("pi.prompts.instructionPlaceholder")}
              darkMode={darkMode}
              minHeight="calc(100vh - 360px)"
            />
            {blank && (
              <p className="mt-2 text-xs text-destructive">
                {t("pi.prompts.blankInstruction")}
              </p>
            )}
          </div>
        </div>
      </FullScreenPanel>

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
        onConfirm={() => save.mutate()}
        onCancel={() => setConfirmCreate(false)}
      />

      <ConfirmDialog
        isOpen={confirmDelete}
        title={t("pi.prompts.removeFileTitle", {
          filename: file.filename,
        })}
        message={t("pi.prompts.removeFileMessage", {
          filename: file.filename,
        })}
        confirmText={t("common.delete")}
        zIndex="top"
        onConfirm={() => remove.mutate()}
        onCancel={() => setConfirmDelete(false)}
      />
    </>
  );
}

function PiInstructionFileCard({
  file,
}: {
  file: (typeof EDITABLE_FILES)[number];
}) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const query = useQuery({
    queryKey: promptFileKey(file.kind),
    queryFn: () => promptsApi.getPiPromptFile(file.kind),
  });

  const status = (() => {
    if (query.isLoading) return t("common.loading");
    if (query.isError) return t("pi.prompts.unavailable");
    if (!query.data?.exists) return t("pi.prompts.notConfigured");
    if (!query.data.content.trim()) return t("pi.prompts.configuredEmpty");
    return t("pi.prompts.configured");
  })();

  return (
    <>
      <div
        className={cn(
          "group flex min-h-[92px] items-center gap-3 rounded-xl border border-border bg-card transition-colors",
          query.data && "hover:bg-accent/50",
          query.isError && "border-destructive/30",
        )}
      >
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-3 p-4 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
          disabled={!query.data}
          onClick={() => setEditing(true)}
        >
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground transition-colors group-hover:text-foreground">
            {file.kind === "system_append" ? (
              <FilePlus2 className="h-5 w-5" aria-hidden="true" />
            ) : (
              <FileText className="h-5 w-5" aria-hidden="true" />
            )}
          </div>

          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-sm font-medium">{file.filename}</span>
              <Badge
                variant={
                  query.isError
                    ? "destructive"
                    : query.data?.exists
                      ? "secondary"
                      : "outline"
                }
                className="font-normal"
              >
                {query.isLoading && (
                  <Loader2
                    className="mr-1 h-3 w-3 animate-spin"
                    aria-hidden="true"
                  />
                )}
                {status}
              </Badge>
              {file.recommended && (
                <Badge
                  variant="outline"
                  className="border-blue-500/30 text-blue-600 dark:text-blue-400"
                >
                  {t("pi.prompts.recommended")}
                </Badge>
              )}
            </div>
            <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
              {t(file.descriptionKey)}
            </p>
          </div>

          {query.data && (
            <ChevronRight
              className="h-4 w-4 shrink-0 text-muted-foreground"
              aria-hidden="true"
            />
          )}
        </button>

        {query.isError && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="mr-3 shrink-0"
            onClick={() => void query.refetch()}
            disabled={query.isFetching}
            title={t("common.refresh")}
          >
            <RefreshCw
              className={cn("h-4 w-4", query.isFetching && "animate-spin")}
              aria-hidden="true"
            />
          </Button>
        )}
      </div>

      {editing && query.data && (
        <PiInstructionFileEditor
          file={file}
          snapshot={query.data}
          onClose={() => setEditing(false)}
        />
      )}
    </>
  );
}

export function PiSystemPromptFiles() {
  const { t } = useTranslation();

  return (
    <section>
      <p className="mb-4 max-w-3xl text-xs leading-relaxed text-muted-foreground">
        {t("pi.prompts.systemFilesDescription")}
      </p>

      <div className="grid grid-cols-1 gap-3">
        {EDITABLE_FILES.map((file) => (
          <PiInstructionFileCard key={file.kind} file={file} />
        ))}
      </div>
    </section>
  );
}

interface PiPromptTemplateEditorProps {
  template?: PiPromptTemplate;
  existingSlugs: Set<string>;
  onClose: () => void;
  onChanged: () => Promise<void>;
}

function PiPromptTemplateEditor({
  template,
  existingSlugs,
  onClose,
  onChanged,
}: PiPromptTemplateEditorProps) {
  const { t } = useTranslation();
  const darkMode = useDarkMode();
  const initialDescription = getPiPromptTemplateDescription(
    template?.content ?? "",
  );
  const initialContent = stripPiPromptTemplateDescription(
    template?.content ?? "",
  );
  const [slug, setSlug] = useState(template?.slug ?? "");
  const [description, setDescription] = useState(initialDescription ?? "");
  const [content, setContent] = useState(initialContent);
  const [helpOpen, setHelpOpen] = useState(false);
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
      toast.success(
        isCreate
          ? t("pi.prompts.templateCreated")
          : t("pi.prompts.templateSaved", { slug: saved.slug }),
        { description: t("pi.prompts.reloadNotice") },
      );
      onClose();
    },
    onError: (error) =>
      showMutationError(error, t("pi.prompts.templateSaveFailed")),
  });

  const busy = save.isPending;
  const canSave = slugIsValid && !slugAlreadyExists && changed && !busy;

  return (
    <>
      <FullScreenPanel
        isOpen
        title={
          isCreate
            ? t("pi.prompts.newTemplate")
            : t("pi.prompts.editTemplate", { slug: template.slug })
        }
        onClose={onClose}
        footer={
          <Button
            type="button"
            disabled={!canSave || busy}
            onClick={() => save.mutate()}
          >
            {save.isPending && (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            )}
            {isCreate ? t("pi.prompts.createTemplate") : t("common.save")}
          </Button>
        }
      >
        <div className="glass w-full space-y-6 rounded-xl border border-white/10 p-6">
          <div>
            <Label htmlFor="pi-template-slug">
              {t("pi.prompts.templateCommand")}
            </Label>
            <div className="relative mt-2">
              <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 font-mono text-sm text-muted-foreground">
                /
              </span>
              <Input
                id="pi-template-slug"
                value={slug}
                onChange={(event) => setSlug(event.target.value)}
                disabled={busy}
                className="pl-7 font-mono"
                placeholder={t("pi.prompts.templateSlug")}
                aria-invalid={
                  normalizedSlug.length > 0 &&
                  (!slugIsValid || slugAlreadyExists)
                }
              />
            </div>
            {normalizedSlug.length > 0 && !slugIsValid && (
              <p className="mt-1.5 text-xs text-destructive">
                {t("pi.prompts.templateSlugInvalid")}
              </p>
            )}
            {slugAlreadyExists && (
              <p className="mt-1.5 text-xs text-destructive">
                {t("pi.prompts.templateSlugExists")}
              </p>
            )}
          </div>

          <div>
            <Label htmlFor="pi-template-description">
              {t("pi.prompts.templateDescription")}
            </Label>
            <Input
              id="pi-template-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              disabled={busy}
              className="mt-2"
              placeholder={t("pi.prompts.templateDescriptionPlaceholder")}
            />
          </div>

          <div>
            <Label className="mb-2 block">
              {t("pi.prompts.templateContent")}
            </Label>
            <MarkdownEditor
              value={content}
              onChange={setContent}
              placeholder={t("pi.prompts.templateContentPlaceholder")}
              darkMode={darkMode}
              minHeight="calc(100vh - 430px)"
            />
          </div>

          <Collapsible open={helpOpen} onOpenChange={setHelpOpen}>
            <CollapsibleTrigger asChild>
              <button
                type="button"
                className="flex min-h-11 w-full items-center justify-between gap-3 rounded-lg border border-border px-3 py-2 text-left text-sm transition-colors hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <span className="flex items-center gap-2">
                  <Braces
                    className="h-4 w-4 text-muted-foreground"
                    aria-hidden="true"
                  />
                  {t("pi.prompts.templateSyntax")}
                </span>
                <ChevronDown
                  className={cn(
                    "h-4 w-4 text-muted-foreground transition-transform",
                    helpOpen && "rotate-180",
                  )}
                  aria-hidden="true"
                />
              </button>
            </CollapsibleTrigger>
            <CollapsibleContent>
              <div className="mt-2 rounded-lg bg-muted/50 p-3 text-xs leading-relaxed text-muted-foreground">
                <p>{t("pi.prompts.templateSyntaxDescription")}</p>
                <pre className="mt-3 overflow-x-auto rounded-md border border-border bg-background p-3 font-mono text-foreground">
                  {`---
description: Review the current changes
argument-hint: "<target> [focus]"
---
Review $1.
Focus on: $2
Remaining arguments: \${@:2}
All arguments: $ARGUMENTS`}
                </pre>
              </div>
            </CollapsibleContent>
          </Collapsible>
        </div>
      </FullScreenPanel>
    </>
  );
}

export interface PiPromptTemplatesHandle {
  openCreate: () => void;
}

export const PiPromptTemplates = forwardRef<PiPromptTemplatesHandle>(
  function PiPromptTemplates(_props, ref) {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const [search, setSearch] = useState("");
    const [editor, setEditor] = useState<
      { mode: "create" } | { mode: "edit"; template: PiPromptTemplate } | null
    >(null);
    const [pendingDelete, setPendingDelete] = useState<PiPromptTemplate | null>(
      null,
    );

    const templates = useQuery({
      queryKey: promptTemplatesKey,
      queryFn: () => promptsApi.listPiPromptTemplates(),
    });

    useImperativeHandle(ref, () => ({
      openCreate: () => setEditor({ mode: "create" }),
    }));

    const refresh = async () => {
      await queryClient.invalidateQueries({ queryKey: promptTemplatesKey });
    };

    const remove = useMutation({
      mutationFn: (template: PiPromptTemplate) =>
        promptsApi.deletePiPromptTemplate(template.slug, template.revision),
      onSuccess: async (_removed, template) => {
        await refresh();
        toast.success(
          t("pi.prompts.templateDeleted", { slug: template.slug }),
          { description: t("pi.prompts.reloadNotice") },
        );
        setPendingDelete(null);
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

    return (
      <section className="flex h-full min-h-0 flex-col">
        <p className="mb-4 max-w-3xl shrink-0 text-xs leading-relaxed text-muted-foreground">
          {t("pi.prompts.templatesDescription")}
        </p>

        {!templates.isLoading && !templates.isError && (
          <ManagementListSearch
            value={search}
            onValueChange={setSearch}
            placeholder={t("pi.prompts.searchTemplates")}
            ariaLabel={t("pi.prompts.searchTemplates")}
            clearLabel={t("common.clear")}
          />
        )}

        <ScrollArea className="-mr-3 min-h-0 flex-1" type="auto">
          <div className="pb-16 pr-3">
            {templates.isLoading ? (
              <div className="flex min-h-48 items-center justify-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                {t("common.loading")}
              </div>
            ) : templates.isError ? (
              <div className="flex min-h-48 flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-destructive/30 px-6 text-center">
                <AlertTriangle
                  className="h-8 w-8 text-destructive/70"
                  aria-hidden="true"
                />
                <p className="text-sm text-muted-foreground">
                  {t("pi.prompts.templateLoadFailed")}
                </p>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void templates.refetch()}
                >
                  <RefreshCw className="h-4 w-4" aria-hidden="true" />
                  {t("common.refresh")}
                </Button>
              </div>
            ) : (templates.data ?? []).length === 0 ? (
              <div className="flex min-h-52 flex-col items-center justify-center rounded-xl border border-dashed px-6 text-center">
                <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-muted">
                  <SquareTerminal
                    className="h-5 w-5 text-muted-foreground"
                    aria-hidden="true"
                  />
                </div>
                <h4 className="text-sm font-medium">
                  {t("pi.prompts.noTemplates")}
                </h4>
                <p className="mt-1 max-w-sm text-xs leading-relaxed text-muted-foreground">
                  {t("pi.prompts.noTemplatesDescription")}
                </p>
              </div>
            ) : filteredTemplates.length === 0 ? (
              <div className="flex min-h-40 flex-col items-center justify-center rounded-xl border border-dashed px-6 text-center">
                <Search
                  className="mb-3 h-8 w-8 text-muted-foreground/50"
                  aria-hidden="true"
                />
                <p className="text-sm text-muted-foreground">
                  {t("pi.prompts.noTemplateResults")}
                </p>
              </div>
            ) : (
              <div className="overflow-hidden rounded-xl border border-border bg-card">
                {filteredTemplates.map((template, index) => {
                  const summary = getPiPromptTemplateSummary(template.content);
                  return (
                    <ListItemRow
                      key={template.slug}
                      isLast={index === filteredTemplates.length - 1}
                    >
                      <button
                        type="button"
                        className="flex min-h-11 min-w-0 flex-1 items-center gap-3 rounded-md text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        onClick={() => setEditor({ mode: "edit", template })}
                        title={t("common.edit")}
                      >
                        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted font-mono text-sm text-muted-foreground">
                          /
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <code className="truncate text-sm font-medium text-foreground">
                              /{template.slug}
                            </code>
                            {summary.argumentHint && (
                              <code className="hidden truncate text-xs text-muted-foreground sm:block">
                                {summary.argumentHint}
                              </code>
                            )}
                          </div>
                          {summary.description && (
                            <p className="mt-0.5 truncate text-xs text-muted-foreground">
                              {summary.description}
                            </p>
                          )}
                        </div>
                        <Edit3
                          className="h-4 w-4 shrink-0 text-muted-foreground"
                          aria-hidden="true"
                        />
                      </button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="shrink-0 hover:text-destructive"
                        onClick={() => setPendingDelete(template)}
                        title={t("common.delete")}
                      >
                        <Trash2 className="h-4 w-4" aria-hidden="true" />
                      </Button>
                    </ListItemRow>
                  );
                })}
              </div>
            )}
          </div>
        </ScrollArea>

        {editor && (
          <PiPromptTemplateEditor
            template={editor.mode === "edit" ? editor.template : undefined}
            existingSlugs={existingSlugs}
            onClose={() => setEditor(null)}
            onChanged={refresh}
          />
        )}

        <ConfirmDialog
          isOpen={Boolean(pendingDelete)}
          title={t("pi.prompts.deleteTemplateTitle", {
            slug: pendingDelete?.slug,
          })}
          message={t("pi.prompts.deleteTemplateMessage", {
            slug: pendingDelete?.slug,
          })}
          confirmText={t("common.delete")}
          onConfirm={() => {
            if (pendingDelete) remove.mutate(pendingDelete);
          }}
          onCancel={() => setPendingDelete(null)}
        />
      </section>
    );
  },
);

/**
 * Kept as a compatibility export for callers that render the native resources
 * directly. The Pi page itself places these sections in separate tabs.
 */
export function PiNativePromptResources() {
  return (
    <div className="space-y-6">
      <PiSystemPromptFiles />
      <PiPromptTemplates />
    </div>
  );
}

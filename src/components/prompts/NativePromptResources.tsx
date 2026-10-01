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
  type OhMyPiPromptFileKind,
  type PiPromptFileKind,
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

/** Union of every native prompt file kind across pi and ohmypi. */
export type NativePromptFileKind = PiPromptFileKind | OhMyPiPromptFileKind;

export interface NativePromptFileSnapshot {
  exists: boolean;
  revision: string;
  content: string;
}

export interface NativePromptTemplate {
  slug: string;
  content: string;
  revision: string;
}

export interface NativePromptFileSpec {
  kind: NativePromptFileKind;
  filename: string;
  /** e.g. "APPEND_SYSTEM.md" vs "AGENTS.md" — the file the card represents. */
  titleKey: string;
  descriptionKey: string;
  recommended?: boolean;
  /** Card icon variant. */
  icon: "append" | "file";
}

export interface NativePromptResourcesApi {
  getFile: (kind: NativePromptFileKind) => Promise<NativePromptFileSnapshot>;
  replaceFile: (
    kind: NativePromptFileKind,
    expectedRevision: string,
    content: string,
  ) => Promise<NativePromptFileSnapshot>;
  deleteFile: (
    kind: NativePromptFileKind,
    expectedRevision: string,
  ) => Promise<boolean>;
  listTemplates: () => Promise<NativePromptTemplate[]>;
  upsertTemplate: (
    slug: string,
    expectedRevision: string,
    content: string,
    originalSlug?: string,
  ) => Promise<NativePromptTemplate>;
  deleteTemplate: (slug: string, expectedRevision: string) => Promise<boolean>;
}

export interface NativePromptResourcesConfig {
  /** i18n key namespace, e.g. "pi.prompts". */
  i18nKey: "pi.prompts" | "ohmypi.prompts";
  /** React Query key scope, kept distinct per app. */
  scope: string;
  /** id/htmlFor prefix used inside editors. */
  idPrefix: string;
  files: readonly NativePromptFileSpec[];
  api: NativePromptResourcesApi;
}

const promptFileKey = (
  config: NativePromptResourcesConfig,
  kind: NativePromptFileKind,
) => [config.scope, "promptFile", kind] as const;

const promptTemplatesKey = (config: NativePromptResourcesConfig) =>
  [config.scope, "promptTemplates"] as const;

/** Resolve a key inside the config's i18n namespace. */
const key = (config: NativePromptResourcesConfig, name: string) =>
  `${config.i18nKey}.${name}`;

function showMutationError(error: unknown, fallback: string) {
  toast.error(extractErrorMessage(error) || fallback);
}

function NativeInstructionFileEditor({
  config,
  file,
  snapshot,
  onClose,
}: {
  config: NativePromptResourcesConfig;
  file: NativePromptFileSpec;
  snapshot: NativePromptFileSnapshot;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const darkMode = useDarkMode();
  const queryClient = useQueryClient();
  const [baseSnapshot] = useState(() => snapshot);
  const [draft, setDraft] = useState(baseSnapshot.content);
  const [confirmCreate, setConfirmCreate] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const queryKey = promptFileKey(config, file.kind);

  const save = useMutation({
    mutationFn: () =>
      config.api.replaceFile(file.kind, baseSnapshot.revision, draft),
    onSuccess: (nextSnapshot) => {
      queryClient.setQueryData<NativePromptFileSnapshot>(
        queryKey,
        nextSnapshot,
      );
      toast.success(t(key(config, "fileSaved"), { filename: file.filename }), {
        description: t(key(config, "reloadNotice")),
      });
      setConfirmCreate(false);
      onClose();
    },
    onError: async (error) => {
      showMutationError(error, t(key(config, "saveFailed")));
      await queryClient.invalidateQueries({ queryKey });
    },
  });

  const remove = useMutation({
    mutationFn: () => config.api.deleteFile(file.kind, baseSnapshot.revision),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey });
      toast.success(
        t(key(config, "fileRemoved"), { filename: file.filename }),
        {
          description: t(key(config, "reloadNotice")),
        },
      );
      setConfirmDelete(false);
      onClose();
    },
    onError: async (error) => {
      showMutationError(error, t(key(config, "deleteFailed")));
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
                {t(key(config, "removeGlobalFile"))}
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
                : t(key(config, "saveAndConfigure"))}
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
                {t(key(config, "systemOverrideWarning"))}
              </span>
            </div>
          )}

          <div>
            <Label
              htmlFor={`${config.idPrefix}-${file.kind}`}
              className="mb-2 block"
            >
              {t(key(config, "markdownContent"))}
            </Label>
            <MarkdownEditor
              value={draft}
              onChange={setDraft}
              placeholder={t(key(config, "instructionPlaceholder"))}
              darkMode={darkMode}
              minHeight="calc(100vh - 360px)"
            />
            {blank && (
              <p className="mt-2 text-xs text-destructive">
                {t(key(config, "blankInstruction"))}
              </p>
            )}
          </div>
        </div>
      </FullScreenPanel>

      <ConfirmDialog
        isOpen={confirmCreate}
        title={t(key(config, "activateOverrideTitle"), {
          filename: file.filename,
        })}
        message={t(key(config, "activateOverrideMessage"), {
          filename: file.filename,
        })}
        confirmText={t(key(config, "saveAndConfigure"))}
        variant="info"
        zIndex="top"
        onConfirm={() => save.mutate()}
        onCancel={() => setConfirmCreate(false)}
      />

      <ConfirmDialog
        isOpen={confirmDelete}
        title={t(key(config, "removeFileTitle"), {
          filename: file.filename,
        })}
        message={t(key(config, "removeFileMessage"), {
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

function NativeInstructionFileCard({
  config,
  file,
}: {
  config: NativePromptResourcesConfig;
  file: NativePromptFileSpec;
}) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const query = useQuery({
    queryKey: promptFileKey(config, file.kind),
    queryFn: () => config.api.getFile(file.kind),
  });

  const status = (() => {
    if (query.isLoading) return t("common.loading");
    if (query.isError) return t(key(config, "unavailable"));
    if (!query.data?.exists) return t(key(config, "notConfigured"));
    if (!query.data.content.trim()) return t(key(config, "configuredEmpty"));
    return t(key(config, "configured"));
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
            {file.icon === "append" ? (
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
                  {t(key(config, "recommended"))}
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
        <NativeInstructionFileEditor
          config={config}
          file={file}
          snapshot={query.data}
          onClose={() => setEditing(false)}
        />
      )}
    </>
  );
}

export function NativeSystemPromptFiles({
  config,
}: {
  config: NativePromptResourcesConfig;
}) {
  const { t } = useTranslation();

  return (
    <section>
      <p className="mb-4 max-w-3xl text-xs leading-relaxed text-muted-foreground">
        {t(key(config, "systemFilesDescription"))}
      </p>

      <div className="grid grid-cols-1 gap-3">
        {config.files.map((file) => (
          <NativeInstructionFileCard
            key={file.kind}
            config={config}
            file={file}
          />
        ))}
      </div>
    </section>
  );
}

interface NativePromptTemplateEditorProps {
  config: NativePromptResourcesConfig;
  template?: NativePromptTemplate;
  existingSlugs: Set<string>;
  onClose: () => void;
  onChanged: () => Promise<void>;
}

function NativePromptTemplateEditor({
  config,
  template,
  existingSlugs,
  onClose,
  onChanged,
}: NativePromptTemplateEditorProps) {
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
      config.api.upsertTemplate(
        normalizedSlug,
        template?.revision ?? "missing",
        serializedContent,
        template?.slug,
      ),
    onSuccess: async (saved) => {
      await onChanged();
      toast.success(
        isCreate
          ? t(key(config, "templateCreated"))
          : t(key(config, "templateSaved"), { slug: saved.slug }),
        { description: t(key(config, "reloadNotice")) },
      );
      onClose();
    },
    onError: (error) =>
      showMutationError(error, t(key(config, "templateSaveFailed"))),
  });

  const busy = save.isPending;
  const canSave = slugIsValid && !slugAlreadyExists && changed && !busy;

  return (
    <>
      <FullScreenPanel
        isOpen
        title={
          isCreate
            ? t(key(config, "newTemplate"))
            : t(key(config, "editTemplate"), { slug: template.slug })
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
            {isCreate ? t(key(config, "createTemplate")) : t("common.save")}
          </Button>
        }
      >
        <div className="glass w-full space-y-6 rounded-xl border border-white/10 p-6">
          <div>
            <Label htmlFor={`${config.idPrefix}-template-slug`}>
              {t(key(config, "templateCommand"))}
            </Label>
            <div className="relative mt-2">
              <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 font-mono text-sm text-muted-foreground">
                /
              </span>
              <Input
                id={`${config.idPrefix}-template-slug`}
                value={slug}
                onChange={(event) => setSlug(event.target.value)}
                disabled={busy}
                className="pl-7 font-mono"
                placeholder={t(key(config, "templateSlug"))}
                aria-invalid={
                  normalizedSlug.length > 0 &&
                  (!slugIsValid || slugAlreadyExists)
                }
              />
            </div>
            {normalizedSlug.length > 0 && !slugIsValid && (
              <p className="mt-1.5 text-xs text-destructive">
                {t(key(config, "templateSlugInvalid"))}
              </p>
            )}
            {slugAlreadyExists && (
              <p className="mt-1.5 text-xs text-destructive">
                {t(key(config, "templateSlugExists"))}
              </p>
            )}
          </div>

          <div>
            <Label htmlFor={`${config.idPrefix}-template-description`}>
              {t(key(config, "templateDescription"))}
            </Label>
            <Input
              id={`${config.idPrefix}-template-description`}
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              disabled={busy}
              className="mt-2"
              placeholder={t(key(config, "templateDescriptionPlaceholder"))}
            />
          </div>

          <div>
            <Label className="mb-2 block">
              {t(key(config, "templateContent"))}
            </Label>
            <MarkdownEditor
              value={content}
              onChange={setContent}
              placeholder={t(key(config, "templateContentPlaceholder"))}
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
                  {t(key(config, "templateSyntax"))}
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
                <p>{t(key(config, "templateSyntaxDescription"))}</p>
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

export interface NativePromptTemplatesHandle {
  openCreate: () => void;
}

export const NativePromptTemplates = forwardRef<
  NativePromptTemplatesHandle,
  { config: NativePromptResourcesConfig }
>(function NativePromptTemplates({ config }, ref) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [editor, setEditor] = useState<
    { mode: "create" } | { mode: "edit"; template: NativePromptTemplate } | null
  >(null);
  const [pendingDelete, setPendingDelete] =
    useState<NativePromptTemplate | null>(null);

  const templates = useQuery({
    queryKey: promptTemplatesKey(config),
    queryFn: () => config.api.listTemplates(),
  });

  useImperativeHandle(ref, () => ({
    openCreate: () => setEditor({ mode: "create" }),
  }));

  const refresh = async () => {
    await queryClient.invalidateQueries({
      queryKey: promptTemplatesKey(config),
    });
  };

  const remove = useMutation({
    mutationFn: (template: NativePromptTemplate) =>
      config.api.deleteTemplate(template.slug, template.revision),
    onSuccess: async (_removed, template) => {
      await refresh();
      toast.success(
        t(key(config, "templateDeleted"), { slug: template.slug }),
        { description: t(key(config, "reloadNotice")) },
      );
      setPendingDelete(null);
    },
    onError: (error) =>
      showMutationError(error, t(key(config, "templateDeleteFailed"))),
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
        {t(key(config, "templatesDescription"))}
      </p>

      {!templates.isLoading && !templates.isError && (
        <ManagementListSearch
          value={search}
          onValueChange={setSearch}
          placeholder={t(key(config, "searchTemplates"))}
          ariaLabel={t(key(config, "searchTemplates"))}
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
                {t(key(config, "templateLoadFailed"))}
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
                {t(key(config, "noTemplates"))}
              </h4>
              <p className="mt-1 max-w-sm text-xs leading-relaxed text-muted-foreground">
                {t(key(config, "noTemplatesDescription"))}
              </p>
            </div>
          ) : filteredTemplates.length === 0 ? (
            <div className="flex min-h-40 flex-col items-center justify-center rounded-xl border border-dashed px-6 text-center">
              <Search
                className="mb-3 h-8 w-8 text-muted-foreground/50"
                aria-hidden="true"
              />
              <p className="text-sm text-muted-foreground">
                {t(key(config, "noTemplateResults"))}
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
        <NativePromptTemplateEditor
          config={config}
          template={editor.mode === "edit" ? editor.template : undefined}
          existingSlugs={existingSlugs}
          onClose={() => setEditor(null)}
          onChanged={refresh}
        />
      )}

      <ConfirmDialog
        isOpen={Boolean(pendingDelete)}
        title={t(key(config, "deleteTemplateTitle"), {
          slug: pendingDelete?.slug,
        })}
        message={t(key(config, "deleteTemplateMessage"), {
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
});

export const PI_PROMPT_RESOURCES_CONFIG: NativePromptResourcesConfig = {
  i18nKey: "pi.prompts",
  scope: "pi",
  idPrefix: "pi",
  files: [
    {
      kind: "system_append",
      filename: "APPEND_SYSTEM.md",
      titleKey: "pi.prompts.systemAppend",
      descriptionKey: "pi.prompts.systemAppendDescription",
      recommended: true,
      icon: "append",
    },
    {
      kind: "system_override",
      filename: "SYSTEM.md",
      titleKey: "pi.prompts.systemOverride",
      descriptionKey: "pi.prompts.systemOverrideDescription",
      icon: "file",
    },
  ],
  api: {
    getFile: (kind) => promptsApi.getPiPromptFile(kind as PiPromptFileKind),
    replaceFile: (kind, expectedRevision, content) =>
      promptsApi.replacePiPromptFile(
        kind as PiPromptFileKind,
        expectedRevision,
        content,
      ),
    deleteFile: (kind, expectedRevision) =>
      promptsApi.deletePiPromptFile(kind as PiPromptFileKind, expectedRevision),
    listTemplates: () => promptsApi.listPiPromptTemplates(),
    upsertTemplate: (slug, expectedRevision, content, originalSlug) =>
      promptsApi.upsertPiPromptTemplate(
        slug,
        expectedRevision,
        content,
        originalSlug,
      ),
    deleteTemplate: (slug, expectedRevision) =>
      promptsApi.deletePiPromptTemplate(slug, expectedRevision),
  },
};

export const OHMYPI_PROMPT_RESOURCES_CONFIG: NativePromptResourcesConfig = {
  i18nKey: "ohmypi.prompts",
  scope: "ohmypi",
  idPrefix: "ohmypi",
  files: [
    {
      kind: "agents",
      filename: "AGENTS.md",
      titleKey: "ohmypi.prompts.agentsFile",
      descriptionKey: "ohmypi.prompts.agentsFileDescription",
      recommended: true,
      icon: "file",
    },
    {
      kind: "system_override",
      filename: "SYSTEM.md",
      titleKey: "ohmypi.prompts.systemOverride",
      descriptionKey: "ohmypi.prompts.systemOverrideDescription",
      icon: "file",
    },
    {
      kind: "system_append",
      filename: "APPEND_SYSTEM.md",
      titleKey: "ohmypi.prompts.systemAppend",
      descriptionKey: "ohmypi.prompts.systemAppendDescription",
      icon: "file",
    },
  ],
  api: {
    getFile: (kind) =>
      promptsApi.getOhMyPiPromptFile(kind as OhMyPiPromptFileKind),
    replaceFile: (kind, expectedRevision, content) =>
      promptsApi.replaceOhMyPiPromptFile(
        kind as OhMyPiPromptFileKind,
        expectedRevision,
        content,
      ),
    deleteFile: (kind, expectedRevision) =>
      promptsApi.deleteOhMyPiPromptFile(
        kind as OhMyPiPromptFileKind,
        expectedRevision,
      ),
    listTemplates: () => promptsApi.listOhMyPiPromptTemplates(),
    upsertTemplate: (slug, expectedRevision, content, originalSlug) =>
      promptsApi.upsertOhMyPiPromptTemplate(
        slug,
        expectedRevision,
        content,
        originalSlug,
      ),
    deleteTemplate: (slug, expectedRevision) =>
      promptsApi.deleteOhMyPiPromptTemplate(slug, expectedRevision),
  },
};

export const NATIVE_PROMPT_RESOURCES_CONFIGS: Record<
  "pi" | "ohmypi",
  NativePromptResourcesConfig
> = {
  pi: PI_PROMPT_RESOURCES_CONFIG,
  ohmypi: OHMYPI_PROMPT_RESOURCES_CONFIG,
};

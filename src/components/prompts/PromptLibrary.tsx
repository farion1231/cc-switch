import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Download, FileText, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId, Prompt } from "@/lib/api";
import PromptListItem from "./PromptListItem";
import {
  firstLine,
  formatSize,
  formatUpdated,
  renderSegs,
  utf8Bytes,
} from "./promptUtils";

interface PromptLibraryProps {
  prompts: Record<string, Prompt>;
  searchQuery: string;
  listLabel: string;
  disabled?: boolean;
  onClearSearch: () => void;
  onToggle: (id: string, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
  onCopyToApps?: (id: string) => void;
  onCopyContent?: (id: string) => void;
  /** 删除不能用时的原因；默认启用中的不能删 */
  getDeleteBlockedReason?: (id: string, prompt: Prompt) => string | undefined;
}

export function filterPromptEntries(
  entries: Array<[string, Prompt]>,
  query: string,
): Array<[string, Prompt]> {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return entries;
  return entries.filter(([recordId, prompt]) =>
    [recordId, prompt.id, prompt.name, prompt.description, prompt.content].some(
      (value) => value?.toLocaleLowerCase().includes(normalized),
    ),
  );
}

/** 提示库列表（Prompts.dc.html 第 165–190 行）。空库由页面自己画空状态，这里只管有条目时。 */
export function PromptLibrary({
  prompts,
  searchQuery,
  listLabel,
  disabled = false,
  onClearSearch,
  onToggle,
  onEdit,
  onDelete,
  onCopyToApps,
  onCopyContent,
  getDeleteBlockedReason,
}: PromptLibraryProps) {
  const { t, i18n } = useTranslation();
  const entries = useMemo(() => Object.entries(prompts), [prompts]);
  const filtered = useMemo(
    () => filterPromptEntries(entries, searchQuery),
    [entries, searchQuery],
  );

  if (filtered.length === 0) {
    return (
      <div className="flex flex-col items-center gap-3 rounded-panel border border-border bg-surface px-6 py-10 text-center">
        <span className="text-body text-fg-2">
          {t("prompts.noSearchResults", { query: searchQuery.trim() })}
        </span>
        <Button variant="neutral" size="compact" onClick={onClearSearch}>
          {t("prompts.clearSearch")}
        </Button>
      </div>
    );
  }

  return (
    <ul
      aria-label={listLabel}
      className="m-0 min-h-0 shrink list-none overflow-y-auto scroll-stable rounded-panel border border-border bg-surface p-0"
    >
      {filtered.map(([id, prompt], index) => {
        const summary =
          prompt.description?.trim() ||
          firstLine(prompt.content) ||
          t("prompts.emptyContent");
        const updated = formatUpdated(t, i18n.language, prompt.updatedAt);
        const detail = [summary, formatSize(utf8Bytes(prompt.content)), updated]
          .filter(Boolean)
          .join(" · ");
        const blocked = getDeleteBlockedReason
          ? getDeleteBlockedReason(id, prompt)
          : prompt.enabled
            ? t("prompts.deleteBlocked")
            : undefined;
        return (
          <PromptListItem
            key={id}
            id={id}
            prompt={prompt}
            active={prompt.enabled}
            detail={detail}
            first={index === 0}
            disabled={disabled}
            onToggle={onToggle}
            onEdit={onEdit}
            onDelete={onDelete}
            onCopyToApps={onCopyToApps}
            onCopyContent={onCopyContent}
            deleteBlockedReason={blocked}
          />
        );
      })}
    </ul>
  );
}

interface PromptEmptyStateProps {
  app: AppId;
  displayPath: string;
  /** 目标文件现在的内容；null = 文件不存在 */
  fileText: string | null;
  disabled?: boolean;
  onImport: () => void;
  onAdd: () => void;
}

/**
 * 提示库还是空的（PromptsImport.dc.html）：只说启用的那条写到哪里；
 * 文件里已有内容时给「导入现有内容」（导入只存进库、不改文件，后果写在导入后的 toast 里）。
 */
export function PromptEmptyState({
  app,
  displayPath,
  fileText,
  disabled = false,
  onImport,
  onAdd,
}: PromptEmptyStateProps) {
  const { t } = useTranslation();
  const hasFile = Boolean(fileText?.trim());
  const peek = (fileText ?? "")
    .split("\n")
    .filter((line) => line.trim())
    .slice(0, 2)
    .join("  ");

  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-10 pb-10 text-center">
      <h2 className="m-0 text-section text-fg-1">
        {t("prompts.emptyTitle", { app: APP_DISPLAY_NAME[app] })}
      </h2>
      <p className="m-0 max-w-[460px] text-body text-fg-2">
        {renderSegs(t("prompts.emptyTarget", { path: displayPath }))}
      </p>
      {hasFile ? (
        <div
          id="prompt-empty-file"
          className="mt-2 flex w-[460px] max-w-full items-start gap-2.5 rounded-panel bg-subtle px-3.5 py-3 text-left"
        >
          <FileText
            aria-hidden="true"
            strokeWidth={1.5}
            className="mt-0.5 h-4 w-4 shrink-0 text-fg-2"
          />
          <div className="flex min-w-0 flex-col">
            <span className="text-body font-medium text-fg-1">
              {renderSegs(
                t("prompts.emptyFile", {
                  path: displayPath,
                  size: formatSize(utf8Bytes(fileText ?? "")),
                }),
              )}
            </span>
            <code
              title={peek}
              className="mt-1 truncate font-mono text-caption text-fg-2"
            >
              {peek}
            </code>
          </div>
        </div>
      ) : null}
      <div className="mt-2 flex gap-2">
        {hasFile ? (
          <Button
            variant="neutral"
            size="regular"
            aria-describedby="prompt-empty-file"
            disabled={disabled}
            onClick={onImport}
          >
            <Download className="h-3.5 w-3.5" />
            {t("prompts.importExisting")}
          </Button>
        ) : null}
        <Button
          variant="neutral"
          size="regular"
          disabled={disabled}
          onClick={onAdd}
        >
          <Plus className="h-3.5 w-3.5" />
          {t("prompts.add")}
        </Button>
      </div>
    </div>
  );
}

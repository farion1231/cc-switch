import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Download } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import type { PluginSubagent, UnmanagedSubagent } from "@/lib/api/subagents";
import { renderSegs } from "@/components/prompts/promptUtils";
import { cn } from "@/lib/utils";
import { ModelBadge, subagentBadgeClass } from "./SubagentListItem";
import { fieldsOf, matchesQuery } from "./subagentUtils";

const listClass =
  "m-0 min-h-0 shrink list-none overflow-y-auto scroll-stable rounded-panel border border-border bg-surface p-0";

function EmptyBlock({ title, body }: { title: string; body: string }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-10 pb-12 text-center">
      <h2 className="m-0 text-section">{title}</h2>
      <p className="m-0 max-w-[480px] text-body text-fg-2">
        {renderSegs(body)}
      </p>
    </div>
  );
}

export function NoSearchResults({
  query,
  onClear,
}: {
  query: string;
  onClear: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col items-center gap-3 rounded-panel border border-border bg-surface px-6 py-10 text-center">
      <span className="text-body text-fg-2">
        {t("subagents.noSearchResults", { query: query.trim() })}
      </span>
      <Button type="button" variant="neutral" size="compact" onClick={onClear}>
        {t("subagents.clearSearch")}
      </Button>
    </div>
  );
}

interface SubagentUnmanagedListProps {
  items: UnmanagedSubagent[];
  loading: boolean;
  searchQuery: string;
  dirDisplayPath: string;
  pending: boolean;
  onClearSearch: () => void;
  onImport: (fileNames: string[]) => void;
}

/**
 * 「未管理」页签：agents 目录里有、cc-switch 库里没有的文件。勾选后批量导入；
 * frontmatter 解析失败的不能导入（导进来也写不回一个 Claude Code 能读的文件）。
 */
export function SubagentUnmanagedList({
  items,
  loading,
  searchQuery,
  dirDisplayPath,
  pending,
  onClearSearch,
  onImport,
}: SubagentUnmanagedListProps) {
  const { t } = useTranslation();
  const importable = useMemo(
    () => items.filter((item) => !item.parseError),
    [items],
  );
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(importable.map((item) => item.fileName)),
  );
  // 重新扫描后去掉已经不在的；新出现的默认勾上，用户取消过的保持不勾
  const seenRef = useRef<Set<string>>(
    new Set(importable.map((item) => item.fileName)),
  );
  useEffect(() => {
    const seen = seenRef.current;
    setSelected((prev) => {
      const next = new Set<string>();
      for (const item of importable) {
        if (prev.has(item.fileName) || !seen.has(item.fileName)) {
          next.add(item.fileName);
        }
      }
      return next;
    });
    for (const item of importable) seen.add(item.fileName);
  }, [importable]);

  const visible = items.filter((item) =>
    matchesQuery(searchQuery, [item.name, item.description, item.fileName]),
  );

  if (loading && items.length === 0) {
    return (
      <div className="py-12 text-center text-body text-fg-2">
        {t("common.loading")}
      </div>
    );
  }
  if (items.length === 0) {
    return (
      <EmptyBlock
        title={t("subagents.unmanaged.emptyTitle")}
        body={t("subagents.unmanaged.emptyBody", { path: dirDisplayPath })}
      />
    );
  }
  if (visible.length === 0) {
    return <NoSearchResults query={searchQuery} onClear={onClearSearch} />;
  }

  const visibleImportable = visible.filter((item) => !item.parseError);
  const nVisibleSelected = visibleImportable.filter((item) =>
    selected.has(item.fileName),
  ).length;
  const allSelected =
    visibleImportable.length > 0 &&
    nVisibleSelected === visibleImportable.length;
  const chosen = importable
    .filter((item) => selected.has(item.fileName))
    .map((item) => item.fileName);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex shrink-0 items-center gap-3">
        <label className="flex items-center gap-2 text-body text-fg-1">
          <Checkbox
            disabled={pending || visibleImportable.length === 0}
            checked={
              allSelected
                ? true
                : nVisibleSelected > 0
                  ? "indeterminate"
                  : false
            }
            onCheckedChange={() =>
              setSelected((prev) => {
                const next = new Set(prev);
                for (const item of visibleImportable) {
                  if (allSelected) next.delete(item.fileName);
                  else next.add(item.fileName);
                }
                return next;
              })
            }
          />
          {t("subagents.unmanaged.selectAll")}
        </label>
        <span className="text-caption text-fg-2">
          {renderSegs(t("subagents.unmanaged.hint", { path: dirDisplayPath }))}
        </span>
        <div className="flex-1" />
        <Button
          type="button"
          variant="solid"
          size="regular"
          disabled={pending || chosen.length === 0}
          onClick={() => onImport(chosen)}
        >
          <Download className="h-3.5 w-3.5" />
          {pending
            ? t("subagents.unmanaged.importing")
            : t("subagents.unmanaged.importSelected", {
                count: chosen.length,
              })}
        </Button>
      </div>
      <ul aria-label={t("subagents.tabs.unmanaged")} className={listClass}>
        {visible.map((item, index) => {
          const broken = Boolean(item.parseError);
          const model = broken ? undefined : fieldsOf(item).model;
          return (
            <li
              key={item.fileName}
              className={cn(
                "flex items-start gap-3 py-3 pe-4 ps-4",
                index > 0 && "border-t border-border",
              )}
            >
              <Checkbox
                className="mt-0.5"
                disabled={pending || broken}
                checked={!broken && selected.has(item.fileName)}
                aria-label={t("subagents.unmanaged.selectAria", {
                  name: item.name ?? item.fileName,
                })}
                onCheckedChange={(checked) =>
                  setSelected((prev) => {
                    const next = new Set(prev);
                    if (checked) next.add(item.fileName);
                    else next.delete(item.fileName);
                    return next;
                  })
                }
              />
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <div className="flex min-w-0 items-center gap-1.5">
                  <span
                    className={cn(
                      "min-w-0 truncate font-mono text-body font-medium",
                      broken ? "text-fg-3" : "text-fg-1",
                    )}
                  >
                    {item.name ?? item.fileName}
                  </span>
                  {!broken ? <ModelBadge model={model} /> : null}
                  <span
                    title={item.path}
                    className="min-w-0 truncate font-mono text-caption text-fg-3"
                  >
                    {item.fileName}
                  </span>
                </div>
                {item.description ? (
                  <p className="m-0 line-clamp-2 text-caption text-fg-2">
                    {item.description}
                  </p>
                ) : null}
                {broken ? (
                  <span
                    className={cn(
                      subagentBadgeClass,
                      "h-auto self-start whitespace-normal bg-danger-soft py-0.5 text-danger-text",
                    )}
                  >
                    <AlertTriangle
                      aria-hidden="true"
                      className="h-3 w-3 shrink-0"
                      strokeWidth={1.5}
                    />
                    {t("subagents.unmanaged.parseError", {
                      error: item.parseError,
                    })}
                  </span>
                ) : null}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

interface SubagentPluginListProps {
  items: PluginSubagent[];
  loading: boolean;
  searchQuery: string;
  onClearSearch: () => void;
}

/** 「插件」页签：插件自带的 subagent，只读；要改去插件里改 */
export function SubagentPluginList({
  items,
  loading,
  searchQuery,
  onClearSearch,
}: SubagentPluginListProps) {
  const { t } = useTranslation();
  const visible = items.filter((item) =>
    matchesQuery(searchQuery, [item.name, item.description, item.pluginName]),
  );

  if (loading && items.length === 0) {
    return (
      <div className="py-12 text-center text-body text-fg-2">
        {t("common.loading")}
      </div>
    );
  }
  if (items.length === 0) {
    return (
      <EmptyBlock
        title={t("subagents.plugins.emptyTitle")}
        body={t("subagents.plugins.emptyBody")}
      />
    );
  }
  if (visible.length === 0) {
    return <NoSearchResults query={searchQuery} onClear={onClearSearch} />;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <p className="m-0 shrink-0 text-caption text-fg-2">
        {t("subagents.plugins.hint")}
      </p>
      <ul aria-label={t("subagents.tabs.plugins")} className={listClass}>
        {visible.map((item, index) => (
          <li
            key={`${item.pluginName}:${item.name}:${item.path}`}
            className={cn(
              "flex flex-col gap-0.5 py-3 pe-4 ps-4",
              index > 0 && "border-t border-border",
            )}
          >
            <div className="flex min-w-0 items-center gap-1.5">
              <span className="min-w-0 truncate font-mono text-body font-medium text-fg-1">
                {item.name}
              </span>
              <span
                className={cn(subagentBadgeClass, "bg-selected text-fg-1")}
                title={t("subagents.plugins.fromPlugin", {
                  plugin: item.pluginName,
                })}
              >
                {item.pluginName}
              </span>
              <ModelBadge model={item.model} />
            </div>
            {item.description ? (
              <p className="m-0 line-clamp-2 text-caption text-fg-2">
                {item.description}
              </p>
            ) : null}
            <span
              title={item.path}
              className="min-w-0 truncate font-mono text-caption text-fg-3"
            >
              {item.path}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

import { memo, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Copy,
  Folder,
  MoreHorizontal,
  Play,
  Search,
  SlidersHorizontal,
  SquareTerminal,
  X,
} from "lucide-react";

import { AppGlyph } from "@/components/shell/AppGlyph";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HoverTip } from "@/components/ui/hover-tip";
import { fieldClass } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import type { SessionMeta, TurnIndex } from "@/types";
import { sessionMenuItemClass } from "../SessionItem";
import {
  buildCdResumeCommand,
  canDeleteSession,
  formatClock,
  formatShortDateTime,
  isSessionAppId,
} from "../utils";
import { useReaderT } from "./i18n";
import { PathChip } from "./PathChip";
import { SessionQuestionsMenu } from "./SessionToc";
import { formatDuration } from "./toolSummary";
import type { ReaderFilter } from "./turns";

export const readerIconButton =
  "inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-40";

// ─── 页头：返回、标题、上一个 / 下一个、恢复、⋯ ───────────────────────────

export interface SessionReaderHeaderProps {
  session: SessionMeta;
  title: string;
  appName: string;
  launchTerminal: string | null;
  hasPrev: boolean;
  hasNext: boolean;
  canCopyMarkdown: boolean;
  includeThinking: boolean;
  onIncludeThinkingChange: (value: boolean) => void;
  onPrev: () => void;
  onNext: () => void;
  onBack: () => void;
  onLaunch: () => void;
  onCopy: (text: string, message: string) => void;
  onCopyMarkdown: () => void;
  onOpenTerminalSettings: () => void;
  onReload: () => void;
  onDelete: () => void;
}

export const SessionReaderHeader = memo(function SessionReaderHeader({
  session,
  title,
  appName,
  launchTerminal,
  hasPrev,
  hasNext,
  canCopyMarkdown,
  includeThinking,
  onIncludeThinkingChange,
  onPrev,
  onNext,
  onBack,
  onLaunch,
  onCopy,
  onCopyMarkdown,
  onOpenTerminalSettings,
  onReload,
  onDelete,
}: SessionReaderHeaderProps) {
  const { t } = useTranslation();
  const rt = useReaderT();
  const appId = isSessionAppId(session.providerId) ? session.providerId : null;
  const isCodex = session.providerId === "codex";
  const command = session.resumeCommand;
  const deletable = canDeleteSession(session);

  const launchLabel = launchTerminal
    ? t("sessionManager.resumeIn", {
        defaultValue: "在 {{terminal}} 中恢复",
        terminal: launchTerminal,
      })
    : null;

  const copyResume = () =>
    command &&
    onCopy(
      command,
      launchTerminal
        ? t("sessionManager.resumeCommandCopied", {
            defaultValue: "已复制恢复命令",
          })
        : t("sessionManager.resumeCopiedNoLaunch", {
            defaultValue:
              "已复制恢复命令。这个平台暂不支持一键恢复，请在项目目录下粘贴到终端运行",
          }),
    );

  let resume: ReactNode;
  if (command && launchLabel) {
    resume = (
      <div className="flex shrink-0">
        <Button
          variant="solid"
          size="regular"
          onClick={onLaunch}
          className="rounded-e-none pe-3.5 ps-3"
        >
          <Play className="h-3.5 w-3.5" strokeWidth={2} />
          {launchLabel}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="solid"
              size="regular"
              aria-label={t("sessionManager.moreResumeOptions", {
                defaultValue: "更多恢复方式",
              })}
              className="w-8 rounded-s-none border-s border-s-[color-mix(in_srgb,var(--action-fg)_20%,transparent)] px-0"
            >
              <ChevronDown className="h-3.5 w-3.5" strokeWidth={2} />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="start"
            className="w-[300px] max-w-[calc(100vw-32px)] rounded-panel p-1 shadow-v7-md"
          >
            <DropdownMenuItem
              className={sessionMenuItemClass}
              onSelect={onLaunch}
            >
              <Play className="h-3.5 w-3.5 text-fg-2" strokeWidth={1.5} />
              {launchLabel}
            </DropdownMenuItem>
            <DropdownMenuItem
              className={sessionMenuItemClass}
              title={command}
              onSelect={copyResume}
            >
              <Copy className="h-3.5 w-3.5 text-fg-2" strokeWidth={1.5} />
              <span className="shrink-0">
                {t("sessionManager.copyResumeCommand", {
                  defaultValue: "复制恢复命令",
                })}
              </span>
              <span
                aria-hidden="true"
                className="min-w-0 flex-1 truncate text-right font-mono text-caption text-fg-3"
              >
                {command}
              </span>
            </DropdownMenuItem>
            {session.projectDir && (
              <DropdownMenuItem
                className={sessionMenuItemClass}
                onSelect={() =>
                  onCopy(
                    buildCdResumeCommand(session.projectDir!, command),
                    t("sessionManager.cdResumeCopied", {
                      defaultValue: "已复制「进入目录并恢复」命令",
                    }),
                  )
                }
              >
                <Folder className="h-3.5 w-3.5 text-fg-2" strokeWidth={1.5} />
                {t("sessionManager.copyCdResume", {
                  defaultValue: "复制「进入目录并恢复」命令",
                })}
              </DropdownMenuItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className={sessionMenuItemClass}
              onSelect={onOpenTerminalSettings}
            >
              <SquareTerminal
                className="h-3.5 w-3.5 text-fg-2"
                strokeWidth={1.5}
              />
              {t("sessionManager.changeTerminal", {
                defaultValue: "更换首选终端…",
              })}
            </DropdownMenuItem>
            {isCodex && (
              <p className="-mx-1 -mb-1 mt-1 border-t border-border px-3.5 pb-2.5 pt-2 text-caption text-fg-2">
                {t("sessionManager.codexResumeNote", {
                  defaultValue:
                    "切换过供应商后，旧会话可能无法继续：Codex 按供应商分开保存历史。",
                })}
              </p>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    );
  } else {
    // 不能一键恢复：有命令就复制；没有命令按钮禁用，信息栏下面写原因
    resume = (
      <Button
        variant="solid"
        size="regular"
        aria-disabled={command ? undefined : true}
        aria-describedby={command ? undefined : "session-resume-note"}
        onClick={command ? copyResume : undefined}
        className={cn(
          "shrink-0 pe-3.5 ps-3",
          !command && "cursor-not-allowed opacity-45 hover:bg-action",
        )}
      >
        {command || !launchTerminal ? (
          <Copy className="h-3.5 w-3.5" strokeWidth={2} />
        ) : (
          <Play className="h-3.5 w-3.5" strokeWidth={2} />
        )}
        {launchLabel && !command
          ? launchLabel
          : t("sessionManager.copyResumeCommand", {
              defaultValue: "复制恢复命令",
            })}
      </Button>
    );
  }

  const moreMenu = (
    <DropdownMenu>
      <HoverTip content={t("common.more", { defaultValue: "更多" })}>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            id="session-reader-more"
            aria-label={t("sessionManager.moreFor", {
              defaultValue: "{{title}} 的更多操作",
              title,
            })}
            className={cn(readerIconButton, "h-8 w-8")}
          >
            <MoreHorizontal className="h-4 w-4" strokeWidth={1.5} />
          </button>
        </DropdownMenuTrigger>
      </HoverTip>
      <DropdownMenuContent
        align="end"
        className="w-[230px] rounded-panel p-1 shadow-v7-md"
      >
        <DropdownMenuItem
          className={sessionMenuItemClass}
          disabled={!canCopyMarkdown}
          onSelect={onCopyMarkdown}
        >
          {t("sessionManager.copyAsMarkdown", {
            defaultValue: "复制整段为 Markdown",
          })}
        </DropdownMenuItem>
        <DropdownMenuCheckboxItem
          className={sessionMenuItemClass}
          checked={includeThinking}
          onCheckedChange={(value) => onIncludeThinkingChange(value === true)}
          onSelect={(event) => event.preventDefault()}
        >
          {rt("copyWithThinking")}
        </DropdownMenuCheckboxItem>
        <DropdownMenuItem
          className={sessionMenuItemClass}
          onSelect={() =>
            onCopy(
              session.sessionId,
              t("sessionManager.sessionIdCopied", {
                defaultValue: "已复制会话 ID",
              }),
            )
          }
        >
          {t("sessionManager.copySessionId", { defaultValue: "复制会话 ID" })}
        </DropdownMenuItem>
        <DropdownMenuItem
          className={sessionMenuItemClass}
          disabled={!session.sourcePath}
          onSelect={() =>
            session.sourcePath &&
            onCopy(
              session.sourcePath,
              t("sessionManager.sourcePathCopied", {
                defaultValue: "已复制源文件路径",
              }),
            )
          }
        >
          {t("sessionManager.copySourcePath", {
            defaultValue: "复制源文件路径",
          })}
        </DropdownMenuItem>
        <DropdownMenuItem className={sessionMenuItemClass} onSelect={onReload}>
          {t("sessionManager.reload", { defaultValue: "重新读取" })}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          className={cn(
            sessionMenuItemClass,
            deletable ? "text-danger-text" : "text-fg-3",
          )}
          disabled={!deletable}
          onSelect={onDelete}
        >
          {t("sessionManager.deleteEllipsis", { defaultValue: "删除…" })}
        </DropdownMenuItem>
        {!deletable && session.providerId === "mcode" && (
          <p className="px-2.5 pb-1.5 text-caption text-fg-2">
            {t("sessionManager.mcodeDeleteHint", {
              defaultValue: "请在 MiniMax Code 中删除",
            })}
          </p>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );

  return (
    <AppPageHeader
      variant="app"
      truncateTitle
      leading={
        <HoverTip content={t("sessionManager.back", { defaultValue: "返回" })}>
          <button
            type="button"
            id="session-reader-back"
            aria-label={t("sessionManager.backToList", {
              defaultValue: "返回会话列表",
            })}
            onClick={onBack}
            className={readerIconButton}
          >
            <ArrowLeft className="h-4 w-4" strokeWidth={1.5} />
          </button>
        </HoverTip>
      }
      icon={
        appId ? (
          <AppGlyph app={appId} size={16} badgeClassName="bg-app" />
        ) : undefined
      }
      title={<span title={title}>{title}</span>}
      titleExtra={
        <span className="sr-only">
          {t("sessionManager.sessionOf", {
            defaultValue: "{{app}} 的会话",
            app: appName,
          })}
        </span>
      }
      actions={
        <>
          <div className="flex shrink-0 items-center">
            <HoverTip
              content={t("sessionManager.prevShort", {
                defaultValue: "上一个",
              })}
            >
              <button
                type="button"
                aria-label={t("sessionManager.prevSession", {
                  defaultValue: "上一个会话",
                })}
                disabled={!hasPrev}
                onClick={onPrev}
                className={cn(readerIconButton, "h-8 w-8")}
              >
                <ChevronLeft className="h-4 w-4" strokeWidth={1.5} />
              </button>
            </HoverTip>
            <HoverTip
              content={t("sessionManager.nextShort", {
                defaultValue: "下一个",
              })}
            >
              <button
                type="button"
                aria-label={t("sessionManager.nextSession", {
                  defaultValue: "下一个会话",
                })}
                disabled={!hasNext}
                onClick={onNext}
                className={cn(readerIconButton, "h-8 w-8")}
              >
                <ChevronRight className="h-4 w-4" strokeWidth={1.5} />
              </button>
            </HoverTip>
          </div>
          {resume}
          {moreMenu}
        </>
      }
    />
  );
});

// ─── 信息栏 + 阅读工具 ────────────────────────────────────────────────────

/** 「10月2日 15:25 → 17:33」+ 时长 */
const formatRange = (created?: number, last?: number) => {
  if (!created && !last) return { range: "", duration: null };
  if (!created || !last || created === last) {
    return { range: formatShortDateTime(last ?? created), duration: null };
  }
  const sameDay =
    new Date(created).toDateString() === new Date(last).toDateString();
  return {
    range: `${formatShortDateTime(created)} → ${
      sameDay ? formatClock(last) : formatShortDateTime(last)
    }`,
    duration: last > created ? formatDuration(last - created) : null,
  };
};

export interface FindState {
  open: boolean;
  query: string;
  current: number;
  total: number;
}

export interface SessionReaderToolbarProps {
  session: SessionMeta;
  failed: boolean;
  /** 轮次索引（提问目录、次数统计） */
  turnIndex: TurnIndex[];
  messageCount: number;
  model?: string;
  progress: { loaded: number; total: number | null } | null;
  noResumeReason: string | null;
  filter: ReaderFilter;
  onFilterChange: (filter: ReaderFilter) => void;
  expandAll: boolean;
  onExpandAllChange: (value: boolean) => void;
  showInjected: boolean;
  onShowInjectedChange: (value: boolean) => void;
  onJumpToTurn: (item: TurnIndex) => void;
  find: FindState;
  findInputRef: RefObject<HTMLInputElement>;
  findButtonRef: RefObject<HTMLButtonElement>;
  onOpenFind: () => void;
  onCloseFind: () => void;
  onFindQueryChange: (query: string) => void;
  onStepFind: (delta: number) => void;
}

const Sep = () => (
  <span aria-hidden="true" className="text-fg-3">
    ·
  </span>
);

export const SessionReaderToolbar = memo(function SessionReaderToolbar({
  session,
  failed,
  turnIndex,
  messageCount,
  model,
  progress,
  noResumeReason,
  filter,
  onFilterChange,
  expandAll,
  onExpandAllChange,
  showInjected,
  onShowInjectedChange,
  onJumpToTurn,
  find,
  findInputRef,
  findButtonRef,
  onOpenFind,
  onCloseFind,
  onFindQueryChange,
  onStepFind,
}: SessionReaderToolbarProps) {
  const { t } = useTranslation();
  const rt = useReaderT();
  const { range, duration } = formatRange(
    session.createdAt,
    session.lastActiveAt,
  );
  const toolCount = turnIndex.reduce((sum, item) => sum + item.stepCount, 0);

  return (
    <div className="flex shrink-0 flex-col border-b border-border py-1.5 pe-4 ps-6">
      <div className="flex min-h-8 flex-wrap items-center gap-x-2 gap-y-1">
        <div className="flex min-w-0 flex-1 basis-72 items-center gap-1.5 overflow-hidden whitespace-nowrap text-caption tabular-nums text-fg-2">
          {session.projectDir ? (
            <PathChip path={session.projectDir} className="min-w-0 shrink" />
          ) : (
            <span>
              {t("sessionManager.unknownDirectory", {
                defaultValue: "未知目录",
              })}
            </span>
          )}
          {range && (
            <>
              <Sep />
              <span className="shrink-0">
                {range}
                {duration && <span className="text-fg-3"> ({duration})</span>}
              </span>
            </>
          )}
          {!failed && progress && (
            <>
              <Sep />
              <span role="status" className="shrink-0">
                {progress.total !== null
                  ? rt("loadingProgress", {
                      loaded: progress.loaded,
                      total: progress.total,
                    })
                  : t("sessionManager.loadingMessages", {
                      defaultValue: "加载会话内容中...",
                    })}
              </span>
            </>
          )}
          {!failed && !progress && messageCount > 0 && (
            <>
              <Sep />
              <span
                className="shrink-0"
                title={rt("turnsCount", {
                  turns: turnIndex.length,
                  messages: messageCount,
                })}
              >
                {rt("questionsCount", { count: turnIndex.length })}
                {toolCount > 0 && (
                  <>
                    {" · "}
                    {rt("toolsCount", { count: toolCount })}
                  </>
                )}
              </span>
            </>
          )}
          {model && (
            <>
              <Sep />
              <span className="min-w-0 truncate font-mono text-fg-3">
                {model}
              </span>
            </>
          )}
        </div>

        {!failed && (
          <div className="flex shrink-0 items-center gap-0.5">
            <SessionQuestionsMenu
              items={turnIndex}
              onItemClick={onJumpToTurn}
            />
            <label className="flex h-8 shrink-0 cursor-pointer items-center gap-2 rounded-control px-1.5 text-body text-fg-1">
              {t("sessionManager.onlyChat", { defaultValue: "只看对话" })}
              <Switch
                checked={filter === "conversation"}
                onCheckedChange={(value) =>
                  onFilterChange(value ? "conversation" : "all")
                }
              />
            </label>
            <label className="flex h-8 shrink-0 cursor-pointer items-center gap-2 rounded-control px-1.5 text-body text-fg-1">
              {rt("onlyChanges")}
              <Switch
                checked={filter === "changes"}
                onCheckedChange={(value) =>
                  onFilterChange(value ? "changes" : "all")
                }
              />
            </label>
            <DropdownMenu>
              <HoverTip content={rt("viewOptions")}>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    aria-label={rt("viewOptions")}
                    className={cn(readerIconButton, "h-8 w-8")}
                  >
                    <SlidersHorizontal className="h-4 w-4" strokeWidth={1.5} />
                  </button>
                </DropdownMenuTrigger>
              </HoverTip>
              <DropdownMenuContent
                align="end"
                className="w-[220px] rounded-panel p-1 shadow-v7-md"
              >
                <DropdownMenuCheckboxItem
                  className={sessionMenuItemClass}
                  checked={expandAll}
                  onCheckedChange={(value) => onExpandAllChange(value === true)}
                  onSelect={(event) => event.preventDefault()}
                >
                  {rt("expandAllTimelines")}
                </DropdownMenuCheckboxItem>
                <DropdownMenuCheckboxItem
                  className={sessionMenuItemClass}
                  checked={showInjected}
                  onCheckedChange={(value) =>
                    onShowInjectedChange(value === true)
                  }
                  onSelect={(event) => event.preventDefault()}
                >
                  {rt("injectedToggle")}
                </DropdownMenuCheckboxItem>
              </DropdownMenuContent>
            </DropdownMenu>
            {!find.open && (
              <HoverTip
                content={t("sessionManager.findShort", {
                  defaultValue: "查找",
                })}
              >
                <button
                  ref={findButtonRef}
                  type="button"
                  aria-label={t("sessionManager.findInSession", {
                    defaultValue: "在会话中查找",
                  })}
                  aria-expanded={false}
                  onClick={onOpenFind}
                  className={cn(readerIconButton, "h-8 w-8")}
                >
                  <Search className="h-4 w-4" strokeWidth={1.5} />
                </button>
              </HoverTip>
            )}
          </div>
        )}
        {find.open && !failed && (
          <div
            role="search"
            aria-label={t("sessionManager.findInSession", {
              defaultValue: "在会话中查找",
            })}
            className="flex shrink-0 items-center gap-0.5"
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.stopPropagation();
                onCloseFind();
              } else if (event.key === "Enter") {
                event.preventDefault();
                onStepFind(event.shiftKey ? -1 : 1);
              }
            }}
          >
            <div className="relative w-[148px]">
              <Search
                aria-hidden="true"
                className="pointer-events-none absolute start-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-fg-3"
                strokeWidth={1.5}
              />
              <input
                ref={findInputRef}
                type="text"
                value={find.query}
                onChange={(event) => onFindQueryChange(event.target.value)}
                aria-label={t("sessionManager.findLabel", {
                  defaultValue: "查找内容",
                })}
                placeholder={t("sessionManager.findShort", {
                  defaultValue: "查找",
                })}
                autoComplete="off"
                spellCheck={false}
                className={cn(fieldClass, "h-7 pe-2 ps-7 text-caption")}
              />
            </div>
            <span
              role="status"
              className="min-w-12 text-center text-caption tabular-nums text-fg-2"
            >
              {find.query.trim() ? `${find.current} / ${find.total}` : ""}
            </span>
            <button
              type="button"
              aria-label={t("sessionManager.findPrev", {
                defaultValue: "上一个匹配",
              })}
              disabled={!find.total}
              onClick={() => onStepFind(-1)}
              className={readerIconButton}
            >
              <ArrowUp className="h-[15px] w-[15px]" strokeWidth={1.5} />
            </button>
            <button
              type="button"
              aria-label={t("sessionManager.findNext", {
                defaultValue: "下一个匹配",
              })}
              disabled={!find.total}
              onClick={() => onStepFind(1)}
              className={readerIconButton}
            >
              <ArrowDown className="h-[15px] w-[15px]" strokeWidth={1.5} />
            </button>
            <button
              type="button"
              aria-label={t("sessionManager.findClose", {
                defaultValue: "关闭查找",
              })}
              onClick={onCloseFind}
              className={readerIconButton}
            >
              <X className="h-3.5 w-3.5" strokeWidth={1.5} />
            </button>
          </div>
        )}
      </div>

      {noResumeReason && (
        <p id="session-resume-note" className="m-0 mb-1 text-caption text-fg-2">
          {noResumeReason}
        </p>
      )}
    </div>
  );
});

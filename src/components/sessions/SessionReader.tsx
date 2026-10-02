import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
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
  SquareTerminal,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { AppGlyph } from "@/components/shell/AppGlyph";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { SessionMessage, SessionMeta } from "@/types";
import { SessionMessageItem, SessionToolGroup } from "./SessionMessageItem";
import { SessionQuestionsMenu, type SessionTocItem } from "./SessionToc";
import { sessionMenuItemClass } from "./SessionItem";
import {
  buildCdResumeCommand,
  buildSessionDisplayItems,
  canDeleteSession,
  countMatches,
  extractCodexPromptPreview,
  formatClock,
  formatSessionMessagePreview,
  formatSessionTitle,
  formatShortDateTime,
  formatToolNames,
  isChatItem,
  isSessionAppId,
  shortenHomePath,
  shouldHideCodexMessageFromToc,
  type SessionDisplayItem,
} from "./utils";

const iconButton =
  "inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 disabled:pointer-events-none disabled:opacity-40";

interface SessionReaderProps {
  session: SessionMeta;
  appName: string;
  messages: SessionMessage[];
  isLoading: boolean;
  error: unknown;
  /** 列表的搜索词：阅读页里照样高亮 */
  listQuery: string;
  /** 一键恢复用的终端名；不是 macOS 时为 null（只能复制命令） */
  launchTerminal: string | null;
  hasPrev: boolean;
  hasNext: boolean;
  onPrev: () => void;
  onNext: () => void;
  onBack: () => void;
  onLaunch: () => void;
  onCopy: (text: string, message: string) => void;
  onOpenTerminalSettings: () => void;
  onReload: () => void;
  onDelete: () => void;
}

export function SessionReader({
  session,
  appName,
  messages,
  isLoading,
  error,
  listQuery,
  launchTerminal,
  hasPrev,
  hasNext,
  onPrev,
  onNext,
  onBack,
  onLaunch,
  onCopy,
  onOpenTerminalSettings,
  onReload,
  onDelete,
}: SessionReaderProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const findInputRef = useRef<HTMLInputElement | null>(null);
  const findButtonRef = useRef<HTMLButtonElement | null>(null);
  const [onlyChat, setOnlyChat] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findIndex, setFindIndex] = useState(1);
  const [flashKey, setFlashKey] = useState<string | null>(null);
  const [atBottom, setAtBottom] = useState<boolean | null>(null);

  const title = formatSessionTitle(session);
  const appId = isSessionAppId(session.providerId) ? session.providerId : null;
  const isCodex = session.providerId === "codex";
  const command = session.resumeCommand;
  const failed = Boolean(error);
  const deletable = canDeleteSession(session);

  const allItems = useMemo(
    () => buildSessionDisplayItems(messages),
    [messages],
  );
  const items = useMemo(
    () => (onlyChat ? allItems.filter(isChatItem) : allItems),
    [allItems, onlyChat],
  );

  const questions = useMemo<SessionTocItem[]>(
    () =>
      messages
        .map((msg, index) => ({ msg, index }))
        .filter(({ msg }) => {
          if (msg.role.toLowerCase() !== "user") return false;
          return !(isCodex && shouldHideCodexMessageFromToc(msg.content));
        })
        .map(({ msg, index }) => ({
          index,
          preview: formatSessionMessagePreview(
            (isCodex ? extractCodexPromptPreview(msg.content) : msg.content)
              .replace(/\s+/g, " ")
              .trim(),
            40,
          ),
          ts: msg.ts,
        })),
    [isCodex, messages],
  );

  const roleLabel = useCallback(
    (role: string) => {
      const normalized = role.toLowerCase();
      if (normalized === "user")
        return t("sessionManager.you", { defaultValue: "你" });
      if (normalized === "assistant") return appName;
      if (normalized === "system")
        return t("sessionManager.roleSystem", { defaultValue: "系统" });
      if (normalized === "tool")
        return t("sessionManager.roleTool", { defaultValue: "工具" });
      return role;
    },
    [appName, t],
  );

  const itemText = (item: SessionDisplayItem) =>
    item.kind === "message"
      ? item.content
      : `${item.names.join(" ")}\n${item.output}`;

  // 会话内查找：每处匹配对应一个条目下标
  const findHits = useMemo(() => {
    const query = findQuery.trim();
    if (!findOpen || !query) return [] as number[];
    const hits: number[] = [];
    items.forEach((item, index) => {
      const count = countMatches(itemText(item), query);
      for (let i = 0; i < count; i += 1) hits.push(index);
    });
    return hits;
  }, [findOpen, findQuery, items]);
  const findTotal = findHits.length;
  const findCurrent = findTotal ? Math.min(findIndex, findTotal) : 0;
  const findActiveItem =
    findTotal && findCurrent ? items[findHits[findCurrent - 1]]?.key : null;
  const highlightQuery = findOpen && findQuery.trim() ? findQuery : listQuery;

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 120,
    getItemKey: (index) => items[index]?.key ?? index,
    overscan: 6,
    gap: 16,
    paddingStart: 16,
    paddingEnd: 64,
  });

  useEffect(() => {
    if (!findTotal || !findCurrent) return;
    virtualizer.scrollToIndex(findHits[findCurrent - 1], { align: "center" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [findCurrent, findTotal, findQuery]);

  const scrollToMessage = (messageIndex: number) => {
    const target = items.findIndex(
      (item) =>
        item.messageIndex === messageIndex ||
        (item.kind === "tools" && item.messageIndexes.includes(messageIndex)),
    );
    if (target < 0) return;
    virtualizer.scrollToIndex(target, { align: "start" });
    const key = items[target].key;
    setFlashKey(key);
    window.setTimeout(
      () => setFlashKey((current) => (current === key ? null : current)),
      2000,
    );
  };

  const scrollToLatest = () => {
    if (items.length === 0) return;
    virtualizer.scrollToIndex(items.length - 1, { align: "end" });
    setAtBottom(true);
  };

  const openFind = () => {
    setFindOpen(true);
    setFindIndex(1);
    window.setTimeout(() => findInputRef.current?.focus(), 0);
  };
  const closeFind = () => {
    setFindOpen(false);
    setFindQuery("");
    window.setTimeout(() => findButtonRef.current?.focus(), 0);
  };
  const stepFind = (delta: number) => {
    if (!findTotal) return;
    setFindIndex((current) => {
      const base = Math.min(current, findTotal);
      const next = base + delta;
      if (next < 1) return findTotal;
      if (next > findTotal) return 1;
      return next;
    });
  };

  const copyMarkdown = () => {
    const body = allItems
      .map((item) => {
        if (item.kind === "tools") {
          return item.names.length
            ? `> ${t("sessionManager.toolCalls", { defaultValue: "工具调用：" })}${formatToolNames(item.names)}`
            : "";
        }
        const time = item.ts ? ` ${formatClock(item.ts)}` : "";
        return `**${roleLabel(item.role)}**${time}\n\n${item.content}`;
      })
      .filter(Boolean)
      .join("\n\n---\n\n");
    onCopy(
      `# ${title}\n\n${body}\n`,
      t("sessionManager.markdownCopied", {
        defaultValue: "已复制整段对话（Markdown）",
      }),
    );
  };

  const range = (() => {
    const created = session.createdAt;
    const last = session.lastActiveAt;
    if (!created && !last) return "";
    if (!created || !last || created === last)
      return formatShortDateTime(last ?? created);
    const sameDay =
      new Date(created).toDateString() === new Date(last).toDateString();
    return `${formatShortDateTime(created)} → ${
      sameDay ? formatClock(last) : formatShortDateTime(last)
    }`;
  })();

  const noResumeReason = !command
    ? session.providerId === "openclaw"
      ? t("sessionManager.noResumeOpenclaw", {
          defaultValue: "OpenClaw 会话由网关管理，不能在终端恢复",
        })
      : session.providerId === "hermes"
        ? t("sessionManager.noResumeHermes", {
            defaultValue: "暂不支持从命令行恢复 Hermes 会话",
          })
        : t("sessionManager.noResumeCommand", {
            defaultValue: "此会话无法恢复",
          })
    : null;
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

  const showLatest =
    !failed &&
    items.length > 0 &&
    (atBottom === null ? items.length >= 5 : !atBottom);

  const renderResume = () => {
    if (command && launchLabel) {
      return (
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
    }

    // 不能一键恢复：有命令就复制；没有命令按钮禁用，下面写原因
    return (
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
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-col border-b border-border py-3 pe-4 ps-6">
        <div className="flex h-7 min-w-0 items-center gap-2">
          <button
            type="button"
            id="session-reader-back"
            aria-label={t("sessionManager.backToList", {
              defaultValue: "返回会话列表",
            })}
            title={t("sessionManager.back", { defaultValue: "返回" })}
            onClick={onBack}
            className={cn(iconButton, "-ms-1.5")}
          >
            <ArrowLeft className="h-4 w-4" strokeWidth={1.5} />
          </button>
          {appId && <AppGlyph app={appId} size={16} badgeClassName="bg-app" />}
          <h2
            title={title}
            className={cn(
              "m-0 min-w-0 flex-1 truncate text-section text-fg-1",
              !session.title && !session.projectDir && "font-mono",
            )}
          >
            {title}
          </h2>
          <span className="sr-only">
            {t("sessionManager.sessionOf", {
              defaultValue: "{{app}} 的会话",
              app: appName,
            })}
          </span>
          <button
            type="button"
            aria-label={t("sessionManager.prevSession", {
              defaultValue: "上一个会话",
            })}
            title={t("sessionManager.prevShort", { defaultValue: "上一个" })}
            disabled={!hasPrev}
            onClick={onPrev}
            className={iconButton}
          >
            <ChevronLeft className="h-4 w-4" strokeWidth={1.5} />
          </button>
          <button
            type="button"
            aria-label={t("sessionManager.nextSession", {
              defaultValue: "下一个会话",
            })}
            title={t("sessionManager.nextShort", { defaultValue: "下一个" })}
            disabled={!hasNext}
            onClick={onNext}
            className={iconButton}
          >
            <ChevronRight className="h-4 w-4" strokeWidth={1.5} />
          </button>
        </div>

        <div className="mt-1 flex min-w-0 items-center gap-1.5 whitespace-nowrap ps-[30px] text-caption tabular-nums text-fg-2">
          {session.projectDir ? (
            <button
              type="button"
              aria-label={t("sessionManager.copyPathAria", {
                defaultValue: "复制路径 {{path}}",
                path: session.projectDir,
              })}
              title={t("sessionManager.copyPathTitle", {
                defaultValue: "复制路径",
              })}
              onClick={() =>
                onCopy(
                  session.projectDir!,
                  t("sessionManager.pathCopied", {
                    defaultValue: "已复制路径 {{path}}",
                    path: session.projectDir,
                  }),
                )
              }
              className="min-w-0 truncate font-mono decoration-border-strong underline-offset-[3px] hover:text-fg-1 hover:underline"
            >
              {shortenHomePath(session.projectDir)}
            </button>
          ) : (
            <span>
              {t("sessionManager.unknownDirectory", {
                defaultValue: "未知目录",
              })}
            </span>
          )}
          {range && (
            <>
              <span aria-hidden="true">·</span>
              <span className="shrink-0">{range}</span>
            </>
          )}
          {!failed && !isLoading && (
            <>
              <span aria-hidden="true">·</span>
              <span className="shrink-0">
                {t("sessionManager.messageCount", {
                  defaultValue: "{{count}} 条消息",
                  count: messages.length,
                })}
              </span>
            </>
          )}
        </div>

        <div className="mt-2.5 flex min-h-8 flex-wrap items-center gap-2 ps-[30px]">
          {renderResume()}

          {!failed && (
            <>
              <label className="ms-1 flex h-8 shrink-0 cursor-pointer items-center gap-2 rounded-control px-1.5 text-body text-fg-1">
                {t("sessionManager.onlyChat", { defaultValue: "只看对话" })}
                <Switch checked={onlyChat} onCheckedChange={setOnlyChat} />
              </label>
              <SessionQuestionsMenu
                items={questions}
                onItemClick={scrollToMessage}
              />
              {!findOpen && (
                <button
                  ref={findButtonRef}
                  type="button"
                  aria-label={t("sessionManager.findInSession", {
                    defaultValue: "在会话中查找",
                  })}
                  aria-expanded={false}
                  title={t("sessionManager.findShort", {
                    defaultValue: "查找",
                  })}
                  onClick={openFind}
                  className={cn(iconButton, "h-8 w-8")}
                >
                  <Search className="h-4 w-4" strokeWidth={1.5} />
                </button>
              )}
            </>
          )}
          <div className="flex-1" />
          {findOpen && !failed && (
            <div
              role="search"
              aria-label={t("sessionManager.findInSession", {
                defaultValue: "在会话中查找",
              })}
              className="flex shrink-0 items-center gap-0.5"
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.stopPropagation();
                  closeFind();
                } else if (event.key === "Enter") {
                  event.preventDefault();
                  stepFind(event.shiftKey ? -1 : 1);
                }
              }}
            >
              <div className="relative w-[136px]">
                <Search
                  aria-hidden="true"
                  className="pointer-events-none absolute start-2 top-[7px] h-3.5 w-3.5 text-fg-2"
                  strokeWidth={1.5}
                />
                <input
                  ref={findInputRef}
                  type="text"
                  value={findQuery}
                  onChange={(event) => {
                    setFindQuery(event.target.value);
                    setFindIndex(1);
                  }}
                  aria-label={t("sessionManager.findLabel", {
                    defaultValue: "查找内容",
                  })}
                  placeholder={t("sessionManager.findShort", {
                    defaultValue: "查找",
                  })}
                  autoComplete="off"
                  spellCheck={false}
                  className="h-7 w-full rounded-[8px] border border-border-strong bg-surface pe-2 ps-7 text-caption text-fg-1 placeholder:text-fg-3 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                />
              </div>
              <span
                role="status"
                className="min-w-12 text-center text-caption tabular-nums text-fg-2"
              >
                {findQuery.trim() ? `${findCurrent} / ${findTotal}` : ""}
              </span>
              <button
                type="button"
                aria-label={t("sessionManager.findPrev", {
                  defaultValue: "上一个匹配",
                })}
                disabled={!findTotal}
                onClick={() => stepFind(-1)}
                className={iconButton}
              >
                <ArrowUp className="h-[15px] w-[15px]" strokeWidth={1.5} />
              </button>
              <button
                type="button"
                aria-label={t("sessionManager.findNext", {
                  defaultValue: "下一个匹配",
                })}
                disabled={!findTotal}
                onClick={() => stepFind(1)}
                className={iconButton}
              >
                <ArrowDown className="h-[15px] w-[15px]" strokeWidth={1.5} />
              </button>
              <button
                type="button"
                aria-label={t("sessionManager.findClose", {
                  defaultValue: "关闭查找",
                })}
                onClick={closeFind}
                className={iconButton}
              >
                <X className="h-3.5 w-3.5" strokeWidth={1.5} />
              </button>
            </div>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                id="session-reader-more"
                aria-label={t("sessionManager.moreFor", {
                  defaultValue: "{{title}} 的更多操作",
                  title,
                })}
                title={t("common.more", { defaultValue: "更多" })}
                className={cn(iconButton, "h-8 w-8")}
              >
                <MoreHorizontal className="h-4 w-4" strokeWidth={1.5} />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="end"
              className="w-[210px] rounded-panel p-1 shadow-v7-md"
            >
              <DropdownMenuItem
                className={sessionMenuItemClass}
                disabled={failed || messages.length === 0}
                onSelect={copyMarkdown}
              >
                {t("sessionManager.copyAsMarkdown", {
                  defaultValue: "复制整段为 Markdown",
                })}
              </DropdownMenuItem>
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
                {t("sessionManager.copySessionId", {
                  defaultValue: "复制会话 ID",
                })}
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
              <DropdownMenuItem
                className={sessionMenuItemClass}
                onSelect={onReload}
              >
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
                {t("sessionManager.deleteEllipsis", {
                  defaultValue: "删除…",
                })}
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
        </div>

        {noResumeReason && (
          <p
            id="session-resume-note"
            className="m-0 mt-2 ps-[30px] text-caption text-fg-2"
          >
            {noResumeReason}
          </p>
        )}
      </div>

      <div className="relative flex min-h-0 flex-1 flex-col">
        {failed ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 pb-10 text-center">
            <p className="m-0 text-section text-fg-1">
              {t("sessionManager.readFailed", {
                defaultValue: "无法读取这个会话",
              })}
            </p>
            <code className="max-w-[560px] whitespace-pre-wrap rounded-[8px] bg-subtle px-2.5 py-1.5 text-left font-mono text-caption text-fg-2 [overflow-wrap:anywhere]">
              {extractErrorMessage(error) || String(error)}
            </code>
            <div className="mt-2 flex gap-2">
              <Button variant="neutral" size="regular" onClick={onReload}>
                {t("common.retry", { defaultValue: "重试" })}
              </Button>
              {session.sourcePath && (
                <Button
                  variant="neutral"
                  size="regular"
                  onClick={() =>
                    onCopy(
                      session.sourcePath!,
                      t("sessionManager.sourcePathCopied", {
                        defaultValue: "已复制源文件路径",
                      }),
                    )
                  }
                >
                  {t("sessionManager.copySourcePath", {
                    defaultValue: "复制源文件路径",
                  })}
                </Button>
              )}
            </div>
          </div>
        ) : isLoading ? (
          <div className="flex flex-1 items-center justify-center text-body text-fg-2">
            {t("sessionManager.loadingMessages", {
              defaultValue: "加载会话内容中...",
            })}
          </div>
        ) : items.length === 0 ? (
          <div className="flex flex-1 items-center justify-center px-6 text-body text-fg-2">
            {t("sessionManager.emptySession", {
              defaultValue: "这个会话没有可显示的消息",
            })}
          </div>
        ) : (
          <div
            ref={scrollRef}
            onScroll={(event) => {
              const el = event.currentTarget;
              const bottom =
                el.scrollTop + el.clientHeight >= el.scrollHeight - 24;
              setAtBottom((current) => (current === bottom ? current : bottom));
            }}
            className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-6"
          >
            <div
              className="relative w-full"
              style={{ height: virtualizer.getTotalSize() }}
            >
              {virtualizer.getVirtualItems().map((row) => {
                const item = items[row.index];
                const active =
                  item.key === flashKey || item.key === findActiveItem;
                return (
                  <div
                    key={row.key}
                    data-index={row.index}
                    ref={virtualizer.measureElement}
                    className="absolute left-0 top-0 w-full"
                    style={{ transform: `translateY(${row.start}px)` }}
                  >
                    {item.kind === "message" ? (
                      <SessionMessageItem
                        item={item}
                        roleLabel={roleLabel(item.role)}
                        isActive={active}
                        searchQuery={highlightQuery}
                        onCopy={onCopy}
                      />
                    ) : (
                      <SessionToolGroup
                        item={item}
                        isActive={active}
                        searchQuery={highlightQuery}
                      />
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}
        {showLatest && (
          <Button
            variant="neutral"
            size="regular"
            aria-label={t("sessionManager.jumpLatest", {
              defaultValue: "跳到最新消息",
            })}
            onClick={scrollToLatest}
            className="absolute bottom-4 end-6 gap-1.5 pe-3 ps-2.5 shadow-v7-md"
          >
            <ArrowDown className="h-3.5 w-3.5" strokeWidth={2} />
            {t("sessionManager.latest", { defaultValue: "最新" })}
          </Button>
        )}
      </div>
    </div>
  );
}

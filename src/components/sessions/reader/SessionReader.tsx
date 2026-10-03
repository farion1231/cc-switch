import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { SessionTranscriptResult } from "@/lib/query/sessions";
import { toast } from "@/lib/toast";

import { sessionsApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import type { SessionMessage, SessionMeta, TurnIndex } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import { getBaseName } from "../utils";
import { getAgentReaderStyle } from "./agentStyles";
import { ReaderContext, type ReaderContextValue } from "./context";
import { transcriptToMarkdown, turnToMarkdown } from "./exportMarkdown";
import { useReaderT } from "./i18n";
import { SessionEventRow, SessionInjectedRow } from "./SessionEventRow";
import {
  SessionAgentAvatar,
  SessionAgentHeader,
  agentTurnTs,
} from "./SessionAgentAvatar";
import { SessionFinalReply } from "./SessionFinalReply";
import { SessionOutline } from "./SessionOutline";
import { SessionQuestion } from "./SessionQuestion";
import {
  SessionReaderHeader,
  SessionReaderToolbar,
} from "./SessionReaderHeader";
import { SessionStep } from "./SessionStep";
import { SessionTimeline } from "./SessionTimeline";
import { isFailureStep } from "./toolSummary";
import {
  buildTurnIndex,
  buildTurns,
  estimateRowHeight,
  findMatchRowIndex,
  findSearchHits,
  findTurnRowIndex,
  flattenRows,
  type ReaderFilter,
  type ReaderRow,
  type SearchMatch,
  type SessionTurn,
} from "./turns";

/** 「展开全部过程」偏好（与会话列表的分组方式一样存在 localStorage） */
const EXPAND_ALL_STORAGE_KEY = "cc-switch.sessionReader.expandTimelines";

const readExpandAll = () => {
  try {
    return window.localStorage.getItem(EXPAND_ALL_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
};

const EMPTY_OVERRIDES: ReadonlyMap<string, boolean> = new Map();

/** 会话标题：会话自带标题 → 第一条提问 → 项目目录名 → 会话 ID 前 8 位 */
export const readerTitle = (session: SessionMeta, turns: TurnIndex[]) =>
  session.title?.trim() ||
  turns[0]?.questionPreview ||
  getBaseName(session.projectDir) ||
  session.sessionId.slice(0, 8);

/** 最近一条带模型名的消息 */
const latestModel = (messages: SessionMessage[]) => {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const model = messages[i].meta?.model;
    if (model) return model;
  }
  return undefined;
};

/** 该轮的执行过程有没有摘要行（规则 5：只有 1 步且没失败时没有） */
const OUTLINE_STORAGE_KEY = "cc-switch.sessionReader.outline";

/** 会话标题转成文件名：去掉各系统不允许的字符，压掉空白，限制长度 */
const toFileName = (title: string) =>
  title
    .replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 60);

/** 属于 Agent 一侧（靠左、带头像列）的行；提问靠右，事件和分隔线居中 */
const AGENT_ROW_KINDS: ReadonlySet<ReaderRow["kind"]> = new Set([
  "timeline",
  "step",
  "final",
  "injected",
]);

const hasTimelineRow = (turn: SessionTurn) =>
  turn.steps.length > 1 ||
  (turn.steps.length === 1 && isFailureStep(turn.steps[0]));

/** 第 n 处命中（从 1 计）落在哪个 SearchMatch 上 */
const matchAt = (matches: SearchMatch[], position: number) => {
  let remaining = position;
  for (const match of matches) {
    if (remaining <= match.count) return match;
    remaining -= match.count;
  }
  return undefined;
};

const isEditableTarget = (target: EventTarget | null) => {
  const element = target as HTMLElement | null;
  if (!element || typeof element.closest !== "function") return false;
  return Boolean(
    element.closest(
      'input, textarea, select, [contenteditable="true"], [role="menu"], [role="dialog"], [role="alertdialog"]',
    ),
  );
};

export interface SessionReaderProps {
  session: SessionMeta;
  appName: string;
  transcript: SessionTranscriptResult;
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

/**
 * 会话阅读页：只做编排——页头、信息栏、阅读工具、
 * 流式加载状态、虚拟列表、查找与跳转。turn 结构与折叠规则在 turns.ts，各行由独立组件渲染。
 */
export function SessionReader({
  session,
  appName,
  transcript,
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
  const rt = useReaderT();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const findInputRef = useRef<HTMLInputElement>(null);
  const findButtonRef = useRef<HTMLButtonElement>(null);

  const [filter, setFilter] = useState<ReaderFilter>("all");
  const [expandAll, setExpandAll] = useState(readExpandAll);
  const [showInjected, setShowInjected] = useState(false);
  const [includeThinking, setIncludeThinking] = useState(false);
  const [turnOverrides, setTurnOverrides] = useState(EMPTY_OVERRIDES);
  const [stepOverrides, setStepOverrides] = useState(EMPTY_OVERRIDES);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findIndex, setFindIndex] = useState(1);
  const [flashKey, setFlashKey] = useState<string | null>(null);
  const [atBottom, setAtBottom] = useState<boolean | null>(null);
  // 右侧对话目录：默认打开，开关记在本地
  const [outlineOpen, setOutlineOpen] = useState(() => {
    try {
      return window.localStorage.getItem(OUTLINE_STORAGE_KEY) !== "false";
    } catch {
      return true;
    }
  });
  const toggleOutline = useCallback(() => {
    setOutlineOpen((open) => {
      try {
        window.localStorage.setItem(OUTLINE_STORAGE_KEY, String(!open));
      } catch {
        // 存不了就只在本次生效
      }
      return !open;
    });
  }, []);

  const style = useMemo(
    () => getAgentReaderStyle(session.providerId),
    [session.providerId],
  );
  const failed = transcript.isError;

  // 流式数据每到一批就重建 turn；交给低优先级渲染，滚动和点击不被打断
  const messages = useDeferredValue(transcript.messages);
  const turns = useMemo(
    () => buildTurns(messages, { style }),
    [messages, style],
  );
  const localIndex = useMemo(() => buildTurnIndex(turns), [turns]);
  const headerTurns = transcript.header?.turns;
  const turnIndex =
    headerTurns && headerTurns.length > 0 ? headerTurns : localIndex;
  const model = useMemo(() => latestModel(messages), [messages]);
  const title = readerTitle(session, turnIndex);

  const deferredFind = useDeferredValue(findOpen ? findQuery.trim() : "");
  const search = useMemo(
    () => findSearchHits(turns, deferredFind),
    [deferredFind, turns],
  );
  const rows = useMemo(
    () =>
      flattenRows(turns, {
        filter,
        expandAll,
        turnOverrides,
        stepOverrides,
        search,
        showInjected,
      }),
    [
      expandAll,
      filter,
      search,
      showInjected,
      stepOverrides,
      turnOverrides,
      turns,
    ],
  );

  // 左右布局：每轮 Agent 输出的第一行（折叠摘要 / 首个步骤 / 最终回复）左侧挂一次头像
  const agentStartKeys = useMemo(() => {
    const keys = new Set<string>();
    const seen = new Set<number>();
    for (const row of rows) {
      if (seen.has(row.turn) || !AGENT_ROW_KINDS.has(row.kind)) continue;
      seen.add(row.turn);
      keys.add(row.key);
    }
    return keys;
  }, [rows]);

  const findTotal = search?.total ?? 0;
  const findCurrent = findTotal ? Math.min(findIndex, findTotal) : 0;
  const activeMatch =
    search && findCurrent ? matchAt(search.matches, findCurrent) : undefined;
  const activeRowIndex = activeMatch
    ? findMatchRowIndex(rows, activeMatch)
    : -1;
  const activeKey = activeRowIndex >= 0 ? rows[activeRowIndex].key : null;
  const highlightQuery =
    findOpen && findQuery.trim() ? findQuery.trim() : listQuery;

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) =>
      rows[index] ? estimateRowHeight(rows[index]) : 32,
    getItemKey: (index) => rows[index]?.key ?? index,
    overscan: 8,
    paddingStart: 16,
    paddingEnd: 64,
  });

  // 查找：跳到当前命中所在行
  useEffect(() => {
    if (activeRowIndex < 0) return;
    virtualizer.scrollToIndex(activeRowIndex, { align: "center" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [findCurrent, deferredFind, activeRowIndex]);

  const flash = useCallback((key: string) => {
    setFlashKey(key);
    window.setTimeout(
      () => setFlashKey((current) => (current === key ? null : current)),
      2000,
    );
  }, []);

  const scrollToRow = useCallback(
    (index: number) => {
      if (index < 0) return;
      virtualizer.scrollToIndex(index, { align: "start" });
      flash(rows[index].key);
    },
    [flash, rows, virtualizer],
  );

  const jumpToQuestion = useCallback(
    (turn: SessionTurn) => {
      const index = rows.findIndex(
        (row) => row.turn === turn.index && row.kind === "question",
      );
      scrollToRow(index >= 0 ? index : findTurnRowIndex(rows, turns, turn.key));
    },
    [rows, scrollToRow, turns],
  );

  /** 目录里点 Agent：优先跳到最终回复，没有回复时跳到这一轮 Agent 输出的第一行 */
  const jumpToReply = useCallback(
    (turn: SessionTurn) => {
      let index = rows.findIndex(
        (row) => row.turn === turn.index && row.kind === "final",
      );
      if (index < 0) {
        index = rows.findIndex(
          (row) => row.turn === turn.index && AGENT_ROW_KINDS.has(row.kind),
        );
      }
      scrollToRow(index >= 0 ? index : findTurnRowIndex(rows, turns, turn.key));
    },
    [rows, scrollToRow, turns],
  );

  // j / k 在轮之间跳（‹ › 是会话之间）
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "j" && event.key !== "k") return;
      if (
        event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        event.defaultPrevented
      )
        return;
      if (isEditableTarget(event.target) || rows.length === 0) return;
      const start = virtualizer.range?.startIndex ?? 0;
      const current = rows[Math.min(start, rows.length - 1)]?.turn ?? 0;
      let target = -1;
      if (event.key === "j") {
        target = rows.findIndex(
          (row) => row.turn > current && row.kind !== "turn_divider",
        );
      } else {
        const previous = rows
          .slice(0, start)
          .reverse()
          .find((row) => row.turn < current);
        if (previous) {
          target = rows.findIndex(
            (row) => row.turn === previous.turn && row.kind !== "turn_divider",
          );
        }
      }
      if (target < 0) return;
      event.preventDefault();
      scrollToRow(target);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [rows, scrollToRow, virtualizer]);

  const toggleTurn = useCallback((key: string, expanded: boolean) => {
    setTurnOverrides((current) => new Map(current).set(key, !expanded));
  }, []);
  const toggleStep = useCallback((id: string, expanded: boolean) => {
    setStepOverrides((current) => new Map(current).set(id, !expanded));
  }, []);

  const handleExpandAll = useCallback((value: boolean) => {
    setExpandAll(value);
    // 切换「展开全部」时清掉逐轮的手动开合，免得部分轮不跟随
    setTurnOverrides(EMPTY_OVERRIDES);
    try {
      window.localStorage.setItem(EXPAND_ALL_STORAGE_KEY, value ? "1" : "0");
    } catch {
      // 隐私模式等写不了 localStorage：只在本次生效
    }
  }, []);

  const exportOptions = useMemo(
    () => ({
      style,
      appName,
      youLabel: t("sessionManager.you", { defaultValue: "你" }),
      projectDir: session.projectDir ?? undefined,
      t: rt,
      includeThinking,
    }),
    [appName, includeThinking, rt, session.projectDir, style, t],
  );

  const copyTurn = useCallback(
    (turnKey: string) => {
      const turn = turns.find((item) => item.key === turnKey);
      if (!turn) return;
      onCopy(turnToMarkdown(turn, exportOptions), rt("turnCopied"));
    },
    [exportOptions, onCopy, rt, turns],
  );

  const copyMarkdown = useCallback(() => {
    onCopy(
      transcriptToMarkdown(title, turns, exportOptions),
      t("sessionManager.markdownCopied", {
        defaultValue: "已复制整段对话（Markdown）",
      }),
    );
  }, [exportOptions, onCopy, t, title, turns]);

  const exportMarkdownFile = useCallback(async () => {
    const name = `${toFileName(title) || session.sessionId}.md`;
    try {
      const path = await sessionsApi.exportMarkdown(
        name,
        transcriptToMarkdown(title, turns, exportOptions),
      );
      if (path) toast.success(rt("exported", { path }));
    } catch (error) {
      toast.error(rt("exportFailed", { error: extractErrorMessage(error) }));
    }
  }, [exportOptions, rt, session.sessionId, title, turns]);

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
      const next = Math.min(current, findTotal) + delta;
      if (next < 1) return findTotal;
      if (next > findTotal) return 1;
      return next;
    });
  };

  const scrollToLatest = () => {
    if (rows.length === 0) return;
    virtualizer.scrollToIndex(rows.length - 1, { align: "end" });
    setAtBottom(true);
  };

  const context = useMemo<ReaderContextValue>(
    () => ({
      providerId: session.providerId,
      sourcePath: session.sourcePath ?? undefined,
      projectDir: session.projectDir ?? undefined,
      style,
      appName,
      searchQuery: highlightQuery || undefined,
      onCopy,
    }),
    [
      appName,
      highlightQuery,
      onCopy,
      session.projectDir,
      session.providerId,
      session.sourcePath,
      style,
    ],
  );

  const noResumeReason = !session.resumeCommand
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

  const loadingFirst =
    !failed &&
    messages.length === 0 &&
    (transcript.isLoading || transcript.isStreaming);
  const progress =
    transcript.isStreaming || messages !== transcript.messages
      ? transcript.progress
      : null;
  // 正文顶部停在哪一轮：目录据此高亮（虚拟列表滚动时会重渲染，这里直接算）
  const scrollOffset = virtualizer.scrollOffset ?? 0;
  const firstVisible = virtualizer
    .getVirtualItems()
    .find((item) => item.end > scrollOffset + 8);
  // 滚到底时最后几轮到不了顶部，直接算作最后一轮
  const activeTurn =
    atBottom && rows.length > 0
      ? rows[rows.length - 1].turn
      : firstVisible
        ? (rows[firstVisible.index]?.turn ?? 0)
        : 0;

  const showLatest =
    !failed &&
    rows.length > 0 &&
    (atBottom === null ? rows.length >= 8 : !atBottom);

  /** Agent 输出一侧：左边 40px 头像列，右边留白和人的气泡错开 */
  const renderAgentRow = (row: ReaderRow, content: ReactNode): ReactNode => {
    const start = agentStartKeys.has(row.key);
    const turn = turns[row.turn];
    return (
      <div className="relative pe-10 ps-10">
        {start && (
          <>
            <span className="absolute start-0 top-0">
              <SessionAgentAvatar />
            </span>
            {/* Agent 名 · 时间 · 模型 与头像同一行；最终回复不再重复这一行 */}
            <SessionAgentHeader
              ts={agentTurnTs(turn, messages)}
              model={turn.final?.model}
            />
          </>
        )}
        {content}
      </div>
    );
  };

  const renderRow = (row: ReaderRow): ReactNode => {
    const content = renderRowContent(row);
    return AGENT_ROW_KINDS.has(row.kind) && content
      ? renderAgentRow(row, content)
      : content;
  };

  const renderRowContent = (row: ReaderRow): ReactNode => {
    const turn = turns[row.turn];
    switch (row.kind) {
      case "question":
        // 人的输入靠右，气泡最宽 85%，左边留出 Agent 侧的空间
        return turn.question ? (
          <div className="flex justify-end pb-3 ps-16">
            <div className="min-w-0 max-w-[72%]">
              <SessionQuestion
                question={turn.question}
                forceExpanded={row.forceExpanded}
              />
            </div>
          </div>
        ) : null;
      case "timeline":
        return (
          <div className="py-0.5">
            <SessionTimeline
              turnKey={turn.key}
              summary={row.summary}
              stepTotal={turn.steps.length}
              expanded={row.expanded}
              hiddenFailures={row.hiddenFailures}
              noFinal={!turn.final && !turn.aborted}
              onToggle={toggleTurn}
              onCopyTurn={copyTurn}
            />
          </div>
        );
      case "step": {
        const inTimeline = filter === "all" && hasTimelineRow(turn);
        return (
          <div
            className={cn(
              inTimeline && "ms-[13px] border-s border-border ps-2",
              row.depth === 1 && "ps-7",
            )}
          >
            <SessionStep
              step={row.step}
              expanded={row.expanded}
              previewLines={row.previewLines}
              onToggle={toggleStep}
            />
          </div>
        );
      }
      case "final":
        return turn.final ? (
          <div className="pb-1 pt-2">
            <SessionFinalReply
              final={turn.final}
              forceExpanded={row.forceExpanded}
            />
          </div>
        ) : null;
      case "event":
        return <SessionEventRow block={row.block} body={row.body} />;
      case "injected":
        return messages[row.messageIndex] ? (
          <SessionInjectedRow message={messages[row.messageIndex]} />
        ) : null;
      case "turn_divider":
        return (
          <div aria-hidden className="py-4">
            <div className="h-px bg-border" />
          </div>
        );
    }
  };

  let body: ReactNode;
  if (failed) {
    body = (
      <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 pb-10 text-center">
        <p className="m-0 text-section text-fg-1">
          {t("sessionManager.readFailed", { defaultValue: "无法读取这个会话" })}
        </p>
        <code className="max-w-[560px] whitespace-pre-wrap rounded-[8px] bg-subtle px-2.5 py-1.5 text-left font-mono text-caption text-fg-2 [overflow-wrap:anywhere]">
          {extractErrorMessage(transcript.error) || String(transcript.error)}
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
    );
  } else if (loadingFirst) {
    body = (
      <div
        role="status"
        className="flex flex-1 items-center justify-center text-body text-fg-2"
      >
        {t("sessionManager.loadingMessages", {
          defaultValue: "加载会话内容中...",
        })}
      </div>
    );
  } else if (rows.length === 0) {
    body = (
      <div className="flex flex-1 items-center justify-center px-6 text-body text-fg-2">
        {filter === "changes" && messages.length > 0
          ? rt("noChanges")
          : t("sessionManager.emptySession", {
              defaultValue: "这个会话没有可显示的消息",
            })}
      </div>
    );
  } else {
    body = (
      <div
        ref={scrollRef}
        role="region"
        aria-label={rt("conversationRegion")}
        onScroll={(event) => {
          const el = event.currentTarget;
          const bottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 24;
          setAtBottom((current) => (current === bottom ? current : bottom));
        }}
        className="min-h-0 flex-1 overflow-y-auto scroll-stable overscroll-contain px-6"
      >
        <div
          className="relative w-full"
          style={{ height: virtualizer.getTotalSize() }}
        >
          {virtualizer.getVirtualItems().map((item) => {
            const row = rows[item.index];
            if (!row) return null;
            const active = row.key === activeKey || row.key === flashKey;
            return (
              <div
                key={item.key}
                data-index={item.index}
                data-row-kind={row.kind}
                ref={virtualizer.measureElement}
                className="absolute left-0 top-0 w-full"
                style={{ transform: `translateY(${item.start}px)` }}
              >
                <div
                  className={cn(
                    "mx-auto w-full max-w-[820px] rounded-panel transition-shadow motion-reduce:transition-none",
                    active && "ring-2 ring-ring ring-offset-2 ring-offset-app",
                  )}
                >
                  {renderRow(row)}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    );
  }

  return (
    <ReaderContext.Provider value={context}>
      <div
        data-reader-style={style.id}
        className="flex min-h-0 flex-1 flex-col"
        style={
          { "--reader-accent": `var(${style.accentVar})` } as CSSProperties
        }
      >
        <SessionReaderHeader
          session={session}
          title={title}
          appName={appName}
          launchTerminal={launchTerminal}
          hasPrev={hasPrev}
          hasNext={hasNext}
          canCopyMarkdown={!failed && turns.length > 0}
          includeThinking={includeThinking}
          onIncludeThinkingChange={setIncludeThinking}
          onPrev={onPrev}
          onNext={onNext}
          onBack={onBack}
          onLaunch={onLaunch}
          onCopy={onCopy}
          onCopyMarkdown={copyMarkdown}
          onExportMarkdown={exportMarkdownFile}
          onOpenTerminalSettings={onOpenTerminalSettings}
          onReload={onReload}
          onDelete={onDelete}
        />
        <SessionReaderToolbar
          session={session}
          failed={failed}
          turnIndex={turnIndex}
          messageCount={messages.length}
          model={model}
          progress={progress}
          noResumeReason={noResumeReason}
          filter={filter}
          onFilterChange={setFilter}
          expandAll={expandAll}
          onExpandAllChange={handleExpandAll}
          showInjected={showInjected}
          onShowInjectedChange={setShowInjected}
          outlineOpen={outlineOpen}
          onToggleOutline={toggleOutline}
          find={{
            open: findOpen,
            query: findQuery,
            current: findCurrent,
            total: findTotal,
          }}
          findInputRef={findInputRef}
          findButtonRef={findButtonRef}
          onOpenFind={openFind}
          onCloseFind={closeFind}
          onFindQueryChange={(value) => {
            setFindQuery(value);
            setFindIndex(1);
          }}
          onStepFind={stepFind}
          onExportMarkdown={exportMarkdownFile}
        />
        <div className="flex min-h-0 flex-1">
          <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
            {body}
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
          {outlineOpen && !failed && turns.length > 0 && (
            <SessionOutline
              turns={turns}
              activeTurn={activeTurn}
              onJumpQuestion={jumpToQuestion}
              onJumpReply={jumpToReply}
            />
          )}
        </div>
      </div>
    </ReaderContext.Provider>
  );
}

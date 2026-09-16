import { useCallback, useEffect, useRef, useState } from "react";
import {
  Bold,
  Italic,
  Code2,
  List,
  Heading1,
  Quote,
  Wand2,
  SendHorizonal,
  History,
  Trash2,
  Loader2,
  ChevronDown,
  ChevronUp,
  ListOrdered,
  Play,
  Pause,
  SkipForward,
  Settings2,
  X,
  Clock,
  Pencil,
} from "lucide-react";
import { toast } from "sonner";

import { terminalApi } from "@/lib/api/terminal";
import { terminalQueue } from "@/lib/terminalQueue";
import { cn } from "@/lib/utils";

const HISTORY_KEY = "cc-switch-terminal-prompt-history";
const HISTORY_LIMIT = 50;
/** 草稿按终端隔离：切换终端不会互相覆盖 */
const draftKey = (instanceId: string) =>
  `cc-switch-terminal-draft-${instanceId}`;

interface PromptHistoryEntry {
  text: string;
  at: number;
}

function loadHistory(): PromptHistoryEntry[] {
  try {
    const raw = window.localStorage.getItem(HISTORY_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter(
        (item): item is PromptHistoryEntry =>
          typeof item === "object" &&
          item !== null &&
          typeof (item as PromptHistoryEntry).text === "string",
      )
      .slice(0, HISTORY_LIMIT);
  } catch {
    return [];
  }
}

function saveHistory(entries: PromptHistoryEntry[]) {
  try {
    window.localStorage.setItem(
      HISTORY_KEY,
      JSON.stringify(entries.slice(0, HISTORY_LIMIT)),
    );
  } catch {
    // 忽略存储失败
  }
}

export interface TerminalPromptProps {
  /** 当前终端实例 id（草稿与队列都按它隔离） */
  instanceId: string;
  /** 获取当前内嵌会话 pty id（无会话时为 null） */
  getPtyId: () => number | null;
}

/** 订阅队列 store（单例，跨组件共享）。 */
function useQueueState(instanceId: string) {
  const [, bump] = useState(0);
  useEffect(() => terminalQueue.subscribe(() => bump((n) => n + 1)), []);
  return terminalQueue.get(instanceId);
}

/** 插入 markdown 标记到选区。 */
function wrapSelection(
  textarea: HTMLTextAreaElement,
  prefix: string,
  suffix = prefix,
) {
  const { selectionStart, selectionEnd, value } = textarea;
  const selected = value.slice(selectionStart, selectionEnd) || "文本";
  const next =
    value.slice(0, selectionStart) +
    prefix +
    selected +
    suffix +
    value.slice(selectionEnd);
  textarea.value = next;
  textarea.focus();
  textarea.selectionStart = selectionStart + prefix.length;
  textarea.selectionEnd = selectionStart + prefix.length + selected.length;
  textarea.dispatchEvent(new Event("input", { bubbles: true }));
}

/**
 * 终端底部输入面板：富文本（markdown 快捷插入）+ AI 润色 + 发送/排队到内嵌终端。
 * - 草稿按终端实例保存（切换终端互不影响，重启后仍在）；
 * - 队列按终端实例隔离，跟随当前终端，上一条执行完成后自动发下一条。
 */
export function TerminalPrompt({ instanceId, getPtyId }: TerminalPromptProps) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState(() => {
    try {
      return window.localStorage.getItem(draftKey(instanceId)) ?? "";
    } catch {
      return "";
    }
  });
  const [polishing, setPolishing] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [history, setHistory] = useState<PromptHistoryEntry[]>(() =>
    loadHistory(),
  );
  /** 队列面板展开状态 */
  const [queueOpen, setQueueOpen] = useState(false);
  /** 队列参数（静默时长 / 首字节超时）编辑 */
  const [queueSettingsOpen, setQueueSettingsOpen] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  const queue = useQueueState(instanceId);
  const settings = terminalQueue.getSettings();

  // 草稿持久化（防抖：输入过程中不必每次写盘）
  useEffect(() => {
    const timer = window.setTimeout(() => {
      try {
        const key = draftKey(instanceId);
        if (text) window.localStorage.setItem(key, text);
        else window.localStorage.removeItem(key);
      } catch {
        // 忽略
      }
    }, 400);
    return () => window.clearTimeout(timer);
  }, [instanceId, text]);

  const persistEntry = useCallback((entry: PromptHistoryEntry) => {
    setHistory((prev) => {
      const next = [
        entry,
        ...prev.filter((item) => item.text !== entry.text),
      ].slice(0, HISTORY_LIMIT);
      saveHistory(next);
      return next;
    });
  }, []);

  const clearHistory = useCallback(() => {
    setHistory([]);
    saveHistory([]);
    toast.success("发送记录已清空");
  }, []);

  /** 直接写入终端（不进队列）。 */
  const sendNow = useCallback(
    async (content: string) => {
      const ptyId = getPtyId();
      if (ptyId === null) {
        toast.error("当前终端会话不可用，请先启动终端");
        return false;
      }
      // 换行统一成 \r\n：cmd/ConPTY 只认回车执行，裸 \n 会打断当前行，
      // 表现为多行命令被截断（只有第一行生效、其余错位）。
      const payload = content.replace(/\r\n|\r|\n/g, "\r\n");
      try {
        await terminalApi.writeEmbedded(ptyId, payload);
        await new Promise((resolve) => setTimeout(resolve, 150));
        await terminalApi.writeEmbedded(ptyId, "\r");
      } catch (error) {
        toast.error(`发送失败：${String(error)}`);
        return false;
      }
      return true;
    },
    [getPtyId],
  );

  /** 入队（并自动开始派发）。 */
  const enqueue = useCallback(
    (content: string) => {
      if (!terminalQueue.enqueue(instanceId, content)) return;
      terminalQueue.start(instanceId);
      setQueueOpen(true);
    },
    [instanceId],
  );

  const handleSend = useCallback(() => {
    const content = text.trim();
    if (!content) return;
    void (async () => {
      if (settings.queueMode) {
        enqueue(content);
        setText("");
        return;
      }
      const ok = await sendNow(content);
      if (!ok) return;
      persistEntry({ text: content, at: Date.now() });
      setText("");
    })();
  }, [enqueue, persistEntry, sendNow, settings.queueMode, text]);

  const handlePolish = useCallback(async () => {
    const content = text.trim();
    if (!content) {
      toast.error("请先输入需要润色的内容");
      return;
    }
    setPolishing(true);
    try {
      const polished = await terminalApi.polishPrompt("claude", content);
      setText(polished);
      toast.success("润色完成");
    } catch (error) {
      toast.error(`润色失败：${String(error)}`);
    } finally {
      setPolishing(false);
    }
  }, [text]);

  const insertMark = useCallback((prefix: string, suffix = prefix) => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    wrapSelection(textarea, prefix, suffix);
    setText(textarea.value);
  }, []);

  useEffect(() => {
    if (!open) setHistoryOpen(false);
  }, [open]);

  const pending = queue.items.filter(
    (item) => item.status === "pending" || item.status === "sending",
  ).length;

  /** 队列面板（展开态展示，跟随终端） */
  const renderQueuePanel = () => (
    <div className="border-b border-border bg-muted/30">
      <div className="flex items-center gap-1 px-2 py-1">
        <ListOrdered className="h-3.5 w-3.5 text-muted-foreground" />
        <span className="text-[11px] font-medium text-foreground/80">
          发送队列
        </span>
        <span className="rounded bg-muted px-1 text-[10px] text-muted-foreground">
          {pending} 待发 / {queue.items.length} 总计
        </span>
        {queue.running ? (
          <span className="flex items-center gap-1 text-[10px] text-emerald-600">
            <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500" />
            {queue.phase === "awaitStart"
              ? "等待响应…"
              : queue.phase === "awaitFinish"
                ? "执行中…"
                : "派发中…"}
          </span>
        ) : (
          <span className="text-[10px] text-muted-foreground">已暂停</span>
        )}
        <div className="flex-1" />
        {queue.running ? (
          <button
            type="button"
            onClick={() => terminalQueue.pause(instanceId)}
            title="暂停派发（已发的这条会跑完）"
            className="flex h-6 items-center gap-1 rounded px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <Pause className="h-3 w-3" />
            暂停
          </button>
        ) : (
          <button
            type="button"
            onClick={() => terminalQueue.start(instanceId)}
            disabled={pending === 0}
            title="开始派发"
            className="flex h-6 items-center gap-1 rounded px-1.5 text-[11px] text-emerald-600 transition-colors hover:bg-muted disabled:opacity-40"
          >
            <Play className="h-3 w-3" />
            继续
          </button>
        )}
        <button
          type="button"
          onClick={() => terminalQueue.skip(instanceId)}
          disabled={queue.phase === "idle"}
          title="跳过当前这条，立即发下一条"
          className="flex h-6 items-center gap-1 rounded px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
        >
          <SkipForward className="h-3 w-3" />
          跳过
        </button>
        <button
          type="button"
          onClick={() => setQueueSettingsOpen((prev) => !prev)}
          title="队列参数"
          className="flex h-6 w-6 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <Settings2 className="h-3 w-3" />
        </button>
        <button
          type="button"
          onClick={() => terminalQueue.clear(instanceId)}
          disabled={queue.items.length === 0}
          title="清空队列"
          className="flex h-6 items-center gap-1 rounded px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-red-500 disabled:opacity-40"
        >
          <Trash2 className="h-3 w-3" />
          清空
        </button>
      </div>

      {queueSettingsOpen && (
        <div className="flex flex-wrap items-center gap-3 border-b border-border px-2 py-1.5">
          <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <Clock className="h-3 w-3" />
            静默判定
            <input
              type="number"
              min={1}
              max={300}
              value={Math.round(settings.idleMs / 1000)}
              onChange={(event) =>
                terminalQueue.updateSettings({
                  idleMs: Math.max(1000, Number(event.target.value) * 1000),
                })
              }
              className="w-16 rounded border border-border bg-background px-1.5 py-0.5 text-[11px] text-foreground"
            />
            秒
          </label>
          <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            首响超时
            <input
              type="number"
              min={1}
              max={600}
              value={Math.round(settings.startTimeoutMs / 1000)}
              onChange={(event) =>
                terminalQueue.updateSettings({
                  startTimeoutMs: Math.max(
                    1000,
                    Number(event.target.value) * 1000,
                  ),
                })
              }
              className="w-16 rounded border border-border bg-background px-1.5 py-0.5 text-[11px] text-foreground"
            />
            秒
          </label>
          <span className="text-[10px] text-muted-foreground/80">
            连续静默达到设定秒数即视为上一条执行完成，自动发送下一条
          </span>
        </div>
      )}

      <div className="max-h-40 overflow-y-auto px-2 pb-1.5">
        {queue.items.length === 0 ? (
          <p className="py-3 text-center text-[11px] text-muted-foreground">
            队列为空。开启「排队发送」后，点击发送会先入队，逐条执行。
          </p>
        ) : (
          <ol className="space-y-0.5">
            {queue.items.map((item, index) => (
              <li
                key={item.id}
                className="flex items-start gap-1.5 rounded px-1 py-1 text-[11px] hover:bg-muted/50"
              >
                <span className="w-4 shrink-0 text-right text-muted-foreground">
                  {index + 1}
                </span>
                <span
                  className={
                    item.status === "sending"
                      ? "shrink-0 rounded bg-amber-500/15 px-1 text-[9px] text-amber-600"
                      : item.status === "done"
                        ? "shrink-0 rounded bg-emerald-500/15 px-1 text-[9px] text-emerald-600"
                        : item.status === "error"
                          ? "shrink-0 rounded bg-red-500/15 px-1 text-[9px] text-red-500"
                          : "shrink-0 rounded bg-muted px-1 text-[9px] text-muted-foreground"
                  }
                  title={item.error ?? undefined}
                >
                  {item.status === "sending"
                    ? "执行中"
                    : item.status === "done"
                      ? "完成"
                      : item.status === "error"
                        ? "失败"
                        : "待发"}
                </span>
                <span className="min-w-0 flex-1 whitespace-pre-wrap break-all text-foreground/80">
                  {item.text}
                </span>
                {item.status === "pending" && (
                  <>
                    <button
                      type="button"
                      onClick={() => {
                        setText(item.text);
                        terminalQueue.remove(instanceId, item.id);
                        textareaRef.current?.focus();
                      }}
                      title="取回到输入框"
                      className="shrink-0 rounded p-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                    >
                      <Pencil className="h-3 w-3" />
                    </button>
                    <button
                      type="button"
                      onClick={() => terminalQueue.move(instanceId, item.id, -1)}
                      disabled={index === 0}
                      title="上移"
                      className="shrink-0 rounded p-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-30"
                    >
                      <ChevronUp className="h-3 w-3" />
                    </button>
                    <button
                      type="button"
                      onClick={() => terminalQueue.move(instanceId, item.id, 1)}
                      disabled={index === queue.items.length - 1}
                      title="下移"
                      className="shrink-0 rounded p-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-30"
                    >
                      <ChevronDown className="h-3 w-3" />
                    </button>
                  </>
                )}
                <button
                  type="button"
                  onClick={() => terminalQueue.remove(instanceId, item.id)}
                  title="移除"
                  className="shrink-0 rounded p-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-red-500"
                >
                  <X className="h-3 w-3" />
                </button>
              </li>
            ))}
          </ol>
        )}
      </div>
    </div>
  );

  if (!open) {
    return (
      <div className="flex shrink-0 flex-col border-t border-border">
        {queueOpen && renderQueuePanel()}
        <div className="flex items-center gap-1.5 px-2 py-1.5">
          <button
            type="button"
            onClick={() => setOpen(true)}
            title="展开输入面板（富文本 / AI 润色）"
            className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <ChevronDown className="h-3.5 w-3.5" />
          </button>
          <input
            value={text}
            onChange={(event) => setText(event.target.value)}
            onKeyDown={(event) => {
              // 统一 Ctrl/⌘+Enter 发送（与展开态一致），避免回车误发
              if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
                event.preventDefault();
                handleSend();
              }
            }}
            placeholder="输入要发送到终端的内容（Ctrl/⌘+Enter 发送）"
            className="min-w-0 flex-1 rounded-md border border-border bg-background px-2.5 py-1 text-xs text-foreground placeholder:text-muted-foreground focus:border-foreground/40 focus:outline-none"
          />
          <button
            type="button"
            onClick={() => setQueueOpen((prev) => !prev)}
            title="发送队列（按终端隔离）"
            className={cn(
              "flex h-6 shrink-0 items-center gap-1 rounded-md border px-1.5 text-[11px] transition-colors",
              pending > 0
                ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-600"
                : "border-transparent text-muted-foreground hover:bg-muted hover:text-foreground",
            )}
          >
            <ListOrdered className="h-3 w-3" />
            {pending > 0 ? pending : ""}
          </button>
          <button
            type="button"
            onClick={handleSend}
            title={
              settings.queueMode
                ? "加入队列并按顺序发送（Ctrl/⌘ + Enter）"
                : "立即发送到终端（Ctrl/⌘ + Enter）"
            }
            className="flex h-6 shrink-0 items-center gap-1 rounded-md bg-emerald-600/90 px-2.5 text-xs font-medium text-white transition-colors hover:bg-emerald-500 disabled:opacity-50"
            disabled={!text.trim()}
          >
            <SendHorizonal className="h-3 w-3" />
            {settings.queueMode ? "入队" : "发送"}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex shrink-0 flex-col border-t border-border bg-background/60">
      {(queueOpen || queue.items.length > 0) && renderQueuePanel()}
      {/* 工具栏 */}
      <div className="flex items-center gap-0.5 px-2 pt-1.5">
        {[
          { icon: Bold, title: "加粗", mark: "**" },
          { icon: Italic, title: "斜体", mark: "*" },
          { icon: Code2, title: "行内代码", mark: "`" },
          { icon: List, title: "列表", mark: "- ", suffix: "" },
          { icon: Heading1, title: "标题", mark: "# ", suffix: "" },
          { icon: Quote, title: "引用", mark: "> ", suffix: "" },
        ].map(({ icon: Icon, title, mark, suffix }) => (
          <button
            key={title}
            type="button"
            title={title}
            onClick={() => insertMark(mark, suffix ?? mark)}
            className="flex h-6 w-6 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <Icon className="h-3.5 w-3.5" />
          </button>
        ))}
        <div className="mx-1 h-4 w-px bg-border" />
        <button
          type="button"
          onClick={() => void handlePolish()}
          disabled={polishing}
          title="AI 润色（经本地代理调用当前供应商）"
          className="flex h-6 items-center gap-1 rounded px-1.5 text-[11px] text-violet-600 transition-colors hover:bg-muted disabled:opacity-50 dark:text-violet-300"
        >
          {polishing ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Wand2 className="h-3.5 w-3.5" />
          )}
          AI 润色
        </button>
        <div className="mx-1 h-4 w-px bg-border" />
        <label
          className="flex h-6 cursor-pointer items-center gap-1 rounded px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          title="开启后：发送的内容进入队列，上一条执行完成再发下一条"
        >
          <input
            type="checkbox"
            checked={settings.queueMode}
            onChange={(event) =>
              terminalQueue.updateSettings({ queueMode: event.target.checked })
            }
            className="h-3 w-3 accent-emerald-600"
          />
          排队发送
        </label>
        <button
          type="button"
          onClick={() => setQueueOpen((prev) => !prev)}
          title="展开/收起发送队列"
          className={cn(
            "flex h-6 items-center gap-1 rounded px-1.5 text-[11px] transition-colors",
            pending > 0
              ? "text-emerald-600 hover:bg-muted"
              : "text-muted-foreground hover:bg-muted hover:text-foreground",
          )}
        >
          <ListOrdered className="h-3 w-3" />
          队列 {pending > 0 ? `(${pending})` : ""}
        </button>
        <div className="flex-1" />
        <div className="relative">
          <button
            type="button"
            onClick={() => setHistoryOpen((prev) => !prev)}
            title="发送记录"
            className="flex h-6 items-center gap-1 rounded px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <History className="h-3.5 w-3.5" />
            记录 {history.length > 0 ? `(${history.length})` : ""}
            <ChevronDown className="h-3 w-3" />
          </button>
          {historyOpen && (
            <div className="absolute bottom-full right-0 z-20 mb-1 w-80 overflow-hidden rounded-lg border border-border bg-popover shadow-xl">
              <div className="flex items-center justify-between border-b border-border px-2 py-1.5">
                <span className="text-[10px] text-muted-foreground">
                  发送记录（点击还原）
                </span>
                <button
                  type="button"
                  onClick={clearHistory}
                  title="清空记录"
                  className="flex h-5 items-center gap-0.5 rounded px-1 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-red-500"
                >
                  <Trash2 className="h-3 w-3" />
                  清空
                </button>
              </div>
              <div className="max-h-48 overflow-y-auto">
                {history.length === 0 ? (
                  <p className="px-3 py-4 text-center text-[11px] text-muted-foreground">
                    暂无发送记录
                  </p>
                ) : (
                  history.map((entry, index) => (
                    <button
                      key={`${entry.at}-${index}`}
                      type="button"
                      onClick={() => {
                        setText(entry.text);
                        setHistoryOpen(false);
                        textareaRef.current?.focus();
                      }}
                      className="block w-full truncate px-2 py-1.5 text-left text-[11px] text-foreground transition-colors hover:bg-muted"
                      title="点击还原到输入框"
                    >
                      {entry.text}
                    </button>
                  ))
                )}
              </div>
            </div>
          )}
        </div>
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="ml-1 flex h-6 items-center rounded px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          收起
        </button>
      </div>

      {/* 输入区 */}
      <div className="flex items-end gap-2 px-2 py-1.5">
        <textarea
          ref={textareaRef}
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              handleSend();
            }
          }}
          placeholder="输入要发送到终端的内容"
          rows={3}
          className="min-h-0 flex-1 resize-none rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground placeholder:text-muted-foreground focus:border-foreground/40 focus:outline-none"
        />
        <div className="flex shrink-0 flex-col gap-1">
          <button
            type="button"
            onClick={handleSend}
            title={
              settings.queueMode
                ? "加入队列并按顺序发送（Ctrl/⌘ + Enter）"
                : "立即发送到终端（Ctrl/⌘ + Enter）"
            }
            className="flex h-8 items-center gap-1.5 rounded-md bg-emerald-600/90 px-3 text-xs font-medium text-white transition-colors hover:bg-emerald-500 disabled:opacity-50"
            disabled={!text.trim()}
          >
            <SendHorizonal className="h-3.5 w-3.5" />
            {settings.queueMode ? "入队" : "发送"}
          </button>
          {settings.queueMode && (
            <button
              type="button"
              onClick={() => {
                const content = text.trim();
                if (!content) return;
                void (async () => {
                  const ok = await sendNow(content);
                  if (!ok) return;
                  persistEntry({ text: content, at: Date.now() });
                  setText("");
                })();
              }}
              title="跳过队列，立即发送"
              className="flex h-6 items-center justify-center rounded-md border border-border px-2 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
              disabled={!text.trim()}
            >
              立即发送
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import {
  EditorView,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import {
  bracketMatching,
  defaultHighlightStyle,
  foldEffect,
  foldGutter,
  foldKeymap,
  foldService,
  foldState,
  foldable,
  ensureSyntaxTree,
  indentOnInput,
  syntaxHighlighting,
  syntaxTree,
  unfoldEffect,
} from "@codemirror/language";
import { json } from "@codemirror/lang-json";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorState } from "@codemirror/state";
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  ChevronsDownUp,
  ChevronsUpDown,
  Clock,
  Copy,
  FileText,
  FolderOpen,
  RefreshCw,
  Search,
  WrapText,
  X,
  Zap,
} from "lucide-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { proxyApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { useProxyRequestLogFiles } from "@/lib/query/proxy";
import type {
  ProxyRequestLogFileMeta,
  ProxyRequestLogListRow,
  ProxyRequestLogRecord,
} from "@/types/proxy";
import { APP_ICON_MAP } from "@/config/appConfig";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useDarkMode } from "@/hooks/useDarkMode";
import { extractErrorMessage } from "@/utils/errorUtils";

const PAGE_SIZE = 50;
const SEARCH_DEBOUNCE_MS = 300;

type DetailTab =
  | "requestHeaders"
  | "requestBody"
  | "responseHeaders"
  | "responseBody";

/** 复制按钮：点击后短暂变成对勾反馈。text 支持惰性求值——
 * 大记录的序列化（cURL / 整条 JSON）只在真正点击时才执行 */
function CopyButton({
  text,
  label,
}: {
  text: string | (() => string);
  label?: string;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      // copyText：优先走 Tauri 原生剪贴板命令。WKWebView 的
      // navigator.clipboard 在部分 macOS 版本上会静默挂起（promise 永不落定）
      await copyText(typeof text === "function" ? text() : text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      toast.error(extractErrorMessage(error));
    }
  }, [text]);

  return (
    <Button
      variant="ghost"
      size="icon"
      // 带 label 时不能用固定宽度（size=icon 是正方形），否则文字会溢出按钮边界
      className={
        label ? "h-6 w-auto gap-1 px-1.5 py-0 shrink-0" : "h-6 w-6 shrink-0"
      }
      onClick={handleCopy}
      title={t("common.copy")}
    >
      {copied ? (
        <Check className="w-3 h-3 shrink-0 text-emerald-500" />
      ) : (
        <Copy className="w-3 h-3 shrink-0" />
      )}
      {label && <span className="text-xs">{label}</span>}
    </Button>
  );
}

function headersToText(
  headers: { name: string; value: string }[] | undefined,
): string {
  if (!headers || headers.length === 0) return "";
  return headers.map((h) => `${h.name}: ${h.value}`).join("\n");
}

/** shell 单引号转义：'…' 内部出现 ' 时拆成 '\'' */
function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/** 由记录生成可直接执行的 cURL 命令（含真实凭据，注意不要外传） */
function recordToCurl(record: ProxyRequestLogRecord): string {
  const method = record.method || "GET";
  // endpoint 只记 path（如 /v1/messages）；上游 host 在转发时被原位写入 host 头
  const hostHeader = (record.requestHeaders ?? []).find(
    (h) => h.name.toLowerCase() === "host",
  )?.value;
  // 拼出可执行 URL；取不到 host（记录缺失/host 被剥）时保留 path 并提示
  const endpoint = record.endpoint || "/";
  const url =
    hostHeader && /^https?:\/\//i.test(endpoint)
      ? endpoint
      : hostHeader
        ? `https://${hostHeader}${endpoint.startsWith("/") ? endpoint : `/${endpoint}`}`
        : endpoint;
  // GET/HEAD 不写 -X：curl 手册明确 -X 会破坏重定向方法降级
  // （301/302 时 GET 是默认安全行为），显式 -X GET 反而制造隐患
  const methodPart =
    method === "GET" || method === "HEAD" ? "curl" : `curl -X ${method}`;
  const parts = [`${methodPart} ${shellQuote(url)}`];
  for (const h of record.requestHeaders ?? []) {
    if (!h.name || !h.value) continue;
    // 跳过 hop-by-hop / 内容长度类头：重放时由 curl/客户端重新生成
    const lower = h.name.toLowerCase();
    if (
      lower === "content-length" ||
      lower === "host" ||
      lower === "connection" ||
      lower === "accept-encoding" ||
      lower.startsWith("transfer-")
    ) {
      continue;
    }
    parts.push(`  -H ${shellQuote(`${h.name}: ${h.value}`)}`);
  }
  const body = record.requestBody;
  if (body !== null && body !== undefined) {
    const bodyText =
      typeof body === "string" ? body : JSON.stringify(body, null, 2);
    if (bodyText) {
      parts.push(`  -d ${shellQuote(bodyText)}`);
    }
  }
  return parts.join(" \\\n");
}

function valueToText(value: unknown): string {
  if (value === null || value === undefined) return "";
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function statusColor(code: number | null | undefined): string {
  if (code == null) return "bg-zinc-400";
  if (code < 300) return "bg-emerald-500";
  if (code < 500) return "bg-amber-500";
  return "bg-red-500";
}

function statusText(code: number | null | undefined): string {
  if (code == null) return "text-zinc-500";
  if (code < 300) return "text-emerald-600 dark:text-emerald-400";
  if (code < 500) return "text-amber-600 dark:text-amber-400";
  return "text-red-600 dark:text-red-400";
}

function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "-";
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

/** 千分位紧凑展示 token 数（1200 → 1.2k，1500000 → 1.5M） */
function formatTokens(n: number): string {
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${+(n / 1000).toFixed(1)}k`;
  return `${+(n / 1_000_000).toFixed(1)}M`;
}

/** Intl 格式化器按 locale+选项 缓存：虚拟列表滚动时每个可见行都会格式化时间，
 * 每次 new Date().toLocaleString() 都要走一遍 Intl 查找，行多时明显掉帧。
 * 键必须含选项集——同一 locale 会同时用多种格式（行内时钟 / 完整时间 / 日期分隔） */
const formatterCache = new Map<string, Intl.DateTimeFormat>();

function getCachedFormatter(
  locale: string | undefined,
  options: Intl.DateTimeFormatOptions,
): Intl.DateTimeFormat {
  const key = `${locale ?? ""}|${JSON.stringify(options)}`;
  let fmt = formatterCache.get(key);
  if (!fmt) {
    fmt = new Intl.DateTimeFormat(locale, options);
    formatterCache.set(key, fmt);
  }
  return fmt;
}

/** 只显示时间部分（HH:mm:ss），完整日期在 hover 时可见 */
function formatClock(iso: string | null | undefined, locale?: string): string {
  if (!iso) return "--:--:--";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "--:--:--";
  return getCachedFormatter(locale, {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(d);
}

/** 记录所在日期（用于跨天分隔行），同天返回 null */
function dateOf(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  return d.toDateString();
}

function formatFullTime(
  iso: string | null | undefined,
  locale: string,
): string {
  if (!iso) return "-";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return getCachedFormatter(locale, {
    hour12: false,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(d);
}

function HeaderTable({
  headers,
}: {
  headers: { name: string; value: string }[] | undefined;
}) {
  const { t } = useTranslation();
  if (!headers || headers.length === 0) {
    return <div className="text-xs text-muted-foreground py-2">—</div>;
  }
  return (
    <div className="rounded-md border border-border overflow-hidden">
      <div className="flex justify-end bg-muted/30 px-1.5 py-0.5 border-b border-border">
        <CopyButton
          text={headersToText(headers)}
          label={t("proxy.requestLogViewer.copyAsText")}
        />
      </div>
      <table className="w-full text-xs font-mono">
        <tbody>
          {headers.map((h, i) => (
            <tr
              key={i}
              className={i % 2 === 0 ? "bg-transparent" : "bg-muted/30"}
            >
              <td className="px-2 py-1 align-top whitespace-nowrap text-muted-foreground border-r border-border w-1/4">
                {h.name}
              </td>
              <td className="px-2 py-1 break-all">{h.value}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** 查找覆盖 [from,to] 的已折叠范围（foldState 是 Decoration 集） */
function findFoldedRange(
  state: EditorState,
  from: number,
  to: number,
): { from: number; to: number } | null {
  let found: { from: number; to: number } | null = null;
  state.field(foldState, false)?.between(from, to, (f, t) => {
    if (t > from && f < to) found = { from: f, to: t };
  });
  return found;
}

/**
 * 折叠到一级：遍历语法树的顶层属性，折叠「key: {…}/[…]」的值部分，标量值保留。
 * 不能逐行调 foldable()：它把 foldService 与语法内置 foldInside 混在一起——
 * 根对象行（CM 行号从 1 起，第 1 行就是「{」）会把整个文档折成 {…}，
 * 复合值属性行则同时命中两个来源产生嵌套的重复折叠区间。
 */
function foldToLevel1(view: EditorView): boolean {
  // 大文档的树是惰性解析的（只解析到视口附近），直接读 syntaxTree 会拿到
  // 残缺树——顶层属性遍历到未解析区间就断，只折出第一个复合属性。
  // ensureSyntaxTree 强制解析到文档末尾（timeout 500ms 应对 MB 级 body）。
  const tree =
    ensureSyntaxTree(view.state, view.state.doc.length, 500) ??
    syntaxTree(view.state);
  // 树顶层是 JsonText，真正的根对象在它的子节点
  const root = tree.topNode.firstChild ?? tree.topNode;
  if (root.name !== "Object") return false;
  const effects: ReturnType<typeof foldEffect.of>[] = [];
  for (let prop = root.firstChild; prop; prop = prop.nextSibling) {
    if (prop.name !== "Property") continue;
    const name = prop.firstChild;
    const colon = name?.nextSibling;
    const value = colon?.nextSibling;
    if (
      !name ||
      !colon ||
      !value ||
      (value.name !== "Object" && value.name !== "Array") ||
      value.to <= colon.to + 1
    ) {
      continue;
    }
    effects.push(foldEffect.of({ from: colon.to, to: value.to }));
  }
  if (!effects.length) return false;
  view.dispatch({ effects });
  return true;
}

/** 全部展开：清除所有折叠范围 */
function unfoldEverything(view: EditorView): boolean {
  const field = view.state.field(foldState, false);
  if (!field || !field.size) return false;
  const effects: ReturnType<typeof unfoldEffect.of>[] = [];
  field.between(0, view.state.doc.length, (from, to) => {
    effects.push(unfoldEffect.of({ from, to }));
  });
  view.dispatch({ effects });
  return true;
}

/** 是否存在任何折叠区间（用于切换按钮显示折叠/展开哪个动作） */
function hasAnyFold(view: EditorView | null): boolean {
  if (!view) return false;
  const field = view.state.field(foldState, false);
  return !!field && field.size > 0;
}

/**
 * 只读 CodeMirror JSON 视图。
 * - gutter 箭头 / 点击行号区折叠（Array 与 Object 均可）
 * - 点击正文某行 = 折叠/展开该行所在结构（只读视图无法设置光标，
 *   官方 foldCode 依赖 selection，必须自己按 posAtDOM 定位）
 * - foldService：对值是 Object/Array 的属性返回折叠范围，
 *   使「折叠到一级」能把值收成 … 而保留一级 key 与标量值
 */
function JsonViewer({
  text,
  dark,
  wrapped,
  onViewReady,
  onHeightChange,
  onFoldChange,
}: {
  text: string;
  dark: boolean;
  wrapped: boolean;
  /** 暴露 EditorView 供工具栏执行 全部展开/折叠到一级 */
  onViewReady?: (view: EditorView | null) => void;
  /** 高度变化（折叠/展开/换行改变行数）→ 通知虚拟列表重新测量 */
  onHeightChange?: () => void;
  /** 折叠区间数量变化（工具栏或点击行折叠/展开）→ 同步按钮状态 */
  onFoldChange?: (folded: boolean) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const heightChangeRef = useRef(onHeightChange);
  heightChangeRef.current = onHeightChange;
  const foldChangeRef = useRef(onFoldChange);
  foldChangeRef.current = onFoldChange;
  const viewRef = useRef<EditorView | null>(null);
  // 记录当前视图的 dark/wrapped 配置：这两项无法热切换（oneDark / lineWrapping
  // 都在创建时进 extensions，动态切换需要 compartment），变化时只能重建
  const viewConfigRef = useRef<{ dark: boolean; wrapped: boolean } | null>(
    null,
  );

  // 首次挂载 / dark / wrapped 变化 → 重建视图；
  // 仅 text 变化 → 原视图整文档替换，保住滚动位置。
  // （折叠在文档替换后仍会失效——旧折叠区间对新文档无意义。）
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handlers = {
      onViewReady,
      onHeightChange: () => heightChangeRef.current?.(),
      onFoldChange: (folded: boolean) => foldChangeRef.current?.(folded),
    };

    const needsRebuild =
      !viewRef.current ||
      viewConfigRef.current?.dark !== dark ||
      viewConfigRef.current?.wrapped !== wrapped;

    if (needsRebuild) {
      viewRef.current?.destroy();
      viewRef.current = createView(container, text, dark, wrapped, handlers);
      viewConfigRef.current = { dark, wrapped };
      return;
    }

    const view = viewRef.current;
    if (view && view.state.doc.toString() !== text) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
      });
      // 整文档替换后旧折叠区间全部失效；updateListener 跳过 docChanged
      // 的事务，这里手动同步按钮状态
      foldChangeRef.current?.(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text, dark, wrapped]);

  useEffect(
    () => () => {
      viewRef.current?.destroy();
      viewRef.current = null;
      onViewReady?.(null);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  return (
    <div
      ref={containerRef}
      className="rounded-md border border-border bg-muted/40"
    />
  );
}

/** 创建只读 CodeMirror 视图（extensions 组装集中在这里） */
function createView(
  container: HTMLDivElement,
  text: string,
  dark: boolean,
  wrapped: boolean,
  handlers: {
    onViewReady?: (view: EditorView | null) => void;
    onHeightChange?: () => void;
    onFoldChange?: (folded: boolean) => void;
  },
): EditorView {
  const theme = EditorView.theme({
    // 限高放在 .cm-editor 上 + scroller 开 overflow，纵向/横向才能在编辑器内部滚动
    "&": { maxHeight: "420px" },
    ".cm-scroller": {
      fontFamily:
        "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace",
      fontSize: "11px",
      lineHeight: "1.6",
      overflow: "auto",
    },
    ".cm-gutters": {
      background: "transparent",
      borderRight: "1px solid hsl(var(--border))",
      color: "hsl(var(--muted-foreground))",
    },
    ".cm-activeLine": { background: "transparent" },
    ".cm-activeLineGutter": { background: "transparent" },
    ".cm-foldGutter .cm-gutterElement": {
      cursor: "pointer",
      color: "hsl(var(--muted-foreground))",
      padding: "0 4px",
    },
    ".cm-foldGutter .cm-gutterElement:hover": {
      color: "hsl(var(--foreground))",
    },
  });

  // 「折叠到一级」的支撑 + gutter 标记定位：仅「键在本行声明、值是跨行
  // 复合类型」的属性行可折。不能只判「整行被 Property 覆盖」——那样值内部
  // 的每一行（role/content 等嵌套行）都会命中外层属性，gutter 会出现一排
  // 重复按钮；而属性声明行本身（有缩进，Property 不含行首缩进）反而漏掉。
  // 键不在本行 → 说明本行在某个外层属性的值内部，继续向外找只会命中同一
  // 个属性，直接返回 null，让 syntaxFolding（Object/Array 开括号行）接管。
  const valueFoldService = foldService.of((state, lineStart, lineEnd) => {
    // 同 foldToLevel1：行在惰性解析边界之外时树是残缺的，先确保解析到位
    const tree = ensureSyntaxTree(state, lineEnd, 500) ?? syntaxTree(state);
    if (tree.length < lineEnd) return null;
    let iter: ReturnType<typeof tree.resolveStack> | null = tree.resolveStack(
      lineEnd,
      1,
    );
    while (iter) {
      const cur = iter.node;
      if (cur.name === "Property") {
        const name = cur.firstChild;
        const colon = name?.nextSibling;
        const value = colon?.nextSibling;
        if (!name || !colon || !value) return null;
        // 键必须在本行：值内部行（键在外层声明）不给折叠按钮
        if (name.from < lineStart || name.to > lineEnd) return null;
        if (value.name !== "Object" && value.name !== "Array") return null;
        if (value.to <= lineEnd) return null;
        return { from: colon.to, to: value.to };
      }
      iter = iter.next;
    }
    return null;
  });

  const extensions = [
    // 只读视图：不引 basicSetup——其中 drawSelection 会把原生选区置为透明，
    // 只读下无法编辑复制体验差；closeBrackets/autocompletion 等编辑功能也无意义
    lineNumbers(),
    highlightSpecialChars(),
    foldGutter(),
    indentOnInput(),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    bracketMatching(),
    json(),
    rectangularSelection(),
    valueFoldService,
    ...(wrapped ? [EditorView.lineWrapping] : []),
    EditorView.editable.of(false),
    EditorState.readOnly.of(true),
    keymap.of([...foldKeymap]),
    // 单击正文：折叠/展开该行所在结构（gutter 点击之外的补充入口）。
    // 用 click 而非 mousedown，且拖拽产生选区时跳过——保护文本选择复制
    EditorView.domEventHandlers({
      click: (event, view) => {
        const target = event.target as HTMLElement | null;
        if (!target || !target.closest(".cm-content")) return false;
        if (target.closest(".cm-foldPlaceholder")) return false;
        const sel = window.getSelection();
        if (sel && !sel.isCollapsed) return false;
        const pos = view.posAtDOM(target);
        const line = view.state.doc.lineAt(pos);
        const folded = findFoldedRange(view.state, line.from, line.to);
        if (folded) {
          view.dispatch({ effects: unfoldEffect.of(folded) });
          return true;
        }
        const range = foldable(view.state, line.from, line.to);
        if (range) {
          view.dispatch({ effects: foldEffect.of(range) });
          return true;
        }
        return false;
      },
    }),
    // 折叠/展开会改变高度 → 通知外层（虚拟列表测量），
    // 并把「存在折叠区间」同步给工具栏（切换按钮显示哪个动作）。
    // 不能用 viewportChanged 排除：折叠占位符缩短内容会让 CM 重算视口，
    // 折叠事务本身就带着 viewportChanged=true，跳过它就漏通知
    EditorView.updateListener.of((update) => {
      if (update.docChanged) return;
      if (update.transactions.some((tr) => tr.effects.length > 0)) {
        handlers.onHeightChange?.();
        handlers.onFoldChange?.(hasAnyFold(update.view));
      }
    }),
    theme,
  ];
  if (dark) extensions.push(oneDark);

  const view = new EditorView({
    state: EditorState.create({ doc: text, extensions }),
    parent: container,
  });
  handlers.onViewReady?.(view);
  return view;
}

/** WrapText 的对偶图标（lucide 无 unwrap-text）：行向右延伸，不折回 */
function UnwrapText(props: React.SVGProps<SVGSVGElement>) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      {...props}
    >
      <path d="M3 6h18" />
      <path d="M3 12h15a3 3 0 1 1 0 6h-4" />
      <path d="m18 15 3 3-3 3" />
    </svg>
  );
}

function JsonBlock({
  value,
  onHeightChange,
}: {
  value: unknown;
  onHeightChange?: () => void;
}) {
  const { t } = useTranslation();
  const dark = useDarkMode();
  const [wrapped, setWrapped] = useState(false);
  // 折叠状态提升为 React state：EditorView 的折叠区间变化不触发 render，
  // 按钮显示哪个动作（折叠到一级 / 全部展开）需要它
  const [folded, setFolded] = useState(false);
  const viewRef = useRef<EditorView | null>(null);
  // 大 body（MB 级）的 stringify 是同步阻塞操作，用 useMemo 保证
  // 仅在 value 真正变化时计算一次，而非每次 render 都重算
  const fullText = useMemo(
    () => (value === null || value === undefined ? "" : valueToText(value)),
    [value],
  );
  if (value === null || value === undefined) {
    return <div className="text-xs text-muted-foreground py-2">—</div>;
  }

  const toolBtn =
    "inline-flex items-center gap-1 h-6 px-1.5 rounded text-[11px] text-muted-foreground hover:text-foreground hover:bg-muted transition-colors";

  return (
    <div className="space-y-1">
      {/* 常驻工具栏：复制 / 折叠到一级 / 全部展开 / 换行开关，不再悬浮遮挡内容 */}
      <div className="flex flex-wrap items-center gap-1">
        <CopyButton
          text={fullText}
          label={t("proxy.requestLogViewer.copyAsJson")}
        />
        <span className="text-muted-foreground/40 text-[11px]">|</span>
        <button
          type="button"
          className={`${toolBtn} ${folded ? "text-foreground bg-muted" : ""}`}
          onClick={() => {
            const view = viewRef.current;
            if (!view) return;
            if (hasAnyFold(view)) unfoldEverything(view);
            else foldToLevel1(view);
          }}
          title={
            folded
              ? t("proxy.requestLogViewer.unfoldAll")
              : t("proxy.requestLogViewer.foldToLevel1")
          }
        >
          {folded ? (
            <ChevronsUpDown className="w-3 h-3" />
          ) : (
            <ChevronsDownUp className="w-3 h-3" />
          )}
          {folded
            ? t("proxy.requestLogViewer.unfoldAll")
            : t("proxy.requestLogViewer.foldToLevel1")}
        </button>
        <span className="text-muted-foreground/40 text-[11px]">|</span>
        <button
          type="button"
          className={`${toolBtn} ${wrapped ? "text-foreground bg-muted" : ""}`}
          onClick={() => setWrapped((w) => !w)}
          title={t("proxy.requestLogViewer.wrapLines")}
        >
          {wrapped ? (
            <UnwrapText className="w-3 h-3" />
          ) : (
            <WrapText className="w-3 h-3" />
          )}
          {t("proxy.requestLogViewer.wrapLines")}
        </button>
      </div>
      <JsonViewer
        text={fullText}
        dark={dark}
        wrapped={wrapped}
        onViewReady={(v) => (viewRef.current = v)}
        onHeightChange={onHeightChange}
        onFoldChange={setFolded}
      />
    </div>
  );
}

function SessionFileButton({
  file,
  active,
  locale,
  onSelect,
}: {
  file: ProxyRequestLogFileMeta;
  active: boolean;
  locale: string;
  onSelect: () => void;
}) {
  const app = APP_ICON_MAP[file.appType as keyof typeof APP_ICON_MAP];
  const sessionLabel = file.fileName.replace(/\.jsonl$/, "");
  return (
    <button
      type="button"
      onClick={onSelect}
      title={file.fileName}
      className={`w-full text-left rounded-lg border px-3 py-2.5 transition-colors ${
        active
          ? "border-primary/30 bg-primary/10"
          : "border-transparent hover:bg-muted/60"
      }`}
    >
      <div className="flex items-center gap-2 mb-1">
        {app?.icon && <span className="shrink-0">{app.icon}</span>}
        <span className="text-sm font-medium truncate min-w-0">
          {file.sessionTitle ?? sessionLabel}
        </span>
      </div>
      <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        {file.sessionTitle && (
          <span className="font-mono truncate min-w-0">{sessionLabel}</span>
        )}
        <span className="font-mono shrink-0">{formatSize(file.sizeBytes)}</span>
        <span className="shrink-0">
          {getCachedFormatter(locale, {
            month: "short",
            day: "numeric",
          }).format(new Date(file.modifiedAtMs))}
        </span>
      </div>
    </button>
  );
}

/** 网格列模板：与表头列一一对应 */
const RECORD_GRID_COLS =
  "grid grid-cols-[3rem_0.6rem_5rem_4.5rem_minmax(0,1fr)_9rem_6.5rem_7rem_4rem_2rem] items-center";

function RecordRowLine({
  record,
  locale,
  expanded,
  onToggle,
}: {
  record: ProxyRequestLogListRow;
  locale: string;
  expanded: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      onClick={onToggle}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onToggle();
        }
      }}
      tabIndex={0}
      role="button"
      aria-expanded={expanded}
      className={`${RECORD_GRID_COLS} cursor-pointer border-b border-border/40 py-1.5 pr-1 focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none ${
        expanded ? "bg-primary/5" : "hover:bg-muted/30"
      }`}
    >
      <div className="px-2 text-right font-mono text-[11px] text-muted-foreground tabular-nums">
        {record.lineNo ?? "-"}
      </div>
      <div className="px-1">
        <span
          className={`block w-1 h-4 rounded-full ${statusColor(record.statusCode)}`}
          title={
            record.error
              ? record.error
              : record.statusCode == null
                ? t("proxy.requestLogViewer.errorIndicator")
                : undefined
          }
        />
      </div>
      <div
        className="px-2 font-mono text-[11px] text-muted-foreground whitespace-nowrap tabular-nums"
        title={formatFullTime(record.startTime ?? undefined, locale)}
      >
        {formatClock(record.startTime, locale)}
      </div>
      <div className="px-2 text-xs font-semibold whitespace-nowrap">
        {record.method}
      </div>
      <div
        className="px-2 font-mono text-xs truncate"
        title={record.endpoint ?? undefined}
      >
        {record.endpoint}
      </div>
      <div className="px-2">
        {record.model ? (
          <Badge
            variant="secondary"
            className="text-[10px] h-5 max-w-36 truncate"
          >
            {record.model}
          </Badge>
        ) : (
          <span className="text-muted-foreground">—</span>
        )}
      </div>
      {(() => {
        const hasTokens =
          record.promptTokens != null || record.completionTokens != null;
        return (
          <div
            className="px-2 flex items-center justify-end gap-1.5 font-mono text-[10px] tabular-nums whitespace-nowrap text-muted-foreground"
            title={
              hasTokens
                ? `${record.promptTokens ?? 0} + ${record.completionTokens ?? 0}`
                : undefined
            }
          >
            {hasTokens ? (
              <>
                <span title={t("proxy.requestLogViewer.colTokensIn")}>
                  ↑{formatTokens(record.promptTokens ?? 0)}
                </span>
                <span title={t("proxy.requestLogViewer.colTokensOut")}>
                  ↓{formatTokens(record.completionTokens ?? 0)}
                </span>
              </>
            ) : (
              <span>—</span>
            )}
          </div>
        );
      })()}
      <div className="px-2 flex items-center justify-end gap-1.5 whitespace-nowrap">
        {(record.error || record.statusCode == null) && (
          <AlertCircle
            className="w-3 h-3 text-red-500 shrink-0"
            aria-label={t("proxy.requestLogViewer.errorIndicator")}
          />
        )}
        {record.isStreaming && (
          <span className="flex items-center gap-0.5 text-[10px] text-muted-foreground">
            <Zap className="w-3 h-3" />
            SSE
          </span>
        )}
        <span className="flex items-center gap-0.5 text-[10px] text-muted-foreground tabular-nums">
          <Clock className="w-3 h-3" />
          {formatDuration(record.durationMs)}
        </span>
      </div>
      <div
        className={`px-2 text-right font-mono text-xs font-semibold tabular-nums whitespace-nowrap ${statusText(record.statusCode)}`}
      >
        {record.statusCode ?? "?"}
      </div>
      <div className="px-1 flex justify-center">
        {expanded ? (
          <ChevronDown className="w-3.5 h-3.5 text-muted-foreground" />
        ) : (
          <ChevronRight className="w-3.5 h-3.5 text-muted-foreground" />
        )}
      </div>
    </div>
  );
}

function RecordDetail({
  record,
  locale,
  onContentHeightChange,
}: {
  record: ProxyRequestLogRecord;
  locale: string;
  /** JSON 折叠/展开/换行改变行高 → 虚拟列表重新测量 */
  onContentHeightChange: () => void;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<DetailTab>("requestBody");

  const tabs: { key: DetailTab; label: string }[] = [
    {
      key: "requestHeaders",
      label: t("proxy.requestLogViewer.requestHeaders"),
    },
    { key: "requestBody", label: t("proxy.requestLogViewer.requestBody") },
    {
      key: "responseHeaders",
      label: t("proxy.requestLogViewer.responseHeaders"),
    },
    { key: "responseBody", label: t("proxy.requestLogViewer.responseBody") },
  ];

  return (
    <div className="px-3 pb-3 pt-1 space-y-2.5">
      {/* 元信息行 */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
        <span className="flex items-center gap-1">
          <Clock className="w-3 h-3" />
          {t("proxy.requestLogViewer.startTimeColon")}
          <span className="font-mono text-foreground">
            {formatFullTime(record.startTime, locale)}
          </span>
        </span>
        <span>
          {t("proxy.requestLogViewer.endTimeColon")}
          <span className="font-mono text-foreground">
            {formatFullTime(record.endTime, locale)}
          </span>
        </span>
        <span className="font-mono">{record.requestId?.slice(0, 8)}</span>
        {record.providerId && (
          <Badge variant="outline" className="text-[10px] h-5">
            {record.providerId}
          </Badge>
        )}
        <span className="ml-auto flex items-center gap-1">
          {/* 惰性序列化：整条记录可能几 MB，render 时不 stringify */}
          <CopyButton
            text={() => recordToCurl(record)}
            label={t("proxy.requestLogViewer.copyAsCurl")}
          />
        </span>
      </div>
      {record.error && (
        // 错误单独一行：混进 flex-wrap 元信息行会把整行撑高
        <div className="text-xs text-red-500 break-all font-mono">
          {record.error}
        </div>
      )}

      {/* 详情标签页 */}
      <div className="flex items-center gap-1 border-b border-border">
        {tabs.map((tb) => (
          <button
            key={tb.key}
            type="button"
            onClick={() => setTab(tb.key)}
            className={`px-2.5 py-1 text-xs rounded-t-md transition-colors -mb-px border-b-2 ${
              tab === tb.key
                ? "border-primary text-foreground font-medium"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
          >
            {tb.label}
          </button>
        ))}
      </div>
      {tab === "requestHeaders" && (
        <HeaderTable headers={record.requestHeaders} />
      )}
      {tab === "requestBody" && (
        <JsonBlock
          value={record.requestBody}
          onHeightChange={onContentHeightChange}
        />
      )}
      {tab === "responseHeaders" && (
        <HeaderTable headers={record.responseHeaders} />
      )}
      {tab === "responseBody" && (
        <JsonBlock
          value={record.responseBody}
          onHeightChange={onContentHeightChange}
        />
      )}
    </div>
  );
}

export function RequestLogViewer() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const locale =
    i18n.language === "zh"
      ? "zh-CN"
      : i18n.language === "zh-TW"
        ? "zh-TW"
        : i18n.language === "ja"
          ? "ja-JP"
          : "en-US";

  const { data: files = [], isLoading } = useProxyRequestLogFiles(true);

  const [selected, setSelected] = useState<ProxyRequestLogFileMeta | null>(
    null,
  );
  const [records, setRecords] = useState<ProxyRequestLogListRow[]>([]);
  const [total, setTotal] = useState(0);
  const [recordsLoading, setRecordsLoading] = useState(false);
  // desc：新→旧（默认）；asc：旧→新
  const [order, setOrder] = useState<"desc" | "asc">("desc");
  // 展开的记录（虚拟化后展开状态提升到列表层，键为 lineNo）
  const [expandedKeys, setExpandedKeys] = useState<Set<number>>(new Set());
  // 展开行的完整记录缓存（列表行是轻量字段，body 详情按 lineNo 单独取）
  const [details, setDetails] = useState<Map<number, ProxyRequestLogRecord>>(
    new Map(),
  );
  // 加载中的详情行（显示骨架，避免重复发请求）
  const [loadingDetails, setLoadingDetails] = useState<Set<number>>(new Set());
  // 搜索：searchInput 为输入框即时值，searchKeyword 为防抖后的生效值
  const [searchInput, setSearchInput] = useState("");
  const [searchKeyword, setSearchKeyword] = useState("");
  // 左栏会话过滤
  const [sessionFilter, setSessionFilter] = useState("");
  const requestIdRef = useRef(0);
  const recordsLoadingRef = useRef(false);
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const lastBoundaryRefreshRef = useRef(0);
  const lastBoundaryLoadRef = useRef(0);

  useEffect(() => {
    const timer = setTimeout(
      () => setSearchKeyword(searchInput.trim()),
      SEARCH_DEBOUNCE_MS,
    );
    return () => clearTimeout(timer);
  }, [searchInput]);

  const loadRecords = useCallback(
    async (
      file: ProxyRequestLogFileMeta,
      offset: number,
      mode: "replace" | "append" | "prepend",
      opts: {
        order: "desc" | "asc";
        search: string;
        beforeLineNo?: number;
        afterLineNo?: number;
      },
    ) => {
      const requestId = ++requestIdRef.current;
      recordsLoadingRef.current = true;
      setRecordsLoading(true);
      try {
        const page = await proxyApi.getProxyRequestLogRecords({
          appType: file.appType,
          fileName: file.fileName,
          offset,
          limit: PAGE_SIZE,
          order: opts.order,
          search: opts.search || undefined,
          beforeLineNo: opts.beforeLineNo,
          afterLineNo: opts.afterLineNo,
        });
        if (requestId !== requestIdRef.current) return; // 已被更新的请求取代
        setTotal(page.total);
        const incoming = page.records;
        const scrollEl = scrollContainerRef.current;
        const previousScrollHeight = scrollEl?.scrollHeight ?? 0;
        const previousScrollTop = scrollEl?.scrollTop ?? 0;
        setRecords((prev) => {
          if (mode === "replace") return incoming;
          const seen = new Set(prev.map((r) => r.lineNo).filter(Boolean));
          const unique = incoming.filter(
            (r) => !r.lineNo || !seen.has(r.lineNo),
          );
          return mode === "prepend"
            ? [...unique, ...prev]
            : [...prev, ...unique];
        });
        if (!scrollEl) return;
        // 滚动补偿放在 setRecords 之外：updater 必须是纯函数，
        // 在里面排 rAF 会在 StrictMode double-invoke 下补偿两次
        if (mode === "replace") {
          // 换文件 / 换排序 / 换搜索词：列表整体替换，回到顶部。
          // 不重置的话 scrollTop 可能停在超出新列表高度的位置（被浏览器 clamp），
          // 或落在内容中间让用户误以为结果错乱
          requestAnimationFrame(() => {
            scrollEl.scrollTop = 0;
          });
        } else if (mode === "prepend") {
          requestAnimationFrame(() => {
            // prepend 只会在顶部拉取（scrollTop < 40）时发生：
            // 停在顶部 = 用户在看最新记录，保持吸顶让新记录自然进入视口
            if (previousScrollTop <= 40) {
              scrollEl.scrollTop = 0;
              return;
            }
            // 视口在中下部（拉取期间用户滚下去了）：补偿增量，保持视口内容不动
            const added = scrollEl.scrollHeight - previousScrollHeight;
            if (added > 0) {
              scrollEl.scrollTop = previousScrollTop + added;
            }
          });
        }
      } catch (error) {
        if (requestId === requestIdRef.current) {
          toast.error(extractErrorMessage(error));
        }
      } finally {
        if (requestId === requestIdRef.current) {
          recordsLoadingRef.current = false;
          setRecordsLoading(false);
        }
      }
    },
    [],
  );

  // 文件列表变化时自动选中第一个；
  // 当前选中文件已不存在（日志被清理/轮换）时回退到第一个，
  // 否则工具栏和右侧列表会继续显示已删除文件的残留数据
  useEffect(() => {
    if (files.length === 0) {
      setSelected(null);
      return;
    }
    const stillExists = selected
      ? files.some(
          (f) =>
            f.appType === selected.appType && f.fileName === selected.fileName,
        )
      : false;
    if (!stillExists) {
      setSelected(files[0]);
    }
  }, [files, selected]);

  // 选中文件、排序或搜索变化时，重新加载第一页
  useEffect(() => {
    if (selected) {
      setRecords([]);
      setTotal(0);
      setExpandedKeys(new Set());
      setDetails(new Map());
      loadRecords(selected, 0, "replace", { order, search: searchKeyword });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, order, searchKeyword]);

  const handleRefresh = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["proxyRequestLogFiles"] });
    if (selected) {
      loadRecords(selected, 0, "replace", { order, search: searchKeyword });
    }
  }, [queryClient, selected, order, searchKeyword, loadRecords]);

  const handleOpenDir = useCallback(async () => {
    try {
      await proxyApi.openProxyRequestLogDir();
    } catch (error) {
      toast.error(extractErrorMessage(error));
    }
  }, []);

  const toggleExpanded = useCallback(
    (lineNo: number) => {
      setExpandedKeys((prev) => {
        const next = new Set(prev);
        if (next.has(lineNo)) {
          next.delete(lineNo);
          return next;
        }
        next.add(lineNo);
        return next;
      });
      // 展开时按需拉取完整记录（列表行只有轻量字段）
      if (!details.has(lineNo) && !loadingDetails.has(lineNo) && selected) {
        const file = selected;
        setLoadingDetails((prev) => new Set(prev).add(lineNo));
        proxyApi
          .getProxyRequestLogRecord({
            appType: file.appType,
            fileName: file.fileName,
            lineNo,
          })
          .then((record) => {
            setDetails((prev) =>
              new Map(prev).set(lineNo, { ...record, lineNo }),
            );
          })
          .catch((error) => {
            toast.error(extractErrorMessage(error));
            setExpandedKeys((prev) => {
              const next = new Set(prev);
              next.delete(lineNo);
              return next;
            });
          })
          .finally(() => {
            setLoadingDetails((prev) => {
              const next = new Set(prev);
              next.delete(lineNo);
              return next;
            });
          });
      }
    },
    [details, loadingDetails, selected],
  );

  // 扁平行模型：日期分隔 / 记录主行 / 展开详情行，虚拟化列表统一消费
  const rows = useMemo(() => {
    type Row =
      | { type: "date"; timestamp: string }
      | { type: "record"; record: ProxyRequestLogListRow }
      | { type: "detail"; lineNo: number };
    const out: Row[] = [];
    let prevDate: string | null = null;
    for (const r of records) {
      const date = dateOf(r.startTime);
      if (date !== null && date !== prevDate) {
        out.push({ type: "date", timestamp: r.startTime! });
        prevDate = date;
      }
      out.push({ type: "record", record: r });
      if (typeof r.lineNo === "number" && expandedKeys.has(r.lineNo)) {
        out.push({ type: "detail", lineNo: r.lineNo });
      }
    }
    return out;
  }, [records, expandedKeys]);

  // 行唯一键：按 lineNo 定位而非数组下标。展开/收起、prepend/append
  // 会让行发生位移，若用默认的 index 作键，缓存的行高会错配到别的行 → 行间重叠
  const rowKeyOf = useCallback(
    (index: number): string => {
      const row = rows[index];
      if (row.type === "date") return `date:${row.timestamp}`;
      if (row.type === "detail") return `detail:${row.lineNo}`;
      return `record:${row.record.lineNo ?? index}`;
    },
    [rows],
  );

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollContainerRef.current,
    estimateSize: (index) => {
      const row = rows[index];
      if (row.type === "date") return 26;
      // 详情行实际高度 = 元信息(~24) + tabs(~30) + 工具栏(~26) + 编辑器(max 420) + padding(~20)
      // ≈ 520。估算与实测差距越小，快速滚动时行位跳变越小
      if (row.type === "detail") return 520;
      return 33;
    },
    getItemKey: rowKeyOf,
    overscan: 6,
  });

  // JSON 折叠/展开/换行改变详情行高度：等 DOM 重排完成后强制重测。
  // 注意不能用 virtualizer.measure()——它会把已测得的行高缓存全部清空，
  // 已挂载行立即回退到估算高度且 ResizeObserver 不会重触发（元素尺寸没变），
  // 行位置与实际高度错开 → request body 与列表行重叠。
  // 这里改为遍历已挂载行逐行 resizeItem 重测，位置始终与真实高度一致。
  const handleContentHeightChange = useCallback(() => {
    requestAnimationFrame(() => {
      virtualizer.getVirtualItems().forEach((item) => {
        const el = virtualizer.elementsCache.get(item.key);
        if (el) {
          virtualizer.resizeItem(item.index, (el as HTMLElement).offsetHeight);
        }
      });
    });
  }, [virtualizer]);

  // asc 模式：底部加载更新记录。records.length >= total 时没有更多可加载，
  // 边界检测不应继续发起请求（否则滚到底部不动 = 每秒 2 次全文件扫描）
  const hasMore = records.length < total;

  const handleLoadLatest = useCallback(() => {
    if (!selected || order !== "desc" || recordsLoadingRef.current) return;
    const lineNos = records
      .map((record) => record.lineNo)
      .filter((lineNo): lineNo is number => typeof lineNo === "number");
    if (lineNos.length === 0) {
      loadRecords(selected, 0, "replace", {
        order,
        search: searchKeyword,
      });
      return;
    }

    loadRecords(selected, 0, "prepend", {
      order,
      search: searchKeyword,
      afterLineNo: Math.max(...lineNos),
    });
  }, [selected, records, order, searchKeyword, loadRecords]);

  const handleLoadMore = useCallback(() => {
    if (!selected || recordsLoadingRef.current) return;
    if (order === "desc" && records.length >= total) return;

    const lineNos = records
      .map((record) => record.lineNo)
      .filter((lineNo): lineNo is number => typeof lineNo === "number");
    const beforeLineNo = lineNos.length > 0 ? Math.min(...lineNos) : undefined;
    const afterLineNo = lineNos.length > 0 ? Math.max(...lineNos) : undefined;

    loadRecords(selected, lineNos.length > 0 ? 0 : records.length, "append", {
      order,
      search: searchKeyword,
      beforeLineNo: order === "desc" ? beforeLineNo : undefined,
      afterLineNo: order === "asc" ? afterLineNo : undefined,
    });
  }, [selected, records, total, order, searchKeyword, loadRecords]);

  const handleLogListBoundary = useCallback(
    (el: HTMLDivElement) => {
      const now = Date.now();
      const nearTop = el.scrollTop < 40;
      const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 100;

      if (
        nearTop &&
        order === "desc" &&
        now - lastBoundaryRefreshRef.current > 1000
      ) {
        lastBoundaryRefreshRef.current = now;
        handleLoadLatest();
      }

      if (
        nearBottom &&
        !recordsLoadingRef.current &&
        hasMore &&
        now - lastBoundaryLoadRef.current > 500
      ) {
        lastBoundaryLoadRef.current = now;
        handleLoadMore();
      }
    },
    [order, hasMore, handleLoadLatest, handleLoadMore],
  );

  // 按 app 分组（应用会话过滤）
  const grouped = useMemo(() => {
    const keyword = sessionFilter.trim().toLowerCase();
    const map = new Map<string, ProxyRequestLogFileMeta[]>();
    for (const f of files) {
      if (keyword) {
        const hay = `${f.fileName} ${f.sessionTitle ?? ""}`.toLowerCase();
        if (!hay.includes(keyword)) continue;
      }
      const list = map.get(f.appType) ?? [];
      list.push(f);
      map.set(f.appType, list);
    }
    return [...map.entries()];
  }, [files, sessionFilter]);

  return (
    <div className="mx-auto h-full min-h-0 w-full max-w-[1600px] px-4 sm:px-6">
      {files.length === 0 ? (
        <div className="flex h-full flex-col items-center justify-center text-muted-foreground gap-2">
          <FileText className="w-8 h-8 opacity-40" />
          <span className="text-sm">
            {isLoading
              ? t("proxy.requestLogViewer.loading")
              : t("proxy.requestLogViewer.empty")}
          </span>
        </div>
      ) : (
        /* 与会话管理页一致的双栏布局：左列固定宽，右列自适应 */
        <div className="grid h-full min-h-0 gap-4 md:grid-cols-[320px_1fr]">
          {/* 左栏：会话文件列表 */}
          <Card className="flex min-h-0 flex-col overflow-hidden">
            <CardHeader className="flex flex-col gap-2 border-b py-2 px-3">
              <div className="flex items-center justify-between gap-2">
                <div className="flex min-w-0 items-center gap-2">
                  <CardTitle className="text-sm font-medium whitespace-nowrap">
                    {t("proxy.requestLogViewer.sessionFiles")}
                  </CardTitle>
                  <Badge variant="secondary" className="text-xs">
                    {files.length}
                  </Badge>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    onClick={() => void handleOpenDir()}
                    title={t("proxy.requestLogViewer.openDir")}
                  >
                    <FolderOpen className="w-3.5 h-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    onClick={handleRefresh}
                    disabled={recordsLoading}
                    title={t("proxy.requestLogViewer.refresh")}
                  >
                    <RefreshCw
                      className={`w-3.5 h-3.5 ${recordsLoading ? "animate-spin" : ""}`}
                    />
                  </Button>
                </div>
              </div>
              <div className="relative">
                <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
                {/* IME 安全：中文输入法组合中的拼音不应触发过滤重算 */}
                <ImeSafeInput
                  value={sessionFilter}
                  onValueChange={setSessionFilter}
                  placeholder={t(
                    "proxy.requestLogViewer.searchSessionsPlaceholder",
                  )}
                  className="h-7 pl-7 pr-7 text-xs bg-background"
                />
                {sessionFilter && (
                  <button
                    type="button"
                    onClick={() => setSessionFilter("")}
                    className="absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                    title={t("common.clear")}
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                )}
              </div>
            </CardHeader>
            <CardContent className="min-h-0 flex-1 p-0">
              <ScrollArea className="h-full">
                <div className="space-y-4 py-3 px-2">
                  {grouped.length === 0 && (
                    <div className="flex flex-col items-center justify-center text-muted-foreground gap-2 py-8">
                      <Search className="w-5 h-5 opacity-40" />
                      <span className="text-xs">
                        {t("proxy.requestLogViewer.noSessionsMatch")}
                      </span>
                    </div>
                  )}
                  {grouped.map(([appType, appFiles]) => {
                    const app =
                      APP_ICON_MAP[appType as keyof typeof APP_ICON_MAP];
                    return (
                      <div key={appType}>
                        <div className="flex items-center gap-1.5 px-2.5 mb-1">
                          {app?.icon}
                          <span className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
                            {app?.label ?? appType}
                          </span>
                          <span className="text-[10px] text-muted-foreground/60 ml-auto">
                            {appFiles.length}
                          </span>
                        </div>
                        <div className="space-y-0.5">
                          {appFiles.map((f) => (
                            <SessionFileButton
                              key={`${f.appType}/${f.fileName}`}
                              file={f}
                              locale={locale}
                              active={
                                selected?.fileName === f.fileName &&
                                selected?.appType === f.appType
                              }
                              onSelect={() => setSelected(f)}
                            />
                          ))}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </ScrollArea>
            </CardContent>
          </Card>

          {/* 右栏：记录列表 */}
          <Card className="flex min-h-0 flex-col overflow-hidden">
            {!selected ? (
              <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground gap-2">
                <FileText className="w-8 h-8 opacity-40" />
                <span className="text-sm">
                  {t("proxy.requestLogViewer.noFileSelected")}
                </span>
              </div>
            ) : (
              <>
                {/* 会话头部：标题（身份信息）在上，操作与列表工具栏在下 */}
                <CardHeader className="flex flex-col gap-2.5 border-b py-3 px-4">
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2 mb-1">
                        {(() => {
                          const app =
                            APP_ICON_MAP[
                              selected.appType as keyof typeof APP_ICON_MAP
                            ];
                          return app?.icon ? (
                            <span className="shrink-0">{app.icon}</span>
                          ) : null;
                        })()}
                        <h2 className="text-base font-semibold truncate">
                          {selected.sessionTitle ??
                            selected.fileName.replace(/\.jsonl$/, "")}
                        </h2>
                      </div>
                      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                        {selected.sessionTitle && (
                          <span
                            className="font-mono truncate max-w-72"
                            title={selected.fileName}
                          >
                            {selected.fileName.replace(/\.jsonl$/, "")}
                          </span>
                        )}
                        <span className="font-mono">
                          {formatSize(selected.sizeBytes)}
                        </span>
                        <span>
                          {t("proxy.requestLogViewer.shownOf", {
                            shown: records.length,
                            total,
                          })}
                        </span>
                      </div>
                    </div>
                  </div>

                  {/* 列表工具栏：排序切换 + 搜索 */}
                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-7"
                      onClick={() =>
                        setOrder((o) => (o === "desc" ? "asc" : "desc"))
                      }
                      title={t(
                        order === "desc"
                          ? "proxy.requestLogViewer.sortNewest"
                          : "proxy.requestLogViewer.sortOldest",
                      )}
                    >
                      {order === "desc" ? (
                        <ChevronDown className="w-3.5 h-3.5" />
                      ) : (
                        <ChevronUp className="w-3.5 h-3.5" />
                      )}
                      {order === "desc"
                        ? t("proxy.requestLogViewer.sortNewest")
                        : t("proxy.requestLogViewer.sortOldest")}
                    </Button>
                    <div className="relative flex-1 max-w-xs">
                      <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
                      {/* IME 安全 + 防抖：组合中的拼音不触发搜索，
                       * 防抖避免每敲一个字母就全文件扫描一次 */}
                      <ImeSafeInput
                        value={searchInput}
                        onValueChange={setSearchInput}
                        placeholder={t(
                          "proxy.requestLogViewer.searchPlaceholder",
                        )}
                        className="h-7 pl-7 pr-7 text-xs"
                      />
                      {searchInput && (
                        <button
                          type="button"
                          onClick={() => {
                            setSearchInput("");
                            setSearchKeyword("");
                          }}
                          className="absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                          title={t("common.clear")}
                        >
                          <X className="w-3.5 h-3.5" />
                        </button>
                      )}
                    </div>
                  </div>
                </CardHeader>
                <CardContent className="min-h-0 flex-1 flex flex-col p-0">
                  <div
                    ref={scrollContainerRef}
                    className="flex-1 min-h-0 overflow-auto"
                    onScroll={(e) => handleLogListBoundary(e.currentTarget)}
                    onWheel={(e) => {
                      const el = e.currentTarget;
                      const atTop = el.scrollTop <= 0;
                      const atBottom =
                        el.scrollHeight - el.scrollTop - el.clientHeight <= 1;
                      if (
                        (e.deltaY < 0 && atTop) ||
                        (e.deltaY > 0 && atBottom)
                      ) {
                        handleLogListBoundary(el);
                      }
                    }}
                  >
                    {/* sticky 表头：置于滚动容器内第一层，背景遮住下方虚拟行 */}
                    <div
                      className={`${RECORD_GRID_COLS} sticky top-0 z-10 h-9 bg-background shadow-[0_1px_0_0_hsl(var(--border))]`}
                    >
                      <div className="px-2 text-right font-mono font-medium text-muted-foreground text-xs">
                        #
                      </div>
                      <div className="px-1" />
                      <div className="px-2 text-left font-medium text-muted-foreground text-xs">
                        {t("proxy.requestLogViewer.colTime")}
                      </div>
                      <div className="px-2 text-left font-medium text-muted-foreground text-xs">
                        {t("proxy.requestLogViewer.colMethod")}
                      </div>
                      <div className="px-2 text-left font-medium text-muted-foreground text-xs">
                        {t("proxy.requestLogViewer.colEndpoint")}
                      </div>
                      <div className="px-2 text-left font-medium text-muted-foreground text-xs">
                        {t("proxy.requestLogViewer.colModel")}
                      </div>
                      <div className="px-2 text-right font-medium text-muted-foreground text-xs">
                        {t("proxy.requestLogViewer.colTokens")}
                      </div>
                      <div className="px-2 text-right font-medium text-muted-foreground text-xs">
                        {t("proxy.requestLogViewer.colDuration")}
                      </div>
                      <div className="px-2 text-right font-medium text-muted-foreground text-xs">
                        {t("proxy.requestLogViewer.colStatus")}
                      </div>
                      <div className="px-1" />
                    </div>
                    {!recordsLoading && records.length === 0 && (
                      <div className="flex flex-col items-center justify-center text-muted-foreground gap-2 py-10">
                        <Search className="w-6 h-6 opacity-40" />
                        <span className="text-sm">
                          {searchKeyword
                            ? t("proxy.requestLogViewer.noMatches")
                            : t("proxy.requestLogViewer.emptyFile")}
                        </span>
                      </div>
                    )}
                    {/* 虚拟化行：spacer 撑总高，行绝对定位 translateY */}
                    <div
                      style={{
                        height: virtualizer.getTotalSize(),
                        position: "relative",
                      }}
                    >
                      {virtualizer.getVirtualItems().map((virtualRow) => {
                        const row = rows[virtualRow.index];
                        return (
                          <div
                            key={virtualRow.key}
                            data-index={virtualRow.index}
                            ref={virtualizer.measureElement}
                            style={{
                              position: "absolute",
                              top: 0,
                              left: 0,
                              width: "100%",
                              transform: `translateY(${virtualRow.start}px)`,
                            }}
                          >
                            {row.type === "date" ? (
                              <div className="bg-muted/60 px-3 py-1 border-y border-border-default text-[10px] font-medium text-muted-foreground">
                                {getCachedFormatter(locale, {
                                  year: "numeric",
                                  month: "long",
                                  day: "numeric",
                                  weekday: "short",
                                }).format(new Date(row.timestamp!))}
                              </div>
                            ) : row.type === "detail" ? (
                              details.has(row.lineNo) ? (
                                <RecordDetail
                                  record={details.get(row.lineNo)!}
                                  locale={locale}
                                  onContentHeightChange={
                                    handleContentHeightChange
                                  }
                                />
                              ) : (
                                <div className="flex items-center justify-center gap-2 text-xs text-muted-foreground py-8">
                                  <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                                  {t("proxy.requestLogViewer.loading")}
                                </div>
                              )
                            ) : (
                              <RecordRowLine
                                record={row.record}
                                locale={locale}
                                expanded={expandedKeys.has(row.record.lineNo!)}
                                onToggle={() =>
                                  toggleExpanded(row.record.lineNo!)
                                }
                              />
                            )}
                          </div>
                        );
                      })}
                    </div>
                    {recordsLoading && (
                      <div className="flex items-center justify-center gap-2 text-xs text-muted-foreground py-3">
                        <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                        {t("proxy.requestLogViewer.loading")}
                      </div>
                    )}
                  </div>
                  <div className="px-4 py-2.5 flex items-center justify-end border-t border-border-default">
                    {hasMore && (
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7"
                        onClick={handleLoadMore}
                        disabled={recordsLoading}
                      >
                        <ChevronDown className="w-3.5 h-3.5" />
                        {order === "desc"
                          ? t("proxy.requestLogViewer.loadMore")
                          : t("proxy.requestLogViewer.loadMoreOldest")}
                      </Button>
                    )}
                  </div>
                </CardContent>
              </>
            )}
          </Card>
        </div>
      )}
    </div>
  );
}

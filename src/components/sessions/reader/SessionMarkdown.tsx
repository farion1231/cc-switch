import { markdownLanguage } from "@codemirror/lang-markdown";
import { invoke } from "@tauri-apps/api/core";
import { ArrowUpRight } from "lucide-react";
import {
  createContext,
  Fragment,
  memo,
  type MouseEvent,
  type ReactNode,
  useContext,
  useMemo,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { highlightText } from "../utils";
import { fileUrlToPath, PathChip, shortenPath } from "./PathChip";
import {
  SESSION_INLINE_CODE_CLASS,
  SessionCodeBlock,
} from "./SessionCodeBlock";

// 底本：farion1231/cc-switch#6332 的 src/components/sessions/SessionMarkdown.tsx。
// 迁入阅读页后改为 v7 token、链接走 open_external、本地路径渲染为 PathChip，
// 并加了超长内容的纯文本退化。

type MarkdownNode = ReturnType<typeof markdownLanguage.parser.parse>["topNode"];

/** 超过该长度不做 Markdown 解析，退化为「纯文本 + 围栏代码」（§7.1 单块 ≤ 64KB） */
export const MARKDOWN_PARSE_LIMIT = 65536;

export interface SessionMarkdownProps {
  content: string;
  searchQuery?: string;
  /** 会话项目目录：本地路径显示为相对路径 */
  projectDir?: string;
  /** body：最终回复正文；note：过程中的说明（小字、次级色） */
  variant?: "body" | "note";
  /** 打开外链（http/https/mailto）；不传时调用 `open_external` 命令 */
  onOpenLink?: (url: string) => void;
  /** 「在 Finder 中显示」；不传时调用 `reveal_session_path` 命令 */
  onRevealPath?: (path: string) => void;
  /** 远程图片是否直接加载；默认 false，需用户点击后才发请求 */
  autoLoadRemoteImages?: boolean;
  /** 本地图片（`![](/abs/path.png)`）的渲染；不传时显示为路径 chip */
  renderLocalImage?: (path: string, alt: string) => ReactNode;
  className?: string;
}

const MARKER_NODES = new Set([
  "CodeMark",
  "EmphasisMark",
  "HeaderMark",
  "LinkMark",
  "ListMark",
  "QuoteMark",
  "StrikethroughMark",
  "SubscriptMark",
  "SuperscriptMark",
  "TaskMarker",
]);

// 无法解析的链接/图片按 CommonMark 原样输出：这些节点渲染成字面文本，
// 其余行内标记（如粗体分界）照常隐藏。
const LINK_SYNTAX_NODES: ReadonlySet<string> = new Set([
  "LinkMark",
  "LinkLabel",
  "LinkTitle",
  "URL",
]);
const LITERAL_LINK_SKIPPED_NODES: ReadonlySet<string> = new Set(
  [...MARKER_NODES].filter((name) => name !== "LinkMark"),
);
const NO_NODES: ReadonlySet<string> = new Set();

interface TraversalOptions {
  skippedNodes?: ReadonlySet<string>;
  // 按源码原样输出的节点。
  literalNodes?: ReadonlySet<string>;
  // 只遍历该区间内的子节点（用于只取链接的可见文本）。
  from?: number;
  to?: number;
}

const LITERAL_LINK_OPTIONS: TraversalOptions = {
  skippedNodes: LITERAL_LINK_SKIPPED_NODES,
  literalNodes: LINK_SYNTAX_NODES,
};

// 折叠预览、可见匹配检测与渲染会在同一次渲染流程中连续解析同一段内容，
// 缓存最近一次解析结果即可覆盖这种访问模式。
let lastParse: {
  content: string;
  tree: ReturnType<typeof markdownLanguage.parser.parse>;
} | null = null;

const parseMarkdown = (content: string) => {
  if (lastParse?.content !== content) {
    lastParse = { content, tree: markdownLanguage.parser.parse(content) };
  }
  return lastParse.tree;
};

// `[docs](<https://example.com/page>)` 是合法 Markdown，但 lezer 的 URL 节点
// 文本带着尖括号，需要先剥掉再做协议校验。
const stripAngleBrackets = (value: string) => {
  const trimmed = value.trim();
  return trimmed.length > 1 && trimmed.startsWith("<") && trimmed.endsWith(">")
    ? trimmed.slice(1, -1).trim()
    : trimmed;
};

const safeExternalUrl = (value: string) => {
  const url = stripAngleBrackets(value);
  if (/^(https?:|mailto:)/i.test(url)) return url;
  if (/^www\./i.test(url)) return `https://${url}`;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(url)) return `mailto:${url}`;
  return null;
};

const safeRemoteImageUrl = (value: string) => {
  const candidate = stripAngleBrackets(value);
  const normalized = /^www\./i.test(candidate)
    ? `https://${candidate}`
    : candidate;

  try {
    const url = new URL(normalized);
    return url.protocol === "http:" || url.protocol === "https:"
      ? normalized
      : null;
  } catch {
    return null;
  }
};

// 本地路径：`file://` URL，或绝对路径（POSIX、`~/`、Windows 盘符）。
// 相对路径无法可靠解析，仍按字面输出。
const localPathTarget = (value: string) => {
  const target = stripAngleBrackets(value);
  if (/^file:\/\//i.test(target)) {
    const path = fileUrlToPath(target);
    return path ? path : null;
  }
  if (/^(\/(?!\/)|~\/|[A-Za-z]:[\\/])/.test(target)) return target;
  return null;
};

type LinkTarget =
  | { kind: "external"; url: string }
  | { kind: "path"; path: string };

const resolveLinkTargetValue = (value: string): LinkTarget | null => {
  const url = safeExternalUrl(value);
  if (url) return { kind: "external", url };
  const path = localPathTarget(value);
  return path ? { kind: "path", path } : null;
};

type ImageTarget =
  | { kind: "remote"; src: string }
  | { kind: "path"; path: string };

const resolveImageTargetValue = (value: string): ImageTarget | null => {
  const src = safeRemoteImageUrl(value);
  if (src) return { kind: "remote", src };
  const path = localPathTarget(value);
  return path ? { kind: "path", path } : null;
};

const renderText = (text: string, searchQuery?: string) =>
  searchQuery ? highlightText(text, searchQuery) : text;

const childNodes = (node: MarkdownNode) => {
  const children: MarkdownNode[] = [];
  for (let child = node.firstChild; child; child = child.nextSibling) {
    children.push(child);
  }
  return children;
};

const childNodesWithin = (node: MarkdownNode, from: number, to: number) =>
  childNodes(node).filter((child) => child.from >= from && child.to <= to);

// 引用定义：label → 原始目标（已确认能解析为链接或图片）。
type LinkReferences = ReadonlyMap<string, string>;
const EMPTY_LINK_REFERENCES: LinkReferences = new Map();

interface RenderContext {
  searchQuery?: string;
  linkReferences: LinkReferences;
  projectDir?: string;
}

const EMPTY_CONTEXT: RenderContext = { linkReferences: EMPTY_LINK_REFERENCES };

const normalizeReferenceLabel = (value: string) => {
  const label =
    value.startsWith("[") && value.endsWith("]") ? value.slice(1, -1) : value;
  return label.trim().replace(/\s+/g, " ").toLowerCase();
};

// 链接可见文本的区间：前两个 LinkMark 之间。URL、title 以及它们之间的
// 空白都在区间之外，不能混进 label。
const getLinkLabelRange = (node: MarkdownNode) => {
  const marks = childNodes(node).filter((child) => child.name === "LinkMark");
  return marks.length >= 2 ? { from: marks[0].to, to: marks[1].from } : null;
};

const getLinkLabelSource = (node: MarkdownNode, source: string) => {
  const range = getLinkLabelRange(node);
  return range ? source.slice(range.from, range.to) : "";
};

const getReferenceLabel = (node: MarkdownNode, source: string) => {
  const labelNode = node.getChild("LinkLabel");
  const explicitLabel = labelNode
    ? normalizeReferenceLabel(source.slice(labelNode.from, labelNode.to))
    : "";

  // Collapsed and shortcut references use their source label, not the rendered
  // text. Formatting markers are part of the CommonMark reference key.
  return (
    explicitLabel || normalizeReferenceLabel(getLinkLabelSource(node, source))
  );
};

const collectLinkReferences = (root: MarkdownNode, source: string) => {
  const references = new Map<string, string>();

  const visit = (node: MarkdownNode) => {
    if (node.name === "LinkReference") {
      const labelNode = node.getChild("LinkLabel");
      const urlNode = node.getChild("URL");
      if (labelNode && urlNode) {
        const label = normalizeReferenceLabel(
          source.slice(labelNode.from, labelNode.to),
        );
        const target = source.slice(urlNode.from, urlNode.to);
        if (
          !references.has(label) &&
          (resolveLinkTargetValue(target) || resolveImageTargetValue(target))
        ) {
          references.set(label, target);
        }
      }
    }

    childNodes(node).forEach(visit);
  };

  visit(root);
  return references;
};

const getTargetSource = (
  node: MarkdownNode,
  source: string,
  linkReferences: LinkReferences,
) => {
  const urlNode = node.getChild("URL");
  return urlNode
    ? source.slice(urlNode.from, urlNode.to)
    : linkReferences.get(getReferenceLabel(node, source));
};

const resolveLinkTarget = (
  node: MarkdownNode,
  source: string,
  linkReferences: LinkReferences,
) => {
  const target = getTargetSource(node, source, linkReferences);
  return target ? resolveLinkTargetValue(target) : null;
};

const resolveImageTarget = (
  node: MarkdownNode,
  source: string,
  linkReferences: LinkReferences,
) => {
  const target = getTargetSource(node, source, linkReferences);
  return target ? resolveImageTargetValue(target) : null;
};

interface UnclosedFence {
  fence: string;
  // 围栏起始行的容器前缀。闭合行保留引用标记与等宽缩进；无前缀的
  // 顶层围栏会先结束容器、再开启一个只含省略号的新代码块。
  prefix: string;
}

const findUnclosedFence = (
  node: MarkdownNode,
  source: string,
): UnclosedFence | null => {
  if (node.name === "FencedCode" && node.to === source.length) {
    const marks = childNodes(node).filter((child) => child.name === "CodeMark");
    if (marks.length === 1) {
      const lineStart = source.lastIndexOf("\n", marks[0].from - 1) + 1;
      // 列表标记只允许出现在列表项首行，闭合行上换成等宽空格保持缩进，
      // 否则会另起一个列表项。
      const prefix = source
        .slice(lineStart, marks[0].from)
        .replace(/[^>\s]/g, " ");
      return { fence: source.slice(marks[0].from, marks[0].to), prefix };
    }
  }

  for (const child of childNodes(node)) {
    const fence = findUnclosedFence(child, source);
    if (fence) return fence;
  }

  return null;
};

/** 折叠预览：截到 maxLength，若截断处在未闭合的围栏内则补上闭合围栏 */
export const createCollapsedMarkdownPreview = (
  content: string,
  maxLength: number,
) => {
  const preview = content.slice(0, maxLength);
  const tree = parseMarkdown(preview);
  const unclosed = findUnclosedFence(tree.topNode, preview);

  return unclosed
    ? `${preview}\n${unclosed.prefix}${unclosed.fence}\n\n…`
    : `${preview}…`;
};

// 与 renderNode 一样不产生可见文本的节点（TableDelimiter、链接引用定义、
// 代码块语言标记）。CodeInfo 仅作为 data-language 属性输出。
const HIDDEN_TEXT_NODES = new Set([
  "CodeInfo",
  "LinkReference",
  "TableDelimiter",
]);

const decodeEntityText = (raw: string) => {
  const element = document.createElement("textarea");
  element.innerHTML = raw;
  return element.value;
};

// 行内代码渲染前的规范化：换行折成空格，成对的首尾空格去掉。
const getInlineCodeText = (node: MarkdownNode, source: string) => {
  const marks = childNodes(node).filter((child) => child.name === "CodeMark");
  const firstMark = marks[0];
  const lastMark = marks[marks.length - 1];
  const code = (
    firstMark && lastMark
      ? source.slice(firstMark.to, lastMark.from)
      : source.slice(node.from, node.to)
  ).replace(/\n/g, " ");

  return code.startsWith(" ") && code.endsWith(" ") && code.trim()
    ? code.slice(1, -1)
    : code;
};

// 收集渲染后真正可见的文本。对渲染时会做变换的节点（实体、转义、行内
// 代码、路径缩短）必须 push 变换后的结果而不是源码切片，否则命中判定会和
// 高亮结果脱节：判定说“可高亮”、渲染时却匹配不上，两头落空。
const collectVisibleTextPieces = (
  node: MarkdownNode,
  source: string,
  pieces: string[],
  ctx: RenderContext,
  {
    skippedNodes = MARKER_NODES,
    literalNodes = NO_NODES,
    from = node.from,
    to = node.to,
  }: TraversalOptions = {},
) => {
  // 表格 cell 之间的 `|` 分隔符渲染时不输出，只收 cell 自身。
  if (node.name === "TableHeader" || node.name === "TableRow") {
    for (const child of childNodes(node)) {
      if (child.name === "TableCell") {
        collectVisibleTextPieces(child, source, pieces, ctx);
      }
    }
    return;
  }

  let cursor = from;

  for (const child of childNodesWithin(node, from, to)) {
    if (child.from > cursor) {
      pieces.push(source.slice(cursor, child.from));
    }

    if (literalNodes.has(child.name)) {
      pieces.push(source.slice(child.from, child.to));
    } else if (
      !skippedNodes.has(child.name) &&
      !HIDDEN_TEXT_NODES.has(child.name)
    ) {
      if (child.name === "Image") {
        const target = resolveImageTarget(child, source, ctx.linkReferences);
        if (target?.kind === "remote") {
          pieces.push(getLinkLabelSource(child, source));
        } else if (target?.kind === "path") {
          pieces.push(shortenPath(target.path, ctx.projectDir));
        } else {
          collectVisibleTextPieces(
            child,
            source,
            pieces,
            ctx,
            LITERAL_LINK_OPTIONS,
          );
        }
      } else if (child.name === "Link" || child.name === "Autolink") {
        const range = getLinkLabelRange(child);
        const target = range
          ? resolveLinkTarget(child, source, ctx.linkReferences)
          : null;
        if (target?.kind === "path" && child.name === "Autolink") {
          pieces.push(shortenPath(target.path, ctx.projectDir));
        } else {
          collectVisibleTextPieces(
            child,
            source,
            pieces,
            ctx,
            range && target ? range : LITERAL_LINK_OPTIONS,
          );
        }
      } else if (child.name === "Entity") {
        pieces.push(decodeEntityText(source.slice(child.from, child.to)));
      } else if (child.name === "Escape") {
        pieces.push(source.slice(child.from + 1, child.to));
      } else if (child.name === "InlineCode") {
        pieces.push(getInlineCodeText(child, source));
      } else {
        collectVisibleTextPieces(child, source, pieces, ctx);
      }
    }
    cursor = child.to;
  }

  if (cursor < to) {
    pieces.push(source.slice(cursor, to));
  }
};

// ---------------------------------------------------------------------------
// 超长内容的退化：只识别围栏代码，其余按纯文本显示
// ---------------------------------------------------------------------------

export type PlainTextBlock =
  | { type: "text"; text: string }
  | { type: "code"; language: string; code: string };

/** 把内容切成纯文本段与围栏代码段；未闭合的围栏一直延续到末尾 */
export const splitFencedCode = (content: string): PlainTextBlock[] => {
  const blocks: PlainTextBlock[] = [];
  const lines = content.replace(/\r\n/g, "\n").split("\n");
  let text: string[] = [];
  let code: string[] | null = null;
  let language = "";
  let fence = "";

  const pushText = () => {
    const value = text.join("\n").replace(/^\n+|\n+$/g, "");
    if (value.trim()) blocks.push({ type: "text", text: value });
    text = [];
  };

  for (const line of lines) {
    if (code === null) {
      const open = /^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)/.exec(line);
      if (open) {
        pushText();
        code = [];
        fence = open[1];
        language = open[2] ?? "";
      } else {
        text.push(line);
      }
      continue;
    }

    const trimmed = line.trim();
    if (
      trimmed.length >= fence.length &&
      trimmed === fence[0].repeat(trimmed.length)
    ) {
      blocks.push({ type: "code", language, code: code.join("\n") });
      code = null;
      continue;
    }
    code.push(line);
  }

  if (code !== null) {
    blocks.push({ type: "code", language, code: code.join("\n") });
  }
  pushText();
  return blocks;
};

export interface SessionPlainTextProps {
  content: string;
  searchQuery?: string;
  className?: string;
}

/**
 * 纯文本 + 围栏代码：用户提问（决策 D7）与超长 Markdown 的退化都用它，
 * 不解析任何行内语法。
 */
export const SessionPlainText = memo(function SessionPlainText({
  content,
  searchQuery,
  className,
}: SessionPlainTextProps) {
  const blocks = useMemo(() => splitFencedCode(content), [content]);

  return (
    <div
      className={cn(
        "min-w-0 break-words text-body text-fg-1 [overflow-wrap:anywhere]",
        className,
      )}
    >
      {blocks.map((block, index) =>
        block.type === "code" ? (
          <SessionCodeBlock
            key={index}
            code={block.code}
            language={block.language}
            searchQuery={searchQuery}
          />
        ) : (
          <p
            key={index}
            className="my-1.5 whitespace-pre-wrap first:mt-0 last:mb-0"
          >
            {renderText(block.text, searchQuery)}
          </p>
        ),
      )}
    </div>
  );
});

// 判断搜索词是否会出现在某个连续渲染文本片段中。highlightText 只能高亮
// 单个文本片段内的匹配；藏在链接 URL 里或跨越行内节点边界（如跨越粗体
// 分界）的匹配渲染后不可见，需要调用方另行展示原文片段。
export const hasHighlightableMarkdownMatch = (
  content: string,
  query: string,
  projectDir?: string,
) => {
  if (!query) return false;
  const normalized = query.toLowerCase();

  if (content.length > MARKDOWN_PARSE_LIMIT) {
    return splitFencedCode(content).some((block) =>
      (block.type === "code" ? block.code : block.text)
        .toLowerCase()
        .includes(normalized),
    );
  }

  const pieces: string[] = [];
  const root = parseMarkdown(content).topNode;
  collectVisibleTextPieces(root, content, pieces, {
    linkReferences: collectLinkReferences(root, content),
    projectDir,
  });
  return pieces.some((piece) => piece.toLowerCase().includes(normalized));
};

// ---------------------------------------------------------------------------
// 交互：链接打开、路径显示、远程图片加载。经 Context 下发，回调变化不触发重新渲染整棵树。
// ---------------------------------------------------------------------------

/** 默认的外链打开：后端 `open_external` 只放行 http/https/mailto */
export const openSessionLink = async (url: string) => {
  await invoke("open_external", { url });
};

interface MarkdownActions {
  openLink: (url: string) => void;
  revealPath?: (path: string) => void;
  autoLoadRemoteImages: boolean;
  renderLocalImage?: (path: string, alt: string) => ReactNode;
}

const defaultOpenLink = (url: string) => {
  openSessionLink(url).catch((error: unknown) => {
    toast.error(
      error instanceof Error && error.message ? error.message : String(error),
    );
  });
};

const MarkdownActionsContext = createContext<MarkdownActions>({
  openLink: defaultOpenLink,
  autoLoadRemoteImages: false,
});

const LINK_CLASS =
  "rounded-[2px] text-action-text underline-offset-[3px] hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

interface ExternalLinkProps {
  url: string;
  children: ReactNode;
}

/**
 * 外链：保留 href 供辅助技术识别为链接，但拦截默认导航，统一交给
 * `open_external`（系统浏览器）打开。
 */
const ExternalLink = ({ url, children }: ExternalLinkProps) => {
  const { openLink } = useContext(MarkdownActionsContext);
  const isWeb = /^https?:/i.test(url);

  const handleClick = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    openLink(url);
  };

  return (
    <a
      href={url}
      title={url}
      rel="noopener noreferrer"
      onClick={handleClick}
      onAuxClick={(event) => event.preventDefault()}
      className={LINK_CLASS}
    >
      {children}
      {isWeb && (
        <ArrowUpRight
          aria-hidden
          className="ms-px inline size-3 align-[-1px] opacity-70"
        />
      )}
    </a>
  );
};

const MarkdownPathChip = (props: {
  path: string;
  projectDir?: string;
  label?: ReactNode;
  searchQuery?: string;
}) => {
  const { revealPath } = useContext(MarkdownActionsContext);
  return <PathChip {...props} onReveal={revealPath} />;
};

/** 本地图片：调用方提供渲染器（阅读页用 SessionImage）时显示图片，否则退回路径 chip */
const MarkdownLocalImage = (props: {
  path: string;
  alt: string;
  projectDir?: string;
  searchQuery?: string;
}) => {
  const { renderLocalImage } = useContext(MarkdownActionsContext);
  if (renderLocalImage) return <>{renderLocalImage(props.path, props.alt)}</>;
  return (
    <MarkdownPathChip
      path={props.path}
      projectDir={props.projectDir}
      searchQuery={props.searchQuery}
    />
  );
};

interface RemoteImageProps {
  src: string;
  alt: string;
  searchQuery?: string;
}

const RemoteImage = ({ src, alt, searchQuery }: RemoteImageProps) => {
  const { t } = useTranslation();
  const { autoLoadRemoteImages } = useContext(MarkdownActionsContext);
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);
  const [failedSrc, setFailedSrc] = useState<string | null>(null);

  const label =
    alt ||
    t("sessionManager.remoteImage", {
      defaultValue: "远程图片",
    });

  // 加载失败退化为链接
  if (failedSrc === src) {
    return (
      <ExternalLink url={src}>{renderText(label, searchQuery)}</ExternalLink>
    );
  }

  if (autoLoadRemoteImages || loadedSrc === src) {
    return (
      <img
        src={src}
        alt={alt}
        title={src}
        loading="lazy"
        decoding="async"
        referrerPolicy="no-referrer"
        onError={() => setFailedSrc(src)}
        className="my-2 block max-h-80 max-w-full rounded-[8px] border border-border object-contain"
      />
    );
  }

  const loadLabel = t("sessionManager.reader.image.loadRemote", {
    defaultValue: "加载远程图片",
  });
  return (
    <button
      type="button"
      title={src}
      aria-label={`${loadLabel}: ${label}`}
      onClick={() => setLoadedSrc(src)}
      className="my-2 flex max-w-full flex-col items-start rounded-[8px] border border-dashed border-border-strong bg-subtle px-3 py-2 text-left text-caption text-fg-2 transition-colors hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <span className="max-w-full truncate font-medium">
        {renderText(label, searchQuery)}
      </span>
      <span className="text-fg-3">{loadLabel}</span>
    </button>
  );
};

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

const renderChildren = (
  node: MarkdownNode,
  source: string,
  ctx: RenderContext = EMPTY_CONTEXT,
  {
    skippedNodes = MARKER_NODES,
    literalNodes = NO_NODES,
    from = node.from,
    to = node.to,
  }: TraversalOptions = {},
): ReactNode[] => {
  const { searchQuery } = ctx;
  const result: ReactNode[] = [];
  let cursor = from;

  childNodesWithin(node, from, to).forEach((child, index) => {
    if (child.from > cursor) {
      result.push(
        <Fragment key={`text-${cursor}`}>
          {renderText(source.slice(cursor, child.from), searchQuery)}
        </Fragment>,
      );
    }

    if (literalNodes.has(child.name)) {
      result.push(
        <Fragment key={`literal-${child.from}`}>
          {renderText(source.slice(child.from, child.to), searchQuery)}
        </Fragment>,
      );
    } else if (!skippedNodes.has(child.name)) {
      result.push(
        <Fragment key={`${child.name}-${child.from}-${index}`}>
          {renderNode(child, source, ctx)}
        </Fragment>,
      );
    }
    cursor = child.to;
  });

  if (cursor < to) {
    result.push(
      <Fragment key={`text-${cursor}`}>
        {renderText(source.slice(cursor, to), searchQuery)}
      </Fragment>,
    );
  }

  return result;
};

const renderCode = (node: MarkdownNode, source: string, ctx: RenderContext) => {
  const codeNodes = childNodes(node).filter(
    (child) => child.name === "CodeText",
  );
  const languageNode = node.getChild("CodeInfo");
  const code = codeNodes
    .map((codeNode) => source.slice(codeNode.from, codeNode.to))
    .join("");
  const language = languageNode
    ? source.slice(languageNode.from, languageNode.to).trim()
    : undefined;

  return (
    <SessionCodeBlock
      code={code}
      language={language}
      searchQuery={ctx.searchQuery}
    />
  );
};

const renderInlineCode = (
  node: MarkdownNode,
  source: string,
  ctx: RenderContext,
) => (
  <code className={SESSION_INLINE_CODE_CLASS}>
    {renderText(getInlineCodeText(node, source), ctx.searchQuery)}
  </code>
);

const renderLink = (node: MarkdownNode, source: string, ctx: RenderContext) => {
  const target = resolveLinkTarget(node, source, ctx.linkReferences);
  const range = getLinkLabelRange(node);

  // 解析不出目标（引用未定义、相对路径、非白名单协议）时按 CommonMark
  // 原样输出，方括号和目标一起保留；lezer 对任何 `[...]` 都会产出 Link 节点，
  // 不这样处理的话 `[0]`、`[Tool: shell]` 这类普通文本会丢掉方括号。
  if (!target || !range) {
    return <>{renderChildren(node, source, ctx, LITERAL_LINK_OPTIONS)}</>;
  }

  if (target.kind === "path") {
    return (
      <MarkdownPathChip
        path={target.path}
        projectDir={ctx.projectDir}
        searchQuery={ctx.searchQuery}
        label={
          node.name === "Autolink"
            ? undefined
            : renderChildren(node, source, ctx, range)
        }
      />
    );
  }

  const label =
    node.name === "Autolink"
      ? renderText(source.slice(range.from, range.to), ctx.searchQuery)
      : renderChildren(node, source, ctx, range);

  return <ExternalLink url={target.url}>{label}</ExternalLink>;
};

const renderImage = (
  node: MarkdownNode,
  source: string,
  ctx: RenderContext,
) => {
  const target = resolveImageTarget(node, source, ctx.linkReferences);

  if (!target) {
    return <>{renderChildren(node, source, ctx, LITERAL_LINK_OPTIONS)}</>;
  }

  if (target.kind === "path") {
    return (
      <MarkdownLocalImage
        path={target.path}
        alt={getLinkLabelSource(node, source)}
        projectDir={ctx.projectDir}
        searchQuery={ctx.searchQuery}
      />
    );
  }

  return (
    <RemoteImage
      src={target.src}
      alt={getLinkLabelSource(node, source)}
      searchQuery={ctx.searchQuery}
    />
  );
};

const renderTable = (
  node: MarkdownNode,
  source: string,
  ctx: RenderContext,
) => {
  const header = node.getChild("TableHeader");
  const rows = childNodes(node).filter((child) => child.name === "TableRow");

  return (
    <div className="my-2 max-w-full overflow-x-auto">
      <table className="w-full border-collapse text-left text-caption">
        {header && <thead>{renderNode(header, source, ctx)}</thead>}
        {rows.length > 0 && (
          <tbody>
            {rows.map((row) => (
              <Fragment key={row.from}>{renderNode(row, source, ctx)}</Fragment>
            ))}
          </tbody>
        )}
      </table>
    </div>
  );
};

const renderTableRow = (
  node: MarkdownNode,
  source: string,
  ctx: RenderContext,
) => (
  <tr>
    {childNodes(node)
      .filter((child) => child.name === "TableCell")
      .map((cell) => (
        <Fragment key={cell.from}>{renderNode(cell, source, ctx)}</Fragment>
      ))}
  </tr>
);

// 标题层级映射到阅读页字号：h1 → title、h2 → section、h3 及以下 → body
const HEADING_CLASS: Record<number, string> = {
  1: "mt-4 text-title",
  2: "mt-3 text-section",
  3: "mt-3 text-body font-semibold",
};

const renderNode = (
  node: MarkdownNode,
  source: string,
  ctx: RenderContext = EMPTY_CONTEXT,
): ReactNode => {
  const { searchQuery } = ctx;
  const children = () => renderChildren(node, source, ctx);

  if (/^(ATX|Setext)Heading[1-6]$/.test(node.name)) {
    const level = Number(node.name.at(-1));
    const className = cn(
      "mb-1.5 tracking-normal text-fg-1 first:mt-0",
      HEADING_CLASS[level] ?? "mt-2 text-body font-semibold text-fg-2",
    );
    if (level === 1) return <h1 className={className}>{children()}</h1>;
    if (level === 2) return <h2 className={className}>{children()}</h2>;
    if (level === 3) return <h3 className={className}>{children()}</h3>;
    if (level === 4) return <h4 className={className}>{children()}</h4>;
    if (level === 5) return <h5 className={className}>{children()}</h5>;
    return <h6 className={className}>{children()}</h6>;
  }

  switch (node.name) {
    case "Document":
      return <>{children()}</>;
    case "Paragraph":
      return <p className="my-1.5 first:mt-0 last:mb-0">{children()}</p>;
    case "StrongEmphasis":
      return <strong className="font-semibold">{children()}</strong>;
    case "Emphasis":
      return <em>{children()}</em>;
    case "Strikethrough":
      return <del className="text-fg-2">{children()}</del>;
    case "Subscript":
      return <sub>{children()}</sub>;
    case "Superscript":
      return <sup>{children()}</sup>;
    case "InlineCode":
      return renderInlineCode(node, source, ctx);
    case "FencedCode":
    case "CodeBlock":
      return renderCode(node, source, ctx);
    case "BulletList":
      return (
        <ul className="my-1.5 list-disc space-y-1 pl-5 marker:text-fg-3">
          {children()}
        </ul>
      );
    case "OrderedList": {
      const firstMark = node.getChild("ListItem")?.getChild("ListMark");
      const start = firstMark
        ? Number.parseInt(source.slice(firstMark.from, firstMark.to), 10)
        : 1;
      return (
        <ol
          className="my-1.5 list-decimal space-y-1 pl-5 marker:text-fg-3"
          start={Number.isNaN(start) ? 1 : start}
        >
          {children()}
        </ol>
      );
    }
    case "ListItem":
      return <li className="pl-0.5">{children()}</li>;
    case "Task": {
      const marker = node.getChild("TaskMarker");
      const checked = marker
        ? /x/i.test(source.slice(marker.from, marker.to))
        : false;
      return (
        <span className="inline-flex items-start gap-1.5">
          <input
            type="checkbox"
            checked={checked}
            readOnly
            disabled
            className="mt-[3px] size-3.5 accent-action"
          />
          <span>{children()}</span>
        </span>
      );
    }
    case "Blockquote":
      return (
        <blockquote className="my-2 border-l-2 border-border-strong pl-3 text-fg-2">
          {children()}
        </blockquote>
      );
    case "HorizontalRule":
      return <hr className="my-3 border-border" />;
    case "Table":
      return renderTable(node, source, ctx);
    case "TableHeader":
    case "TableRow":
      return renderTableRow(node, source, ctx);
    case "TableCell": {
      const isHeader = node.parent?.name === "TableHeader";
      const Cell = isHeader ? "th" : "td";
      return (
        <Cell
          className={cn(
            "border border-border px-2 py-1.5 align-top",
            isHeader && "bg-subtle font-semibold",
          )}
        >
          {children()}
        </Cell>
      );
    }
    case "TableDelimiter":
    case "LinkReference":
      return null;
    case "Link":
    case "Autolink":
      return renderLink(node, source, ctx);
    case "URL": {
      const label = source.slice(node.from, node.to);
      const href = safeExternalUrl(label);
      return href ? (
        <ExternalLink url={href}>{renderText(label, searchQuery)}</ExternalLink>
      ) : (
        renderText(label, searchQuery)
      );
    }
    case "Image":
      return renderImage(node, source, ctx);
    case "HardBreak":
      return <br />;
    case "Escape":
      return renderText(source.slice(node.from + 1, node.to), searchQuery);
    case "Entity":
      return renderText(
        decodeEntityText(source.slice(node.from, node.to)),
        searchQuery,
      );
    default:
      if (MARKER_NODES.has(node.name)) return null;
      return <>{children()}</>;
  }
};

const VARIANT_CLASS = {
  body: "text-body text-fg-1",
  note: "text-caption text-fg-2",
} as const;

/** 完整 Markdown 渲染（超过 MARKDOWN_PARSE_LIMIT 时退化为 SessionPlainText） */
const MarkdownTree = memo(function MarkdownTree({
  content,
  searchQuery,
  projectDir,
}: Pick<SessionMarkdownProps, "content" | "searchQuery" | "projectDir">) {
  const tree = useMemo(() => parseMarkdown(content), [content]);
  const linkReferences = useMemo(
    () => collectLinkReferences(tree.topNode, content),
    [content, tree],
  );
  const rendered = useMemo(
    () =>
      renderNode(tree.topNode, content, {
        searchQuery,
        linkReferences,
        projectDir,
      }),
    [content, linkReferences, projectDir, searchQuery, tree],
  );
  return <>{rendered}</>;
});

export const SessionMarkdown = memo(function SessionMarkdown({
  content,
  searchQuery,
  projectDir,
  variant = "body",
  onOpenLink,
  onRevealPath,
  autoLoadRemoteImages = false,
  renderLocalImage,
  className,
}: SessionMarkdownProps) {
  const actions = useMemo<MarkdownActions>(
    () => ({
      openLink: onOpenLink ?? defaultOpenLink,
      revealPath: onRevealPath,
      autoLoadRemoteImages,
      renderLocalImage,
    }),
    [autoLoadRemoteImages, onOpenLink, onRevealPath, renderLocalImage],
  );

  if (content.length > MARKDOWN_PARSE_LIMIT) {
    return (
      <SessionPlainText
        content={content}
        searchQuery={searchQuery}
        className={cn(VARIANT_CLASS[variant], className)}
      />
    );
  }

  return (
    <MarkdownActionsContext.Provider value={actions}>
      <div
        data-session-markdown=""
        className={cn(
          "min-w-0 break-words [overflow-wrap:anywhere]",
          VARIANT_CLASS[variant],
          className,
        )}
      >
        <MarkdownTree
          content={content}
          searchQuery={searchQuery}
          projectDir={projectDir}
        />
      </div>
    </MarkdownActionsContext.Provider>
  );
});

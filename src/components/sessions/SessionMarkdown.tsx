import { markdownLanguage } from "@codemirror/lang-markdown";
import { Fragment, memo, type ReactNode, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";
import { highlightText } from "./utils";

type MarkdownNode = ReturnType<typeof markdownLanguage.parser.parse>["topNode"];

interface SessionMarkdownProps {
  content: string;
  searchQuery?: string;
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

const LINK_METADATA_NODES = new Set([
  ...MARKER_NODES,
  "LinkLabel",
  "LinkTitle",
  "URL",
]);

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

const safeExternalUrl = (value: string) => {
  const url = value.trim();
  if (/^(https?:|mailto:)/i.test(url)) return url;
  if (/^www\./i.test(url)) return `https://${url}`;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(url)) return `mailto:${url}`;
  return null;
};

const safeRemoteImageUrl = (value: string) => {
  const candidate = value.trim();
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

const renderText = (text: string, searchQuery?: string) =>
  searchQuery ? highlightText(text, searchQuery) : text;

const childNodes = (node: MarkdownNode) => {
  const children: MarkdownNode[] = [];
  for (let child = node.firstChild; child; child = child.nextSibling) {
    children.push(child);
  }
  return children;
};

type LinkReferences = ReadonlyMap<string, string>;
const EMPTY_LINK_REFERENCES: LinkReferences = new Map();

const normalizeReferenceLabel = (value: string) => {
  const label =
    value.startsWith("[") && value.endsWith("]") ? value.slice(1, -1) : value;
  return label.trim().replace(/\s+/g, " ").toLowerCase();
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
        const href = safeExternalUrl(source.slice(urlNode.from, urlNode.to));
        if (href && !references.has(label)) references.set(label, href);
      }
    }

    childNodes(node).forEach(visit);
  };

  visit(root);
  return references;
};

const findUnclosedFence = (
  node: MarkdownNode,
  source: string,
): string | null => {
  if (node.name === "FencedCode" && node.to === source.length) {
    const marks = childNodes(node).filter((child) => child.name === "CodeMark");
    if (marks.length === 1) {
      return source.slice(marks[0].from, marks[0].to);
    }
  }

  for (const child of childNodes(node)) {
    const fence = findUnclosedFence(child, source);
    if (fence) return fence;
  }

  return null;
};

export const createCollapsedMarkdownPreview = (
  content: string,
  maxLength: number,
) => {
  const preview = content.slice(0, maxLength);
  const tree = parseMarkdown(preview);
  const unclosedFence = findUnclosedFence(tree.topNode, preview);

  return unclosedFence ? `${preview}\n${unclosedFence}\n\n…` : `${preview}…`;
};

// 与 renderNode 一样不产生可见文本的节点（TableDelimiter、链接引用定义、
// 代码块语言标记）。CodeInfo 仅作为 data-language 属性输出。
const HIDDEN_TEXT_NODES = new Set([
  "CodeInfo",
  "LinkReference",
  "TableDelimiter",
]);

const collectVisibleTextPieces = (
  node: MarkdownNode,
  source: string,
  pieces: string[],
  skippedNodes = MARKER_NODES,
) => {
  let cursor = node.from;

  for (const child of childNodes(node)) {
    if (child.from > cursor) {
      pieces.push(source.slice(cursor, child.from));
    }

    if (!skippedNodes.has(child.name) && !HIDDEN_TEXT_NODES.has(child.name)) {
      if (child.name === "Image") {
        const raw = source.slice(child.from, child.to);
        pieces.push(/^!\[([^\]]*)\]/.exec(raw)?.[1] ?? "");
      } else if (child.name === "Link") {
        collectVisibleTextPieces(child, source, pieces, LINK_METADATA_NODES);
      } else {
        collectVisibleTextPieces(child, source, pieces);
      }
    }
    cursor = child.to;
  }

  if (cursor < node.to) {
    pieces.push(source.slice(cursor, node.to));
  }
};

// 判断搜索词是否会出现在某个连续渲染文本片段中。highlightText 只能高亮
// 单个文本片段内的匹配；藏在链接 URL 里或跨越行内节点边界（如跨越粗体
// 分界）的匹配渲染后不可见，需要调用方另行展示原文片段。
export const hasHighlightableMarkdownMatch = (
  content: string,
  query: string,
) => {
  if (!query) return false;
  const pieces: string[] = [];
  collectVisibleTextPieces(parseMarkdown(content).topNode, content, pieces);
  const normalized = query.toLowerCase();
  return pieces.some((piece) => piece.toLowerCase().includes(normalized));
};

const renderChildren = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
  linkReferences: LinkReferences = EMPTY_LINK_REFERENCES,
  skippedNodes = MARKER_NODES,
): ReactNode[] => {
  const result: ReactNode[] = [];
  let cursor = node.from;

  childNodes(node).forEach((child, index) => {
    if (child.from > cursor) {
      result.push(
        <Fragment key={`text-${cursor}`}>
          {renderText(source.slice(cursor, child.from), searchQuery)}
        </Fragment>,
      );
    }

    if (!skippedNodes.has(child.name)) {
      result.push(
        <Fragment key={`${child.name}-${child.from}-${index}`}>
          {renderNode(child, source, searchQuery, linkReferences)}
        </Fragment>,
      );
    }
    cursor = child.to;
  });

  if (cursor < node.to) {
    result.push(
      <Fragment key={`text-${cursor}`}>
        {renderText(source.slice(cursor, node.to), searchQuery)}
      </Fragment>,
    );
  }

  return result;
};

const renderCode = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
) => {
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
    <pre className="my-2 max-w-full overflow-x-auto rounded-md border border-border/60 bg-muted/70 px-3 py-2 text-xs leading-relaxed">
      <code data-language={language}>{renderText(code, searchQuery)}</code>
    </pre>
  );
};

const renderInlineCode = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
) => {
  const marks = childNodes(node).filter((child) => child.name === "CodeMark");
  const firstMark = marks[0];
  const lastMark = marks[marks.length - 1];
  let code =
    firstMark && lastMark
      ? source.slice(firstMark.to, lastMark.from)
      : source.slice(node.from, node.to);
  code = code.replace(/\n/g, " ");
  if (code.startsWith(" ") && code.endsWith(" ") && code.trim()) {
    code = code.slice(1, -1);
  }

  return (
    <code className="rounded bg-muted px-1 py-0.5 font-mono text-[0.9em]">
      {renderText(code, searchQuery)}
    </code>
  );
};

const renderLink = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
  linkReferences: LinkReferences = EMPTY_LINK_REFERENCES,
) => {
  const urlNode = node.getChild("URL");
  const referenceLabelNode = node.getChild("LinkLabel");
  const href = urlNode
    ? safeExternalUrl(source.slice(urlNode.from, urlNode.to))
    : referenceLabelNode
      ? linkReferences.get(
          normalizeReferenceLabel(
            source.slice(referenceLabelNode.from, referenceLabelNode.to),
          ),
        )
      : null;
  const label =
    node.name === "Autolink" && urlNode
      ? renderText(source.slice(urlNode.from, urlNode.to), searchQuery)
      : renderChildren(
          node,
          source,
          searchQuery,
          linkReferences,
          LINK_METADATA_NODES,
        );

  if (!href) return <>{label}</>;

  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="text-primary underline decoration-primary/40 underline-offset-2 hover:decoration-primary"
    >
      {label}
    </a>
  );
};

interface RemoteImageProps {
  src: string;
  alt: string;
  searchQuery?: string;
}

const RemoteImage = ({ src, alt, searchQuery }: RemoteImageProps) => {
  const { t } = useTranslation();
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);

  if (loadedSrc === src) {
    return (
      <img
        src={src}
        alt={alt}
        loading="lazy"
        referrerPolicy="no-referrer"
        className="my-2 max-h-96 max-w-full rounded-md border border-border/60 object-contain"
      />
    );
  }

  const label =
    alt ||
    t("sessionManager.remoteImage", {
      defaultValue: "远程图片",
    });
  const loadLabel = t("sessionManager.loadRemoteImage", {
    defaultValue: "加载远程图片",
  });
  return (
    <button
      type="button"
      aria-label={`${loadLabel}: ${label}`}
      onClick={() => setLoadedSrc(src)}
      className="my-2 flex max-w-full flex-col items-start rounded-md border border-dashed border-border/70 bg-muted/40 px-3 py-2 text-left text-xs text-muted-foreground transition-colors hover:border-primary/50 hover:text-foreground"
    >
      <span className="max-w-full truncate font-medium">
        {renderText(label, searchQuery)}
      </span>
      <span>{loadLabel}</span>
    </button>
  );
};

const renderImage = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
) => {
  const raw = source.slice(node.from, node.to);
  const alt = /^!\[([^\]]*)\]/.exec(raw)?.[1] ?? "";
  const urlNode = node.getChild("URL");
  const src = urlNode
    ? safeRemoteImageUrl(source.slice(urlNode.from, urlNode.to))
    : null;

  if (!src) return renderText(alt, searchQuery);

  return <RemoteImage src={src} alt={alt} searchQuery={searchQuery} />;
};

const renderTable = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
  linkReferences: LinkReferences = EMPTY_LINK_REFERENCES,
) => {
  const header = node.getChild("TableHeader");
  const rows = childNodes(node).filter((child) => child.name === "TableRow");

  return (
    <div className="my-2 max-w-full overflow-x-auto">
      <table className="w-full border-collapse text-left text-xs">
        {header && (
          <thead>
            {renderNode(header, source, searchQuery, linkReferences)}
          </thead>
        )}
        {rows.length > 0 && (
          <tbody>
            {rows.map((row) => (
              <Fragment key={row.from}>
                {renderNode(row, source, searchQuery, linkReferences)}
              </Fragment>
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
  searchQuery?: string,
  linkReferences: LinkReferences = EMPTY_LINK_REFERENCES,
) => (
  <tr>
    {childNodes(node)
      .filter((child) => child.name === "TableCell")
      .map((cell) => (
        <Fragment key={cell.from}>
          {renderNode(cell, source, searchQuery, linkReferences)}
        </Fragment>
      ))}
  </tr>
);

const renderNode = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
  linkReferences: LinkReferences = EMPTY_LINK_REFERENCES,
): ReactNode => {
  const children = () =>
    renderChildren(node, source, searchQuery, linkReferences);

  if (/^(ATX|Setext)Heading[1-6]$/.test(node.name)) {
    const level = Number(node.name.at(-1));
    const className = cn(
      "font-semibold tracking-normal text-foreground",
      level === 1 && "mt-3 text-lg",
      level === 2 && "mt-3 text-base",
      level >= 3 && "mt-2 text-sm",
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
      return <del>{children()}</del>;
    case "Subscript":
      return <sub>{children()}</sub>;
    case "Superscript":
      return <sup>{children()}</sup>;
    case "InlineCode":
      return renderInlineCode(node, source, searchQuery);
    case "FencedCode":
    case "CodeBlock":
      return renderCode(node, source, searchQuery);
    case "BulletList":
      return <ul className="my-1.5 list-disc space-y-1 pl-5">{children()}</ul>;
    case "OrderedList": {
      const firstMark = node.getChild("ListItem")?.getChild("ListMark");
      const start = firstMark
        ? Number.parseInt(source.slice(firstMark.from, firstMark.to), 10)
        : 1;
      return (
        <ol
          className="my-1.5 list-decimal space-y-1 pl-5"
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
            className="mt-0.5 size-3.5 accent-primary"
          />
          <span>{children()}</span>
        </span>
      );
    }
    case "Blockquote":
      return (
        <blockquote className="my-2 border-l-2 border-primary/40 pl-3 text-muted-foreground">
          {children()}
        </blockquote>
      );
    case "HorizontalRule":
      return <hr className="my-3 border-border/70" />;
    case "Table":
      return renderTable(node, source, searchQuery, linkReferences);
    case "TableHeader":
    case "TableRow":
      return renderTableRow(node, source, searchQuery, linkReferences);
    case "TableCell": {
      const Cell = node.parent?.name === "TableHeader" ? "th" : "td";
      return (
        <Cell className="border border-border/70 px-2 py-1.5 align-top">
          {children()}
        </Cell>
      );
    }
    case "TableDelimiter":
    case "LinkReference":
      return null;
    case "Link":
    case "Autolink":
      return renderLink(node, source, searchQuery, linkReferences);
    case "URL": {
      const label = source.slice(node.from, node.to);
      const href = safeExternalUrl(label);
      return href ? (
        <a
          href={href}
          target="_blank"
          rel="noopener noreferrer"
          className="text-primary underline decoration-primary/40 underline-offset-2 hover:decoration-primary"
        >
          {renderText(label, searchQuery)}
        </a>
      ) : (
        renderText(label, searchQuery)
      );
    }
    case "Image":
      return renderImage(node, source, searchQuery);
    case "HardBreak":
      return <br />;
    case "Escape":
      return renderText(source.slice(node.from + 1, node.to), searchQuery);
    case "Entity": {
      const element = document.createElement("textarea");
      element.innerHTML = source.slice(node.from, node.to);
      return renderText(element.value, searchQuery);
    }
    default:
      if (MARKER_NODES.has(node.name)) return null;
      return <>{children()}</>;
  }
};

export const SessionMarkdown = memo(function SessionMarkdown({
  content,
  searchQuery,
}: SessionMarkdownProps) {
  const tree = useMemo(() => parseMarkdown(content), [content]);
  const linkReferences = useMemo(
    () => collectLinkReferences(tree.topNode, content),
    [content, tree],
  );
  const rendered = useMemo(
    () => renderNode(tree.topNode, content, searchQuery, linkReferences),
    [content, linkReferences, searchQuery, tree],
  );

  return (
    <div className="min-w-0 break-words text-sm leading-relaxed [overflow-wrap:anywhere]">
      {rendered}
    </div>
  );
});

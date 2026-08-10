import { markdownLanguage } from "@codemirror/lang-markdown";
import { Fragment, memo, type ReactNode, useMemo } from "react";

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
  "TaskMarker",
]);

const LINK_METADATA_NODES = new Set([
  ...MARKER_NODES,
  "LinkLabel",
  "LinkTitle",
  "URL",
]);

const safeExternalUrl = (value: string) => {
  const url = value.trim();
  if (/^(https?:|mailto:)/i.test(url)) return url;
  if (/^www\./i.test(url)) return `https://${url}`;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(url)) return `mailto:${url}`;
  return null;
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

const renderChildren = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
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
          {renderNode(child, source, searchQuery)}
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
  const codeNode = node.getChild("CodeText");
  const languageNode = node.getChild("CodeInfo");
  const code = codeNode ? source.slice(codeNode.from, codeNode.to) : "";
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
) => {
  const urlNode = node.getChild("URL");
  const href = urlNode
    ? safeExternalUrl(source.slice(urlNode.from, urlNode.to))
    : null;
  const label =
    node.name === "Autolink" && urlNode
      ? renderText(source.slice(urlNode.from, urlNode.to), searchQuery)
      : renderChildren(node, source, searchQuery, LINK_METADATA_NODES);

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

const renderImage = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
) => {
  const raw = source.slice(node.from, node.to);
  const alt = /^!\[([^\]]*)\]/.exec(raw)?.[1] ?? "";
  const urlNode = node.getChild("URL");
  const src = urlNode
    ? safeExternalUrl(source.slice(urlNode.from, urlNode.to))
    : null;

  if (!src) return renderText(alt, searchQuery);

  return (
    <img
      src={src}
      alt={alt}
      loading="lazy"
      referrerPolicy="no-referrer"
      className="my-2 max-h-96 max-w-full rounded-md border border-border/60 object-contain"
    />
  );
};

const renderTable = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
) => {
  const header = node.getChild("TableHeader");
  const rows = childNodes(node).filter((child) => child.name === "TableRow");

  return (
    <div className="my-2 max-w-full overflow-x-auto">
      <table className="w-full border-collapse text-left text-xs">
        {header && <thead>{renderNode(header, source, searchQuery)}</thead>}
        {rows.length > 0 && (
          <tbody>
            {rows.map((row) => (
              <Fragment key={row.from}>
                {renderNode(row, source, searchQuery)}
              </Fragment>
            ))}
          </tbody>
        )}
      </table>
    </div>
  );
};

const renderNode = (
  node: MarkdownNode,
  source: string,
  searchQuery?: string,
): ReactNode => {
  const children = () => renderChildren(node, source, searchQuery);

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
      return renderTable(node, source, searchQuery);
    case "TableHeader":
    case "TableRow":
      return <tr>{children()}</tr>;
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
      return renderLink(node, source, searchQuery);
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
  const tree = useMemo(() => markdownLanguage.parser.parse(content), [content]);
  const rendered = useMemo(
    () => renderNode(tree.topNode, content, searchQuery),
    [content, searchQuery, tree],
  );

  return (
    <div className="min-w-0 break-words text-sm leading-relaxed [overflow-wrap:anywhere]">
      {rendered}
    </div>
  );
});

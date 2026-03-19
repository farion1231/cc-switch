import { useState, useMemo, useCallback, useEffect } from "react";
import { Copy, User, Bot, Terminal, Settings, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { SessionMessage } from "@/types";
import { formatTimestamp, getRoleLabel, getRoleTone } from "./utils";

interface SessionMessageItemProps {
  message: SessionMessage;
  index: number;
  isActive: boolean;
  defaultCollapsed?: boolean;
  setRef: (el: HTMLDivElement | null) => void;
  onCopy: (content: string) => void;
}

const COLLAPSE_THRESHOLD = 600;

function getRoleIcon(role: string) {
  const normalized = role.toLowerCase();
  if (normalized === "user") return User;
  if (normalized === "assistant") return Bot;
  if (normalized === "tool") return Wrench;
  if (normalized === "system") return Settings;
  return Terminal;
}

function getRoleBg(role: string) {
  const normalized = role.toLowerCase();
  if (normalized === "user") return "bg-emerald-500/8 dark:bg-emerald-500/10";
  if (normalized === "assistant") return "bg-blue-500/8 dark:bg-blue-500/10";
  if (normalized === "tool") return "bg-purple-500/8 dark:bg-purple-500/10";
  if (normalized === "system") return "bg-amber-500/8 dark:bg-amber-500/10";
  return "bg-muted/40";
}

function getRoleBorder(role: string) {
  const normalized = role.toLowerCase();
  if (normalized === "user") return "border-emerald-500/20";
  if (normalized === "assistant") return "border-blue-500/20";
  if (normalized === "tool") return "border-purple-500/20";
  if (normalized === "system") return "border-amber-500/20";
  return "border-border/60";
}

function getRoleIconBg(role: string) {
  const normalized = role.toLowerCase();
  if (normalized === "user")
    return "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400";
  if (normalized === "assistant")
    return "bg-blue-500/15 text-blue-600 dark:text-blue-400";
  if (normalized === "tool")
    return "bg-purple-500/15 text-purple-600 dark:text-purple-400";
  if (normalized === "system")
    return "bg-amber-500/15 text-amber-600 dark:text-amber-400";
  return "bg-muted text-muted-foreground";
}

/** Lightweight code block with a copy button and language label */
function CodeBlock({
  children,
  className,
  onCopy,
}: {
  children: string;
  className?: string;
  onCopy: (text: string) => void;
}) {
  const language = className?.replace(/^language-/, "") ?? "";
  return (
    <div className="relative group/code my-3 rounded-lg overflow-hidden border border-border/60 bg-[hsl(var(--background))]">
      {language && (
        <div className="flex items-center justify-between px-3 py-1.5 border-b border-border/40 bg-muted/30">
          <span className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground/70">
            {language}
          </span>
          <Button
            variant="ghost"
            size="icon"
            className="size-5 opacity-0 group-hover/code:opacity-100 transition-opacity"
            onClick={() => onCopy(children)}
          >
            <Copy className="size-3" />
          </Button>
        </div>
      )}
      <pre className="overflow-x-auto p-3 text-[13px] leading-relaxed">
        <code className={cn("font-mono", className)}>{children}</code>
      </pre>
      {!language && (
        <Button
          variant="ghost"
          size="icon"
          className="absolute top-1.5 right-1.5 size-5 opacity-0 group-hover/code:opacity-100 transition-opacity"
          onClick={() => onCopy(children)}
        >
          <Copy className="size-3" />
        </Button>
      )}
    </div>
  );
}

export function SessionMessageItem({
  message,
  isActive,
  defaultCollapsed,
  setRef,
  onCopy,
}: SessionMessageItemProps) {
  const { t } = useTranslation();
  const isLong = message.content.length > COLLAPSE_THRESHOLD;
  const shouldDefaultCollapse =
    defaultCollapsed ??
    (message.role.toLowerCase() === "tool" ||
      message.role.toLowerCase() === "system");

  const [collapsed, setCollapsed] = useState(shouldDefaultCollapse && isLong);

  // Respond to parent's collapse-all toggle
  useEffect(() => {
    if (defaultCollapsed !== undefined && isLong) {
      setCollapsed(defaultCollapsed);
    }
  }, [defaultCollapsed, isLong]);

  const RoleIcon = getRoleIcon(message.role);

  const displayContent = useMemo(() => {
    if (collapsed) {
      return message.content.slice(0, 200) + "…";
    }
    return message.content;
  }, [collapsed, message.content]);

  const handleCopyCode = useCallback(
    (text: string) => {
      onCopy(text);
    },
    [onCopy],
  );

  return (
    <div
      ref={setRef}
      className={cn(
        "relative rounded-xl border transition-all min-w-0 overflow-hidden group",
        getRoleBg(message.role),
        getRoleBorder(message.role),
        isActive && "ring-2 ring-primary ring-offset-2 ring-offset-background",
      )}
    >
      {/* Header bar */}
      <div className="flex items-center gap-2 px-3 py-2">
        {/* Role icon */}
        <div
          className={cn(
            "size-6 rounded-md flex items-center justify-center shrink-0",
            getRoleIconBg(message.role),
          )}
        >
          <RoleIcon className="size-3.5" />
        </div>

        {/* Role label */}
        <span
          className={cn(
            "text-xs font-semibold tracking-wide uppercase",
            getRoleTone(message.role),
          )}
        >
          {getRoleLabel(message.role, t)}
        </span>

        {/* Timestamp */}
        {message.ts && (
          <span className="text-[11px] text-muted-foreground/60 ml-auto mr-8 tabular-nums">
            {formatTimestamp(message.ts)}
          </span>
        )}

        {/* Copy button */}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="absolute top-2 right-2 size-6 opacity-0 group-hover:opacity-100 transition-opacity"
              onClick={() => onCopy(message.content)}
            >
              <Copy className="size-3" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            {t("sessionManager.copyMessage", {
              defaultValue: "Copy Message",
            })}
          </TooltipContent>
        </Tooltip>
      </div>

      {/* Content */}
      <div
        className={cn(
          "px-3 text-sm leading-relaxed overflow-hidden",
          collapsed ? "max-h-[120px] relative" : "pb-3",
        )}
      >
        {collapsed && (
          <div className="absolute bottom-0 left-0 right-0 h-10 bg-gradient-to-t from-[hsl(var(--card))] to-transparent z-10 pointer-events-none" />
        )}
        <div className="session-markdown prose prose-sm dark:prose-invert max-w-none break-words overflow-hidden [word-break:break-word]">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            components={{
              code({ className, children, ...props }) {
                const codeString = String(children).replace(/\n$/, "");
                const isBlock = className || codeString.includes("\n");
                if (isBlock) {
                  return (
                    <CodeBlock className={className} onCopy={handleCopyCode}>
                      {codeString}
                    </CodeBlock>
                  );
                }
                return (
                  <code
                    className={cn(
                      "px-1.5 py-0.5 rounded bg-muted/60 font-mono text-[13px]",
                      className,
                    )}
                    {...props}
                  >
                    {children}
                  </code>
                );
              },
              pre({ children }) {
                return <>{children}</>;
              },
              a({ href, children }) {
                return (
                  <a
                    href={href}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-primary hover:underline break-all"
                  >
                    {children}
                  </a>
                );
              },
              table({ children }) {
                return (
                  <div className="overflow-x-auto my-3 rounded-lg border border-border/40">
                    <table className="w-full text-sm">{children}</table>
                  </div>
                );
              },
            }}
          >
            {displayContent}
          </ReactMarkdown>
        </div>
      </div>

      {/* Show more/less - outside the clipped area */}
      {isLong && (
        <button
          type="button"
          onClick={() => setCollapsed(!collapsed)}
          className="px-3 pb-2 text-xs text-primary hover:text-primary/80 font-medium"
        >
          {collapsed
            ? t("sessionManager.showMore", { defaultValue: "Show more" })
            : t("sessionManager.showLess", { defaultValue: "Show less" })}
        </button>
      )}
    </div>
  );
}

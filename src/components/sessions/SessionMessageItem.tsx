import { useState } from "react";
import { Copy, ChevronDown, ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
    Tooltip,
    TooltipContent,
    TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { SessionMessage } from "@/types";
import { formatTimestamp, getRoleLabel, getRoleTone } from "./utils";
import { MarkdownContent } from "./MarkdownContent";

const COLLAPSE_HEIGHT = 150;

interface SessionMessageItemProps {
    message: SessionMessage;
    index: number;
    isActive: boolean;
    setRef: (el: HTMLDivElement | null) => void;
    onCopy: (content: string) => void;
    renderMarkdown: boolean;
    defaultCollapsed?: boolean;
}

export function SessionMessageItem({
    message,
    isActive,
    setRef,
    onCopy,
    renderMarkdown,
    defaultCollapsed = true,
}: SessionMessageItemProps) {
    const { t } = useTranslation();
    const [collapsed, setCollapsed] = useState(defaultCollapsed);
    const isLong = message.content.length > 500;

    return (
        <div
            ref={setRef}
            className={cn(
                "rounded-lg border px-3 py-2.5 relative group transition-all min-w-0",
                message.role.toLowerCase() === "user"
                    ? "bg-primary/5 border-primary/20 ml-8"
                    : message.role.toLowerCase() === "assistant"
                        ? "bg-blue-500/5 border-blue-500/20 mr-8"
                        : "bg-muted/40 border-border/60 mr-8",
                isActive && "ring-2 ring-primary ring-offset-2",
            )}
        >
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
                        defaultValue: "复制内容",
                    })}
                </TooltipContent>
            </Tooltip>
            <div className="flex items-center justify-between text-xs mb-1.5 pr-6">
                <span className={cn("font-semibold", getRoleTone(message.role))}>
                    {getRoleLabel(message.role, t)}
                    {message.toolName && (
                        <span className="text-muted-foreground font-normal ml-1">
                            ({message.toolName})
                        </span>
                    )}
                </span>
                {message.ts && (
                    <span className="text-muted-foreground">
                        {formatTimestamp(message.ts)}
                    </span>
                )}
            </div>
            <div
                className={cn(
                    "relative",
                    isLong && collapsed && "overflow-hidden",
                )}
                style={isLong && collapsed ? { maxHeight: COLLAPSE_HEIGHT } : undefined}
            >
                {renderMarkdown ? (
                    <MarkdownContent content={message.content} />
                ) : (
                    <div className="whitespace-pre-wrap text-sm leading-relaxed break-words w-0 min-w-full overflow-x-auto">
                        {message.content}
                    </div>
                )}
                {isLong && collapsed && (
                    <div className="absolute bottom-0 left-0 right-0 h-12 bg-gradient-to-t from-background/90 to-transparent pointer-events-none" />
                )}
            </div>
            {isLong && (
                <div className="flex items-center gap-1 text-xs text-muted-foreground py-1">
                    <button
                        type="button"
                        className="flex items-center gap-1 hover:text-foreground transition-colors"
                        onClick={() => setCollapsed((v) => !v)}
                    >
                        {collapsed ? (
                            <>
                                <ChevronRight className="size-3" />
                                {t("sessionManager.expandMessage")}
                            </>
                        ) : (
                            <>
                                <ChevronDown className="size-3" />
                                {t("sessionManager.collapseMessage")}
                            </>
                        )}
                    </button>
                </div>
            )}
        </div>
    );
}

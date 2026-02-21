import { useEffect, useRef, useState } from "react";
import { ChevronRight, Clock, Pencil, RotateCcw } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
    Tooltip,
    TooltipContent,
    TooltipTrigger,
} from "@/components/ui/tooltip";
import {
    ContextMenu,
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { SessionMeta } from "@/types";
import {
    formatRelativeTime,
    formatSessionTitle,
    getProviderIconName,
    getProviderLabel,
    getSessionKey,
} from "./utils";

interface SessionItemProps {
    session: SessionMeta;
    isSelected: boolean;
    onSelect: (key: string) => void;
    alias?: string;
    onRename?: (sessionKey: string, newName: string | null) => void;
}

export function SessionItem({
    session,
    isSelected,
    onSelect,
    alias,
    onRename,
}: SessionItemProps) {
    const { t } = useTranslation();
    const defaultTitle = formatSessionTitle(session);
    const displayTitle = alias || defaultTitle;
    const lastActive = session.lastActiveAt || session.createdAt || undefined;
    const sessionKey = getSessionKey(session);

    const [isEditing, setIsEditing] = useState(false);
    const [editValue, setEditValue] = useState("");
    const inputRef = useRef<HTMLInputElement>(null);

    useEffect(() => {
        if (isEditing) {
            // 延迟 focus 以确保 input 已渲染
            requestAnimationFrame(() => {
                inputRef.current?.focus();
                inputRef.current?.select();
            });
        }
    }, [isEditing]);

    const handleStartRename = () => {
        setEditValue(displayTitle);
        setIsEditing(true);
    };

    const handleConfirmRename = () => {
        const trimmed = editValue.trim();
        setIsEditing(false);
        if (!trimmed || trimmed === defaultTitle) {
            // 清空或与默认名相同，删除别名
            onRename?.(sessionKey, null);
        } else if (trimmed !== alias) {
            onRename?.(sessionKey, trimmed);
        }
    };

    const handleResetName = () => {
        onRename?.(sessionKey, null);
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === "Enter") {
            e.preventDefault();
            handleConfirmRename();
        } else if (e.key === "Escape") {
            e.preventDefault();
            setIsEditing(false);
        }
    };

    return (
        <ContextMenu>
            <ContextMenuTrigger asChild>
                <button
                    type="button"
                    onClick={() => onSelect(sessionKey)}
                    className={cn(
                        "w-full text-left rounded-lg px-3 py-2.5 transition-all group",
                        isSelected
                            ? "bg-primary/10 border border-primary/30"
                            : "hover:bg-muted/60 border border-transparent",
                    )}
                >
                    <div className="flex items-center gap-2 mb-1">
                        <Tooltip>
                            <TooltipTrigger asChild>
                                <span className="shrink-0">
                                    <ProviderIcon
                                        icon={getProviderIconName(session.providerId)}
                                        name={session.providerId}
                                     size={18}
                                    />
                                </span>
                            </TooltipTrigger>
                            <TooltipContent>
                                {getProviderLabel(session.providerId, t)}
                            </TooltipContent>
                        </Tooltip>
                        {isEditing ? (
                            <input
                                ref={inputRef}
                                value={editValue}
                                onChange={(e) => setEditValue(e.target.value)}
                                onBlur={handleConfirmRename}
                                onKeyDown={handleKeyDown}
                                className="text-sm font-medium flex-1 min-w-0 bg-transparent border-b border-primary outline-none"
                                onClick={(e) => e.stopPropagation()}
                            />
                        ) : (
                            <span className="text-sm truncate flex-1">
                                <span className="font-medium">{displayTitle}</span>
                                {alias && (
                                    <span className="text-muted-foreground text-xs ml-1">
                                        ({defaultTitle})
                                    </span>
                                )}
                            </span>
                        )}
                        <ChevronRight
                            className={cn(
                                "size-4 text-muted-foreground/50 shrink-0 transition-transform",
                                isSelected && "text-primary rotate-90",
                            )}
                        />
                    </div>

                    <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
                        <Clock className="size-3" />
                        <span>
                            {lastActive
                                ? formatRelativeTime(lastActive, t)
                                : t("common.unknown")}
                        </span>
                    </div>
                </button>
            </ContextMenuTrigger>
            <ContextMenuContent>
                <ContextMenuItem onClick={handleStartRename}>
                    <Pencil className="size-3.5 mr-2" />
                    {t("sessionManager.rename")}
                </ContextMenuItem>
                {alias && (
                    <ContextMenuItem onClick={handleResetName}>
                        <RotateCcw className="size-3.5 mr-2" />
                        {t("sessionManager.resetName")}
                    </ContextMenuItem>
                )}
            </ContextMenuContent>
        </ContextMenu>
    );
}

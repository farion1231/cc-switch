import { useRef, useCallback } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

import type { SessionMessage } from "@/types";
import { SessionMessageItem } from "./SessionMessageItem";

interface VirtualMessageListProps {
    messages: SessionMessage[];
    activeMessageIndex: number | null;
    messageRefs: React.MutableRefObject<Map<number, HTMLDivElement>>;
    onCopy: (content: string) => void;
    renderMarkdown: boolean;
    defaultCollapsed: boolean;
    showMessageIndex: boolean;
}

const ESTIMATED_ITEM_HEIGHT = 120;
const GAP = 12;

export function VirtualMessageList({
    messages,
    activeMessageIndex,
    messageRefs,
    onCopy,
    renderMarkdown,
    defaultCollapsed,
    showMessageIndex,
}: VirtualMessageListProps) {
    const scrollRef = useRef<HTMLDivElement>(null);
    // 展开/收起前记录锚定信息
    const anchorRef = useRef<{
        el: HTMLDivElement;
        offset: number;
        isCollapsing: boolean;
    } | null>(null);

    const virtualizer = useVirtualizer({
        count: messages.length,
        getScrollElement: () => scrollRef.current,
        estimateSize: () => ESTIMATED_ITEM_HEIGHT,
        overscan: 5,
        gap: GAP,
    });

    const handleBeforeToggle = useCallback((el: HTMLDivElement | null, isCollapsing: boolean) => {
        if (!el || !scrollRef.current) return;
        const scrollRect = scrollRef.current.getBoundingClientRect();
        if (isCollapsing) {
            // 收起：锚定卡片底部相对视口的位置（下面不动，上面缩）
            const elBottom = el.getBoundingClientRect().bottom;
            anchorRef.current = {
                el,
                offset: elBottom - scrollRect.bottom,
                isCollapsing: true,
            };
        } else {
            // 展开：锚定卡片顶部相对视口的位置（上面不动，向下展开）
            const elTop = el.getBoundingClientRect().top;
            anchorRef.current = {
                el,
                offset: elTop - scrollRect.top,
                isCollapsing: false,
            };
        }
    }, []);

    const handleAfterToggle = useCallback(() => {
        const anchor = anchorRef.current;
        if (!anchor || !scrollRef.current) return;
        anchorRef.current = null;

        const { el, offset, isCollapsing } = anchor;
        const scrollEl = scrollRef.current;

        // 双 rAF 确保 virtualizer 的 ResizeObserver 完成重新布局
        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                if (!scrollEl) return;
                const scrollRect = scrollEl.getBoundingClientRect();
                if (isCollapsing) {
                    // 收起：让卡片底部保持在视口中的同一位置
                    const newBottom = el.getBoundingClientRect().bottom;
                    const currentOffset = newBottom - scrollRect.bottom;
                    scrollEl.scrollTop += currentOffset - offset;
                } else {
                    // 展开：让卡片顶部保持在视口中的同一位置
                    const newTop = el.getBoundingClientRect().top;
                    const currentOffset = newTop - scrollRect.top;
                    scrollEl.scrollTop += currentOffset - offset;
                }
            });
        });
    }, []);

    const virtualItems = virtualizer.getVirtualItems();

    const paddingTop = virtualItems.length > 0 ? virtualItems[0].start : 0;
    const paddingBottom = virtualItems.length > 0
        ? virtualizer.getTotalSize() - virtualItems[virtualItems.length - 1].end
        : 0;

    return (
        <div
            ref={scrollRef}
            className="h-full overflow-y-auto overflow-x-hidden"
            style={{ overflowAnchor: "none" }}
        >
            <div
                style={{
                    paddingTop,
                    paddingBottom,
                }}
            >
                {virtualItems.map((virtualItem) => {
                    const message = messages[virtualItem.index];
                    return (
                        <div
                            key={virtualItem.key}
                            data-index={virtualItem.index}
                            ref={virtualizer.measureElement}
                            className="px-4"
                            style={{ marginBottom: GAP }}
                        >
                            <SessionMessageItem
                                message={message}
                                index={virtualItem.index}
                                isActive={activeMessageIndex === virtualItem.index}
                                setRef={(el) => {
                                    if (el) messageRefs.current.set(virtualItem.index, el);
                                }}
                                onCopy={onCopy}
                                renderMarkdown={renderMarkdown}
                                defaultCollapsed={defaultCollapsed}
                                showMessageIndex={showMessageIndex}
                                onBeforeToggle={handleBeforeToggle}
                                onAfterToggle={handleAfterToggle}
                            />
                        </div>
                    );
                })}
            </div>
        </div>
    );
}

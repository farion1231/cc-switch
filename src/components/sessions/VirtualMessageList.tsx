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
}: VirtualMessageListProps) {
    const scrollRef = useRef<HTMLDivElement>(null);
    // 展开/收起前记录卡片顶部相对视口的偏移
    const anchorRef = useRef<{ el: HTMLDivElement; offsetTop: number } | null>(null);

    const virtualizer = useVirtualizer({
        count: messages.length,
        getScrollElement: () => scrollRef.current,
        estimateSize: () => ESTIMATED_ITEM_HEIGHT,
        overscan: 5,
        gap: GAP,
    });

    const handleBeforeToggle = useCallback((el: HTMLDivElement | null) => {
        if (!el || !scrollRef.current) return;
        // 记录卡片顶部相对于滚动容器视口顶部的偏移
        const scrollTop = scrollRef.current.scrollTop;
        const elTop = el.getBoundingClientRect().top
            - scrollRef.current.getBoundingClientRect().top
            + scrollTop;
        anchorRef.current = { el, offsetTop: elTop - scrollTop };
    }, []);

    const handleAfterToggle = useCallback(() => {
        const anchor = anchorRef.current;
        if (!anchor || !scrollRef.current) return;
        anchorRef.current = null;

        const el = anchor.el;
        const savedOffset = anchor.offsetTop;
        const scrollEl = scrollRef.current;

        // 用双 rAF 确保 virtualizer 的 ResizeObserver 也完成了重新布局
        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                if (!scrollEl) return;
                const newElTop = el.getBoundingClientRect().top
                    - scrollEl.getBoundingClientRect().top
                    + scrollEl.scrollTop;
                scrollEl.scrollTop = newElTop - savedOffset;
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

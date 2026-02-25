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

    const virtualizer = useVirtualizer({
        count: messages.length,
        getScrollElement: () => scrollRef.current,
        estimateSize: () => ESTIMATED_ITEM_HEIGHT,
        overscan: 5,
        gap: GAP,
    });

    // 展开/收起前记录当前 item 顶部相对于视口的偏移
    const handleBeforeToggle = useCallback(
        (index: number) => {
            const scrollEl = scrollRef.current;
            if (!scrollEl) return undefined;
            const item = virtualizer.getVirtualItems().find(
                (v) => v.index === index,
            );
            if (!item) return undefined;
            return item.start - scrollEl.scrollTop;
        },
        [virtualizer],
    );

    // 展开/收起后，根据之前记录的偏移修正 scrollTop，保持视觉位置不变
    const handleAfterToggle = useCallback(
        (index: number, offsetBefore: number | undefined) => {
            if (offsetBefore === undefined) return;
            requestAnimationFrame(() => {
                const scrollEl = scrollRef.current;
                if (!scrollEl) return;
                const item = virtualizer.getVirtualItems().find(
                    (v) => v.index === index,
                );
                if (!item) return;
                scrollEl.scrollTop = item.start - offsetBefore;
            });
        },
        [virtualizer],
    );

    return (
        <div
            ref={scrollRef}
            className="h-full overflow-y-auto overflow-x-hidden"
        >
            <div
                className="relative w-full"
                style={{ height: virtualizer.getTotalSize() }}
            >
                {virtualizer.getVirtualItems().map((virtualItem) => {
                    const message = messages[virtualItem.index];
                    return (
                        <div
                            key={virtualItem.key}
                            data-index={virtualItem.index}
                            ref={virtualizer.measureElement}
                            className="absolute left-0 right-0 px-4"
                            style={{ top: virtualItem.start }}
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
                                onBeforeToggle={() => handleBeforeToggle(virtualItem.index)}
                                onAfterToggle={(offset) => handleAfterToggle(virtualItem.index, offset)}
                            />
                        </div>
                    );
                })}
            </div>
        </div>
    );
}

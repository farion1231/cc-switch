import { memo, useRef, useState, useEffect } from "react";
import ReactMarkdown from "react-markdown";
import "./markdown.css";

interface MarkdownContentProps {
    content: string;
}

// 懒渲染：只在元素进入视口后才执行 Markdown 解析
export const MarkdownContent = memo(function MarkdownContent({
    content,
}: MarkdownContentProps) {
    const ref = useRef<HTMLDivElement>(null);
    const [visible, setVisible] = useState(false);

    useEffect(() => {
        const el = ref.current;
        if (!el) return;

        const observer = new IntersectionObserver(
            ([entry]) => {
                if (entry.isIntersecting) {
                    setVisible(true);
                    observer.disconnect();
                }
            },
            { rootMargin: "200px" },
        );
        observer.observe(el);
        return () => observer.disconnect();
    }, []);

    return (
        <div ref={ref} className="markdown-body text-sm leading-relaxed w-0 min-w-full overflow-x-auto">
            {visible ? (
                <ReactMarkdown>{content}</ReactMarkdown>
            ) : (
                <div className="whitespace-pre-wrap">{content}</div>
            )}
        </div>
    );
});

import React, { useRef, useEffect } from "react";
import { EditorView, basicSetup } from "codemirror";
import { markdown } from "@codemirror/lang-markdown";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorState } from "@codemirror/state";
import { placeholder as placeholderExt } from "@codemirror/view";

interface MarkdownEditorProps {
  value: string;
  onChange?: (value: string) => void;
  placeholder?: string;
  darkMode?: boolean;
  readOnly?: boolean;
  className?: string;
  minHeight?: string;
  maxHeight?: string;
}

const MarkdownEditor: React.FC<MarkdownEditorProps> = ({
  value,
  onChange,
  placeholder: placeholderText = "",
  darkMode = false,
  readOnly = false,
  className = "",
  minHeight = "300px",
  maxHeight,
}) => {
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);

  useEffect(() => {
    if (!editorRef.current) return;

    const editorTheme = EditorView.theme(
      {
        "&": {
          height: "100%",
          minHeight,
          maxHeight: maxHeight || "none",
        },
        ".cm-editor": {
          height: "100%",
          backgroundColor: "transparent",
          color: "hsl(var(--foreground))",
        },
        ".cm-scroller": {
          overflow: "auto",
          backgroundColor: "transparent",
          fontFamily:
            "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace",
          fontSize: "14px",
        },
        ".cm-content": {
          padding: "0.875rem 1rem",
          caretColor: "hsl(var(--primary))",
          lineHeight: "1.6",
        },
        ".cm-placeholder": {
          color: "hsl(var(--muted-foreground) / 0.85)",
        },
        ".cm-cursor, .cm-dropCursor": {
          borderLeftColor: "hsl(var(--primary))",
        },
        ".cm-gutters": {
          minHeight: "100%",
          backgroundColor: "hsl(var(--muted) / 0.65)",
          color: "hsl(var(--muted-foreground))",
          borderRight: "1px solid hsl(var(--border))",
        },
        ".cm-gutterElement": {
          padding: "0 0.75rem",
        },
        ".cm-activeLine": {
          backgroundColor: readOnly
            ? "transparent"
            : "hsl(var(--accent) / 0.28)",
        },
        ".cm-activeLineGutter": {
          backgroundColor: readOnly
            ? "transparent"
            : "hsl(var(--accent) / 0.4)",
          color: "hsl(var(--foreground))",
        },
        ".cm-selectionBackground, .cm-content ::selection": {
          backgroundColor: "hsl(var(--primary) / 0.18) !important",
        },
        "&.cm-focused": {
          outline: "none",
        },
      },
      { dark: darkMode },
    );

    const extensions = [
      basicSetup,
      markdown(),
      editorTheme,
      EditorView.lineWrapping,
      EditorState.readOnly.of(readOnly),
    ];

    if (!readOnly) {
      extensions.push(
        placeholderExt(placeholderText),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && onChange) {
            onChange(update.state.doc.toString());
          }
        }),
      );
    } else {
      // 只读模式下隐藏光标和高亮行
      extensions.push(
        EditorView.theme({
          ".cm-cursor, .cm-dropCursor": { border: "none" },
          ".cm-activeLine": { backgroundColor: "transparent !important" },
          ".cm-activeLineGutter": { backgroundColor: "transparent !important" },
        }),
      );
    }

    if (darkMode) {
      extensions.unshift(oneDark);
    }

    // 创建初始状态
    const state = EditorState.create({
      doc: value,
      extensions,
    });

    // 创建编辑器视图
    const view = new EditorView({
      state,
      parent: editorRef.current,
    });

    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, [darkMode, readOnly, minHeight, maxHeight, placeholderText]); // 添加 placeholderText 依赖以支持国际化切换

  // 当 value 从外部改变时更新编辑器内容
  useEffect(() => {
    if (viewRef.current && viewRef.current.state.doc.toString() !== value) {
      const transaction = viewRef.current.state.update({
        changes: {
          from: 0,
          to: viewRef.current.state.doc.length,
          insert: value,
        },
      });
      viewRef.current.dispatch(transaction);
    }
  }, [value]);

  return (
    <div
      ref={editorRef}
      className={`overflow-hidden rounded-[calc(var(--radius)+0.125rem)] border border-border/80 bg-card/85 shadow-sm ${className}`}
    />
  );
};

export default MarkdownEditor;

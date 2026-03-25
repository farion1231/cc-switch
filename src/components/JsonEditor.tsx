import React, { useRef, useEffect, useMemo } from "react";
import { EditorView, basicSetup } from "codemirror";
import { json } from "@codemirror/lang-json";
import { javascript } from "@codemirror/lang-javascript";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorState } from "@codemirror/state";
import { placeholder } from "@codemirror/view";
import { linter, Diagnostic } from "@codemirror/lint";
import { useTranslation } from "react-i18next";
import { Wand2 } from "lucide-react";
import { toast } from "sonner";
import { formatJSON } from "@/utils/formatters";

interface JsonEditorProps {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  darkMode?: boolean;
  rows?: number;
  showValidation?: boolean;
  language?: "json" | "javascript";
  height?: string | number;
  showMinimap?: boolean; // 添加此属性以防未来使用
}

const JsonEditor: React.FC<JsonEditorProps> = ({
  value,
  onChange,
  placeholder: placeholderText = "",
  darkMode = false,
  rows = 12,
  showValidation = true,
  language = "json",
  height,
}) => {
  const { t } = useTranslation();
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);

  // JSON linter 函数
  const jsonLinter = useMemo(
    () =>
      linter((view) => {
        const diagnostics: Diagnostic[] = [];
        if (!showValidation || language !== "json") return diagnostics;

        const doc = view.state.doc.toString();
        if (!doc.trim()) return diagnostics;

        try {
          const parsed = JSON.parse(doc);
          // 检查是否是JSON对象
          if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
            // 格式正确
          } else {
            diagnostics.push({
              from: 0,
              to: doc.length,
              severity: "error",
              message: t("jsonEditor.mustBeObject"),
            });
          }
        } catch (e) {
          // 简单处理JSON解析错误
          const message =
            e instanceof SyntaxError ? e.message : t("jsonEditor.invalidJson");
          diagnostics.push({
            from: 0,
            to: doc.length,
            severity: "error",
            message,
          });
        }

        return diagnostics;
      }),
    [showValidation, language, t],
  );

  useEffect(() => {
    if (!editorRef.current) return;

    // 创建编辑器扩展
    const minHeightPx = height ? undefined : Math.max(1, rows) * 18;

    // 使用 theme 定义尺寸和字体样式
    const heightValue = height
      ? typeof height === "number"
        ? `${height}px`
        : height
      : undefined;
    const editorTheme = EditorView.theme(
      {
        "&": heightValue
          ? { height: heightValue }
          : { minHeight: `${minHeightPx}px` },
        ".cm-editor": {
          height: "100%",
          backgroundColor: "transparent",
          color: "hsl(var(--foreground))",
        },
        ".cm-editor.cm-focused": {
          outline: "none",
        },
        ".cm-scroller": {
          overflow: "auto",
          backgroundColor: "transparent",
        },
        ".cm-content": {
          padding: "0.875rem 0",
          caretColor: "hsl(var(--primary))",
          fontFamily:
            "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace",
          fontSize: "14px",
        },
        ".cm-line": {
          padding: "0 1rem",
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
          backgroundColor: "hsl(var(--accent) / 0.28)",
        },
        ".cm-activeLineGutter": {
          backgroundColor: "hsl(var(--accent) / 0.4)",
          color: "hsl(var(--foreground))",
        },
        ".cm-selectionBackground, .cm-content ::selection": {
          backgroundColor: "hsl(var(--primary) / 0.18) !important",
        },
        ".cm-selectionMatch": {
          backgroundColor: "hsl(var(--primary) / 0.12)",
        },
        ".cm-matchingBracket": {
          backgroundColor: "hsl(var(--accent) / 0.45)",
          outline: "1px solid hsl(var(--border))",
          color: "hsl(var(--foreground))",
        },
        ".cm-nonmatchingBracket": {
          color: "hsl(var(--destructive))",
        },
        ".cm-panels, .cm-tooltip": {
          backgroundColor: "hsl(var(--popover))",
          color: "hsl(var(--popover-foreground))",
          border: "1px solid hsl(var(--border))",
        },
        ".cm-diagnosticText": {
          color: "hsl(var(--destructive))",
        },
      },
      { dark: darkMode },
    );
    const sizingTheme = EditorView.theme({
      ".cm-content": {
        lineHeight: "1.6",
      },
    });

    const extensions = [
      basicSetup,
      language === "javascript" ? javascript() : json(),
      placeholder(placeholderText || ""),
      editorTheme,
      sizingTheme,
      jsonLinter,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          const newValue = update.state.doc.toString();
          onChange(newValue);
        }
      }),
    ];

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

    // 清理函数
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, [darkMode, rows, height, language, jsonLinter, placeholderText]);

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

  // 格式化处理函数
  const handleFormat = () => {
    if (!viewRef.current) return;

    const currentValue = viewRef.current.state.doc.toString();
    if (!currentValue.trim()) return;

    try {
      const formatted = formatJSON(currentValue);
      onChange(formatted);
      toast.success(t("common.formatSuccess", { defaultValue: "格式化成功" }), {
        closeButton: true,
      });
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      toast.error(
        t("common.formatError", {
          defaultValue: "格式化失败：{{error}}",
          error: errorMessage,
        }),
      );
    }
  };

  const isFullHeight = height === "100%";

  return (
    <div
      style={{ width: "100%", height: isFullHeight ? "100%" : "auto" }}
      className={isFullHeight ? "flex flex-col" : ""}
    >
      <div
        className={`overflow-hidden rounded-[calc(var(--radius)+0.125rem)] border border-border/80 bg-card/85 shadow-sm ${
          isFullHeight ? "flex min-h-0 flex-1 flex-col" : ""
        }`}
      >
        <div
          ref={editorRef}
          style={{ width: "100%", height: isFullHeight ? undefined : "auto" }}
          className={isFullHeight ? "min-h-0 flex-1" : ""}
        />
        {language === "json" && (
          <div className="flex items-center justify-end border-t border-border/70 bg-muted/35 px-3 py-2">
            <button
              type="button"
              onClick={handleFormat}
              className="inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent/70 hover:text-accent-foreground"
            >
              <Wand2 className="h-3.5 w-3.5" />
              {t("common.format", { defaultValue: "格式化" })}
            </button>
          </div>
        )}
      </div>
    </div>
  );
};

export default JsonEditor;

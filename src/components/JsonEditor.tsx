import React, { useRef, useEffect, useMemo } from "react";
import { EditorView, basicSetup } from "codemirror";
import { json } from "@codemirror/lang-json";
import { javascript } from "@codemirror/lang-javascript";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorSelection, EditorState } from "@codemirror/state";
import { placeholder } from "@codemirror/view";
import { linter, Diagnostic } from "@codemirror/lint";
import { useTranslation } from "react-i18next";
import { Wand2 } from "lucide-react";
import { toast } from "sonner";
import { formatJSON } from "@/utils/formatters";

interface JsonEditorProps {
  id?: string;
  ariaLabel?: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  darkMode?: boolean;
  rows?: number;
  showValidation?: boolean;
  language?: "json" | "javascript";
  height?: string | number;
  showMinimap?: boolean; // 添加此属性以防未来使用
  readOnly?: boolean;
}

function minimalTextChange(current: string, next: string) {
  let from = 0;
  const sharedLength = Math.min(current.length, next.length);
  while (from < sharedLength && current[from] === next[from]) from += 1;

  let currentTo = current.length;
  let nextTo = next.length;
  while (
    currentTo > from &&
    nextTo > from &&
    current[currentTo - 1] === next[nextTo - 1]
  ) {
    currentTo -= 1;
    nextTo -= 1;
  }

  return {
    from,
    to: currentTo,
    insert: next.slice(from, nextTo),
  };
}

function uniqueContextIndex(value: string, context: string): number | null {
  if (!context) return null;
  const index = value.indexOf(context);
  if (index < 0 || value.indexOf(context, index + 1) >= 0) return null;
  return index;
}

function mapPositionByContext(
  current: string,
  next: string,
  position: number,
): number | null {
  if (position <= 0) return 0;
  if (position >= current.length) return next.length;

  const maxContextLength = 96;
  const rightLimit = Math.min(maxContextLength, current.length - position);
  for (let length = rightLimit; length > 0; length -= 1) {
    const context = current.slice(position, position + length);
    const index = uniqueContextIndex(next, context);
    if (index !== null) return index;
  }

  const leftLimit = Math.min(maxContextLength, position);
  for (let length = leftLimit; length > 0; length -= 1) {
    const context = current.slice(position - length, position);
    const index = uniqueContextIndex(next, context);
    if (index !== null) return index + length;
  }

  return null;
}

const JsonEditor: React.FC<JsonEditorProps> = ({
  id,
  ariaLabel,
  value,
  onChange,
  placeholder: placeholderText = "",
  darkMode = false,
  rows = 3,
  showValidation = true,
  language = "json",
  height,
  readOnly = false,
}) => {
  const { t } = useTranslation();
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const hasExplicitHeight = height !== undefined;
  const heightValue =
    typeof height === "number" ? `${height}px` : (height ?? undefined);

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
    const minHeightPx = hasExplicitHeight ? undefined : Math.max(1, rows) * 18;

    // 使用 baseTheme 定义基础样式，优先级低于 oneDark，但可以正确响应主题
    const baseTheme = EditorView.baseTheme({
      ".cm-editor": {
        border: "1px solid hsl(var(--border))",
        borderRadius: "0.5rem",
        background: "transparent",
      },
      ".cm-editor.cm-focused": {
        outline: "none",
        borderColor: "hsl(var(--primary))",
      },
      ".cm-scroller": {
        background: "transparent",
      },
      ".cm-gutters": {
        background: "transparent",
        borderRight: "1px solid hsl(var(--border))",
        color: "hsl(var(--muted-foreground))",
      },
      ".cm-selectionBackground, .cm-content ::selection": {
        background: "hsl(var(--primary) / 0.18)",
      },
      ".cm-selectionMatch": {
        background: "hsl(var(--primary) / 0.12)",
      },
      ".cm-activeLine": {
        background: "hsl(var(--primary) / 0.08)",
      },
      ".cm-activeLineGutter": {
        background: "hsl(var(--primary) / 0.08)",
      },
    });

    // 使用 theme 定义尺寸和字体样式
    const sizingTheme = EditorView.theme({
      "&": hasExplicitHeight
        ? { height: "100%" }
        : { minHeight: `${minHeightPx}px` },
      ".cm-scroller": { overflow: "auto" },
      ".cm-content": {
        fontFamily:
          "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace",
        fontSize: "14px",
      },
    });

    const extensions = [
      basicSetup,
      language === "javascript" ? javascript() : json(),
      placeholder(placeholderText || ""),
      baseTheme,
      sizingTheme,
      jsonLinter,
      EditorState.readOnly.of(readOnly),
      EditorView.editable.of(!readOnly),
      EditorView.contentAttributes.of(
        ariaLabel ? { "aria-label": ariaLabel } : {},
      ),
      EditorView.updateListener.of((update) => {
        if (!readOnly && update.docChanged) {
          const newValue = update.state.doc.toString();
          onChangeRef.current(newValue);
        }
      }),
    ];

    // 如果启用深色模式，添加深色主题
    if (darkMode) {
      extensions.push(oneDark);
      // 在 oneDark 之后强制覆盖边框样式
      extensions.push(
        EditorView.theme({
          ".cm-editor": {
            border: "1px solid hsl(var(--border))",
            borderRadius: "0.5rem",
            background: "transparent",
          },
          ".cm-editor.cm-focused": {
            outline: "none",
            borderColor: "hsl(var(--primary))",
          },
          ".cm-scroller": {
            background: "transparent",
          },
          ".cm-gutters": {
            background: "transparent",
            borderRight: "1px solid hsl(var(--border))",
            color: "hsl(var(--muted-foreground))",
          },
          ".cm-selectionBackground, .cm-content ::selection": {
            background: "hsl(var(--primary) / 0.18)",
          },
          ".cm-selectionMatch": {
            background: "hsl(var(--primary) / 0.12)",
          },
          ".cm-activeLine": {
            background: "hsl(var(--primary) / 0.08)",
          },
          ".cm-activeLineGutter": {
            background: "hsl(var(--primary) / 0.08)",
          },
        }),
      );
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
  }, [
    ariaLabel,
    darkMode,
    hasExplicitHeight,
    jsonLinter,
    language,
    placeholderText,
    readOnly,
    rows,
  ]);

  // 当 value 从外部改变时更新编辑器内容
  useEffect(() => {
    if (viewRef.current) {
      const currentValue = viewRef.current.state.doc.toString();
      if (currentValue === value) return;
      const changes = viewRef.current.state.changes(
        minimalTextChange(currentValue, value),
      );
      const mappedSelection = viewRef.current.state.selection.map(changes, 1);
      const contextualRanges = viewRef.current.state.selection.ranges.map(
        (range, index) => {
          const fallback = mappedSelection.ranges[index];
          const anchor =
            mapPositionByContext(currentValue, value, range.anchor) ??
            fallback.anchor;
          const head =
            mapPositionByContext(currentValue, value, range.head) ??
            fallback.head;
          return range.empty
            ? EditorSelection.cursor(head)
            : EditorSelection.range(anchor, head);
        },
      );
      const transaction = viewRef.current.state.update({
        // Preserve the unchanged prefix and suffix so CodeMirror can map the
        // current selection through automatic JSON normalization.
        changes,
        selection: EditorSelection.create(
          contextualRanges,
          viewRef.current.state.selection.mainIndex,
        ),
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
      onChangeRef.current(formatted);
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
        id={id}
        ref={editorRef}
        tabIndex={-1}
        onFocus={(event) => {
          if (event.target === event.currentTarget) {
            viewRef.current?.focus();
          }
        }}
        style={{
          width: "100%",
          height: isFullHeight ? undefined : (heightValue ?? "auto"),
        }}
        className={isFullHeight ? "flex-1 min-h-0" : ""}
      />
      {language === "json" && !readOnly && (
        <button
          type="button"
          onClick={handleFormat}
          className={`${isFullHeight ? "mt-2 flex-shrink-0" : "mt-2"} inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-gray-700 dark:text-gray-300 hover:text-blue-600 dark:hover:text-blue-400 transition-colors`}
        >
          <Wand2 className="w-3.5 h-3.5" />
          {t("common.format", { defaultValue: "格式化" })}
        </button>
      )}
    </div>
  );
};

export default JsonEditor;

import React, { useRef, useEffect, useMemo } from "react";
import { EditorView, basicSetup } from "codemirror";
import { json } from "@codemirror/lang-json";
import { javascript } from "@codemirror/lang-javascript";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorState } from "@codemirror/state";
import { placeholder } from "@codemirror/view";
import { linter, Diagnostic } from "@codemirror/lint";
import { useTranslation } from "react-i18next";

interface JsonEditorProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  darkMode?: boolean;
  rows?: number;
  showValidation?: boolean;
  language?: "json" | "javascript";
  height?: string;
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
    const sizingTheme = EditorView.theme({
      "&": height ? { height } : { minHeight: `${minHeightPx}px` },
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
      sizingTheme,
      jsonLinter,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          const newValue = update.state.doc.toString();
          onChange(newValue);
        }
      }),
    ];

    // 如果启用深色模式，添加深色主题
    if (darkMode) {
      extensions.push(oneDark);
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
  }, [darkMode, rows, height, language, jsonLinter]); // 依赖项中不包含 onChange 和 placeholder，避免不必要的重建

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

  return <div ref={editorRef} style={{ width: "100%" }} />;
};

export default JsonEditor;

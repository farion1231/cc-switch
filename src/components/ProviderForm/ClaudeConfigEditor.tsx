import React, { useEffect, useState } from "react";
import JsonEditor from "../JsonEditor";
import { X, Save } from "lucide-react";
import { isLinux } from "../../lib/platform";

interface ClaudeConfigEditorProps {
  value: string;
  onChange: (value: string) => void;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  commonConfigSnippet: string;
  onCommonConfigSnippetChange: (value: string) => void;
  commonConfigError: string;
  configError: string;
}

const ClaudeConfigEditor: React.FC<ClaudeConfigEditorProps> = ({
  value,
  onChange,
  useCommonConfig,
  onCommonConfigToggle,
  commonConfigSnippet,
  onCommonConfigSnippetChange,
  commonConfigError,
  configError,
}) => {
  const [isDarkMode, setIsDarkMode] = useState(false);
  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);

  useEffect(() => {
    // 检测暗色模式
    const checkDarkMode = () => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    };

    checkDarkMode();

    // 监听暗色模式变化
    const observer = new MutationObserver((mutations) => {
      mutations.forEach((mutation) => {
        if (mutation.attributeName === "class") {
          checkDarkMode();
        }
      });
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (commonConfigError && !isCommonConfigModalOpen) {
      setIsCommonConfigModalOpen(true);
    }
  }, [commonConfigError, isCommonConfigModalOpen]);

  // 支持按下 ESC 关闭弹窗
  useEffect(() => {
    if (!isCommonConfigModalOpen) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closeModal();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isCommonConfigModalOpen]);

  const closeModal = () => {
    setIsCommonConfigModalOpen(false);
  };
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <label
          htmlFor="settingsConfig"
          className="block text-sm font-medium text-gray-900 dark:text-gray-100"
        >
          Claude Code 配置 (JSON) *
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400 cursor-pointer">
          <input
            type="checkbox"
            checked={useCommonConfig}
            onChange={(e) => onCommonConfigToggle(e.target.checked)}
            className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-gray-200 dark:border-gray-700 rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
          />
          写入通用配置
        </label>
      </div>
      <div className="flex items-center justify-end">
        <button
          type="button"
          onClick={() => setIsCommonConfigModalOpen(true)}
          className="text-xs text-blue-500 dark:text-blue-400 hover:underline"
        >
          编辑通用配置
        </button>
      </div>
      {commonConfigError && !isCommonConfigModalOpen && (
        <p className="text-xs text-red-500 dark:text-red-400 text-right">
          {commonConfigError}
        </p>
      )}
      <JsonEditor
        value={value}
        onChange={onChange}
        darkMode={isDarkMode}
        placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://your-api-endpoint.com",
    "ANTHROPIC_AUTH_TOKEN": "your-api-key-here"
  }
}`}
        rows={12}
      />
      {configError && (
        <p className="text-xs text-red-500 dark:text-red-400">{configError}</p>
      )}
      <p className="text-xs text-gray-500 dark:text-gray-400">
        完整的 Claude Code settings.json 配置内容
      </p>
      {isCommonConfigModalOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) closeModal();
          }}
        >
          {/* Backdrop - 统一背景样式 */}
          <div
            className={`absolute inset-0 bg-black/50 dark:bg-black/70${
              isLinux() ? "" : " backdrop-blur-sm"
            }`}
          />

          {/* Modal - 统一窗口样式 */}
          <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-lg max-w-2xl w-full mx-4 max-h-[90vh] overflow-hidden flex flex-col">
            {/* Header - 统一标题栏样式 */}
            <div className="flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-800">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
                编辑通用配置片段
              </h2>
              <button
                type="button"
                onClick={closeModal}
                className="p-1 text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
                aria-label="关闭"
              >
                <X size={18} />
              </button>
            </div>

            {/* Content - 统一内容区域样式 */}
            <div className="flex-1 overflow-auto p-6 space-y-4">
              <p className="text-sm text-gray-500 dark:text-gray-400">
                该片段会在勾选"写入通用配置"时合并到 settings.json 中
              </p>
              <JsonEditor
                value={commonConfigSnippet}
                onChange={onCommonConfigSnippetChange}
                darkMode={isDarkMode}
                rows={12}
              />
              {commonConfigError && (
                <p className="text-sm text-red-500 dark:text-red-400">
                  {commonConfigError}
                </p>
              )}
            </div>

            {/* Footer - 统一底部按钮样式 */}
            <div className="flex items-center justify-end gap-3 p-6 border-t border-gray-200 dark:border-gray-800 bg-gray-100 dark:bg-gray-800">
              <button
                type="button"
                onClick={closeModal}
                className="px-4 py-2 text-sm font-medium text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-white dark:hover:bg-gray-700 rounded-lg transition-colors"
              >
                取消
              </button>
              <button
                type="button"
                onClick={closeModal}
                className="px-4 py-2 bg-blue-500 dark:bg-blue-600 text-white rounded-lg hover:bg-blue-600 dark:hover:bg-blue-700 transition-colors text-sm font-medium flex items-center gap-2"
              >
                <Save className="w-4 h-4" />
                保存
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default ClaudeConfigEditor;

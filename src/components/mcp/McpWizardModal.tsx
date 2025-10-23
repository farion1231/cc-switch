import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Save } from "lucide-react";
import { McpServerSpec } from "../../types";
import { isLinux } from "../../lib/platform";

interface McpWizardModalProps {
  isOpen: boolean;
  onClose: () => void;
  onApply: (title: string, json: string) => void;
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number,
  ) => void;
  initialTitle?: string;
  initialServer?: McpServerSpec;
}

/**
 * 解析环境变量文本为对象
 */
const parseEnvText = (text: string): Record<string, string> => {
  const lines = text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  const env: Record<string, string> = {};
  for (const l of lines) {
    const idx = l.indexOf("=");
    if (idx > 0) {
      const k = l.slice(0, idx).trim();
      const v = l.slice(idx + 1).trim();
      if (k) env[k] = v;
    }
  }
  return env;
};

/**
 * 解析headers文本为对象（支持 KEY: VALUE 或 KEY=VALUE）
 */
const parseHeadersText = (text: string): Record<string, string> => {
  const lines = text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  const headers: Record<string, string> = {};
  for (const l of lines) {
    // 支持 KEY: VALUE 或 KEY=VALUE
    const colonIdx = l.indexOf(":");
    const equalIdx = l.indexOf("=");
    let idx = -1;
    if (colonIdx > 0 && (equalIdx === -1 || colonIdx < equalIdx)) {
      idx = colonIdx;
    } else if (equalIdx > 0) {
      idx = equalIdx;
    }
    if (idx > 0) {
      const k = l.slice(0, idx).trim();
      const v = l.slice(idx + 1).trim();
      if (k) headers[k] = v;
    }
  }
  return headers;
};

/**
 * MCP 配置向导模态框
 * 帮助用户快速生成 MCP JSON 配置
 */
const McpWizardModal: React.FC<McpWizardModalProps> = ({
  isOpen,
  onClose,
  onApply,
  onNotify,
  initialTitle,
  initialServer,
}) => {
  const { t } = useTranslation();
  const [wizardType, setWizardType] = useState<"stdio" | "http">("stdio");
  const [wizardTitle, setWizardTitle] = useState("");
  // stdio 字段
  const [wizardCommand, setWizardCommand] = useState("");
  const [wizardArgs, setWizardArgs] = useState("");
  const [wizardEnv, setWizardEnv] = useState("");
  const [wizardStartupTimeout, setWizardStartupTimeout] = useState("");
  // http 字段
  const [wizardUrl, setWizardUrl] = useState("");
  const [wizardHeaders, setWizardHeaders] = useState("");

  // 生成预览 JSON
  const generatePreview = (): string => {
    const config: McpServerSpec = {
      type: wizardType,
    };

    if (wizardType === "stdio") {
      // stdio 类型必需字段
      config.command = wizardCommand.trim();

      // 可选字段
      if (wizardArgs.trim()) {
        config.args = wizardArgs
          .split("\n")
          .map((s) => s.trim())
          .filter((s) => s.length > 0);
      }

      if (wizardEnv.trim()) {
        const env = parseEnvText(wizardEnv);
        if (Object.keys(env).length > 0) {
          config.env = env;
        }
      }

      const timeoutText = wizardStartupTimeout.trim();
      if (timeoutText) {
        const timeoutValue = Number(timeoutText);
        if (Number.isInteger(timeoutValue) && timeoutValue >= 0) {
          config.startup_timeout_ms = timeoutValue;
        }
      }
    } else {
      // http 类型必需字段
      config.url = wizardUrl.trim();

      // 可选字段
      if (wizardHeaders.trim()) {
        const headers = parseHeadersText(wizardHeaders);
        if (Object.keys(headers).length > 0) {
          config.headers = headers;
        }
      }
    }

    return JSON.stringify(config, null, 2);
  };

  const handleApply = () => {
    if (!wizardTitle.trim()) {
      onNotify?.(t("mcp.error.idRequired"), "error", 3000);
      return;
    }
    if (wizardType === "stdio" && !wizardCommand.trim()) {
      onNotify?.(t("mcp.error.commandRequired"), "error", 3000);
      return;
    }
    if (wizardType === "http" && !wizardUrl.trim()) {
      onNotify?.(t("mcp.wizard.urlRequired"), "error", 3000);
      return;
    }

    const json = generatePreview();
    onApply(wizardTitle.trim(), json);
    handleClose();
  };

  const handleClose = () => {
    // 重置表单
    setWizardType("stdio");
    setWizardTitle("");
    setWizardCommand("");
    setWizardArgs("");
    setWizardEnv("");
    setWizardStartupTimeout("");
    setWizardUrl("");
    setWizardHeaders("");
    onClose();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      handleApply();
    }
  };

  useEffect(() => {
    if (!isOpen) return;

    const title = initialTitle ?? "";
    setWizardTitle(title);

    const resolvedType =
      initialServer?.type ??
      (initialServer?.url ? "http" : "stdio");

    setWizardType(resolvedType);

    if (resolvedType === "http") {
      setWizardUrl(initialServer?.url ?? "");
      const headersCandidate = initialServer?.headers;
      const headers =
        headersCandidate && typeof headersCandidate === "object"
          ? headersCandidate
          : undefined;
      setWizardHeaders(
        headers
          ? Object.entries(headers)
              .map(([k, v]) => `${k}: ${v ?? ""}`)
              .join("\n")
          : "",
      );
      setWizardCommand("");
      setWizardArgs("");
      setWizardEnv("");
      setWizardStartupTimeout("");
      return;
    }

    setWizardCommand(initialServer?.command ?? "");
    const argsValue = initialServer?.args;
    setWizardArgs(Array.isArray(argsValue) ? argsValue.join("\n") : "");
    const envCandidate = initialServer?.env;
    const env =
      envCandidate && typeof envCandidate === "object" ? envCandidate : undefined;
    setWizardEnv(
      env
        ? Object.entries(env)
            .map(([k, v]) => `${k}=${v ?? ""}`)
            .join("\n")
        : "",
    );
    const startupTimeoutValue = initialServer?.startup_timeout_ms;
    setWizardStartupTimeout(
      typeof startupTimeoutValue === "number" && Number.isFinite(startupTimeoutValue)
        ? String(startupTimeoutValue)
        : ""
    );
    setWizardUrl("");
    setWizardHeaders("");
  }, [isOpen]);

  if (!isOpen) return null;

  const preview = generatePreview();

  return (
    <div
      className="fixed inset-0 z-[70] flex items-center justify-center"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) {
          handleClose();
        }
      }}
    >
      {/* Backdrop */}
      <div
        className={`absolute inset-0 bg-black/50 dark:bg-black/70${
          isLinux() ? "" : " backdrop-blur-sm"
        }`}
      />

      {/* Modal */}
      <div className="relative mx-4 flex max-h-[90vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl bg-white shadow-lg dark:bg-gray-900">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-gray-200 p-6 dark:border-gray-800">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
            {t("mcp.wizard.title")}
          </h2>
          <button
            type="button"
            onClick={handleClose}
            className="rounded-md p-1 text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-900 dark:text-gray-400 dark:hover:bg-gray-800 dark:hover:text-gray-100"
            aria-label={t("common.close")}
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 min-h-0 space-y-4 overflow-auto p-6">
          {/* Hint */}
          <div className="rounded-lg border border-blue-200 bg-blue-50 p-3 dark:border-blue-800 dark:bg-blue-900/20">
            <p className="text-sm text-blue-800 dark:text-blue-200">
              {t("mcp.wizard.hint")}
            </p>
          </div>

          {/* Form Fields */}
          <div className="space-y-4 min-h-[400px]">
            {/* Type */}
            <div>
              <label className="mb-2 block text-sm font-medium text-gray-900 dark:text-gray-100">
                {t("mcp.wizard.type")} <span className="text-red-500">*</span>
              </label>
              <div className="flex gap-4">
                <label className="inline-flex items-center gap-2 cursor-pointer">
                  <input
                    type="radio"
                    value="stdio"
                    checked={wizardType === "stdio"}
                    onChange={(e) =>
                      setWizardType(e.target.value as "stdio" | "http")
                    }
                    className="w-4 h-4 text-emerald-500 bg-white dark:bg-gray-800 border-gray-200 dark:border-gray-700 focus:ring-emerald-500 dark:focus:ring-emerald-400 focus:ring-2"
                  />
                  <span className="text-sm text-gray-900 dark:text-gray-100">
                    {t("mcp.wizard.typeStdio")}
                  </span>
                </label>
                <label className="inline-flex items-center gap-2 cursor-pointer">
                  <input
                    type="radio"
                    value="http"
                    checked={wizardType === "http"}
                    onChange={(e) =>
                      setWizardType(e.target.value as "stdio" | "http")
                    }
                    className="w-4 h-4 text-emerald-500 bg-white dark:bg-gray-800 border-gray-200 dark:border-gray-700 focus:ring-emerald-500 dark:focus:ring-emerald-400 focus:ring-2"
                  />
                  <span className="text-sm text-gray-900 dark:text-gray-100">
                    {t("mcp.wizard.typeHttp")}
                  </span>
                </label>
              </div>
            </div>

            {/* Title */}
            <div>
              <label className="mb-1 block text-sm font-medium text-gray-900 dark:text-gray-100">
                {t("mcp.form.title")} <span className="text-red-500">*</span>
              </label>
              <input
                type="text"
                value={wizardTitle}
                onChange={(e) => setWizardTitle(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t("mcp.form.titlePlaceholder")}
                className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
              />
            </div>

            {/* Stdio 类型字段 */}
            {wizardType === "stdio" && (
              <>
                {/* Command */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-gray-900 dark:text-gray-100">
                    {t("mcp.wizard.command")}{" "}
                    <span className="text-red-500">*</span>
                  </label>
                  <input
                    type="text"
                    value={wizardCommand}
                    onChange={(e) => setWizardCommand(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={t("mcp.wizard.commandPlaceholder")}
                    className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                  />
                </div>

                {/* Args */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-gray-900 dark:text-gray-100">
                    {t("mcp.wizard.args")}
                  </label>
                  <textarea
                    value={wizardArgs}
                    onChange={(e) => setWizardArgs(e.target.value)}
                    placeholder={t("mcp.wizard.argsPlaceholder")}
                    rows={3}
                    className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 resize-y"
                  />
                </div>

                {/* Env */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-gray-900 dark:text-gray-100">
                    {t("mcp.wizard.env")}
                  </label>
                  <textarea
                    value={wizardEnv}
                    onChange={(e) => setWizardEnv(e.target.value)}
                    placeholder={t("mcp.wizard.envPlaceholder")}
                    rows={3}
                    className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 resize-y"
                  />
                </div>

                {/* Startup Timeout */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-gray-900 dark:text-gray-100">
                    {t("mcp.wizard.startupTimeout")}
                  </label>
                  <input
                    type="number"
                    min={0}
                    step={100}
                    value={wizardStartupTimeout}
                    onChange={(e) => setWizardStartupTimeout(e.target.value)}
                    placeholder={t("mcp.wizard.startupTimeoutPlaceholder")}
                    className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                  />
                  <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                    {t("mcp.wizard.startupTimeoutHint")}
                  </p>
                </div>
              </>
            )}

            {/* HTTP 类型字段 */}
            {wizardType === "http" && (
              <>
                {/* URL */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-gray-900 dark:text-gray-100">
                    {t("mcp.wizard.url")}{" "}
                    <span className="text-red-500">*</span>
                  </label>
                  <input
                    type="text"
                    value={wizardUrl}
                    onChange={(e) => setWizardUrl(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={t("mcp.wizard.urlPlaceholder")}
                    className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                  />
                </div>

                {/* Headers */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-gray-900 dark:text-gray-100">
                    {t("mcp.wizard.headers")}
                  </label>
                  <textarea
                    value={wizardHeaders}
                    onChange={(e) => setWizardHeaders(e.target.value)}
                    placeholder={t("mcp.wizard.headersPlaceholder")}
                    rows={3}
                    className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 resize-y"
                  />
                </div>
              </>
            )}
          </div>

          {/* Preview */}
          {(wizardCommand ||
            wizardArgs ||
            wizardEnv ||
            wizardStartupTimeout ||
            wizardUrl ||
            wizardHeaders) && (
            <div className="space-y-2 border-t border-gray-200 pt-4 dark:border-gray-700">
              <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">
                {t("mcp.wizard.preview")}
              </h3>
              <pre className="overflow-x-auto rounded-lg bg-gray-50 p-3 text-xs font-mono text-gray-700 dark:bg-gray-800 dark:text-gray-300">
                {preview}
              </pre>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 border-t border-gray-200 bg-gray-100 p-6 dark:border-gray-800 dark:bg-gray-800">
          <button
            type="button"
            onClick={handleClose}
            className="rounded-lg px-4 py-2 text-sm font-medium text-gray-500 transition-colors hover:bg-white hover:text-gray-900 dark:text-gray-400 dark:hover:bg-gray-700 dark:hover:text-gray-100"
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            onClick={handleApply}
            className="flex items-center gap-2 rounded-lg bg-emerald-500 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700"
          >
            <Save className="h-4 w-4" />
            {t("mcp.wizard.apply")}
          </button>
        </div>
      </div>
    </div>
  );
};

export default McpWizardModal;

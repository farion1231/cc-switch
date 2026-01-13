import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { McpServerSpec } from "@/types";

interface McpWizardModalProps {
  isOpen: boolean;
  onClose: () => void;
  onApply: (title: string, json: string) => void;
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
  initialTitle,
  initialServer,
}) => {
  const { t } = useTranslation();
  const [wizardType, setWizardType] = useState<"stdio" | "http" | "sse">(
    "stdio",
  );
  const [wizardTitle, setWizardTitle] = useState("");
  // stdio 字段
  const [wizardCommand, setWizardCommand] = useState("");
  const [wizardArgs, setWizardArgs] = useState("");
  const [wizardEnv, setWizardEnv] = useState("");
  // http 和 sse 字段
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
    } else {
      // http 和 sse 类型必需字段
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
      toast.error(t("mcp.error.idRequired"), { duration: 3000 });
      return;
    }
    if (wizardType === "stdio" && !wizardCommand.trim()) {
      toast.error(t("mcp.error.commandRequired"), { duration: 3000 });
      return;
    }
    if ((wizardType === "http" || wizardType === "sse") && !wizardUrl.trim()) {
      toast.error(t("mcp.wizard.urlRequired"), { duration: 3000 });
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
      initialServer?.type ?? (initialServer?.url ? "http" : "stdio");

    setWizardType(resolvedType);

    if (resolvedType === "http" || resolvedType === "sse") {
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
      return;
    }

    setWizardCommand(initialServer?.command ?? "");
    const argsValue = initialServer?.args;
    setWizardArgs(Array.isArray(argsValue) ? argsValue.join("\n") : "");
    const envCandidate = initialServer?.env;
    const env =
      envCandidate && typeof envCandidate === "object"
        ? envCandidate
        : undefined;
    setWizardEnv(
      env
        ? Object.entries(env)
            .map(([k, v]) => `${k}=${v ?? ""}`)
            .join("\n")
        : "",
    );
    setWizardUrl("");
    setWizardHeaders("");
  }, [isOpen]);

  const preview = generatePreview();

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && handleClose()}>
      <DialogContent
        className="max-w-2xl max-h-[90vh] flex flex-col"
        zIndex="alert"
      >
        <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
          <DialogTitle className="text-lg font-semibold">
            {t("mcp.wizard.title")}
          </DialogTitle>
        </DialogHeader>

        {/* Content */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* Hint */}
          <div className="rounded-lg border border-border-default bg-gray-100/50 dark:bg-gray-800/50 p-3">
            <p className="text-sm text-muted-foreground">
              {t("mcp.wizard.hint")}
            </p>
          </div>

          {/* Form Fields */}
          <div className="space-y-4 min-h-[400px]">
            {/* Type */}
            <div>
              <label className="mb-2 block text-sm font-medium text-foreground">
                {t("mcp.wizard.type")} <span className="text-red-500">*</span>
              </label>
              <div className="flex gap-4">
                <label className="inline-flex items-center gap-2 cursor-pointer">
                  <input
                    type="radio"
                    value="stdio"
                    checked={wizardType === "stdio"}
                    onChange={(e) =>
                      setWizardType(e.target.value as "stdio" | "http" | "sse")
                    }
                    className="w-4 h-4 accent-blue-500"
                  />
                  <span className="text-sm text-foreground">
                    {t("mcp.wizard.typeStdio")}
                  </span>
                </label>
                <label className="inline-flex items-center gap-2 cursor-pointer">
                  <input
                    type="radio"
                    value="http"
                    checked={wizardType === "http"}
                    onChange={(e) =>
                      setWizardType(e.target.value as "stdio" | "http" | "sse")
                    }
                    className="w-4 h-4 accent-blue-500"
                  />
                  <span className="text-sm text-foreground">
                    {t("mcp.wizard.typeHttp")}
                  </span>
                </label>
                <label className="inline-flex items-center gap-2 cursor-pointer">
                  <input
                    type="radio"
                    value="sse"
                    checked={wizardType === "sse"}
                    onChange={(e) =>
                      setWizardType(e.target.value as "stdio" | "http" | "sse")
                    }
                    className="w-4 h-4 accent-blue-500"
                  />
                  <span className="text-sm text-foreground">
                    {t("mcp.wizard.typeSse")}
                  </span>
                </label>
              </div>
            </div>

            {/* Title */}
            <div>
              <label className="mb-1 block text-sm font-medium text-foreground">
                {t("mcp.form.title")} <span className="text-red-500">*</span>
              </label>
              <Input
                type="text"
                value={wizardTitle}
                onChange={(e) => setWizardTitle(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t("mcp.form.titlePlaceholder")}
                className="font-mono"
              />
            </div>

            {/* Stdio 类型字段 */}
            {wizardType === "stdio" && (
              <>
                {/* Command */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.command")}{" "}
                    <span className="text-red-500">*</span>
                  </label>
                  <Input
                    type="text"
                    value={wizardCommand}
                    onChange={(e) => setWizardCommand(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={t("mcp.wizard.commandPlaceholder")}
                    className="font-mono"
                  />
                </div>

                {/* Args */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.args")}
                  </label>
                  <textarea
                    value={wizardArgs}
                    onChange={(e) => setWizardArgs(e.target.value)}
                    placeholder={t("mcp.wizard.argsPlaceholder")}
                    rows={3}
                    className="w-full rounded-md border border-border-default bg-background px-3 py-2 text-sm font-mono text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20 resize-y"
                  />
                </div>

                {/* Env */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.env")}
                  </label>
                  <textarea
                    value={wizardEnv}
                    onChange={(e) => setWizardEnv(e.target.value)}
                    placeholder={t("mcp.wizard.envPlaceholder")}
                    rows={3}
                    className="w-full rounded-md border border-border-default bg-background px-3 py-2 text-sm font-mono text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20 resize-y"
                  />
                </div>
              </>
            )}

            {/* HTTP 和 SSE 类型字段 */}
            {(wizardType === "http" || wizardType === "sse") && (
              <>
                {/* URL */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.url")}{" "}
                    <span className="text-red-500">*</span>
                  </label>
                  <Input
                    type="text"
                    value={wizardUrl}
                    onChange={(e) => setWizardUrl(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={t("mcp.wizard.urlPlaceholder")}
                    className="font-mono"
                  />
                </div>

                {/* Headers */}
                <div>
                  <label className="mb-1 block text-sm font-medium text-foreground">
                    {t("mcp.wizard.headers")}
                  </label>
                  <textarea
                    value={wizardHeaders}
                    onChange={(e) => setWizardHeaders(e.target.value)}
                    placeholder={t("mcp.wizard.headersPlaceholder")}
                    rows={3}
                    className="w-full rounded-md border border-border-default bg-background px-3 py-2 text-sm font-mono text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20 resize-y"
                  />
                </div>
              </>
            )}
          </div>

          {/* Preview */}
          {(wizardCommand ||
            wizardArgs ||
            wizardEnv ||
            wizardUrl ||
            wizardHeaders) && (
            <div className="space-y-2 border-t border-border-default pt-4">
              <h3 className="text-sm font-medium text-foreground">
                {t("mcp.wizard.preview")}
              </h3>
              <pre className="overflow-x-auto rounded-lg bg-gray-100 dark:bg-gray-800 p-3 text-xs font-mono text-gray-700 dark:text-gray-300">
                {preview}
              </pre>
            </div>
          )}
        </div>

        {/* Footer */}
        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button variant="outline" onClick={handleClose}>
            {t("common.cancel")}
          </Button>
          <Button variant="mcp" onClick={handleApply}>
            <Save className="h-4 w-4" />
            {t("mcp.wizard.apply")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

export default McpWizardModal;

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import {
  CheckCircle2,
  AlertCircle,
  XCircle,
  Info,
  Download,
  Wrench,
  Loader2,
  AlertTriangle,
  Trash2,
  Eye,
  Copy,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from "@/components/ui/dialog";
import { Checkbox } from "@/components/ui/checkbox";
import { doctorApi } from "@/lib/api/doctor";
import type {
  DiagnosisResult,
  DiagnosisIssue,
  UninstallReport,
} from "@/lib/api/doctor";
import { useInstallLogStream } from "@/hooks/useInstallLogStream";
import { InstallLogPanel } from "./InstallLogPanel";

interface EnvironmentDoctorPanelProps {
  diagnosis: DiagnosisResult;
  /** 用户点「一键安装」时调用，channelId 由 panel 内部生成并透传给后端流式日志 */
  onInstall: (tool: string, channelId: string) => Promise<void>;
  onFix: () => Promise<void>;
  isInstalling: boolean;
  isFixing: boolean;
}

interface IssueCardProps {
  issue: DiagnosisIssue;
}

function IssueCard({ issue }: IssueCardProps) {
  const { t } = useTranslation();

  const getSeverityConfig = (severity: DiagnosisIssue["severity"]) => {
    switch (severity) {
      case "Critical":
        return {
          icon: <XCircle className="h-4 w-4" />,
          className:
            "bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/20",
          label: t("doctor.severity.critical"),
        };
      case "High":
        return {
          icon: <AlertTriangle className="h-4 w-4" />,
          className:
            "bg-orange-500/10 text-orange-600 dark:text-orange-400 border-orange-500/20",
          label: t("doctor.severity.high"),
        };
      case "Medium":
        return {
          icon: <AlertCircle className="h-4 w-4" />,
          className:
            "bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 border-yellow-500/20",
          label: t("doctor.severity.medium"),
        };
      case "Low":
        return {
          icon: <Info className="h-4 w-4" />,
          className:
            "bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20",
          label: t("doctor.severity.low"),
        };
    }
  };

  const severityConfig = getSeverityConfig(issue.severity);

  return (
    <motion.div
      initial={{ opacity: 0, x: -10 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.2 }}
      className="rounded-lg border border-border bg-card/50 p-3 space-y-2"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-start gap-2 flex-1">
          <div className="mt-0.5">{severityConfig.icon}</div>
          <div className="space-y-1 flex-1">
            <div className="flex items-center gap-2">
              <h4 className="text-sm font-medium">{issue.title}</h4>
              <Badge variant="outline" className={severityConfig.className}>
                {severityConfig.label}
              </Badge>
              {issue.auto_fixable && (
                <Badge variant="secondary" className="text-xs">
                  {t("doctor.autoFixable")}
                </Badge>
              )}
            </div>
            <p className="text-xs text-muted-foreground">{issue.description}</p>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

export function EnvironmentDoctorPanel({
  diagnosis,
  onInstall,
  onFix,
  isInstalling,
  isFixing,
}: EnvironmentDoctorPanelProps) {
  const { t } = useTranslation();

  // ─── 弹窗 / 报告状态 ───
  const [showPreview, setShowPreview] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const [uninstallReport, setUninstallReport] =
    useState<UninstallReport | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const [previewReport, setPreviewReport] = useState<UninstallReport | null>(
    null,
  );
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);

  // ─── 流式日志 ───
  // 同一时间只有一个会话；用 sessionType 区分本次是 install 还是 uninstall
  const log = useInstallLogStream();
  const [sessionType, setSessionType] = useState<
    "install" | "uninstall" | null
  >(null);
  const [isCancelling, setIsCancelling] = useState(false);

  const isUninstalling =
    sessionType === "uninstall" && log.status === "running";

  // ─── 处理函数 ───
  const handleStartInstall = async (tool: string) => {
    const cid = log.start();
    setSessionType("install");
    try {
      await onInstall(tool, cid);
    } catch (err) {
      console.error("[Doctor] install failed:", err);
      log.finish("failed");
    }
  };

  const handlePreview = async () => {
    setIsPreviewLoading(true);
    try {
      // 预览是 dry-run、瞬时返回，不需要日志通道；用一次性 channelId 占位
      const cid = `preview-${Date.now()}`;
      const report = await doctorApi.uninstallClaudeCode(true, cid);
      setPreviewReport(report);
      setShowPreview(true);
    } catch (err) {
      console.error("预览卸载内容失败:", err);
    } finally {
      setIsPreviewLoading(false);
    }
  };

  const handleConfirmUninstall = async () => {
    setShowConfirm(false);
    setAcknowledged(false);
    const cid = log.start();
    setSessionType("uninstall");
    setUninstallReport(null);

    try {
      const report = await doctorApi.uninstallClaudeCode(false, cid);
      setUninstallReport(report);
    } catch (err) {
      console.error("卸载失败:", err);
      log.finish("failed");
    }
  };

  const handleCancel = async () => {
    if (!log.channelId || isCancelling) return;
    setIsCancelling(true);
    try {
      await doctorApi.cancelInstall(log.channelId);
      // 后端收到 kill 后会 emit done(cancelled=true)，hook 自动切 status
    } catch (err) {
      console.error("取消失败:", err);
    } finally {
      setIsCancelling(false);
    }
  };

  const handleReset = () => {
    setUninstallReport(null);
    setSessionType(null);
    setCopySuccess(false);
    log.reset();
  };

  const handleCopy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopySuccess(true);
      setTimeout(() => setCopySuccess(false), 1500);
    } catch {
      // 剪贴板不可用时静默失败
    }
  };

  const getStatusIcon = () => {
    switch (diagnosis.overall_status) {
      case "Healthy":
        return <CheckCircle2 className="h-5 w-5 text-green-500" />;
      case "NeedsInstall":
        return <AlertCircle className="h-5 w-5 text-yellow-500" />;
      case "NeedsRepair":
        return <XCircle className="h-5 w-5 text-red-500" />;
      case "PartiallyHealthy":
        return <Info className="h-5 w-5 text-blue-500" />;
    }
  };

  const getStatusText = () => {
    switch (diagnosis.overall_status) {
      case "Healthy":
        return t("doctor.status.healthy");
      case "NeedsInstall":
        return t("doctor.status.needsInstall");
      case "NeedsRepair":
        return t("doctor.status.needsRepair");
      case "PartiallyHealthy":
        return t("doctor.status.partiallyHealthy");
    }
  };

  return (
    <>
      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3, delay: 0.2 }}
        className="rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-6 space-y-4 shadow-sm"
      >
        <div className="flex items-center gap-3">
          {getStatusIcon()}
          <div>
            <h3 className="text-lg font-semibold">
              {t("doctor.environmentStatus")}
            </h3>
            <p className="text-sm text-muted-foreground">{getStatusText()}</p>
          </div>
        </div>

        {/* 问题列表 */}
        {diagnosis.issues.length > 0 && (
          <div className="space-y-2">
            <h4 className="text-sm font-medium text-muted-foreground">
              {t("doctor.issuesFound", { count: diagnosis.issues.length })}
            </h4>
            {diagnosis.issues.map((issue) => (
              <IssueCard key={issue.id} issue={issue} />
            ))}
          </div>
        )}

        {/* 操作按钮区域 */}
        {diagnosis.overall_status === "NeedsInstall" &&
          (() => {
            const installIssue = diagnosis.issues.find((issue) => {
              const actionType = issue.fix_action?.type;
              return (
                actionType === "InstallTool" || actionType === "InstallNodeJs"
              );
            });
            const toolToInstall = installIssue?.fix_action?.tool || "claude";

            return (
              <div className="space-y-3 pt-2">
                <div className="flex justify-end">
                  <Button
                    onClick={() => handleStartInstall(toolToInstall)}
                    disabled={isInstalling || sessionType === "install"}
                  >
                    {isInstalling || sessionType === "install" ? (
                      <>
                        <Loader2 className="h-4 w-4 animate-spin" />
                        {t("doctor.installing")}
                      </>
                    ) : (
                      <>
                        <Download className="h-4 w-4" />
                        {t("doctor.oneClickInstall")}
                      </>
                    )}
                  </Button>
                </div>
                {sessionType === "install" && (
                  <InstallLogPanel
                    status={log.status}
                    lines={log.lines}
                    onCancel={handleCancel}
                    isCancelling={isCancelling}
                  />
                )}
              </div>
            );
          })()}

        {diagnosis.overall_status === "NeedsRepair" &&
          (() => {
            // 检查是否有可自动修复的问题
            const hasAutoFixableIssues = diagnosis.issues.some(
              (issue) => issue.auto_fixable,
            );

            return hasAutoFixableIssues ? (
              <div className="flex justify-end pt-2">
                <Button
                  onClick={onFix}
                  disabled={isFixing}
                  variant="destructive"
                >
                  {isFixing ? (
                    <>
                      <Loader2 className="h-4 w-4 animate-spin" />
                      {t("doctor.fixing")}
                    </>
                  ) : (
                    <>
                      <Wrench className="h-4 w-4" />
                      {t("doctor.oneClickFix")}
                    </>
                  )}
                </Button>
              </div>
            ) : null;
          })()}

        {/* 健康状态 */}
        {diagnosis.overall_status === "Healthy" && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <CheckCircle2 className="h-4 w-4 text-green-500" />
            {t("doctor.allGood")}
          </div>
        )}
      </motion.div>

      {/* 危险区域 --- 一键卸载 Claude Code */}
      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3, delay: 0.4 }}
        className="rounded-xl border border-red-500/30 bg-red-500/5 p-6 space-y-4 shadow-sm"
      >
        <div className="flex items-center gap-3">
          <AlertTriangle className="h-5 w-5 text-red-600 dark:text-red-400" />
          <h3 className="text-lg font-semibold text-red-600 dark:text-red-400">
            危险操作
          </h3>
        </div>

        <div className="space-y-2">
          <h4 className="text-base font-medium">一键卸载 Claude Code</h4>
          <p className="text-sm text-muted-foreground">
            将删除 Claude Code CLI、~/.claude/ 目录、系统凭证和 shell 环境变量
          </p>
        </div>

        {/* --- 空闲态：显示操作按钮 --- */}
        {sessionType !== "uninstall" && (
          <div className="flex flex-wrap gap-2 pt-2">
            <Button
              variant="outline"
              onClick={handlePreview}
              disabled={isPreviewLoading}
            >
              {isPreviewLoading ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Eye className="h-4 w-4" />
              )}
              预览卸载内容
            </Button>
            <Button variant="destructive" onClick={() => setShowConfirm(true)}>
              <Trash2 className="h-4 w-4" />
              执行卸载
            </Button>
          </div>
        )}

        {/* --- 取消 / 失败但没有 report：仅显示日志 + 关闭按钮 --- */}
        {sessionType === "uninstall" &&
          !isUninstalling &&
          !uninstallReport && (
            <div className="space-y-3 pt-2">
              <InstallLogPanel status={log.status} lines={log.lines} />
              <div className="flex justify-end">
                <Button variant="outline" onClick={handleReset}>
                  关闭
                </Button>
              </div>
            </div>
          )}

        {/* --- 执行中：实时日志 --- */}
        {sessionType === "uninstall" && isUninstalling && (
          <div className="space-y-3 pt-2">
            <InstallLogPanel
              status={log.status}
              lines={log.lines}
              onCancel={handleCancel}
              isCancelling={isCancelling}
            />
          </div>
        )}

        {/* --- 完成态 --- */}
        {uninstallReport && !isUninstalling && sessionType === "uninstall" && (
          <div className="space-y-3 pt-2">
            {/* 折叠日志区，方便用户回看 */}
            <InstallLogPanel status={log.status} lines={log.lines} />

            {/* 总体状态 */}
            <div className="flex items-center gap-2">
              {uninstallReport.overall === "Success" ? (
                <CheckCircle2 className="h-5 w-5 text-green-500" />
              ) : uninstallReport.overall === "Partial" ? (
                <AlertTriangle className="h-5 w-5 text-yellow-500" />
              ) : (
                <XCircle className="h-5 w-5 text-red-500" />
              )}
              <span className="font-medium">
                {uninstallReport.overall === "Success"
                  ? "卸载完成"
                  : uninstallReport.overall === "Partial"
                    ? "部分完成"
                    : "卸载失败"}
              </span>
            </div>

            {/* 各步骤详情 */}
            <div className="space-y-2">
              {uninstallReport.steps.map((step, idx) => (
                <div key={idx} className="flex items-start gap-2 text-sm">
                  {step.status === "Success" ? (
                    <CheckCircle2 className="h-4 w-4 text-green-500 mt-0.5 shrink-0" />
                  ) : step.status === "Skipped" ? (
                    <Info className="h-4 w-4 text-yellow-500 mt-0.5 shrink-0" />
                  ) : (
                    <XCircle className="h-4 w-4 text-red-500 mt-0.5 shrink-0" />
                  )}
                  <div>
                    <span className="font-medium">{step.name}</span>
                    {step.message && (
                      <p className="text-xs text-muted-foreground">
                        {step.message}
                      </p>
                    )}
                  </div>
                </div>
              ))}
            </div>

            {/* 备份路径 */}
            <div className="flex items-center gap-2 rounded-lg bg-muted/50 p-2">
              <span className="text-xs text-muted-foreground shrink-0">
                备份路径：
              </span>
              <code className="text-xs text-foreground truncate">
                {uninstallReport.backupPath}
              </code>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 shrink-0"
                onClick={() => handleCopy(uninstallReport.backupPath)}
              >
                {copySuccess ? (
                  <CheckCircle2 className="h-3 w-3 text-green-500" />
                ) : (
                  <Copy className="h-3 w-3" />
                )}
              </Button>
            </div>

            {/* 重新诊断建议 */}
            {uninstallReport.overall !== "Success" && (
              <p className="text-xs text-yellow-600 dark:text-yellow-400">
                卸载过程未完全成功，建议重新诊断环境后手动处理遗留问题
              </p>
            )}

            {/* 关闭按钮 */}
            <div className="flex justify-end pt-1">
              <Button variant="outline" onClick={handleReset}>
                关闭
              </Button>
            </div>
          </div>
        )}

        <p className="text-xs text-muted-foreground pt-1">
          卸载前会自动备份到 ~/.cc-doctor/backups/
        </p>
      </motion.div>

      {/* 预览弹窗 */}
      <Dialog
        open={showPreview}
        onOpenChange={(open) => {
          setShowPreview(open);
          if (!open) setPreviewReport(null);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>卸载预览</DialogTitle>
            <DialogDescription>以下是将要执行的操作步骤：</DialogDescription>
          </DialogHeader>
          <div className="px-6 py-4 space-y-3 max-h-[60vh] overflow-y-auto">
            {previewReport?.steps.map((step, idx) => (
              <div key={idx} className="flex items-start gap-3">
                <Info className="h-4 w-4 text-blue-500 mt-0.5 shrink-0" />
                <div>
                  <p className="text-sm font-medium">{step.name}</p>
                  {step.message && (
                    <p className="text-xs text-muted-foreground mt-0.5">
                      {step.message}
                    </p>
                  )}
                </div>
              </div>
            ))}
          </div>
          {previewReport?.backupPath && (
            <div className="px-6 pb-3">
              <p className="text-xs text-muted-foreground">
                备份路径：{previewReport.backupPath}
              </p>
            </div>
          )}
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="outline">关闭</Button>
            </DialogClose>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 二次确认弹窗 */}
      <Dialog
        open={showConfirm}
        onOpenChange={(open) => {
          setShowConfirm(open);
          if (!open) setAcknowledged(false);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="text-red-600 dark:text-red-400 flex items-center gap-2">
              <AlertTriangle className="h-5 w-5" />
              确认卸载 Claude Code
            </DialogTitle>
            <DialogDescription>
              此操作将永久删除以下内容，无法恢复：
            </DialogDescription>
          </DialogHeader>
          <div className="px-6 py-4 space-y-3">
            {[
              "删除 Claude Code CLI 可执行文件",
              "删除 ~/.claude/ 配置目录",
              "删除系统凭证（keychain / keyring）",
              "清除 shell 环境变量配置",
            ].map((item, idx) => (
              <div key={idx} className="flex items-center gap-3 text-sm">
                <XCircle className="h-4 w-4 text-red-500 shrink-0" />
                <span>{item}</span>
              </div>
            ))}
          </div>
          <div className="flex items-center gap-2 px-6 pb-4">
            <Checkbox
              id="acknowledge"
              checked={acknowledged}
              onCheckedChange={(checked) => setAcknowledged(checked === true)}
            />
            <label
              htmlFor="acknowledge"
              className="text-xs text-muted-foreground cursor-pointer select-none"
            >
              我已知晓此操作不可逆，已做好备份准备
            </label>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => {
                setShowConfirm(false);
                setAcknowledged(false);
              }}
            >
              取消
            </Button>
            <Button
              variant="destructive"
              disabled={!acknowledged}
              onClick={handleConfirmUninstall}
            >
              <Trash2 className="h-4 w-4" />
              确认卸载
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

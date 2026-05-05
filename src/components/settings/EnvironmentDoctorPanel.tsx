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
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import type { DiagnosisResult, DiagnosisIssue } from "@/lib/api/doctor";

interface EnvironmentDoctorPanelProps {
  diagnosis: DiagnosisResult;
  onInstall: (tool: string) => Promise<void>;
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
          className: "bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/20",
          label: t("doctor.severity.critical"),
        };
      case "High":
        return {
          icon: <AlertTriangle className="h-4 w-4" />,
          className: "bg-orange-500/10 text-orange-600 dark:text-orange-400 border-orange-500/20",
          label: t("doctor.severity.high"),
        };
      case "Medium":
        return {
          icon: <AlertCircle className="h-4 w-4" />,
          className: "bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 border-yellow-500/20",
          label: t("doctor.severity.medium"),
        };
      case "Low":
        return {
          icon: <Info className="h-4 w-4" />,
          className: "bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20",
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
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, delay: 0.2 }}
      className="rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-6 space-y-4 shadow-sm"
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          {getStatusIcon()}
          <div>
            <h3 className="text-lg font-semibold">{t("doctor.environmentStatus")}</h3>
            <p className="text-sm text-muted-foreground">{getStatusText()}</p>
          </div>
        </div>

        {/* 操作按钮 */}
        {diagnosis.overall_status === "NeedsInstall" && (
          <Button onClick={() => onInstall("claude")} disabled={isInstalling}>
            {isInstalling ? (
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
        )}

        {diagnosis.overall_status === "NeedsRepair" && (
          <Button onClick={onFix} disabled={isFixing} variant="destructive">
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
        )}
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

      {/* 健康状态 */}
      {diagnosis.overall_status === "Healthy" && (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <CheckCircle2 className="h-4 w-4 text-green-500" />
          {t("doctor.allGood")}
        </div>
      )}
    </motion.div>
  );
}

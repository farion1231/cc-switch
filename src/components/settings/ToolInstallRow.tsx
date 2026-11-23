import { useTranslation } from "react-i18next";
import type {
  ToolInstallation,
  ToolInstallationPath,
} from "@/lib/api/settings";

/**
 * 单处工具安装的信息行：来源徽章 + 路径 + 版本（或「无法运行」）+「默认」标记。
 * 冲突诊断列表与升级确认对话框共用，确保两处的视觉与「默认」判定始终一致。
 * 也渲染安装概览（list_tool_installations）里未探测的条目：没有 runnable / version
 * 时只标来源与路径，不臆造「无法运行」——是否可运行以诊断探测为准。
 */
export function ToolInstallRow({
  inst,
}: {
  inst: ToolInstallation | ToolInstallationPath;
}) {
  const { t } = useTranslation();
  // `"runnable" in inst` 兼作判别器：诊断结果带它，概览条目没有。
  const notRunnable = "runnable" in inst && !inst.runnable;
  return (
    <div className="flex items-center gap-1.5 text-badge">
      <span className="shrink-0 rounded bg-surface px-1 py-0.5 font-mono text-fg-2">
        {inst.source}
      </span>
      <span
        className="min-w-0 flex-1 truncate font-mono text-fg-2"
        title={inst.path}
      >
        {inst.path}
      </span>
      {notRunnable ? (
        <span className="shrink-0 text-warning-text">
          {t("settings.toolConflictNotRunnable")}
        </span>
      ) : "runnable" in inst ? (
        <span className="shrink-0 font-mono text-fg-1">{inst.version}</span>
      ) : null}
      {inst.is_path_default && (
        <span className="shrink-0 rounded-full border border-primary/30 bg-primary/10 px-1 py-0.5 text-badge text-primary">
          {t("settings.toolConflictDefault")}
        </span>
      )}
    </div>
  );
}

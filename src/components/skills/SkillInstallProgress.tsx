import { useTranslation } from "react-i18next";
import type { SkillInstallProgress as InstallProgress } from "@/lib/api/skills";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

/** 只把实际下载量表示为百分比；解压、同步等阶段使用不定进度。 */
export function SkillInstallProgress({
  progress,
  name,
}: {
  progress?: InstallProgress;
  name: string;
}) {
  const { t } = useTranslation();
  const phase = progress?.phase ?? "downloading";
  const downloaded = progress?.downloadedBytes ?? 0;
  const total = progress?.totalBytes;
  const percent =
    phase === "completed"
      ? 100
      : phase === "downloading" && total && total > 0
        ? Math.min(100, Math.floor((downloaded / total) * 100))
        : undefined;
  const label = t(
    `skills.installProgress.${phase === "downloading" && !downloaded && !total ? "preparing" : phase}`,
  );
  const bytes = total
    ? `${formatBytes(downloaded)} / ${formatBytes(total)}`
    : formatBytes(downloaded);

  return (
    <div className="flex w-full min-w-0 flex-col gap-1 text-caption text-fg-2">
      <div className="flex items-center justify-between gap-2">
        <span role="status">{label}</span>
        {percent !== undefined && (
          <span className="tabular-nums">{percent}%</span>
        )}
      </div>
      <div
        role="progressbar"
        aria-label={t("skillsPage.discover.installingAria", { name })}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={phase === "downloading" ? `${label}, ${bytes}` : label}
        className="h-1 overflow-hidden rounded-full bg-selected"
      >
        <div
          className={
            percent === undefined
              ? "h-full w-1/3 bg-action motion-safe:animate-skill-download"
              : "h-full bg-action transition-transform duration-150 motion-reduce:transition-none"
          }
          style={
            percent === undefined
              ? undefined
              : { transform: `translateX(${percent - 100}%)` }
          }
        />
      </div>
      {phase === "downloading" && downloaded > 0 && (
        <span className="whitespace-nowrap tabular-nums">{bytes}</span>
      )}
    </div>
  );
}

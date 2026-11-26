import { useTranslation } from "react-i18next";
import type { ToolInstallation } from "@/lib/api/settings";

const INSTALL_SOURCE_LABELS: Record<string, string> = {
  official: "Official",
  homebrew: "Homebrew",
  npm: "npm",
  curl: "curl",
  powershell: "PowerShell",
  nix: "Nix",
  nvm: "nvm",
  volta: "Volta",
  fnm: "fnm",
  mise: "mise",
  bun: "Bun",
  pnpm: "pnpm",
  yarn: "Yarn",
  vite: "Vite+",
  scoop: "Scoop",
  pip: "pip",
  wsl: "WSL",
};

export function getToolInstallSourceLabel(source: string): string | undefined {
  return Object.prototype.hasOwnProperty.call(INSTALL_SOURCE_LABELS, source)
    ? INSTALL_SOURCE_LABELS[source]
    : undefined;
}

/**
 * 单处工具安装的信息行：来源徽章、可选路径与版本。
 * 冲突诊断列表与升级确认对话框共用，升级确认标注实际目标。
 */
export function ToolInstallRow({
  inst,
  packageVersion,
  updateTarget,
  hidePath,
}: {
  inst: ToolInstallation;
  packageVersion?: string | null;
  updateTarget?: boolean;
  hidePath?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-badge">
      <span className="shrink-0 rounded bg-surface px-1 py-0.5 font-mono text-fg-2">
        {inst.source === "desktop"
          ? t("appsPage.desktopInstallation")
          : (getToolInstallSourceLabel(inst.source) ??
            t("appsPage.installSourceUnknown"))}
      </span>
      {!hidePath && (
        <span
          className="min-w-0 flex-1 truncate font-mono text-fg-2"
          title={inst.path}
        >
          {inst.path}
        </span>
      )}
      <span
        className={
          inst.runnable
            ? "shrink-0 font-mono text-fg-1"
            : "shrink-0 text-warning-text"
        }
      >
        {inst.runnable ? (
          inst.version
        ) : (
          <>
            {packageVersion && (
              <span className="me-1 font-mono text-fg-1">{packageVersion}</span>
            )}
            {t("settings.toolConflictNotRunnable")}
          </>
        )}
      </span>
      {updateTarget && (
        <span className="shrink-0 rounded bg-surface px-1 py-0.5 text-warning-text">
          {t("appsPage.updateTarget")}
        </span>
      )}
    </div>
  );
}

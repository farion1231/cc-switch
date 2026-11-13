import type { TFunction } from "i18next";
import { toast } from "@/lib/toast";
import type { TargetReport } from "@/lib/api/appTargets";
import { reasonLabel, targetLabel } from "@/hooks/useAppTargets";

/**
 * 切换后跟随环境的结果：全部写入时不另弹（「切换成功」已经说了）；有未同步、保持原配置
 * 或跳过的 MCP 时，明确是哪几个环境、为什么。
 */
export function notifyTargetResults(
  targets: TargetReport[],
  providerName: string,
  t: TFunction,
) {
  const failed = targets.filter(
    (r) => r.status === "error" || r.status === "deferred",
  );
  const kept = targets.filter((r) => r.status === "skipped");
  if (failed.length) {
    toast.warning(
      t("appTargets.toast.switchPartial", {
        provider: providerName,
        targets: failed.map((r) => targetLabel(r.target)).join("、"),
      }),
      {
        description: failed
          .map(
            (r) =>
              `${targetLabel(r.target)}: ${reasonLabel(t, r.reason)}${
                r.detail ? ` (${r.detail})` : ""
              }`,
          )
          .join("\n"),
        duration: 10000,
      },
    );
  }
  if (kept.length) {
    toast.info(
      t("appTargets.toast.keptOriginal", {
        targets: kept.map((r) => targetLabel(r.target)).join("、"),
      }),
      { duration: 8000 },
    );
  }
  const skippedMcp = targets
    .flatMap((r) => r.warnings ?? [])
    .filter((w) => w.startsWith("mcp_skipped_windows_only:"));
  if (skippedMcp.length) {
    toast.info(
      t("appTargets.toast.mcpSkipped", {
        servers: [...new Set(skippedMcp.map((w) => w.split(":")[1]))].join(
          "、",
        ),
      }),
      { duration: 8000 },
    );
  }
}

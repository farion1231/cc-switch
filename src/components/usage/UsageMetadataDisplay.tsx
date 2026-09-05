import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import type { RequestLog } from "@/types/usage";

export const serviceTiers = [
  "default",
  "fast",
  "flex",
  "auto",
  "ultrafast",
  "scale",
] as const;
export const reasoningEfforts = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const;

export function serviceTierLabel(
  tier: string | null | undefined,
  t: TFunction,
  model?: string,
) {
  if (!tier) return t("usage.metadataUnknown");
  if (tier === "priority" && model?.startsWith("claude-")) return "Priority";
  if (tier === "fast" || tier === "priority") return "Fast";
  if (tier === "default") return t("usage.standardTier");
  if (tier === "auto") return t("usage.autoTier");
  return tier;
}

export function UsageMetadataDisplay({
  request,
  showSource = false,
}: {
  request: Pick<
    RequestLog,
    "serviceTier" | "serviceTierSource" | "reasoningEffort"
  > & { model?: string };
  showSource?: boolean;
}) {
  const { t } = useTranslation();
  const source =
    request.serviceTierSource === "response"
      ? t("usage.tierResponse")
      : request.serviceTierSource === "request"
        ? t("usage.tierRequest")
        : "";
  return (
    <div className="flex flex-wrap items-center justify-center gap-1 text-[10px] font-sans text-muted-foreground">
      <span
        className={
          serviceTierLabel(request.serviceTier, t, request.model) === "Fast"
            ? "rounded bg-green-100 px-1.5 py-0.5 text-green-700 dark:bg-green-900/40 dark:text-green-300"
            : "rounded bg-muted px-1.5 py-0.5"
        }
        title={`${t("usage.serviceTier")}${source ? ` · ${source}` : ""}`}
      >
        {serviceTierLabel(request.serviceTier, t, request.model)}
        {showSource && source ? ` · ${source}` : ""}
      </span>
      <span
        className="rounded bg-muted px-1.5 py-0.5"
        title={t("usage.reasoningEffort")}
      >
        {request.reasoningEffort ?? t("usage.metadataUnknown")}
      </span>
    </div>
  );
}

import { useTranslation } from "react-i18next";
import type { SubscriptionQuota } from "@/types/subscription";
import { cn } from "@/lib/utils";
import { useNow } from "./QuotaLines";

/** Membership validity has its own line, separate from usage reset countdowns. */
export function CodexMembershipLine({ quota }: { quota: SubscriptionQuota }) {
  const { t, i18n } = useTranslation();
  const now = useNow(Boolean(quota.membership?.activeUntil));
  if (quota.tool !== "codex" && quota.tool !== "codex_oauth") return null;
  if (!quota.membership) return null;
  const { planType, activeUntil } = quota.membership;
  const at = activeUntil ? Date.parse(activeUntil) : NaN;
  const hasDate = Number.isFinite(at);
  const expired = hasDate && at <= now;
  const soon = hasDate && !expired && at - now <= 7 * 86400_000;
  const date = hasDate
    ? new Intl.DateTimeFormat(i18n.language, {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
      }).format(at)
    : null;
  const plan = planType?.trim() || "ChatGPT";
  const label = hasDate
    ? t(
        expired
          ? "subscription.membership.expired"
          : "subscription.membership.validUntil",
        { plan, date },
      )
    : t("subscription.membership.unavailable", { plan });
  return (
    <p
      className={cn(
        "text-caption text-fg-3",
        expired && "text-danger-text",
        soon && "text-orange-500 dark:text-orange-400",
      )}
      title={
        hasDate
          ? `${t("subscription.membership.hint")} ${new Date(at).toLocaleString(i18n.language)}`
          : t("subscription.membership.hint")
      }
    >
      {label}
    </p>
  );
}

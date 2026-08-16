import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { AgentUsageMeasure } from "@/types/usage";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { UsageCostStatus } from "./format";

type UsageQualityTooltipProps = {
  partial?: boolean;
  timeSemantics?: AgentUsageMeasure["timeSemantics"] | null;
  costStatus?: UsageCostStatus;
  replayInProgress?: boolean;
  className?: string;
};

function qualityMessages(
  t: ReturnType<typeof useTranslation>["t"],
  {
    partial,
    timeSemantics,
    costStatus,
    replayInProgress,
  }: UsageQualityTooltipProps,
) {
  const messages: string[] = [];
  if (partial) {
    messages.push(
      t("usage.qualityPartialHint", {
        defaultValue: "Some usage fields are partial or unavailable.",
      }),
    );
  }
  if (timeSemantics === "sync_window_end") {
    messages.push(
      t("usage.qualitySyncWindowHint", {
        defaultValue: "Sync-window increment; not per-request.",
      }),
    );
  }
  if (timeSemantics === "session_time") {
    messages.push(
      t("usage.qualitySessionTimeHint", {
        defaultValue: "Usage is aggregated by session time.",
      }),
    );
  }
  if (timeSemantics === "unavailable") {
    messages.push(
      t("usage.qualityTimeUnavailableHint", {
        defaultValue: "Source time is unavailable.",
      }),
    );
  }
  if (costStatus === "unavailable") {
    messages.push(
      replayInProgress
        ? t("usage.qualityCostReplayInProgressHint", {
            defaultValue:
              "Codex history is still being replayed; cost will appear when replay completes.",
          })
        : t("usage.qualityCostUnavailableHint", {
            defaultValue:
              "Cost cannot be estimated from the available pricing or usage fields.",
          }),
    );
  }
  return messages;
}

export function UsageQualityTooltip(props: UsageQualityTooltipProps) {
  const { t } = useTranslation();
  const messages = qualityMessages(t, props);
  if (messages.length === 0) return null;

  const label = messages.join(" ");
  return (
    <TooltipProvider delayDuration={220}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            aria-label={label}
            className={props.className ?? "inline-flex shrink-0"}
            role="img"
            tabIndex={0}
          >
            <TriangleAlert
              className="size-3.5 text-amber-500"
              aria-hidden="true"
            />
          </span>
        </TooltipTrigger>
        <TooltipContent
          side="bottom"
          align="end"
          collisionPadding={12}
          className="w-max max-w-[min(320px,calc(100vw-2rem))] whitespace-normal break-words text-left"
        >
          <div className="space-y-1">
            {messages.map((message) => (
              <div key={message}>{message}</div>
            ))}
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

export function UsageCostTooltip({
  status,
  replayInProgress = false,
  children,
}: {
  status: UsageCostStatus;
  replayInProgress?: boolean;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  if (status === "reported") return <>{children}</>;

  const message =
    status === "estimated"
      ? t("usage.qualityCostEstimateHint", {
          defaultValue:
            "API-equivalent estimate from current model pricing; not a Codex subscription bill.",
        })
      : replayInProgress
        ? t("usage.qualityCostReplayInProgressHint", {
            defaultValue:
              "Codex history is still being replayed; cost will appear when replay completes.",
          })
        : t("usage.qualityCostUnavailableHint", {
            defaultValue:
              "Cost cannot be estimated from the available pricing or usage fields.",
          });

  return (
    <TooltipProvider delayDuration={220}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            aria-label={message}
            className="inline-flex min-w-0 items-center"
            tabIndex={0}
          >
            {children}
          </span>
        </TooltipTrigger>
        <TooltipContent
          side="bottom"
          align="end"
          collisionPadding={12}
          className="max-w-[320px] whitespace-normal text-left"
        >
          {message}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

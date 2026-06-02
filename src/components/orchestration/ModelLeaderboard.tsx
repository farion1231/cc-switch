import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Trophy, TrendingUp, TrendingDown, Minus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

interface ModelStats {
  modelKey: string;
  provider: string;
  qualityScore: number;
  passRate: number;
  avgLatencyMs: number;
  totalCalls: number;
  trend: "up" | "down" | "stable";
}

const MOCK_STATS: ModelStats[] = [
  {
    modelKey: "frontier_planner",
    provider: "anthropic",
    qualityScore: 0.92,
    passRate: 96,
    avgLatencyMs: 3200,
    totalCalls: 142,
    trend: "up",
  },
  {
    modelKey: "mid_executor_code",
    provider: "openai",
    qualityScore: 0.85,
    passRate: 89,
    avgLatencyMs: 1800,
    totalCalls: 347,
    trend: "stable",
  },
  {
    modelKey: "cheap_executor_code",
    provider: "deepseek",
    qualityScore: 0.78,
    passRate: 82,
    avgLatencyMs: 1200,
    totalCalls: 523,
    trend: "up",
  },
  {
    modelKey: "cheap_executor_fast",
    provider: "qwen",
    qualityScore: 0.71,
    passRate: 74,
    avgLatencyMs: 800,
    totalCalls: 210,
    trend: "down",
  },
];

const TREND_ICONS = {
  up: TrendingUp,
  down: TrendingDown,
  stable: Minus,
};

export function ModelLeaderboard() {
  const { t } = useTranslation();

  return (
    <Card className="border-border/50">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="flex items-center gap-2">
          <Trophy className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-medium">
            {t("orchestration.modelLeaderboard", {
              defaultValue: "Model Leaderboard",
            })}
          </CardTitle>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {MOCK_STATS.map((stat, idx) => {
          const TrendIcon = TREND_ICONS[stat.trend];
          return (
            <div
              key={stat.modelKey}
              className="rounded-lg border border-border/50 p-3 space-y-2"
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span
                    className={cn(
                      "flex h-5 w-5 items-center justify-center rounded-full text-[10px] font-bold",
                      idx === 0
                        ? "bg-amber-500/20 text-amber-600"
                        : idx === 1
                          ? "bg-slate-400/20 text-slate-500"
                          : idx === 2
                            ? "bg-orange-500/20 text-orange-600"
                            : "bg-muted text-muted-foreground",
                    )}
                  >
                    {idx + 1}
                  </span>
                  <div>
                    <span className="text-sm font-medium">{stat.modelKey}</span>
                    <span className="text-xs text-muted-foreground ml-1">
                      ({stat.provider})
                    </span>
                  </div>
                </div>
                <div className="flex items-center gap-1">
                  <TrendIcon
                    className={cn(
                      "h-3.5 w-3.5",
                      stat.trend === "up" && "text-emerald-500",
                      stat.trend === "down" && "text-red-500",
                      stat.trend === "stable" && "text-muted-foreground",
                    )}
                  />
                  <Badge variant="outline" className="text-[10px] px-1">
                    {t("orchestration.calls", {
                      defaultValue: `${stat.totalCalls} calls`,
                      count: stat.totalCalls,
                    })}
                  </Badge>
                </div>
              </div>

              <div className="grid grid-cols-2 gap-2">
                <div>
                  <span className="text-[10px] text-muted-foreground">
                    {t("orchestration.quality", { defaultValue: "Quality" })}
                  </span>
                  <div className="flex items-center gap-1">
                    <div className="h-1.5 flex-1 rounded-full bg-muted overflow-hidden">
                      <div
                        className="h-full rounded-full bg-primary"
                        style={{ width: `${stat.qualityScore * 100}%` }}
                      />
                    </div>
                    <span className="text-xs font-medium">
                      {(stat.qualityScore * 100).toFixed(0)}%
                    </span>
                  </div>
                </div>
                <div>
                  <span className="text-[10px] text-muted-foreground">
                    {t("orchestration.passRate", {
                      defaultValue: "Pass Rate",
                    })}
                  </span>
                  <div className="flex items-center gap-1">
                    <div className="h-1.5 flex-1 rounded-full bg-muted overflow-hidden">
                      <div
                        className="h-full rounded-full bg-primary"
                        style={{ width: `${stat.passRate}%` }}
                      />
                    </div>
                    <span className="text-xs font-medium">
                      {stat.passRate}%
                    </span>
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}

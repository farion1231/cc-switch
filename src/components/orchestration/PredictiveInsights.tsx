import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Brain, Lightbulb, AlertTriangle, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

interface Insight {
  id: string;
  type: "optimization" | "warning" | "suggestion";
  title: string;
  description: string;
  confidence: number;
}

const MOCK_INSIGHTS: Insight[] = [
  {
    id: "1",
    type: "optimization",
    title: "Route more tasks to cheap_executor_code",
    description:
      "Analysis shows 23% of tasks currently routed to mid_executor_code could be handled by cheap_executor_code with 94% quality retention.",
    confidence: 0.87,
  },
  {
    id: "2",
    type: "warning",
    title: "frontier_planner latency spike",
    description:
      "Average latency increased 40% in the last 24h. Consider adding a fallback to mid_executor_text for non-critical tasks.",
    confidence: 0.92,
  },
  {
    id: "3",
    type: "suggestion",
    title: "Enable cascade for medium-complexity coding",
    description:
      "Based on 500+ historical samples, cascade strategy would reduce cost by 35% while maintaining quality above threshold.",
    confidence: 0.78,
  },
];

const INSIGHT_ICONS = {
  optimization: Zap,
  warning: AlertTriangle,
  suggestion: Lightbulb,
};

const INSIGHT_COLORS = {
  optimization: "text-emerald-500",
  warning: "text-amber-500",
  suggestion: "text-blue-500",
};

export function PredictiveInsights() {
  const { t } = useTranslation();

  return (
    <Card className="border-border/50">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="flex items-center gap-2">
          <Brain className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-medium">
            {t("orchestration.predictiveInsights", {
              defaultValue: "Predictive Insights",
            })}
          </CardTitle>
        </div>
        <Badge variant="outline" className="text-xs">
          {t("orchestration.insights", {
            defaultValue: `${MOCK_INSIGHTS.length} insights`,
            count: MOCK_INSIGHTS.length,
          })}
        </Badge>
      </CardHeader>
      <CardContent className="space-y-3">
        {MOCK_INSIGHTS.map((insight) => {
          const Icon = INSIGHT_ICONS[insight.type];
          const color = INSIGHT_COLORS[insight.type];
          return (
            <div
              key={insight.id}
              className="rounded-lg border border-border/50 p-3 space-y-1.5"
            >
              <div className="flex items-start gap-2">
                <Icon className={cn("h-4 w-4 mt-0.5 flex-shrink-0", color)} />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between">
                    <span className="text-sm font-medium">{insight.title}</span>
                    <Badge
                      variant="secondary"
                      className="text-[10px] ml-2 flex-shrink-0"
                    >
                      {t("orchestration.confidence", {
                        defaultValue: `${(insight.confidence * 100).toFixed(0)}% confidence`,
                        value: (insight.confidence * 100).toFixed(0),
                      })}
                    </Badge>
                  </div>
                  <p className="text-xs text-muted-foreground mt-0.5">
                    {insight.description}
                  </p>
                </div>
              </div>
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}

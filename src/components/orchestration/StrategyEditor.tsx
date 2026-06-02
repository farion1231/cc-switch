import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Settings2, Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

interface StrategyRule {
  id: string;
  name: string;
  type: "route" | "cascade" | "debate" | "moa";
  complexityRange: [number, number];
  riskLevels: string[];
  models: string[];
  qualityThreshold: number;
}

const MOCK_STRATEGIES: StrategyRule[] = [
  {
    id: "1",
    name: "route",
    type: "route",
    complexityRange: [0, 0.4],
    riskLevels: ["low"],
    models: ["cheap_executor_fast"],
    qualityThreshold: 0.65,
  },
  {
    id: "2",
    name: "cascade",
    type: "cascade",
    complexityRange: [0.4, 0.7],
    riskLevels: ["medium", "high"],
    models: ["cheap_executor_code", "mid_executor_code", "frontier_planner"],
    qualityThreshold: 0.65,
  },
  {
    id: "3",
    name: "debate",
    type: "debate",
    complexityRange: [0.7, 1.0],
    riskLevels: ["high", "critical"],
    models: ["mid_executor_code", "cheap_executor_code", "frontier_planner"],
    qualityThreshold: 0.75,
  },
];

const TYPE_COLORS: Record<string, string> = {
  route: "bg-blue-500/15 text-blue-600",
  cascade: "bg-amber-500/15 text-amber-600",
  debate: "bg-purple-500/15 text-purple-600",
  moa: "bg-rose-500/15 text-rose-600",
};

export function StrategyEditor() {
  const { t } = useTranslation();
  const [strategies, setStrategies] = useState<StrategyRule[]>(MOCK_STRATEGIES);

  const removeStrategy = (id: string) => {
    setStrategies((prev) => prev.filter((s) => s.id !== id));
  };

  return (
    <Card className="border-border/50">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="flex items-center gap-2">
          <Settings2 className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-medium">
            {t("orchestration.strategyEditor", {
              defaultValue: "Strategy Editor",
            })}
          </CardTitle>
        </div>
        <Button
          variant="outline"
          size="sm"
          className="h-7 text-xs gap-1"
          title={t("orchestration.addStrategy", {
            defaultValue: "Add Strategy",
          })}
        >
          <Plus className="h-3 w-3" />
          {t("orchestration.addStrategy", { defaultValue: "Add Strategy" })}
        </Button>
      </CardHeader>
      <CardContent className="space-y-3">
        {strategies.map((strategy) => (
          <div
            key={strategy.id}
            className="rounded-lg border border-border/50 p-3 space-y-2"
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="font-medium text-sm">{strategy.name}</span>
                <Badge
                  variant="secondary"
                  className={`text-xs ${TYPE_COLORS[strategy.type] ?? ""}`}
                >
                  {strategy.type.toUpperCase()}
                </Badge>
              </div>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 text-muted-foreground hover:text-destructive"
                onClick={() => removeStrategy(strategy.id)}
                title={t("orchestration.removeStrategy", {
                  defaultValue: "Remove strategy",
                })}
              >
                <Trash2 className="h-3 w-3" />
              </Button>
            </div>

            <div className="grid grid-cols-2 gap-2 text-xs">
              <div>
                <Label className="text-muted-foreground">
                  {t("orchestration.complexity", {
                    defaultValue: "Complexity",
                  })}
                </Label>
                <div className="flex items-center gap-1 mt-0.5">
                  <Input
                    type="number"
                    value={strategy.complexityRange[0]}
                    className="h-6 text-xs"
                    step={0.1}
                    min={0}
                    max={1}
                    readOnly
                  />
                  <span className="text-muted-foreground">-</span>
                  <Input
                    type="number"
                    value={strategy.complexityRange[1]}
                    className="h-6 text-xs"
                    step={0.1}
                    min={0}
                    max={1}
                    readOnly
                  />
                </div>
              </div>
              <div>
                <Label className="text-muted-foreground">
                  {t("orchestration.risk", { defaultValue: "Risk" })}
                </Label>
                <div className="flex gap-1 mt-0.5">
                  {strategy.riskLevels.map((r) => (
                    <Badge key={r} variant="outline" className="text-xs">
                      {r}
                    </Badge>
                  ))}
                </div>
              </div>
            </div>

            <div>
              <Label className="text-muted-foreground text-xs">
                {t("orchestration.models", { defaultValue: "Models" })}
              </Label>
              <div className="flex flex-wrap gap-1 mt-0.5">
                {strategy.models.map((m) => (
                  <Badge key={m} variant="outline" className="text-xs">
                    {m}
                  </Badge>
                ))}
              </div>
            </div>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

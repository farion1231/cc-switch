import { useState, useEffect, useCallback } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Settings2, Trash2, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  type OrchestrationConfig,
  type StrategyRule,
  getConfig,
  saveConfig,
  getConfigPath,
  configToStrategyRules,
} from "@/lib/api/orchestration";

const TYPE_COLORS: Record<string, string> = {
  route: "bg-blue-500/15 text-blue-600",
  cascade: "bg-amber-500/15 text-amber-600",
  debate: "bg-purple-500/15 text-purple-600",
  moa: "bg-rose-500/15 text-rose-600",
};

export function StrategyEditor() {
  const { t } = useTranslation();
  const [strategies, setStrategies] = useState<StrategyRule[]>([]);
  const [savedConfig, setSavedConfig] = useState<OrchestrationConfig | null>(null);
  const [configPath, setConfigPath] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadConfig = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const config = await getConfig();
      setSavedConfig(config);
      const rules = configToStrategyRules(config);
      setStrategies(rules);
      try {
        const path = await getConfigPath();
        setConfigPath(path);
      } catch {
        setConfigPath("configs/strategies.yaml");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  const handleSave = useCallback(async () => {
    try {
      setSaving(true);
      setError(null);
      // Rebuild config from current strategy rules, preserving existing models
      const strategiesMap: OrchestrationConfig["strategies"] = {};
      for (const s of strategies) {
        const action = buildAction(s);
        if (!action) continue;
        strategiesMap[s.name] = {
          description: s.description ?? s.name,
          when: {
            complexity: s.complexityRange,
            risk: s.riskLevels,
          },
          action: action as OrchestrationConfig["strategies"][string]["action"],
        };
      }
      const config: OrchestrationConfig = {
        enabled: true,
        models: savedConfig?.models ?? {},
        strategies: strategiesMap,
      };
      await saveConfig(config);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }, [strategies, savedConfig]);

  const removeStrategy = (id: string) => {
    setStrategies((prev) => prev.filter((s) => s.name !== id));
  };

  if (loading) {
    return (
      <Card className="border-border/50">
        <CardContent className="p-4 text-sm text-muted-foreground">
          {t("orchestration.loading", { defaultValue: "Loading strategies..." })}
        </CardContent>
      </Card>
    );
  }

  return (
    <Card className="border-border/50">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="flex items-center gap-2">
          <Settings2 className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-medium">
            {t("orchestration.strategyEditor", { defaultValue: "Strategy Editor" })}
          </CardTitle>
          {configPath && (
            <span className="text-xs text-muted-foreground">({configPath})</span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-xs gap-1"
            onClick={loadConfig}
            title={t("orchestration.reload", { defaultValue: "Reload" })}
          >
            <RefreshCw className="h-3 w-3" />
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="h-7 text-xs gap-1"
            disabled={saving}
            onClick={handleSave}
            title={t("orchestration.save", { defaultValue: "Save to YAML" })}
          >
            {saving
              ? t("orchestration.saving", { defaultValue: "Saving..." })
              : t("orchestration.save", { defaultValue: "Save" })}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {error && (
          <div className="text-xs text-destructive bg-destructive/10 rounded p-2">
            {error}
          </div>
        )}
        {strategies.length === 0 && (
          <div className="text-xs text-muted-foreground">
            {t("orchestration.noStrategies", {
              defaultValue: "No strategies defined. Edit configs/strategies.yaml to add some.",
            })}
          </div>
        )}
        {strategies.map((strategy) => (
          <div
            key={strategy.name}
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
                onClick={() => removeStrategy(strategy.name)}
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
                  {t("orchestration.complexity", { defaultValue: "Complexity" })}
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
                  {t("orchestration.description", { defaultValue: "Description" })}
                </Label>
                <div className="text-xs mt-0.5">{strategy.description}</div>
              </div>
            </div>

            {strategy.judge && (
              <div className="text-xs text-muted-foreground">
                Judge: <Badge variant="outline" className="text-xs">{strategy.judge}</Badge>
              </div>
            )}
            {strategy.aggregator && (
              <div className="text-xs text-muted-foreground">
                Aggregator: <Badge variant="outline" className="text-xs">{strategy.aggregator}</Badge>
              </div>
            )}

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

function buildAction(
  s: StrategyRule,
): Record<string, unknown> | null {
  switch (s.type) {
    case "route":
      return {
        type: "route",
        use_model: s.models[0] ?? "unknown",
        verify: false,
      };
    case "cascade":
      return {
        type: "cascade",
        models: s.models,
        verify_each: true,
        escalate_on_fail: true,
        quality_threshold: s.qualityThreshold,
      };
    case "debate":
      return {
        type: "debate",
        debaters: s.models,
        judge: s.judge ?? s.models[0] ?? "unknown",
        quality_threshold: s.qualityThreshold,
      };
    case "moa":
      return {
        type: "moa",
        proposers: s.models,
        aggregator: s.aggregator ?? s.models[0] ?? "unknown",
        verify_each: true,
        quality_threshold: s.qualityThreshold,
      };
    default:
      return null;
  }
}

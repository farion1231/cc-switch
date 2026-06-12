import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { GitBranch, ArrowRight } from "lucide-react";
import { useTranslation } from "react-i18next";

interface WorkflowStep {
  id: string;
  label: string;
  type: "route" | "cascade" | "debate" | "moa" | "classify" | "verify";
  status: "pending" | "running" | "done" | "failed";
  details?: string;
}

interface WorkflowVisualizerProps {
  steps?: WorkflowStep[];
  activeStrategy?: string;
  activeModels?: string[];
}

const STEP_COLORS: Record<string, string> = {
  route: "border-blue-400 bg-blue-50 dark:bg-blue-950",
  cascade: "border-amber-400 bg-amber-50 dark:bg-amber-950",
  debate: "border-purple-400 bg-purple-50 dark:bg-purple-950",
  moa: "border-rose-400 bg-rose-50 dark:bg-rose-950",
  classify: "border-slate-400 bg-slate-50 dark:bg-slate-950",
  verify: "border-emerald-400 bg-emerald-50 dark:bg-emerald-950",
};

const STATUS_COLORS: Record<string, string> = {
  pending: "bg-slate-200 text-slate-600",
  running: "bg-blue-200 text-blue-700 animate-pulse",
  done: "bg-emerald-200 text-emerald-700",
  failed: "bg-red-200 text-red-700",
};

const DEFAULT_STEPS: WorkflowStep[] = [
  { id: "1", label: "Classify", type: "classify", status: "pending", details: "Complexity & Risk" },
  { id: "2", label: "Select", type: "route", status: "pending", details: "Strategy Selection" },
  { id: "3", label: "Execute", type: "cascade", status: "pending", details: "Model Invocation" },
  { id: "4", label: "Verify", type: "verify", status: "pending", details: "Quality Gate" },
];

export function WorkflowVisualizer({
  steps = DEFAULT_STEPS,
  activeStrategy,
  activeModels,
}: WorkflowVisualizerProps) {
  const { t } = useTranslation();

  return (
    <Card className="border-border/50">
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <GitBranch className="h-4 w-4 text-muted-foreground" />
            <CardTitle className="text-sm font-medium">
              {t("orchestration.workflow", { defaultValue: "Execution Pipeline" })}
            </CardTitle>
          </div>
          {activeStrategy && (
            <Badge variant="outline" className="text-xs">
              {activeStrategy}
            </Badge>
          )}
        </div>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-1 overflow-x-auto py-2">
          {steps.map((step, i) => (
            <div key={step.id} className="flex items-center gap-1 flex-shrink-0">
              <div
                className={`rounded-lg border-2 px-3 py-2 min-w-[80px] text-center ${STEP_COLORS[step.type] ?? "border-slate-300"}`}
              >
                <div className="text-xs font-semibold">{step.label}</div>
                {step.details && (
                  <div className="text-[10px] text-muted-foreground mt-0.5">
                    {step.details}
                  </div>
                )}
                <Badge
                  variant="secondary"
                  className={`mt-1 text-[10px] px-1 py-0 ${STATUS_COLORS[step.status] ?? ""}`}
                >
                  {step.status}
                </Badge>
              </div>
              {i < steps.length - 1 && (
                <ArrowRight className="h-4 w-4 text-muted-foreground flex-shrink-0" />
              )}
            </div>
          ))}
        </div>
        {activeModels && activeModels.length > 0 && (
          <div className="mt-3 flex flex-wrap gap-1 border-t border-border/50 pt-2">
            <span className="text-[10px] text-muted-foreground mr-1">
              {t("orchestration.models", { defaultValue: "Models:" })}
            </span>
            {activeModels.map((m) => (
              <Badge key={m} variant="outline" className="text-[10px] px-1 py-0">
                {m}
              </Badge>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

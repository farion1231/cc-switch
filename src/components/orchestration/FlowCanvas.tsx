import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { GitBranch } from "lucide-react";
import { useTranslation } from "react-i18next";

interface FlowStep {
  id: string;
  label: string;
  type:
    | "input"
    | "classify"
    | "route"
    | "cascade"
    | "debate"
    | "moa"
    | "output";
}

const DEMO_FLOW: FlowStep[] = [
  { id: "1", label: "Request", type: "input" },
  { id: "2", label: "Classify", type: "classify" },
  { id: "3", label: "Cascade", type: "cascade" },
  { id: "4", label: "Quality Gate", type: "route" },
  { id: "5", label: "Escalate", type: "debate" },
  { id: "6", label: "Response", type: "output" },
];

const STEP_COLORS: Record<string, string> = {
  input: "bg-slate-500/15 text-slate-600 border-slate-300",
  classify: "bg-blue-500/15 text-blue-600 border-blue-300",
  route: "bg-emerald-500/15 text-emerald-600 border-emerald-300",
  cascade: "bg-amber-500/15 text-amber-600 border-amber-300",
  debate: "bg-purple-500/15 text-purple-600 border-purple-300",
  moa: "bg-rose-500/15 text-rose-600 border-rose-300",
  output: "bg-slate-500/15 text-slate-600 border-slate-300",
};

export function FlowCanvas() {
  const { t } = useTranslation();

  return (
    <Card className="border-border/50">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="flex items-center gap-2">
          <GitBranch className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-medium">
            {t("orchestration.flowCanvas", {
              defaultValue: "Execution Flow",
            })}
          </CardTitle>
        </div>
        <Badge variant="outline" className="text-xs">
          {t("orchestration.lastRun", { defaultValue: "Last run: demo" })}
        </Badge>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-2 overflow-x-auto pb-2">
          {DEMO_FLOW.map((step, idx) => (
            <div key={step.id} className="flex items-center gap-2">
              <div
                className={`flex-shrink-0 rounded-lg border px-3 py-2 text-center min-w-[80px] ${STEP_COLORS[step.type] ?? "bg-muted"}`}
              >
                <span className="text-xs font-medium">{step.label}</span>
              </div>
              {idx < DEMO_FLOW.length - 1 && (
                <div className="flex-shrink-0 text-muted-foreground">
                  <svg
                    width="20"
                    height="12"
                    viewBox="0 0 20 12"
                    className="text-muted-foreground/50"
                    aria-hidden="true"
                  >
                    <path
                      d="M0 6 L14 6 M10 2 L14 6 L10 10"
                      stroke="currentColor"
                      strokeWidth="1.5"
                      fill="none"
                    />
                  </svg>
                </div>
              )}
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Shield, CheckCircle2, XCircle, Clock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

interface GateRequest {
  id: string;
  reason: string;
  riskLevel: string;
  timestamp: string;
  status: "pending" | "approved" | "rejected" | "expired";
}

const MOCK_GATES: GateRequest[] = [
  {
    id: "gate_001",
    reason: "Critical risk task requires human approval",
    riskLevel: "critical",
    timestamp: "2026-06-01T10:30:00Z",
    status: "pending",
  },
  {
    id: "gate_002",
    reason: "Low consensus among debate judges",
    riskLevel: "high",
    timestamp: "2026-06-01T10:25:00Z",
    status: "approved",
  },
];

const STATUS_CONFIG: Record<
  string,
  { icon: typeof CheckCircle2; color: string }
> = {
  pending: { icon: Clock, color: "text-amber-500" },
  approved: { icon: CheckCircle2, color: "text-emerald-500" },
  rejected: { icon: XCircle, color: "text-red-500" },
  expired: { icon: XCircle, color: "text-muted-foreground" },
};

export function HumanGatePanel() {
  const { t } = useTranslation();

  return (
    <Card className="border-border/50">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="flex items-center gap-2">
          <Shield className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-medium">
            {t("orchestration.humanGate", {
              defaultValue: "Human Gate",
            })}
          </CardTitle>
        </div>
        <Badge variant="outline" className="text-xs">
          {t("orchestration.pending", {
            defaultValue: `${MOCK_GATES.filter((g) => g.status === "pending").length} pending`,
            count: MOCK_GATES.filter((g) => g.status === "pending").length,
          })}
        </Badge>
      </CardHeader>
      <CardContent className="space-y-3">
        {MOCK_GATES.map((gate) => {
          const config = STATUS_CONFIG[gate.status];
          const Icon = config.icon;
          return (
            <div
              key={gate.id}
              className="rounded-lg border border-border/50 p-3 space-y-2"
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Icon className={cn("h-4 w-4", config.color)} />
                  <span className="text-sm font-medium">{gate.id}</span>
                </div>
                <Badge
                  variant="outline"
                  className={cn(
                    "text-xs",
                    gate.riskLevel === "critical" &&
                      "border-red-300 text-red-600",
                    gate.riskLevel === "high" &&
                      "border-amber-300 text-amber-600",
                  )}
                >
                  {gate.riskLevel}
                </Badge>
              </div>
              <p className="text-xs text-muted-foreground">{gate.reason}</p>
              <div className="flex items-center justify-between">
                <span className="text-[10px] text-muted-foreground">
                  {new Date(gate.timestamp).toLocaleTimeString()}
                </span>
                {gate.status === "pending" && (
                  <div className="flex gap-1">
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-6 text-xs gap-1 text-emerald-600 hover:text-emerald-700"
                    >
                      <CheckCircle2 className="h-3 w-3" />
                      {t("orchestration.approve", {
                        defaultValue: "Approve",
                      })}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-6 text-xs gap-1 text-red-600 hover:text-red-700"
                    >
                      <XCircle className="h-3 w-3" />
                      {t("orchestration.reject", { defaultValue: "Reject" })}
                    </Button>
                  </div>
                )}
                {gate.status !== "pending" && (
                  <Badge
                    variant="secondary"
                    className={cn("text-xs", config.color)}
                  >
                    {gate.status}
                  </Badge>
                )}
              </div>
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}

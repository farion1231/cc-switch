import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { FileText, AlertCircle, CheckCircle2, Clock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

interface AuditEntry {
  id: string;
  timestamp: string;
  eventType: string;
  requestId: string;
  details: Record<string, string>;
}

const MOCK_ENTRIES: AuditEntry[] = [
  {
    id: "1",
    timestamp: "2026-06-01T10:30:00Z",
    eventType: "request_received",
    requestId: "req_abc123",
    details: { strategy: "cascade", model: "deepseek-chat" },
  },
  {
    id: "2",
    timestamp: "2026-06-01T10:30:01Z",
    eventType: "strategy_selected",
    requestId: "req_abc123",
    details: {
      strategy: "cascade",
      complexity: "0.55",
      risk: "medium",
    },
  },
  {
    id: "3",
    timestamp: "2026-06-01T10:30:03Z",
    eventType: "model_call",
    requestId: "req_abc123",
    details: { model: "deepseek-chat", latency: "2300ms" },
  },
  {
    id: "4",
    timestamp: "2026-06-01T10:30:04Z",
    eventType: "quality_check",
    requestId: "req_abc123",
    details: { score: "0.72", passed: "true" },
  },
  {
    id: "5",
    timestamp: "2026-06-01T10:30:05Z",
    eventType: "response_sent",
    requestId: "req_abc123",
    details: { model: "deepseek-chat", tokens: "1247" },
  },
];

const EVENT_ICONS: Record<string, typeof CheckCircle2> = {
  request_received: Clock,
  strategy_selected: FileText,
  model_call: FileText,
  quality_check: CheckCircle2,
  response_sent: CheckCircle2,
  error: AlertCircle,
};

export function AuditLog() {
  const { t } = useTranslation();

  return (
    <Card className="border-border/50">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="flex items-center gap-2">
          <FileText className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-medium">
            {t("orchestration.auditLog", { defaultValue: "Audit Log" })}
          </CardTitle>
        </div>
        <Badge variant="outline" className="text-xs">
          {t("orchestration.events", {
            defaultValue: `${MOCK_ENTRIES.length} events`,
            count: MOCK_ENTRIES.length,
          })}
        </Badge>
      </CardHeader>
      <CardContent>
        <ScrollArea className="h-[200px]">
          <div className="space-y-2">
            {MOCK_ENTRIES.map((entry) => {
              const Icon = EVENT_ICONS[entry.eventType] ?? FileText;
              const isError = entry.eventType === "error";
              return (
                <div
                  key={entry.id}
                  className={cn(
                    "flex items-start gap-2 rounded-md border border-border/30 p-2 text-xs",
                    isError && "border-red-200 bg-red-50/50",
                  )}
                >
                  <Icon
                    className={cn(
                      "h-3.5 w-3.5 mt-0.5 flex-shrink-0",
                      isError ? "text-red-500" : "text-muted-foreground",
                    )}
                  />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center justify-between">
                      <span className="font-medium">
                        {entry.eventType.replace(/_/g, " ")}
                      </span>
                      <span className="text-muted-foreground">
                        {new Date(entry.timestamp).toLocaleTimeString()}
                      </span>
                    </div>
                    <div className="flex flex-wrap gap-1 mt-1">
                      {Object.entries(entry.details).map(([k, v]) => (
                        <Badge
                          key={k}
                          variant="secondary"
                          className="text-[10px] px-1 py-0"
                        >
                          {k}: {v}
                        </Badge>
                      ))}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </ScrollArea>
      </CardContent>
    </Card>
  );
}

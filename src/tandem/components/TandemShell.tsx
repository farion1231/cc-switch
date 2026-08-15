import { useState, type ComponentType } from "react";
import { ListTodo, Settings } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { TaskLedgerPage } from "./TaskLedgerPage";

type Route = "tasks" | "config";

export function TandemShell({
  LegacyConfigApp,
}: {
  LegacyConfigApp: ComponentType;
}) {
  const [route, setRoute] = useState<Route>("tasks");

  return (
    <div className="flex h-screen min-h-[320px] flex-col overflow-hidden bg-background text-foreground">
      <header className="flex h-[52px] shrink-0 items-center border-b border-border px-2 sm:px-4">
        <span className="mr-2 hidden text-sm font-semibold sm:inline">
          Tandem
        </span>
        <nav aria-label="主导航" className="flex h-full items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            className={cn(
              "h-9 rounded-md px-3",
              route === "tasks" && "bg-accent text-accent-foreground",
            )}
            aria-current={route === "tasks" ? "page" : undefined}
            onClick={() => setRoute("tasks")}
          >
            <ListTodo className="h-4 w-4" aria-hidden="true" />
            任务
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className={cn(
              "h-9 rounded-md px-3",
              route === "config" && "bg-accent text-accent-foreground",
            )}
            aria-current={route === "config" ? "page" : undefined}
            onClick={() => setRoute("config")}
          >
            <Settings className="h-4 w-4" aria-hidden="true" />
            Agent 配置
          </Button>
        </nav>
      </header>
      <main className="min-h-0 flex-1 overflow-hidden">
        <div className={cn("h-full", route !== "tasks" && "hidden")}>
          <TaskLedgerPage />
        </div>
        {route === "config" ? (
          <div className="h-full overflow-auto">
            <LegacyConfigApp />
          </div>
        ) : null}
      </main>
    </div>
  );
}
